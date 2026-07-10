use serde_json::Value;

use crate::domain::{
    model::Event,
    vo::{AgentEvent, AgentEventPayload},
};

// ── EventDisplay ───────────────────────────────────────────────────────────────

pub(super) struct EventDisplay {
    pub tag: &'static str,
    pub one_liner: String,
    pub detail_text: String,
}

impl EventDisplay {
    pub fn from_event(event: &Event) -> Option<Self> {
        let ae: AgentEvent = serde_json::from_value(event.payload.0.clone()).ok()?;
        let payload = &ae.payload;
        let tag = tag_for_kind(payload);

        let one_liner = match payload {
            AgentEventPayload::RunFailed { run, error, .. } => {
                let preview: String = error.chars().take(60).collect();
                format!("FAIL   {run}  {preview}")
            }
            AgentEventPayload::Preempted { run, .. } => {
                format!("PREEMPT  {run}")
            }
            AgentEventPayload::ModelRetry { run, reason, .. } => {
                format!("RETRY  {run}  {reason}")
            }
            AgentEventPayload::RunStarted {
                run,
                reason,
                msg_count,
                ..
            } => {
                format!("RUN    {run}  {reason}  msgs:{msg_count}")
            }
            AgentEventPayload::ToolCall {
                run, tool, args, ..
            } => {
                let preview: String = args.chars().take(40).collect();
                format!("TOOL   {run}  {tool}({preview})")
            }
            AgentEventPayload::ToolCallResult {
                run,
                tool,
                summary,
                success,
                ..
            } => {
                let preview: String = summary.chars().take(40).collect();
                format!(
                    "TOOL   {run}  {tool}  {}  {}",
                    preview,
                    if *success { "✓" } else { "✗" }
                )
            }
            AgentEventPayload::RunCompleted {
                output,
                tool_calls,
                elapsed_ms,
                prompt_tokens,
                completion_tokens,
                ..
            } => {
                format!(
                    "DONE   {}  {tool_calls}tools  {:.1}s  {prompt_tokens}/{completion_tokens}tok",
                    output.run,
                    *elapsed_ms as f64 / 1000.0
                )
            }
            AgentEventPayload::TurnCompleted(tc) => {
                format!("TURN   {}.{}", tc.run, tc.turn)
            }
        };

        let detail_text = build_detail(event, payload);

        Some(Self {
            tag,
            one_liner,
            detail_text,
        })
    }
}

fn tag_for_kind(payload: &AgentEventPayload) -> &'static str {
    match payload {
        AgentEventPayload::RunStarted { .. } => "RUN",
        AgentEventPayload::ToolCall { .. } | AgentEventPayload::ToolCallResult { .. } => "TOOL",
        AgentEventPayload::TurnCompleted(..) => "TURN",
        AgentEventPayload::RunCompleted { .. } => "DONE",
        AgentEventPayload::ModelRetry { .. } => "RETRY",
        AgentEventPayload::RunFailed { .. } => "FAIL",
        AgentEventPayload::Preempted { .. } => "PREEMPT",
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
        AgentEventPayload::RunStarted {
            run,
            reason,
            msg_count,
            full_prompt,
            ..
        } => {
            d.field("Run", run);
            d.field("Reason", reason);
            d.field("Messages", msg_count);
            if !full_prompt.is_empty() {
                d.dump("Prompt", &display_json_value(full_prompt));
            }
        }
        AgentEventPayload::Preempted {
            run,
            count,
            reasons,
            content,
            ..
        } => {
            d.field("Run", run);
            d.field("Events", count);
            d.field("Reasons", reasons);
            if !content.is_empty() {
                d.block("Injected", content);
            }
        }
        AgentEventPayload::RunFailed {
            run,
            elapsed_ms,
            error,
            ..
        } => {
            d.field("Run", run);
            d.field("Duration", format!("{:.1}s", *elapsed_ms as f64 / 1000.0));
            d.field("Error", error);
        }
        AgentEventPayload::ModelRetry { run, reason, .. } => {
            d.field("Run", run);
            d.field("Reason", reason);
        }
        AgentEventPayload::TurnCompleted(tc) => {
            d.field("Turn", format!("{}.{}", tc.run, tc.turn));
            if let Some(rs) = &tc.reasoning {
                d.block("Thinking", rs);
            }
            d.block("Response", &tc.response);
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
        AgentEventPayload::RunCompleted {
            output,
            tool_calls,
            elapsed_ms,
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            d.field("Turn", format!("{}.{}", output.run, output.turn));
            if let Some(rs) = &output.reasoning {
                d.block("Thinking", rs);
            }
            if !output.response.is_empty() {
                d.block("Response", &output.response);
            }
            d.field("Tool Calls", tool_calls);
            d.field("Duration", format!("{:.1}s", *elapsed_ms as f64 / 1000.0));
            d.field(
                "Tokens",
                format!("{prompt_tokens} (prompt) + {completion_tokens} (completion)"),
            );
        }
    }

    format!("{header}{}", d.build())
}

// ── Utilities ──────────────────────────────────────────────────────────────────

fn display_json_value(s: &str) -> String {
    match serde_json::from_str::<Value>(s) {
        Ok(Value::String(s)) => s,
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
        Err(_) => s.to_string(),
    }
}

pub(super) fn fmt_time(ts: jiff::Timestamp) -> String {
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    zoned.strftime("%H:%M:%S").to_string()
}

pub(super) fn chat_display(event: &Event) -> String {
    let ae: AgentEvent = match serde_json::from_value(event.payload.0.clone()) {
        Ok(ae) => ae,
        Err(_) => return String::new(),
    };
    format!("{:+}", ae.chat_id)
}

// ── 颜色映射 ─────────────────────────────────────────────────────────────

pub(super) fn color_rgb(event: &Event) -> (u8, u8, u8) {
    let ae: AgentEvent = match serde_json::from_value(event.payload.0.clone()) {
        Ok(ae) => ae,
        Err(_) => return (156, 163, 175),
    };
    match &ae.payload {
        AgentEventPayload::RunFailed { .. } => (239, 68, 68),
        AgentEventPayload::Preempted { .. } => (250, 176, 5),
        AgentEventPayload::ModelRetry { .. } => (250, 176, 5),
        AgentEventPayload::TurnCompleted(..) => (56, 189, 248),
        AgentEventPayload::RunStarted { .. } => (234, 179, 8),
        AgentEventPayload::ToolCall { .. } => (59, 130, 246),
        AgentEventPayload::ToolCallResult { success, .. } if !success => (239, 68, 68),
        AgentEventPayload::ToolCallResult { .. } => (34, 197, 94),
        AgentEventPayload::RunCompleted { .. } => (255, 255, 255),
    }
}

// ── Queries ────────────────────────────────────────────────────────────────────

pub(super) async fn query_events(
    db: &toasty::Db,
    chat_id: Option<i64>,
    event_kind: Option<&str>,
    limit: usize,
) -> crate::error::Result<Vec<Event>> {
    let raw = Event::filter(Event::fields().seq().gt(0_i64))
        .order_by(Event::fields().seq().desc())
        .limit(limit.max(200))
        .exec(&mut db.clone())
        .await?;

    Ok(filter_in_rust(raw, chat_id, event_kind))
}

pub(super) async fn query_new_events(
    db: &toasty::Db,
    after_seq: i64,
    chat_id: Option<i64>,
    event_kind: Option<&str>,
) -> crate::error::Result<Vec<Event>> {
    let raw = Event::filter(Event::fields().seq().gt(after_seq))
        .order_by(Event::fields().seq().asc())
        .exec(&mut db.clone())
        .await?;

    Ok(filter_in_rust(raw, chat_id, event_kind))
}

pub(super) async fn query_event_by_seq(
    db: &toasty::Db,
    seq: i64,
) -> crate::error::Result<Option<Event>> {
    let mut events = Event::filter(Event::fields().seq().eq(seq))
        .limit(1)
        .exec(&mut db.clone())
        .await?;
    Ok(events.pop())
}

fn filter_in_rust(raw: Vec<Event>, chat_id: Option<i64>, event_kind: Option<&str>) -> Vec<Event> {
    if chat_id.is_none() && event_kind.is_none() {
        return raw;
    }
    raw.into_iter()
        .filter(|e| {
            let ae: AgentEvent = match serde_json::from_value(e.payload.0.clone()) {
                Ok(ae) => ae,
                Err(_) => return false,
            };
            let chat_ok = chat_id.is_none_or(|cid| ae.chat_id.0 == cid);
            let kind_ok = event_kind.is_none_or(|k| ae.payload.kind() == k);
            chat_ok && kind_ok
        })
        .collect()
}
