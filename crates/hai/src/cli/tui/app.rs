use std::time::Duration;

use crossterm::event::{
    Event as TermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use futures::StreamExt;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    macros::line,
    prelude::Stylize,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

use super::{
    detail::{Detail, DetailLayout},
    event_store::EventStore,
    events::Events,
    filter::{Filter, FilterRow},
};
use crate::domain::repo::Repos;

const POLL_MS: u64 = 200;

// ── 焦点与命令 ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Focus {
    Filter,
    Events,
    Detail,
}

/// 组件 → App 的通信命令。App 消费：同步的立即执行，异步的（reload）进 pending 队列。
pub(super) enum Cmd {
    ApplyText,
    ApplyChatFilter(Option<i64>),
    ApplyKindFilter(Option<String>),
    JumpStart,
    JumpEnd,
    ExtendBack,
    AppendNew,
    ToggleDetail,
    FollowDetail,
    ToggleFullscreen,
    Focus(Focus),
    FilterRow(FilterRow),
    ShowError(String),
}

// ── 终端守卫（RAII：任何退出路径都恢复终端） ─────────────────────

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> crate::error::Result<Self> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
        enable_raw_mode()?;
        let res = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
        if let Err(e) = res {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use std::io::Write;

        use crossterm::terminal::disable_raw_mode;
        let mut out = std::io::stdout();
        let _ = crossterm::execute!(out, crossterm::terminal::LeaveAlternateScreen);
        let _ = disable_raw_mode();
        let _ = out.flush();
    }
}

// ── App（循环 + 调度） ─────────────────────────────────────────

pub(super) struct App {
    store: EventStore,
    focus: Focus,
    filter: Filter,
    events: Events,
    detail: Detail,
    help_open: bool,
    error: Option<String>,
    pending: Vec<Cmd>,
    quit: bool,
    dirty: bool,
}

impl App {
    fn new(repos: Repos, chat_filter: Option<i64>, kind_filter: Option<String>) -> Self {
        Self {
            store: EventStore::new(repos, chat_filter, kind_filter.clone()),
            focus: Focus::Events,
            filter: Filter::new(kind_filter),
            events: Events::new(),
            detail: Detail::new(),
            help_open: false,
            error: None,
            pending: Vec::new(),
            quit: false,
            dirty: false,
        }
    }

    async fn load_initial(&mut self) -> crate::error::Result<()> {
        self.store.load_end().await?;
        self.events.rebuild(&mut self.store, &self.filter.text);
        let cmds = self.events.follow_end(false);
        self.apply_cmds(cmds);
        // detail 常驻：follow_end 已设置 selected_seq，再打开显示选中事件
        self.detail.open(self.events.selected_seq);
        Ok(())
    }

    // ── 输入 ───────────────────────────────────────────────────

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let code = key.code;

        if self.help_open {
            if matches!(
                code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?')
            ) {
                self.help_open = false;
            }
            return;
        }

        // 退出：Ctrl+C 恒退出（raw mode 无 SIGINT）；q 仅在非 text 输入上下文
        if ctrl && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        let text_typing = self.focus == Focus::Filter && self.filter.row == FilterRow::Text;
        if !text_typing && matches!(code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.quit = true;
            return;
        }

        // 全局键
        match code {
            KeyCode::Char('?') => {
                self.help_open = true;
                return;
            }
            KeyCode::Esc => {
                if self.error.is_some() {
                    self.error = None;
                } else if self.focus == Focus::Filter && self.filter.row == FilterRow::Chat {
                    self.filter.cancel();
                } else if self.detail.is_open() {
                    self.close_detail();
                }
                return;
            }
            _ => {}
        }

        let cmds = match self.focus {
            Focus::Filter => self.filter.handle_key(key),
            Focus::Events => self.events.handle_key(key),
            Focus::Detail => self.detail.handle_key(key, &mut self.store),
        };
        self.apply_cmds(cmds);
    }

    fn apply_cmds(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            match cmd {
                Cmd::ApplyText => self.events.rebuild(&mut self.store, &self.filter.text),
                Cmd::ApplyChatFilter(_)
                | Cmd::ApplyKindFilter(_)
                | Cmd::JumpStart
                | Cmd::JumpEnd
                | Cmd::ExtendBack
                | Cmd::AppendNew => self.pending.push(cmd),
                Cmd::ToggleDetail => {
                    if self.detail.is_open() {
                        self.close_detail();
                    } else if let Some(seq) = self.events.selected_seq {
                        self.detail.open(Some(seq));
                    }
                }
                Cmd::FollowDetail => {
                    if let Some(seq) = self.events.selected_seq {
                        self.detail.follow(seq);
                    }
                }
                Cmd::ToggleFullscreen => self.detail.toggle_layout(),
                Cmd::Focus(f) => self.focus = f,
                Cmd::FilterRow(r) => self.filter.row = r,
                Cmd::ShowError(msg) => self.set_error(msg),
            }
        }
    }

    fn close_detail(&mut self) {
        if self.detail.is_open() {
            self.detail.close();
            if self.focus == Focus::Detail {
                self.focus = Focus::Events;
            }
        }
    }

    fn set_error(&mut self, msg: String) {
        if self.error.is_none() {
            self.error = Some(msg);
        }
    }

    // ── 主循环 ─────────────────────────────────────────────────

    pub async fn run(&mut self) -> crate::error::Result<()> {
        use ratatui::{Terminal, backend::CrosstermBackend};

        let _guard = TerminalGuard::enter()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

        if let Err(e) = self.load_initial().await {
            self.error = Some(format!("load: {e}"));
        }

        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(POLL_MS));

        loop {
            tokio::select! {
                result = events.next() => {
                    match result {
                        Some(Ok(TermEvent::Key(key))) => self.handle_key(key),
                        Some(Ok(_)) => {}
                        Some(Err(_)) | None => break,
                    }
                    self.dirty = true;
                }
                _ = tick.tick() => {
                    self.tick().await;
                    self.dirty = true;
                }
            }
            if self.quit {
                break;
            }
            if self.dirty {
                terminal.draw(|f| self.render(f))?;
                self.dirty = false;
            }
        }
        Ok(())
    }

    async fn tick(&mut self) {
        let cmds: Vec<Cmd> = std::mem::take(&mut self.pending);
        for cmd in cmds {
            match cmd {
                Cmd::JumpStart => match self.store.load_start().await {
                    Ok(()) => {
                        self.events.rebuild(&mut self.store, &self.filter.text);
                        self.events.follow = false;
                        let cmds = self.events.select_at(0, true);
                        self.apply_cmds(cmds);
                    }
                    Err(e) => self.set_error(format!("load start: {e}")),
                },
                Cmd::JumpEnd => match self.store.load_end().await {
                    Ok(()) => {
                        self.events.rebuild(&mut self.store, &self.filter.text);
                        let cmds = self.events.follow_end(true);
                        self.apply_cmds(cmds);
                    }
                    Err(e) => self.set_error(format!("load end: {e}")),
                },
                Cmd::ExtendBack => {
                    self.events.back_loading = true;
                    let prev = self.events.selected_seq;
                    // 记录加载前选中行的视觉位置，加载后保持（无极滚动不跳变）
                    let visual = self.events.visual_pos();
                    match self.store.extend_back().await {
                        Ok(n) if n > 0 => {
                            self.events.rebuild(&mut self.store, &self.filter.text);
                            if let Some(seq) = prev
                                && self.events.contains(seq)
                            {
                                self.events.selected_seq = Some(seq);
                            }
                            self.events.anchor_after_prepend(visual);
                        }
                        Ok(_) => {}
                        Err(e) => self.set_error(format!("load back: {e}")),
                    }
                    self.events.back_loading = false;
                }
                Cmd::AppendNew => {
                    self.events.auto_loaded = true;
                    match self.store.append_new().await {
                        Ok(n) if n > 0 => {
                            self.events.rebuild(&mut self.store, &self.filter.text);
                            let cmds = self.events.follow_end(false);
                            self.apply_cmds(cmds);
                            // 剩余 gap 显示为 +N new，等用户滚动后再触发
                            match self.store.count_new().await {
                                Ok(m) => self.events.new_count = m,
                                Err(e) => self.set_error(format!("poll: {e}")),
                            }
                        }
                        Ok(_) => self.events.new_count = 0,
                        Err(e) => self.set_error(format!("append: {e}")),
                    }
                }
                Cmd::ApplyChatFilter(filter) => {
                    self.store.chat_filter = filter;
                    match self.store.load_end().await {
                        Ok(()) => {
                            self.events.rebuild(&mut self.store, &self.filter.text);
                            self.detail.close();
                            let cmds = self.events.follow_end(true);
                            self.apply_cmds(cmds);
                        }
                        Err(e) => self.set_error(format!("chat filter: {e}")),
                    }
                }
                Cmd::ApplyKindFilter(tag) => {
                    self.store.kind_filter = tag;
                    match self.store.load_end().await {
                        Ok(()) => {
                            self.events.rebuild(&mut self.store, &self.filter.text);
                            self.detail.close();
                            let cmds = self.events.follow_end(true);
                            self.apply_cmds(cmds);
                        }
                        Err(e) => self.set_error(format!("kind filter: {e}")),
                    }
                }
                _ => {}
            }
        }

        // 轮询：统计新事件数（滚动驱动——触底由 AppendNew 触发加载，不自动追平）
        match self.store.count_new().await {
            Ok(n) => self.events.new_count = n,
            Err(e) => self.set_error(format!("poll: {e}")),
        }

        // 滚到列表顶部（选中第 0 条）→ 前置加载更旧事件。
        // selected_seq.is_some()：fallback 的 selected_index()==0 是选中丢失（None），非真顶部。
        // 防重复：加载进行中（back_loading）或 pending 已排队时不重复 push。
        if self.events.selected_index() == 0
            && self.events.selected_seq.is_some()
            && !self.store.at_start
            && !self.events.back_loading
            && !self.events.is_empty()
            && !self.pending.iter().any(|c| matches!(c, Cmd::ExtendBack))
        {
            self.pending.push(Cmd::ExtendBack);
        }

        // 触底且有新事件 → 加载一批（滚动驱动；auto_loaded 防停在底部自动连续追平）
        if self.events.selected_index() + 1 >= self.events.len()
            && self.events.new_count > 0
            && !self.events.auto_loaded
            && !self.pending.iter().any(|c| matches!(c, Cmd::AppendNew))
        {
            self.pending.push(Cmd::AppendNew);
        }
    }

    // ── 渲染 ───────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        self.store.set_viewport(self.events.vp_h());

        let err_h = if self.error.is_some() { 1 } else { 0 };
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(err_h),
            Constraint::Length(1),
        ])
        .split(f.area());
        let Some([top, body, err, bot]) = chunks.first_chunk::<4>() else {
            return;
        };

        self.render_header(f, *top);
        self.render_body(f, *body);
        if err_h > 0 {
            self.render_error(f, *err);
        }
        self.render_status(f, *bot);

        if self.help_open {
            self.render_help(f, f.area());
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(
            format!(
                " hai {} {}/{} events",
                self.store
                    .chat_filter
                    .map(|c| format!("Chat {c:+}"))
                    .unwrap_or_default(),
                self.events.len(),
                self.store.window().len(),
            ),
            Style::new().bold(),
        )];
        if self.events.new_count > 0 {
            spans.push(Span::styled(
                format!("   +{} new (G to load)", self.events.new_count),
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
        f.render_widget(Line::from(spans), area);
    }

    fn render_body(&mut self, f: &mut Frame, area: Rect) {
        let is_full = self.detail.layout == DetailLayout::Full;
        let present = self.detail.is_open();

        let (lp, dp) = match (present, is_full) {
            (true, true) => (0, 100),
            (true, false) => (60, 40),
            (false, _) => (100, 0),
        };

        let chunks = Layout::horizontal([Constraint::Percentage(lp), Constraint::Percentage(dp)])
            .split(area);
        let Some([left, right]) = chunks.first_chunk::<2>() else {
            return;
        };

        if lp > 0 {
            // 左列：filter panel（3 行 + 边框）+ events panel
            let left_chunks =
                Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).split(*left);
            let Some([filter_area, events_area]) = left_chunks.first_chunk::<2>() else {
                return;
            };
            self.filter
                .render(f, *filter_area, &self.store, self.focus == Focus::Filter);
            self.events.render(
                f,
                *events_area,
                &mut self.store,
                self.focus == Focus::Events,
            );
        }
        if dp > 0 {
            self.detail
                .render(f, *right, &mut self.store, self.focus == Focus::Detail);
        }
    }

    fn render_error(&self, f: &mut Frame, area: Rect) {
        if let Some(msg) = &self.error {
            f.render_widget(
                Line::from(Span::styled(
                    format!(" error: {msg}  (Esc to clear)"),
                    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                area,
            );
        }
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let label = match (self.focus, self.detail.is_open(), &self.detail.layout) {
            (Focus::Detail, true, DetailLayout::Full) => " detail fullscreen ",
            (Focus::Detail, true, DetailLayout::Split) => " detail ",
            (Focus::Filter, _, _) => " filter ",
            _ => " events ",
        };
        let nav: &str = match self.focus {
            Focus::Filter => "j/k rows  ←→ cursor/type  Enter chat",
            Focus::Events => "j/k events  Enter detail  g/G ends",
            Focus::Detail => "j/k scroll  Enter fullscreen",
        };
        f.render_widget(
            line![
                label.black().on_cyan(),
                "  ",
                nav.dark_gray(),
                "  Tab/h/l focus  / search  c chat  ? help  q quit"
            ],
            area,
        );
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(66, 18, area);
        let text = Text::from(
            [
                "focus:  Tab/h/l  filter ⇄ events ⇄ detail",
                "filter  j/k      move row (text/chat/type)",
                "        ←/→      text/chat: cursor | type: switch",
                "        text      type to filter (one_liner+seq+chat)",
                "        chat      enter chat id, Enter apply",
                "        type      (all) TURN STEP TOOL DONE RETRY FAIL STEER WRAP",
                "events  j/k      navigate  |  Enter open/close detail",
                "        g/G      oldest / newest",
                "        PgUp/PgDn, Ctrl+U/D  page",
                "detail  j/k      scroll  |  Enter fullscreen toggle",
                "global  /         jump to text  |  c  jump to chat",
                "        Esc      cancel edit / close detail / clear error",
                "        ?         help  |  q / Ctrl+C  quit",
            ]
            .join("\n"),
        );
        f.render_widget(
            Paragraph::new(text).block(Block::default().title(" help ").borders(Borders::ALL)),
            popup,
        );
    }
}

// ── Entry point ─────────────────────────────────────────────────

pub async fn run_tui(
    repos: Repos,
    chat_filter: Option<i64>,
    kind_filter: Option<String>,
) -> crate::error::Result<()> {
    App::new(repos, chat_filter, kind_filter).run().await
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}
