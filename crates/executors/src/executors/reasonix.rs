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

pub use super::acp::AcpAgentHarness;
use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, CommandParts, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, BaseCodingAgent, ExecutorError, SpawnedChild,
        StandardCodingAgentExecutor, utils::SlashCommandCacheKey,
    },
    logs::{
        stdout_processor::normalize_stdout_logs,
        utils::{EntryIndexProvider, patch},
    },
};

/// Session namespace used by the ACP harness to persist reasonix sessions.
const REASONIX_SESSION_NAMESPACE: &str = "reasonix_sessions";

fn base_command(native_binary: bool) -> &'static str {
    if native_binary {
        "reasonix"
    } else {
        "npx -y reasonix@1.36.0"
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
    fn is_code_mode(&self) -> bool {
        self.use_code_mode.unwrap_or(false)
    }

    /// Build the one-shot `reasonix run` command, wrapping with
    /// `script -q /dev/null` so the agent gets a PTY for its file tools.
    fn build_run_command_builder(
        &self,
        wrap_script: bool,
    ) -> Result<CommandBuilder, CommandBuildError> {
        let native = detect_reasonix_binary();
        let base = base_command(native);
        #[cfg(not(windows))]
        let cmd = if wrap_script {
            format!("script -q /dev/null {base} run")
        } else {
            format!("{base} run")
        };
        #[cfg(windows)]
        let cmd = format!("{base} run");
        let mut builder = CommandBuilder::new(cmd);

        if let Some(model) = &self.model {
            builder = builder.extend_params(["--model", model.as_str()]);
        }

        apply_overrides(builder, &self.cmd)
    }

    /// Build the `reasonix acp` command used for the interactive (code) mode.
    ///
    /// reasonix speaks the Agent Client Protocol over stdio, which lets the
    /// shared ACP harness drive the session, detect turn completion (so the
    /// process is reaped instead of hanging in a perpetual "loading" state),
    /// stream structured logs and support follow-ups / session resume.
    fn build_acp_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let native = detect_reasonix_binary();
        let base = base_command(native);
        let builder = CommandBuilder::new(format!("{base} acp"));
        apply_overrides(builder, &self.cmd)
    }

    /// Approvals to hand to the ACP harness. When the user opts into YOLO we
    /// pass `None`, which makes the ACP client auto-approve every tool call.
    fn acp_approvals(&self) -> Option<Arc<dyn ExecutorApprovalService>> {
        if self.dangerously_skip_permissions.unwrap_or(false) {
            None
        } else {
            self.approvals_service.clone()
        }
    }

    fn acp_harness(&self) -> AcpAgentHarness {
        let mut harness = AcpAgentHarness::with_session_namespace(REASONIX_SESSION_NAMESPACE);
        if let Some(model) = &self.model {
            harness = harness.with_model(model.clone());
        }
        harness
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
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            // Don't shutdown — keep stdin open so reasonix can continue
            // reading from the PTY during its session
        }
    }

    Ok(child.into())
}

#[async_trait]
impl StandardCodingAgentExecutor for Reasonix {
    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals_service = Some(approvals);
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let combined_prompt = self.append_prompt.combine_prompt(prompt);

        if self.is_code_mode() {
            // code mode: drive reasonix through ACP over stdio. The harness
            // detects turn completion and signals the container, so the process
            // is reaped instead of hanging forever in a "loading" state.
            let command = self.build_acp_command_builder()?.build_initial()?;
            self.acp_harness()
                .spawn_with_command(
                    current_dir,
                    combined_prompt,
                    command,
                    env,
                    &self.cmd,
                    self.acp_approvals(),
                )
                .await
        } else {
            // run mode: one-shot `reasonix run <prompt>` that exits on completion
            let command = self
                .build_run_command_builder(true)?
                .extend_params([combined_prompt])
                .build_initial()?;
            spawn_reasonix(command, None, current_dir, env, &self.cmd).await
        }
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        if !self.is_code_mode() {
            return Err(ExecutorError::FollowUpNotSupported(
                "Reasonix run mode does not support session resume; enable code mode".into(),
            ));
        }

        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        let command = self.build_acp_command_builder()?.build_follow_up(&[])?;
        self.acp_harness()
            .spawn_follow_up_with_command(
                current_dir,
                combined_prompt,
                session_id,
                reset_to_message_id,
                command,
                env,
                &self.cmd,
                self.acp_approvals(),
            )
            .await
    }

    async fn available_slash_commands(
        &self,
        workdir: &Path,
    ) -> Result<futures::stream::BoxStream<'static, json_patch::Patch>, ExecutorError> {
        if !self.is_code_mode() {
            return Ok(Box::pin(futures::stream::once(async move {
                patch::slash_commands(Vec::new(), false, None)
            })));
        }

        let this = self.clone();
        let workdir = workdir.to_path_buf();
        Ok(Box::pin(futures::stream::once(async move {
            let commands = match this
                .build_acp_command_builder()
                .and_then(|b| b.build_initial())
            {
                Ok(parts) => {
                    let key = SlashCommandCacheKey::new(&workdir, &BaseCodingAgent::Reasonix);
                    super::acp::discover_acp_slash_commands(parts, &workdir, &this.cmd, &key).await
                }
                Err(e) => {
                    tracing::warn!("Failed to build Reasonix command for slash command probe: {e}");
                    Vec::new()
                }
            };
            patch::slash_commands(commands, false, None)
        })))
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, worktree_path: &Path) {
        if self.is_code_mode() {
            super::acp::normalize_logs(msg_store, worktree_path);
        } else {
            let entry_index_provider = EntryIndexProvider::start_from(&msg_store);
            normalize_stdout_logs(msg_store, entry_index_provider);
        }
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
