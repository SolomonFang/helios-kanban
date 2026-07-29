pub mod client;
pub mod harness;
pub mod normalize_logs;
pub mod session;

use std::{fmt::Display, str::FromStr};

pub use client::AcpClient;
pub use harness::AcpAgentHarness;
pub use normalize_logs::*;
use serde::{Deserialize, Serialize};
pub use session::SessionManager;
use workspace_utils::approvals::{ApprovalStatus, QuestionStatus};

/// Parsed event types for internal processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcpEvent {
    User(String),
    SessionStart(String),
    Message(agent_client_protocol::ContentBlock),
    Thought(agent_client_protocol::ContentBlock),
    ToolCall(agent_client_protocol::ToolCall),
    ToolUpdate(agent_client_protocol::ToolCallUpdate),
    Plan(agent_client_protocol::Plan),
    AvailableCommands(Vec<agent_client_protocol::AvailableCommand>),
    CurrentMode(agent_client_protocol::SessionModeId),
    /// Permission request from the agent. The optional metadata links the
    /// request to a vibe-kanban approval (once one has been created) so the
    /// conversation entry can be rendered as `pending_approval`.
    RequestPermission(
        agent_client_protocol::RequestPermissionRequest,
        Option<PendingApprovalMeta>,
    ),
    ApprovalResponse(ApprovalResponse),
    /// Resolution of an AskUserQuestion approval: carries the final question
    /// status so the conversation entry can leave the pending state.
    QuestionResponse(QuestionResponse),
    Error(String),
    Done(String),
    Other(agent_client_protocol::SessionNotification),
}

impl Display for AcpEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap_or_default())
    }
}

impl FromStr for AcpEvent {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub tool_call_id: String,
    pub status: ApprovalStatus,
}

/// Final status of an AskUserQuestion bridge request, mirroring how
/// [`ApprovalResponse`] resolves a tool approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResponse {
    pub tool_call_id: String,
    pub status: QuestionStatus,
}

/// Approval metadata attached to a permission request event, allowing the log
/// normalizer to mark the tool call entry as `pending_approval`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApprovalMeta {
    pub approval_id: String,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub timeout_at: chrono::DateTime<chrono::Utc>,
}
