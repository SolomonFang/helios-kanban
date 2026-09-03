use std::{collections::HashMap, process::Stdio, sync::Arc};

use agent_client_protocol::{self as acp};
use async_trait::async_trait;
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::{Mutex, mpsc, watch},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use workspace_utils::approvals::{
    APPROVAL_TIMEOUT_SECONDS, ApprovalStatus, QuestionAnswer, QuestionStatus,
};

use crate::{
    approvals::{ExecutorApprovalError, ExecutorApprovalService},
    executors::acp::{AcpEvent, ApprovalResponse, PendingApprovalMeta, QuestionResponse},
};

/// Tool call title the kimi ACP adapter uses when bridging the AskUserQuestion
/// tool through `session/request_permission` (options are namespaced
/// `q{n}_opt_*` plus a trailing `reject_once` skip option).
const ASK_USER_QUESTION_TITLE: &str = "AskUserQuestion";

/// Fallback cap on retained terminal output when the agent does not send
/// `output_byte_limit`, so a chatty command cannot grow memory unboundedly.
const DEFAULT_TERMINAL_OUTPUT_BYTE_LIMIT: u64 = 1024 * 1024;

/// Accumulated output of a terminal command. Once the byte limit is exceeded
/// the buffer is truncated from the beginning, as the ACP spec requires.
#[derive(Default)]
struct TerminalBuffer {
    output: Vec<u8>,
    truncated: bool,
}

impl TerminalBuffer {
    fn push(&mut self, chunk: &[u8], byte_limit: u64) {
        self.output.extend_from_slice(chunk);
        let limit = byte_limit as usize;
        if self.output.len() > limit {
            // Truncate on a UTF-8 character boundary: skip leading
            // continuation bytes of a multi-byte character.
            let mut start = self.output.len() - limit;
            while start < self.output.len() && (self.output[start] & 0b1100_0000) == 0b1000_0000 {
                start += 1;
            }
            self.output.drain(..start);
            self.truncated = true;
        }
    }
}

/// A terminal spawned on behalf of the agent via `terminal/create`.
struct Terminal {
    buffer: Arc<Mutex<TerminalBuffer>>,
    exit_tx: watch::Sender<Option<acp::TerminalExitStatus>>,
    kill_tx: mpsc::UnboundedSender<()>,
}

/// Convert a process exit status into the ACP wire shape.
fn terminal_exit_status(status: std::process::ExitStatus) -> acp::TerminalExitStatus {
    let mut exit_status = acp::TerminalExitStatus::new();
    if let Some(code) = status.code() {
        // Exit codes are non-negative on all supported platforms.
        exit_status = exit_status.exit_code(code as u32);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            exit_status = exit_status.signal(signal.to_string());
        }
    }
    exit_status
}

/// Drain a child output stream into the shared terminal buffer.
async fn drain_terminal_stream<R: tokio::io::AsyncRead + Unpin>(
    mut stream: R,
    buffer: Arc<Mutex<TerminalBuffer>>,
    byte_limit: u64,
) {
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buffer.lock().await.push(&chunk[..n], byte_limit),
        }
    }
}

/// ACP client that handles agent-client protocol communication
#[derive(Clone)]
pub struct AcpClient {
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    approvals: Option<Arc<dyn ExecutorApprovalService>>,
    feedback_queue: Arc<Mutex<Vec<String>>>,
    cancel: CancellationToken,
    terminals: Arc<Mutex<HashMap<String, Terminal>>>,
}

impl AcpClient {
    /// Create a new ACP client
    pub fn new(
        event_tx: mpsc::UnboundedSender<AcpEvent>,
        approvals: Option<Arc<dyn ExecutorApprovalService>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            event_tx,
            approvals,
            feedback_queue: Arc::new(Mutex::new(Vec::new())),
            cancel,
            terminals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn record_user_prompt_event(&self, prompt: &str) {
        self.send_event(AcpEvent::User(prompt.to_string()));
    }

    /// Send an event to the event channel
    fn send_event(&self, event: AcpEvent) {
        if let Err(e) = self.event_tx.send(event) {
            warn!("Failed to send ACP event: {}", e);
        }
    }

    /// Queue a user feedback message to be sent after a denial.
    pub async fn enqueue_feedback(&self, message: String) {
        let trimmed = message.trim().to_string();
        if !trimmed.is_empty() {
            let mut q = self.feedback_queue.lock().await;
            q.push(trimmed);
        }
    }

    /// Drain and return queued feedback messages.
    pub async fn drain_feedback(&self) -> Vec<String> {
        let mut q = self.feedback_queue.lock().await;
        q.drain(..).collect()
    }

    /// Handle an AskUserQuestion bridge request: surface the question through
    /// the question-approval flow and translate the user's answer back into
    /// the permission option the agent offered (`q{n}_opt_*`).
    async fn handle_question_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> Result<acp::RequestPermissionResponse, acp::Error> {
        let tool_call_id = args.tool_call.tool_call_id.0.to_string();
        let approval_service = self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)
            .map_err(|_| acp::Error::invalid_request())?;

        let tool_title = args.tool_call.fields.title.as_deref().unwrap_or("tool");
        // The ACP question bridge degrades multi-question prompts to the first
        // question, so there is always exactly one question on the wire.
        let approval_id = match approval_service
            .create_question_approval(tool_title, 1)
            .await
        {
            Ok(id) => id,
            Err(ExecutorApprovalError::Cancelled) => {
                debug!("ACP question cancelled for tool_call_id={tool_call_id}");
                self.send_event(AcpEvent::RequestPermission(args.clone(), None));
                return Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            }
            Err(err) => {
                tracing::error!(
                    "ACP question approval failed for tool_call_id={tool_call_id}: {err}"
                );
                self.send_event(AcpEvent::RequestPermission(args.clone(), None));
                return Err(acp::Error::internal_error());
            }
        };

        // Link the question approval to the tool call entry so the UI can
        // render the interactive question banner on it.
        let requested_at = chrono::Utc::now();
        let timeout_at = requested_at + chrono::Duration::seconds(APPROVAL_TIMEOUT_SECONDS);
        self.send_event(AcpEvent::RequestPermission(
            args.clone(),
            Some(PendingApprovalMeta {
                approval_id: approval_id.clone(),
                requested_at,
                timeout_at,
            }),
        ));

        let status = match approval_service
            .wait_question_answer(&approval_id, self.cancel.clone())
            .await
        {
            Ok(s) => s,
            Err(ExecutorApprovalError::Cancelled) => {
                debug!("ACP question cancelled for tool_call_id={tool_call_id}");
                return Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            }
            Err(err) => {
                tracing::error!(
                    "ACP question approval failed for tool_call_id={tool_call_id}: {err}"
                );
                return Err(acp::Error::internal_error());
            }
        };

        let outcome = match &status {
            QuestionStatus::Answered { answers } => {
                let (selected, additional, custom) =
                    match_question_option(&args.options, answers);
                if !additional.is_empty() {
                    self.enqueue_feedback(format_answer_feedback(
                        "The user also answered additional question(s):",
                        &additional,
                    ))
                    .await;
                }
                if !custom.is_empty() {
                    self.enqueue_feedback(format_answer_feedback(
                        "The user answered the question(s) with custom text:",
                        &custom,
                    ))
                    .await;
                }
                if let Some(option_id) = selected {
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        option_id,
                    ))
                } else {
                    // The answer does not map to any offered option (e.g. a
                    // custom free-text reply): dismiss the question via its
                    // skip option; the agent gets the canonical dismissal and
                    // the queued feedback carries the user's input.
                    match skip_option(&args.options) {
                        Some(opt) => acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(opt.option_id.clone()),
                        ),
                        None => acp::RequestPermissionOutcome::Cancelled,
                    }
                }
            }
            QuestionStatus::TimedOut => {
                warn!("Question approval timed out");
                acp::RequestPermissionOutcome::Cancelled
            }
        };

        self.send_event(AcpEvent::QuestionResponse(QuestionResponse {
            tool_call_id: tool_call_id.clone(),
            status: status.clone(),
        }));

        Ok(acp::RequestPermissionResponse::new(outcome))
    }
}

/// Find the permission option that dismisses a bridged question (the kimi
/// adapter appends a `reject_once` "Skip" option for this).
fn skip_option(options: &[acp::PermissionOption]) -> Option<&acp::PermissionOption> {
    options
        .iter()
        .find(|o| matches!(o.kind, acp::PermissionOptionKind::RejectOnce))
}

/// Map question answers back to the permission option the agent offered.
///
/// Returns the selected option id — the first answer label of the first
/// question matching an `allow_once` option name — plus the answers that
/// could not be mapped, split into `additional` (valid answers the bridge
/// cannot represent: questions beyond the first, and extra labels of a
/// multi-select first question) and `custom` (free-text replies matching no
/// offered option).
fn match_question_option(
    options: &[acp::PermissionOption],
    answers: &[QuestionAnswer],
) -> (
    Option<acp::PermissionOptionId>,
    Vec<QuestionAnswer>,
    Vec<QuestionAnswer>,
) {
    let mut selected = None;
    let mut additional = Vec::new();
    let mut custom = Vec::new();
    for (idx, qa) in answers.iter().enumerate() {
        let mut additional_labels = Vec::new();
        let mut custom_labels = Vec::new();
        for label in &qa.answer {
            let option = options.iter().find(|o| {
                matches!(o.kind, acp::PermissionOptionKind::AllowOnce) && o.name == *label
            });
            match (idx == 0, option) {
                (true, Some(opt)) if selected.is_none() => {
                    selected = Some(opt.option_id.clone());
                }
                (true, Some(_)) => additional_labels.push(label.clone()),
                (true, None) => custom_labels.push(label.clone()),
                (false, _) => additional_labels.push(label.clone()),
            }
        }
        if !additional_labels.is_empty() {
            additional.push(QuestionAnswer {
                question: qa.question.clone(),
                answer: additional_labels,
            });
        }
        if !custom_labels.is_empty() {
            custom.push(QuestionAnswer {
                question: qa.question.clone(),
                answer: custom_labels,
            });
        }
    }
    (selected, additional, custom)
}

/// Format answers the bridge could not map as user feedback, so no input is
/// silently dropped.
fn format_answer_feedback(header: &str, answers: &[QuestionAnswer]) -> String {
    let lines = answers
        .iter()
        .map(|qa| format!("- {}: {}", qa.question, qa.answer.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}\n{lines}")
}

#[async_trait(?Send)]
impl acp::Client for AcpClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> Result<acp::RequestPermissionResponse, acp::Error> {
        // Kimi bridges the AskUserQuestion tool through `session/request_permission`,
        // tagging the embedded tool call with the fixed title.
        let is_question = args.tool_call.fields.title.as_deref() == Some(ASK_USER_QUESTION_TITLE);

        if self.approvals.is_none() {
            self.send_event(AcpEvent::RequestPermission(args.clone(), None));
            // Auto-approve with best available option when no approval service is
            // configured. Questions are the exception: never fabricate an answer —
            // dismiss the prompt via its skip option so the agent decides on its own.
            // Prefer AllowOnce over AllowAlways: an always-allow may be persisted
            // by the agent (e.g. kimi writes always-allow rules), leaking the
            // auto-approval beyond this run.
            let chosen_option = if is_question {
                skip_option(&args.options)
            } else {
                args.options
                    .iter()
                    .find(|o| matches!(o.kind, acp::PermissionOptionKind::AllowOnce))
                    .or_else(|| {
                        args.options
                            .iter()
                            .find(|o| matches!(o.kind, acp::PermissionOptionKind::AllowAlways))
                    })
                    .or_else(|| args.options.first())
            };

            let outcome = if let Some(opt) = chosen_option {
                debug!("Auto-approving permission with option: {}", opt.option_id);
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    opt.option_id.clone(),
                ))
            } else {
                warn!("No permission options available, cancelling");
                acp::RequestPermissionOutcome::Cancelled
            };

            return Ok(acp::RequestPermissionResponse::new(outcome));
        }

        if is_question {
            return self.handle_question_permission(args).await;
        }

        let tool_call_id = args.tool_call.tool_call_id.0.to_string();
        let approval_service = self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)
            .map_err(|_| acp::Error::invalid_request())?;

        let tool_title = args.tool_call.fields.title.as_deref().unwrap_or("tool");
        let approval_id = match approval_service.create_tool_approval(tool_title).await {
            Ok(id) => id,
            Err(ExecutorApprovalError::Cancelled) => {
                debug!("ACP approval cancelled for tool_call_id={tool_call_id}");
                self.send_event(AcpEvent::RequestPermission(args.clone(), None));
                return Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            }
            Err(err) => {
                tracing::error!("ACP approval failed for tool_call_id={tool_call_id}: {err}");
                self.send_event(AcpEvent::RequestPermission(args.clone(), None));
                return Err(acp::Error::internal_error());
            }
        };

        // The approval now exists: re-emit the permission request with the
        // approval metadata so the UI can render an approve/deny card on the
        // tool call entry.
        let requested_at = chrono::Utc::now();
        let timeout_at = requested_at + chrono::Duration::seconds(APPROVAL_TIMEOUT_SECONDS);
        self.send_event(AcpEvent::RequestPermission(
            args.clone(),
            Some(PendingApprovalMeta {
                approval_id: approval_id.clone(),
                requested_at,
                timeout_at,
            }),
        ));

        let status = match approval_service
            .wait_tool_approval(&approval_id, self.cancel.clone())
            .await
        {
            Ok(s) => s,
            Err(ExecutorApprovalError::Cancelled) => {
                debug!("ACP approval cancelled for tool_call_id={tool_call_id}");
                return Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            }
            Err(err) => {
                tracing::error!("ACP approval failed for tool_call_id={tool_call_id}: {err}");
                return Err(acp::Error::internal_error());
            }
        };

        // Map our ApprovalStatus to ACP outcome
        let outcome = match &status {
            ApprovalStatus::Approved => {
                let chosen = args
                    .options
                    .iter()
                    .find(|o| matches!(o.kind, acp::PermissionOptionKind::AllowOnce));
                if let Some(opt) = chosen {
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        opt.option_id.clone(),
                    ))
                } else {
                    tracing::error!("No suitable approval option found, cancelling");
                    return Err(acp::Error::invalid_request());
                }
            }
            ApprovalStatus::Denied { reason } => {
                // If user provided a reason, queue it to send after denial
                if let Some(feedback) = reason.as_ref() {
                    self.enqueue_feedback(feedback.clone()).await;
                }
                let chosen = args
                    .options
                    .iter()
                    .find(|o| matches!(o.kind, acp::PermissionOptionKind::RejectOnce));
                if let Some(opt) = chosen {
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        opt.option_id.clone(),
                    ))
                } else {
                    warn!("No permission options for denial, cancelling");
                    acp::RequestPermissionOutcome::Cancelled
                }
            }
            ApprovalStatus::TimedOut => {
                warn!("Approval timed out");
                acp::RequestPermissionOutcome::Cancelled
            }
            ApprovalStatus::Pending => {
                // This should not occur after waiter resolves
                warn!("Approval resolved to Pending");
                acp::RequestPermissionOutcome::Cancelled
            }
        };

        self.send_event(AcpEvent::ApprovalResponse(ApprovalResponse {
            tool_call_id: tool_call_id.clone(),
            status: status.clone(),
        }));

        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> Result<(), acp::Error> {
        // Convert to typed events
        let event = match args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => Some(AcpEvent::Message(chunk.content)),
            acp::SessionUpdate::AgentThoughtChunk(chunk) => Some(AcpEvent::Thought(chunk.content)),
            acp::SessionUpdate::ToolCall(tc) => Some(AcpEvent::ToolCall(tc)),
            acp::SessionUpdate::ToolCallUpdate(update) => Some(AcpEvent::ToolUpdate(update)),
            acp::SessionUpdate::Plan(plan) => Some(AcpEvent::Plan(plan)),
            _ => Some(AcpEvent::Other(args)),
        };

        if let Some(event) = event {
            self.send_event(event);
        }

        Ok(())
    }

    // File system operations - not implemented as we don't expose FS
    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> Result<acp::WriteTextFileResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> Result<acp::ReadTextFileResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    // Terminal operations: spawn local commands on behalf of the agent and
    // track their output and exit status. Advertised via the `terminal`
    // client capability at initialize time.
    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> Result<acp::CreateTerminalResponse, acp::Error> {
        let byte_limit = args
            .output_byte_limit
            .unwrap_or(DEFAULT_TERMINAL_OUTPUT_BYTE_LIMIT);

        let mut command = Command::new(&args.command);
        command
            .args(&args.args)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &args.cwd {
            command.current_dir(cwd);
        }
        for var in &args.env {
            command.env(&var.name, &var.value);
        }

        let mut child = command.spawn().map_err(|err| {
            acp::Error::internal_error().data(format!("failed to spawn `{}`: {err}", args.command))
        })?;

        let buffer = Arc::new(Mutex::new(TerminalBuffer::default()));
        let (exit_tx, _) = watch::channel(None::<acp::TerminalExitStatus>);
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel::<()>();

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(drain_terminal_stream(stdout, buffer.clone(), byte_limit));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_terminal_stream(stderr, buffer.clone(), byte_limit));
        }

        // Reap the child (killing it first if requested) and record its exit
        // status so `terminal/output` and `terminal/wait_for_exit` can see it.
        let exit_status_tx = exit_tx.clone();
        tokio::spawn(async move {
            let exited = tokio::select! {
                _ = kill_rx.recv() => None,
                status = child.wait() => Some(status),
            };
            let status = match exited {
                Some(status) => status,
                None => {
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            let exit_status = status.map(terminal_exit_status).unwrap_or_else(|err| {
                warn!("failed to wait on ACP terminal command: {err}");
                acp::TerminalExitStatus::new()
            });
            let _ = exit_status_tx.send(Some(exit_status));
        });

        let terminal_id = format!("term-{}", uuid::Uuid::new_v4());
        self.terminals.lock().await.insert(
            terminal_id.clone(),
            Terminal {
                buffer,
                exit_tx,
                kill_tx,
            },
        );

        Ok(acp::CreateTerminalResponse::new(terminal_id))
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> Result<acp::TerminalOutputResponse, acp::Error> {
        let terminals = self.terminals.lock().await;
        let terminal = terminals
            .get(args.terminal_id.0.as_ref())
            .ok_or_else(|| acp::Error::invalid_params().data("unknown terminal id"))?;

        let buffer = terminal.buffer.lock().await;
        let mut response = acp::TerminalOutputResponse::new(
            String::from_utf8_lossy(&buffer.output).into_owned(),
            buffer.truncated,
        );
        if let Some(exit_status) = terminal.exit_tx.borrow().as_ref().cloned() {
            response = response.exit_status(exit_status);
        }
        Ok(response)
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> Result<acp::ReleaseTerminalResponse, acp::Error> {
        // Releasing is idempotent: kill the command if it is still running
        // and drop the terminal from the registry.
        if let Some(terminal) = self
            .terminals
            .lock()
            .await
            .remove(args.terminal_id.0.as_ref())
        {
            let _ = terminal.kill_tx.send(());
        }
        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> Result<acp::WaitForTerminalExitResponse, acp::Error> {
        let mut exit_rx = {
            let terminals = self.terminals.lock().await;
            terminals
                .get(args.terminal_id.0.as_ref())
                .ok_or_else(|| acp::Error::invalid_params().data("unknown terminal id"))?
                .exit_tx
                .subscribe()
        };

        let exit_status = exit_rx
            .wait_for(Option::is_some)
            .await
            .map_err(|_| acp::Error::internal_error().data("terminal exited without status"))?
            .as_ref()
            .cloned()
            .unwrap_or_default();

        Ok(acp::WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal_command(
        &self,
        args: acp::KillTerminalCommandRequest,
    ) -> Result<acp::KillTerminalCommandResponse, acp::Error> {
        let terminals = self.terminals.lock().await;
        let terminal = terminals
            .get(args.terminal_id.0.as_ref())
            .ok_or_else(|| acp::Error::invalid_params().data("unknown terminal id"))?;
        // Fails silently if the command already exited.
        let _ = terminal.kill_tx.send(());
        Ok(acp::KillTerminalCommandResponse::new())
    }

    // Extension methods
    async fn ext_method(&self, _args: acp::ExtRequest) -> Result<acp::ExtResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> Result<(), acp::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &str, name: &str, kind: acp::PermissionOptionKind) -> acp::PermissionOption {
        acp::PermissionOption::new(acp::PermissionOptionId::new(id), name, kind)
    }

    /// Mirrors the kimi ACP question bridge: one `allow_once` option per
    /// question option, plus a trailing `reject_once` skip option.
    fn question_options() -> Vec<acp::PermissionOption> {
        vec![
            option("q0_opt_0", "方案 A", acp::PermissionOptionKind::AllowOnce),
            option("q0_opt_1", "方案 B", acp::PermissionOptionKind::AllowOnce),
            option("q0_skip", "Skip", acp::PermissionOptionKind::RejectOnce),
        ]
    }

    fn answered(question: &str, labels: &[&str]) -> QuestionAnswer {
        QuestionAnswer {
            question: question.to_string(),
            answer: labels.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn match_question_option_maps_label_to_offered_option() {
        let options = question_options();
        let answers = vec![answered("按哪个方案改?", &["方案 B"])];
        let (selected, additional, custom) = match_question_option(&options, &answers);
        assert_eq!(selected.unwrap().0.as_ref(), "q0_opt_1");
        assert!(additional.is_empty());
        assert!(custom.is_empty());
    }

    #[test]
    fn match_question_option_never_maps_an_answer_to_the_skip_option() {
        let options = question_options();
        let answers = vec![answered("q", &["Skip"])];
        let (selected, additional, custom) = match_question_option(&options, &answers);
        assert!(selected.is_none());
        assert!(additional.is_empty());
        assert_eq!(custom.len(), 1);
    }

    #[test]
    fn match_question_option_collects_custom_answers() {
        let options = question_options();
        let answers = vec![answered("q", &["自定义回答"])];
        let (selected, additional, custom) = match_question_option(&options, &answers);
        assert!(selected.is_none());
        assert!(additional.is_empty());
        assert_eq!(custom[0].answer, vec!["自定义回答".to_string()]);
    }

    #[test]
    fn match_question_option_only_maps_the_first_question() {
        let options = question_options();
        let answers = vec![
            answered("q1", &["方案 A"]),
            // The ACP bridge cannot represent answers beyond the first question
            answered("q2", &["方案 B"]),
        ];
        let (selected, additional, custom) = match_question_option(&options, &answers);
        assert_eq!(selected.unwrap().0.as_ref(), "q0_opt_0");
        assert_eq!(additional.len(), 1);
        assert_eq!(additional[0].question, "q2");
        assert!(custom.is_empty());
    }

    #[test]
    fn match_question_option_treats_extra_multi_select_labels_as_additional() {
        let options = question_options();
        let answers = vec![answered("q1", &["方案 A", "方案 B"])];
        let (selected, additional, custom) = match_question_option(&options, &answers);
        assert_eq!(selected.unwrap().0.as_ref(), "q0_opt_0");
        assert_eq!(additional.len(), 1);
        assert_eq!(additional[0].answer, vec!["方案 B".to_string()]);
        assert!(custom.is_empty());
    }

    #[test]
    fn skip_option_finds_the_reject_once_option() {
        let options = question_options();
        assert_eq!(
            skip_option(&options).unwrap().option_id.0.as_ref(),
            "q0_skip"
        );

        let without_skip = [option(
            "allow",
            "Allow",
            acp::PermissionOptionKind::AllowOnce,
        )];
        assert!(skip_option(&without_skip).is_none());
    }

    #[test]
    fn format_answer_feedback_keeps_questions_and_labels() {
        let feedback =
            format_answer_feedback("header:", &[answered("q1", &["x", "y"])]);
        assert!(feedback.starts_with("header:"));
        assert!(feedback.contains("q1: x, y"));
    }

    #[test]
    fn terminal_buffer_keeps_output_under_limit_untouched() {
        let mut buffer = TerminalBuffer::default();
        buffer.push(b"hello", 10);
        assert!(!buffer.truncated);
        assert_eq!(buffer.output, b"hello");
    }

    #[test]
    fn terminal_buffer_truncates_from_the_front() {
        let mut buffer = TerminalBuffer::default();
        buffer.push("héllo wörld".as_bytes(), 5);
        assert!(buffer.truncated);
        assert_eq!(String::from_utf8(buffer.output).unwrap(), "örld");
    }

    #[test]
    fn terminal_buffer_never_splits_multibyte_chars() {
        let mut buffer = TerminalBuffer::default();
        // "é" is two bytes; a 1-byte limit cannot hold it, so it is dropped
        // entirely rather than split.
        buffer.push("aé".as_bytes(), 1);
        assert!(buffer.truncated);
        assert!(buffer.output.is_empty());
    }
}
