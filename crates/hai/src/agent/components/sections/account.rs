//! 账户组件 - 构建 RenderElement

use crate::agent::components::context::AccountInfo;
use crate::agent::render::elements::{RenderElement, Section, item, section};
use crate::domain::entity::Account;

/// 构建单个账户元素
pub fn account_element(account: &Account) -> RenderElement {
    let info = AccountInfo::from_account(account);

    let mut builder = item("account").with_attr("id", info.id);

    if let Some(u) = info.username {
        builder = builder.with_attr("username", format!("@{}", u));
    }

    if let Some(name) = info.full_name {
        builder = builder.with_attr("name", name);
    }

    if let Some(iid) = info.identity_id {
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
