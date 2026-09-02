use std::sync::Arc;

use crate::{
    agent::context::{
        Attachment, AttachmentPerceptionMap, ContentParser, ParsedContent,
        render_context::ContentRenderer,
    },
    domain::vo::TelegramContentPart,
    platform::telegram::render::render_content as telegram_render_content,
};

pub struct TelegramContentParser;

impl ContentParser for TelegramContentParser {
    fn parse(&self, value: &serde_json::Value) -> ParsedContent {
        let Ok(parts) = serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) else {
            return ParsedContent {
                text: String::new(),
                attachments: Vec::new(),
                text_fragments: Vec::new(),
            };
        };

        let mut text = String::new();
        let mut attachments = Vec::new();
        let mut text_fragments = Vec::new();

        for part in parts {
            if let Some(aid) = part.attachment_id()
                && let Some(fid) = part.file_id()
            {
                attachments.push(Attachment {
                    id: aid,
                    file_id: fid.to_string(),
                });
            }
            match &part {
                TelegramContentPart::Text { text: t } => {
                    text.push_str(t);
                    text_fragments.push(t.clone());
                }
                _ => {
                    if let Some(t) = part.text() {
                        text.push_str(t);
                        text_fragments.push(t.to_string());
                    }
                }
            }
        }

        ParsedContent {
            text,
            attachments,
            text_fragments,
        }
    }

    fn create_renderer(&self, map: &AttachmentPerceptionMap) -> ContentRenderer {
        let by_id = map.by_attachment_id.clone();
        let same_resource = map.same_resource_as.clone();
        Arc::new(move |value| telegram_render_content(value, &by_id, &same_resource))
    }
}
