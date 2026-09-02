use std::collections::{HashMap, HashSet};

use crate::{
    domain::{
        model::{Account, Message},
        service::DbServices,
        vo::{AccountId, ChatId, IdentityId, MessageId},
    },
    error::{ErrorKind, OptionAppExt, Result},
};

pub async fn load_chat(
    services: &DbServices,
    chat_id: ChatId,
) -> Result<crate::domain::model::Chat> {
    services
        .platform
        .get_chat_by_id(chat_id)
        .await?
        .ok_or_err_msg(ErrorKind::NotFound, format!("Chat not found: {chat_id}"))
}

pub async fn load_reply_map(
    services: &DbServices,
    messages: &[Message],
) -> Result<HashMap<i64, Message>> {
    let window: HashSet<i64> = messages.iter().map(|m| m.id).collect();
    let missing: Vec<i64> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !window.contains(rid))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if missing.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<MessageId> = missing.into_iter().map(MessageId).collect();
    let found = services.message.get_messages_by_ids(&ids).await?;
    Ok(found.into_iter().map(|m| (m.id, m)).collect())
}

/// 含 identity 关联的 sibling account（reference 发送者名渲染依赖；缺失降级为 User{id}）。
pub async fn collect_accounts(
    services: &DbServices,
    messages: &[Message],
    reply_map: &HashMap<i64, Message>,
) -> Result<Vec<Account>> {
    let raw_ids: HashSet<i64> = messages
        .iter()
        .chain(reply_map.values())
        .filter_map(|m| m.account_id)
        .collect();
    let mut account_map: HashMap<i64, Account> = HashMap::new();

    for id in raw_ids {
        if account_map.contains_key(&id) {
            continue;
        }
        if let Some(account) = services.platform.get_account_by_id(AccountId(id)).await? {
            if let Some(identity_id) = account.identity_id {
                for sibling in services
                    .platform
                    .get_identity_accounts(IdentityId(identity_id))
                    .await?
                {
                    account_map.insert(sibling.id, sibling);
                }
            } else {
                account_map.insert(id, account);
            }
        }
    }
    Ok(account_map.into_values().collect())
}
