pub mod client;
pub mod harness;
pub mod normalize_logs;
pub mod session;

use std::{fmt::Display, path::Path, str::FromStr};

pub use client::AcpClient;
pub use harness::AcpAgentHarness;
pub use normalize_logs::*;
use serde::{Deserialize, Serialize};
pub use session::SessionManager;
use workspace_utils::approvals::{ApprovalStatus, QuestionStatus};

use crate::{
    command::{CmdOverrides, CommandParts},
    env::{ExecutionEnv, RepoContext},
    executors::{
        SlashCommandDescription,
        utils::{SlashCommandCache, SlashCommandCacheKey},
    },
};

/// User prompt payload. Serializes as an object carrying a generated
/// `message_id` (used for per-message session rollback); the legacy
/// bare-string form (`{"User": "prompt"}` in old log/session files) still
/// deserializes.
#[derive(Debug, Clone, Serialize)]
pub struct UserPrompt {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl UserPrompt {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            message_id: Some(uuid::Uuid::new_v4().to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for UserPrompt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Legacy(String),
            Full {
                prompt: String,
                #[serde(default)]
                message_id: Option<String>,
            },
        }

        Ok(match Repr::deserialize(deserializer)? {
            Repr::Legacy(prompt) => Self {
                prompt,
                message_id: None,
            },
            Repr::Full { prompt, message_id } => Self { prompt, message_id },
        })
    }
}

/// Discover an ACP agent's slash commands by probing a short-lived session.
/// Cached per (workdir, agent); failures degrade to an empty list.
pub async fn discover_acp_slash_commands(
    command_parts: CommandParts,
    workdir: &Path,
    cmd_overrides: &CmdOverrides,
    cache_key: &SlashCommandCacheKey,
) -> Vec<SlashCommandDescription> {
    if let Some(cached) = SlashCommandCache::instance().get(cache_key) {
        return cached.as_ref().clone();
    }

    let env = ExecutionEnv::new(RepoContext::default(), false, String::new());
    let commands =
        AcpAgentHarness::probe_available_commands(command_parts, workdir, &env, cmd_overrides)
            .await
            .into_iter()
            .map(|cmd| SlashCommandDescription {
                name: cmd.name,
                description: if cmd.description.is_empty() {
                    None
                } else {
                    Some(cmd.description)
                },
            })
            .collect::<Vec<_>>();

    SlashCommandCache::instance().put(cache_key.clone(), commands.clone());
    commands
}

/// Parsed event types for internal processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcpEvent {
    User(UserPrompt),
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
    /// An `elicitation/create` form request (kimi's native multi-question
    /// channel). Carries the tool call id the form belongs to plus the
    /// approval metadata, so the normalizer can mark the matching tool call
    /// entry as `pending_approval`.
    Elicitation {
        tool_call_id: String,
        meta: Option<PendingApprovalMeta>,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_event_legacy_format_parses() {
        let event = AcpEvent::from_str(r#"{"User":"hello world"}"#).unwrap();
        let AcpEvent::User(user) = event else {
            panic!("expected User event");
        };
        assert_eq!(user.prompt, "hello world");
        assert_eq!(user.message_id, None);
    }

    #[test]
    fn user_event_with_message_id_round_trips() {
        let event = AcpEvent::User(UserPrompt::new("fix the bug"));
        let line = event.to_string();

        let parsed = AcpEvent::from_str(&line).unwrap();
        let AcpEvent::User(user) = parsed else {
            panic!("expected User event");
        };
        assert_eq!(user.prompt, "fix the bug");
        assert!(user.message_id.is_some());
    }

    #[test]
    fn user_event_object_without_message_id_parses() {
        let event = AcpEvent::from_str(r#"{"User":{"prompt":"hi"}}"#).unwrap();
        let AcpEvent::User(user) = event else {
            panic!("expected User event");
        };
        assert_eq!(user.prompt, "hi");
        assert_eq!(user.message_id, None);
    }
}
