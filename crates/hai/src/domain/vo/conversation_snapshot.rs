use serde_json::Value;

use crate::domain::vo::{ContextMeta, StoredMessage};

#[derive(Clone, Debug)]
pub struct ConversationSnapshot {
    /// The provider-neutral chapter message log: text, tool calls, and media references.
    pub messages: Vec<StoredMessage>,
    pub context_meta: ContextMeta,
}

/// The two JSONB columns one conversation row is written from. Named because both are `Value`: a
/// positional pair compiles in either order and would put the messages where the chapter scalars go.
pub struct ChapterWrite {
    pub messages: Value,
    pub context_meta: Value,
}
