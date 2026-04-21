use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    entity::{Account, Chat, ChatType, Platform},
    repo::{AccountRepo, ChatRepo},
};

/// 平台 ID 映射服务：将平台原始 ID 转换为内部 ID
pub struct PlatformService {
    pool: PgPool,
}

impl PlatformService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 确保会话和账号都存在（通常在收到消息时调用）
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

    /// 通过平台标识获取或创建账号
    pub async fn get_or_create_account(
        &self,
        platform: Platform,
        external_id: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<Account> {
        AccountRepo::get_or_create(&self.pool, platform.into(), external_id, meta).await
    }

    /// 通过平台标识获取或创建会话/群组
    pub async fn get_or_create_chat(
        &self,
        platform: Platform,
        external_id: &str,
        chat_type: ChatType,
        name: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> Result<Chat> {
        ChatRepo::get_or_create(
            &self.pool,
            platform.into(),
            external_id,
            chat_type.into(),
            name,
            meta,
        )
        .await
    }

    /// 通过内部 ID 获取会话
    pub async fn get_chat_by_id(&self, id: i64) -> Result<Option<Chat>> {
        ChatRepo::find_by_id(&self.pool, id).await
    }

    /// 通过内部 ID 获取账号
    pub async fn get_account_by_id(&self, id: i64) -> Result<Option<Account>> {
        AccountRepo::find_by_id(&self.pool, id).await
    }

    /// 获取身份关联的所有账号
    pub async fn get_identity_accounts(&self, identity_id: Uuid) -> Result<Vec<Account>> {
        AccountRepo::list_by_identity_id(&self.pool, identity_id).await
    }

    /// 确保 Bot 账号存在
    pub async fn ensure_bot_account(&self) -> Result<Account> {
        AccountRepo::get_or_create(&self.pool, Platform::System.into(), "bot", None).await
    }
}
