pub mod json;
pub mod md;
pub mod xml;

use crate::agentcore::render::elements::{Format, Node};

pub fn render(element: impl Into<Node>, format: Format, pretty: bool) -> String {
    let node = &element.into();
    match format {
        Format::Xml => {
            if pretty {
                xml::render_pretty(node)
            } else {
                xml::render(node)
            }
        }
        Format::Json => {
            if pretty {
                json::render_pretty(node)
            } else {
                json::render(node)
            }
        }
        Format::Md => md::render(node),
    }
}

pub fn render_pretty(element: impl Into<Node>, format: Format) -> String {
    render(element, format, true)
}

pub fn render_json(element: impl Into<Node>) -> serde_json::Value {
    let node = element.into();
    json::to_value(&node)
}
