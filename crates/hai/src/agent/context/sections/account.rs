//! 账户组件 - 构建 XML 节点

use super::fmt::display_name;
use crate::{
    agentcore::render::elements::Node,
    domain::{entity::Account, vo::PlatformAccountMeta},
};

/// 构建单个账户元素
pub fn account_element(account: &Account) -> Node {
    let meta = account
        .meta
        .as_ref()
        .and_then(|v| serde_json::from_value::<PlatformAccountMeta>(v.clone()).ok());

    let mut b = Node::tag("account").attr("id", account.id);

    if let Some(m) = &meta {
        if let Some(u) = m.username() {
            b = b.attr("username", format!("@{}", u));
        }
        b = b.attr("name", m.full_name());
    } else {
        b = b.attr("name", display_name(Some(account), account.id));
    }

    if let Some(iid) = account.identity_id {
        b = b.attr("identity_id", iid);
    }

    b
}

/// 构建账户列表元素
pub fn accounts_elements(accounts: &[Account]) -> Vec<Node> {
    accounts.iter().map(account_element).collect()
}

/// 构建账户 Section
pub fn accounts_section(accounts: &[Account], tag: &str) -> Node {
    Node::tag(tag).children(accounts_elements(accounts))
}
