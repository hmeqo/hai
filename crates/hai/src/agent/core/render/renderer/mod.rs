pub mod json;
pub mod md;
pub mod xml;

use crate::agent::render::elements::{Format, RenderElement};

pub fn render(element: impl Into<RenderElement>, format: Format, pretty: bool) -> String {
    let element = element.into();
    match format {
        Format::Xml => {
            if pretty {
                xml::render_pretty(&element)
            } else {
                xml::render(&element)
            }
        }
        Format::Json => {
            if pretty {
                json::render_pretty(&element)
            } else {
                json::render(&element)
            }
        }
        Format::Md => md::render(&element),
    }
}

pub fn render_pretty(element: impl Into<RenderElement>, format: Format) -> String {
    render(element, format, true)
}

pub fn render_json(element: impl Into<RenderElement>) -> String {
    render_pretty(element, Format::Json)
}
