use crate::{
    agent::{process::registry::AgentRegistry, settings::ConversationSettings},
    domain::{
        service::Services,
        vo::{ChatId, ConversationId, HostAction},
    },
    error::Result,
};

/// What a command needs from the host: the Chat it arrived in, the Conversation it belongs to, the
/// settings it may read or change, the services it may write through, and the agent it may ring.
pub(crate) struct CommandContext {
    pub(crate) chat_id: ChatId,
    pub(crate) conversation_id: ConversationId,
    pub(crate) settings: ConversationSettings,
    pub(crate) agents: AgentRegistry,
    pub(crate) services: Services,
}

impl CommandContext {
    /// Records a host change once, for both the command line and a dialog page. The setting itself is
    /// already committed; an append failure is logged rather than turned into a failed command.
    pub(crate) async fn record_action(&self, action: HostAction) -> Result<()> {
        self.services
            .conversation
            .record_host_action(self.conversation_id, self.chat_id, action)
            .await
    }
}
