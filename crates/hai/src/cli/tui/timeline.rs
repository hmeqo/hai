//! The event log timeline: the turn groups, the rows inside them, and where the selection sits.

use std::collections::{HashMap, HashSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use super::{
    event_store::{EventStore, SessionGroup, TurnGroup, group_sessions, totals},
    filter::Terms,
};
use crate::{
    cli::display::{self, EventDisplay, compact_count},
    domain::vo::{SessionId, SessionKind},
};

const FAILED: (u8, u8, u8) = (239, 68, 68);

/// What a row stands for: a session's header, a turn's header, or one event of an open turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Level {
    Session,
    Turn,
    Event,
}

/// What the selection points at: an event's seq and its level. An open turn's first event is both its
/// header and one of its children, so the seq alone does not name a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct RowKey {
    pub seq: i64,
    pub level: Level,
}

/// One row of the timeline, and the session and turn it belongs to.
#[derive(Debug, Clone, Copy)]
struct Row {
    key: RowKey,
    /// Index into `Timeline::groups`.
    session: usize,
    /// Index into that session's turns; absent on the session header.
    turn: Option<usize>,
}

/// The window's sessions, and the rows the viewport shows of them.
pub(super) struct Timeline {
    groups: Vec<SessionGroup>,
    rows: Vec<Row>,
    /// The sessions the reader opened; every session starts as its header alone.
    open: HashSet<Option<SessionId>>,
    /// The open turns, keyed by the first record of each.
    expanded: HashSet<i64>,
    pub selected: Option<RowKey>,
    pub follow: bool,
    pub new_count: usize,
    offset: usize,
    vp_h: usize,
    pub back_loading: bool,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            rows: Vec::new(),
            open: HashSet::new(),
            expanded: HashSet::new(),
            selected: None,
            follow: false,
            new_count: 0,
            offset: 0,
            vp_h: 20,
            back_loading: false,
        }
    }

    /// Regroup the window with the query applied. Rows the reader had open stay open, and a selection
    /// whose row is still there stays put.
    pub fn rebuild(&mut self, displays: &[(i64, &EventDisplay)], query: &Terms) {
        let (groups, rows) = rows_for(displays, &self.open, &self.expanded, query);
        self.groups = groups;
        self.rows = rows;
        if let Some(key) = self.selected {
            self.selected = self.reselect(key);
        }
        self.clamp_offset();
    }

    /// Where the selection goes after a rebuild. A prepend can change which event of a turn comes
    /// first, so the row the reader was on can be gone while the turn it stood for is still there: the
    /// selection follows its event to that turn's row rather than falling back to the head, which would
    /// leave the reader at the top of every freshly loaded page.
    fn reselect(&self, key: RowKey) -> Option<RowKey> {
        if let Some(row) = self.rows.iter().find(|row| row.key == key) {
            return Some(row.key);
        }
        // The row moved to another seq: follow the event to the header of the turn it belongs to,
        // rather than leaving the reader at the top of a freshly loaded page.
        let (session, turn) = self.groups.iter().enumerate().find_map(|(index, group)| {
            group
                .turns
                .iter()
                .position(|turn| turn.seqs.contains(&key.seq))
                .map(|turn| (index, turn))
        })?;
        self.rows
            .iter()
            .find(|row| {
                row.session == session && row.turn == Some(turn) && row.key.level == Level::Turn
            })
            .map(|row| row.key)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The sessions the window holds, after the query: what the chats pane counts.
    pub fn groups(&self) -> &[SessionGroup] {
        &self.groups
    }

    pub fn vp_h(&self) -> usize {
        self.vp_h
    }

    pub fn selected_seq(&self) -> Option<i64> {
        self.selected.map(|key| key.seq)
    }

    pub fn selected_row(&self) -> Option<RowKey> {
        self.selected
    }

    /// Where the selection sits: the tail while following, the head otherwise.
    pub fn selected_index(&self) -> usize {
        let fallback = if self.follow {
            self.rows.len().saturating_sub(1)
        } else {
            0
        };
        self.selected
            .and_then(|key| self.index_of(key))
            .unwrap_or(fallback)
    }

    pub fn select_at(&mut self, index: usize) {
        if self.rows.is_empty() {
            self.selected = None;
            return;
        }
        let index = index.clamp(0, self.rows.len() - 1);
        self.selected = Some(self.rows[index].key);
        self.follow = index + 1 == self.rows.len();
        self.clamp_offset();
    }

    pub fn select_rel(&mut self, delta: isize) {
        let index = (self.selected_index() as isize + delta).max(0) as usize;
        self.select_at(index);
    }

    /// Move the selection by half the pane's visible height, or by a quarter of it.
    pub fn page(&mut self, dir: isize, half: bool) {
        let step = (self.vp_h / if half { 4 } else { 2 }).max(1) as isize;
        self.select_rel(dir * step);
    }

    /// Put the selection on the last row and keep following: the newest thing that happened, even
    /// when it is a child of an open turn.
    pub fn follow_end(&mut self) {
        self.follow = true;
        self.new_count = 0;
        self.selected = self.rows.last().map(|row| row.key);
        self.clamp_offset();
    }

    /// Open or close the session or turn a row stands for. A record with no turn of its own stands
    /// alone and opens its detail instead.
    pub fn toggle(&mut self, key: RowKey) {
        let Some(row) = self.rows.iter().find(|row| row.key == key) else {
            return;
        };
        match key.level {
            Level::Session => {
                let session = self.groups[row.session].session.map(|session| session.id);
                if !self.open.remove(&session) {
                    self.open.insert(session);
                }
            }
            Level::Turn | Level::Event => {
                let Some(turn) = row
                    .turn
                    .and_then(|index| self.groups.get(row.session)?.turns.get(index))
                else {
                    return;
                };
                if turn.turn.is_none() {
                    return;
                }
                let anchor = turn.anchor;
                if !self.expanded.remove(&anchor) {
                    self.expanded.insert(anchor);
                }
            }
        }
    }

    /// The turn group a row belongs to.
    pub fn group_of(&self, key: RowKey) -> Option<&TurnGroup> {
        let row = self.rows.iter().find(|row| row.key == key)?;
        let index = row.turn?;
        self.groups.get(row.session)?.turns.get(index)
    }

    pub fn clamp_offset(&mut self) {
        if self.rows.is_empty() || self.vp_h == 0 {
            return;
        }
        let max_off = self.rows.len().saturating_sub(self.vp_h);
        let scrolloff = (self.vp_h / 3).max(1);
        let selected = self.selected_index();
        self.offset = selected.saturating_sub(scrolloff).min(max_off);
    }

    /// Re-anchor the view after a prepend: `visual` is the selected row's distance from the viewport
    /// top before the load.
    pub fn anchor_after_prepend(&mut self, visual: usize) {
        if self.rows.is_empty() || self.vp_h == 0 {
            return;
        }
        let max_off = self.rows.len().saturating_sub(self.vp_h);
        let selected = self.selected_index();
        self.offset = selected.saturating_sub(visual).min(max_off);
    }

    pub fn visual_pos(&self) -> usize {
        self.selected_index().saturating_sub(self.offset)
    }

    fn index_of(&self, key: RowKey) -> Option<usize> {
        self.rows.iter().position(|row| row.key == key)
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, store: &mut EventStore, active: bool) {
        let border = if active {
            Color::White
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .title(" timeline ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border));

        let height = block.inner(area).height as usize;
        if height != self.vp_h {
            // The viewport changed — the first frame after the placeholder height, or a resize — so
            // the offset is recomputed around the selection instead of being left over from the old
            // height. A steady frame leaves it alone, which is what keeps a prepend's anchor.
            self.vp_h = height;
            self.clamp_offset();
        }
        let max_off = self.rows.len().saturating_sub(self.vp_h);
        self.offset = self.offset.min(max_off);

        // Only the rows the viewport shows become widgets: the window holds thousands of events, and
        // one row's summary is built from a payload that may run to tens of kilobytes.
        let end = (self.offset + self.vp_h).min(self.rows.len());
        let mut items = Vec::with_capacity(end - self.offset);
        for row in &self.rows[self.offset..end] {
            let Some(item) = self.row_item(row, store) else {
                continue;
            };
            items.push(item);
        }

        let mut state = ListState::default()
            .with_offset(0)
            .with_selected(Some(self.selected_index().saturating_sub(self.offset)));

        f.render_stateful_widget(
            List::new(items).block(block).highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
            &mut state,
        );
    }

    fn row_item(&self, row: &Row, store: &mut EventStore) -> Option<ListItem<'static>> {
        let seq = row.key.seq;
        let time = display::fmt_time(store.event_by_seq(seq)?.created_at);
        // A filter can hide a turn's start row from the rows, so its line is read by seq.
        let summary = if row.key.level == Level::Turn {
            row.turn
                .and_then(|index| self.groups.get(row.session)?.turns.get(index))
                .and_then(|group| group.start_seq)
                .filter(|start| *start != seq)
                .and_then(|start| store.display_by_seq(start))
                .map(|start| start.one_liner.clone())
        } else {
            None
        };
        let display = store.display_by_seq(seq)?;
        let (text, failed) = match row.key.level {
            Level::Session => {
                let group = self.groups.get(row.session)?;
                (session_line(group, seq, &time, &self.open), false)
            }
            Level::Event => (
                format!(
                    "    #{seq:>5}  {time}  {:+}  {}",
                    display.chat_id, display.one_liner
                ),
                false,
            ),
            Level::Turn => {
                let group = row
                    .turn
                    .and_then(|index| self.groups.get(row.session)?.turns.get(index))?;
                let main = self
                    .groups
                    .get(row.session)
                    .and_then(|group| group.session)
                    .is_none_or(|session| session.kind == SessionKind::Main);
                (
                    header_line(
                        group,
                        seq,
                        &time,
                        display,
                        summary.as_deref(),
                        &self.expanded,
                        main,
                    ),
                    group.outcome.is_some_and(|outcome| outcome.failed),
                )
            }
        };
        let (cr, cg, cb) = if failed { FAILED } else { display.color };
        Some(ListItem::new(Line::from(Span::styled(
            text,
            Style::new().fg(Color::Rgb(cr, cg, cb)),
        ))))
    }
}

/// One session as one line: what run it is, what it covered, and what its rows add up to.
fn session_line(
    group: &SessionGroup,
    seq: i64,
    time: &str,
    open: &HashSet<Option<SessionId>>,
) -> String {
    let session = group
        .session
        .expect("a session row is only pushed for a group that names one");
    let marker = if open.contains(&Some(session.id)) {
        "▾"
    } else {
        "▸"
    };
    let kind: &str = session.kind.into();
    let parent = session
        .parent
        .map(|parent| format!(" © {}", short_id(parent)))
        .unwrap_or_default();
    let totals = group.totals;
    format!(
        "#{seq:>5}  {time}  {marker}  {} · {kind}{parent} · chat {}   {} turns · {} steps · {} calls · {} tok",
        session.short(),
        group.chat_id,
        totals.turns,
        totals.steps,
        totals.tools,
        compact_count(totals.tokens),
    )
}

/// The prefix a session id is shown by; `SessionRef::short` is the same four digits for its own id.
fn short_id(id: SessionId) -> String {
    id.to_string()[..4].to_owned()
}

/// The window's displays as session groups, then turn groups, then the rows they show: one header per
/// session the query keeps, one per turn inside an open session, and one per matching event inside an
/// open turn. The query decides which events are listed and whether a group is listed at all — never
/// what a group is summarised as.
fn rows_for(
    displays: &[(i64, &EventDisplay)],
    open: &HashSet<Option<SessionId>>,
    expanded: &HashSet<i64>,
    query: &Terms,
) -> (Vec<SessionGroup>, Vec<Row>) {
    let by_seq: HashMap<i64, &EventDisplay> = displays.iter().copied().collect();

    let mut groups: Vec<SessionGroup> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    for mut group in group_sessions(displays) {
        for turn in &mut group.turns {
            turn.seqs.retain(|seq| {
                by_seq
                    .get(seq)
                    .is_some_and(|display| query.matches(*seq, display))
            });
        }
        group.turns.retain(|turn| !turn.seqs.is_empty());
        if group.turns.is_empty() {
            continue;
        }
        // The header counts what the reader sees, not what the filter hid.
        group.totals = totals(
            group
                .turns
                .iter()
                .flat_map(|turn| turn.seqs.iter())
                .filter_map(|seq| by_seq.get(seq).copied()),
        );

        let session = groups.len();
        let shown = group
            .session
            .is_none_or(|session| open.contains(&Some(session.id)));
        if group.session.is_some() {
            rows.push(Row {
                key: RowKey {
                    seq: group.turns[0].seqs[0],
                    level: Level::Session,
                },
                session,
                turn: None,
            });
        }
        if shown {
            for (index, turn) in group.turns.iter().enumerate() {
                rows.push(Row {
                    key: RowKey {
                        seq: turn.seqs[0],
                        level: Level::Turn,
                    },
                    session,
                    turn: Some(index),
                });
                if expanded.contains(&turn.anchor) {
                    rows.extend(turn.seqs.iter().map(|seq| Row {
                        key: RowKey {
                            seq: *seq,
                            level: Level::Event,
                        },
                        session,
                        turn: Some(index),
                    }));
                }
            }
        }
        groups.push(group);
    }
    (groups, rows)
}

/// One turn as one line: the marker that says whether it is open, what the turn was, and how it
/// ended. `summary` is the turn's own start line when that event is not the row itself. A turn of a
/// non-main session (a wrap-up or a digest) has no `turn-started` by design, so only a main session's
/// turn can be one the window cut off.
fn header_line(
    group: &TurnGroup,
    seq: i64,
    time: &str,
    display: &EventDisplay,
    summary: Option<&str>,
    expanded: &HashSet<i64>,
    main: bool,
) -> String {
    let Some(turn) = group.turn else {
        // A record that carries no turn of its own stands alone: its rendering is the whole row.
        return format!(
            "#{seq:>5}  {time}  {:+}  {}",
            display.chat_id, display.one_liner
        );
    };
    let marker = if expanded.contains(&group.anchor) {
        "▾"
    } else {
        "▸"
    };
    let mut line = format!("#{seq:>5}  {time}  {:+}  {marker}  ", display.chat_id);
    match (summary, group.start_seq) {
        (Some(summary), _) => line.push_str(summary),
        (None, Some(_)) => line.push_str(&display.one_liner),
        // The window begins inside the turn: say so rather than pass a child off as the whole turn.
        (None, None) if main => line.push_str(&format!("… Turn {turn}  (older events not loaded)")),
        // A pass's turn has no start record to miss; the row speaks for itself.
        (None, None) => line.push_str(&display.one_liner),
    }
    match group.outcome {
        Some(outcome) => {
            let mark = if outcome.failed {
                "✗"
            } else if outcome.replied {
                "✓"
            } else {
                "–"
            };
            line.push_str("  →");
            if let (Some(tools), Some(ms)) = (outcome.tools, outcome.elapsed_ms) {
                line.push_str(&format!("  {tools} tools  {:.1}s", ms as f64 / 1000.0));
            }
            line.push_str(&format!("  {mark}"));
        }
        // Only a turn whose start is in the window can be said to be running: another turn's end may
        // simply be further along than the window reaches.
        None if group.start_seq.is_some() => line.push_str("  running"),
        None => {}
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::display::{EventFacts, TurnOutcome},
        domain::vo::{EventKind, SessionId, SessionRef, TurnNumber},
    };

    fn display(kind: EventKind, turn: Option<usize>, outcome: Option<TurnOutcome>) -> EventDisplay {
        display_in(None, kind, turn, outcome)
    }

    fn display_in(
        session: Option<SessionRef>,
        kind: EventKind,
        turn: Option<usize>,
        outcome: Option<TurnOutcome>,
    ) -> EventDisplay {
        EventDisplay {
            tag: "T",
            one_liner: String::from("TURN  5  direct  msgs:3  by:window"),
            sections: Vec::new(),
            chat_id: 7,
            session,
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

    fn ended() -> TurnOutcome {
        TurnOutcome {
            tools: Some(1),
            elapsed_ms: Some(1200),
            failed: false,
            replied: true,
        }
    }

    /// Collapsed, a turn is one row; open, it lists the events that happened inside it.
    #[test]
    fn a_collapsed_turn_is_one_row_and_an_expanded_one_lists_its_events() {
        let (first, second) = (
            display(EventKind::TurnStarted, Some(5), None),
            display(EventKind::TurnEnded, Some(5), Some(ended())),
        );
        let displays = [(10, &first), (11, &second)];

        let (groups, rows) = rows_for(
            &displays,
            &HashSet::new(),
            &HashSet::new(),
            &Terms::default(),
        );
        assert_eq!(groups.len(), 1, "one session of one turn");
        assert_eq!(groups[0].turns.len(), 1, "one turn is one turn group");
        assert_eq!(
            groups[0].turns[0].outcome.expect("the end record").tools,
            Some(1)
        );
        assert_eq!(rows.len(), 1, "collapsed, the turn is one row");
        assert_eq!(
            rows[0].key.seq, 10,
            "the header anchors on the turn's start"
        );

        let mut expanded = HashSet::new();
        expanded.insert(10);
        let (_, rows) = rows_for(&displays, &HashSet::new(), &expanded, &Terms::default());
        assert_eq!(rows.len(), 3, "the header plus its two events");
        assert_eq!(
            rows[1].key,
            RowKey {
                seq: 10,
                level: Level::Event
            }
        );
        assert_eq!(
            rows[2].key,
            RowKey {
                seq: 11,
                level: Level::Event
            }
        );
    }

    /// A query that keeps any of a turn's events keeps the turn, showing only what matched; a query
    /// that keeps none of them drops the turn whole.
    #[test]
    fn a_filtered_turn_loses_its_rows() {
        let (first, second) = (
            display(EventKind::TurnStarted, Some(5), None),
            display(EventKind::TurnEnded, Some(5), Some(ended())),
        );
        let displays = [(10, &first), (11, &second)];
        let mut expanded = HashSet::new();
        expanded.insert(10);

        let keep_second = Terms::parse("kind:turn-ended").expect("a query that parses");
        let (groups, rows) = rows_for(&displays, &HashSet::new(), &expanded, &keep_second);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].turns[0].seqs,
            [11],
            "only the matching event is left in the turn"
        );
        assert_eq!(rows.len(), 2, "the header, plus the one event that matched");
        assert_eq!(rows[0].key.seq, 11, "the header anchors on what is left");
        assert_eq!(
            rows[1].key,
            RowKey {
                seq: 11,
                level: Level::Event
            },
            "only the matching child is listed"
        );

        let keep_nothing = Terms::parse("kind:tool-call").expect("a query that parses");
        let (groups, rows) = rows_for(&displays, &HashSet::new(), &expanded, &keep_nothing);
        assert!(
            groups.is_empty(),
            "a turn none of whose events match is gone"
        );
        assert!(rows.is_empty());
    }

    /// Following selects the bottom row: an open turn's last child, not its header, and a collapsed
    /// turn's own header.
    #[test]
    fn following_lands_on_the_last_row() {
        let (first, second) = (
            display(EventKind::TurnStarted, Some(5), None),
            display(EventKind::TurnEnded, Some(5), Some(ended())),
        );
        let displays = [(10, &first), (11, &second)];

        let mut expanded = HashSet::new();
        expanded.insert(10);
        let (groups, rows) = rows_for(&displays, &HashSet::new(), &expanded, &Terms::default());
        let mut timeline = Timeline::new();
        timeline.groups = groups;
        timeline.rows = rows;
        timeline.follow_end();
        assert_eq!(
            timeline.selected_row(),
            Some(RowKey {
                seq: 11,
                level: Level::Event
            }),
            "the open turn's last child, not its header"
        );
        assert!(timeline.follow);

        let (groups, rows) = rows_for(
            &displays,
            &HashSet::new(),
            &HashSet::new(),
            &Terms::default(),
        );
        timeline.groups = groups;
        timeline.rows = rows;
        timeline.follow_end();
        assert_eq!(
            timeline.selected_row(),
            Some(RowKey {
                seq: 10,
                level: Level::Turn
            }),
            "collapsed, the turn's own header is the bottom"
        );
    }

    /// A page moves by the pane's visible height, a quarter of it for the half-page keys.
    #[test]
    fn paging_moves_by_the_pane() {
        let turns: Vec<EventDisplay> = (1..=40)
            .map(|turn| display(EventKind::TurnStarted, Some(turn), None))
            .collect();
        let displays: Vec<(i64, &EventDisplay)> = turns
            .iter()
            .enumerate()
            .map(|(index, display)| (index as i64 + 1, display))
            .collect();

        let mut timeline = Timeline::new();
        timeline.rebuild(&displays, &Terms::default());
        assert_eq!(
            timeline.vp_h(),
            20,
            "the placeholder height until a frame sets it"
        );

        timeline.page(1, false);
        assert_eq!(timeline.selected_index(), 10, "half the pane");
        timeline.page(1, true);
        assert_eq!(timeline.selected_index(), 15, "a quarter more");
        timeline.page(-1, false);
        assert_eq!(timeline.selected_index(), 5);
    }

    /// A prepend can hand a turn events that come before the one the reader was on. The selection
    /// follows its turn there; falling back to the head would re-anchor at the top and pull page
    /// after page.
    #[test]
    fn a_prepend_keeps_the_selection() {
        let (third, fourth) = (
            display(EventKind::ToolCallResult, Some(1), None),
            display(EventKind::StepCompleted, Some(1), None),
        );
        let window = [(4, &third), (5, &fourth)];
        let mut timeline = Timeline::new();
        timeline.rebuild(&window, &Terms::default());
        timeline.select_at(0);
        assert_eq!(
            timeline.selected_row().map(|key| key.seq),
            Some(4),
            "the turn's first event in this window"
        );

        // Older events arrive: the same turn now begins three events earlier.
        let (first, second) = (
            display(EventKind::TurnStarted, Some(1), None),
            display(EventKind::ToolCall, Some(1), None),
        );
        let grown = [(1, &first), (2, &second), (4, &third), (5, &fourth)];
        timeline.rebuild(&grown, &Terms::default());

        assert_eq!(
            timeline.selected_row(),
            Some(RowKey {
                seq: 1,
                level: Level::Turn
            }),
            "the selection moved with its turn, not to the head"
        );
    }

    /// A session starts as its header alone; opening it adds its turns, closing it takes them away.
    #[test]
    fn a_session_starts_collapsed() {
        let session = SessionRef::main(SessionId::new());
        let turn = display_in(Some(session), EventKind::TurnStarted, Some(1), None);
        let displays = [(10, &turn)];

        let (groups, rows) = rows_for(
            &displays,
            &HashSet::new(),
            &HashSet::new(),
            &Terms::default(),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(rows.len(), 1, "collapsed, a session is its header alone");
        assert_eq!(rows[0].key.level, Level::Session);
        assert_eq!(rows[0].key.seq, 10);

        let mut open = HashSet::new();
        open.insert(Some(session.id));
        let (_, rows) = rows_for(&displays, &open, &HashSet::new(), &Terms::default());
        assert_eq!(rows.len(), 2, "open, the session shows its turn");
        assert_eq!(rows[1].key.level, Level::Turn);
    }

    /// The session header names the run, its parent and its totals.
    #[test]
    fn a_session_header_names_its_run_and_totals() {
        let main = SessionRef::main(SessionId::new());
        let wrap = SessionRef::wrap_up(SessionId::new(), main.id);
        let turn = display_in(Some(wrap), EventKind::TurnStarted, Some(1), Some(ended()));
        let displays = [(10, &turn)];
        let (groups, rows) = rows_for(
            &displays,
            &HashSet::new(),
            &HashSet::new(),
            &Terms::default(),
        );

        let line = session_line(&groups[0], rows[0].key.seq, "12:00:00", &HashSet::new());

        assert!(line.contains("wrap-up"), "{line}");
        assert!(
            line.contains(&format!("© {}", &main.id.to_string()[..4])),
            "the parent is named: {line}"
        );
        assert!(line.contains("1 turns"), "{line}");
    }

    /// Two sessions in one chat may each hold a turn numbered 5. Each session starts as its header,
    /// and opening one session's turn leaves the other session's turn shut.
    #[test]
    fn opening_a_turn_leaves_another_session_shut() {
        let main = SessionRef::main(SessionId::new());
        let wrap = SessionRef::wrap_up(SessionId::new(), main.id);
        let first = display_in(Some(main), EventKind::TurnStarted, Some(5), Some(ended()));
        let second = display_in(Some(wrap), EventKind::TurnStarted, Some(5), Some(ended()));
        let displays = [(10, &first), (20, &second)];

        let mut timeline = Timeline::new();
        timeline.rebuild(&displays, &Terms::default());
        assert_eq!(timeline.rows.len(), 2, "both sessions collapsed");

        for seq in [10, 20] {
            timeline.toggle(RowKey {
                seq,
                level: Level::Session,
            });
        }
        timeline.rebuild(&displays, &Terms::default());
        assert_eq!(
            timeline.rows.len(),
            4,
            "two session headers and their turns"
        );
        assert_eq!(timeline.rows[0].key.level, Level::Session);
        assert_eq!(timeline.rows[2].key.level, Level::Session);

        timeline.toggle(RowKey {
            seq: 10,
            level: Level::Turn,
        });
        timeline.rebuild(&displays, &Terms::default());
        assert_eq!(timeline.rows.len(), 5, "the opened turn shows its event");
        assert_eq!(
            timeline
                .rows
                .iter()
                .filter(|row| row.key.level == Level::Event)
                .count(),
            1,
            "one turn was opened, so one event row exists"
        );
    }
}
