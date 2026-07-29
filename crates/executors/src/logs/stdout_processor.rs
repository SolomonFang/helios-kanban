//! Standard stdout log processor for executors that merge output into stdout
//! (e.g., when wrapped with `script` to provide a PTY).
//!
//! Mirrors `stderr_processor` but reads from stdout_chunked_stream instead.
//! PTY output contains \r\n line endings, so \r is stripped before emitting.

use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use workspace_utils::msg_store::MsgStore;

use super::{NormalizedEntry, NormalizedEntryType, plain_text_processor::PlainTextLogProcessor};
use crate::logs::utils::EntryIndexProvider;

pub fn normalize_stdout_logs(msg_store: Arc<MsgStore>, entry_index_provider: EntryIndexProvider) {
    tokio::spawn(async move {
        let mut stdout = msg_store.stdout_chunked_stream();

        let mut processor = PlainTextLogProcessor::builder()
            .normalized_entry_producer(Box::new(|content: String| NormalizedEntry {
                timestamp: None,
                entry_type: NormalizedEntryType::AssistantMessage,
                // PTY output via `script` uses \r\n; strip \r
                content: strip_ansi_escapes::strip_str(&content.replace('\r', "")),
                metadata: None,
            }))
            .time_gap(Duration::from_secs(2))
            .index_provider(entry_index_provider)
            .build();

        while let Some(Ok(chunk)) = stdout.next().await {
            for patch in processor.process(chunk) {
                msg_store.push_patch(patch);
            }
        }
    });
}
