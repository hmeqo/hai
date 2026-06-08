use super::{
    ctx::RoundCtx,
    round::{RoundTask, RoundTaskPayload},
};
use crate::{
    agent::{
        AgentEngine, context,
        round::{Round, RoundResult},
    },
    domain::entity::Message,
};

pub(super) struct TaskSession {
    pub(super) rounds: Vec<Round>,
    pub(super) round_task: Option<RoundTask>,
}

impl TaskSession {
    pub fn new() -> Self {
        Self {
            rounds: Vec::new(),
            round_task: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }

    pub fn is_task_active(&self) -> bool {
        self.round_task.is_some()
    }

    pub fn full_prompt(&self) -> String {
        self.rounds
            .iter()
            .map(|r| r.segment.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn push(&mut self, round: Round) {
        self.rounds.push(round);
    }

    pub fn clear_task(&mut self) {
        self.round_task = None;
    }

    pub async fn spawn(
        &mut self,
        engine: AgentEngine,
        ctx: RoundCtx,
        messages: Vec<Message>,
        on_complete: impl FnOnce(RoundResult) + Send + 'static,
    ) {
        let last = self.rounds.last().cloned();

        let built = if last.is_some() {
            context::build_next_round_prompt(&ctx, &messages, last.as_ref())
                .await
                .ok()
        } else {
            context::build_first_round_prompt(&ctx, &messages)
                .await
                .ok()
        };

        let Some(built) = built else {
            return;
        };

        let segment = built.rendered_prompt;
        let prompt = if !self.rounds.is_empty() {
            format!("{}\n{segment}", self.full_prompt())
        } else {
            segment.clone()
        };

        self.round_task = Some(RoundTask::spawn(
            engine,
            ctx,
            RoundTaskPayload {
                prompt,
                segment,
                message_ids: built.message_ids,
            },
            on_complete,
        ));
    }
}
