//! The chapter wrap-up pass: its start, its success/failure completion, and the chapter images it
//! releases.

use uuid::Uuid;

use super::{super::ConversationAgent, BusyRun};
use crate::{
    agent::events::{Doorbell, WakeEvents},
    domain::vo::{AgentEvent, AgentEventPayload, SessionRef},
    error::Result,
};

impl ConversationAgent {
    pub(crate) fn should_wrap_up_by_tokens(&self) -> bool {
        let threshold = self
            .engine
            .runtime
            .app
            .cfg
            .agent
            .context
            .compact_token_threshold;
        threshold > 0 && self.conversation.context_tokens() >= threshold
    }

    pub(crate) async fn start_wrap_up(&mut self, doorbell: &Doorbell) {
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
                self.pacing.defer_wrap_up();
                return;
            }
        };
        self.pacing.clear_wrap_up();
        let consumed = self.inbox.policy_ids();
        let chat_id = self.chats.origin();
        let ctx = self.build_turn_context(
            chat_id,
            Vec::new(),
            WakeEvents::new(vec![]),
            settings.language,
            None,
            settings.content_policy,
        );
        let model = match self.selected_model(settings.model).await {
            Ok(model) => model,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "model selection failed: {e}");
                self.pacing.defer_wrap_up();
                return;
            }
        };
        let tools = self
            .chats
            .surface(chat_id)
            .expect("the chat was just resolved")
            .tools();
        let messages = match self
            .conversation
            .messages_for_wrap_up(
                &self.engine.runtime.app.services.images,
                model.capabilities.image_input,
            )
            .await
        {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "wrap-up render failed: {e}");
                self.pacing.defer_wrap_up();
                return;
            }
        };
        self.chats.deactivate();
        let result_rx = self.engine.spawn_wrap_up(
            ctx,
            tools,
            messages,
            doorbell.clone(),
            self.conversation.session(),
            &model,
        );
        self.pass.begin(BusyRun::wrap_up(result_rx, consumed));
    }

    /// A closed chapter keeps no images: the next chapter starts from a summary and holds no image
    /// part, so every file under the Conversation is unreferenced.
    async fn clear_chapter_images(&self) {
        if let Err(e) = self
            .engine
            .runtime
            .app
            .services
            .images
            .clear(self.conversation_id)
            .await
        {
            tracing::warn!(%self.conversation_id, "chapter image clear failed: {e}");
        }
    }

    pub(crate) async fn on_wrap_up_done(
        &mut self,
        summary: String,
        context_tokens: u32,
        session: SessionRef,
        consumed: Vec<Uuid>,
    ) -> Result<()> {
        let wrapped_up_steps = self.conversation.step_count();
        let snapshot = self
            .conversation
            .next_chapter_snapshot(Some(summary.clone()));
        self.persist_conversation(&snapshot, &consumed).await?;
        self.conversation.apply_snapshot(snapshot);
        self.inbox.clear_policy();
        self.inbox.consume(&consumed);
        self.clear_chapter_images().await;
        let chat_id = self.chats.origin();
        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id,
            session: Some(session),
            payload: AgentEventPayload::WrapUpCompleted {
                step_count: wrapped_up_steps as usize,
                context_tokens,
                summary,
            },
        });
        Ok(())
    }

    pub(crate) async fn on_wrap_up_failed(
        &mut self,
        consumed: Vec<Uuid>,
        error: String,
        session: SessionRef,
    ) {
        let chat_id = self.chats.origin();
        self.engine.runtime.app.event_bus.emit(AgentEvent {
            chat_id,
            session: Some(session),
            payload: AgentEventPayload::WrapUpFailed { error },
        });
        let snapshot = self.conversation.next_chapter_snapshot(None);
        if self
            .persist_conversation(&snapshot, &consumed)
            .await
            .is_ok()
        {
            self.conversation.apply_snapshot(snapshot);
            self.inbox.clear_policy();
            self.inbox.consume(&consumed);
            self.clear_chapter_images().await;
        } else {
            self.stand_down();
        }
    }
}
