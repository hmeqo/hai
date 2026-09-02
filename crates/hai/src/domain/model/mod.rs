pub mod account;
pub mod chat;
pub mod conversation_record;
pub mod event;
pub mod identity;
pub mod knowledge_chunk;
pub mod knowledge_document;
pub mod memory;
pub mod message;
pub mod perception;
pub mod scheduled_task;
pub mod scratchpad;
pub mod topic;

pub use account::*;
pub use chat::*;
pub use conversation_record::*;
pub use event::*;
pub use identity::*;
pub use knowledge_chunk::*;
pub use knowledge_document::*;
pub use memory::*;
pub use message::*;
pub use perception::*;
pub use scheduled_task::*;
pub use scratchpad::*;
use sqlx::ValueRef;
pub use topic::*;

/// 时间列解码桥接：`jiff-sqlx` 只为 `jiff_sqlx::Timestamp` wrapper 实现 Decode/Type，
/// 且 FromRow 的 `try_from` 对 Option 字段无特殊处理——本地实现 NULL → None 桥接。
#[derive(Debug, Clone, Copy)]
pub(crate) struct SqlxNullableTimestamp(pub(crate) Option<jiff_sqlx::Timestamp>);

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SqlxNullableTimestamp {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        if value.is_null() {
            Ok(SqlxNullableTimestamp(None))
        } else {
            <jiff_sqlx::Timestamp as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)
                .map(|t| SqlxNullableTimestamp(Some(t)))
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for SqlxNullableTimestamp {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <jiff_sqlx::Timestamp as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

impl From<SqlxNullableTimestamp> for Option<jiff::Timestamp> {
    fn from(v: SqlxNullableTimestamp) -> Self {
        v.0.map(Into::into)
    }
}
