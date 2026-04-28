use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 用于从 file_id 确定性生成 resource_id 的命名空间 UUID。
const FILE_ID_NS: Uuid = Uuid::from_u128(0x6ba7b811_9dad_11d1_80b4_00c04fd430c8);

/// 从平台 file_id 确定性生成 UUID（用于 perception 去重）。
pub fn resource_id_from_file_id(file_id: &str) -> Uuid {
    Uuid::new_v5(&FILE_ID_NS, file_id.as_bytes())
}

/// 感知来源
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// 平台附件（如 Telegram file_id）
    Platform { platform: String, file_id: String },
    /// 直接 URL
    Url { url: String },
}

impl Source {
    pub fn platform(platform: impl Into<String>, file_id: impl Into<String>) -> Self {
        Source::Platform {
            platform: platform.into(),
            file_id: file_id.into(),
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Source::Url { url: url.into() }
    }

    pub fn cache_key(&self) -> String {
        match self {
            Source::Platform { platform, file_id } => format!("{}_{}", platform, file_id),
            Source::Url { url } => url.clone(),
        }
    }
}
