use quick_xml::{
    Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};

use crate::agentcore::render::elements::{AttrValue, Node};

pub fn render(node: &Node) -> String {
    let mut writer = Writer::new(Vec::new());
    render_node(node, &mut writer);
    String::from_utf8(writer.into_inner()).unwrap()
}

pub fn render_pretty(node: &Node) -> String {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    render_node(node, &mut writer);
    String::from_utf8(writer.into_inner()).unwrap()
}

fn render_node(node: &Node, writer: &mut Writer<Vec<u8>>) {
    match node {
        Node::Elem {
            tag,
            attrs,
            children,
        } => render_elem(tag, attrs, children, writer),
        Node::Text(t) => {
            let _ = writer.write_event(Event::Text(BytesText::new(t)));
        }
    }
}

fn render_elem(
    tag: &str,
    attrs: &indexmap::IndexMap<String, AttrValue>,
    children: &[Node],
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
            render_node(child, writer);
        }
        let _ = writer.write_event(Event::End(BytesEnd::new(tag)));
    }
}
