use crate::{
    domain::{
        model::{Account, Chat, ChatType, Platform},
        repo::Repos,
        vo::{AccountId, ChatId, IdentityId},
    },
    error::Result,
};

#[derive(Debug)]
pub struct PlatformService {
    repos: Repos,
}

impl PlatformService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    pub async fn ensure_chat_and_account(
        &self,
        platform: Platform,
        external_chat_id: &str,
        chat_type: ChatType,
        chat_name: Option<&str>,
        external_user_id: &str,
        user_meta: Option<serde_json::Value>,
    ) -> Result<(Chat, Account)> {
        let chat = self
            .get_or_create_chat(platform, external_chat_id, chat_type, chat_name, None)
            .await?;
        let account = self
            .get_or_create_account(platform, external_user_id, user_meta)
            .await?;
        Ok((chat, account))
    }

    pub async fn get_or_create_account(
        &self,
        platform: Platform,
        external_id: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<Account> {
        let platform_str: &str = platform.into();
        if let Some(account) = self
            .repos
            .account
            .by_platform_external_id(platform_str, external_id)
            .await?
        {
            // 活动刷新（消息路径每次活动更新——last_active_at 语义即最后活动）
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
            .create(platform_str, external_id, meta, now, now, now)
            .await
    }

    pub async fn get_or_create_chat(
        &self,
        platform: Platform,
        external_id: &str,
        chat_type: ChatType,
        name: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> Result<Chat> {
        let platform_str: &str = platform.into();
        let chat_type_str: &str = chat_type.into();
        if let Some(chat) = self
            .repos
            .chat
            .by_platform_external_id(platform_str, external_id)
            .await?
        {
            return Ok(chat);
        }
        let now = jiff::Timestamp::now();
        self.repos
            .chat
            .create(
                platform_str,
                external_id,
                chat_type_str,
                name,
                meta,
                now,
                now,
            )
            .await
    }

    pub async fn get_chat_by_id(&self, id: ChatId) -> Result<Option<Chat>> {
        match self.repos.chat.by_id(id.0).await {
            Ok(chat) => Ok(chat),
            Err(e) => {
                tracing::warn!(chat_id = %id, "get_chat_by_id failed: {e}");
                Ok(None)
            }
        }
    }

    pub async fn get_account_by_id(&self, id: AccountId) -> Result<Option<Account>> {
        match self.repos.account.by_id(id.0).await {
            Ok(account) => Ok(account),
            Err(e) => {
                tracing::warn!(account_id = %id, "get_account_by_id failed: {e}");
                Ok(None)
            }
        }
    }

    pub async fn get_identity_accounts(&self, identity_id: IdentityId) -> Result<Vec<Account>> {
        self.repos.account.by_identity_id(identity_id.0).await
    }

    pub async fn ensure_bot_account(&self) -> Result<Account> {
        self.get_or_create_account(Platform::System, "bot", None)
            .await
    }
}
