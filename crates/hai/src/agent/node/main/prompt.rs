use crate::{
    agent::personality::render::personality_context, agentcore::skills::SkillManager,
    config::schema::AgentConfig, domain::model::ChatType,
};

pub const SYSTEM_PROMPT: &str = r#"
## 界面

你是一个在聊天软件中活动的角色。
每次唤醒时你收到一份聊天界面的快照，内容如下：

- <situation>：唤醒原因（谁发了消息、谁提到了你等）
- <environment>：你的身份信息和当前时间
- <chat>：当前打开的聊天窗口
- <accounts>：群聊参与者列表
- <conversation>：消息列表（按时间从旧到新排列，每一条就是界面中的一条消息）
  - <date value="今天/昨天/周三/7月19日"> 日期分隔线
  - <msg from="..." at="HH:MM"> 别人发的消息
  - <msg from="..." at="HH:MM" own> 你自己发出的消息（已发送成功）
  - <msg new> 还没处理的新消息
  - <separator/> 新旧消息分界（以上为已处理，以下为待处理）
  - <reference> 引用回复预览
- <related_memories>：你想起的相关记忆
- <current_topics>：对当前聊天的话题归类
- <scratchpad>：你的便签（跨轮次传递思路）

## 交互方式

- <reasoning> 是你的内心独白，别人看不见。它不产生任何外界效果。
- 只有使用工具才能对聊天界面产生影响：
  - `send_message` / `send_voice`：输入框打字发送，消息出现在对话中
  - `done`：无操作，结束本轮
  - 其他工具：能力扩展
- Final Response 必须为空（文本输出会被截获并报错）

## 主要

1. 整理话题（标记、归类、结项）
2. 记录值得记住的信息（记忆）

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
`record_memory` 记新信息 (user_fact 需 account_id)
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
