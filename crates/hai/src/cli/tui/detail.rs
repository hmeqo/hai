use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use super::{app::Cmd, event_store::EventStore};

const PAGE_STEP: usize = 10;
const CTRL_STEP: usize = 15;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum DetailLayout {
    Split,
    Full,
}

/// 右 panel：事件详情。
pub(super) struct Detail {
    pub seq: Option<i64>,
    scroll: u16,
    width: usize,
    pub layout: DetailLayout,
}

impl Detail {
    pub fn new() -> Self {
        Self {
            seq: None,
            scroll: 0,
            width: 1,
            layout: DetailLayout::Split,
        }
    }

    pub fn is_open(&self) -> bool {
        self.seq.is_some()
    }

    /// 打开/切换指定事件（Enter 在列表选中时）。
    pub fn open(&mut self, seq: Option<i64>) {
        self.seq = seq;
        self.scroll = 0;
    }

    /// 选中变化时跟随（仅当 detail 已打开时更新，并重置滚动位置）。
    pub fn follow(&mut self, seq: i64) {
        if self.seq.is_some() {
            self.seq = Some(seq);
            self.scroll = 0;
        }
    }

    pub fn close(&mut self) {
        self.seq = None;
        self.scroll = 0;
    }

    pub fn toggle_layout(&mut self) {
        self.layout = match self.layout {
            DetailLayout::Split => DetailLayout::Full,
            DetailLayout::Full => DetailLayout::Split,
        };
    }

    pub fn handle_key(&mut self, key: KeyEvent, store: &mut EventStore) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if key.kind != KeyEventKind::Press {
            return cmds;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let code = key.code;
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1, store),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1, store),
            KeyCode::PageUp => self.scroll_detail(-(PAGE_STEP as i16), store),
            KeyCode::PageDown => self.scroll_detail(PAGE_STEP as i16, store),
            KeyCode::Char('u') if ctrl => self.scroll_detail(-(CTRL_STEP as i16), store),
            KeyCode::Char('d') if ctrl => self.scroll_detail(CTRL_STEP as i16, store),
            KeyCode::Enter | KeyCode::Right => cmds.push(Cmd::ToggleFullscreen),
            // Tab/l 循环到最左（Filter）；h 左移一格回 Events
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('l') => {
                cmds.push(Cmd::Focus(super::app::Focus::Filter))
            }
            KeyCode::Char('h') => cmds.push(Cmd::Focus(super::app::Focus::Events)),
            _ => {}
        }
        cmds
    }

    fn scroll_detail(&mut self, delta: i16, store: &mut EventStore) {
        let max = self.text_lines(store).saturating_sub(1);
        let new = (self.scroll as i16 + delta).max(0) as u16;
        self.scroll = new.min(max);
    }

    /// wrap 后的可视行数（长行换行后行数增加，滚动上限按此计算）。
    fn text_lines(&mut self, store: &mut EventStore) -> u16 {
        let Some(d) = self.seq.and_then(|seq| store.display_by_seq(seq)) else {
            return 0;
        };
        let w = self.width.max(1);
        d.detail_text
            .lines()
            .map(|l| unicode_width::UnicodeWidthStr::width(l).div_ceil(w))
            .sum::<usize>() as u16
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, store: &mut EventStore, active: bool) {
        let border = if active {
            Color::White
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .title(" detail ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border));

        let text = self
            .seq
            .and_then(|seq| store.display_by_seq(seq))
            .map(|d| d.detail_text)
            .unwrap_or_default();
        let inner = block.inner(area);
        self.width = inner.width as usize;
        let scroll_len: usize = self.text_lines(store) as usize + 1;

        f.render_widget(
            Paragraph::new(Text::from(text))
                .block(block)
                .scroll((self.scroll, 0))
                .wrap(Wrap { trim: false }),
            area,
        );

        let pos = (self.scroll as usize).min(scroll_len.saturating_sub(1));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut ScrollbarState::new(scroll_len).position(pos),
        );
    }
}
