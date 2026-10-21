use std::collections::{HashMap, HashSet};

use crate::{
    cli::display::{EventDisplay, TurnOutcome},
    domain::{
        model::Event,
        repo::{EventQuery, Repos},
        vo::{ChatId, EventKind, SessionRef, TurnNumber},
    },
};

const PAGE_SIZE: usize = 200;

/// One turn's events in the order they happened: the unit a reader thinks in, while the log stores
/// events. A record that carries no turn of its own (a wrap-up) is a group of one.
pub(super) struct TurnGroup {
    pub chat_id: i64,
    pub turn: Option<TurnNumber>,
    pub seqs: Vec<i64>,
    /// What an open turn is remembered by: the turn's first record in the window. A turn number is
    /// unique only inside its session, and the session is the group above this one, so the anchor
    /// names the turn a reader has open.
    pub anchor: i64,
    /// The turn's own start record, when the window holds it: what a row summarises the turn from.
    pub start_seq: Option<i64>,
    /// Present once the turn's own end record is in the window.
    pub outcome: Option<TurnOutcome>,
}

/// Groups the window into turns: a group runs while the chat id and the turn number stay the same, in
/// seq order. Grouping what the window holds, rather than what a filter keeps of it, is what lets a
/// turn stay one row when its start is filtered out.
pub(super) fn group_turns(rows: &[(i64, &EventDisplay)]) -> Vec<TurnGroup> {
    let mut groups: Vec<TurnGroup> = Vec::new();
    for (seq, display) in rows {
        let (chat_id, turn) = (display.chat_id, display.facts.turn);
        let continues = turn.is_some()
            && groups
                .last()
                .is_some_and(|group| group.chat_id == chat_id && group.turn == turn);
        if continues {
            let group = groups.last_mut().expect("a continuing group exists");
            group.seqs.push(*seq);
            if display.facts.outcome.is_some() {
                group.outcome = display.facts.outcome;
            }
        } else {
            groups.push(TurnGroup {
                chat_id,
                turn,
                seqs: vec![*seq],
                anchor: *seq,
                start_seq: (display.kind == Some(EventKind::TurnStarted)).then_some(*seq),
                outcome: display.facts.outcome,
            });
        }
    }
    groups
}

/// One session's events in the order they happened, its turns underneath.
pub(super) struct SessionGroup {
    /// `None` for a host record (a purge): it has no run and no header row.
    pub session: Option<SessionRef>,
    pub chat_id: i64,
    pub turns: Vec<TurnGroup>,
    pub totals: SessionTotals,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SessionTotals {
    /// Distinct turn numbers the session carries.
    pub turns: usize,
    pub steps: usize,
    pub tools: usize,
    /// Prompt plus completion tokens summed over `StepCompleted` rows.
    pub tokens: u64,
}

/// Groups the window into sessions, then each session's rows into turns with the existing rule.
/// Consecutive rows with the same `session` form one group; a `None` session is its own group, so a
/// host record splits the run it interrupts.
pub(super) fn group_sessions(rows: &[(i64, &EventDisplay)]) -> Vec<SessionGroup> {
    let mut groups: Vec<SessionGroup> = Vec::new();
    let mut start = 0;
    for end in 1..=rows.len() {
        let changed = end == rows.len() || rows[end].1.session != rows[start].1.session;
        if !changed {
            continue;
        }
        let chunk = &rows[start..end];
        groups.push(SessionGroup {
            session: chunk[0].1.session,
            chat_id: chunk[0].1.chat_id,
            turns: group_turns(chunk),
            totals: totals(chunk.iter().map(|(_, display)| *display)),
        });
        start = end;
    }
    groups
}

/// What a session's rows add up to, over exactly the rows given.
pub(super) fn totals<'a>(displays: impl IntoIterator<Item = &'a EventDisplay>) -> SessionTotals {
    let mut totals = SessionTotals::default();
    let mut turns: Vec<TurnNumber> = Vec::new();
    for display in displays {
        match display.kind {
            Some(EventKind::StepCompleted) => totals.steps += 1,
            Some(EventKind::ToolCall) => totals.tools += 1,
            _ => {}
        }
        if let Some((prompt, completion)) = display.facts.tokens {
            totals.tokens += u64::from(prompt) + u64::from(completion);
        }
        if let Some(turn) = display.facts.turn
            && !turns.contains(&turn)
        {
            turns.push(turn);
        }
    }
    totals.turns = turns.len();
    totals
}

/// Seq-ascending sliding window over the events matching `chat_filter` and
/// `kind_filter`. Query errors propagate as `Result`; the `at_start` edge is
/// decided by what a query returned (a short or empty page), so a transient
/// error never poisons it and the load can be retried.
pub(super) struct EventStore {
    repos: Repos,
    pub chat_filter: Option<ChatId>,
    pub kind_filter: Option<EventKind>,
    pub session_filter: Option<String>,

    window: Vec<Event>,
    pub at_start: bool,

    capacity: usize,
    cache: HashMap<i64, EventDisplay>,
}

impl EventStore {
    pub fn new(
        repos: Repos,
        chat_filter: Option<ChatId>,
        kind_filter: Option<EventKind>,
        session_filter: Option<String>,
    ) -> Self {
        Self {
            repos,
            chat_filter,
            kind_filter,
            session_filter,
            window: Vec::new(),
            at_start: false,
            capacity: PAGE_SIZE * 2,
            cache: HashMap::new(),
        }
    }

    pub fn set_viewport(&mut self, lines: usize) {
        self.capacity = (lines * 3).max(PAGE_SIZE * 2);
    }

    pub fn window(&self) -> &[Event] {
        &self.window
    }

    /// The window's displays, paired with their seqs, oldest first: what a pane groups and filters
    /// over. Every display is parsed at most once and read from the cache, so a rebuild costs no
    /// payload parse.
    pub fn displays(&mut self) -> Vec<(i64, &EventDisplay)> {
        let seqs: Vec<i64> = self.window.iter().map(|event| event.seq).collect();
        for &seq in &seqs {
            // Fills the cache; the rows are read back below, once every display is in place.
            let _ = self.display_by_seq(seq);
        }
        seqs.iter()
            .filter_map(|&seq| self.cache.get(&seq).map(|display| (seq, display)))
            .collect()
    }

    /// The window is seq-ascending, so a lookup is a binary search rather than a scan: a frame asks for
    /// every row it shows.
    pub fn event_by_seq(&self, seq: i64) -> Option<&Event> {
        let index = self
            .window
            .binary_search_by_key(&seq, |event| event.seq)
            .ok()?;
        self.window.get(index)
    }

    /// Cached display for one seq, so an event is deserialized at most once. Borrowed rather than
    /// cloned: a `TurnStarted`'s detail runs to tens of kilobytes, and every redraw reads it.
    pub fn display_by_seq(&mut self, seq: i64) -> Option<&EventDisplay> {
        if !self.cache.contains_key(&seq) {
            let event = self.event_by_seq(seq)?;
            let display = EventDisplay::from_event(event).unwrap_or_else(EventDisplay::unparsed);
            self.cache.insert(seq, display);
        }
        self.cache.get(&seq)
    }

    pub fn min_seq(&self) -> Option<i64> {
        self.window.first().map(|e| e.seq)
    }

    pub fn max_seq(&self) -> Option<i64> {
        self.window.last().map(|e| e.seq)
    }

    /// The page the store's filters ask for; a caller narrows it with a struct update.
    fn page(&self) -> EventQuery<'_> {
        EventQuery {
            chat: self.chat_filter,
            kind: self.kind_filter,
            session: self.session_filter.as_deref(),
            before: None,
            after: None,
            desc: true,
            limit: self.capacity,
        }
    }

    pub async fn load_end(&mut self) -> crate::error::Result<()> {
        let mut events = self.repos.event.query(self.page()).await?;
        events.reverse(); // query returns newest-first; the window is seq-ascending
        // A short page means nothing older exists.
        self.at_start = events.len() < self.capacity;
        self.set_window(events);
        Ok(())
    }

    pub async fn extend_back(&mut self) -> crate::error::Result<usize> {
        let Some(min) = self.min_seq() else {
            self.load_end().await?;
            return Ok(0);
        };
        let mut older = self
            .repos
            .event
            .query(EventQuery {
                before: Some(min),
                ..self.page()
            })
            .await?;
        older.reverse();
        if older.is_empty() {
            self.at_start = true;
            return Ok(0);
        }
        self.at_start = false;
        let n = older.len();
        self.window.splice(0..0, older);
        Ok(n)
    }

    pub async fn append_new(&mut self) -> crate::error::Result<usize> {
        let after = self.max_seq();
        let events = self
            .repos
            .event
            .query(EventQuery {
                after,
                desc: false,
                ..self.page()
            })
            .await?;
        if events.is_empty() {
            return Ok(0);
        }
        let n = events.len();
        self.window.extend(events);
        Ok(n)
    }

    pub async fn count_new(&mut self) -> crate::error::Result<usize> {
        let Some(after) = self.max_seq() else {
            return Ok(0);
        };
        let count = self
            .repos
            .event
            .count_after(
                after,
                self.chat_filter,
                self.kind_filter,
                self.session_filter.as_deref(),
            )
            .await?;
        Ok(count.max(0) as usize)
    }

    fn set_window(&mut self, events: Vec<Event>) {
        self.window = events;
        self.trim_to_capacity_back();
    }

    // Only a freshly loaded window is trimmed; prepends and appends keep every
    // loaded event so browsed history and the selection stay addressable.
    fn trim_to_capacity_back(&mut self) {
        if self.window.len() > self.capacity {
            self.window.truncate(self.capacity);
        }
        self.trim_cache();
    }

    /// Drops entries that left the window once the cache outgrows it.
    fn trim_cache(&mut self) {
        if self.cache.len() > self.capacity * 4 {
            let window: HashSet<i64> = self.window.iter().map(|event| event.seq).collect();
            self.cache.retain(|seq, _| window.contains(seq));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::display::EventFacts,
        domain::vo::{SessionId, SessionKind},
    };

    fn display(
        kind: EventKind,
        chat_id: i64,
        turn: Option<usize>,
        outcome: Option<TurnOutcome>,
    ) -> EventDisplay {
        EventDisplay {
            tag: "T",
            one_liner: String::new(),
            sections: Vec::new(),
            chat_id,
            session: None,
            kind: Some(kind),
            color: (0, 0, 0),
            facts: EventFacts {
                turn: turn.map(TurnNumber),
                tool: None,
                ok: None,
                outcome,
                tokens: None,
            },
        }
    }

    fn successful(tools: usize) -> TurnOutcome {
        TurnOutcome {
            tools: Some(tools),
            elapsed_ms: Some(1200),
            failed: false,
            replied: true,
        }
    }

    /// A turn is one group; a chat switch, a new turn number, or a record carrying no turn starts a
    /// new one — and the group ends up with the outcome its end record reported and the start its own
    /// record gave it.
    #[test]
    fn turns_group_by_chat_and_turn() {
        let (first, second) = (
            display(EventKind::TurnStarted, 1, Some(1), None),
            display(EventKind::TurnEnded, 1, Some(1), Some(successful(2))),
        );
        let (other_chat, next_turn) = (
            display(EventKind::TurnStarted, 2, Some(1), None),
            display(EventKind::TurnStarted, 1, Some(2), None),
        );
        let (wrap_up, wrap_up_again) = (
            display(EventKind::WrapUpStarted, 1, None, None),
            display(EventKind::WrapUpStarted, 1, None, None),
        );
        let rows = [
            (10, &first),
            (11, &second),
            (12, &other_chat),
            (13, &next_turn),
            (14, &wrap_up),
            (15, &wrap_up_again),
        ];

        let groups = group_turns(&rows);

        assert_eq!(groups.len(), 5, "one per turn, chat, or turn-less record");
        assert_eq!(groups[0].seqs, [10, 11]);
        assert_eq!(groups[0].turn, Some(TurnNumber(1)));
        assert_eq!(groups[0].outcome.expect("the end record").tools, Some(2));
        assert_eq!(groups[0].start_seq, Some(10), "the turn's own start");
        assert_eq!(groups[1].chat_id, 2, "another chat is another group");
        assert_eq!(groups[3].turn, None);
        assert_eq!(
            groups[3].start_seq, None,
            "a record with no turn has no start"
        );
        assert_eq!(groups[4].seqs, [15], "a turn-less record stands alone");
    }

    fn in_session(
        session: Option<SessionRef>,
        kind: EventKind,
        chat_id: i64,
        turn: Option<usize>,
    ) -> EventDisplay {
        EventDisplay {
            session,
            ..display(kind, chat_id, turn, None)
        }
    }

    /// The same turn number in three sessions is three runs: the session split keeps them apart, and
    /// without it the middle turns would have merged into one group.
    #[test]
    fn turns_of_two_sessions_do_not_merge() {
        let main = SessionRef::main(SessionId::new());
        let wrap = SessionRef::wrap_up(SessionId::new(), main.id);
        let digest = SessionRef::digest(SessionId::new());
        let first = in_session(Some(main), EventKind::TurnStarted, 7, Some(1));
        let second = in_session(Some(wrap), EventKind::TurnStarted, 7, Some(1));
        let third = in_session(Some(digest), EventKind::TurnStarted, 7, Some(1));
        let displays = [(1, &first), (2, &second), (3, &third)];

        let groups = group_sessions(&displays);

        assert_eq!(groups.len(), 3, "one group per session");
        assert!(groups.iter().all(|group| group.turns.len() == 1));
        assert_eq!(
            groups[1].session.map(|session| session.kind),
            Some(SessionKind::WrapUp)
        );
        assert_eq!(
            groups[2].session.map(|session| session.kind),
            Some(SessionKind::Digest)
        );
    }

    /// A session's header totals the rows it holds: turns it names, steps, tool calls and tokens.
    #[test]
    fn a_session_header_counts_its_own_rows() {
        let session = SessionRef::main(SessionId::new());
        let started = in_session(Some(session), EventKind::TurnStarted, 7, Some(1));
        let mut step = in_session(Some(session), EventKind::StepCompleted, 7, Some(1));
        step.facts.tokens = Some((100, 20));
        let call = in_session(Some(session), EventKind::ToolCall, 7, Some(1));
        let displays = [(1, &started), (2, &step), (3, &call)];

        let groups = group_sessions(&displays);

        let totals = groups[0].totals;
        assert_eq!(
            (totals.turns, totals.steps, totals.tools, totals.tokens),
            (1, 1, 1, 120)
        );
    }
}
