use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_trait::async_trait;
use command_group::AsyncCommandGroup;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, process::Command};
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, CommandParts, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, ExecutorError, SpawnedChild, StandardCodingAgentExecutor,
    },
    logs::stdout_processor::normalize_stdout_logs,
    logs::utils::EntryIndexProvider,
};

fn base_command(native_binary: bool) -> &'static str {
    if native_binary {
        "reasonix"
    } else {
        "npx -y reasonix@latest"
    }
}

/// Auto-detect whether the native `reasonix` binary is available in PATH.
fn detect_reasonix_binary() -> bool {
    static DETECTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DETECTED.get_or_init(|| {
        if let Ok(val) = std::env::var("VIBE_KANBAN_DISABLE_NATIVE_REASONIX_DETECTION") {
            if val == "1" || val.to_lowercase() == "true" {
                return false;
            }
        }
        which::which("reasonix").is_ok()
    })
}

use derivative::Derivative;

#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct Reasonix {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Model",
        description = "Model to use: \"deepseek-v4-flash\" (default), \"deepseek-v4-pro\", or \"mimo-pro\""
    )]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Dangerously Skip Permissions (YOLO)",
        description = "Skip tool-approval prompts (--yolo flag)"
    )]
    pub dangerously_skip_permissions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Use Code Mode",
        description = "Use \"reasonix code\" (with filesystem tools) instead of \"reasonix run\" (chat-only one-shot)"
    )]
    pub use_code_mode: Option<bool>,
    #[serde(flatten)]
    pub cmd: CmdOverrides,
    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    approvals_service: Option<Arc<dyn ExecutorApprovalService>>,
}

impl Reasonix {
    fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let native = detect_reasonix_binary();
        let base = base_command(native);
        let subcommand = if self.use_code_mode.unwrap_or(false) {
            "code"
        } else {
            "run"
        };
        // Wrap with `script` to provide a PTY — reasonix uses Bubble Tea (Go TUI)
        // which opens /dev/tty directly. `script -q` creates a PTY and suppresses
        // start/done banners.
        #[cfg(not(windows))]
        let cmd = format!("script -q /dev/null {base} {subcommand}");
        #[cfg(windows)]
        let cmd = format!("{base} {subcommand}");
        let mut builder = CommandBuilder::new(cmd);

        if let Some(model) = &self.model {
            builder = builder.extend_params(["--model", model.as_str()]);
        }

        if self.dangerously_skip_permissions.unwrap_or(false) && self.use_code_mode.unwrap_or(false) {
            builder = builder.extend_params(["--yolo"]);
        }

        apply_overrides(builder, &self.cmd)
    }
}

async fn spawn_reasonix(
    command_parts: CommandParts,
    prompt: Option<&str>,
    current_dir: &Path,
    env: &ExecutionEnv,
    cmd_overrides: &CmdOverrides,
) -> Result<SpawnedChild, ExecutorError> {
    let (program_path, args) = command_parts.into_resolved().await?;

    let mut command = Command::new(program_path);
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(current_dir)
        .args(args);

    env.clone()
        .with_profile(cmd_overrides)
        .apply_to_command(&mut command);

    let mut child = command.group_spawn()?;

    if let Some(prompt_text) = prompt {
        if let Some(mut stdin) = child.inner().stdin.take() {
            stdin.write_all(prompt_text.as_bytes()).await?;
            stdin.shutdown().await?;
        }
    }

    Ok(child.into())
}

#[async_trait]
impl StandardCodingAgentExecutor for Reasonix {
    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let combined_prompt = self.append_prompt.combine_prompt(prompt);

        let command = self
            .build_command_builder()?
            .extend_params([combined_prompt])
            .build_initial()?;
        spawn_reasonix(command, None, current_dir, env, &self.cmd).await
    }

    async fn spawn_follow_up(
        &self,
        _current_dir: &Path,
        _prompt: &str,
        _session_id: &str,
        _reset_to_message_id: Option<&str>,
        _env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        Err(ExecutorError::FollowUpNotSupported(
            "Reasonix does not support session resume via CLI".into(),
        ))
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, _worktree_path: &Path) {
        let entry_index_provider = EntryIndexProvider::start_from(&msg_store);
        normalize_stdout_logs(msg_store, entry_index_provider);
    }

    fn default_mcp_config_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| {
            let toml_path = home.join(".reasonix").join("config.toml");
            if toml_path.exists() {
                toml_path
            } else {
                home.join(".reasonix").join("config.json")
            }
        })
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        let reasonix_dir = dirs::home_dir().map(|home| home.join(".reasonix"));

        if let Some(ref dir) = reasonix_dir {
            if dir.exists() {
                return AvailabilityInfo::InstallationFound;
            }
        }

        let config_found = self
            .default_mcp_config_path()
            .map(|p| p.exists())
            .unwrap_or(false);

        if config_found {
            AvailabilityInfo::InstallationFound
        } else {
            AvailabilityInfo::NotFound
        }
    }
}
