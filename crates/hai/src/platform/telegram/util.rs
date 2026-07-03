use teloxide::{
    prelude::*,
    types::{MessageEntity, MessageEntityKind},
};
use uuid::Uuid;

use crate::domain::{
    model::ChatType,
    vo::{FileId, MessageMeta, PlatformMessageMeta, TelegramContentPart, TelegramMessageMeta},
};

/// MarkdownV2 保留字符中无格式化意义的字符。
/// 这些字符在文本中自然出现时需要转义，但不应破坏 `*`, `_`, `` ` `` 等 Markdown 语法。
const MD_V2_ESCAPE_CHARS: &[char] = &['.', '!', '+', '-', '=', '>', '#', '|', '{', '}', '(', ')'];

/// 转义 MarkdownV2 保留字符中无格式化意义的符号，使 AI 生成文本可安全使用 MarkdownV2 发送。
pub(super) fn escape_md_v2(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if MD_V2_ESCAPE_CHARS.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 从 Telegram Message 中提取 ChatType
pub fn msg_chat_type(msg: &Message) -> ChatType {
    if msg.chat.is_private() {
        ChatType::Private
    } else {
        ChatType::Group
    }
}

/// 检查消息是否提及了用户（支持纯文本和 caption 中的 @）
pub fn is_mentioning_user(msg: &Message, username: &str) -> bool {
    let username = format!("@{}", username);

    // 检查纯文本中的 entities（文字消息）
    if let Some(entities) = msg.entities()
        && let Some(text) = msg.text()
        && check_entities(entities, text, &username)
    {
        return true;
    }

    // 检查 caption 中的 entities（媒体消息：图片、视频、音频等）
    if let Some(entities) = msg.caption_entities()
        && let Some(caption) = msg.caption()
        && check_entities(entities, caption, &username)
    {
        return true;
    }

    false
}

fn check_entities(entities: &[MessageEntity], text: &str, username: &str) -> bool {
    entities.iter().any(|e| {
        if !matches!(e.kind, MessageEntityKind::Mention) {
            return false;
        }
        // Telegram entity offset/length 以 UTF-16 code unit 计算，
        // 需转换为 char 边界后再提取，避免多字节字符导致 panic
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let end = e.offset + e.length;
        utf16
            .get(e.offset..end)
            .and_then(|slice| String::from_utf16(slice).ok())
            .as_deref()
            == Some(username)
    })
}

pub struct ExtractedTelegramMessage {
    pub parts: Vec<TelegramContentPart>,
    pub meta: MessageMeta,
}

impl ExtractedTelegramMessage {
    pub fn extract(msg: &Message) -> Self {
        let mut parts = Vec::new();
        let caption = msg.caption().map(|c| c.to_string());

        // 1. 处理文本
        if let Some(text) = msg.text() {
            parts.push(TelegramContentPart::Text {
                text: text.to_string(),
            });
        }

        // 2. 处理媒体（每个附件生成唯一 attachment_id）
        if let Some(photos) = msg.photo() {
            if let Some(photo) = photos.last() {
                parts.push(TelegramContentPart::Photo {
                    attachment_id: Uuid::now_v7(),
                    file_id: FileId(photo.file.id.0.clone()),
                    width: photo.width,
                    height: photo.height,
                    caption,
                });
            }
        } else if let Some(video) = msg.video() {
            parts.push(TelegramContentPart::Video {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(video.file.id.0.clone()),
                caption,
            });
        } else if let Some(audio) = msg.audio() {
            parts.push(TelegramContentPart::Audio {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(audio.file.id.0.clone()),
                caption,
            });
        } else if let Some(voice) = msg.voice() {
            parts.push(TelegramContentPart::Voice {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(voice.file.id.0.clone()),
                meta: None,
            });
        } else if let Some(document) = msg.document() {
            parts.push(TelegramContentPart::Document {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(document.file.id.0.clone()),
                file_name: document.file_name.clone(),
                caption,
            });
        } else if let Some(sticker) = msg.sticker() {
            parts.push(TelegramContentPart::Sticker {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(sticker.file.id.0.clone()),
                emoji: sticker.emoji.clone(),
            });
        } else if let Some(animation) = msg.animation() {
            parts.push(TelegramContentPart::Animation {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(animation.file.id.0.clone()),
            });
        } else if let Some(video_note) = msg.video_note() {
            parts.push(TelegramContentPart::VideoNote {
                attachment_id: Uuid::now_v7(),
                file_id: FileId(video_note.file.id.0.clone()),
            });
        }

        // 3. 提取元数据 (如 thread_id)
        let tg_meta = TelegramMessageMeta {
            message_thread_id: msg.thread_id.map(|id| id.0.0),
        };
        let meta = MessageMeta {
            platform: Some(PlatformMessageMeta::Telegram(tg_meta)),
            llm: None,
        };

        Self { parts, meta }
    }
}
