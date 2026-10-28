//! The member Chats of one Conversation: their surfaces, roster and the channel router each surface
//! reaches its platform through.

use std::{collections::HashMap, sync::Arc};

use crate::{
    agent::{
        channel::ChannelRouter,
        environment::{sandbox::Sandbox, workspace::Workspace},
        process::AgentRuntime,
        run::{ToolContext, engine},
    },
    agentcore::tool::AgentTool,
    domain::{
        model::{Chat, ChatRoster},
        vo::{ChatId, ConversationId},
    },
    error::{AppError, Result},
};

/// One Conversation's member Chats. The roster, each Chat's tool surface and the router that mounts
/// a new surface are one invariant: a Chat is in `chats` if and only if it is in `membership`.
pub(super) struct ChatSet {
    conversation_id: ConversationId,
    chats: HashMap<ChatId, ChatSurface>,
    membership: Membership,
    router: ChannelRouter,
    sandbox: Arc<Sandbox>,
}

/// One member Chat's execution surface: its channel handle and the tools built for that Chat.
pub(super) struct ChatSurface {
    roster: ChatRoster,
    tool: ToolContext,
    tools: Arc<[Arc<dyn AgentTool>]>,
}

impl ChatSurface {
    pub(super) fn roster(&self) -> &ChatRoster {
        &self.roster
    }

    pub(super) fn tool(&self) -> &ToolContext {
        &self.tool
    }

    pub(super) fn tools(&self) -> Arc<[Arc<dyn AgentTool>]> {
        self.tools.clone()
    }
}

impl ChatSet {
    /// Mounts every Chat the Conversation currently owns, or fails when it owns none. The workspace
    /// and sandbox belong to the same lifecycle and are built here.
    pub(super) async fn mount(
        runtime: &AgentRuntime,
        conversation_id: ConversationId,
        router: ChannelRouter,
    ) -> Result<Self> {
        let workspace = Workspace::create(conversation_id).await?;
        let cfg = &runtime.cfg().sandbox;
        let sandbox = Arc::new(if cfg.enabled {
            Sandbox::container(cfg, runtime.skill_manager.roots().to_vec(), workspace)
        } else {
            Sandbox::host(workspace, cfg.timeout_secs)
        });

        let rows = runtime
            .services()
            .platform
            .chats_in_conversation(conversation_id)
            .await?;
        if rows.is_empty() {
            return Err(AppError::message(format!(
                "Conversation {conversation_id} has no Chat"
            )));
        }

        let mut chats = HashMap::with_capacity(rows.len());
        for row in rows {
            let chat = mount_chat(runtime, conversation_id, &router, sandbox.clone(), row).await?;
            chats.insert(chat.roster.id, chat);
        }
        let membership =
            Membership::restored(chats.values().map(|chat| chat.roster.clone()).collect())?;

        Ok(Self {
            conversation_id,
            chats,
            membership,
            router,
            sandbox,
        })
    }

    /// Mounts Chats attached while this agent was live and drops Chats no longer in the
    /// Conversation; the next Turn re-renders the roster when membership changed.
    pub(super) async fn sync(&mut self, runtime: &AgentRuntime) -> Result<()> {
        let rows = runtime
            .services()
            .platform
            .chats_in_conversation(self.conversation_id)
            .await?;
        if rows.is_empty() {
            return Err(AppError::message(format!(
                "Conversation {} has no Chat",
                self.conversation_id
            )));
        }
        let ids: std::collections::HashSet<ChatId> = rows.iter().map(|row| row.id).collect();
        for row in &rows {
            if !self.chats.contains_key(&row.id) {
                let chat = mount_chat(
                    runtime,
                    self.conversation_id,
                    &self.router,
                    self.sandbox.clone(),
                    row.clone(),
                )
                .await?;
                self.chats.insert(row.id, chat);
            }
        }
        self.chats.retain(|id, _| ids.contains(id));
        self.membership.replace(
            self.chats
                .values()
                .map(|chat| chat.roster.clone())
                .collect(),
        )?;
        Ok(())
    }

    pub(super) fn surface(&self, chat_id: ChatId) -> Result<&ChatSurface> {
        self.chats.get(&chat_id).ok_or_else(|| {
            AppError::message(format!(
                "Chat {chat_id} is not a member of Conversation {}",
                self.conversation_id
            ))
        })
    }

    pub(super) fn origin(&self) -> ChatId {
        self.membership.origin()
    }

    pub(super) fn roster(&self) -> Arc<[ChatRoster]> {
        self.membership.roster()
    }

    pub(super) fn roster_ref(&self) -> &[ChatRoster] {
        self.membership.roster_ref()
    }

    pub(super) fn dirty(&self) -> bool {
        self.membership.dirty()
    }

    pub(super) fn clear_dirty(&mut self) {
        self.membership.clear_dirty();
    }

    pub(super) fn activate(&mut self, chat_id: ChatId) {
        self.membership.activate(chat_id);
    }

    pub(super) fn deactivate(&mut self) {
        self.membership.deactivate();
    }

    pub(super) fn remember(&mut self, chat_id: ChatId) {
        self.membership.remember(chat_id);
    }

    pub(super) fn contains(&self, chat_id: ChatId) -> bool {
        self.membership.contains(chat_id)
    }
}

async fn mount_chat(
    runtime: &AgentRuntime,
    conversation_id: ConversationId,
    router: &ChannelRouter,
    sandbox: Arc<Sandbox>,
    row: Chat,
) -> Result<ChatSurface> {
    let channel = runtime
        .services()
        .platform
        .platform_channel(row.channel_id)
        .await?;
    let handle = router
        .handle(&channel.channel_key, row.id)
        .await
        .ok_or_else(|| {
            AppError::message(format!(
                "channel {} is not running; Chat {} cannot mount its tools",
                channel.channel_key, row.id
            ))
        })?;
    let tool = ToolContext {
        chat: handle,
        conversation_id,
        router: router.clone(),
        services: runtime.services().clone(),
        sandbox,
        skill_manager: runtime.skill_manager.clone(),
        multimodal: runtime.multimodal().clone(),
    };
    let tools = engine::mounted_tools(runtime, &tool);
    Ok(ChatSurface {
        roster: row.roster(),
        tool,
        tools,
    })
}

/// The prompt/dispatch projection of the member Chats: sorted roster, the last Chat this agent used,
/// the origin of the Turn now running, and whether the roster changed after the chapter opened.
/// `ChatSurface` keeps each Chat's tool surface in `ChatSet::chats`.
struct Membership {
    roster: Arc<[ChatRoster]>,
    last: ChatId,
    active: Option<ChatId>,
    dirty: bool,
}

impl Membership {
    fn new(mut roster: Vec<ChatRoster>, dirty: bool) -> Result<Self> {
        roster.sort_by_key(|chat| chat.id);
        let first = roster
            .first()
            .map(|chat| chat.id)
            .ok_or_else(|| AppError::message("Conversation has no Chat"))?;
        Ok(Self {
            last: first,
            roster: roster.into(),
            active: None,
            dirty,
        })
    }

    /// A restored agent cannot know whether membership changed while it was gone, so its first
    /// roster is dirty.
    fn restored(roster: Vec<ChatRoster>) -> Result<Self> {
        Self::new(roster, true)
    }

    /// Replaces the roster when it changed; an unchanged roster leaves the dirty flag as it is.
    fn replace(&mut self, mut roster: Vec<ChatRoster>) -> Result<bool> {
        roster.sort_by_key(|chat| chat.id);
        let first = roster
            .first()
            .map(|chat| chat.id)
            .ok_or_else(|| AppError::message("Conversation has no Chat"))?;
        if roster.as_slice() == self.roster.as_ref() {
            return Ok(false);
        }
        self.roster = roster.into();
        self.dirty = true;
        if !self.contains(self.last) {
            self.last = first;
        }
        if self.active.is_some_and(|chat_id| !self.contains(chat_id)) {
            self.active = None;
        }
        Ok(true)
    }

    fn roster(&self) -> Arc<[ChatRoster]> {
        self.roster.clone()
    }

    fn roster_ref(&self) -> &[ChatRoster] {
        &self.roster
    }

    fn contains(&self, chat_id: ChatId) -> bool {
        self.roster.iter().any(|chat| chat.id == chat_id)
    }

    fn origin(&self) -> ChatId {
        self.active.unwrap_or(self.last)
    }

    #[cfg(test)]
    fn active(&self) -> Option<ChatId> {
        self.active
    }

    fn dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn activate(&mut self, chat_id: ChatId) {
        assert!(self.contains(chat_id), "the origin is always a member");
        self.active = Some(chat_id);
        self.last = chat_id;
    }

    fn deactivate(&mut self) {
        self.active = None;
    }

    fn remember(&mut self, chat_id: ChatId) {
        assert!(self.contains(chat_id), "the last Chat is always a member");
        self.last = chat_id;
    }
}

#[cfg(test)]
mod tests {
    use super::Membership;
    use crate::domain::{
        model::{ChatRoster, ChatType, Platform},
        vo::ChatId,
    };

    fn roster(id: i64) -> ChatRoster {
        ChatRoster {
            id: ChatId(id),
            alias: format!("chat-{id}"),
            platform: Platform::Telegram,
            chat_type: ChatType::Group,
            name: None,
        }
    }

    fn ids(membership: &Membership) -> Vec<ChatId> {
        membership.roster().iter().map(|chat| chat.id).collect()
    }

    #[test]
    fn replacing_membership_keeps_the_invariants() {
        let mut membership = Membership::new(vec![roster(2), roster(1)], true).unwrap();
        assert_eq!(ids(&membership), vec![ChatId(1), ChatId(2)]);
        assert_eq!(membership.origin(), ChatId(1));

        membership.activate(ChatId(2));
        assert_eq!(membership.origin(), ChatId(2));

        assert!(membership.replace(vec![roster(1)]).unwrap());
        assert_eq!(ids(&membership), vec![ChatId(1)]);
        assert_eq!(membership.origin(), ChatId(1));
        assert_eq!(membership.active(), None);
        assert!(membership.dirty());

        membership.clear_dirty();
        assert!(!membership.replace(vec![roster(1)]).unwrap());
        assert!(!membership.dirty());
    }

    #[test]
    fn an_empty_membership_is_rejected() {
        assert!(Membership::new(Vec::new(), true).is_err());
        let mut membership = Membership::new(vec![roster(1)], false).unwrap();
        assert!(membership.replace(Vec::new()).is_err());
        assert_eq!(membership.origin(), ChatId(1));
    }
}
