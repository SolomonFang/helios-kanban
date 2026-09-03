use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use derivative::Derivative;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, BaseCodingAgent, ExecutorError, SpawnedChild,
        StandardCodingAgentExecutor, gemini::AcpAgentHarness, utils::SlashCommandCacheKey,
    },
    logs::utils::patch,
};

/// dsh profile the ACP composition is installed into.
const DSH_PROFILE: &str = "acp";
/// npm package of the official ACP server plugin.
const DSH_ACP_PACKAGE: &str = "@deepseek-ai/dsh-acp";
/// Provider route owned by `@deepseek-ai/dsh-llm-deepseek`.
const DSH_PROVIDER: &str = "deepseek-official";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
/// Session namespace used by the ACP harness to persist dsh sessions.
const SESSION_NAMESPACE: &str = "deepseek_harness_sessions";

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

fn dsh_home() -> Option<PathBuf> {
    std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".dsh")))
}

fn dsh_profile_dir() -> Option<PathBuf> {
    dsh_home().map(|home| home.join("profiles").join(DSH_PROFILE))
}

/// Entry point of the profile-installed ACP plugin.
fn dsh_acp_plugin_entry() -> Option<PathBuf> {
    dsh_profile_dir().map(|dir| {
        dir.join("node_modules")
            .join("@deepseek-ai/dsh-acp")
            .join("lib")
            .join("index.js")
    })
}

/// DeepSeek Harness (`dsh`) is driven through the official
/// `@deepseek-ai/dsh-acp` server: an automation-only Agent Client Protocol
/// transport over stdio. The shared ACP harness drives the session, detects
/// turn completion, streams committed assistant messages and supports
/// follow-ups (by forking the stored session, since dsh-acp does not
/// implement `session/load` yet).
///
/// The first spawn bootstraps a dedicated `acp` dsh profile
/// (`dsh plugin --profile acp add @deepseek-ai/dsh-acp`), which can take a
/// minute; later runs reuse it. Model selection is injected per run through
/// a `--patch` overlay owned by this executor.
#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct DeepseekHarness {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Model",
        description = "Model to use on the deepseek-official provider route: \"deepseek-v4-flash\" (default) or \"deepseek-v4-pro\""
    )]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Dangerously Skip Permissions (YOLO)",
        description = "Auto-approve all permission requests instead of asking"
    )]
    pub dangerously_skip_permissions: Option<bool>,
    #[serde(flatten)]
    pub cmd: CmdOverrides,
    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    approvals: Option<Arc<dyn ExecutorApprovalService>>,
}

impl DeepseekHarness {
    fn model(&self) -> &str {
        self.model.as_deref().unwrap_or(DEFAULT_MODEL)
    }

    /// Approvals to hand to the ACP harness. When the user opts into YOLO we
    /// pass `None`, which makes the ACP client auto-approve every tool call.
    fn acp_approvals(&self) -> Option<Arc<dyn ExecutorApprovalService>> {
        if self.dangerously_skip_permissions.unwrap_or(false) {
            None
        } else {
            self.approvals.clone()
        }
    }

    /// Overlay file carrying the ACP plugin row for the configured model.
    /// One file per model so concurrent sessions never fight over content.
    fn overlay_path(&self) -> Option<PathBuf> {
        let slug: String = self
            .model()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        dsh_profile_dir().map(|dir| dir.join(format!("helios-acp-overlay-{slug}.yml")))
    }

    /// Make sure the `acp` dsh profile exists with `@deepseek-ai/dsh-acp`
    /// installed, then (re)write the `--patch` overlay for this run.
    async fn ensure_acp_profile(&self) -> Result<PathBuf, ExecutorError> {
        let plugin_entry = dsh_acp_plugin_entry()
            .ok_or_else(|| ExecutorError::Io(std::io::Error::other("cannot resolve dsh home")))?;

        if !plugin_entry.exists() {
            // One-time bootstrap: create the profile and install the plugin.
            let native = detect_dsh_binary();
            let parts = CommandBuilder::new(base_command(native))
                .extend_params(["plugin", "--profile", DSH_PROFILE, "add", DSH_ACP_PACKAGE])
                .build_initial()?;
            let (program, args) = parts.into_resolved().await?;
            let status = Command::new(program)
                .args(args)
                .status()
                .await
                .map_err(ExecutorError::Io)?;
            if !status.success() || !plugin_entry.exists() {
                return Err(ExecutorError::Io(std::io::Error::other(format!(
                    "failed to bootstrap dsh ACP profile: `{DSH_ACP_PACKAGE}` install exited with {status}"
                ))));
            }
        }

        let overlay = self
            .overlay_path()
            .ok_or_else(|| ExecutorError::Io(std::io::Error::other("cannot resolve dsh home")))?;
        // The loader imports plugin rows as ES modules; directory specifiers
        // are rejected, so the row points at the package entry file directly.
        let content = format!(
            "- insert:\n    - id: acp\n      name: '{}'\n      config:\n        provider: {DSH_PROVIDER}\n        model: {}\n",
            plugin_entry.display(),
            self.model()
        );
        tokio::fs::write(&overlay, content)
            .await
            .map_err(ExecutorError::Io)?;
        Ok(overlay)
    }

    async fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let overlay = self
            .ensure_acp_profile()
            .await
            .map_err(|e| CommandBuildError::InvalidShellParams(e.to_string()))?;
        let native = detect_dsh_binary();
        let builder = CommandBuilder::new(base_command(native)).extend_params([
            "--profile",
            DSH_PROFILE,
            "--patch",
            &overlay.to_string_lossy(),
        ]);
        apply_overrides(builder, &self.cmd)
    }

    fn acp_harness(&self) -> AcpAgentHarness {
        // dsh-acp answers `session/load` with "Method not found", so keep
        // native resume off: follow-ups fork the stored session and carry
        // the history in a generated resume prompt.
        AcpAgentHarness::with_session_namespace(SESSION_NAMESPACE)
    }
}

#[async_trait]
impl StandardCodingAgentExecutor for DeepseekHarness {
    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals = Some(approvals);
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let command = self.build_command_builder().await?.build_initial()?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
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
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let command = self.build_command_builder().await?.build_follow_up(&[])?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
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
        let this = self.clone();
        let workdir = workdir.to_path_buf();
        Ok(Box::pin(futures::stream::once(async move {
            let commands = match this.build_command_builder().await {
                Ok(builder) => match builder.build_initial() {
                    Ok(parts) => {
                        let key =
                            SlashCommandCacheKey::new(&workdir, &BaseCodingAgent::DeepseekHarness);
                        crate::executors::acp::discover_acp_slash_commands(
                            parts, &workdir, &this.cmd, &key,
                        )
                        .await
                    }
                    Err(e) => {
                        tracing::warn!("Failed to build dsh command for slash command probe: {e}");
                        Vec::new()
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to bootstrap dsh profile for slash command probe: {e}");
                    Vec::new()
                }
            };
            patch::slash_commands(commands, false, None)
        })))
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, worktree_path: &Path) {
        crate::executors::acp::normalize_logs(msg_store, worktree_path);
    }

    fn default_mcp_config_path(&self) -> Option<PathBuf> {
        // dsh has no documented MCP config file we can write to yet; MCP
        // servers cannot be passed over `session/new` either (dsh-acp
        // rejects non-empty `mcpServers`).
        None
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        let dsh_dir_found = dsh_home().map(|home| home.exists()).unwrap_or(false);

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
        let json = r#"{"append_prompt":null,"model":"deepseek-v4-pro","dangerously_skip_permissions":true}"#;
        let agent: DeepseekHarness = serde_json::from_str(json).unwrap();
        assert_eq!(agent.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(agent.dangerously_skip_permissions, Some(true));
        let serialized = serde_json::to_string(&agent).unwrap();
        let deserialized: DeepseekHarness = serde_json::from_str(&serialized).unwrap();
        assert_eq!(agent, deserialized);
    }

    #[test]
    fn test_deserialize_empty_config() {
        let agent: DeepseekHarness = serde_json::from_str("{}").unwrap();
        assert!(agent.model.is_none());
        assert_eq!(agent.model(), DEFAULT_MODEL);
    }

    fn sessions_dir() -> PathBuf {
        let mut dir = dirs::home_dir().unwrap().join(".vibe-kanban");
        if cfg!(debug_assertions) {
            dir = dir.join("dev");
        }
        dir.join(SESSION_NAMESPACE)
    }

    /// dsh-acp session ids are UUIDs, so any `.jsonl` file is a candidate.
    fn newest_session_file() -> Option<(String, PathBuf)> {
        let dir = sessions_dir();
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        entries.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        let latest = entries.last()?;
        let id = latest
            .file_name()
            .to_string_lossy()
            .trim_end_matches(".jsonl")
            .to_string();
        Some((id, latest.path()))
    }

    /// Manual e2e: spawns a real `dsh --profile acp` session through the ACP
    /// harness, then a follow-up that must recall the first turn's codeword
    /// (via the session-fork fallback, since dsh-acp has no `session/load`).
    ///
    /// Requires DEEPSEEK_API_KEY. Run with:
    /// `DEEPSEEK_API_KEY=sk-... cargo test -p executors --lib deepseek_harness -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn dsh_acp_follow_up_recalls_context() {
        use std::time::Duration;

        use crate::env::RepoContext;

        let api_key = std::env::var("DEEPSEEK_API_KEY").expect("set DEEPSEEK_API_KEY");
        let workdir =
            PathBuf::from(std::env::var("DSH_E2E_WORKDIR").unwrap_or_else(|_| "/tmp".into()));
        let mut env = ExecutionEnv::new(
            RepoContext::new(workdir.clone(), vec![".".to_string()]),
            false,
            String::new(),
        );
        env.insert("DEEPSEEK_API_KEY", api_key);

        let agent = || DeepseekHarness {
            append_prompt: Default::default(),
            model: None,
            dangerously_skip_permissions: Some(true),
            cmd: Default::default(),
            approvals: None,
        };

        // Initial turn: plant a codeword. The first spawn also bootstraps the
        // dsh ACP profile, which can take a few minutes.
        let child = agent()
            .spawn(
                &workdir,
                "Remember the codeword 'blue-elephant-42'. Reply with just: OK",
                &env,
            )
            .await
            .expect("spawn initial");
        if let Some(exit) = child.exit_signal {
            let _ = tokio::time::timeout(Duration::from_secs(600), exit).await;
        }

        let (session_id, _) =
            newest_session_file().expect("no dsh session file after initial spawn");
        eprintln!("[e2e] initial session: {session_id}");

        // Follow-up turn: ask for the codeword.
        let child = agent()
            .spawn_follow_up(
                &workdir,
                "What is the codeword I asked you to remember? Answer with just the codeword.",
                &session_id,
                None,
                &env,
            )
            .await
            .expect("spawn follow-up");
        if let Some(exit) = child.exit_signal {
            let _ = tokio::time::timeout(Duration::from_secs(600), exit).await;
        }

        // Follow-ups fork the stored session: the newest file is a fresh
        // display id whose content carries the full prior history plus the
        // new turn.
        let (follow_up_id, follow_up_path) =
            newest_session_file().expect("no session file after follow-up");
        eprintln!("[e2e] follow-up session: {follow_up_id} (forked from {session_id})");
        let content = std::fs::read_to_string(&follow_up_path).unwrap();
        assert!(
            content.contains("blue-elephant-42"),
            "agent lost session context in follow-up"
        );
        eprintln!("[e2e] follow-up carried context across sessions");
    }
}
