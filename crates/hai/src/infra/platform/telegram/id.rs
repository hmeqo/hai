use std::sync::Arc;

use derive_more::Deref;
use teloxide::{Bot, prelude::Requester};

use crate::error::Result;

// ─── 数据结构 ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct BotIdentityInner {
    pub account_id: i64,
    pub username: String,
    pub name: String,
}

impl BotIdentityInner {
    pub async fn new(account_id: i64, bot: &Bot) -> Result<Self> {
        let username = bot.get_me().await?.username.clone().unwrap_or_default();
        let name = bot.get_my_name().await?.name;
        Ok(Self {
            account_id,
            username,
            name,
        })
    }
}

/// Bot 自身身份信息
#[derive(Debug, Clone, Deref)]
pub struct BotIdentity {
    inner: Arc<BotIdentityInner>,
}

impl BotIdentity {
    pub async fn new(account_id: i64, bot: &Bot) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(BotIdentityInner::new(account_id, bot).await?),
        })
    }
}
