use serde::{Deserialize, Serialize};

use super::SessionId;

/// Chapter-level scalars persisted alongside the conversation snapshot; reopening a chapter
/// (`start_new_chapter`) resets the counters and mints a new session id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextMeta {
    /// Context occupancy: prompt tokens of the last successful Turn. The overflow trigger reads
    /// it.
    pub tokens: u32,
    /// Successful Turns in the chapter; `Success` and `Steered` advance it, `Failed` does not.
    pub turn_count: u64,
    /// Steps accumulated in the chapter; reported as `WrapUpCompleted.step_count`.
    pub step_count: u64,
    /// The session this chapter's events belong to; the trace's top unit for the run.
    pub session: SessionId,
}

impl Default for ContextMeta {
    fn default() -> Self {
        Self {
            tokens: 0,
            turn_count: 0,
            step_count: 0,
            session: SessionId::new(),
        }
    }
}

impl ContextMeta {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn advance(&mut self, step_count: usize, last_prompt_tokens: u32) {
        self.tokens = last_prompt_tokens;
        self.turn_count += 1;
        self.step_count += step_count as u64;
    }

    pub fn is_non_empty(&self) -> bool {
        self.turn_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chapter's session survives its turns and changes only when the chapter does.
    #[test]
    fn a_chapter_keeps_its_session() {
        let mut meta = ContextMeta::new();
        let session = meta.session;
        assert_ne!(meta.session, ContextMeta::new().session);

        meta.advance(4, 1_200);
        assert_eq!(meta.session, session);

        meta = ContextMeta::new();
        assert_ne!(meta.session, session);
    }

    /// A snapshot missing a field is unreadable, not a chapter with an invented session.
    #[test]
    fn a_snapshot_without_a_session_is_an_error() {
        let without_session = serde_json::json!({ "tokens": 0, "turn_count": 0, "step_count": 0 });
        assert!(serde_json::from_value::<ContextMeta>(without_session).is_err());
    }
}
