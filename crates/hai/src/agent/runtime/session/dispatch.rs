//! 事件派发：dispatch + on_complete

use tokio::time::Instant;

use super::{
    super::{context::TurnContext, event::WakeEvents},
    AgentSession, SessionState,
    conversation::TurnInput,
};
use crate::{
    agent::runtime::{event::Inbox, types::TurnOutput},
    domain::{
        model::Message,
        vo::{AgentEventPayload, MessageId, TurnNumber},
    },
    error::Result,
};

// ── 生命周期 ───────────────────────────────────────────────────────────────

impl AgentSession {
    pub async fn dispatch(&mut self, events: WakeEvents, inbox: &Inbox) {
        let ctx = self.build_turn_context(events);
        let (messages, next_since_id) = match self.gather_messages().await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(%self.chat_id, "gather_messages failed: {e}");
                self.state = SessionState::Idle;
                return;
            }
        };

        // 游标暂存：turn 正常结束（Success/Steered）才提交（失败零状态副作用）
        self.conversation.stage_since_id(next_since_id);

        // 首轮判定契约见 conversation.rs:is_first_render（turn_count==0 → 完整构建）
        let is_first = self.conversation.is_first_render();
        let built = match self
            .runtime
            .build_prompt(
                &ctx,
                &messages,
                self.conversation.shown_memory_ids(),
                self.conversation.shown_topic_ids(),
                is_first,
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(%self.chat_id, "build_prompt failed: {e}");
                self.conversation.discard_since_id();
                self.conversation.discard_shown();
                self.state = SessionState::Idle;
                return;
            }
        };

        self.conversation
            .stage_shown(&built.shown_memory_ids, &built.shown_topic_ids);
        let messages = self.conversation.build_full_messages(built.messages);
        // 编号从章节内 turn_count 递增（重开归零，章节独立单元；失败不消耗）
        let turn_number = TurnNumber::from((self.conversation.turn_count() + 1) as usize);

        let reason = ctx
            .events
            .first()
            .map(|e| e.reason.label())
            .unwrap_or("unknown")
            .to_string();

        self.engine.app.event_bus.emit(
            self.chat_id,
            AgentEventPayload::TurnStarted {
                turn: turn_number,
                reason,
                msg_count: built.message_ids.len(),
                full_prompt: built.rendered_prompt.clone(),
            },
        );

        let (handle, result_rx) = self.runtime.spawn_turn(
            ctx,
            TurnInput {
                messages,
                message_ids: built.message_ids.iter().map(|id| MessageId(*id)).collect(),
            },
            inbox.clone(),
            turn_number,
        );

        self.state = SessionState::Busy {
            handle,
            result_rx,
            started_at: Instant::now(),
        };
    }

    /// Success / Steered 统一完成路径：推进上下文消息 + 章节元信息 + 游标提交 + 落盘。
    pub async fn on_complete(&mut self, output: TurnOutput, inbox: &Inbox) {
        let has_spoken = output.steps.iter().flat_map(|t| &t.tool_calls).any(|tc| {
            matches!(
                tc.tool_name.as_str(),
                "send_message" | "send_voice" | "generate_image"
            )
        });

        if has_spoken {
            self.schedule.refresh();
        }

        let last_prompt_tokens = output.steps.last().map(|t| t.prompt_tokens).unwrap_or(0);
        self.conversation
            .update(output.messages, output.steps.len(), last_prompt_tokens);
        // 游标/shown 只在正常结束（Success/Steered）提交——失败路径见 event_loop
        self.conversation.commit_since_id();
        self.conversation.commit_shown();

        let snap = self.conversation.snapshot();
        if let Err(e) = self
            .engine
            .app
            .db
            .srv
            .conversation
            .save(&snap, self.chat_id)
            .await
        {
            tracing::warn!(%self.chat_id, "Failed to persist conversation: {e}");
        }
        let events = inbox.drain();
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
    }

    pub fn build_turn_context(&self, events: WakeEvents) -> TurnContext {
        TurnContext {
            app: self.engine.app.clone(),
            chat_id: self.chat_id,
            chat_type: self.chat_type,
            handler: self.runtime.handler.clone(),
            events,
            skill_manager: self.engine.skill_manager.clone(),
            db: self.engine.app.db.srv.clone(),
            shell: self.runtime.shell.clone(),
            multimodal: self.engine.app.provider.multimodal.clone(),
        }
    }

    /// 拉取驱动 = 首轮判定（`is_first_render`）：首轮完整构建（所有未读 + 不足历史凑 seed cap）；非首轮游标增量。
    pub async fn gather_messages(&self) -> Result<(Vec<Message>, MessageId)> {
        let srv = &self.engine.app.db.srv.message;
        let cfg = &self.engine.app.cfg.agent.context;

        if self.conversation.is_first_render() {
            let (msgs, last_id) = srv
                .get_context_messages(self.chat_id, cfg.context_seed_cap)
                .await?;
            Ok((msgs, MessageId(last_id)))
        } else {
            let since_id = self.conversation.since_id();
            let msgs = srv
                .get_messages_window(self.chat_id, Some(since_id))
                .await?;
            let next_id = msgs.last().map(|m| m.id_()).unwrap_or(since_id);
            Ok((msgs, next_id))
        }
    }
}
