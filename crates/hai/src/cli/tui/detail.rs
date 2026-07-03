use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use super::{Focus, app::TuiApp};
use crate::{cli::display::EventDisplay, domain::model::Event};

impl TuiApp {
    pub(super) fn render_detail(&mut self, f: &mut Frame, area: Rect) {
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
            .map(|e| EventDisplay::from_event(e).detail_text.clone())
            .unwrap_or_default();
        let scroll_len: usize = text.lines().count().max(1);
        let inner = block.inner(area);

        let detail = Paragraph::new(Text::from(text))
            .block(block)
            .scroll((self.detail_scroll, 0))
            .wrap(Wrap { trim: false });

        f.render_widget(detail, area);

        let pos = (self.detail_scroll as usize).min(scroll_len.saturating_sub(1));
        let mut state = ScrollbarState::new(scroll_len).position(pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut state,
        );
    }

    pub(super) fn detail_event(&self) -> Option<&Event> {
        self.detail_seq
            .and_then(|seq| self.events.iter().find(|e| e.seq == seq))
    }

    pub(super) fn detail_text_lines(&self) -> u16 {
        self.detail_event()
            .map(|e| EventDisplay::from_event(e).detail_text.lines().count() as u16)
            .unwrap_or(0)
    }
}
