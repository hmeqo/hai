use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    agentcore::render::elements::{Item, RenderElement, item},
    domain::{entity::Perception, vo::TelegramContentPart},
};

/// 从 JSON Value 解析内容部分并渲染为结构化元素列表
///
/// `perception_map` 为 attachment_id → Vec<Perception>，首个 occurrence 内嵌全部 <analysis>。
/// `same_resource_as` 为重复 attachment_id → 首个 attachment_id。
pub fn render_content(
    value: &serde_json::Value,
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Vec<RenderElement> {
    match serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) {
        Ok(parts) => render_content_parts(&parts, perception_map, same_resource_as),
        Err(_) => vec![RenderElement::Text(value.to_string().into())],
    }
}

fn render_content_parts(
    parts: &[TelegramContentPart],
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Vec<RenderElement> {
    parts
        .iter()
        .filter_map(|p| render_part(p, perception_map, same_resource_as))
        .collect()
}

fn render_part(
    part: &TelegramContentPart,
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Option<RenderElement> {
    match part {
        TelegramContentPart::Text { text } => Some(RenderElement::Text(text.clone().into())),
        _ => {
            let attachment_id = part.attachment_id()?;
            let mut element = item("attachment")
                .with_attr("id", attachment_id.to_string())
                .with_attr("type", part.display_label());

            // 首个 occurrence：内嵌该 resource 的全部 <analysis>
            if let Some(perceptions) = perception_map.get(&attachment_id) {
                for p in perceptions {
                    let mut analysis = item("analysis")
                        .with_attr("parser", &p.parser)
                        .with_content(&p.content);
                    if let Some(prompt) = &p.prompt {
                        analysis = analysis.with_attr("prompt", prompt.as_str());
                    }
                    element = element.add_child(analysis);
                }
            }

            // 重复 resource：标记指向首个 occurrence
            if let Some(&first_id) = same_resource_as.get(&attachment_id) {
                element = element.with_attr("same_resource_as", first_id.to_string());
            }

            if let Some(hint) = part.extra_hint() {
                element = element.with_attr("hint", hint);
            }

            if let Some(caption) = attachment_caption(part) {
                element = element.with_attr("caption", caption);
            }

            if let TelegramContentPart::Voice { meta: Some(m), .. } = part {
                element = element.with_attr("prompt", &m.prompt);
            }

            Some(RenderElement::Item(element))
        }
    }
}

fn attachment_caption(part: &TelegramContentPart) -> Option<&str> {
    match part {
        TelegramContentPart::Photo { caption, .. } => caption.as_deref(),
        TelegramContentPart::Video { caption, .. } => caption.as_deref(),
        TelegramContentPart::Audio { caption, .. } => caption.as_deref(),
        TelegramContentPart::Document { caption, .. } => caption.as_deref(),
        _ => None,
    }
}

/// 将单条 perception 渲染为子 item
///
/// 无 prompt → `<perception id="...">content</perception>`
/// 有 prompt → `<perception id="..." prompt="...">content</perception>`
pub fn perception_item(p: &Perception) -> Item {
    let mut el = item("perception").with_attr("id", p.id);
    if let Some(prompt) = &p.prompt {
        el = el.with_attr("prompt", prompt.as_str());
    }
    el.with_content(&p.content)
}
