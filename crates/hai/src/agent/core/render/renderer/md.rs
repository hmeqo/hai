//! Markdown 渲染器

use std::fmt::Write;

use crate::agent::render::elements::{AttrValue, Item, KeyValue, RenderElement, Section, Text};

fn format_attr(value: &AttrValue) -> String {
    match value {
        AttrValue::Null => "null".to_string(),
        AttrValue::String(s) => format!("\"{s}\""),
        AttrValue::Int(i) => i.to_string(),
        AttrValue::Float(f) => f.to_string(),
        AttrValue::Bool(b) => b.to_string(),
    }
}

fn format_attrs(attrs: &indexmap::IndexMap<String, AttrValue>) -> String {
    attrs
        .iter()
        .map(|(k, v)| format!("{{{k}:{}}}", format_attr(v)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn render(element: &RenderElement) -> String {
    let mut output = String::new();
    render_element(element, &mut output, 0);
    output.trim().to_string()
}

fn render_element(element: &RenderElement, output: &mut String, indent: usize) {
    match element {
        RenderElement::Section(s) => render_section(s, output, indent),
        RenderElement::Item(i) => render_item(i, output, indent),
        RenderElement::Text(t) => render_text(t, output, indent),
        RenderElement::KeyValue(kv) => render_kv(kv, output, indent),
        RenderElement::Empty => {}
    }
}

fn render_section(section: &Section, output: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);
    let _ = writeln!(output, "{prefix}**{}**", section.tag);
    if !section.attrs.is_empty() {
        let attrs = format_attrs(&section.attrs);
        let _ = writeln!(output, "{prefix}{attrs}");
    }
    for child in &section.children {
        render_element(child, output, indent + 1);
    }
}

fn render_item(item: &Item, output: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);
    let attrs = format_attrs(&item.attrs);
    let attrs_prefix = if attrs.is_empty() {
        String::new()
    } else {
        format!("{attrs} ")
    };

    if let Some(content) = &item.content {
        let _ = writeln!(output, "{prefix}- {attrs_prefix}```");
        let _ = writeln!(output, "{prefix}{content}");
        let _ = writeln!(output, "{prefix}```");
    } else if !item.children.is_empty() || !attrs_prefix.is_empty() {
        let _ = writeln!(output, "{prefix}- {attrs_prefix}**{}**", item.tag);
        for child in &item.children {
            render_element(child, output, indent + 1);
        }
    } else {
        let _ = writeln!(output, "{prefix}- *{}*", item.tag);
    }
}

fn render_text(text: &Text, output: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);
    let _ = writeln!(output, "{}{}", prefix, text.content);
}

fn render_kv(kv: &KeyValue, output: &mut String, indent: usize) {
    let prefix = "  ".repeat(indent);
    let _ = writeln!(output, "{}**{}:** {}", prefix, kv.key, kv.value);
}
