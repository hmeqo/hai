use std::collections::HashSet;

use uuid::Uuid;

use crate::domain::vo::ContextMeta;

/// 当前章节：状态聚合（独立封装）。
///
/// - `meta`（ContextMeta）非空 = 章节非空（idle 到期重开判据）
/// - `shown_*` = 已展示记忆/话题 id（防重复注入；持久化，随章节生命周期）
pub(super) struct Chapter {
    meta: ContextMeta,
    shown_memory_ids: HashSet<Uuid>,
    shown_topic_ids: HashSet<Uuid>,
}

impl Chapter {
    pub fn new() -> Self {
        Self {
            meta: ContextMeta::new(),
            shown_memory_ids: HashSet::new(),
            shown_topic_ids: HashSet::new(),
        }
    }

    pub fn from_snapshot(
        meta: ContextMeta,
        shown_memory_ids: HashSet<Uuid>,
        shown_topic_ids: HashSet<Uuid>,
    ) -> Self {
        Self {
            meta,
            shown_memory_ids,
            shown_topic_ids,
        }
    }

    pub fn meta(&self) -> ContextMeta {
        self.meta
    }

    /// 章节非空判定（idle 到期需要重开的判据）。
    pub fn is_non_empty(&self) -> bool {
        self.meta.is_non_empty()
    }

    /// turn 正常结束（Success/Steered）后推进章节元信息。
    pub fn advance(&mut self, step_count: usize, last_prompt_tokens: u32) {
        self.meta.advance(step_count, last_prompt_tokens);
    }

    pub fn record_shown(&mut self, memory_ids: &[Uuid], topic_ids: &[Uuid]) {
        self.shown_memory_ids.extend(memory_ids.iter().copied());
        self.shown_topic_ids.extend(topic_ids.iter().copied());
    }

    pub fn shown_memory_ids(&self) -> &HashSet<Uuid> {
        &self.shown_memory_ids
    }

    pub fn shown_topic_ids(&self) -> &HashSet<Uuid> {
        &self.shown_topic_ids
    }
}
