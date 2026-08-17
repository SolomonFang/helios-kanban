use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_trait::async_trait;
use command_group::AsyncCommandGroup;
use derivative::Derivative;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

use crate::{
    command::{CmdOverrides, CommandBuildError, CommandBuilder, CommandParts, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, ExecutorError, SpawnedChild, StandardCodingAgentExecutor,
    },
    logs::{stdout_processor::normalize_stdout_logs, utils::EntryIndexProvider},
};

fn base_command(native_binary: bool) -> &'static str {
    if native_binary {
        "dsh"
    } else {
        "npx -y @deepseek-ai/dsh"
    }
}

/// Auto-detect whether the native `dsh` binary is available in PATH.
fn detect_dsh_binary() -> bool {
    static DETECTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DETECTED.get_or_init(|| which::which("dsh").is_ok())
}

/// DeepSeek Harness (`dsh`) is driven through its official one-shot headless
/// profile: `dsh --profile headless "<prompt>"`. Upstream is still in
/// developer preview — the headless profile only streams plain text (no
/// structured tool events) and has no session-resume surface, so follow-ups
/// are not supported yet.
#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct DeepseekHarness {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Model",
        description = "Model to use, passed via the DSH_MODEL environment variable: \"deepseek-v4-flash\" (default), \"deepseek-v4-pro\", or any custom model id"
    )]
    pub model: Option<String>,
    #[serde(flatten)]
    pub cmd: CmdOverrides,
}

impl DeepseekHarness {
    fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let native = detect_dsh_binary();
        let builder = CommandBuilder::new(base_command(native));
        let builder = builder.extend_params(["--profile", "headless"]);
        apply_overrides(builder, &self.cmd)
    }
}

async fn spawn_dsh(
    command_parts: CommandParts,
    current_dir: &Path,
    env: &ExecutionEnv,
    model: Option<&str>,
    cmd_overrides: &CmdOverrides,
) -> Result<SpawnedChild, ExecutorError> {
    let (program_path, args) = command_parts.into_resolved().await?;

    let mut command = Command::new(program_path);
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(current_dir)
        .args(args);

    let mut env = env.clone();
    if let Some(model) = model {
        env.insert("DSH_MODEL", model);
    }
    env.with_profile(cmd_overrides)
        .apply_to_command(&mut command);

    let child = command.group_spawn()?;
    Ok(child.into())
}

#[async_trait]
impl StandardCodingAgentExecutor for DeepseekHarness {
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
        spawn_dsh(
            command,
            current_dir,
            env,
            self.model.as_deref(),
            &self.cmd,
        )
        .await
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
            "DeepSeek Harness headless mode does not support session resume yet".into(),
        ))
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, _worktree_path: &Path) {
        let entry_index_provider = EntryIndexProvider::start_from(&msg_store);
        normalize_stdout_logs(msg_store, entry_index_provider);
    }

    fn default_mcp_config_path(&self) -> Option<PathBuf> {
        // dsh has no documented MCP config file we can write to yet.
        None
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        let dsh_dir_found = dirs::home_dir()
            .map(|home| home.join(".dsh").exists())
            .unwrap_or(false);

        if dsh_dir_found || detect_dsh_binary() {
            AvailabilityInfo::InstallationFound
        } else {
            AvailabilityInfo::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_round_trip() {
        let json = r#"{"append_prompt":null,"model":"deepseek-v4-pro"}"#;
        let agent: DeepseekHarness = serde_json::from_str(json).unwrap();
        assert_eq!(agent.model.as_deref(), Some("deepseek-v4-pro"));
        let serialized = serde_json::to_string(&agent).unwrap();
        let deserialized: DeepseekHarness = serde_json::from_str(&serialized).unwrap();
        assert_eq!(agent, deserialized);
    }

    #[test]
    fn test_deserialize_empty_config() {
        let agent: DeepseekHarness = serde_json::from_str("{}").unwrap();
        assert!(agent.model.is_none());
    }
}
