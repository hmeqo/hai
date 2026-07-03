use std::time::Duration;

use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::domain::model::Event;

use super::display::{self, EventDisplay};

const POLL_MS: u64 = 200;

// ── Focus / Layout ──────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Focus {
    List,
    Detail,
}

enum DetailLayout {
    Split,
    Full,
}

// ── TuiApp ──────────────────────────────────────────────────────

pub struct TuiApp {
    events: Vec<Event>,
    selected: usize,
    focus: Focus,
    detail_layout: DetailLayout,
    detail_seq: Option<i64>,
    detail_scroll: u16,
    filter: String,
    search_active: bool,
    last_seq: i64,
    chat_filter: Option<i64>,
    kind_filter: Option<String>,
    db: toasty::Db,
}

impl TuiApp {
    pub fn new(db: toasty::Db) -> Self {
        Self {
            events: Vec::new(),
            selected: 0,
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
        let events = display::query_events(&self.db, self.chat_filter, self.kind_filter.as_deref(), 200).await?;
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
            &self.db, self.last_seq, self.chat_filter, self.kind_filter.as_deref(),
        ).await?;
        if !events.is_empty() {
            self.last_seq = events.last().map(|e| e.seq).unwrap_or(self.last_seq);
            for e in events { self.events.push(e); }
            self.selected = self.events.len().saturating_sub(1);
        }
        Ok(())
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
        use ratatui::backend::CrosstermBackend;
        use ratatui::Terminal;

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
                    if result? == Control::Quit { break; }
                }
            }

            terminal.draw(|f| self.render(f))?;
        }

        disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }

    // ── Input ──────────────────────────────────────────────────

    async fn handle_input(&mut self) -> anyhow::Result<Control> {
        if !event::poll(Duration::from_millis(10))? { return Ok(Control::Continue); }
        let event::Event::Key(key) = event::read()? else { return Ok(Control::Continue); };
        if key.kind != KeyEventKind::Press { return Ok(Control::Continue); }

        if self.search_active {
            return Ok(self.handle_search_key(key.code));
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(Control::Quit),
            KeyCode::Tab | KeyCode::BackTab => self.toggle_focus(),
            KeyCode::Esc if matches!(self.focus, Focus::Detail) && self.detail_seq.is_some() => {
                match self.detail_layout {
                    DetailLayout::Full => self.detail_layout = DetailLayout::Split,
                    DetailLayout::Split => { self.detail_seq = None; self.focus = Focus::List; }
                }
            }
            KeyCode::Enter | KeyCode::Right => self.open_detail(),
            KeyCode::Left if self.detail_seq.is_some() => {
                match self.detail_layout {
                    DetailLayout::Full => self.detail_layout = DetailLayout::Split,
                    DetailLayout::Split => { self.detail_seq = None; self.focus = Focus::List; }
                }
            }
            KeyCode::Char('/') => { self.search_active = true; self.filter.clear(); }
            KeyCode::Char('g') if !key.modifiers.contains(KeyModifiers::CONTROL) => {}
            KeyCode::Char('g') => self.selected = 0,
            KeyCode::Char('G') => self.selected = self.events.len().saturating_sub(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if matches!(self.focus, Focus::Detail) {
                    self.detail_scroll = self.detail_scroll.saturating_sub(15);
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if matches!(self.focus, Focus::Detail) {
                    let max = self.detail_text_lines().saturating_sub(1);
                    self.detail_scroll = (self.detail_scroll + 15).min(max);
                }
            }
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            _ => {}
        }
        Ok(Control::Continue)
    }

    fn handle_search_key(&mut self, code: KeyCode) -> Control {
        match code {
            KeyCode::Esc | KeyCode::Enter => { self.search_active = false; Control::Continue }
            KeyCode::Char(c) => { self.filter.push(c); Control::Continue }
            KeyCode::Backspace => { self.filter.pop(); Control::Continue }
            _ => Control::Continue,
        }
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

    fn move_up(&mut self) {
        if self.events.is_empty() { return; }
        match self.focus {
            Focus::List => {
                self.selected = self.selected.saturating_sub(1);
                self.detail_seq = Some(self.events[self.selected].seq);
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
        }
    }

    fn move_down(&mut self) {
        if self.events.is_empty() { return; }
        match self.focus {
            Focus::List => {
                self.selected = (self.selected + 1).min(self.events.len() - 1);
                self.detail_seq = Some(self.events[self.selected].seq);
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                let max = self.detail_text_lines().saturating_sub(1);
                self.detail_scroll = (self.detail_scroll + 1).min(max);
            }
        }
    }

    fn page_up(&mut self) {
        match self.focus {
            Focus::List => {
                self.selected = self.selected.saturating_sub(10);
                if !self.events.is_empty() { self.detail_seq = Some(self.events[self.selected].seq); }
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(10);
            }
        }
    }

    fn page_down(&mut self) {
        match self.focus {
            Focus::List => {
                self.selected = (self.selected + 10).min(self.events.len().saturating_sub(1));
                if !self.events.is_empty() { self.detail_seq = Some(self.events[self.selected].seq); }
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                let max = self.detail_text_lines().saturating_sub(1);
                self.detail_scroll = (self.detail_scroll + 10).min(max);
            }
        }
    }

    // ── Render ─────────────────────────────────────────────────

    fn render(&self, f: &mut Frame) {
        let [top, body, bot] = *Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ]).split(f.area()) else { return };

        self.render_header(f, top);
        self.render_body(f, body);
        self.render_status(f, bot);

        if self.search_active {
            let area = centered_rect(50, 3, f.area());
            let p = Paragraph::new(self.filter.as_str())
                .block(Block::default().title(" search ").borders(Borders::ALL));
            f.render_widget(p, area);
            f.set_cursor_position((area.x + 1 + self.filter.len() as u16, area.y + 1));
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let left = format!(
            " hai {} {}events ",
            self.chat_filter.map(|c| format!("Chat {c:+}")).unwrap_or_default(),
            self.events.len(),
        );
        let right = if !self.filter.is_empty() {
            format!(" filter: {} ", self.filter)
        } else {
            String::new()
        };

        let line = Line::from(vec![
            Span::styled(left, Style::new().bold()),
            Span::raw(" "),
            Span::styled(right, Style::new().fg(Color::DarkGray).italic()),
        ]);
        f.render_widget(line, area);
    }

    fn render_body(&self, f: &mut Frame, area: Rect) {
        let is_full = matches!(self.detail_layout, DetailLayout::Full);
        let has_detail = self.detail_seq.is_some() && !is_full;

        let chunks = if has_detail {
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area)
        } else {
            Layout::horizontal([Constraint::Percentage(100)]).split(area)
        };

        self.render_list(f, chunks[0]);

        if has_detail {
            self.render_detail(f, chunks[1]);
        }
    }

    fn render_list(&self, f: &mut Frame, area: Rect) {
        let focus_border = match self.focus {
            Focus::List => Color::White,
            Focus::Detail => Color::DarkGray,
        };

        let block = Block::default()
            .title(" events ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(focus_border));

        let items: Vec<ListItem> = self.events.iter().enumerate().map(|(i, event)| {
            let d = EventDisplay::from_event(event);
            let time = display::fmt_time(event.created_at);
            let chat = display::chat_display(event.chat_id);
            let line = format!("#{:>5}  {}  {}  {}", event.seq, time, chat, d.one_liner);
            let style = if i == self.selected {
                Style::new().fg(Color::Black).bg(Color::LightYellow).add_modifier(Modifier::BOLD)
            } else {
                color_for_kind(&event.kind)
            };
            ListItem::new(Line::from(Span::styled(line, style)))
        }).collect();

        let list = List::new(items).block(block).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        f.render_widget(list, area);
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        let focus_border = match self.focus {
            Focus::Detail => Color::White,
            Focus::List => Color::DarkGray,
        };

        let block = Block::default()
            .title(" detail ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(focus_border));

        let text = self.detail_event()
            .map(|e| EventDisplay::from_event(e).detail_text.clone())
            .unwrap_or_default();

        let scroll_len: usize = text.lines().count().max(1);
        let inner = block.inner(area);

        let detail = Paragraph::new(Text::from(text))
            .block(block)
            .scroll((self.detail_scroll, 0))
            .wrap(Wrap { trim: false });

        f.render_widget(detail, area);
        let pos: usize = (self.detail_scroll as usize).min(scroll_len.saturating_sub(1));
        let mut state = ScrollbarState::new(scroll_len).position(pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut state,
        );
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let label = match (self.focus, self.detail_seq, &self.detail_layout) {
            (Focus::Detail, Some(_), DetailLayout::Full) => " detail fullscreen ",
            (Focus::Detail, Some(_), DetailLayout::Split) => " detail ",
            _ => " list ",
        };

        let nav_mode = match self.focus {
            Focus::List => "↑↓ pg events",
            Focus::Detail => "↑↓ scroll  PgUp/PgDn page",
        };

        let line = Line::from(vec![
            Span::styled(format!(" {} ", label), Style::new().fg(Color::Black).bg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(nav_mode, Style::new().fg(Color::DarkGray)),
            Span::raw("  Tab focus  Enter full  / search  q quit"),
        ]);
        f.render_widget(line, area);
    }

    fn detail_event(&self) -> Option<&Event> {
        self.detail_seq.and_then(|seq| self.events.iter().find(|e| e.seq == seq))
    }

    fn detail_text_lines(&self) -> u16 {
        self.detail_event()
            .map(|e| EventDisplay::from_event(e).detail_text.lines().count() as u16)
            .unwrap_or(0)
    }
}

// ── Utils ───────────────────────────────────────────────────────

fn color_for_kind(kind: &str) -> Style {
    match kind {
        "turn_started" | "context_built" => Style::new().fg(Color::Yellow),
        "tool_call" | "tool_call_result" => Style::new().fg(Color::Cyan),
        "turn_completed" => Style::new().fg(Color::Green),
        "session_created" | "session_done" => Style::new().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
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
        .split(popup_layout[1])[1]
}

#[derive(PartialEq)]
enum Control {
    Continue,
    Quit,
}
