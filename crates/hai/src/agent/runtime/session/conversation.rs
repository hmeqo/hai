use super::{super::types::Messages, chapter::Chapter};
use crate::domain::vo::{ConversationSnapshot, MessageId};

/// 对话状态（持久化，快照是完整代表；执行环境可重建、状态不消失）。
/// 职责边界见 docs/topics/session.md「Conversation 快照层边界」（对话级 vs 章节级）。
pub(crate) struct Conversation {
    /// 给 LLM 的累积消息序列（持久化，恢复无缝续接；
    /// 收尾摘要置开头/全新为空；首轮判定 = 章节初始状态 turn_count==0，见 `is_first_render`）。
    context_messages: Messages,
    chapter: Chapter,
    since_id: MessageId,
    /// 游标暂存：dispatch 拉取后暂存，turn 正常结束（Success/Steered）才提交；
    /// 失败丢弃——游标只在成功时推进。
    pending_since_id: Option<MessageId>,
    /// 本次 turn 检索注入的记忆/话题暂存：成功才记入章节 shown（失败不污染——
    /// 重试时重新检索注入（失败零状态副作用）。
    pending_shown: Option<(Vec<uuid::Uuid>, Vec<uuid::Uuid>)>,
    /// 恢复时的最后活动时间（= conversation.updated_at 近似；仅恢复会话 Some，
    /// 首次 idle_tick 消费——超期立即重开，见 event_loop.rs）。
    restored_last_active: Option<jiff::Timestamp>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            context_messages: Messages::new(Vec::new()),
            chapter: Chapter::new(),
            since_id: MessageId(0),
            pending_since_id: None,
            pending_shown: None,
            restored_last_active: None,
        }
    }

    pub fn from_snapshot(
        snap: ConversationSnapshot,
        restored_last_active: Option<jiff::Timestamp>,
    ) -> Self {
        Self {
            context_messages: Messages::new(snap.context_messages),
            chapter: Chapter::from_snapshot(
                snap.context_meta,
                snap.shown_memory_ids,
                snap.shown_topic_ids,
            ),
            since_id: snap.since_id,
            pending_since_id: None,
            pending_shown: None,
            restored_last_active,
        }
    }

    pub fn snapshot(&self) -> ConversationSnapshot {
        ConversationSnapshot {
            context_messages: self.context_messages.to_vec(),
            since_id: self.since_id,
            shown_memory_ids: self.chapter.shown_memory_ids().clone(),
            shown_topic_ids: self.chapter.shown_topic_ids().clone(),
            context_meta: self.chapter.meta(),
        }
    }

    /// turn 正常结束（Success/Steered）后写回：上下文消息 + 章节元信息推进。
    /// `last_prompt_tokens` = 最后一次 LLM 调用的输入占用（tokens 判据来源）。
    pub fn update(&mut self, messages: Messages, step_count: usize, last_prompt_tokens: u32) {
        self.context_messages = messages;
        self.chapter.advance(step_count, last_prompt_tokens);
    }

    /// 暂存本次 turn 检索注入的记忆/话题（dispatch 调用；成功才提交）。
    pub fn stage_shown(&mut self, memory_ids: &[uuid::Uuid], topic_ids: &[uuid::Uuid]) {
        self.pending_shown = Some((memory_ids.to_vec(), topic_ids.to_vec()));
    }

    /// 提交暂存 shown（Success/Steered 路径）。
    pub fn commit_shown(&mut self) {
        if let Some((memory_ids, topic_ids)) = self.pending_shown.take() {
            self.chapter.record_shown(&memory_ids, &topic_ids);
        }
    }

    /// 丢弃暂存 shown（Failed 路径——零状态副作用，重试重新检索注入）。
    pub fn discard_shown(&mut self) {
        self.pending_shown = None;
    }

    pub fn build_full_messages(&self, chunk: Vec<genai::chat::ChatMessage>) -> Messages {
        let mut full = self.context_messages.clone();
        full.extend(chunk);
        full
    }

    pub fn since_id(&self) -> MessageId {
        self.since_id
    }

    /// 首轮判定契约见 docs/topics/session.md「首轮判定 = 章节初始状态（turn_count==0）」。
    pub fn is_first_render(&self) -> bool {
        self.turn_count() == 0
    }

    /// 游标暂存（dispatch 拉取后调用；turn 成功才提交）。
    pub fn stage_since_id(&mut self, id: MessageId) {
        self.pending_since_id = Some(id);
    }

    /// 提交暂存游标（Success/Steered 路径）。
    pub fn commit_since_id(&mut self) {
        if let Some(id) = self.pending_since_id.take() {
            self.since_id = id;
        }
    }

    /// 丢弃暂存游标（Failed 路径——零状态副作用）。
    pub fn discard_since_id(&mut self) {
        self.pending_since_id = None;
    }

    pub fn messages_for_wrap_up(&self) -> Messages {
        self.context_messages.clone()
    }

    pub fn message_count(&self) -> usize {
        self.context_messages.len()
    }

    pub fn context_tokens(&self) -> u32 {
        self.chapter.meta().tokens
    }

    pub fn step_count(&self) -> u64 {
        self.chapter.meta().step_count
    }

    pub fn turn_count(&self) -> u64 {
        self.chapter.meta().turn_count
    }

    /// 存在未收尾（未归档）的当前章节内容（idle 到期需要重开的判据）。
    pub fn has_unwrapped_content(&self) -> bool {
        self.chapter.is_non_empty()
    }

    /// 取恢复时的最后活动时间并清除（idle_tick 一次性消费：超期立即重开）。
    pub fn take_restored_last_active(&mut self) -> Option<jiff::Timestamp> {
        self.restored_last_active.take()
    }

    pub fn shown_memory_ids(&self) -> &std::collections::HashSet<uuid::Uuid> {
        self.chapter.shown_memory_ids()
    }

    pub fn shown_topic_ids(&self) -> &std::collections::HashSet<uuid::Uuid> {
        self.chapter.shown_topic_ids()
    }

    /// 重开章节：新章节整体替换（`Chapter::new()`）；`summary`（收尾留存摘要）置入新章节开头。
    pub fn start_new_chapter(&mut self, summary: Option<String>) {
        self.context_messages = match summary {
            Some(s) => Messages::new(vec![genai::chat::ChatMessage::user(s)]),
            None => Messages::new(Vec::new()),
        };
        self.chapter = Chapter::new();
        self.pending_since_id = None;
        self.pending_shown = None;
    }
}

#[derive(Clone)]
pub(crate) struct TurnInput {
    pub messages: Messages,
    pub message_ids: Vec<MessageId>,
}
