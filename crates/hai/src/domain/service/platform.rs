use crate::{
    domain::{
        model::{
            Account, Chat, ChatLocation, ChatType, Message, NewChat, Platform, PlatformChannel,
            chat_address,
        },
        repo::{PlatformChange, Repos},
        vo::{
            AccountId, ChannelKey, ChatAddress, ChatId, ChatOrigin, ConversationId,
            PlatformChannelId,
        },
    },
    error::{AppError, Result},
};

#[derive(Debug)]
pub struct PlatformService {
    repos: Repos,
}

impl PlatformService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// One incoming platform change: the message row and its `ChatEntry` land together.
    pub async fn apply_platform_change(&self, change: PlatformChange) -> Result<Message> {
        self.repos.ingest.apply(change).await
    }

    /// One configured HAI channel, created on first use. A changed remote origin is refused rather
    /// than merged: the channel key is the local trust anchor.
    pub async fn ensure_platform_channel(
        &self,
        channel_key: &ChannelKey,
        platform: Platform,
        origin: Option<&ChatOrigin>,
    ) -> Result<PlatformChannel> {
        if let Some(channel) = self.repos.platform_channel.by_key(channel_key).await? {
            if channel.platform != platform {
                return Err(AppError::message(format!(
                    "platform channel {} is {}, not {}",
                    channel_key,
                    channel.platform.as_str(),
                    platform.as_str()
                )));
            }
            if let Some(incoming) = origin {
                match &channel.origin {
                    Some(stored) if stored != incoming => {
                        return Err(AppError::message(format!(
                            "platform channel {}: origin changed; use a new channel key",
                            channel_key
                        )));
                    }
                    None => {
                        return self
                            .repos
                            .platform_channel
                            .set_origin_if_null(channel.id, incoming)
                            .await;
                    }
                    Some(_) => {}
                }
            }
            return Ok(channel);
        }

        self.repos
            .platform_channel
            .create(channel_key, platform, origin)
            .await
    }

    pub async fn platform_channel(&self, id: PlatformChannelId) -> Result<PlatformChannel> {
        self.repos
            .platform_channel
            .by_id(id)
            .await?
            .ok_or_else(|| AppError::message(format!("platform channel {id} not found")))
    }

    pub async fn ensure_chat_and_account(
        &self,
        location: ChatLocation<'_>,
        chat_type: ChatType,
        chat_name: Option<&str>,
        external_user_id: &str,
        user_meta: Option<serde_json::Value>,
    ) -> Result<(Chat, Account)> {
        let channel = self
            .ensure_platform_channel(
                location.channel_key,
                location.platform,
                location.address.origin.as_ref(),
            )
            .await?;
        let chat = self
            .get_or_create_chat_in(location, &channel, chat_type, chat_name)
            .await?;
        let account = self
            .get_or_create_account(channel.id, external_user_id, user_meta)
            .await?;
        Ok((chat, account))
    }

    async fn get_or_create_account(
        &self,
        channel_id: PlatformChannelId,
        external_id: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<Account> {
        if let Some(account) = self
            .repos
            .account
            .by_channel_external_id(channel_id, external_id)
            .await?
        {
            if let Err(e) = self
                .repos
                .account
                .update_last_active_at(account.id, jiff::Timestamp::now())
                .await
            {
                tracing::warn!(?e, "failed to refresh account last_active_at");
            }
            return Ok(account);
        }
        let now = jiff::Timestamp::now();
        self.repos
            .account
            .create(channel_id, external_id, meta, now, now, now)
            .await
    }

    pub async fn get_or_create_chat(
        &self,
        location: ChatLocation<'_>,
        chat_type: ChatType,
        name: Option<&str>,
    ) -> Result<Chat> {
        let channel = self
            .ensure_platform_channel(
                location.channel_key,
                location.platform,
                location.address.origin.as_ref(),
            )
            .await?;
        self.get_or_create_chat_in(location, &channel, chat_type, name)
            .await
    }

    async fn get_or_create_chat_in(
        &self,
        location: ChatLocation<'_>,
        channel: &PlatformChannel,
        chat_type: ChatType,
        name: Option<&str>,
    ) -> Result<Chat> {
        if let Some(chat) = self
            .repos
            .chat
            .by_external_id(channel.id, &location.address.external_id)
            .await?
        {
            return Ok(chat);
        }
        let now = jiff::Timestamp::now();
        self.repos
            .chat
            .create(
                NewChat {
                    channel_id: channel.id,
                    external_id: location.address.external_id.clone(),
                    chat_type,
                    name: name.map(str::to_owned),
                    created_at: now,
                    updated_at: now,
                },
                location.scope,
            )
            .await
    }

    pub async fn get_chat_by_id(&self, id: ChatId) -> Result<Option<Chat>> {
        self.repos.chat.by_id(id).await
    }

    /// The remote address of one Chat: its channel's origin plus its own external id.
    pub async fn chat_address(&self, chat: &Chat) -> Result<ChatAddress> {
        let channel = self.platform_channel(chat.channel_id).await?;
        Ok(chat_address(
            channel.origin.as_ref(),
            chat.external_id.clone(),
        ))
    }

    pub async fn chats_in_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<Chat>> {
        self.repos.chat.by_conversation(conversation_id).await
    }

    pub async fn chats_by_ids(&self, ids: &[ChatId]) -> Result<Vec<Chat>> {
        self.repos.chat.by_ids(ids).await
    }

    pub async fn get_account_by_id(&self, id: AccountId) -> Result<Option<Account>> {
        self.repos.account.by_id(id).await
    }

    /// The account a channel speaks through. It lives on the configured channel, named `bot`; the
    /// channel key is already the unique column, so no prefix is needed here.
    pub async fn ensure_bot_account(
        &self,
        platform: Platform,
        key: &ChannelKey,
    ) -> Result<Account> {
        let channel = self.ensure_platform_channel(key, platform, None).await?;
        self.get_or_create_account(channel.id, "bot", None).await
    }
}
