use std::sync::Arc;

use derive_more::Deref;
use teloxide::{Bot, prelude::Requester};

use crate::{domain::entity::Account, error::Result};

// ─── 数据结构 ───────────────────────────────────────────────────────────────

/// Bot 自身身份信息
#[derive(Debug, Clone, Deref)]
pub struct BotIdentity {
    inner: Arc<BotIdentityInner>,
}

impl BotIdentity {
    pub async fn new(account: Account, bot: &Bot) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(BotIdentityInner::new(account, bot).await?),
        })
    }
}

#[derive(Debug)]
pub struct BotIdentityInner {
    pub account: Account,
    pub username: String,
    pub name: String,
}

impl BotIdentityInner {
    pub async fn new(account: Account, bot: &Bot) -> Result<Self> {
        let username = bot.get_me().await?.username.clone().unwrap_or_default();
        let name = bot.get_my_name().await?.name;
        Ok(Self {
            account,
            username,
            name,
        })
    }

    pub fn account_id(&self) -> i64 {
        self.account.id
    }
}
