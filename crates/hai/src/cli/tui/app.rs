use std::time::Duration;

use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
};

use super::input::CTRL_STEP;
use crate::{cli::display, domain::model::Event};

const POLL_MS: u64 = 200;

// ── Focus / Layout ──────────────────────────────────────────────

#[derive(Clone, Copy)]
pub enum Focus {
    List,
    Detail,
}

pub enum DetailLayout {
    Split,
    Full,
}

// ── TuiApp ──────────────────────────────────────────────────────

pub struct TuiApp {
    pub(super) events: Vec<Event>,
    pub(super) selected: usize,
    pub(super) list_offset: usize,
    pub(super) list_vp_h: usize,
    pub(super) focus: Focus,
    pub(super) detail_layout: DetailLayout,
    pub(super) detail_seq: Option<i64>,
    pub(super) detail_scroll: u16,
    pub(super) filter: String,
    pub(super) search_active: bool,
    pub(super) last_seq: i64,
    pub(super) chat_filter: Option<i64>,
    pub(super) kind_filter: Option<String>,
    pub(super) db: toasty::Db,
}

impl TuiApp {
    pub fn new(db: toasty::Db) -> Self {
        Self {
            events: Vec::new(),
            selected: 0,
            list_offset: 0,
            list_vp_h: 20,
            focus: Focus::List,
            detail_layout: DetailLayout::Split,
            detail_seq: None,
            detail_scroll: 0,
            filter: String::new(),
            search_active: false,
            last_seq: 0,
            chat_filter: None,
            kind_filter: None,
            db,
        }
    }

    pub fn set_chat_filter(&mut self, chat_id: Option<i64>) {
        self.chat_filter = chat_id;
    }

    pub fn set_kind_filter(&mut self, kind: Option<String>) {
        self.kind_filter = kind;
    }

    async fn load_initial(&mut self) -> crate::error::Result<()> {
        let events =
            display::query_events(&self.db, self.chat_filter, self.kind_filter.as_deref(), 200)
                .await?;
        self.last_seq = events.first().map(|e| e.seq).unwrap_or(0);
        self.events = events;
        self.events.reverse();
        self.selected = self.events.len().saturating_sub(1);
        if !self.events.is_empty() {
            self.detail_seq = Some(self.events[self.selected].seq);
        }
        Ok(())
    }

    async fn poll_new(&mut self) -> crate::error::Result<()> {
        let events = display::query_new_events(
            &self.db,
            self.last_seq,
            self.chat_filter,
            self.kind_filter.as_deref(),
        )
        .await?;
        if !events.is_empty() {
            self.last_seq = events.last().map(|e| e.seq).unwrap_or(self.last_seq);
            for e in events {
                self.events.push(e);
            }
            self.selected = self.events.len().saturating_sub(1);
        }
        Ok(())
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
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
                _ = tick.tick() => { let _ = self.poll_new().await; }
                result = self.handle_input() => {
                    if result == Control::Quit { break; }
                }
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

    // ── Input ──────────────────────────────────────────────────

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

        if key.code == KeyCode::Char('q') || key.code == KeyCode::Char('Q') {
            return Control::Quit;
        }

        if self.search_active {
            self.handle_search_input(key.code);
            return Control::Continue;
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.toggle_focus();
            }
            KeyCode::Esc | KeyCode::Left
                if matches!(self.focus, Focus::Detail) && self.detail_seq.is_some() =>
            {
                self.close_detail()
            }
            KeyCode::Enter | KeyCode::Right => self.open_detail(),
            KeyCode::Char('/') => {
                self.search_active = true;
                self.filter.clear();
            }

            // 列表导航
            KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Char('g')
            | KeyCode::Char('G')
                if matches!(self.focus, Focus::List) =>
            {
                self.handle_list_input(key.code);
            }

            // 详情导航
            KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::PageUp
            | KeyCode::PageDown
                if matches!(self.focus, Focus::Detail) =>
            {
                self.handle_detail_input(key.code);
            }

            // Ctrl+U/D
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.focus {
                    Focus::List => {
                        self.selected = self.selected.saturating_sub(CTRL_STEP);
                        self.clamp_list_offset();
                    }
                    Focus::Detail => {
                        self.detail_scroll = self.detail_scroll.saturating_sub(CTRL_STEP as u16)
                    }
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.focus {
                    Focus::List => {
                        self.selected =
                            (self.selected + CTRL_STEP).min(self.events.len().saturating_sub(1));
                        self.clamp_list_offset();
                    }
                    Focus::Detail => {
                        let max = self.detail_text_lines().saturating_sub(1) as u16;
                        self.detail_scroll = (self.detail_scroll + CTRL_STEP as u16).min(max);
                    }
                }
            }
            _ => {}
        }
        Control::Continue
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::Detail,
            Focus::Detail => Focus::List,
        };
    }

    fn open_detail(&mut self) {
        if self.detail_seq.is_some() {
            self.detail_layout = match self.detail_layout {
                DetailLayout::Split => DetailLayout::Full,
                DetailLayout::Full => DetailLayout::Split,
            };
        } else if !self.events.is_empty() {
            self.detail_seq = Some(self.events[self.selected].seq);
            self.detail_scroll = 0;
            self.focus = Focus::Detail;
        }
    }

    fn close_detail(&mut self) {
        match self.detail_layout {
            DetailLayout::Full => self.detail_layout = DetailLayout::Split,
            DetailLayout::Split => {
                self.detail_seq = None;
                self.focus = Focus::List;
                self.detail_scroll = 0;
            }
        }
    }

    // ── Render ─────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        let [top, body, bot] = *Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area()) else {
            return;
        };

        self.render_header(f, top);
        self.render_body(f, body);
        self.render_status(f, bot);

        if self.search_active {
            self.render_search(f, f.area());
        }
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
        if lp > 0 {
            self.render_list(f, chunks[0]);
        }
        if dp > 0 {
            self.render_detail(f, chunks[1]);
        }
    }
}

#[derive(PartialEq)]
enum Control {
    Continue,
    Quit,
}
