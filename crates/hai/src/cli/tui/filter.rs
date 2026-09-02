use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

use super::{
    app::{Cmd, Focus},
    event_store::EventStore,
};
use crate::cli::display::KIND_TAGS;

/// filter panel 内的行焦点。
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FilterRow {
    Text,
    Chat,
    Type,
}

/// 左上 panel：text/chat/type 三行过滤表单。
/// 输入状态内聚于此；过滤效果通过 Cmd 通知 App（reload / 列表重建）。
pub(super) struct Filter {
    pub text: String,
    text_cursor: usize,
    pub chat_input: String,
    chat_cursor: usize,
    pub kind: Option<String>,
    pub row: FilterRow,
}

impl Filter {
    pub fn new(kind: Option<String>) -> Self {
        Self {
            text: String::new(),
            text_cursor: 0,
            chat_input: String::new(),
            chat_cursor: 0,
            kind,
            row: FilterRow::Text,
        }
    }

    /// 取消当前编辑（chat 行清输入回 text 行）。
    pub fn cancel(&mut self) {
        if self.row == FilterRow::Chat {
            self.chat_input.clear();
            self.chat_cursor = 0;
            self.row = FilterRow::Text;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if key.kind != KeyEventKind::Press {
            return cmds;
        }
        let code = key.code;
        match self.row {
            FilterRow::Text => match code {
                KeyCode::Left => Self::move_cursor(&self.text, &mut self.text_cursor, -1),
                KeyCode::Right => Self::move_cursor(&self.text, &mut self.text_cursor, 1),
                KeyCode::Backspace => {
                    Self::backspace(&mut self.text, &mut self.text_cursor);
                    cmds.push(Cmd::ApplyText);
                }
                KeyCode::Char(c) => {
                    self.text.insert(self.text_cursor, c);
                    self.text_cursor += c.len_utf8();
                    cmds.push(Cmd::ApplyText);
                }
                KeyCode::Down => self.row = FilterRow::Chat,
                KeyCode::Tab | KeyCode::BackTab => cmds.push(Cmd::Focus(Focus::Events)),
                _ => {}
            },
            FilterRow::Chat => match code {
                KeyCode::Left => Self::move_cursor(&self.chat_input, &mut self.chat_cursor, -1),
                KeyCode::Right => Self::move_cursor(&self.chat_input, &mut self.chat_cursor, 1),
                KeyCode::Backspace => Self::backspace(&mut self.chat_input, &mut self.chat_cursor),
                KeyCode::Char(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                    self.chat_input.insert(self.chat_cursor, c);
                    self.chat_cursor += 1;
                }
                KeyCode::Enter => {
                    match self.parse_chat() {
                        Ok(v) => cmds.push(Cmd::ApplyChatFilter(v)),
                        Err(msg) => cmds.push(Cmd::ShowError(msg)),
                    }
                    self.chat_input.clear();
                    self.chat_cursor = 0;
                    self.row = FilterRow::Text;
                }
                KeyCode::Up | KeyCode::Char('k') => self.row = FilterRow::Text,
                KeyCode::Down | KeyCode::Char('j') => self.row = FilterRow::Type,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('l') => {
                    cmds.push(Cmd::Focus(Focus::Events))
                }
                _ => {}
            },
            FilterRow::Type => match code {
                KeyCode::Left => cmds.push(self.cycle_kind(-1)),
                KeyCode::Right => cmds.push(self.cycle_kind(1)),
                KeyCode::Up | KeyCode::Char('k') => self.row = FilterRow::Chat,
                KeyCode::Down | KeyCode::Char('j') => self.row = FilterRow::Text,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('l') => {
                    cmds.push(Cmd::Focus(Focus::Events))
                }
                _ => {}
            },
        }
        cmds
    }

    /// type 行切换（0..=8：None + 8 类型，循环），立即更新自身并产出 Cmd。
    fn cycle_kind(&mut self, delta: isize) -> Cmd {
        let cur = match &self.kind {
            None => 0,
            Some(tag) => KIND_TAGS
                .iter()
                .position(|(_, t)| t == tag)
                .map(|i| i + 1)
                .unwrap_or(0),
        };
        let next = (cur as isize + delta).rem_euclid(KIND_TAGS.len() as isize + 1) as usize;
        let pick = if next == 0 {
            None
        } else {
            KIND_TAGS.get(next - 1).map(|(_, tag)| (*tag).to_string())
        };
        self.kind = pick.clone();
        Cmd::ApplyKindFilter(pick)
    }

    fn parse_chat(&self) -> Result<Option<i64>, String> {
        let raw = self.chat_input.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        match raw.parse::<i64>() {
            Ok(n) if n != 0 => Ok(Some(n)),
            _ => Err(format!("invalid chat id: {raw}")),
        }
    }

    /// 在字符边界移动光标（byte index）。
    fn move_cursor(s: &str, cur: &mut usize, delta: isize) {
        let len = s.len();
        let c = (*cur).min(len);
        if delta < 0 {
            let mut i = c;
            while i > 0 {
                i -= 1;
                if s.is_char_boundary(i) {
                    break;
                }
            }
            *cur = i;
        } else if delta > 0 {
            let mut i = c;
            while i < len {
                i += 1;
                if s.is_char_boundary(i) {
                    break;
                }
            }
            *cur = i.min(len);
        }
    }

    fn backspace(s: &mut String, cur: &mut usize) {
        let c = (*cur).min(s.len());
        let mut i = c;
        while i > 0 {
            i -= 1;
            if s.is_char_boundary(i) {
                break;
            }
        }
        if i < c {
            s.drain(i..c);
            *cur = i;
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, store: &EventStore, active: bool) {
        let border = if active {
            Color::White
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .title(" filter ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border));
        let inner = block.inner(area);

        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let Some([text_row, chat_row, type_row]) = rows.first_chunk::<3>() else {
            return;
        };

        let text_focus = active && self.row == FilterRow::Text;
        let chat_focus = active && self.row == FilterRow::Chat;
        let type_focus = active && self.row == FilterRow::Type;

        // text 行
        let text_style = if text_focus {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        f.render_widget(
            Line::from(vec![
                Span::styled("text:", text_style),
                Span::raw(" "),
                Span::styled(self.text.as_str(), Style::new().fg(Color::White)),
            ]),
            *text_row,
        );
        if text_focus {
            let x = text_row.x
                + 6
                + unicode_width::UnicodeWidthStr::width(&self.text[..self.text_cursor]) as u16;
            f.set_cursor_position((x, text_row.y));
        }

        // chat 行
        let chat_style = if chat_focus {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        let chat_disp: String = if chat_focus {
            self.chat_input.clone()
        } else {
            match store.chat_filter {
                Some(c) => format!("{c:+}"),
                None => String::from("(all)"),
            }
        };
        f.render_widget(
            Line::from(vec![
                Span::styled("chat:", chat_style),
                Span::raw(" "),
                Span::styled(chat_disp, Style::new().fg(Color::White)),
            ]),
            *chat_row,
        );
        if chat_focus {
            let x = chat_row.x
                + 6
                + unicode_width::UnicodeWidthStr::width(&self.chat_input[..self.chat_cursor])
                    as u16;
            f.set_cursor_position((x, chat_row.y));
        }

        // type 行
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::styled(
            "type:",
            if type_focus {
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::DarkGray)
            },
        ));
        let active_style = || {
            Style::new()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "(all)",
            if self.kind.is_none() {
                active_style()
            } else {
                Style::new().fg(Color::DarkGray)
            },
        ));
        for (name, tag) in KIND_TAGS {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                *name,
                if self.kind.as_deref() == Some(*tag) {
                    active_style()
                } else {
                    Style::new().fg(Color::DarkGray)
                },
            ));
        }
        f.render_widget(Line::from(spans), *type_row);

        f.render_widget(block, area);
    }
}
