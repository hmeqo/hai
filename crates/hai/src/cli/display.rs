use crate::domain::model::Event;

// ── EventDisplay ───────────────────────────────────────────────────────────────

pub(super) struct EventDisplay {
    pub tag: &'static str,
    pub one_liner: String,
    pub detail_text: String,
}

impl EventDisplay {
    pub fn from_event(event: &Event) -> Self {
        let p = &event.payload.0;
        let tag = Self::tag_for_kind(&event.kind);

        let one_liner = match event.kind.as_str() {
            "turn_started" => {
                let turn = p.get("turn").and_then(|v| v.as_u64()).unwrap_or(0);
                let reason = p.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                format!("TURN   {turn}  {reason}")
            }
            "context_built" => {
                let turn = p.get("turn").and_then(|v| v.as_u64()).unwrap_or(0);
                let msgs = p.get("msg_count").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("CTX    {turn}  msgs:{msgs}")
            }
            "tool_call" => {
                let turn = p.get("turn").and_then(|v| v.as_u64()).unwrap_or(0);
                let tool = p.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                let args = p.get("args").and_then(|v| v.as_str()).unwrap_or("");
                let preview: String = args.chars().take(40).collect();
                format!("TOOL   {turn}  {tool}({preview})")
            }
            "tool_call_result" => {
                let turn = p.get("turn").and_then(|v| v.as_u64()).unwrap_or(0);
                let tool = p.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                let summary = p.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                let preview: String = summary.chars().take(40).collect();
                let ok = p.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                format!("TOOL   {turn}  {tool}  {}  {}", preview, if ok { "✓" } else { "✗" })
            }
            "turn_completed" => {
                let turn = p.get("turn").and_then(|v| v.as_u64()).unwrap_or(0);
                let tc = p.get("tool_calls").and_then(|v| v.as_u64()).unwrap_or(0);
                let ms = p.get("elapsed_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let pt = p.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let ct = p.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("DONE   {turn}  {tc}tools  {:.1}s  {pt}/{ct}tok", ms / 1000.0)
            }
            "session_created" => {
                let mode = p.get("mode").and_then(|v| v.as_str()).unwrap_or("");
                let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("");
                format!("SESS   created  {mode}  {model}")
            }
            "session_done" => "SESS   done".to_string(),
            _ => format!("{}  {}", event.kind, serde_json::to_string(p).unwrap_or_default()),
        };

        let detail_text = Self::build_detail(event);

        Self { tag, one_liner, detail_text }
    }

    fn tag_for_kind(kind: &str) -> &'static str {
        match kind {
            "session_created" | "session_done" => "SESS",
            "turn_started" => "TURN",
            "context_built" => "CTX",
            "tool_call" | "tool_call_result" => "TOOL",
            "turn_completed" => "DONE",
            _ => "EVENT",
        }
    }

    fn build_detail(event: &Event) -> String {
        let p = &event.payload.0;
        let time = fmt_time(event.created_at);
        let chat = chat_display(event.chat_id);

        let header = format!("#{}  {}  {}  {}", event.seq, time, chat, Self::tag_for_kind(&event.kind));

        let body = match event.kind.as_str() {
            "context_built" => {
                let mut b = String::new();
                if let Some(msg_count) = p.get("msg_count").and_then(|v| v.as_u64()) {
                    b.push_str(&format!("\n  Messages:  {msg_count}"));
                }
                if let Some(fp) = p.get("full_prompt").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n\nFull Prompt ({} tokens):\n{}", estimate_tokens(fp), fp));
                }
                b
            }
            "tool_call" => {
                let mut b = String::new();
                if let Some(tool) = p.get("tool").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Tool:  {tool}"));
                }
                if let Some(args) = p.get("args").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n\nArguments:\n  {args}"));
                }
                b
            }
            "tool_call_result" => {
                let mut b = String::new();
                if let Some(tool) = p.get("tool").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Tool:   {tool}"));
                }
                if let Some(summary) = p.get("summary").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Result: {summary}"));
                }
                if let Some(success) = p.get("success").and_then(|v| v.as_bool()) {
                    b.push_str(&format!("\n  Status: {}", if success { "✓" } else { "✗" }));
                }
                b
            }
            "turn_completed" => {
                let mut b = String::new();
                if let Some(tc) = p.get("tool_calls").and_then(|v| v.as_u64()) {
                    b.push_str(&format!("\n  Tool Calls:  {tc}"));
                }
                if let Some(ms) = p.get("elapsed_ms").and_then(|v| v.as_u64()) {
                    b.push_str(&format!("\n  Duration:    {:.1}s", ms as f64 / 1000.0));
                }
                if let Some(rs) = p.get("reasoning").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    b.push_str(&format!("\n  Thinking:    {rs}"));
                }
                if let Some(rp) = p.get("response").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    b.push_str(&format!("\n  Response:    {rp}"));
                }
                if let Some(pt) = p.get("prompt_tokens").and_then(|v| v.as_u64())
                    && let Some(ct) = p.get("completion_tokens").and_then(|v| v.as_u64()) {
                    b.push_str(&format!("\n  Tokens:      {pt} (prompt) + {ct} (completion)"));
                }
                b
            }
            "turn_started" => {
                let mut b = String::new();
                if let Some(turn) = p.get("turn").and_then(|v| v.as_u64()) {
                    b.push_str(&format!("\n  Turn:   {turn}"));
                }
                if let Some(reason) = p.get("reason").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Reason: {reason}"));
                }
                b
            }
            "session_created" => {
                let mut b = String::new();
                if let Some(mode) = p.get("mode").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Mode:   {mode}"));
                }
                if let Some(model) = p.get("model").and_then(|v| v.as_str()) {
                    b.push_str(&format!("\n  Model:  {model}"));
                }
                b
            }
            _ => {
                let mut b = String::new();
                b.push_str(&format!("\n{}", serde_json::to_string_pretty(p).unwrap_or_default()));
                b
            }
        };

        format!("{header}{body}")
    }
}

// ── Utilities ──────────────────────────────────────────────────────────────────

pub(super) fn fmt_time(ts: jiff::Timestamp) -> String {
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    zoned.strftime("%H:%M:%S").to_string()
}

pub(super) fn chat_display(chat_id: Option<i64>) -> String {
    match chat_id {
        Some(id) => format!("{:+}", id),
        None => String::new(),
    }
}

fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

// ── Queries ────────────────────────────────────────────────────────────────────

pub(super) async fn query_events(
    db: &toasty::Db,
    chat_id: Option<i64>,
    event_kind: Option<&str>,
    limit: usize,
) -> crate::error::Result<Vec<Event>> {
    let mut filter = Event::fields().seq().gt(0_i64);
    if let Some(cid) = chat_id {
        filter = filter.and(Event::fields().chat_id().eq(cid));
    }
    if let Some(kind) = event_kind {
        filter = filter.and(Event::fields().kind().eq(kind.to_string()));
    }
    let events = Event::filter(filter)
        .order_by(Event::fields().seq().desc())
        .limit(limit)
        .exec(&mut db.clone())
        .await?;
    Ok(events)
}

pub(super) async fn query_new_events(
    db: &toasty::Db,
    after_seq: i64,
    chat_id: Option<i64>,
    event_kind: Option<&str>,
) -> crate::error::Result<Vec<Event>> {
    let mut filter = Event::fields().seq().gt(after_seq);
    if let Some(cid) = chat_id {
        filter = filter.and(Event::fields().chat_id().eq(cid));
    }
    if let Some(kind) = event_kind {
        filter = filter.and(Event::fields().kind().eq(kind.to_string()));
    }
    let events = Event::filter(filter)
        .order_by(Event::fields().seq().asc())
        .exec(&mut db.clone())
        .await?;
    Ok(events)
}

pub(super) async fn query_event_by_seq(db: &toasty::Db, seq: i64) -> crate::error::Result<Option<Event>> {
    let mut events = Event::filter(Event::fields().seq().eq(seq))
        .limit(1)
        .exec(&mut db.clone())
        .await?;
    Ok(events.pop())
}
