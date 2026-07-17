use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use derivative::Derivative;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, ExecutorError, SpawnedChild, StandardCodingAgentExecutor,
        gemini::AcpAgentHarness,
    },
};

/// Kimi permission mode, mapped to the ACP session mode (`session/set_mode`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KimiMode {
    Default,
    Plan,
    Auto,
    Yolo,
}

impl KimiMode {
    /// Mode id as exposed by `kimi acp` in the `mode` config option.
    fn as_acp_mode_id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::Yolo => "yolo",
        }
    }
}

#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct KimiCli {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<KimiMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(flatten)]
    pub cmd: CmdOverrides,
    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    pub approvals: Option<Arc<dyn ExecutorApprovalService>>,
}

impl KimiCli {
    fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        let builder = CommandBuilder::new("npx -y @moonshot-ai/kimi-code");
        let builder = builder.extend_params(["acp"]);
        apply_overrides(builder, &self.cmd)
    }

    fn acp_harness(&self) -> AcpAgentHarness {
        // Kimi advertises `loadSession`, so follow-ups resume the original
        // agent session natively instead of forking into a new one.
        let mut harness = AcpAgentHarness::with_session_namespace("kimi_sessions")
            .with_native_session_resume(true);
        if let Some(mode) = self.mode {
            harness = harness.with_mode(mode.as_acp_mode_id());
        }
        if let Some(model) = &self.model {
            harness = harness.with_model(model.clone());
        }
        harness
    }

    fn approvals_for_mode(&self) -> Option<Arc<dyn ExecutorApprovalService>> {
        // YOLO is handled client-side: when no approval service is attached
        // the ACP harness auto-approves all permission requests.
        if matches!(self.mode, Some(KimiMode::Yolo)) {
            None
        } else {
            self.approvals.clone()
        }
    }
}

#[async_trait]
impl StandardCodingAgentExecutor for KimiCli {
    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals = Some(approvals);
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let kimi_command = self.build_command_builder()?.build_initial()?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        let harness = self.acp_harness();
        let approvals = self.approvals_for_mode();
        harness
            .spawn_with_command(
                current_dir,
                combined_prompt,
                kimi_command,
                env,
                &self.cmd,
                approvals,
            )
            .await
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        _reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let kimi_command = self.build_command_builder()?.build_follow_up(&[])?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        let harness = self.acp_harness();
        let approvals = self.approvals_for_mode();
        harness
            .spawn_follow_up_with_command(
                current_dir,
                combined_prompt,
                session_id,
                kimi_command,
                env,
                &self.cmd,
                approvals,
            )
            .await
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, worktree_path: &Path) {
        crate::executors::acp::normalize_logs(msg_store, worktree_path);
    }

    // MCP configuration methods
    fn default_mcp_config_path(&self) -> Option<std::path::PathBuf> {
        dirs::home_dir().map(|home| home.join(".kimi-code").join("mcp.json"))
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        let installation_indicator_found = dirs::home_dir()
            .map(|home| home.join(".kimi-code").exists())
            .unwrap_or(false);

        if installation_indicator_found {
            AvailabilityInfo::InstallationFound
        } else {
            AvailabilityInfo::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Mutex, time::Duration};

    use tokio_util::sync::CancellationToken;
    use workspace_utils::approvals::{ApprovalStatus, QuestionStatus};

    use super::*;
    use crate::{approvals::ExecutorApprovalError, env::RepoContext};

    #[derive(Default)]
    struct RecordingApprovals {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ExecutorApprovalService for RecordingApprovals {
        async fn create_tool_approval(
            &self,
            tool_name: &str,
        ) -> Result<String, ExecutorApprovalError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(format!("tool:{tool_name}"));
            eprintln!("[e2e] create_tool_approval: {tool_name}");
            Ok(format!("rec-{}", calls.len()))
        }

        async fn create_question_approval(
            &self,
            tool_name: &str,
            question_count: usize,
        ) -> Result<String, ExecutorApprovalError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(format!("question:{tool_name}x{question_count}"));
            eprintln!("[e2e] create_question_approval: {tool_name} x{question_count}");
            Ok(format!("rec-{}", calls.len()))
        }

        async fn wait_tool_approval(
            &self,
            _approval_id: &str,
            _cancel: CancellationToken,
        ) -> Result<ApprovalStatus, ExecutorApprovalError> {
            Ok(ApprovalStatus::Approved)
        }

        async fn wait_question_answer(
            &self,
            _approval_id: &str,
            _cancel: CancellationToken,
        ) -> Result<QuestionStatus, ExecutorApprovalError> {
            Err(ExecutorApprovalError::ServiceUnavailable)
        }
    }

    /// Manual e2e probe: spawns a real `kimi acp` session in plan mode through
    /// the ACP harness and asserts that permission requests (plan exit /
    /// questions) reach the approval service.
    ///
    /// Requires `kimi login` on the machine. Run with:
    /// `KIMI_E2E_WORKDIR=/path/to/repo cargo test -p executors --lib kimi -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn kimi_acp_plan_mode_permission_reaches_approvals() {
        let workdir =
            std::env::var("KIMI_E2E_WORKDIR").expect("set KIMI_E2E_WORKDIR to a real repo");
        let prompt = std::env::var("KIMI_E2E_PROMPT")
            .unwrap_or_else(|_| "审核一下当前仓库的代码，然后退出 plan 模式".to_string());

        let recording = Arc::new(RecordingApprovals::default());
        let mut kimi = KimiCli {
            append_prompt: Default::default(),
            mode: Some(KimiMode::Plan),
            model: None,
            cmd: Default::default(),
            approvals: None,
        };
        kimi.use_approvals(recording.clone());

        let env = ExecutionEnv::new(
            RepoContext::new(PathBuf::from(&workdir), vec![".".to_string()]),
            false,
            String::new(),
        );

        let child = kimi
            .spawn(std::path::Path::new(&workdir), &prompt, &env)
            .await
            .expect("spawn kimi");

        let deadline = std::time::Instant::now() + Duration::from_secs(15 * 60);
        loop {
            if !recording.calls.lock().unwrap().is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for permission request; kimi never asked for approval"
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if let Some(cancel) = &child.cancel {
            cancel.cancel();
        }
        let calls = recording.calls.lock().unwrap().clone();
        assert!(!calls.is_empty(), "no permission requests reached approvals");
        eprintln!("[e2e] success, approval calls: {calls:?}");
    }

    fn kimi_sessions_dir() -> PathBuf {
        let mut dir = dirs::home_dir().unwrap().join(".vibe-kanban");
        if cfg!(debug_assertions) {
            dir = dir.join("dev");
        }
        dir.join("kimi_sessions")
    }

    fn newest_session_file() -> Option<(String, PathBuf)> {
        let dir = kimi_sessions_dir();
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().and_then(|s| s.to_str()) == Some("jsonl")
                    && e.file_name().to_string_lossy().starts_with("session_")
            })
            .collect();
        entries.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        let latest = entries.last()?;
        let id = latest.file_name().to_string_lossy().trim_end_matches(".jsonl").to_string();
        Some((id, latest.path()))
    }

    fn agent_session_ids() -> std::collections::BTreeSet<String> {
        let home = dirs::home_dir().unwrap().join(".kimi-code").join("sessions");
        let mut ids = std::collections::BTreeSet::new();
        if let Ok(workspaces) = std::fs::read_dir(home) {
            for ws in workspaces.flatten() {
                if let Ok(sessions) = std::fs::read_dir(ws.path()) {
                    for s in sessions.flatten() {
                        let name = s.file_name().to_string_lossy().to_string();
                        if name.starts_with("session_") {
                            ids.insert(name);
                        }
                    }
                }
            }
        }
        ids
    }

    fn make_kimi(mode: Option<KimiMode>, approvals: Arc<dyn ExecutorApprovalService>) -> KimiCli {
        let mut kimi = KimiCli {
            append_prompt: Default::default(),
            mode,
            model: None,
            cmd: Default::default(),
            approvals: None,
        };
        kimi.use_approvals(approvals);
        kimi
    }

    /// Manual e2e: follow-ups must resume the ORIGINAL kimi session via ACP
    /// `session/load` — no new agent session may be created, and the agent
    /// must recall the previous turn without a stuffed resume prompt.
    ///
    /// Requires `kimi login`. Run with:
    /// `KIMI_E2E_WORKDIR=/path/to/repo cargo test -p executors --lib kimi -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn kimi_acp_follow_up_resumes_native_session() {
        let workdir = std::env::var("KIMI_E2E_WORKDIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        let workdir = PathBuf::from(workdir);
        let env = ExecutionEnv::new(
            RepoContext::new(workdir.clone(), vec![".".to_string()]),
            false,
            String::new(),
        );

        // Initial turn: plant a codeword
        let kimi = make_kimi(None, Arc::new(RecordingApprovals::default()));
        let child = kimi
            .spawn(
                &workdir,
                "Remember the codeword 'blue-elephant-42'. Reply with just: OK",
                &env,
            )
            .await
            .expect("spawn initial");
        if let Some(exit) = child.exit_signal {
            let _ = tokio::time::timeout(Duration::from_secs(300), exit).await;
        }

        let (session_id, session_path) =
            newest_session_file().expect("no kimi session file after initial spawn");
        eprintln!("[e2e] initial session: {session_id}");

        let agent_sessions_before = agent_session_ids();

        // Follow-up turn: ask for the codeword
        let kimi = make_kimi(None, Arc::new(RecordingApprovals::default()));
        let child = kimi
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
            let _ = tokio::time::timeout(Duration::from_secs(300), exit).await;
        }

        // 1. The follow-up must NOT create a new agent-side session
        let agent_sessions_after = agent_session_ids();
        let new_agent_sessions: Vec<_> =
            agent_sessions_after.difference(&agent_sessions_before).collect();
        assert!(
            new_agent_sessions.is_empty(),
            "follow-up created new agent sessions instead of resuming: {new_agent_sessions:?}"
        );

        // 2. The display session id must be unchanged
        let (follow_up_id, follow_up_path) =
            newest_session_file().expect("no session file after follow-up");
        assert_eq!(
            follow_up_id, session_id,
            "display session id changed across follow-up"
        );

        // 3. The agent must recall the codeword from native context
        let content = std::fs::read_to_string(&follow_up_path).unwrap();
        assert!(
            content.contains("blue-elephant-42"),
            "agent lost session context in follow-up"
        );
        let _ = session_path;
        eprintln!("[e2e] follow-up resumed native session {session_id} with context intact");
    }
}
