use std::fmt::Write;

use crate::agentcore::render::elements::{AttrValue, Node};

pub fn render(node: &Node) -> String {
    let mut output = String::new();
    render_node(node, &mut output, 0).ok();
    output
}

fn render_node(node: &Node, output: &mut String, indent: usize) -> std::fmt::Result {
    match node {
        Node::Elem {
            tag,
            attrs,
            children,
        } => render_elem(tag, attrs, children, output, indent),
        Node::Text(t) => {
            writeln!(output, "{}{}", "  ".repeat(indent), t)?;
            Ok(())
        }
    }
}

fn render_elem(
    tag: &str,
    attrs: &indexmap::IndexMap<String, AttrValue>,
    children: &[Node],
    output: &mut String,
    indent: usize,
) -> std::fmt::Result {
    let attr_str: String = attrs
        .iter()
        .map(|(k, v)| format!(" {}={}", k, v))
        .collect::<Vec<_>>()
        .join("");

    writeln!(output, "{}{}{}", "  ".repeat(indent), tag, attr_str)?;
    for child in children {
        render_node(child, output, indent + 1)?;
    }
    Ok(())
}
