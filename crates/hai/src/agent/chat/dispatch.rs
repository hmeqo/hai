use std::collections::HashSet;

use tokio::time::Instant;

use super::{BusyPass, ChatAgent, ChatAgentState};
use crate::{
    agent::{
        events::{Doorbell, WakeEvent, WakeEvents, WakeReason},
        prompt::context,
        run::{TurnContext, TurnOutput},
    },
    agentcore::Language,
    domain::{
        model::Message,
        vo::{
            AgentEvent, AgentEventPayload, BatchComposition, MessageId, SessionRef, StartedBy,
            TurnNumber,
        },
    },
    error::Result,
};

impl ChatAgent {
    /// Starts a turn over the batch as it stands: the batch is the input, so a failure before the
    /// turn starts leaves it untouched for the next attempt.
    pub async fn dispatch(&mut self, doorbell: &Doorbell, started_by: StartedBy) {
        // The batch was read before this call; an arrival after that read belongs to the next Turn,
        // and the doorbell keeps a permit for it.
        doorbell.take_signal();

        let language = match self.language().await {
            Ok(language) => language,
            Err(e) => {
                tracing::warn!(%self.chat_id, "language lookup failed: {e}");
                return self.stand_down();
            }
        };
        let ctx = self.build_turn_context(self.batch_reasons(), language);

        let messages = match self.gather_batch().await {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(%self.chat_id, "gather_batch failed: {e}");
                return self.stand_down();
            }
        };

        let prompt_messages = messages.len();
        let is_first = self.conversation.is_first_render();
        let new_message_ids: HashSet<MessageId> = self
            .batch
            .iter()
            .filter_map(|entry| entry.message_id())
            .collect();
        let built = match context::build_prompt(&ctx, &messages, is_first, &new_message_ids).await {
            Ok(built) => built,
            Err(e) => {
                tracing::warn!(%self.chat_id, "build_prompt failed: {e}");
                return self.stand_down();
            }
        };

        let messages = self.conversation.build_full_messages(built.messages);
        let turn_number = TurnNumber::from((self.conversation.turn_count() + 1) as usize);

        // A batch that stands for no reason at all is one the idle loop's own pass reads (a purge):
        // a Turn reads changes, and every change a Turn reads is a reason.
        let Some(reason) = ctx.events.strongest() else {
            tracing::debug!(
                %self.chat_id,
                "no wake reason in this batch; the idle loop's own pass reads it",
            );
            return;
        };

        let composition = BatchComposition::of(&self.batch);
        let session = SessionRef::main(self.conversation.session());

        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id: self.chat_id,
            session: Some(session),
            payload: AgentEventPayload::TurnStarted {
                turn: turn_number,
                reason,
                prompt_messages,
                full_prompt: built.rendered_prompt.clone(),
                started_by: Some(started_by),
                composition: Some(composition),
            },
        });

        let result_rx = self.engine.spawn_turn(
            ctx,
            self.tools.clone(),
            messages,
            doorbell.clone(),
            turn_number,
            session,
        );

        self.state = ChatAgentState::busy(result_rx, Instant::now(), BusyPass::Turn);
    }

    /// Commits the turn: the changes it consumed leave the accumulation in the same step as the
    /// snapshot, so a turn that never reaches here leaves both untouched.
    pub async fn on_complete(&mut self, output: TurnOutput) {
        if output.has_spoken {
            self.schedule.on_spoke();
        }

        let last_prompt_tokens = output.steps.last().map(|t| t.prompt_tokens).unwrap_or(0);
        self.conversation
            .update(output.messages, output.steps.len(), last_prompt_tokens);

        let consumed = super::consumed_entries(&self.batch);
        if self.persist_conversation(&consumed).await.is_ok() {
            self.batch.clear();
        }

        self.retry.reopen();
        self.end_run();
    }

    fn batch_reasons(&self) -> WakeEvents {
        WakeEvents::new(
            self.batch
                .iter()
                .filter_map(|entry| WakeReason::of_entry(entry, self.chat_type).map(WakeEvent::new))
                .collect(),
        )
    }

    pub fn build_turn_context(&self, events: WakeEvents, language: Language) -> TurnContext {
        TurnContext {
            tool: self.tool.clone(),
            app: self.engine.runtime.app.clone(),
            chat_type: self.chat_type,
            language,
            events,
        }
    }

    /// The language the chat reads its persona and replies in; a failure is the caller's to handle.
    pub(crate) async fn language(&self) -> Result<Language> {
        self.engine
            .runtime
            .app
            .settings
            .language(self.chat_id)
            .await
    }

    pub async fn gather_batch(&self) -> Result<Vec<Message>> {
        let services = self.engine.runtime.services();

        let mut ids: Vec<MessageId> = self
            .batch
            .iter()
            .filter_map(|entry| entry.message_id())
            .collect();
        ids.sort_unstable();
        ids.dedup();

        let mut messages = services.message.get_messages_by_ids(&ids).await?;

        if self.conversation.is_first_render() {
            let cap = self.engine.runtime.cfg().agent.context.context_seed_cap;
            messages.extend(services.message.recent_asc(self.chat_id, cap).await?);
            messages.sort_by_key(|message| message.id);
            messages.dedup_by_key(|message| message.id);
        }

        Ok(messages)
    }
}
