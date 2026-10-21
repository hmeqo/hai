use crate::{
    agentcore::{Language, LanguageName, PerCatalog, skills::SkillManager},
    domain::model::ChatType,
};

pub const SYSTEM_PROMPT: &str = r#"
## The interface

You are a character living inside a chat app, the same person from one turn to the next. What wakes you is what has happened since you last looked — not a prompt that arrived for you to answer:

- `<conversation>` — the conversation window, with a `<separator>new messages</separator>` divider: **everything above it is behind you, everything below it is new**
- `<date>` — a date divider; `<msg from at>` — someone else's message; `<msg own>` — a message of yours; `<reference>` — a quoted reply preview

## How you act

A turn is you noticing what has happened; a message is you speaking in the moment, not handing something over — it is what you have to say, your read of what is going on, for the people in front of you, not a survey of the subject. Most turns end with you having said nothing, and nothing has to be complete; what you did not say is not owed to anyone. When you do speak, the latest thing that settles it is the whole of it — not the working behind it, not the advice it implies, and not what you turned up on the way. A batch of messages is not a list to work through: one of them is what you would speak to, and often none is. No lead-in, no wrapping-up offer — advice nobody asked for is not a gift, and what they already know is not yours to explain back.

The only way to touch the world is through the interface the chat app gives you, and everything marked `own` is yours. What you want to say leaves through the input box: one call to `send_message`, whose `kind` selects what goes out — that call's own schema lists the kinds this chat can send and what each one takes. A picture you generate is written into the workspace first; send it yourself if it should reach the chat.

**Only what you send through `send_message` reaches the chat**; your response text stays empty. The other tools extend your reach.

`skip` ends the turn.
"#;

const SEPARATOR: &str = "\n----------------\n";

const STATEMENT: PerCatalog<fn(LanguageName) -> String> = PerCatalog {
    zh: statement_zh,
    en: statement_en,
};

fn statement_zh(name: LanguageName) -> String {
    format!("你的母语是{}。", name.as_str())
}

fn statement_en(name: LanguageName) -> String {
    format!("Your native language is {}.", name.as_str())
}

struct Section {
    heading: Option<&'static str>,
    content: String,
}

impl Section {
    fn render(self) -> Option<String> {
        let content = self.content.trim();
        if content.is_empty() {
            return None;
        }
        let body = match self.heading {
            Some(h) => format!("{h}\n\n{content}"),
            None => content.to_string(),
        };
        Some(body)
    }
}

pub struct SystemPromptBuilder {
    sections: Vec<Section>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            sections: vec![Section {
                heading: None,
                content: SYSTEM_PROMPT.to_owned(),
            }],
        }
    }

    /// Replaces the interface text the builder starts with: a pass that does not speak through the
    /// chat writes its output as the response text instead.
    pub fn contract(mut self, text: &str) -> Self {
        self.sections[0].content = text.to_owned();
        self
    }

    pub fn personality(mut self, name: &str, soul: &str) -> Self {
        self.sections.push(Section {
            heading: Some("# Character"),
            content: format!("You are {name}.\n{soul}"),
        });
        self
    }

    pub fn agents(mut self, text: &str) -> Self {
        if !text.is_empty() {
            self.sections.push(Section {
                heading: None,
                content: text.to_owned(),
            });
        }
        self
    }

    pub fn environment(mut self, text: &str) -> Self {
        if !text.is_empty() {
            self.sections.push(Section {
                heading: Some("# Environment"),
                content: text.to_owned(),
            });
        }
        self
    }

    pub fn chat_type(mut self, group: &str, private: &str, chat_type: ChatType) -> Self {
        let extra = match chat_type {
            ChatType::Private => private,
            _ => group,
        };
        if !extra.is_empty() {
            let heading = match chat_type {
                ChatType::Private => Some("# Private chat" as &'static str),
                ChatType::Group => Some("# Group chat" as &'static str),
                ChatType::Channel => None,
            };
            self.sections.push(Section {
                heading,
                content: extra.to_owned(),
            });
        }
        self
    }

    pub fn language(mut self, language: Language) -> Self {
        let catalog = language.text_catalog();
        let statement = (STATEMENT.get(catalog))(language.names().get(catalog));
        self.sections.push(Section {
            heading: Some("# Language"),
            content: statement,
        });
        self
    }

    pub fn skills(mut self, skill_manager: &SkillManager) -> Self {
        if let Some(p) = skill_manager.discovery_prompt() {
            self.sections.push(Section {
                heading: None,
                content: p,
            });
        }
        self
    }

    pub fn build(self) -> String {
        self.sections
            .into_iter()
            .filter_map(Section::render)
            .collect::<Vec<_>>()
            .join(SEPARATOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_section_names_the_language_in_its_own_set() {
        let chinese = SystemPromptBuilder::new().language(Language::Zh).build();
        assert!(chinese.contains("# Language"), "{chinese}");
        assert!(chinese.contains("你的母语是中文。"), "{chinese}");

        let japanese = SystemPromptBuilder::new().language(Language::Ja).build();
        assert!(
            japanese.contains("Your native language is Japanese."),
            "{japanese}"
        );

        let british = SystemPromptBuilder::new().language(Language::EnGb).build();
        assert!(
            british.contains("Your native language is British English."),
            "{british}"
        );
    }
}
