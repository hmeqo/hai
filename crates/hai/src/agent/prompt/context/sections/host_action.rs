use crate::{
    agent::prompt::text::{LanguageText, PerCatalog, TextCatalog},
    agentcore::render::Node,
    config::conversation::Language,
    domain::vo::HostAction,
};

const PERSONA: PerCatalog<&'static str> = PerCatalog {
    zh: "用户更新了人设。",
    en: "The user updated the persona.",
};

const MODEL: PerCatalog<&'static str> = PerCatalog {
    zh: "管理员切换了模型。",
    en: "An administrator switched the model.",
};

const LANGUAGE: PerCatalog<&'static str> = PerCatalog {
    zh: "用户切换了语言。",
    en: "The user switched the language.",
};

const CONTENT_POLICY: PerCatalog<&'static str> = PerCatalog {
    zh: "用户调整了内容尺度。",
    en: "The user adjusted the content policy.",
};

/// The `<host_actions>` section: one `<action kind="...">` per setting the host changed since the
/// Conversation's last Turn. Empty when nothing changed, and the builder then drops it.
pub(super) fn host_actions_element(actions: &[HostAction], language: Language) -> Node {
    let catalog = language.text_catalog();
    let nodes: Vec<Node> = actions
        .iter()
        .map(|action| {
            Node::tag("action")
                .attr("kind", action.kind())
                .child(Node::text(describe(*action, catalog)))
        })
        .collect();
    Node::tag("host_actions").children(nodes)
}

fn describe(action: HostAction, catalog: TextCatalog) -> &'static str {
    match action {
        HostAction::PersonaChanged => PERSONA.get(catalog),
        HostAction::ModelChanged => MODEL.get(catalog),
        HostAction::LanguageChanged => LANGUAGE.get(catalog),
        HostAction::ContentPolicyChanged => CONTENT_POLICY.get(catalog),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentcore::render::render_xml;

    /// Each action names its setting and says what happened in the language the context is written
    /// in; the section is empty when nothing happened.
    #[test]
    fn host_actions_render_one_element_each() {
        let rendered = render_xml(&host_actions_element(
            &[HostAction::PersonaChanged, HostAction::ContentPolicyChanged],
            Language::En,
        ));
        assert!(rendered.starts_with("<host_actions>"), "{rendered}");
        assert!(rendered.contains("<action kind=\"persona\">"), "{rendered}");
        assert!(
            rendered.contains("The user updated the persona."),
            "{rendered}"
        );
        assert!(
            rendered.contains("<action kind=\"content-policy\">"),
            "{rendered}"
        );
        assert!(
            rendered.contains("The user adjusted the content policy."),
            "{rendered}"
        );

        let zh = render_xml(&host_actions_element(
            &[HostAction::PersonaChanged],
            Language::Zh,
        ));
        assert!(zh.contains("用户更新了人设。"), "{zh}");

        assert!(
            host_actions_element(&[], Language::En).is_empty(),
            "an empty section is dropped by the builder"
        );
    }
}
