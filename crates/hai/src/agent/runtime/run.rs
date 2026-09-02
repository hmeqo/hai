use std::{collections::HashSet, sync::Arc};

use genai::chat::ChatMessage;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{
    context::TurnContext,
    engine::AgentEngine,
    event::Inbox,
    react::{LoopMode, ReactLoopConfig, ReactLoopOutput, ReactTurn, run_react_loop},
    session::{HeartbeatTask, TurnInput},
    types::{BusySignal, Messages, TurnOutput},
};
use crate::{
    agent::{
        context,
        link::BuiltContext,
        tools::{get_main_agent_tools, get_wrap_up_tools},
    },
    domain::{
        model::Message,
        vo::{AgentEventPayload, MessageId, StepNumber, StepOutput, TurnEndReason, TurnNumber},
    },
    error::Result,
};

/// 收尾 任务指令（作为 user 消息注入——社区做法：会话结束总结 = 任务式指令 + 提取目标；
/// 质量靠指令与 <summary> 结构约束，不靠长度门槛）。
/// 收尾 系统角色（简短；详细任务指令在 user 消息 WRAP_UP_PROMPT）。
const WRAP_UP_PROMPT_SHORT: &str = "你是整理记录员，正在归档一段对话。按用户消息中的指令执行。";

const WRAP_UP_PROMPT: &str = "\
本次对话即将归档。你是整理记录员，请把这段对话整理成一份留存摘要，供未来的自己续接新会话时理解发生了什么。

步骤：
1. 先调用工具整理：值得长期记住的信息写入记忆（record_memory）；对话中的话题归档或更新（close_topic / append_topic_summary）；不准确的记忆修正、不再相关的清理
2. 整理完成后输出留存摘要。摘要是对**历史对话**的总结，未来的你只能靠它判断事件新旧——因此必须包含时间线信息：
- **时间范围**：先标注这段对话覆盖的时间范围（最早的消息日期 → 收尾时），用对话消息里的 `<date>` 分隔线和 `<msg at>` 时间判断；同一天的对话注明「当天」即可
- 关键事实：按时间顺序组织（早的先列），每条标注发生时间——跨天用日期（如「8月12日：讨论了…」），同天多条可只标「随后/紧接着」；让读者能分辨哪些是较早的、哪些是最新的
- 用户偏好：用户表达过的偏好、风格要求、注意事项
- 未决事项：未完成的事、待办、下次要继续的话题（未决事项默认是最近的——明确标注「尚未开始/进行中」避免误判）

用 <summary> 标签包裹摘要正文：
<summary>
时间范围：...
关键事实：...
用户偏好：...
未决事项：...
</summary>

不要输出对话式内容（如\"好的\"\"已就位\"\"等待下次对话\"），不要向用户发消息。整理完毕直接输出摘要。";

/// 执行引擎。只做 LLM 交互，不关心 session 生命周期。
pub(super) struct AgentRuntime {
    engine: AgentEngine,
    pub(super) handler: Arc<dyn crate::agent::link::PlatformHandler>,
    pub(super) shell: Arc<tokio::sync::Mutex<super::shell::ShellRuntime>>,
}

impl AgentRuntime {
    pub fn new(
        engine: &AgentEngine,
        handler: Arc<dyn crate::agent::link::PlatformHandler>,
        shell: Arc<tokio::sync::Mutex<super::shell::ShellRuntime>>,
    ) -> Self {
        Self {
            engine: engine.clone(),
            handler,
            shell,
        }
    }

    pub async fn build_prompt(
        &self,
        ctx: &TurnContext,
        messages: &[Message],
        shown_memory_ids: &HashSet<Uuid>,
        shown_topic_ids: &HashSet<Uuid>,
        is_first: bool,
    ) -> Result<BuiltContext> {
        context::build_prompt(ctx, messages, shown_memory_ids, shown_topic_ids, is_first).await
    }

    /// rx 收到 `BusySignal::Turn` 或 `BusySignal::TurnFailed`。
    pub fn spawn_turn(
        &self,
        ctx: TurnContext,
        payload: TurnInput,
        inbox: Inbox,
        turn_number: TurnNumber,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<BusySignal>,
    ) {
        let handler = ctx.handler.clone();
        let chat_id = ctx.chat_id;
        let message_ids = payload.message_ids.clone();
        let event_bus = self.engine.app.event_bus.clone();

        let aux = &ctx.app.cfg.auxiliary;
        let mut enabled_parsers = Vec::new();
        if aux.audio.as_ref().is_none_or(|b| b.enabled) {
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Audio.name());
        }
        if aux.vision.as_ref().is_none_or(|b| b.enabled) {
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Video.name());
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Image.name());
        }
        let sandbox_image = if ctx.app.cfg.sandbox.enabled {
            Some(ctx.app.cfg.sandbox.image.clone())
        } else {
            None
        };

        let turn = ReactTurn::new(&self.engine, &ctx, payload.messages, inbox, turn_number);

        let (tx, rx) = oneshot::channel();

        let engine = self.engine.clone();
        let handle = tokio::spawn(async move {
            let started_at = tokio::time::Instant::now();

            let _hb = HeartbeatTask::spawn(handler, chat_id);

            let mut tools =
                get_main_agent_tools(&ctx.tool_ctx(), &enabled_parsers, sandbox_image.as_deref());
            tools.extend(engine.mcp_manager.list_all_tools().await);

            let result = run_react_loop(turn, tools).await;

            let elapsed = started_at.elapsed();

            let signal = match result {
                Ok(output) => {
                    let ReactLoopOutput {
                        steps,
                        messages,
                        steered,
                    } = output;
                    let tool_calls: usize = steps.iter().map(|t| t.tool_calls.len()).sum();
                    let has_spoken = steps.iter().flat_map(|t| &t.tool_calls).any(|tc| {
                        matches!(
                            tc.tool_name.as_str(),
                            "send_message" | "send_voice" | "generate_image"
                        )
                    });

                    // 上下文大小 = 最后一次 LLM 调用的输入（react loop 消息累积后的真实输入）
                    let context_tokens: u32 = steps.last().map(|t| t.prompt_tokens).unwrap_or(0);

                    // react loop 每轮必 commit 至少一个 Step——steps.last() 恒 Some
                    let last = steps.last().map(|t| StepOutput {
                        turn: turn_number,
                        step: StepNumber::from(steps.len()),
                        reasoning: t.reasoning.clone(),
                        response: t.response.clone(),
                    });
                    let last = last.expect("invariant: react loop commits >= 1 step");

                    // Turn 结束统一事件：reason 表达三态（Success / Steered / Failed）
                    let reason = if steered.is_some() {
                        TurnEndReason::Steered {
                            output: last,
                            tool_calls,
                            elapsed_ms: elapsed.as_millis() as u64,
                            context_tokens,
                            has_spoken,
                        }
                    } else {
                        TurnEndReason::Success {
                            output: last,
                            tool_calls,
                            elapsed_ms: elapsed.as_millis() as u64,
                            context_tokens,
                            has_spoken,
                        }
                    };
                    event_bus.emit(
                        chat_id,
                        AgentEventPayload::TurnEnded {
                            turn: turn_number,
                            reason,
                        },
                    );

                    // 只标记本 turn gather 拉取实际处理的消息；turn 期间新到达的
                    // 保持 unread（后续 turn 处理时渲染"新消息"分隔线，见 topics/session.md）
                    let _ = mark_seen(&message_ids, &ctx.db).await;

                    let output = TurnOutput { messages, steps };
                    match steered {
                        Some(events) => BusySignal::Steered(output, events),
                        None => BusySignal::Turn(output),
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        %chat_id,
                        elapsed_secs = %elapsed.as_secs_f64(),
                        error = %e,
                        "Agent turn failed"
                    );
                    event_bus.emit(
                        chat_id,
                        AgentEventPayload::TurnEnded {
                            turn: turn_number,
                            reason: TurnEndReason::Failed {
                                error: e.to_string(),
                            },
                        },
                    );
                    BusySignal::TurnFailed
                }
            };

            let _ = tx.send(signal);
        });

        (handle, rx)
    }

    /// rx 收到 `BusySignal::WrapUp` 或 `BusySignal::WrapUpFailed`。
    /// 收尾 是受限工具集的 react loop：先整理记忆/话题，再输出压缩总结。
    pub fn spawn_wrap_up(
        &self,
        ctx: TurnContext,
        messages: Messages,
        inbox: Inbox,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<BusySignal>,
    ) {
        let cfg = &self.engine.app.cfg.agent;
        // system 保留对话身份（build_react_config 产物）；收尾任务指令作为 user 消息追加——
        // 社区做法：会话结束总结 = 任务式指令注入（比替换 system 效力强）
        let mut wrap_messages = messages;
        wrap_messages.push(ChatMessage::user(WRAP_UP_PROMPT.to_string()));
        let mut turn = ReactTurn::new(
            &self.engine,
            &ctx,
            wrap_messages,
            inbox,
            TurnNumber::from(0),
        );
        turn.config = ReactLoopConfig {
            system_prompt: WRAP_UP_PROMPT_SHORT.to_string(),
            options: ReactLoopConfig::build_chat_options(cfg),
        };
        turn.mode = LoopMode::WrapUp;
        turn.steering = false; // 收尾 不响应 steering：事件留在 inbox，收尾完成后处理

        let (tx, rx) = oneshot::channel();

        let event_bus = self.engine.app.event_bus.clone();
        let chat_id = ctx.chat_id;

        let handle = tokio::spawn(async move {
            event_bus.emit(chat_id, AgentEventPayload::WrapUpStarted);
            let tools = get_wrap_up_tools(&ctx.tool_ctx());
            let result = run_react_loop(turn, tools).await;
            let signal = match result {
                Ok(output) => {
                    // 摘要提取：优先 <summary> 标签内容（prompt 结构约束）；无标签则取非空回复兜底。
                    // 不设长度门槛——质量由任务指令与结构约束保证（社区共识，无长度过滤做法）。
                    let text = output
                        .steps
                        .iter()
                        .rev()
                        .filter_map(|t| extract_summary(&t.response))
                        .next()
                        .or_else(|| {
                            output
                                .steps
                                .iter()
                                .rev()
                                .find(|t| !t.response.trim().is_empty())
                                .map(|t| t.response.clone())
                        });
                    match text {
                        Some(t) => BusySignal::WrapUp(t),
                        None => {
                            BusySignal::WrapUpFailed("no summary (模型未输出实质压缩摘要)".into())
                        }
                    }
                }
                Err(e) => BusySignal::WrapUpFailed(e.to_string()),
            };
            let _ = tx.send(signal);
        });

        (handle, rx)
    }
}

async fn mark_seen(msg_ids: &[MessageId], db: &crate::domain::service::DbServices) -> Result<()> {
    if msg_ids.is_empty() {
        return Ok(());
    }
    db.message.mark_unread_seen(msg_ids).await?;
    Ok(())
}

/// 提取 <summary>...</summary> 标签内的摘要正文（prompt 要求结构输出；标签缺失返回 None）。
fn extract_summary(response: &str) -> Option<String> {
    let start = response.find("<summary>")? + "<summary>".len();
    let rest = &response[start..];
    let end = rest.find("</summary>").unwrap_or(rest.len());
    let body = rest[..end].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}
