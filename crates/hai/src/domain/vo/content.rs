use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumDiscriminants, EnumString, FromRepr, IntoStaticStr};
use teloxide::types::FileId;
use uuid::Uuid;

/// 附件解析器类型
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AttachmentParser {
    Image,
    Audio,
    Video,
    Ocr,
}

impl AttachmentParser {
    pub fn name(&self) -> &'static str {
        self.into()
    }
}

/// 平台特定的消息内容部分
///
/// 每个附件 variant 携带一个 `attachment_id`（UUID），在消息保存时生成，
/// 用于 agent 通过稳定 ID 直接引用单个附件。
#[derive(Debug, Clone, Serialize, Deserialize, EnumString, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TelegramContentPart {
    Text {
        text: String,
    },
    Photo {
        attachment_id: Uuid,
        file_id: FileId,
        width: u32,
        height: u32,
        caption: Option<String>,
    },
    Video {
        attachment_id: Uuid,
        file_id: FileId,
        caption: Option<String>,
    },
    Audio {
        attachment_id: Uuid,
        file_id: FileId,
        caption: Option<String>,
    },
    Voice {
        attachment_id: Uuid,
        file_id: FileId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<VoiceMeta>,
    },
    Document {
        attachment_id: Uuid,
        file_id: FileId,
        file_name: Option<String>,
        caption: Option<String>,
    },
    Sticker {
        attachment_id: Uuid,
        file_id: FileId,
        emoji: Option<String>,
    },
    Animation {
        attachment_id: Uuid,
        file_id: FileId,
    },
    VideoNote {
        attachment_id: Uuid,
        file_id: FileId,
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
            TelegramContentPart::Voice { meta: Some(m), .. } => Some(&m.prompt),
            TelegramContentPart::Document {
                caption: Some(c), ..
            } => Some(c),
            _ => None,
        }
    }

    /// 返回附件的稳定 ID
    pub fn attachment_id(&self) -> Option<Uuid> {
        match self {
            TelegramContentPart::Text { .. } => None,
            TelegramContentPart::Photo { attachment_id, .. }
            | TelegramContentPart::Video { attachment_id, .. }
            | TelegramContentPart::Audio { attachment_id, .. }
            | TelegramContentPart::Voice { attachment_id, .. }
            | TelegramContentPart::Document { attachment_id, .. }
            | TelegramContentPart::Sticker { attachment_id, .. }
            | TelegramContentPart::Animation { attachment_id, .. }
            | TelegramContentPart::VideoNote { attachment_id, .. } => Some(*attachment_id),
        }
    }

    /// 返回附件的 file_id
    pub fn file_id(&self) -> Option<&str> {
        match self {
            TelegramContentPart::Text { .. } => None,
            TelegramContentPart::Photo { file_id, .. }
            | TelegramContentPart::Video { file_id, .. }
            | TelegramContentPart::Audio { file_id, .. }
            | TelegramContentPart::Voice { file_id, .. }
            | TelegramContentPart::Document { file_id, .. }
            | TelegramContentPart::Sticker { file_id, .. }
            | TelegramContentPart::Animation { file_id, .. }
            | TelegramContentPart::VideoNote { file_id, .. } => Some(&file_id.0),
        }
    }

    /// 返回该附件类型对应的解析器（None 表示不支持解析，如纯文本）
    ///
    /// Document 类型会根据文件名扩展名推断实际类型（音频 → Transcribe，其余 → Ocr）。
    pub fn attachment_parser(&self) -> Option<AttachmentParser> {
        match self {
            TelegramContentPart::Photo { .. } | TelegramContentPart::Sticker { .. } => {
                Some(AttachmentParser::Image)
            }
            TelegramContentPart::Video { .. }
            | TelegramContentPart::Animation { .. }
            | TelegramContentPart::VideoNote { .. } => Some(AttachmentParser::Video),
            TelegramContentPart::Document { file_name, .. } => {
                let is_audio = file_name
                    .as_deref()
                    .and_then(MediaCodec::from_ext)
                    .is_some_and(MediaCodec::is_audio);
                if is_audio {
                    Some(AttachmentParser::Audio)
                } else {
                    Some(AttachmentParser::Ocr)
                }
            }
            TelegramContentPart::Audio { .. } | TelegramContentPart::Voice { .. } => {
                Some(AttachmentParser::Audio)
            }
            TelegramContentPart::Text { .. } => None,
        }
    }

    /// 返回媒体编码格式（主要用于语音识别）。
    ///
    /// - Voice → ogg（Telegram 语音消息固定为 OGG Opus）
    /// - Audio → mp3
    /// - Document → 从文件名扩展名推断
    /// - 其余类型 → None
    pub fn media_format(&self) -> Option<MediaCodec> {
        match self {
            TelegramContentPart::Voice { .. } => Some(MediaCodec::Ogg),
            TelegramContentPart::Audio { .. } => Some(MediaCodec::Mp3),
            TelegramContentPart::Document { file_name, .. } => {
                file_name.as_deref().and_then(MediaCodec::from_ext)
            }
            _ => None,
        }
    }

    /// 返回附件的额外提示信息（如 sticker 的 emoji）
    pub fn extra_hint(&self) -> Option<&str> {
        match self {
            TelegramContentPart::Sticker { emoji, .. } => emoji.as_deref(),
            _ => None,
        }
    }

    /// 用于渲染展示的类型标签
    pub fn display_label(&self) -> &'static str {
        self.into()
    }

    /// 从 JSON Value 中解析并提取所有可搜索文字，用于 embedding / 向量检索
    pub fn extract_text_from_value(value: &serde_json::Value) -> String {
        match serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) {
            Ok(parts) => parts
                .iter()
                .filter_map(|p| p.text())
                .collect::<Vec<_>>()
                .join(" "),
            Err(_) => value.to_string(),
        }
    }
}

/// Agent TTS 语音消息的元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceMeta {
    /// agent 在 send_voice 中填写的提示词（也是实际朗读内容）
    pub prompt: String,
}

/// 媒体编码格式，覆盖音频/视频，可从文件名扩展名推断。
///
/// 扩展方式：新增变体 + 在 `from_extension` 中添加映射即可。
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum MediaCodec {
    // 音频
    Mp3,
    Ogg,
    Wav,
    M4a,
    Aac,
    Flac,
    Wma,
    // 视频
    Mp4,
    Mov,
    Avi,
    Mkv,
    Webm,
}

impl MediaCodec {
    pub const fn default_audio() -> Self {
        Self::Wav
    }

    /// 文件扩展名，用于传给下游服务。
    pub fn ext(self) -> &'static str {
        self.into()
    }

    pub fn from_ext(name: &str) -> Option<Self> {
        Self::from_str(name).ok()
    }

    pub fn is_audio(self) -> bool {
        matches!(
            self,
            Self::Mp3 | Self::Ogg | Self::Wav | Self::M4a | Self::Flac | Self::Wma
        )
    }

    /// 是否为视频编码。
    pub fn is_video(self) -> bool {
        matches!(
            self,
            Self::Mp4 | Self::Mov | Self::Avi | Self::Mkv | Self::Webm
        )
    }
}
