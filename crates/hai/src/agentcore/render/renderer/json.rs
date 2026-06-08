use crate::agentcore::render::elements::Node;

pub fn render_pretty(node: &Node) -> String {
    let value = node.to_json_value();
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

pub fn render(node: &Node) -> String {
    let value = node.to_json_value();
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

pub fn to_value(node: &Node) -> serde_json::Value {
    node.to_json_value()
}
