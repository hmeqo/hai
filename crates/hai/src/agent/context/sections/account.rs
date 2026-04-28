//! 账户组件 - 构建 RenderElement

use crate::{
    agent::context::render_context::display_name,
    agentcore::render::elements::{RenderElement, Section, item, section},
    domain::{entity::Account, vo::PlatformAccountMeta},
};

/// 构建单个账户元素
pub fn account_element(account: &Account) -> RenderElement {
    let meta = account
        .meta
        .as_ref()
        .and_then(|v| serde_json::from_value::<PlatformAccountMeta>(v.clone()).ok());

    let mut builder = item("account").with_attr("id", account.id);

    if let Some(m) = &meta {
        if let Some(u) = m.username() {
            builder = builder.with_attr("username", format!("@{}", u));
        }
        builder = builder.with_attr("name", m.full_name());
    } else {
        builder = builder.with_attr("name", display_name(account, account.id));
    }

    if let Some(iid) = account.identity_id {
        builder = builder.with_attr("identity_id", iid);
    }

    builder.into_element()
}

/// 构建账户列表元素
pub fn accounts_elements(accounts: &[Account]) -> Vec<RenderElement> {
    accounts.iter().map(account_element).collect()
}

/// 构建账户 Section
pub fn accounts_section(accounts: &[Account], tag: &str) -> Section {
    section(tag).add_children(accounts_elements(accounts))
}
