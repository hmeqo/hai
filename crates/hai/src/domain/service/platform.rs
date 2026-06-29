use crate::{
    domain::{
        model::{Account, Chat, ChatType, Platform},
        vo::{AccountId, ChatId, IdentityId},
    },
    error::Result,
};

#[derive(Debug)]
pub struct PlatformService {
    db: toasty::Db,
}

impl PlatformService {
    pub fn new(db: toasty::Db) -> Self {
        Self { db }
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
        if let Some(account) = Account::filter(
            Account::fields()
                .platform()
                .eq(platform_str)
                .and(Account::fields().external_id().eq(external_id)),
        )
        .first()
        .exec(&mut self.db.clone())
        .await?
        {
            return Ok(account);
        }
        let now = jiff::Timestamp::now();
        toasty::create!(Account {
            platform: platform_str,
            external_id,
            meta: meta.map(toasty::Json),
            last_active_at: now,
            created_at: now,
            updated_at: now,
        })
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
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
        if let Some(chat) = Chat::filter(
            Chat::fields()
                .platform()
                .eq(platform_str)
                .and(Chat::fields().external_id().eq(external_id)),
        )
        .first()
        .exec(&mut self.db.clone())
        .await?
        {
            return Ok(chat);
        }
        let now = jiff::Timestamp::now();
        toasty::create!(Chat {
            platform: platform_str,
            external_id,
            chat_type: chat_type_str,
            name: name.map(|s| s.to_string()),
            config: None::<toasty::Json<serde_json::Value>>,
            meta: meta.map(toasty::Json),
            created_at: now,
            updated_at: now,
        })
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
    }

    pub async fn get_chat_by_id(&self, id: ChatId) -> Result<Option<Chat>> {
        Chat::get_by_id(&mut self.db.clone(), &id.0)
            .await
            .map(Some)
            .or_else(|_| Ok(None))
    }

    pub async fn get_account_by_id(&self, id: AccountId) -> Result<Option<Account>> {
        Account::get_by_id(&mut self.db.clone(), &id.0)
            .await
            .map(Some)
            .or_else(|_| Ok(None))
    }

    pub async fn get_identity_accounts(&self, identity_id: IdentityId) -> Result<Vec<Account>> {
        Account::filter_by_identity_id(identity_id.0)
            .exec(&mut self.db.clone())
            .await
            .map_err(Into::into)
    }

    pub async fn ensure_bot_account(&self) -> Result<Account> {
        self.get_or_create_account(Platform::System, "bot", None)
            .await
    }
}
