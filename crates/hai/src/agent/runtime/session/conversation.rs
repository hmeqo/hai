use std::collections::HashSet;

use uuid::Uuid;

use super::super::{
    context::RunContext,
    types::{Messages, ProcessingOutput, Turn},
};
use crate::{
    agent::context,
    config::schema::ConversationMode,
    domain::{model::Message, vo::MessageId},
};

pub(super) struct Conversation {
    pub messages: Messages,
    pub since_id: MessageId,
    pub shown_memory_ids: HashSet<Uuid>,
    pub shown_topic_ids: HashSet<Uuid>,
    pub last_turns: Vec<Turn>,
    pub prompt_tokens: u32,
    pub mode: ConversationMode,
    turn_count: usize,
}

impl Conversation {
    pub fn new(mode: ConversationMode) -> Self {
        Self {
            messages: Messages::new(Vec::new()),
            since_id: MessageId(0),
            shown_memory_ids: HashSet::new(),
            shown_topic_ids: HashSet::new(),
            last_turns: Vec::new(),
            prompt_tokens: 0,
            mode,
            turn_count: 0,
        }
    }

    pub fn update(&mut self, output: &ProcessingOutput) {
        self.messages = output.messages.clone();
        self.last_turns = output.turns.clone();
        self.prompt_tokens = output.prompt_tokens;
        self.since_id = output.since_id;
        self.turn_count += 1;
    }

    pub fn turn_count(&self) -> usize {
        self.turn_count
    }

    pub async fn next_prompt(
        &mut self,
        ctx: &RunContext,
        messages: &[Message],
        next_since_id: MessageId,
    ) -> Option<super::prompt::ProcessingPayload> {
        let built = if self.turn_count > 0 {
            context::build_next_run_prompt(
                ctx,
                messages,
                &self.shown_memory_ids,
                &self.shown_topic_ids,
            )
            .await
            .map_err(|e| tracing::error!(?e, "build_next_run_prompt failed"))
            .ok()?
        } else {
            context::build_first_run_prompt(ctx, messages)
                .await
                .map_err(|e| tracing::error!(?e, "build_first_run_prompt failed"))
                .ok()?
        };

        self.shown_memory_ids
            .extend(built.shown_memory_ids.iter().copied());
        self.shown_topic_ids
            .extend(built.shown_topic_ids.iter().copied());

        let mut full = self.messages.clone();
        full.extend(built.messages);
        Some(super::prompt::ProcessingPayload {
            messages: full,
            prompt: built.rendered_prompt,
            message_ids: built.message_ids.iter().map(|id| MessageId(*id)).collect(),
            since_id: next_since_id,
        })
    }
}
