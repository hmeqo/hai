use genai::chat::ChatMessage;
use serde_json::Value;

use crate::domain::vo::ContextMeta;

#[derive(Clone)]
pub struct ConversationSnapshot {
    /// Accumulated sequence handed to the LLM, injections and tool chains included.
    pub context_messages: Vec<ChatMessage>,
    pub context_meta: ContextMeta,
}

/// The two JSONB columns one conversation row is written from. Named because both are `Value`: a
/// positional pair compiles in either order and would put the messages where the chapter scalars go.
pub struct ChapterWrite {
    pub messages: Value,
    pub context_meta: Value,
}
