//! JSON 渲染器

use crate::agent::render::elements::{Item, RenderElement, Section};

pub fn render_pretty(element: &RenderElement) -> String {
    let value = to_value(element);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

pub fn render(element: &RenderElement) -> String {
    let value = to_value(element);
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

fn to_value(element: &RenderElement) -> serde_json::Value {
    match element {
        RenderElement::Section(s) => section_to_json(s),
        RenderElement::Item(i) => item_to_json(i),
        RenderElement::Text(t) => serde_json::Value::String(t.content.clone()),
        RenderElement::KeyValue(kv) => {
            let mut map = serde_json::Map::new();
            map.insert(kv.key.clone(), serde_json::Value::String(kv.value.clone()));
            serde_json::Value::Object(map)
        }
        RenderElement::Empty => serde_json::Value::Null,
    }
}

fn section_to_json(section: &Section) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in &section.attrs {
        map.insert(k.clone(), serde_json::to_value(v).unwrap_or_default());
    }
    if !section.children.is_empty() {
        map.insert(
            "children".to_string(),
            serde_json::Value::Array(section.children.iter().map(to_value).collect()),
        );
    }
    serde_json::Value::Object(map)
}

fn item_to_json(item: &Item) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "_tag".to_string(),
        serde_json::Value::String(item.tag.clone()),
    );
    for (k, v) in &item.attrs {
        map.insert(k.clone(), serde_json::to_value(v).unwrap_or_default());
    }
    if let Some(c) = &item.content {
        map.insert("content".to_string(), serde_json::Value::String(c.clone()));
    }
    if !item.children.is_empty() {
        map.insert(
            "children".to_string(),
            serde_json::Value::Array(item.children.iter().map(to_value).collect()),
        );
    }
    serde_json::Value::Object(map)
}
