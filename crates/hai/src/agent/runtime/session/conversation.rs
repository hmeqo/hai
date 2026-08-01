use std::collections::HashSet;

use uuid::Uuid;

use super::super::types::Messages;
use crate::domain::vo::{ConversationSnapshot, MessageId, Turn};

pub(crate) struct Conversation {
    messages: Messages,
    turns: Vec<Turn>,
    context_tokens: u32,
    since_id: MessageId,
    shown_memory_ids: HashSet<Uuid>,
    shown_topic_ids: HashSet<Uuid>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Messages::new(Vec::new()),
            turns: Vec::new(),
            context_tokens: 0,
            since_id: MessageId(0),
            shown_memory_ids: HashSet::new(),
            shown_topic_ids: HashSet::new(),
        }
    }

    pub fn from_snapshot(snap: ConversationSnapshot) -> Self {
        let context_tokens: u32 = snap.turns.iter().map(|t| t.prompt_tokens).sum();
        Self {
            messages: Messages::new(snap.messages),
            turns: snap.turns,
            context_tokens,
            since_id: snap.since_id,
            shown_memory_ids: snap.shown_memory_ids,
            shown_topic_ids: snap.shown_topic_ids,
        }
    }

    pub fn snapshot(&self) -> ConversationSnapshot {
        ConversationSnapshot {
            messages: self.messages.to_vec(),
            turns: self.turns.clone(),
            since_id: self.since_id,
            shown_memory_ids: self.shown_memory_ids.clone(),
            shown_topic_ids: self.shown_topic_ids.clone(),
        }
    }

    pub fn update(&mut self, turns: Vec<Turn>, messages: Messages) {
        self.context_tokens = turns.last().map(|t| t.prompt_tokens).unwrap_or(0);
        self.messages = messages;
        self.turns.extend(turns);
    }

    pub fn set_since_id(&mut self, id: MessageId) {
        self.since_id = id;
    }

    pub fn record_shown(&mut self, memory_ids: &[Uuid], topic_ids: &[Uuid]) {
        self.shown_memory_ids.extend(memory_ids.iter().copied());
        self.shown_topic_ids.extend(topic_ids.iter().copied());
    }

    pub fn build_full_messages(&self, chunk: Vec<genai::chat::ChatMessage>) -> Messages {
        let mut full = self.messages.clone();
        full.extend(chunk);
        full
    }

    pub fn since_id(&self) -> MessageId {
        self.since_id
    }

    pub fn is_fresh(&self) -> bool {
        self.since_id.0 == 0
    }

    pub fn messages_for_compact(&self) -> Messages {
        self.messages.clone()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn context_tokens(&self) -> u32 {
        self.context_tokens
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    pub fn shown_memory_ids(&self) -> &HashSet<Uuid> {
        &self.shown_memory_ids
    }

    pub fn shown_topic_ids(&self) -> &HashSet<Uuid> {
        &self.shown_topic_ids
    }

    /// 用 compact 开新章节。
    pub fn open_new_chapter(&mut self, compact: String) {
        self.messages = Messages::new(vec![genai::chat::ChatMessage::user(compact)]);
        self.turns.clear();
        self.context_tokens = 0;
        self.shown_memory_ids.clear();
        self.shown_topic_ids.clear();
    }
}

#[derive(Clone)]
pub(crate) struct RunInput {
    pub messages: Messages,
    pub message_ids: Vec<MessageId>,
    pub since_id: MessageId,
}
