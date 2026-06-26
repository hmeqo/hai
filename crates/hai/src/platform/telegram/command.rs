use teloxide::utils::command::BotCommands;

pub(super) const MAJOR_HELP_TEXT: &str = r#"
你现在正与一位 AI 助手（Agent）对话。这个 Agent 拥有以下能力：
- 借助多模态理解并分析图片、视频、音频等附件
- 管理对话历史并持续累积记忆
- 自主识别和推进话题
- 向您请教和学习
- 随时随地请求总结或梳理讨论
- 可在 Telegram 上使用
"#;

#[derive(Debug, BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub(crate) enum Command {
    #[command(description = "启动机器人")]
    Start,
    #[command(description = "获取帮助")]
    Help,
    #[command(description = "查看 Agent 状态")]
    Status,
    #[command(description = "整理记忆和主题")]
    OrganizeMemory,
}
