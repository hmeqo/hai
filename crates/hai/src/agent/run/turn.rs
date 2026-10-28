use std::sync::Arc;

use genai::{Client, chat::ReasoningEffort};

use crate::{
    agent::{
        channel::{ChannelHandle, ChannelRouter},
        environment::sandbox::Sandbox,
        events::WakeEvents,
        model::ModelCapabilities,
        multimodal::MultimodalService,
        process::AppContext,
        prompt::role::ChatOptionsSpec,
    },
    agentcore::skills::SkillManager,
    config::{
        conversation::{ContentPolicy, Language},
        schema::ChatConfig,
    },
    domain::{
        model::{ChatRoster, ChatType},
        service::Services,
        vo::{ChatId, ConversationId, HostAction, ModelAlias},
    },
};

/// The model one Turn runs on: a configured alias when the Conversation selected one, otherwise
/// `[model-roles.default]`.
#[derive(Clone)]
pub(crate) struct SelectedModel {
    pub alias: Option<ModelAlias>,
    pub name: Option<String>,
    pub client: Client,
    pub model: String,
    pub chat: ChatConfig,
    pub capabilities: ModelCapabilities,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl SelectedModel {
    pub(crate) fn chat_options(&self) -> ChatOptionsSpec<'_> {
        ChatOptionsSpec {
            chat: &self.chat,
            max_tokens: self.max_tokens,
            reasoning_effort: self.reasoning_effort.as_ref(),
        }
    }
}

pub struct TurnContext {
    pub tool: ToolContext,
    pub app: AppContext,
    pub chat_type: ChatType,
    /// The Chat this Turn belongs to; every tool and every send is bound to it.
    pub origin_chat: ChatId,
    /// Every member Chat of the Conversation, for the `<chats>` roster.
    pub chats: Arc<[ChatRoster]>,
    /// Whether membership changed after the chapter opened.
    pub roster_dirty: bool,
    /// Fixed for the whole turn.
    pub language: Language,
    pub persona: Option<String>,
    pub content_policy: ContentPolicy,
    /// The setting changes the host made since the Conversation's last Turn. Context only: they are
    /// reasons to read, never reasons to have started.
    pub host_actions: Vec<HostAction>,
    pub events: WakeEvents,
}

/// What one turn acts on and with: its own chat on the channel, and the services and sandbox behind
/// it. Domain reads address the chat by id (`chat.chat_id()`); anything that leaves through the
/// platform goes through the handle, which already knows the chat.
#[derive(Clone)]
pub struct ToolContext {
    pub chat: ChannelHandle,
    pub conversation_id: ConversationId,
    pub router: ChannelRouter,
    pub services: Services,
    pub sandbox: Arc<Sandbox>,
    pub skill_manager: SkillManager,
    pub multimodal: MultimodalService,
}
