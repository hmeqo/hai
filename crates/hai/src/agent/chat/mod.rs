mod attention;
mod chats;
mod conversation;
mod handle;
mod inbox;
mod pacing;
pub(crate) mod passes;
mod run;
mod scheduler;

pub use handle::ConversationAgentHandle;
use tokio::{sync::mpsc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) use self::conversation::Conversation;
use self::{
    chats::ChatSet,
    conversation::PendingWrite,
    inbox::Inbox,
    pacing::Pacing,
    passes::{DigestRequest, PassSlot},
};
use super::{
    process::AgentRuntime,
    run::{SelectedModel, engine::AgentEngine},
};
use crate::{
    agent::channel::ChannelRouter,
    config::conversation::Language,
    domain::vo::{ConversationId, ConversationSnapshot, ModelAlias},
    error::Result,
};

/// One Conversation's agent: the coordinator that owns the member Chats, the accumulation, the
/// pacing, and the one pass holding it Busy.
pub(super) struct ConversationAgent {
    engine: AgentEngine,
    /// The Conversation this agent owns; every member Chat is a surface under it.
    conversation_id: ConversationId,
    /// One entry per member Chat, its roster and the channel router that mounts a new surface.
    chats: ChatSet,
    /// The effective model label the next status query reports.
    model_label: String,
    pacing: Pacing,
    conversation: Conversation,
    /// The changes this Conversation has read and the policy they ask for.
    inbox: Inbox,
    pass: PassSlot,
    /// The digests people asked for, in the order they asked; the chat runs one when it is free.
    digest_rx: mpsc::UnboundedReceiver<DigestRequest>,
    /// A completed Turn whose snapshot could not be written; applied before the next run.
    pending_write: Option<PendingWrite>,
}

impl ConversationAgent {
    pub async fn new(
        runtime: AgentRuntime,
        conversation_id: ConversationId,
        router: ChannelRouter,
        shutdown: CancellationToken,
        digest_rx: mpsc::UnboundedReceiver<DigestRequest>,
    ) -> Result<Self> {
        let engine = AgentEngine::new(&runtime, shutdown.clone());
        let chats = ChatSet::mount(&runtime, conversation_id, router).await?;

        let record = runtime.services().conversation.get(conversation_id).await?;
        let conversation = match record {
            Some(r) => Conversation::from_snapshot(
                conversation_id,
                runtime.services().conversation.restore(&r)?,
                Some(r.updated_at),
            ),
            None => Conversation::new(conversation_id),
        };
        let inbox = Inbox::new(
            runtime
                .app
                .services
                .conversation
                .pending_entries(conversation_id)
                .await?,
        );
        let pacing = Pacing::new(&runtime.cfg().agent.attention);
        let stored_model = runtime.app.settings.model(conversation_id).await?;
        let resolved = runtime.resolve_model(
            stored_model
                .as_ref()
                .filter(|alias| runtime.model_exists(alias)),
        );
        let model_label = model_label(&resolved);
        Ok(Self {
            engine,
            conversation_id,
            chats,
            model_label,
            pacing,
            conversation,
            inbox,
            pass: PassSlot::Idle,
            digest_rx,
            pending_write: None,
        })
    }

    /// The model this Conversation's next pass runs on. A stored alias the catalog no longer has is
    /// cleared and the configured default is used.
    async fn selected_model(&mut self, stored: Option<ModelAlias>) -> Result<SelectedModel> {
        let settings = &self.engine.runtime.app.settings;
        if let Some(alias) = &stored
            && !self.engine.runtime.model_exists(alias)
        {
            tracing::warn!(
                %self.conversation_id,
                alias = %alias,
                "stored model alias is no longer configured; falling back to the default"
            );
            settings.reset_model(self.conversation_id).await?;
            let model = self.engine.runtime.resolve_model(None);
            self.model_label = model_label(&model);
            return Ok(model);
        }
        let model = self.engine.runtime.resolve_model(stored.as_ref());
        self.model_label = model_label(&model);
        Ok(model)
    }

    /// Writes one snapshot and consumes the batch in one statement; the caller applies the
    /// snapshot to memory only after this succeeds.
    async fn persist_conversation(
        &self,
        snap: &ConversationSnapshot,
        consumed: &[Uuid],
    ) -> Result<()> {
        self.engine
            .runtime
            .app
            .services
            .conversation
            .save(snap, self.conversation_id, consumed)
            .await
            .map_err(|e| {
                tracing::warn!(%self.conversation_id, "Failed to persist conversation: {e}");
                e
            })
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(
            self.engine
                .runtime
                .app
                .cfg
                .agent
                .context
                .session_idle_timeout_secs,
        )
    }

    /// The language the Conversation reads its persona and replies in.
    pub(crate) async fn language(&self) -> Result<Language> {
        self.engine
            .runtime
            .app
            .settings
            .language(self.conversation_id)
            .await
    }
}

fn model_label(model: &SelectedModel) -> String {
    model.name.clone().unwrap_or_else(|| {
        model
            .alias
            .as_ref()
            .map(|alias| format!("{alias} ({})", model.model))
            .unwrap_or_else(|| model.model.clone())
    })
}
#[cfg(test)]
mod model_tests {
    /// The status label prefers the configured display name over the raw model id.
    #[test]
    fn a_model_label_prefers_the_configured_name() {
        let model = super::SelectedModel {
            alias: Some(crate::domain::vo::ModelAlias::new("gemini")),
            name: Some("Gemini 3.1 Pro".into()),
            client: crate::agent::test_support::genai_client(),
            model: "google/gemini-3.1-pro".into(),
            chat: crate::config::schema::ChatConfig::default(),
            capabilities: Default::default(),
            max_tokens: None,
            reasoning_effort: None,
        };
        assert_eq!(super::model_label(&model), "Gemini 3.1 Pro");
    }
}
