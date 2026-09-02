use std::time::{Duration, SystemTime};

use jiff::{Timestamp, civil::Weekday, tz::TimeZone};
use timeago::Formatter;

use crate::domain::{model::Account, vo::PlatformAccountMeta};

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

pub fn format_relative_time(ts: Timestamp) -> String {
    let then = SystemTime::UNIX_EPOCH + Duration::from_secs(ts.as_second() as u64);
    let duration = SystemTime::now().duration_since(then).unwrap_or_default();
    Formatter::new().convert(duration)
}

/// HH:MM 时间部分
pub fn format_time_only(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return String::new();
    };
    ts.to_zoned(TimeZone::system())
        .strftime("%H:%M")
        .to_string()
}

/// 智能日期标签：今天 / 昨天 / 周二 / 7月19日 / 2024年7月19日
pub fn format_date_label(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return String::new();
    };
    let now = Timestamp::now();
    let secs = now.as_second() - ts.as_second();
    if secs < 0 {
        return String::from("今天");
    }
    let days = secs / 86400;

    if days < 1 {
        "今天".into()
    } else if days < 2 {
        "昨天".into()
    } else if days < 7 {
        let wd = ts.to_zoned(TimeZone::system()).weekday();
        match wd {
            Weekday::Monday => "周一",
            Weekday::Tuesday => "周二",
            Weekday::Wednesday => "周三",
            Weekday::Thursday => "周四",
            Weekday::Friday => "周五",
            Weekday::Saturday => "周六",
            Weekday::Sunday => "周日",
        }
        .into()
    } else {
        let z = ts.to_zoned(TimeZone::system());
        let now_z = jiff::Zoned::now();
        if z.year() == now_z.year() {
            format!("{}月{}日", z.month(), z.day())
        } else {
            format!("{}年{}月{}日", z.year(), z.month(), z.day())
        }
    }
}
