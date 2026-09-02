use crate::agentcore::render::elements::{AttrValue, Node};

pub fn render(node: &Node) -> String {
    let mut out = String::new();
    render_node(node, &mut out, 0, false);
    out
}

pub fn render_pretty(node: &Node) -> String {
    let mut out = String::new();
    render_node(node, &mut out, 0, true);
    out
}

fn render_node(node: &Node, out: &mut String, depth: usize, pretty: bool) {
    match node {
        Node::Elem {
            tag,
            attrs,
            children,
        } => render_elem(tag, attrs, children, out, depth, pretty),
        Node::Text(t) => {
            if pretty {
                out.push_str(&"  ".repeat(depth));
            }
            escape_text(t, out);
            if pretty {
                out.push('\n');
            }
        }
    }
}

fn render_elem(
    tag: &str,
    attrs: &indexmap::IndexMap<String, AttrValue>,
    children: &[Node],
    out: &mut String,
    depth: usize,
    pretty: bool,
) {
    if pretty {
        out.push_str(&"  ".repeat(depth));
    }
    out.push('<');
    out.push_str(tag);
    for (key, value) in attrs {
        match value {
            // bool / null 是标记属性：存在即真，无值输出
            AttrValue::Bool(true) | AttrValue::Null => {
                out.push(' ');
                out.push_str(key);
            }
            AttrValue::Bool(false) => {}
            _ => {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                escape_attr(&value.to_string(), out);
                out.push('"');
            }
        }
    }

    if children.is_empty() {
        out.push_str("/>");
        if pretty {
            out.push('\n');
        }
        return;
    }

    out.push('>');
    if pretty {
        out.push('\n');
    }
    for child in children {
        render_node(child, out, depth + 1, pretty);
    }
    if pretty {
        out.push_str(&"  ".repeat(depth));
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    if pretty {
        out.push('\n');
    }
}

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_attrs_render_bare() {
        let node = Node::tag("msg")
            .attr("id", 1i64)
            .attr("own", true)
            .attr("urgent", AttrValue::Null);
        assert_eq!(render(&node), r#"<msg id="1" own urgent/>"#);
    }

    #[test]
    fn text_and_attr_escaped() {
        let node = Node::tag("t").child(Node::Text("a & b".into()));
        assert_eq!(render(&node), "<t>a &amp; b</t>");
    }
}
