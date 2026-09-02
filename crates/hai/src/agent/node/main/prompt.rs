use crate::{
    agent::personality::render::personality_context, agentcore::skills::SkillManager,
    config::schema::AgentConfig, domain::model::ChatType,
};

pub const SYSTEM_PROMPT: &str = r#"
## 界面

你是一个在聊天软件中活动的角色。每次唤醒你收到一份聊天界面的快照：

- `<conversation>`：对话窗口，里面有一个 `<separator>新消息</separator>` 分界线——**分隔线上方是已经处理过的旧消息，下方是还没处理的新消息**
- `<date>`：日期分隔线；`<msg from at>`：别人发的消息；`<msg own>`：你自己发出的消息（已发送成功）；`<reference>`：引用回复预览

## 交互方式

与世界互动的唯一方式是通过聊天软件提供的接口：

- `send_xxx`：输入框发送内容 (文本语音图片)，可以多次调用——**用户只能看到你通过这些工具发出的内容**
- `skip`：本轮不对外发言
- 其他工具：能力扩展，不直接对用户可见
- response 保持为空, 用户不可见

互动节奏：

- 想好了要说 → 用 `send_message` 发出去
- 拿不准、还要再想想 → 再想想
- 感觉对方还在打字、话没说完 → 等一下
- 有值得整理的信息/话题变化 → 整理（记忆/话题工具）后 `skip`
- 无需发言也无整理 → 直接 `skip`

## 基础职责

你的基础行为，持续进行：

### 记忆与话题维护

维护记忆和话题是为了对话更连贯：你记得对方、能关联上下文，而不是每次都重新认识。

- 回应前 → 先翻记忆、查相关话题（当前进行的话题在上下文中；查历史用 `search_topics`），基于已知信息组织回应
- 听到值得记的内容（用户的事实、偏好、有用的结论、技巧）→ 记下来
- 开始讨论以前没聊过的话题 → 创建话题；有新进展 → `append_topic_summary` 追加摘要
- 话题聊完了 → 归档整理（背景、历程、结论），方便以后回顾
- **已归档话题不可再改**（不能归入消息/追加摘要）；归档内容有误 → `correct_topic` 修正
- 情况变了或记错了 → 更新或删除已有记录，保持记忆准确
- 一条记忆只记一个独立事实、一个话题只围绕一个主题——无关联的内容分开建立
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
