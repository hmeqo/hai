use crate::domain::{model::Event, vo::AgentEventPayload};

// ── EventDisplay ───────────────────────────────────────────────────────────────

pub(super) struct EventDisplay {
    pub tag: &'static str,
    pub one_liner: String,
    pub detail_text: String,
}

impl EventDisplay {
    pub fn from_event(event: &Event) -> Self {
        let ae: AgentEventPayload =
            serde_json::from_value(event.payload.0.clone()).unwrap_or_else(|_| fallback());
        let tag = tag_for_kind(&ae);

        let one_liner = match &ae {
            AgentEventPayload::RunFailed { turn, error, .. } => {
                let preview: String = error.chars().take(60).collect();
                format!("FAIL   {turn}  {preview}")
            }
            AgentEventPayload::Preempted { turn, .. } => {
                format!("PREEMPT  {turn}")
            }
            AgentEventPayload::ModelRetry { turn, reason, .. } => {
                format!("RETRY  {turn}  {reason}")
            }
            AgentEventPayload::WakeStarted { turn, reason, .. } => {
                format!("WAKE   {turn}  {reason}")
            }
            AgentEventPayload::ContextBuilt {
                turn, msg_count, ..
            } => {
                format!("CTX    {turn}  msgs:{msg_count}")
            }
            AgentEventPayload::ToolCall {
                turn, tool, args, ..
            } => {
                let preview: String = args.chars().take(40).collect();
                format!("TOOL   {turn}  {tool}({preview})")
            }
            AgentEventPayload::ToolCallResult {
                turn,
                tool,
                summary,
                success,
                ..
            } => {
                let preview: String = summary.chars().take(40).collect();
                format!(
                    "TOOL   {turn}  {tool}  {}  {}",
                    preview,
                    if *success { "✓" } else { "✗" }
                )
            }
            AgentEventPayload::RunCompleted {
                turn,
                tool_calls,
                elapsed_ms,
                prompt_tokens,
                completion_tokens,
                ..
            } => {
                format!(
                    "DONE   {turn}  {tool_calls}tools  {:.1}s  {prompt_tokens}/{completion_tokens}tok",
                    *elapsed_ms as f64 / 1000.0
                )
            }
            AgentEventPayload::SessionCreated { mode, model, .. } => {
                format!("SESS   created  {mode}  {model}")
            }
            AgentEventPayload::SessionDone { .. } => "SESS   done".to_string(),
        };

        let detail_text = build_detail(event, &ae);

        Self {
            tag,
            one_liner,
            detail_text,
        }
    }
}

fn fallback() -> AgentEventPayload {
    AgentEventPayload::SessionDone { chat_id: 0.into() }
}

fn tag_for_kind(ae: &AgentEventPayload) -> &'static str {
    match ae {
        AgentEventPayload::SessionCreated { .. } | AgentEventPayload::SessionDone { .. } => "SESS",
        AgentEventPayload::WakeStarted { .. } => "WAKE",
        AgentEventPayload::ContextBuilt { .. } => "CTX",
        AgentEventPayload::ToolCall { .. } | AgentEventPayload::ToolCallResult { .. } => "TOOL",
        AgentEventPayload::RunCompleted { .. } => "DONE",
        AgentEventPayload::ModelRetry { .. } => "RETRY",
        AgentEventPayload::RunFailed { .. } => "FAIL",
        AgentEventPayload::Preempted { .. } => "PREEMPT",
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

    let body = match ae {
        AgentEventPayload::Preempted { turn, .. } => {
            format!("\n  Turn:   {turn}")
        }
        AgentEventPayload::RunFailed {
            turn,
            elapsed_ms,
            error,
            ..
        } => {
            format!(
                "\n  Turn:     {turn}\n  Duration: {:.1}s\n  Error:    {error}",
                *elapsed_ms as f64 / 1000.0
            )
        }
        AgentEventPayload::ModelRetry { turn, reason, .. } => {
            format!("\n  Turn:   {turn}\n  Reason: {reason}")
        }
        AgentEventPayload::ContextBuilt {
            msg_count,
            full_prompt,
            ..
        } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Messages:  {msg_count}"));
            b.push_str(&format!(
                "\n\nFull Prompt ({} tokens):\n{full_prompt}",
                estimate_tokens(full_prompt)
            ));
            b
        }
        AgentEventPayload::ToolCall { tool, args, .. } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Tool:  {tool}"));
            b.push_str(&format!("\n\nArguments:\n  {args}"));
            b
        }
        AgentEventPayload::ToolCallResult {
            tool,
            summary,
            success,
            ..
        } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Tool:   {tool}"));
            b.push_str(&format!("\n  Result: {summary}"));
            b.push_str(&format!("\n  Status: {}", if *success { "✓" } else { "✗" }));
            b
        }
        AgentEventPayload::RunCompleted {
            tool_calls,
            elapsed_ms,
            prompt_tokens,
            completion_tokens,
            response,
            reasoning,
            ..
        } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Tool Calls:  {tool_calls}"));
            b.push_str(&format!(
                "\n  Duration:    {:.1}s",
                *elapsed_ms as f64 / 1000.0
            ));
            if let Some(rs) = reasoning.as_ref().filter(|s| !s.is_empty()) {
                b.push_str(&format!("\n  Thinking:    {rs}"));
            }
            if !response.is_empty() {
                b.push_str(&format!("\n  Response:    {response}"));
            }
            b.push_str(&format!(
                "\n  Tokens:      {prompt_tokens} (prompt) + {completion_tokens} (completion)"
            ));
            b
        }
        AgentEventPayload::WakeStarted { turn, reason, .. } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Turn:   {turn}"));
            b.push_str(&format!("\n  Reason: {reason}"));
            b
        }
        AgentEventPayload::SessionCreated { mode, model, .. } => {
            let mut b = String::new();
            b.push_str(&format!("\n  Mode:   {mode}"));
            b.push_str(&format!("\n  Model:  {model}"));
            b
        }
        AgentEventPayload::SessionDone { .. } => String::new(),
    };

    format!("{header}{body}")
}

// ── Utilities ──────────────────────────────────────────────────────────────────

pub(super) fn fmt_time(ts: jiff::Timestamp) -> String {
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    zoned.strftime("%H:%M:%S").to_string()
}

pub(super) fn chat_display(event: &Event) -> String {
    let ae: AgentEventPayload = match serde_json::from_value(event.payload.0.clone()) {
        Ok(ae) => ae,
        Err(_) => return String::new(),
    };
    match ae.chat_id() {
        Some(id) => format!("{:+}", id),
        None => String::new(),
    }
}

fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

// ── 颜色映射 ─────────────────────────────────────────────────────────────

pub(super) fn color_rgb(event: &Event) -> (u8, u8, u8) {
    let ae: AgentEventPayload = match serde_json::from_value(event.payload.0.clone()) {
        Ok(ae) => ae,
        Err(_) => return (156, 163, 175),
    };
    match &ae {
        AgentEventPayload::RunFailed { .. } => (239, 68, 68),
        AgentEventPayload::Preempted { .. } => (250, 176, 5),
        AgentEventPayload::ModelRetry { .. } => (250, 176, 5),
        AgentEventPayload::ToolCall { .. } => (59, 130, 246),
        AgentEventPayload::ToolCallResult { success, .. } if !success => (239, 68, 68),
        AgentEventPayload::ToolCallResult { .. } => (34, 197, 94),
        AgentEventPayload::RunCompleted { .. } => (255, 255, 255),
        AgentEventPayload::WakeStarted { .. } | AgentEventPayload::ContextBuilt { .. } => {
            (234, 179, 8)
        }
        AgentEventPayload::SessionCreated { .. } | AgentEventPayload::SessionDone { .. } => {
            (107, 114, 128)
        }
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
            let ae: AgentEventPayload = match serde_json::from_value(e.payload.0.clone()) {
                Ok(ae) => ae,
                Err(_) => return false,
            };
            let chat_ok = chat_id.is_none_or(|cid| ae.chat_id() == Some(cid));
            let kind_ok = event_kind.is_none_or(|k| ae.kind() == k);
            chat_ok && kind_ok
        })
        .collect()
}
