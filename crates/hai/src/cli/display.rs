use toasty::stmt::Value;

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
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(serde_json::Value::String(s)) => s,
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

/// Raw SQL query with optional JSONB-based filters.
/// Result order: seq DESC (if `desc=true`) or seq ASC (if `desc=false`).
pub(super) async fn raw_query(
    db: &mut toasty::Db,
    chat_id: Option<i64>,
    kind: Option<&str>,
    before_seq: Option<i64>,
    after_seq: Option<i64>,
    desc: bool,
    limit: usize,
) -> crate::error::Result<Vec<Event>> {
    let mut params: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn FnOnce(toasty::sql::Query) -> toasty::sql::Query>> = Vec::new();
    let mut idx = 0usize;

    fn push_p<T: Into<toasty::stmt::Value> + 'static>(
        params: &mut Vec<String>,
        binds: &mut Vec<Box<dyn FnOnce(toasty::sql::Query) -> toasty::sql::Query>>,
        idx: &mut usize,
        val: T,
        expr: &str,
    ) {
        *idx += 1;
        params.push(expr.replace("?", &format!("${}", *idx)));
        binds.push(Box::new(move |q| q.bind(val)));
    }

    let mut sql = "SELECT seq, domain, payload, created_at FROM event WHERE seq > 0".to_string();

    if let Some(cid) = chat_id {
        push_p(
            &mut params,
            &mut binds,
            &mut idx,
            cid,
            "AND (payload->>'chat_id')::bigint = ?",
        );
    }
    if let Some(k) = kind {
        push_p(
            &mut params,
            &mut binds,
            &mut idx,
            k.to_string(),
            "AND payload->>'event' = ?",
        );
    }
    if let Some(b) = before_seq {
        push_p(&mut params, &mut binds, &mut idx, b, "AND seq < ?");
    }
    if let Some(a) = after_seq {
        push_p(&mut params, &mut binds, &mut idx, a, "AND seq > ?");
    }

    sql.push_str(&params.join(" "));
    sql.push_str(&format!(
        " ORDER BY seq {} LIMIT {}",
        if desc { "DESC" } else { "ASC" },
        limit
    ));

    let mut q = toasty::sql::query(&sql);
    for bind in binds {
        q = bind(q);
    }

    let rows = q.exec(db).await?;
    let events: Vec<Event> = rows.into_iter().filter_map(row_to_event).collect();
    Ok(events)
}

fn row_to_event(row: Value) -> Option<Event> {
    let fields = row.as_record()?.fields.as_slice();
    if fields.len() < 4 {
        return None;
    }

    let seq = match &fields[0] {
        Value::I64(v) => *v,
        _ => return None,
    };
    let domain = match &fields[1] {
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let payload = parse_jsonb(&fields[2])?;
    let created_at = match &fields[3] {
        Value::Timestamp(ts) => *ts,
        _ => return None,
    };

    Some(Event {
        seq,
        domain,
        payload,
        created_at,
    })
}

fn parse_jsonb(v: &Value) -> Option<toasty::Json<serde_json::Value>> {
    let bytes = match v {
        Value::Bytes(b) => b.as_slice(),
        Value::String(s) => s.as_bytes(),
        _ => return None,
    };
    let val: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some(toasty::Json(val))
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
