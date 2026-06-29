use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    agentcore::render::elements::Node,
    domain::{model::Perception, vo::TelegramContentPart},
};

pub fn render_content(
    value: &serde_json::Value,
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Vec<Node> {
    match serde_json::from_value::<Vec<TelegramContentPart>>(value.clone()) {
        Ok(parts) => render_parts(&parts, perception_map, same_resource_as),
        Err(_) => vec![Node::text(value.to_string())],
    }
}

fn render_parts(
    parts: &[TelegramContentPart],
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Vec<Node> {
    parts
        .iter()
        .filter_map(|p| render_part(p, perception_map, same_resource_as))
        .collect()
}

fn render_part(
    part: &TelegramContentPart,
    perception_map: &HashMap<Uuid, Vec<Perception>>,
    same_resource_as: &HashMap<Uuid, Uuid>,
) -> Option<Node> {
    match part {
        TelegramContentPart::Text { text } => Some(Node::text(text.clone())),
        _ => {
            let attachment_id = part.attachment_id()?;
            let mut element = Node::tag("attachment")
                .attr("id", attachment_id.to_string())
                .attr("type", part.display_label());

            if let Some(perceptions) = perception_map.get(&attachment_id) {
                for p in perceptions {
                    let mut analysis = Node::tag("analysis")
                        .attr("parser", &p.parser)
                        .child(Node::text(&p.content));
                    if let Some(prompt) = &p.prompt {
                        analysis = analysis.attr("prompt", prompt.as_str());
                    }
                    element = element.child(analysis);
                }
            }

            if let Some(&first_id) = same_resource_as.get(&attachment_id) {
                element = element.attr("same_resource_as", first_id.to_string());
            }

            if let Some(hint) = part.extra_hint() {
                element = element.attr("hint", hint);
            }

            if let Some(caption) = attachment_caption(part) {
                element = element.attr("caption", caption);
            }

            if let TelegramContentPart::Voice { meta: Some(m), .. } = part {
                element = element.attr("prompt", &m.prompt);
            }

            Some(element)
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
