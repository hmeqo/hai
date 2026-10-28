use std::collections::HashSet;

use super::{
    super::{ConversationAgent, conversation::PendingWrite},
    BusyRun,
};
use crate::{
    agent::{
        events::{Doorbell, WakeEvents},
        prompt::context,
        run::{TurnContext, TurnOutput, engine::TurnRun},
    },
    config::conversation::{ContentPolicy, Language},
    domain::{
        model::Message,
        vo::{
            AgentEvent, AgentEventPayload, BatchComposition, ChatId, HostAction, MessageId,
            SessionRef, StartedBy, TurnNumber,
        },
    },
    error::Result,
};

impl ConversationAgent {
    /// Starts a turn over the batch as it stands: the batch is the input, so a failure before the
    /// turn starts leaves it untouched for the next attempt.
    pub(crate) async fn dispatch(&mut self, doorbell: &Doorbell, started_by: StartedBy) {
        let Some(origin) = self.inbox.origin_for_turn(self.chats.roster_ref()) else {
            tracing::debug!(%self.conversation_id, "no origin chat in this batch");
            return;
        };
        self.chats.activate(origin);

        let settings = match self
            .engine
            .runtime
            .app
            .settings
            .snapshot(self.conversation_id)
            .await
        {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "settings lookup failed: {e}");
                return self.stand_down();
            }
        };
        let ctx = self.build_turn_context(
            origin,
            self.inbox.host_actions(),
            self.inbox.reasons(origin, self.chats.roster_ref()),
            settings.language,
            settings.persona.clone(),
            settings.content_policy,
        );
        let model = match self.selected_model(settings.model).await {
            Ok(model) => model,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "model selection failed: {e}");
                return self.stand_down();
            }
        };
        let tools = self
            .chats
            .surface(origin)
            .expect("the origin came from the chat map")
            .tools();

        let messages = match self.gather_batch(origin).await {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "gather_batch failed: {e}");
                return self.stand_down();
            }
        };

        let prompt_messages = messages.len();
        let is_first = self.conversation.is_first_render();
        let new_message_ids: HashSet<MessageId> =
            self.inbox.message_ids(origin).into_iter().collect();
        let built = match context::build_prompt(&ctx, &messages, is_first, &new_message_ids, origin)
            .await
        {
            Ok(built) => built,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "build_prompt failed: {e}");
                return self.stand_down();
            }
        };

        let messages = match self
            .conversation
            .build_full_messages(
                built.messages,
                &self.engine.runtime.app.services.images,
                model.capabilities.image_input,
            )
            .await
        {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "conversation render failed: {e}");
                return self.stand_down();
            }
        };
        let turn_number = TurnNumber::from((self.conversation.turn_count() + 1) as usize);

        // A batch that stands for no reason at all is one the idle loop's own pass reads (a purge):
        // a Turn reads changes, and every change a Turn reads is a reason.
        let Some(reason) = ctx.events.strongest() else {
            tracing::debug!(
                %self.conversation_id,
                "no wake reason in this batch; the idle loop's own pass reads it",
            );
            return;
        };

        let composition = BatchComposition::of(
            &self
                .inbox
                .entries()
                .iter()
                .filter(|entry| entry.chat_id == origin)
                .cloned()
                .collect::<Vec<_>>(),
        );
        let session = SessionRef::main(self.conversation.session());

        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id: origin,
            session: Some(session),
            payload: AgentEventPayload::TurnStarted {
                turn: turn_number,
                reason,
                prompt_messages,
                full_prompt: built.rendered_prompt.clone(),
                started_by,
                composition,
            },
        });

        let result_rx = self.engine.spawn_turn(
            ctx,
            tools,
            TurnRun {
                messages,
                doorbell: doorbell.clone(),
                steering_since: doorbell.version(origin),
                turn_number,
                session,
                model: &model,
            },
        );

        self.pass.begin(BusyRun::turn(result_rx));
    }

    /// Commits the turn: the changes it consumed leave the accumulation in the same step as the
    /// snapshot, so a turn that never reaches here leaves both untouched.
    pub(crate) async fn on_complete(&mut self, output: TurnOutput) {
        if output.has_spoken {
            self.pacing.on_spoke();
        }

        let last_prompt_tokens = output.steps.last().map(|t| t.prompt_tokens).unwrap_or(0);
        let origin = self.chats.origin();
        let consumed = self.inbox.consumed_for_turn(origin);
        let snapshot = match self
            .conversation
            .next_snapshot(
                output.messages,
                output.steps.len(),
                last_prompt_tokens,
                &self.engine.runtime.app.services.images,
            )
            .await
        {
            Ok(snapshot) => snapshot,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "completed Turn not stored: {e}");
                self.stand_down();
                return;
            }
        };
        if let Err(e) = self.persist_conversation(&snapshot, &consumed).await {
            tracing::warn!(%self.conversation_id, "completed Turn not persisted: {e}");
            self.pending_write = Some(PendingWrite { snapshot, consumed });
            self.stand_down();
            return;
        }

        self.conversation.apply_snapshot(snapshot);
        self.inbox.consume(&consumed);
        self.chats.clear_dirty();
        self.pacing.reopen();
    }

    pub(crate) fn build_turn_context(
        &self,
        chat_id: ChatId,
        host_actions: Vec<HostAction>,
        events: WakeEvents,
        language: Language,
        persona: Option<String>,
        content_policy: ContentPolicy,
    ) -> TurnContext {
        let chat = self
            .chats
            .surface(chat_id)
            .expect("a turn context is built for a member chat");
        TurnContext {
            tool: chat.tool().clone(),
            app: self.engine.runtime.app.clone(),
            chat_type: chat.roster().chat_type,
            origin_chat: chat_id,
            chats: self.chats.roster(),
            roster_dirty: self.chats.dirty(),
            language,
            persona,
            content_policy,
            host_actions,
            events,
        }
    }

    async fn gather_batch(&self, chat_id: ChatId) -> Result<Vec<Message>> {
        let services = self.engine.runtime.services();

        let mut ids: Vec<MessageId> = self.inbox.message_ids(chat_id);
        ids.sort_unstable();
        ids.dedup();

        let mut messages = services.message.get_messages_by_ids(&ids).await?;

        if self.conversation.is_first_render() {
            let cap = self.engine.runtime.cfg().agent.context.context_seed_cap;
            messages.extend(
                services
                    .message
                    .recent_for_conversation(self.conversation_id, cap)
                    .await?,
            );
            messages.sort_by_key(|message| message.id);
            messages.dedup_by_key(|message| message.id);
        }

        Ok(messages)
    }
}
