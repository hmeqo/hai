use quick_xml::{
    Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};

use crate::agentcore::render::elements::{AttrValue, Item, KeyValue, RenderElement};

pub fn render(element: &RenderElement) -> String {
    let mut writer = Writer::new(Vec::new());
    render_element(element, &mut writer);
    String::from_utf8(writer.into_inner()).unwrap()
}

pub fn render_pretty(element: &RenderElement) -> String {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    render_element(element, &mut writer);
    String::from_utf8(writer.into_inner()).unwrap()
}

fn render_element(element: &RenderElement, writer: &mut Writer<Vec<u8>>) {
    match element {
        RenderElement::Section(s) => render_tag(&s.tag, &s.attrs, &s.children, writer),
        RenderElement::Item(i) => render_item(i, writer),
        RenderElement::Text(t) => {
            let _ = writer.write_event(Event::Text(BytesText::new(&t.content)));
        }
        RenderElement::KeyValue(kv) => render_kv(kv, writer),
        RenderElement::Empty => {}
    }
}

fn render_tag(
    tag: &str,
    attrs: &indexmap::IndexMap<String, AttrValue>,
    children: &[RenderElement],
    writer: &mut Writer<Vec<u8>>,
) {
    let mut elem = BytesStart::new(tag);
    for (key, value) in attrs {
        match value {
            AttrValue::Bool(true) => {
                elem.push_attribute((key.as_str(), "true"));
            }
            AttrValue::Bool(false) => {}
            _ => {
                elem.push_attribute((key.as_str(), value.to_string().as_str()));
            }
        }
    }

    if children.is_empty() {
        let _ = writer.write_event(Event::Empty(elem));
    } else {
        let _ = writer.write_event(Event::Start(elem));
        for child in children {
            render_element(child, writer);
        }
        let _ = writer.write_event(Event::End(BytesEnd::new(tag)));
    }
}

fn render_item(item: &Item, writer: &mut Writer<Vec<u8>>) {
    let mut elem = BytesStart::new(&item.tag);
    for (key, value) in &item.attrs {
        match value {
            AttrValue::Bool(true) => {
                elem.push_attribute((key.as_str(), "true"));
            }
            AttrValue::Bool(false) => {}
            _ => {
                elem.push_attribute((key.as_str(), value.to_string().as_str()));
            }
        }
    }

    if let Some(content) = &item.content {
        let _ = writer.write_event(Event::Start(elem));
        let _ = writer.write_event(Event::Text(BytesText::new(content)));
        let _ = writer.write_event(Event::End(BytesEnd::new(&item.tag)));
    } else if item.children.is_empty() {
        let _ = writer.write_event(Event::Empty(elem));
    } else {
        let _ = writer.write_event(Event::Start(elem));
        for child in &item.children {
            render_element(child, writer);
        }
        let _ = writer.write_event(Event::End(BytesEnd::new(&item.tag)));
    }
}

fn render_kv(kv: &KeyValue, writer: &mut Writer<Vec<u8>>) {
    let _ = writer.write_event(Event::Start(BytesStart::new(&kv.key)));
    let _ = writer.write_event(Event::Text(BytesText::new(&kv.value)));
    let _ = writer.write_event(Event::End(BytesEnd::new(&kv.key)));
}
