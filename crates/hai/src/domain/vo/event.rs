use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::{ChatId, StepNumber, TurnNumber};

/// 一次 LLM 调用的输出（Step 级；运行期记录，审计走 events 表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutput {
    pub turn: TurnNumber,
    pub step: StepNumber,
    pub reasoning: Option<String>,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub chat_id: ChatId,
    pub payload: AgentEventPayload,
}

impl AgentEvent {
    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|e| {
            tracing::warn!(
                error = %e, kind = %self.payload.kind(),
                "failed to serialize agent event"
            );
            serde_json::Value::Null
        })
    }
}

/// Turn 结束原因（三态定案映射；TurnEnded 唯一结束事件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnEndReason {
    /// 正常完成（含无发言/无活动）。
    Success {
        output: StepOutput,
        tool_calls: usize,
        elapsed_ms: u64,
        context_tokens: u32,
        has_spoken: bool,
    },
    /// 新事件打断（steering）：提前正常结束，已处理内容生效，立即增量续跑新 Turn。
    Steered {
        output: StepOutput,
        tool_calls: usize,
        elapsed_ms: u64,
        context_tokens: u32,
        has_spoken: bool,
    },
    /// 异常——零状态副作用，唯一重来路径。
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, IntoStaticStr)]
#[serde(tag = "event", rename_all = "kebab-case")]
#[strum(serialize_all = "snake_case")]
pub enum AgentEventPayload {
    TurnStarted {
        turn: TurnNumber,
        reason: String,
        msg_count: usize,
        full_prompt: String,
    },

    ToolCall {
        turn: TurnNumber,
        step: StepNumber,
        tool: String,
        args: String,
    },
    ToolCallResult {
        turn: TurnNumber,
        step: StepNumber,
        tool: String,
        summary: String,
        success: bool,
    },

    StepCompleted {
        turn: TurnNumber,
        step: StepNumber,
        output: StepOutput,
    },

    TurnEnded {
        turn: TurnNumber,
        reason: TurnEndReason,
    },

    ModelRetry {
        turn: TurnNumber,
        reason: ModelRetryReason,
    },

    /// 章节收尾开始（重开前置留存启动）。
    #[serde(alias = "compact-started")]
    WrapUpStarted,

    /// 章节收尾完成。`summary` = 留存摘要全文（旧事件无此字段，default 空串）。
    #[serde(alias = "compact-completed")]
    WrapUpCompleted {
        /// 被收尾章节的 Step 数（收尾前取值）；旧事件缺此字段时默认 0（兼容历史行）
        #[serde(default)]
        step_count: usize,
        /// 留存摘要（置入新章节开头；旧事件为空串）
        #[serde(default)]
        summary: String,
    },

    /// 章节收尾失败。`error` = 失败原因（无摘要 / 生成错误），事件后重开干净章节。
    #[serde(alias = "compact-failed")]
    WrapUpFailed { error: String },
}

impl AgentEventPayload {
    pub fn kind(&self) -> &'static str {
        self.into()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Display, IntoStaticStr, EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ModelRetryReason {
    /// 模型直接输出了文本但没有调用工具（发言契约违规：文本必须配 skip/send_message）
    ResponseWithText,
    /// Reqwest 网络超时 / 连接失败（真重试，最多 2 次退避）
    TimeoutRetry,
}
