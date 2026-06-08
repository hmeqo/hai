use std::time::{Duration, SystemTime};

use jiff::{Timestamp, tz::TimeZone};
use timeago::Formatter;

use crate::domain::{entity::Account, vo::PlatformAccountMeta};

/// 显示名称
pub fn display_name(account: Option<&Account>, fallback_id: i64) -> String {
    let Some(account) = account else {
        return format!("User{}", fallback_id);
    };
    let meta = account
        .meta
        .clone()
        .and_then(|v| serde_json::from_value::<PlatformAccountMeta>(v).ok());

    let Some(meta) = meta else {
        return format!("User{}", fallback_id);
    };

    let full_name = meta.full_name();
    let username = meta.username();

    match username {
        Some(u) => format!("{} (@{})", full_name, u),
        None => full_name,
    }
}

/// 动态时间格式化（24h 内相对时间，超过则绝对时间）
pub fn format_time_dyn(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return "None".to_string();
    };
    let now = Timestamp::now();
    if now.duration_since(ts).as_secs() < 86400 {
        format_relative_time(ts)
    } else {
        ts.to_zoned(TimeZone::system()).to_string()
    }
}

/// 动态时间格式化 v2（绝对 + 相对混合）
pub fn format_time_dyn2(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return "None".to_string();
    };
    let now = Timestamp::now();
    if now.duration_since(ts).as_secs() < 86400 {
        format_relative_time(ts)
    } else {
        format!(
            "{} ({})",
            ts.to_zoned(TimeZone::system()),
            format_relative_time(ts)
        )
    }
}

/// 相对时间格式化
pub fn format_relative_time(ts: Timestamp) -> String {
    let then = SystemTime::UNIX_EPOCH + Duration::from_secs(ts.as_second() as u64);
    let duration = SystemTime::now().duration_since(then).unwrap_or_default();
    Formatter::new().convert(duration)
}
