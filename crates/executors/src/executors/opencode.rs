use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use derivative::Derivative;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

mod config_options;

pub use config_options::{OpencodeConfigOption, OpencodeConfigOptions, OpencodeModelOption};

use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, BaseCodingAgent, ExecutorError, SpawnedChild,
        StandardCodingAgentExecutor, acp::AcpAgentHarness, utils::SlashCommandCacheKey,
    },
    logs::utils::patch,
};

#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct Opencode {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "mode")]
    pub agent: Option<String>,
    /// Auto-approve agent actions
    #[serde(default = "default_to_true")]
    pub auto_approve: bool,
    /// Enable auto-compaction when the context length approaches the model's context window limit
    #[serde(default = "default_to_true")]
    pub auto_compact: bool,
    #[serde(flatten)]
    pub cmd: CmdOverrides,
    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    pub approvals: Option<Arc<dyn ExecutorApprovalService>>,
}

impl Opencode {
    fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let builder = CommandBuilder::new("npx -y @opencode/cli@2.1.4").extend_params(["acp"]);
        apply_overrides(builder, &self.cmd)
    }

    fn acp_model(&self) -> Option<String> {
        self.model.as_ref().map(|model| {
            match self
                .variant
                .as_deref()
                .filter(|variant| !variant.is_empty())
            {
                Some(variant) => format!("{model}/{variant}"),
                None => model.clone(),
            }
        })
    }

    fn acp_harness(&self) -> AcpAgentHarness {
        let mut harness = AcpAgentHarness::with_session_namespace("opencode_sessions")
            .with_native_session_resume(true)
            .with_tool_auto_approve(self.auto_approve)
            .with_model_via_config_option(true);
        if let Some(model) = self.acp_model() {
            harness = harness.with_model(model);
        }
        if let Some(agent) = &self.agent {
            harness = harness.with_mode(agent.clone());
        }
        harness
    }

    fn setup_config_env(&self, env: &ExecutionEnv) -> ExecutionEnv {
        let mut env = env.clone();
        let merged = merge_config(
            env.get("OPENCODE_CONFIG_CONTENT").map(String::as_str),
            self.auto_compact,
        );
        env.insert("OPENCODE_CONFIG_CONTENT", merged);
        env
    }
}

#[async_trait]
impl StandardCodingAgentExecutor for Opencode {
    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals = Some(approvals);
    }

    async fn available_slash_commands(
        &self,
        workdir: &Path,
    ) -> Result<futures::stream::BoxStream<'static, json_patch::Patch>, ExecutorError> {
        let this = self.clone();
        let workdir = workdir.to_path_buf();
        Ok(Box::pin(futures::stream::once(async move {
            let commands = match this.build_command_builder().and_then(|b| b.build_initial()) {
                Ok(parts) => {
                    let key = SlashCommandCacheKey::new(&workdir, &BaseCodingAgent::Opencode);
                    super::acp::discover_acp_slash_commands(parts, &workdir, &this.cmd, &key).await
                }
                Err(e) => {
                    tracing::warn!("Failed to build OpenCode command for slash command probe: {e}");
                    Vec::new()
                }
            };
            patch::slash_commands(commands, false, None)
        })))
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let env = setup_permissions_env(self.auto_approve, env);
        let env = self.setup_config_env(&env);
        let command = self.build_command_builder()?.build_initial()?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        self.acp_harness()
            .spawn_with_command(
                current_dir,
                combined_prompt,
                command,
                &env,
                &self.cmd,
                self.approvals.clone(),
            )
            .await
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let env = setup_permissions_env(self.auto_approve, env);
        let env = self.setup_config_env(&env);
        let command = self.build_command_builder()?.build_follow_up(&[])?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        self.acp_harness()
            .spawn_follow_up_with_command(
                current_dir,
                combined_prompt,
                session_id,
                reset_to_message_id,
                command,
                &env,
                &self.cmd,
                self.approvals.clone(),
            )
            .await
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, worktree_path: &Path) {
        super::acp::normalize_logs(msg_store, worktree_path);
    }

    fn default_mcp_config_path(&self) -> Option<std::path::PathBuf> {
        #[cfg(not(windows))]
        {
            let base_dirs = xdg::BaseDirectories::with_prefix("opencode");
            base_dirs
                .get_config_file("opencode.json")
                .filter(|p| p.exists())
                .or_else(|| base_dirs.get_config_file("opencode.jsonc"))
        }
        #[cfg(windows)]
        {
            let config_dir = std::env::var("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .ok()
                .or_else(|| dirs::home_dir().map(|p| p.join(".config")))
                .map(|p| p.join("opencode"))?;

            let path = Some(config_dir.join("opencode.json"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| config_dir.join("opencode.jsonc"));
            Some(path)
        }
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        let mcp_config_found = self
            .default_mcp_config_path()
            .map(|p| p.exists())
            .unwrap_or(false);

        #[cfg(not(windows))]
        let installation_indicator_found = {
            let base_dirs = xdg::BaseDirectories::with_prefix("opencode");

            let config_dir_exists = base_dirs
                .get_config_home()
                .map(|config| config.exists())
                .unwrap_or(false);

            let data_dir_exists = base_dirs
                .get_data_home()
                .map(|data| data.exists())
                .unwrap_or(false);

            let state_dir_exists = base_dirs
                .get_state_home()
                .map(|state| state.exists())
                .unwrap_or(false);

            config_dir_exists || data_dir_exists || state_dir_exists
        };

        #[cfg(windows)]
        let installation_indicator_found = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .and_then(|p| p.join("opencode").exists().then_some(()))
            .or_else(|| {
                dirs::home_dir()
                    .and_then(|p| p.join(".config").join("opencode").exists().then_some(()))
            })
            .is_some();

        let home_opencode_exists = dirs::home_dir()
            .map(|home| home.join(".opencode").exists())
            .unwrap_or(false);

        if mcp_config_found || installation_indicator_found || home_opencode_exists {
            AvailabilityInfo::InstallationFound
        } else {
            AvailabilityInfo::NotFound
        }
    }
}

fn default_to_true() -> bool {
    true
}

fn setup_permissions_env(auto_approve: bool, env: &ExecutionEnv) -> ExecutionEnv {
    let mut env = env.clone();

    let permissions = match env.get("OPENCODE_PERMISSION") {
        Some(existing) => merge_question_deny(existing),
        None => build_default_permissions(auto_approve),
    };

    env.insert("OPENCODE_PERMISSION", &permissions);
    env
}

fn build_default_permissions(auto_approve: bool) -> String {
    if auto_approve {
        r#"{"question":"deny"}"#.to_string()
    } else {
        r#"{"edit":"ask","bash":"ask","webfetch":"ask","doom_loop":"ask","external_directory":"ask","question":"deny"}"#.to_string()
    }
}

fn merge_question_deny(existing_json: &str) -> String {
    let mut permissions: Map<String, serde_json::Value> = match serde_json::from_str(
        existing_json.trim(),
    ) {
        Ok(permissions) => permissions,
        Err(err) => {
            tracing::warn!(
                "Ignoring invalid OPENCODE_PERMISSION JSON ({err}); falling back to default permissions"
            );
            Map::new()
        }
    };

    permissions.insert(
        "question".to_string(),
        serde_json::Value::String("deny".to_string()),
    );

    serde_json::to_string(&permissions).unwrap_or_else(|_| r#"{"question":"deny"}"#.to_string())
}

fn merge_config(existing_json: Option<&str>, auto_compact: bool) -> String {
    let mut config: Map<String, Value> = existing_json
        .and_then(|value| serde_json::from_str(value.trim()).ok())
        .unwrap_or_default();

    let mut compaction = config
        .remove("compaction")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    compaction.insert("auto".to_string(), Value::Bool(auto_compact));
    config.insert("compaction".to_string(), Value::Object(compaction));

    serde_json::to_string(&config)
        .unwrap_or_else(|_| format!(r#"{{"compaction":{{"auto":{auto_compact}}}}}"#))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{Opencode, merge_config, merge_question_deny};

    #[test]
    fn default_command_uses_opencode_cli_acp() {
        let opencode: Opencode = serde_json::from_str("{}").unwrap();
        let builder = opencode.build_command_builder().unwrap();

        assert_eq!(builder.base, "npx -y @opencode/cli@2.1.4");
        assert_eq!(builder.params, Some(vec!["acp".to_string()]));
    }

    #[test]
    fn acp_model_includes_variant() {
        let opencode: Opencode =
            serde_json::from_str(r#"{"model":"anthropic/claude-sonnet-4-5","variant":"high"}"#)
                .unwrap();

        assert_eq!(
            opencode.acp_model().as_deref(),
            Some("anthropic/claude-sonnet-4-5/high")
        );
    }

    #[test]
    fn compaction_config_applies_enabled_and_disabled_values() {
        for (auto_compact, expected) in [(true, true), (false, false)] {
            let merged = merge_config(
                Some(r#"{"compaction":{"reserved":10000},"model":"test/model"}"#),
                auto_compact,
            );
            let parsed: Value = serde_json::from_str(&merged).unwrap();
            assert_eq!(
                parsed.pointer("/compaction/auto").and_then(Value::as_bool),
                Some(expected)
            );
            assert_eq!(
                parsed
                    .pointer("/compaction/reserved")
                    .and_then(Value::as_u64),
                Some(10000)
            );
            assert_eq!(
                parsed.get("model").and_then(Value::as_str),
                Some("test/model")
            );
        }
    }

    #[test]
    fn merge_question_deny_preserves_valid_custom_permissions() {
        let merged = merge_question_deny(r#"{"bash":"allow","edit":"ask"}"#);
        let parsed: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed.get("bash").and_then(Value::as_str), Some("allow"));
        assert_eq!(parsed.get("edit").and_then(Value::as_str), Some("ask"));
        assert_eq!(parsed.get("question").and_then(Value::as_str), Some("deny"));
    }

    #[test]
    fn merge_question_deny_falls_back_on_invalid_json() {
        let merged = merge_question_deny("not valid json");
        let parsed: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed.get("question").and_then(Value::as_str), Some("deny"));
        assert_eq!(parsed.as_object().unwrap().len(), 1);
    }
}
