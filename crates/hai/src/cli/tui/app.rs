use std::time::Duration;

use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use super::event_store::EventStore;
use crate::cli::display::{self, EventDisplay};

const POLL_MS: u64 = 200;
const PAGE_STEP: usize = 10;
const CTRL_STEP: usize = 15;
const PADDING: usize = 3;

// ── Action ──────────────────────────────────────────────────────

enum Action {
    JumpStart,
    JumpEnd,
    ExtendBack,
    ApplyChatFilter(Option<i64>),
}

// ── Types ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    List,
    Detail,
}

enum DetailLayout {
    Split,
    Full,
}

#[derive(Debug, Clone, Copy)]
enum Nav {
    MoveUp(usize),
    MoveDown(usize),
    GoTop,
    GoEnd,
    ScrollUp(u16),
    ScrollDown(u16),
    ToggleFocus,
    OpenOrToggleFullscreen,
    CloseDetail,
    Search,
    FilterChat,
}

// ── TuiApp ──────────────────────────────────────────────────────

pub(super) struct TuiApp {
    store: EventStore,

    /// Indices into store.window() for events matching text_filter
    visible: Vec<usize>,
    selected: usize,
    list_offset: usize,
    list_vp_h: usize,
    focus: Focus,
    detail_layout: DetailLayout,
    detail_seq: Option<i64>,
    detail_scroll: u16,
    text_filter: String,
    search_active: bool,
    chat_filter_input: String,
    chat_filter_active: bool,
    follow: bool,
    back_loading: bool,
    pending: Vec<Action>,
}

impl TuiApp {
    fn new(db: toasty::Db, chat_filter: Option<i64>, kind_filter: Option<String>) -> Self {
        Self {
            store: EventStore::new(db, chat_filter, kind_filter),
            visible: Vec::new(),
            selected: 0,
            list_offset: 0,
            list_vp_h: 20,
            focus: Focus::List,
            detail_layout: DetailLayout::Split,
            detail_seq: None,
            detail_scroll: 0,
            text_filter: String::new(),
            search_active: false,
            chat_filter_input: String::new(),
            chat_filter_active: false,
            follow: false,
            back_loading: false,
            pending: Vec::new(),
        }
    }

    async fn load_initial(&mut self) -> crate::error::Result<()> {
        self.store.load_end().await;
        self.rebuild_visible();
        self.follow_end();
        Ok(())
    }

    fn rebuild_visible(&mut self) {
        self.visible = if self.text_filter.is_empty() {
            (0..self.store.window().len()).collect()
        } else {
            self.store
                .window()
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    EventDisplay::from_event(e)
                        .is_some_and(|d| d.one_liner.contains(&self.text_filter))
                })
                .map(|(i, _)| i)
                .collect()
        };
        let max = self.visible.len().saturating_sub(1);
        self.selected = self.selected.min(max);
    }

    fn select_at(&mut self, index: usize) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = index.clamp(0, self.visible.len() - 1);
        self.follow = self.selected == self.visible.len() - 1;
        if self.detail_seq.is_some() {
            self.detail_seq = Some(self.store.window()[self.visible[self.selected]].seq);
            self.detail_scroll = 0;
        }
        self.clamp_offset();
    }

    fn move_selection(&mut self, delta: isize) {
        let new = (self.selected as isize + delta).max(0) as usize;
        self.select_at(new.min(self.visible.len().saturating_sub(1)));
    }

    fn follow_end(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = self.visible.len() - 1;
        self.follow = true;
        self.detail_seq = Some(self.store.window()[self.visible[self.selected]].seq);
        self.detail_scroll = 0;
        self.clamp_offset();
    }

    fn selected_seq(&self) -> Option<i64> {
        self.visible
            .get(self.selected)
            .map(|&i| self.store.window()[i].seq)
    }

    fn select_seq(&mut self, seq: i64) {
        self.selected = self
            .visible
            .iter()
            .position(|&i| self.store.window()[i].seq == seq)
            .unwrap_or(0);
    }

    fn scroll_detail(&mut self, delta: i16) {
        let max = self.detail_text_lines().saturating_sub(1);
        let new = (self.detail_scroll as i16 + delta).max(0) as u16;
        self.detail_scroll = new.min(max);
    }

    // ── Nav ─────────────────────────────────────────────────────

    fn key_to_nav(code: KeyCode, modifiers: KeyModifiers, focus: Focus) -> Option<Nav> {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match (code, ctrl, focus) {
            (KeyCode::Up | KeyCode::Char('k'), false, Focus::List) => Some(Nav::MoveUp(1)),
            (KeyCode::Down | KeyCode::Char('j'), false, Focus::List) => Some(Nav::MoveDown(1)),
            (KeyCode::PageUp, _, Focus::List) => Some(Nav::MoveUp(PAGE_STEP)),
            (KeyCode::PageDown, _, Focus::List) => Some(Nav::MoveDown(PAGE_STEP)),
            (KeyCode::Char('u'), true, Focus::List) => Some(Nav::MoveUp(CTRL_STEP)),
            (KeyCode::Char('d'), true, Focus::List) => Some(Nav::MoveDown(CTRL_STEP)),
            (KeyCode::Char('g'), false, Focus::List) => Some(Nav::GoTop),
            (KeyCode::Char('G'), false, Focus::List) => Some(Nav::GoEnd),

            (KeyCode::Up | KeyCode::Char('k'), _, Focus::Detail) => Some(Nav::ScrollUp(1)),
            (KeyCode::Down | KeyCode::Char('j'), _, Focus::Detail) => Some(Nav::ScrollDown(1)),
            (KeyCode::PageUp, _, Focus::Detail) => Some(Nav::ScrollUp(PAGE_STEP as u16)),
            (KeyCode::PageDown, _, Focus::Detail) => Some(Nav::ScrollDown(PAGE_STEP as u16)),
            (KeyCode::Char('u'), true, Focus::Detail) => Some(Nav::ScrollUp(CTRL_STEP as u16)),
            (KeyCode::Char('d'), true, Focus::Detail) => Some(Nav::ScrollDown(CTRL_STEP as u16)),

            (KeyCode::Tab | KeyCode::BackTab, _, _) => Some(Nav::ToggleFocus),
            (KeyCode::Enter | KeyCode::Right, _, _) => Some(Nav::OpenOrToggleFullscreen),
            (KeyCode::Esc | KeyCode::Left, _, _) => Some(Nav::CloseDetail),
            (KeyCode::Char('/'), _, _) => Some(Nav::Search),
            (KeyCode::Char('c'), _, _) => Some(Nav::FilterChat),
            _ => None,
        }
    }

    fn apply_nav(&mut self, nav: Nav) {
        match nav {
            Nav::MoveUp(n) => self.move_selection(-(n as isize)),
            Nav::MoveDown(n) => self.move_selection(n as isize),
            Nav::GoTop => self.pending.push(Action::JumpStart),
            Nav::GoEnd => self.pending.push(Action::JumpEnd),
            Nav::ScrollUp(n) => self.scroll_detail(-(n as i16)),
            Nav::ScrollDown(n) => self.scroll_detail(n as i16),
            Nav::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::List => Focus::Detail,
                    Focus::Detail => Focus::List,
                }
            }
            Nav::OpenOrToggleFullscreen => {
                if self.focus == Focus::Detail {
                    self.detail_layout = match self.detail_layout {
                        DetailLayout::Split => DetailLayout::Full,
                        DetailLayout::Full => DetailLayout::Split,
                    };
                } else if self.detail_seq.is_some() {
                    self.detail_seq = None;
                    self.detail_scroll = 0;
                    self.follow = false;
                } else if !self.visible.is_empty() {
                    self.detail_seq = Some(self.store.window()[self.visible[self.selected]].seq);
                    self.detail_scroll = 0;
                }
            }
            Nav::CloseDetail => {
                if self.detail_seq.is_some() {
                    self.detail_seq = None;
                    self.detail_scroll = 0;
                    self.follow = false;
                    self.focus = Focus::List;
                }
            }
            Nav::Search if !self.search_active => {
                self.search_active = true;
                self.text_filter.clear();
            }
            Nav::Search => {}
            Nav::FilterChat if !self.search_active && !self.chat_filter_active => {
                self.chat_filter_active = true;
                self.chat_filter_input.clear();
            }
            Nav::FilterChat => {}
        }
    }

    // ── Run ─────────────────────────────────────────────────────

    async fn run(&mut self) -> anyhow::Result<()> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
        use ratatui::{Terminal, backend::CrosstermBackend};

        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.load_initial().await?;

        let mut tick = tokio::time::interval(Duration::from_millis(POLL_MS));
        loop {
            tokio::select! {
                _ = tick.tick() => self.tick().await,
                result = self.handle_input() => if result == Control::Quit { break; },
            }
            terminal.draw(|f| self.render(f))?;
        }

        disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;
        Ok(())
    }

    async fn handle_input(&mut self) -> Control {
        if !event::poll(Duration::from_millis(10)).ok().unwrap_or(false) {
            return Control::Continue;
        }
        let event::Event::Key(key) = event::read().unwrap() else {
            return Control::Continue;
        };
        if key.kind != KeyEventKind::Press {
            return Control::Continue;
        }

        if self.search_active {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.search_active = false;
                    self.rebuild_visible();
                }
                KeyCode::Char(c) => {
                    self.text_filter.push(c);
                    self.rebuild_visible();
                }
                KeyCode::Backspace => {
                    self.text_filter.pop();
                    self.rebuild_visible();
                }
                _ => {}
            }
            return Control::Continue;
        }

        if self.chat_filter_active {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.chat_filter_active = false;
                    let filter = if self.chat_filter_input.is_empty() {
                        None
                    } else {
                        self.chat_filter_input
                            .parse::<i64>()
                            .ok()
                            .filter(|&n| n != 0)
                    };
                    self.pending.push(Action::ApplyChatFilter(filter));
                }
                KeyCode::Char(c) if c.is_ascii_digit() || c == '-' => {
                    self.chat_filter_input.push(c);
                }
                KeyCode::Backspace => {
                    self.chat_filter_input.pop();
                }
                _ => {}
            }
            return Control::Continue;
        }

        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            return Control::Quit;
        }

        if let Some(nav) = Self::key_to_nav(key.code, key.modifiers, self.focus) {
            self.apply_nav(nav);
        }

        Control::Continue
    }

    async fn tick(&mut self) {
        let actions: Vec<Action> = std::mem::take(&mut self.pending);
        for action in actions {
            match action {
                Action::JumpStart => {
                    self.store.load_start().await;
                    self.rebuild_visible();
                    self.select_at(0);
                    self.list_offset = 0;
                }
                Action::JumpEnd => {
                    self.store.load_end().await;
                    self.rebuild_visible();
                    self.follow_end();
                }
                Action::ExtendBack => {
                    self.back_loading = true;
                    let prev = self.selected_seq();
                    let n = self.store.extend_back().await;
                    self.back_loading = false;
                    if n > 0 {
                        self.rebuild_visible();
                        if let Some(seq) = prev {
                            self.select_seq(seq);
                        }
                        self.clamp_offset();
                    }
                }
                Action::ApplyChatFilter(filter) => {
                    self.store.chat_filter = filter;
                    self.store.load_end().await;
                    self.rebuild_visible();
                    self.follow_end();
                }
            }
        }

        let new = self.store.poll().await;

        if new > 0 {
            self.rebuild_visible();
            if self.follow {
                self.follow_end();
            }
        }
    }

    // ── Render ──────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        self.store.set_viewport(self.list_vp_h);

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());
        let Some([top, body, bot]) = chunks.first_chunk::<3>() else {
            return;
        };

        self.render_header(f, *top);
        self.render_body(f, *body);
        self.render_status(f, *bot);

        if self.search_active {
            self.render_search(f, f.area(), " search ");
        } else if self.chat_filter_active {
            self.render_search(f, f.area(), " chat filter ");
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let left = format!(
            " hai {} {}/{}events ",
            self.store
                .chat_filter
                .map(|c| format!("Chat {c:+}"))
                .unwrap_or_default(),
            self.visible.len(),
            self.store.window().len(),
        );
        let right = if self.chat_filter_active {
            format!(" chat: {} ", self.chat_filter_input)
        } else if !self.text_filter.is_empty() {
            format!(" filter: {} ", self.text_filter)
        } else {
            String::new()
        };

        f.render_widget(
            Line::from(vec![
                Span::styled(left, Style::new().bold()),
                Span::raw(" "),
                Span::styled(right, Style::new().fg(Color::DarkGray).italic()),
            ]),
            area,
        );
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let label = match (self.focus, self.detail_seq.as_ref(), &self.detail_layout) {
            (Focus::Detail, Some(_), DetailLayout::Full) => " detail fullscreen ",
            (Focus::Detail, Some(_), DetailLayout::Split) => " detail ",
            _ => " list ",
        };
        let nav: &str = match self.focus {
            Focus::List => "↑↓/PgUp/PgDn/Ctrl+U/D events",
            Focus::Detail => "↑↓/PgUp/PgDn scroll",
        };

        f.render_widget(
            Line::from(vec![
                Span::styled(label, Style::new().fg(Color::Black).bg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(nav, Style::new().fg(Color::DarkGray)),
                Span::raw("  Tab focus  Enter full  / search  g start  G end  c chat  q quit"),
            ]),
            area,
        );
    }

    fn render_body(&mut self, f: &mut Frame, area: Rect) {
        let is_full = matches!(self.detail_layout, DetailLayout::Full);
        let present = self.detail_seq.is_some();

        let (lp, dp) = match (present, is_full) {
            (true, true) => (0, 100),
            (true, false) => (50, 50),
            (false, _) => (100, 0),
        };

        let chunks = Layout::horizontal([Constraint::Percentage(lp), Constraint::Percentage(dp)])
            .split(area);
        let Some([lp_area, dp_area]) = chunks.first_chunk::<2>() else {
            return;
        };

        if lp > 0 {
            self.render_list(f, *lp_area);
        }
        if dp > 0 {
            self.render_detail(f, *dp_area);
        }
    }

    fn render_list(&mut self, f: &mut Frame, area: Rect) {
        let focus = match self.focus {
            Focus::List => Color::White,
            Focus::Detail => Color::DarkGray,
        };

        let block = Block::default()
            .title(" events ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(focus));

        self.list_vp_h = block.inner(area).height as usize;
        let max_off = self.visible.len().saturating_sub(self.list_vp_h);
        self.list_offset = self.list_offset.min(max_off);

        let window = self.store.window();
        let items: Vec<ListItem> = self
            .visible
            .iter()
            .filter_map(|&i| {
                let e = &window[i];
                let d = EventDisplay::from_event(e)?;
                let (cr, cg, cb) = display::color_rgb(e);
                Some(ListItem::new(Line::from(Span::styled(
                    format!(
                        "#{:>5}  {}  {}  {}",
                        e.seq,
                        display::fmt_time(e.created_at),
                        display::chat_display(e),
                        d.one_liner
                    ),
                    Style::new().fg(Color::Rgb(cr, cg, cb)),
                ))))
            })
            .collect();

        let mut state = ListState::default()
            .with_offset(self.list_offset)
            .with_selected(Some(self.selected));

        f.render_stateful_widget(
            List::new(items).block(block).highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightYellow)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            area,
            &mut state,
        );
    }

    fn clamp_offset(&mut self) {
        if self.visible.is_empty() || self.list_vp_h == 0 {
            return;
        }
        let max_off = self.visible.len().saturating_sub(self.list_vp_h);

        if self.selected < self.list_offset {
            self.list_offset = self.selected;
        }
        if self.selected >= self.list_offset + self.list_vp_h {
            self.list_offset = self
                .selected
                .saturating_sub(self.list_vp_h.saturating_sub(1));
        }
        self.list_offset = self.list_offset.min(max_off);

        let visual = self.selected - self.list_offset;
        if visual < PADDING && self.list_offset > 0 {
            self.list_offset = self.list_offset.saturating_sub(PADDING - visual);
        }
        if visual >= self.list_vp_h.saturating_sub(PADDING) {
            let overshoot = visual.saturating_sub(self.list_vp_h.saturating_sub(PADDING + 1));
            self.list_offset = (self.list_offset + overshoot).min(max_off);
        }
        self.list_offset = self.list_offset.min(max_off);

        if self.list_offset < PADDING && !self.store.at_start && !self.back_loading {
            self.pending.push(Action::ExtendBack);
        }
    }

    fn render_detail(&mut self, f: &mut Frame, area: Rect) {
        let focus = match self.focus {
            Focus::Detail => Color::White,
            Focus::List => Color::DarkGray,
        };

        let block = Block::default()
            .title(" detail ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(focus));

        let text = self
            .detail_event()
            .and_then(EventDisplay::from_event)
            .map(|d| d.detail_text)
            .unwrap_or_default();
        let scroll_len: usize = text.lines().count().max(1);
        let inner = block.inner(area);

        f.render_widget(
            Paragraph::new(Text::from(text))
                .block(block)
                .scroll((self.detail_scroll, 0))
                .wrap(Wrap { trim: false }),
            area,
        );

        let pos = (self.detail_scroll as usize).min(scroll_len.saturating_sub(1));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut ScrollbarState::new(scroll_len).position(pos),
        );
    }

    fn detail_event(&self) -> Option<&crate::domain::model::Event> {
        self.detail_seq
            .and_then(|seq| self.store.window().iter().find(|e| e.seq == seq))
    }

    fn detail_text_lines(&self) -> u16 {
        self.detail_event()
            .and_then(EventDisplay::from_event)
            .map(|d| d.detail_text.lines().count() as u16)
            .unwrap_or(0)
    }

    fn render_search(&self, f: &mut Frame, area: Rect, title: &str) {
        let popup = centered_rect(50, 3, area);
        f.render_widget(
            Paragraph::new(if self.chat_filter_active {
                self.chat_filter_input.as_str()
            } else {
                self.text_filter.as_str()
            })
            .block(Block::default().title(title).borders(Borders::ALL)),
            popup,
        );
        let input_len = if self.chat_filter_active {
            self.chat_filter_input.len()
        } else {
            self.text_filter.len()
        };
        f.set_cursor_position((popup.x + 1 + input_len as u16, popup.y + 1));
    }
}

#[derive(PartialEq)]
enum Control {
    Continue,
    Quit,
}

// ── Entry point ─────────────────────────────────────────────────

pub async fn run_tui(
    db: toasty::Db,
    chat_filter: Option<i64>,
    kind_filter: Option<String>,
) -> anyhow::Result<()> {
    TuiApp::new(db, chat_filter, kind_filter).run().await
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height * (100 - percent_y) / 200).max(1)),
            Constraint::Length(percent_y),
            Constraint::Length((r.height * (100 - percent_y) / 200).max(1)),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width * (100 - percent_x) / 200).max(1)),
            Constraint::Length((r.width * percent_x / 100).max(1)),
            Constraint::Length((r.width * (100 - percent_x) / 200).max(1)),
        ])
        .split(popup[1])[1]
}
