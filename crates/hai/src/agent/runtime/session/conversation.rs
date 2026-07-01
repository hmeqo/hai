use std::collections::HashSet;

use genai::chat::ChatMessage;
use uuid::Uuid;

use super::super::{
    context::RunContext,
    types::{Run, RunPayload},
};
use crate::agent::context;

/// 持久化对话上下文。Ephemeral 模式下不存在（`None`）。
pub(super) struct Conversation {
    pub messages: Vec<ChatMessage>,
    pub runs: Vec<Run>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            runs: Vec::new(),
        }
    }

    pub fn push_run(&mut self, run: Run, messages: Vec<ChatMessage>) {
        self.messages = messages;
        self.runs.push(run);
    }

    pub fn last_run(&self) -> Option<&Run> {
        self.runs.last()
    }

    fn seen_ids(&self) -> (HashSet<Uuid>, HashSet<Uuid>) {
        let mems = self
            .runs
            .iter()
            .flat_map(|r| r.shown_memory_ids.iter().copied())
            .collect();
        let tops = self
            .runs
            .iter()
            .flat_map(|r| r.shown_topic_ids.iter().copied())
            .collect();
        (mems, tops)
    }

    pub fn runs_completed(&self) -> usize {
        self.runs.len()
    }

    pub async fn next_prompt(
        &self,
        ctx: &RunContext,
        messages: &[crate::domain::model::Message],
        next_since_id: i64,
    ) -> Option<RunPayload> {
        let built = if self.last_run().is_some() {
            let (mem, top) = self.seen_ids();
            context::build_next_run_prompt(ctx, messages, &mem, &top)
                .await
                .map_err(|e| tracing::error!(?e, "build_next_run_prompt failed"))
                .ok()?
        } else {
            context::build_first_run_prompt(ctx, messages)
                .await
                .map_err(|e| tracing::error!(?e, "build_first_run_prompt failed"))
                .ok()?
        };

        let full_messages = {
            let mut msgs = self.messages.clone();
            msgs.extend(built.messages);
            msgs
        };

        Some(RunPayload {
            messages: full_messages,
            prompt: built.rendered_prompt,
            message_ids: built.message_ids,
            since_id: next_since_id,
            shown_memory_ids: built.shown_memory_ids,
            shown_topic_ids: built.shown_topic_ids,
        })
    }
}
