use std::fmt;

use serde::{Deserialize, Serialize};

// ── ID 类型生成宏 ─────────────────────────────────────────────────────────────
// 每个 ID 类型 = 透明 newtype + From/Into + Display。
// raw_ids() 方法批量转 `[Self]` → `Vec<K>`，避免服务层反复写 .map(|id| id.0)。

macro_rules! id_type {
    ($vis:vis struct $name:ident(pub $inner:ty);) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        $vis struct $name(pub $inner);

        impl From<$inner> for $name {
            fn from(v: $inner) -> Self { Self(v) }
        }
        impl From<$name> for $inner {
            fn from(v: $name) -> Self { v.0 }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }
        impl $name {
            /// 批量将 `&[$name]` 转为 `Vec<$inner>`，用于 `WHERE id = ANY($1)` 等批量场景。
            pub fn raw_ids(ids: &[Self]) -> Vec<$inner> {
                ids.iter().map(|id| id.0).collect()
            }
        }
    };
}

id_type!(
    pub struct ChatId(pub i64);
);
id_type!(
    pub struct MessageId(pub i64);
);
id_type!(
    pub struct AccountId(pub i64);
);
id_type!(
    pub struct TopicId(pub uuid::Uuid);
);
id_type!(
    pub struct MemoryId(pub uuid::Uuid);
);
id_type!(
    pub struct PerceptionId(pub uuid::Uuid);
);
id_type!(
    pub struct IdentityId(pub uuid::Uuid);
);
id_type!(
    pub struct KnowledgeDocumentId(pub uuid::Uuid);
);
id_type!(
    pub struct KnowledgeChunkId(pub uuid::Uuid);
);
id_type!(
    pub struct TurnNumber(pub usize);
);
id_type!(
    pub struct StepNumber(pub usize);
);
