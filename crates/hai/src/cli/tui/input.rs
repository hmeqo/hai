use crossterm::event::KeyCode;

use super::app::TuiApp;

pub(super) const PAGE_STEP: usize = 10;
pub(super) const CTRL_STEP: usize = 15;

impl TuiApp {
    pub(super) fn handle_list_input(&mut self, code: KeyCode) {
        if self.events.is_empty() {
            return;
        }
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                self.detail_seq = Some(self.events[self.selected].seq);
                self.detail_scroll = 0;
                self.clamp_list_offset();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.events.len() - 1);
                self.detail_seq = Some(self.events[self.selected].seq);
                self.detail_scroll = 0;
                self.clamp_list_offset();
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(PAGE_STEP);
                if !self.events.is_empty() {
                    self.detail_seq = Some(self.events[self.selected].seq);
                }
                self.detail_scroll = 0;
                self.clamp_list_offset();
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + PAGE_STEP).min(self.events.len() - 1);
                if !self.events.is_empty() {
                    self.detail_seq = Some(self.events[self.selected].seq);
                }
                self.detail_scroll = 0;
                self.clamp_list_offset();
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                self.clamp_list_offset();
            }
            KeyCode::Char('G') => {
                self.selected = self.events.len().saturating_sub(1);
                self.clamp_list_offset();
            }
            _ => {}
        }
    }

    pub(super) fn handle_detail_input(&mut self, code: KeyCode) {
        if !self.detail_seq.is_some() {
            return;
        }
        let max = self.detail_text_lines().saturating_sub(1) as u16;
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = (self.detail_scroll + 1).min(max);
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(PAGE_STEP as u16);
            }
            KeyCode::PageDown => {
                self.detail_scroll = (self.detail_scroll + PAGE_STEP as u16).min(max);
            }
            _ => {}
        }
    }

    pub(super) fn handle_search_input(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc | KeyCode::Enter => {
                self.search_active = false;
                false
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                false
            }
            KeyCode::Backspace => {
                self.filter.pop();
                false
            }
            _ => false,
        }
    }
}
