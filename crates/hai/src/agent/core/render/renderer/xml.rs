//! XML 渲染器

use std::fmt::Write;

use xmlformat::Formatter;

use crate::agent::render::elements::{AttrValue, Item, KeyValue, RenderElement};

pub fn render(element: &RenderElement) -> String {
    let mut output = String::new();
    render_element(element, &mut output);
    output
}

pub fn render_pretty(element: &RenderElement) -> String {
    format(render(element))
}

fn write_attrs(output: &mut String, attrs: &indexmap::IndexMap<String, AttrValue>) {
    for (key, value) in attrs {
        match value {
            AttrValue::Bool(true) => write!(output, " {key}").unwrap(),
            AttrValue::Bool(false) => continue,
            _ => {
                write!(output, " {key}=\"{value}\"").unwrap();
            }
        }
    }
}

fn render_element(element: &RenderElement, output: &mut String) {
    match element {
        RenderElement::Section(s) => render_tag(&s.tag, &s.attrs, &s.children, output),
        RenderElement::Item(i) => render_item(i, output),
        RenderElement::Text(t) => output.push_str(&t.content),
        RenderElement::KeyValue(kv) => render_kv(kv, output),
        RenderElement::Empty => {}
    }
}

fn render_tag(
    tag: &str,
    attrs: &indexmap::IndexMap<String, AttrValue>,
    children: &[RenderElement],
    output: &mut String,
) {
    output.push('<');
    output.push_str(tag);
    write_attrs(output, attrs);

    if children.is_empty() {
        output.push_str("/>");
    } else {
        output.push('>');
        for child in children {
            render_element(child, output);
        }
        write!(output, "</{tag}>").unwrap();
    }
}

fn render_item(item: &Item, output: &mut String) {
    output.push('<');
    output.push_str(&item.tag);
    write_attrs(output, &item.attrs);

    if let Some(content) = &item.content {
        output.push('>');
        output.push_str(content);
        write!(output, "</{}>", item.tag).unwrap();
    } else if item.children.is_empty() {
        output.push_str("/>");
    } else {
        output.push('>');
        for child in &item.children {
            render_element(child, output);
        }
        write!(output, "</{}>", item.tag).unwrap();
    }
}

fn render_kv(kv: &KeyValue, output: &mut String) {
    write!(output, "<{}>{}</{}>", kv.key, kv.value, kv.key).unwrap();
}

pub fn format(raw: String) -> String {
    let formatter = Formatter {
        indent: 2,
        ..Default::default()
    };
    formatter.format_xml(&raw).unwrap_or(raw)
}
