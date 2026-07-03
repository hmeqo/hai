use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::{DetailLayout, Focus, app::TuiApp};

impl TuiApp {
    pub(super) fn render_header(&self, f: &mut Frame, area: Rect) {
        let left = format!(
            " hai {} {}events ",
            self.chat_filter
                .map(|c| format!("Chat {c:+}"))
                .unwrap_or_default(),
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

    pub(super) fn render_status(&self, f: &mut Frame, area: Rect) {
        let label = match (self.focus, self.detail_seq, &self.detail_layout) {
            (Focus::Detail, Some(_), DetailLayout::Full) => " detail fullscreen ",
            (Focus::Detail, Some(_), DetailLayout::Split) => " detail ",
            _ => " list ",
        };
        let nav = match self.focus {
            Focus::List => "↑↓/PgUp/PgDn events",
            Focus::Detail => "↑↓/PgUp/PgDn scroll",
        };

        let line = Line::from(vec![
            Span::styled(label, Style::new().fg(Color::Black).bg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(nav, Style::new().fg(Color::DarkGray)),
            Span::raw("  Tab focus  Enter full  / search  q quit"),
        ]);
        f.render_widget(line, area);
    }

    pub(super) fn render_search(&self, f: &mut Frame, area: Rect) {
        let popup = centered_rect(50, 3, area);
        let p = Paragraph::new(self.filter.as_str())
            .block(Block::default().title(" search ").borders(Borders::ALL));
        f.render_widget(p, popup);
        f.set_cursor_position((popup.x + 1 + self.filter.len() as u16, popup.y + 1));
    }
}

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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
