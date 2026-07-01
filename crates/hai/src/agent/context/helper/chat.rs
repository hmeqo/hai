use std::collections::{HashMap, HashSet};

use crate::{
    domain::{
        model::{Account, Message},
        service::DbServices,
        vo::{AccountId, ChatId, IdentityId, MessageId},
    },
    error::{ErrorKind, OptionAppExt, Result},
};

/// 按 ID 获取聊天记录
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

/// 加载消息中引用但尚未在集合中的回复上下文
pub async fn load_reply_context(
    services: &DbServices,
    messages: &[Message],
) -> Result<Vec<Message>> {
    let main_ids: HashSet<i64> = messages.iter().map(|m| m.id).collect();
    let missing: Vec<i64> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !main_ids.contains(rid))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if missing.is_empty() {
        return Ok(Vec::new());
    }
    services
        .message
        .get_messages_by_ids(&missing.iter().map(|id| MessageId(*id)).collect::<Vec<_>>())
        .await
}

/// 收集消息中所有 account（含 identity 关联的 sibling account）
pub async fn collect_accounts(services: &DbServices, messages: &[Message]) -> Result<Vec<Account>> {
    let raw_ids: HashSet<i64> = messages.iter().filter_map(|m| m.account_id).collect();
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
