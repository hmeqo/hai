use teloxide::utils::command::{BotCommands, ParseError};

/// `/digest [N]`：可选天数，缺省 7。
fn parse_digest_days(input: String) -> Result<(u32,), ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok((7,));
    }
    let n = trimmed
        .parse::<u32>()
        .map_err(|_| ParseError::IncorrectFormat("expected a number of days".into()))?;
    Ok((n.max(1),))
}

#[derive(Debug, BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub(crate) enum Command {
    #[command(description = "启动机器人")]
    Start,
    #[command(description = "查看 Agent 状态")]
    Status,
    #[command(description = "整理记忆和主题")]
    OrganizeMemory,
    #[command(description = "解释当前上下文中的内容")]
    Explain,
    #[command(description = "总结最近聊过的值得注意的内容", parse_with = parse_digest_days)]
    Digest(u32),
}
