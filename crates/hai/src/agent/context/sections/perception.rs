use crate::{agentcore::render::Node, domain::entity::Perception};

pub fn perception_item(p: &Perception) -> Node {
    let mut el = Node::tag("perception").attr("id", p.id);
    if let Some(prompt) = &p.prompt {
        el = el.attr("prompt", prompt.as_str());
    }
    el.child(Node::text(&p.content))
}
