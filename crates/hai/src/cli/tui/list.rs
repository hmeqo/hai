use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use super::{Focus, app::TuiApp};
use crate::cli::display::{self, EventDisplay};

pub(super) const PADDING: usize = 3;

impl TuiApp {
    pub(super) fn render_list(&mut self, f: &mut Frame, area: Rect) {
        let focus = match self.focus {
            Focus::List => Color::White,
            Focus::Detail => Color::DarkGray,
        };

        let block = Block::default()
            .title(" events ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(focus));

        self.list_vp_h = block.inner(area).height as usize;
        let max_off = self.events.len().saturating_sub(self.list_vp_h);
        self.list_offset = self.list_offset.min(max_off);

        let items: Vec<ListItem> = self
            .events
            .iter()
            .map(|e| {
                let d = EventDisplay::from_event(e);
                let t = display::fmt_time(e.created_at);
                let c = display::chat_display(e);
                let (cr, cg, cb) = display::color_rgb(e);
                ListItem::new(Line::from(Span::styled(
                    format!("#{:>5}  {}  {}  {}", e.seq, t, c, d.one_liner),
                    Style::new().fg(Color::Rgb(cr, cg, cb)),
                )))
            })
            .collect();

        let mut state = ListState::default()
            .with_offset(self.list_offset)
            .with_selected(Some(self.selected));

        let list = List::new(items).block(block).highlight_style(
            Style::new()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        );

        f.render_stateful_widget(list, area, &mut state);
    }

    pub(super) fn clamp_list_offset(&mut self) {
        if self.events.is_empty() || self.list_vp_h == 0 {
            return;
        }
        let max_off = self.events.len().saturating_sub(self.list_vp_h);

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
    }
}
