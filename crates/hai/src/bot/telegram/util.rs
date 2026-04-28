use teloxide::{
    net::Download,
    prelude::*,
    types::{FileId, MessageEntity, MessageEntityKind},
};
use uuid::Uuid;

use crate::{
    domain::{
        entity::ChatType,
        vo::{MessageMeta, PlatformMessageMeta, TelegramContentPart, TelegramMessageMeta},
    },
    error::{AppResultExt, ErrorKind, Result},
};

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
                    file_id: photo.file.id.clone(),
                    width: photo.width,
                    height: photo.height,
                    caption,
                });
            }
        } else if let Some(video) = msg.video() {
            parts.push(TelegramContentPart::Video {
                attachment_id: Uuid::now_v7(),
                file_id: video.file.id.clone(),
                caption,
            });
        } else if let Some(audio) = msg.audio() {
            parts.push(TelegramContentPart::Audio {
                attachment_id: Uuid::now_v7(),
                file_id: audio.file.id.clone(),
                caption,
            });
        } else if let Some(voice) = msg.voice() {
            parts.push(TelegramContentPart::Voice {
                attachment_id: Uuid::now_v7(),
                file_id: voice.file.id.clone(),
                meta: None,
            });
        } else if let Some(document) = msg.document() {
            parts.push(TelegramContentPart::Document {
                attachment_id: Uuid::now_v7(),
                file_id: document.file.id.clone(),
                file_name: document.file_name.clone(),
                caption,
            });
        } else if let Some(sticker) = msg.sticker() {
            parts.push(TelegramContentPart::Sticker {
                attachment_id: Uuid::now_v7(),
                file_id: sticker.file.id.clone(),
                emoji: sticker.emoji.clone(),
            });
        } else if let Some(animation) = msg.animation() {
            parts.push(TelegramContentPart::Animation {
                attachment_id: Uuid::now_v7(),
                file_id: animation.file.id.clone(),
            });
        } else if let Some(video_note) = msg.video_note() {
            parts.push(TelegramContentPart::VideoNote {
                attachment_id: Uuid::now_v7(),
                file_id: video_note.file.id.clone(),
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

/// 通过 file_id 下载文件字节（命令处理、sticker 缓存等场景使用）
pub async fn download_file(bot: &Bot, file_id: &str) -> Result<Vec<u8>> {
    let file = bot.get_file(FileId(file_id.to_string())).await?;
    let mut data = Vec::new();
    bot.download_file(&file.path, &mut data)
        .await
        .change_err_msg(ErrorKind::BadRequest, "Failed to download file")?;
    Ok(data)
}

/// 获取文件的 Telegram CDN URL（生成图片等场景使用）
pub async fn get_file_url(bot: &Bot, file_id: FileId) -> Result<String> {
    let file = bot.get_file(file_id).await?;
    Ok(format!(
        "https://api.telegram.org/file/bot{}/{}",
        bot.token(),
        file.path
    ))
}
