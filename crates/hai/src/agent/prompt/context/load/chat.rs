use std::collections::{HashMap, HashSet};

use crate::{
    domain::{
        model::{Account, Message},
        service::Services,
        vo::{AccountId, ChatId, MessageId},
    },
    error::{AppError, Result},
};

pub async fn load_chat(services: &Services, chat_id: ChatId) -> Result<crate::domain::model::Chat> {
    services
        .platform
        .get_chat_by_id(chat_id)
        .await?
        .ok_or_else(|| AppError::message(format!("Chat not found: {chat_id}")))
}

pub async fn load_reply_map(
    services: &Services,
    messages: &[Message],
) -> Result<HashMap<MessageId, Message>> {
    let window: HashSet<MessageId> = messages.iter().map(|m| m.id).collect();
    let missing: Vec<MessageId> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !window.contains(rid))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if missing.is_empty() {
        return Ok(HashMap::new());
    }
    let found = services.message.get_messages_by_ids(&missing).await?;
    Ok(found.into_iter().map(|m| (m.id, m)).collect())
}

/// The accounts a window's senders name; a missing account is named `User{id}`. Sorted by id so the
/// rendered prompt is deterministic.
pub async fn collect_accounts(
    services: &Services,
    messages: &[Message],
    reply_map: &HashMap<MessageId, Message>,
) -> Result<Vec<Account>> {
    let raw_ids: HashSet<AccountId> = messages
        .iter()
        .chain(reply_map.values())
        .filter_map(|m| m.account_id)
        .collect();
    let mut accounts = Vec::with_capacity(raw_ids.len());

    for id in raw_ids {
        if let Some(account) = services.platform.get_account_by_id(id).await? {
            accounts.push(account);
        }
    }
    accounts.sort_by_key(|account| account.id);
    Ok(accounts)
}
