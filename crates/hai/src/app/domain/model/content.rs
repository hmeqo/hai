use serde::{Deserialize, Serialize};

/// 平台特定的消息内容部分
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelegramContentPart {
    Text {
        text: String,
    },
    Photo {
        file_id: String,
        width: u32,
        height: u32,
        caption: Option<String>,
    },
    Video {
        file_id: String,
        caption: Option<String>,
    },
    Audio {
        file_id: String,
        caption: Option<String>,
    },
    Voice {
        file_id: String,
    },
    Document {
        file_id: String,
        file_name: Option<String>,
        caption: Option<String>,
    },
    Sticker {
        file_id: String,
        emoji: Option<String>,
    },
    Animation {
        file_id: String,
    },
    VideoNote {
        file_id: String,
    },
}

impl TelegramContentPart {
    /// 返回可用于全文搜索 / embedding 的文字内容（文本正文或媒体 caption）
    pub fn text(&self) -> Option<&str> {
        match self {
            TelegramContentPart::Text { text } => Some(text),
            TelegramContentPart::Photo {
                caption: Some(c), ..
            } => Some(c),
            TelegramContentPart::Video {
                caption: Some(c), ..
            } => Some(c),
            TelegramContentPart::Audio {
                caption: Some(c), ..
            } => Some(c),
            TelegramContentPart::Document {
                caption: Some(c), ..
            } => Some(c),
            _ => None,
        }
    }

    /// 从 JSON Value 中解析并提取所有可搜索文字，用于 embedding / 向量检索
    pub fn extract_text_from_value(value: &serde_json::Value) -> String {
        match serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) {
            Ok(parts) => {
                let text: Vec<&str> = parts.iter().filter_map(|p| p.text()).collect();
                text.join(" ")
            }
            Err(_) => value.to_string(),
        }
    }
}
