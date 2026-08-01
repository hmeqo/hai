use crate::{
    agent::personality::render::personality_context, agentcore::skills::SkillManager,
    config::schema::AgentConfig, domain::model::ChatType,
};

pub const SYSTEM_PROMPT: &str = r#"
## 怎么互动

你是通过工具来交互的：

- 想好了要说 → 发消息
- 拿不准、还要再想想 → 再想想
- 感觉对方还在打字、话没说完 → 等一下
- 没什么要说的 → 结束

## 次要职责

- 聊到值得记的内容（关于谁的信息、有用的结论、技巧）→ 随时记下来
- 开始讨论一个之前没聊过的主题 → 创建话题，方便以后关联
- 一个话题聊完了 → 归档整理摘要，方便以后回顾
- 提到之前讨论过的事 → 先翻看已有记忆再回应
- 记错了或情况变了 → 更新或删除已有记录
"#;

const SEPARATOR: &str = "\n----------------\n";

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

/// 系统提示词组装器。
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

    pub fn personality(mut self, config: &AgentConfig) -> Self {
        self.sections.push(Section {
            heading: Some("# 人格"),
            content: personality_context(&config.personality),
        });
        self
    }

    pub fn system_prompt(mut self, config: &AgentConfig) -> Self {
        let s = &config.context.system_prompt;
        if !s.is_empty() {
            self.sections.push(Section {
                heading: None,
                content: s.clone(),
            });
        }
        self
    }

    pub fn chat_type(mut self, config: &AgentConfig, chat_type: ChatType) -> Self {
        let extra = match chat_type {
            ChatType::Private => &config.context.private_prompt,
            _ => &config.context.group_prompt,
        };
        if !extra.is_empty() {
            let heading = match chat_type {
                ChatType::Private => Some("# 私聊" as &'static str),
                ChatType::Group | ChatType::Supergroup => Some("# 群聊" as &'static str),
                ChatType::Channel => None,
            };
            self.sections.push(Section {
                heading,
                content: extra.clone(),
            });
        }
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
