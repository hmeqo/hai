use crate::{
    agent::{personality::render::personality_context, prompts::SYSTEM_PROMPT},
    agentcore::skills::SkillManager,
    config::schema::AgentConfig,
    domain::model::ChatType,
};

/// 系统提示词组装器。
///
/// 将静态提示词、性格、配置 extra prompt、skills 分层拼接。
pub struct SystemPromptBuilder {
    parts: Vec<String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            parts: vec![SYSTEM_PROMPT.to_owned()],
        }
    }

    pub fn personality(mut self, config: &AgentConfig) -> Self {
        self.parts.push(personality_context(&config.personality));
        self
    }

    pub fn system_prompt(mut self, config: &AgentConfig) -> Self {
        let s = &config.context.system_prompt;
        if !s.is_empty() {
            self.parts.push(s.clone());
        }
        self
    }

    pub fn chat_type(mut self, config: &AgentConfig, chat_type: ChatType) -> Self {
        let extra = match chat_type {
            ChatType::Private => &config.context.private_prompt,
            _ => &config.context.group_prompt,
        };
        if !extra.is_empty() {
            self.parts.push(extra.clone());
        }
        self
    }

    pub fn skills(mut self, skill_manager: &SkillManager) -> Self {
        if let Some(p) = skill_manager.discovery_prompt() {
            self.parts.push(p);
        }
        self
    }

    pub fn build(self) -> String {
        self.parts.join("\n\n")
    }
}
