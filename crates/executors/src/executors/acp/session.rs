use std::{
    fs::{self, OpenOptions},
    io::{self, Result, Write},
    path::PathBuf,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::executors::acp::AcpEvent;

/// Manages session persistence and state for ACP interactions
pub struct SessionManager {
    base_dir: PathBuf,
}

impl SessionManager {
    /// Create a new session manager with the given namespace
    pub fn new(namespace: impl Into<String>) -> Result<Self> {
        let namespace = namespace.into();
        let mut vk_dir = dirs::home_dir()
            .ok_or_else(|| io::Error::other("Could not determine home directory"))?
            .join(".vibe-kanban");

        if cfg!(debug_assertions) {
            vk_dir = vk_dir.join("dev");
        }

        let base_dir = vk_dir.join(&namespace);

        fs::create_dir_all(&base_dir)?;

        Ok(Self { base_dir })
    }

    /// Get the file path for a session
    fn session_file_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{session_id}.jsonl"))
    }

    /// Append a raw JSON line to the session log
    ///
    /// We normalize ACP payloads by:
    /// - Removing top-level `sessionId`
    /// - Unwrapping the `update` envelope (store its object directly)
    /// - Dropping top-level `options` (permission menu). Note: `options` is
    ///   mutually exclusive with `update`, so when `update` is present we do not
    ///   perform any `options` stripping.
    pub fn append_raw_line(&self, session_id: &str, raw_json: &str) -> Result<()> {
        let Some(normalized) = Self::normalize_session_event(raw_json) else {
            return Ok(());
        };

        let path = self.session_file_path(session_id);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;

        writeln!(file, "{normalized}")?;
        Ok(())
    }

    /// Attempt to normalize a raw ACP JSON event into a cleaner shape.
    /// Rules:
    /// - Remove top-level `sessionId` always.
    /// - If `update` is present with an object that has `sessionUpdate`, emit
    ///   a single-key object where key = camelCase(sessionUpdate) and value =
    ///   the `update` object minus `sessionUpdate`.
    /// - If `update` is absent, remove only top-level `options`.
    ///
    /// Returns None if the input is not a JSON object.
    fn normalize_session_event(raw_json: &str) -> Option<String> {
        let mut event = AcpEvent::from_str(raw_json).ok()?;

        match event {
            AcpEvent::SessionStart(..)
            | AcpEvent::Error(..)
            | AcpEvent::Done(..)
            | AcpEvent::Other(..) => return None,

            AcpEvent::User(..)
            | AcpEvent::Message(..)
            | AcpEvent::Thought(..)
            | AcpEvent::ToolCall(..)
            | AcpEvent::ToolUpdate(..)
            | AcpEvent::Plan(..)
            | AcpEvent::AvailableCommands(..)
            | AcpEvent::ApprovalResponse(..)
            | AcpEvent::QuestionResponse(..)
            | AcpEvent::Elicitation { .. }
            | AcpEvent::CurrentMode(..) => {}

            AcpEvent::RequestPermission(req, ..) => event = AcpEvent::ToolUpdate(req.tool_call),
        }

        match event {
            AcpEvent::User(user_prompt) => {
                let mut normalized = serde_json::json!({ "user": user_prompt.prompt });
                if let Some(message_id) = user_prompt.message_id {
                    normalized["message_id"] = serde_json::Value::String(message_id);
                }
                return serde_json::to_string(&normalized).ok();
            }
            AcpEvent::Message(ref content) | AcpEvent::Thought(ref content) => {
                if let agent_client_protocol::ContentBlock::Text(text) = content {
                    // Special simplification for pure text messages
                    let key = if let AcpEvent::Message(_) = event {
                        "assistant"
                    } else {
                        "thinking"
                    };
                    return serde_json::to_string(&serde_json::json!({ key: text.text })).ok();
                }
            }
            _ => {}
        }

        serde_json::to_string(&event).ok()
    }

    /// Read the raw JSONL content of a session
    pub fn read_session_raw(&self, session_id: &str) -> Result<String> {
        let path = self.session_file_path(session_id);
        if !path.exists() {
            return Ok(String::new());
        }

        fs::read_to_string(path)
    }

    /// Fork a session to create a new one with the same history.
    ///
    /// When `reset_to_message_id` is given, only the events *before* the user
    /// message carrying that id are copied: the edited message and everything
    /// after it is dropped. An unknown id degrades to a full copy.
    pub fn fork_session(
        &self,
        old_id: &str,
        new_id: &str,
        reset_to_message_id: Option<&str>,
    ) -> Result<()> {
        let old_path = self.session_file_path(old_id);
        let new_path = self.session_file_path(new_id);

        if !old_path.exists() {
            // Create empty new file if old doesn't exist
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&new_path)?;
            return Ok(());
        }

        let Some(target) = reset_to_message_id else {
            fs::copy(&old_path, &new_path)?;
            return Ok(());
        };

        let content = fs::read_to_string(&old_path)?;
        let mut kept: Vec<&str> = Vec::new();
        let mut found = false;
        for line in content.lines() {
            if Self::is_user_message_with_id(line, target) {
                found = true;
                break;
            }
            kept.push(line);
        }

        if !found {
            tracing::warn!(
                "reset_to_message_id {target} not found in ACP session {old_id}; forking full history"
            );
            fs::copy(&old_path, &new_path)?;
            return Ok(());
        }

        let mut truncated = kept.join("\n");
        if !truncated.is_empty() {
            truncated.push('\n');
        }
        fs::write(&new_path, truncated)?;
        Ok(())
    }

    fn is_user_message_with_id(line: &str, message_id: &str) -> bool {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        value.get("user").is_some()
            && value.get("message_id").and_then(|v| v.as_str()) == Some(message_id)
    }

    /// Delete a session
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = self.session_file_path(session_id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Generate a resume prompt from session history
    pub fn generate_resume_prompt(&self, session_id: &str, current_prompt: &str) -> Result<String> {
        let session_context = self.read_session_raw(session_id)?;

        Ok(format!(
            concat!(
                "RESUME CONTEXT FOR CONTINUING TASK\n\n",
                "=== EXECUTION HISTORY ===\n",
                "The following is the conversation history from this session:\n",
                "{}\n\n",
                "=== CURRENT REQUEST ===\n",
                "{}\n\n",
                "=== INSTRUCTIONS ===\n",
                "You are continuing work on the above task. The execution history shows ",
                "the previous conversation in this session. Please continue from where ",
                "the previous execution left off, taking into account all the context provided above."
            ),
            session_context, current_prompt
        ))
    }
}

/// Session metadata stored separately from events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub session_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub parent_session: Option<String>,
    pub tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session_manager() -> (SessionManager, PathBuf) {
        let base_dir =
            std::env::temp_dir().join(format!("acp-session-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base_dir).unwrap();
        let manager = SessionManager {
            base_dir: base_dir.clone(),
        };
        (manager, base_dir)
    }

    fn write_session(manager: &SessionManager, session_id: &str, lines: &[&str]) {
        let mut content = lines.join("\n");
        content.push('\n');
        fs::write(manager.session_file_path(session_id), content).unwrap();
    }

    #[test]
    fn fork_session_without_reset_copies_full_history() {
        let (manager, dir) = temp_session_manager();
        let lines = [
            r#"{"user":"first","message_id":"id-1"}"#,
            r#"{"assistant":"reply one"}"#,
            r#"{"user":"second","message_id":"id-2"}"#,
        ];
        write_session(&manager, "old", &lines);

        manager.fork_session("old", "new", None).unwrap();

        let forked = manager.read_session_raw("new").unwrap();
        assert_eq!(forked, lines.join("\n") + "\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fork_session_truncates_before_target_message() {
        let (manager, dir) = temp_session_manager();
        write_session(
            &manager,
            "old",
            &[
                r#"{"user":"first","message_id":"id-1"}"#,
                r#"{"assistant":"reply one"}"#,
                r#"{"user":"second","message_id":"id-2"}"#,
                r#"{"assistant":"reply two"}"#,
                r#"{"user":"third","message_id":"id-3"}"#,
            ],
        );

        manager.fork_session("old", "new", Some("id-2")).unwrap();

        let forked = manager.read_session_raw("new").unwrap();
        assert_eq!(
            forked,
            "{\"user\":\"first\",\"message_id\":\"id-1\"}\n{\"assistant\":\"reply one\"}\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fork_session_truncates_to_empty_when_target_is_first() {
        let (manager, dir) = temp_session_manager();
        write_session(
            &manager,
            "old",
            &[
                r#"{"user":"first","message_id":"id-1"}"#,
                r#"{"assistant":"reply one"}"#,
            ],
        );

        manager.fork_session("old", "new", Some("id-1")).unwrap();

        assert_eq!(manager.read_session_raw("new").unwrap(), "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fork_session_unknown_id_falls_back_to_full_copy() {
        let (manager, dir) = temp_session_manager();
        let lines = [
            r#"{"user":"first","message_id":"id-1"}"#,
            r#"{"assistant":"reply one"}"#,
        ];
        write_session(&manager, "old", &lines);

        manager
            .fork_session("old", "new", Some("missing-id"))
            .unwrap();

        let forked = manager.read_session_raw("new").unwrap();
        assert_eq!(forked, lines.join("\n") + "\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fork_session_legacy_user_lines_do_not_match() {
        let (manager, dir) = temp_session_manager();
        let lines = [r#"{"user":"first"}"#, r#"{"assistant":"reply one"}"#];
        write_session(&manager, "old", &lines);

        manager.fork_session("old", "new", Some("id-1")).unwrap();

        let forked = manager.read_session_raw("new").unwrap();
        assert_eq!(forked, lines.join("\n") + "\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fork_session_missing_source_creates_empty_file() {
        let (manager, dir) = temp_session_manager();

        manager.fork_session("nope", "new", Some("id-1")).unwrap();

        assert_eq!(manager.read_session_raw("new").unwrap(), "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_raw_line_persists_user_message_id() {
        let (manager, dir) = temp_session_manager();
        let event = AcpEvent::User(crate::executors::acp::UserPrompt::new("hello"));
        let AcpEvent::User(ref user) = event else {
            panic!("expected User event");
        };
        let message_id = user.message_id.clone().unwrap();

        manager.append_raw_line("s", &event.to_string()).unwrap();

        let content = manager.read_session_raw("s").unwrap();
        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["user"], "hello");
        assert_eq!(value["message_id"], message_id);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_raw_line_legacy_user_event_has_no_message_id() {
        let (manager, dir) = temp_session_manager();

        manager.append_raw_line("s", r#"{"User":"hello"}"#).unwrap();

        let content = manager.read_session_raw("s").unwrap();
        let value: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(value["user"], "hello");
        assert!(value.get("message_id").is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
