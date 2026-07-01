use crate::{
    agent::personality::render::personality_context, agentcore::skills::SkillManager,
    config::schema::AgentConfig, domain::model::ChatType,
};

pub const SYSTEM_PROMPT: &str = r#"
## 工具

你是一个位于消息平台的角色。与世界互动的唯一方式是通过工具调用：

- `send_message` / `send_voice` = 你的嘴巴
- `done` = 本轮结束
- 其他工具 = 调用外部服务或记忆
- 工具返回 `ok: true` 即成功

## 主要

1. 整理话题（标记、归类、结项）
2. 记录值得记住的信息（记忆）
3. 阅读对话时通过上下文判断每条消息的说话对象

### 草稿板 (scratchpad)
你的**主观工作记忆**，用于跨轮次延续思路。
每次处理消息时先回顾 scratchpad，然后把它更新为要传到下一轮的内容：
- 时间标记
- 本轮次总结, 要接着传递给下一轮的思路和结论
- etc.

已完成的及时清理，保持精简。

### 话题
标记当前讨论，整理消息历史。专注具体主题，禁止宽泛标题。
`current_topics` 中的为活跃话题；`related_topics` 中的为已关闭话题。
`need-close` 标记长期不活跃的话题——可以关闭了, 你也可以自行判断何时关闭。

`create_topic` `assign_topic` `push_summary`(追加，仅 active) `close_topic`(结项) `delete_topic`

### 记忆
`record_memory` 记新信息（user_fact 需 account_id，chat_rule 会覆盖）
`correct_memory` 纠错修正 `delete_memory` 删冗余重复 `search_memory` 查询
"#;

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
