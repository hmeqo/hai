use crate::domain::{
    model::Event,
    vo::{AgentEvent, AgentEventPayload, TurnEndReason},
};

// ── EventDisplay ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct EventDisplay {
    pub tag: &'static str,
    pub one_liner: String,
    pub detail_text: String,
    pub chat_id: i64,
    pub color: (u8, u8, u8),
}

impl EventDisplay {
    /// 解析失败时的事件占位（行可见，详情为空）。
    pub fn unparsed() -> Self {
        Self {
            tag: "?",
            one_liner: String::from("<unparsed>"),
            detail_text: String::from("<unparsed event payload>"),
            chat_id: 0,
            color: (156, 163, 175),
        }
    }

    pub fn from_event(event: &Event) -> Option<Self> {
        let ae: AgentEvent = serde_json::from_value(event.payload.clone()).ok()?;
        let payload = &ae.payload;
        let tag = tag_for_kind(payload);
        let color = color_rgb(payload);

        let one_liner = match payload {
            AgentEventPayload::TurnEnded {
                turn,
                reason: TurnEndReason::Failed { error },
                ..
            } => {
                let preview: String = error.chars().take(60).collect();
                format!("FAIL   {turn}  {preview}")
            }
            AgentEventPayload::ModelRetry { turn, reason, .. } => {
                format!("RETRY  {turn}  {reason}")
            }
            AgentEventPayload::TurnStarted {
                turn,
                reason,
                msg_count,
                ..
            } => {
                format!("TURN   {turn}  {reason}  msgs:{msg_count}")
            }
            AgentEventPayload::ToolCall {
                turn,
                step,
                tool,
                args,
                ..
            } => {
                let preview: String = args.chars().take(40).collect();
                format!("TOOL   {turn}.{step}  {tool}({preview})")
            }
            AgentEventPayload::ToolCallResult {
                turn,
                step,
                tool,
                summary,
                success,
                ..
            } => {
                let preview: String = summary.chars().take(40).collect();
                format!(
                    "TOOL   {turn}.{step}  {tool}  {}  {}",
                    preview,
                    if *success { "✓" } else { "✗" }
                )
            }
            AgentEventPayload::TurnEnded {
                reason:
                    TurnEndReason::Success {
                        output,
                        tool_calls,
                        elapsed_ms,
                        context_tokens,
                        ..
                    },
                ..
            } => {
                format!(
                    "DONE   {}  {tool_calls}tools  {:.1}s  ctx {}t",
                    output.turn,
                    *elapsed_ms as f64 / 1000.0,
                    context_tokens
                )
            }
            AgentEventPayload::TurnEnded {
                turn,
                reason: TurnEndReason::Steered { .. },
                ..
            } => {
                format!("STEER  {turn}")
            }
            AgentEventPayload::StepCompleted { turn, step, .. } => {
                format!("STEP   {turn}.{step}")
            }
            AgentEventPayload::WrapUpStarted => "WRAP  start".to_string(),
            AgentEventPayload::WrapUpCompleted { step_count, .. } => {
                format!("WRAP  {} steps", step_count)
            }
            AgentEventPayload::WrapUpFailed { error, .. } => {
                let preview: String = error.chars().take(60).collect();
                format!("FAIL   wrap-up  {preview}")
            }
        };

        let detail_text = build_detail(event, payload);

        Some(Self {
            tag,
            one_liner,
            detail_text,
            chat_id: ae.chat_id.0,
            color,
        })
    }
}

/// 事件类型菜单：显示名 + 存储 tag（serde kebab-case，与 `payload->>'event'` 匹配）。
pub(super) const KIND_TAGS: &[(&str, &str)] = &[
    ("TURN", "turn-started"),
    ("TOOL", "tool-call"),
    ("TOOL", "tool-call-result"),
    ("STEP", "step-completed"),
    ("DONE", "turn-ended"),
    ("RETRY", "model-retry"),
    ("FAIL", "turn-ended"),
    ("STEER", "turn-ended"),
    ("FAIL", "wrap-up-failed"),
    ("WRAP", "wrap-up-started"),
    ("WRAP", "wrap-up-completed"),
];

fn tag_for_kind(payload: &AgentEventPayload) -> &'static str {
    match payload {
        AgentEventPayload::TurnStarted { .. } => "TURN",
        AgentEventPayload::ToolCall { .. } | AgentEventPayload::ToolCallResult { .. } => "TOOL",
        AgentEventPayload::StepCompleted { .. } => "STEP",
        AgentEventPayload::TurnEnded {
            reason: TurnEndReason::Failed { .. },
            ..
        } => "FAIL",
        AgentEventPayload::TurnEnded {
            reason: TurnEndReason::Steered { .. },
            ..
        } => "STEER",
        AgentEventPayload::TurnEnded { .. } => "DONE",
        AgentEventPayload::ModelRetry { .. } => "RETRY",
        AgentEventPayload::WrapUpFailed { .. } => "FAIL",
        AgentEventPayload::WrapUpStarted | AgentEventPayload::WrapUpCompleted { .. } => "WRAP",
    }
}

// ── Detail Builder ─────────────────────────────────────────────────────────────

struct Detail {
    buf: String,
}

impl Detail {
    fn new() -> Self {
        Self { buf: String::new() }
    }

    fn field(&mut self, label: &str, value: impl std::fmt::Display) {
        use std::fmt::Write;
        write!(self.buf, "\n  {label}:  {value}").unwrap();
    }

    fn block(&mut self, label: &str, body: &str) {
        self.buf.push_str(&format!("\n  {label}:"));
        for line in body.lines() {
            self.buf.push_str(&format!("\n  {line}"));
        }
    }

    fn dump(&mut self, title: &str, body: &str) {
        self.buf.push_str(&format!("\n\n{title}:\n{body}"));
    }

    fn build(self) -> String {
        self.buf
    }
}

fn build_detail(event: &Event, ae: &AgentEventPayload) -> String {
    let header = format!(
        "#{}  {}  {}  {}",
        event.seq,
        fmt_time(event.created_at),
        chat_display(event),
        tag_for_kind(ae)
    );

    let mut d = Detail::new();

    match ae {
        AgentEventPayload::TurnStarted {
            turn,
            reason,
            msg_count,
            full_prompt,
            ..
        } => {
            d.field("Turn", turn);
            d.field("Reason", reason);
            d.field("Messages", msg_count);
            if !full_prompt.is_empty() {
                d.dump("Prompt", &display_json_value(full_prompt));
            }
        }
        AgentEventPayload::TurnEnded {
            turn,
            reason: TurnEndReason::Failed { error },
            ..
        } => {
            d.field("Turn", turn);
            d.field("Outcome", "failed");
            d.field("Error", error);
        }
        AgentEventPayload::ModelRetry { turn, reason, .. } => {
            d.field("Turn", turn);
            d.field("Reason", reason);
        }
        AgentEventPayload::StepCompleted { turn, step, output } => {
            d.field("Step", format!("{}.{}", turn, step));
            if let Some(rs) = &output.reasoning {
                d.block("Thinking", rs);
            }
            d.block("Response", &output.response);
        }
        AgentEventPayload::ToolCall { tool, args, .. } => {
            d.field("Tool", tool);
            d.block("Arguments", &display_json_value(args));
        }
        AgentEventPayload::ToolCallResult {
            tool,
            summary,
            success,
            ..
        } => {
            d.field("Tool", tool);
            d.block("Result", &display_json_value(summary));
            d.field("Status", if *success { "✓" } else { "✗" });
        }
        AgentEventPayload::TurnEnded {
            turn,
            reason:
                TurnEndReason::Success {
                    output,
                    tool_calls,
                    elapsed_ms,
                    context_tokens,
                    ..
                }
                | TurnEndReason::Steered {
                    output,
                    tool_calls,
                    elapsed_ms,
                    context_tokens,
                    ..
                },
            ..
        } => {
            d.field("Turn", turn);
            d.field("Step", format!("{}.{}", output.turn, output.step));
            if let Some(rs) = &output.reasoning {
                d.block("Thinking", rs);
            }
            if !output.response.is_empty() {
                d.block("Response", &output.response);
            }
            d.field("Tool Calls", tool_calls);
            d.field("Duration", format!("{:.1}s", *elapsed_ms as f64 / 1000.0));
            d.field("Context Tokens", format!("{}", context_tokens));
        }
        AgentEventPayload::WrapUpStarted => {
            d.field("Action", "start");
        }
        AgentEventPayload::WrapUpCompleted {
            step_count,
            summary,
        } => {
            d.field("Steps", step_count);
            if !summary.is_empty() {
                d.block("Summary", summary);
            }
        }
        AgentEventPayload::WrapUpFailed { error } => {
            d.field("Error", error);
        }
    }

    format!("{header}{}", d.build())
}

// ── Utilities ──────────────────────────────────────────────────────────────────

fn display_json_value(s: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
        Err(_) => s.to_string(),
    }
}

pub(super) fn fmt_time(ts: jiff::Timestamp) -> String {
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    if zoned.date() == jiff::Zoned::now().date() {
        zoned.strftime("%H:%M:%S").to_string()
    } else {
        zoned.strftime("%m-%d %H:%M").to_string()
    }
}

pub(super) fn chat_display(event: &Event) -> String {
    let ae: AgentEvent = match serde_json::from_value(event.payload.clone()) {
        Ok(ae) => ae,
        Err(_) => return String::new(),
    };
    format!("{:+}", ae.chat_id)
}

// ── 颜色映射 ─────────────────────────────────────────────────────────────

fn color_rgb(payload: &AgentEventPayload) -> (u8, u8, u8) {
    match payload {
        AgentEventPayload::TurnEnded {
            reason: TurnEndReason::Failed { .. },
            ..
        }
        | AgentEventPayload::WrapUpFailed { .. } => (239, 68, 68),
        AgentEventPayload::ModelRetry { .. } => (250, 176, 5),
        AgentEventPayload::StepCompleted { .. } => (56, 189, 248),
        AgentEventPayload::TurnStarted { .. } => (255, 255, 255),
        AgentEventPayload::ToolCall { .. } => (59, 130, 246),
        AgentEventPayload::ToolCallResult { success, .. } if !success => (239, 68, 68),
        AgentEventPayload::ToolCallResult { .. } => (34, 197, 94),
        AgentEventPayload::TurnEnded { .. } => (255, 255, 255),
        AgentEventPayload::WrapUpStarted | AgentEventPayload::WrapUpCompleted { .. } => {
            (168, 85, 247)
        }
    }
}
