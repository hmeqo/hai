use std::fmt::Write;

use crate::domain::vo::TelegramContentPart;

/// 将 `TelegramContentPart` 列表渲染为可读的展示字符串（用于 LLM prompt）
///
/// 每种媒体类型会渲染为 `[Type: ...]` 格式，文字内容直接展示。
pub fn render_content_parts(parts: &[TelegramContentPart]) -> String {
    let mut output = String::new();
    for part in parts {
        render_single_part(part, &mut output);
        output.push('\n');
    }
    output.trim_end_matches('\n').to_string()
}

/// 从 JSON Value 中解析并渲染为展示字符串，解析失败时回退到原始 JSON
pub fn render_content_from_value(value: &serde_json::Value) -> String {
    match serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) {
        Ok(parts) => render_content_parts(&parts),
        Err(_) => value.to_string(),
    }
}

fn render_single_part(part: &TelegramContentPart, output: &mut String) {
    match part {
        TelegramContentPart::Text { text } => {
            let _ = write!(output, "{}", text);
        }
        TelegramContentPart::Photo {
            file_id, caption, ..
        } => {
            let _ = write!(output, "[Photo: {}]", file_id);
            if let Some(c) = caption {
                let _ = write!(output, " {}", c);
            }
        }
        TelegramContentPart::Video { file_id, caption } => {
            let _ = write!(output, "[Video: {}]", file_id);
            if let Some(c) = caption {
                let _ = write!(output, " {}", c);
            }
        }
        TelegramContentPart::Audio { file_id, caption } => {
            let _ = write!(output, "[Audio: {}]", file_id);
            if let Some(c) = caption {
                let _ = write!(output, " {}", c);
            }
        }
        TelegramContentPart::Voice { file_id } => {
            let _ = write!(output, "[Voice: {}]", file_id);
        }
        TelegramContentPart::Document {
            file_id,
            file_name,
            caption,
        } => {
            let _ = write!(
                output,
                "[File: {}]",
                file_name.as_deref().unwrap_or(&file_id.0)
            );
            if let Some(c) = caption {
                let _ = write!(output, " {}", c);
            }
        }
        TelegramContentPart::Sticker { file_id, emoji } => {
            let _ = write!(
                output,
                "[Sticker: {} {}]",
                emoji.as_deref().unwrap_or_default(),
                file_id
            );
        }
        TelegramContentPart::Animation { file_id } => {
            let _ = write!(output, "[Animation: {}]", file_id);
        }
        TelegramContentPart::VideoNote { file_id } => {
            let _ = write!(output, "[VideoNote: {}]", file_id);
        }
    }
}
