use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use super::{
    app::{Cmd, Focus},
    event_store::EventStore,
};
use crate::cli::display;

/// 左下 panel：事件列表。
/// 可见性自包含（`visible_seq` 为可见事件 seq 列表 + `selected_seq` 锚定），
/// 不持有窗口；数据经 App 传入的 `EventStore` 读取。
pub(super) struct Events {
    visible_seq: Vec<i64>,
    pub selected_seq: Option<i64>,
    offset: usize,
    vp_h: usize,
    pub follow: bool,
    pub back_loading: bool,
    pub new_count: usize,
    /// 触底加载一批后置位：用户滚动（select_at）清除——防止停在底部时自动连续追平
    pub auto_loaded: bool,
}

impl Events {
    pub fn new() -> Self {
        Self {
            visible_seq: Vec::new(),
            selected_seq: None,
            offset: 0,
            vp_h: 20,
            follow: false,
            back_loading: false,
            new_count: 0,
            auto_loaded: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.visible_seq.is_empty()
    }

    pub fn len(&self) -> usize {
        self.visible_seq.len()
    }

    pub fn vp_h(&self) -> usize {
        self.vp_h
    }

    /// 按 text 过滤重建可见集（窗口变化/过滤变化后调用）。
    pub fn rebuild(&mut self, store: &mut EventStore, text: &str) {
        let needle = text.to_lowercase();
        let window_seqs: Vec<i64> = store.window().iter().map(|e| e.seq).collect();
        let mut seqs = Vec::with_capacity(window_seqs.len());
        for seq in window_seqs {
            let keep = if needle.is_empty() {
                true
            } else {
                store.display_by_seq(seq).is_some_and(|d| {
                    d.one_liner.to_lowercase().contains(&needle)
                        || seq.to_string().contains(&needle)
                        || d.chat_id.to_string().contains(&needle)
                })
            };
            if keep {
                seqs.push(seq);
            }
        }
        self.visible_seq = seqs;
        if let Some(seq) = self.selected_seq
            && !self.visible_seq.contains(&seq)
        {
            self.selected_seq = None;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if key.kind != KeyEventKind::Press {
            return cmds;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let code = key.code;
        match code {
            KeyCode::Up | KeyCode::Char('k') => cmds.extend(self.select_rel(-1, true)),
            KeyCode::Down | KeyCode::Char('j') => cmds.extend(self.select_rel(1, true)),
            // 页 = 视口/2，半页 = 视口/4（无极滚动）
            KeyCode::PageUp => {
                cmds.extend(self.select_rel(-((self.vp_h / 2).max(1) as isize), true))
            }
            KeyCode::PageDown => {
                cmds.extend(self.select_rel((self.vp_h / 2).max(1) as isize, true))
            }
            KeyCode::Char('u') if ctrl => {
                cmds.extend(self.select_rel(-((self.vp_h / 4).max(1) as isize), true))
            }
            KeyCode::Char('d') if ctrl => {
                cmds.extend(self.select_rel((self.vp_h / 4).max(1) as isize, true))
            }
            KeyCode::Char('g') => cmds.push(Cmd::JumpStart),
            KeyCode::Char('G') => cmds.push(Cmd::JumpEnd),
            KeyCode::Enter | KeyCode::Right => cmds.push(Cmd::ToggleDetail),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('l') => {
                cmds.push(Cmd::Focus(Focus::Detail))
            }
            KeyCode::Char('h') => cmds.push(Cmd::Focus(Focus::Filter)),
            KeyCode::Char('/') => {
                cmds.push(Cmd::Focus(Focus::Filter));
                cmds.push(Cmd::FilterRow(super::filter::FilterRow::Text));
            }
            KeyCode::Char('c') => {
                cmds.push(Cmd::Focus(Focus::Filter));
                cmds.push(Cmd::FilterRow(super::filter::FilterRow::Chat));
            }
            _ => {}
        }
        cmds
    }

    /// 相对移动选中。`update_detail`：用户主动导航为 true（detail 跟随）。
    fn select_rel(&mut self, delta: isize, update_detail: bool) -> Vec<Cmd> {
        let cur = self.selected_index();
        let new = (cur as isize + delta).max(0) as usize;
        self.select_at(new, update_detail)
    }

    /// 选中指定索引。返回 detail 跟随的 Cmd（App 消费）。
    pub fn select_at(&mut self, index: usize, update_detail: bool) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if self.visible_seq.is_empty() {
            self.selected_seq = None;
            return cmds;
        }
        let index = index.clamp(0, self.visible_seq.len() - 1);
        let seq = self.visible_seq[index];
        self.selected_seq = Some(seq);
        self.follow = index == self.visible_seq.len() - 1;
        self.auto_loaded = false;
        if update_detail {
            cmds.push(Cmd::FollowDetail);
        }
        self.clamp_offset();
        cmds
    }

    /// 跳到最新可见事件。`update_detail`：用户主动跳转 true；自动事件（新事件到达）false。
    pub fn follow_end(&mut self, update_detail: bool) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if self.visible_seq.is_empty() {
            return cmds;
        }
        let last = self.visible_seq.len() - 1;
        let seq = self.visible_seq[last];
        self.selected_seq = Some(seq);
        self.follow = true;
        self.new_count = 0;
        if update_detail {
            cmds.push(Cmd::FollowDetail);
        }
        self.clamp_offset();
        cmds
    }

    pub fn selected_index(&self) -> usize {
        let fallback = if self.follow {
            self.visible_seq.len().saturating_sub(1)
        } else {
            0
        };
        match self.selected_seq {
            Some(seq) => self
                .visible_seq
                .iter()
                .position(|&s| s == seq)
                .unwrap_or(fallback),
            None => fallback,
        }
    }

    pub fn contains(&self, seq: i64) -> bool {
        self.visible_seq.contains(&seq)
    }

    pub fn clamp_offset(&mut self) {
        if self.visible_seq.is_empty() || self.vp_h == 0 {
            return;
        }
        let max_off = self.visible_seq.len().saturating_sub(self.vp_h);
        // 无极滚动：选中行保持在视口上部 1/3（scrolloff），视图随滚动连续移动
        let scrolloff = (self.vp_h / 3).max(1);
        let sel = self.selected_index();
        self.offset = sel.saturating_sub(scrolloff).min(max_off);
    }

    /// 前置加载后保持选中行的视觉位置（无极滚动锚定，加载不跳变）。
    /// `visual` = 加载前选中行相对视口顶部的行数。
    pub fn anchor_after_prepend(&mut self, visual: usize) {
        if self.visible_seq.is_empty() || self.vp_h == 0 {
            return;
        }
        let max_off = self.visible_seq.len().saturating_sub(self.vp_h);
        let sel = self.selected_index();
        self.offset = sel.saturating_sub(visual).min(max_off);
    }

    /// 选中行相对视口顶部的行数（无极滚动锚定用）。
    pub fn visual_pos(&self) -> usize {
        self.selected_index().saturating_sub(self.offset)
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, store: &mut EventStore, active: bool) {
        let border = if active {
            Color::White
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .title(" events ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border));

        self.vp_h = block.inner(area).height as usize;
        let max_off = self.visible_seq.len().saturating_sub(self.vp_h);
        self.offset = self.offset.min(max_off);

        let meta: Vec<(i64, String)> = self
            .visible_seq
            .iter()
            .filter_map(|&seq| {
                let e = store.window().iter().find(|e| e.seq == seq)?;
                Some((seq, display::fmt_time(e.created_at)))
            })
            .collect();

        let mut items = Vec::with_capacity(meta.len());
        for (seq, time) in meta {
            let Some(d) = store.display_by_seq(seq) else {
                continue;
            };
            let (cr, cg, cb) = d.color;
            items.push(ListItem::new(Line::from(Span::styled(
                format!("#{seq:>5}  {time}  {:+}  {}", d.chat_id, d.one_liner),
                Style::new().fg(Color::Rgb(cr, cg, cb)),
            ))));
        }

        let mut state = ListState::default()
            .with_offset(self.offset)
            .with_selected(Some(self.selected_index()));

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
}
