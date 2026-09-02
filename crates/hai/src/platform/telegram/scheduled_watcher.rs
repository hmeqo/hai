use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    agent::{
        link::PlatformHandler,
        runtime::{
            AgentEngine,
            event::{TaskPayload, WakeEvent, WakeReason},
            registry::SessionManager,
        },
    },
    domain::vo::ChatId,
    error::Result,
};

/// 计划任务到期 watcher：per-bot 常驻循环。
///
/// 轮询 DB 到期任务 → 唤醒对应 chat 的 session（按需创建）。
/// 复用现有事件日志留痕；周期任务推进 fire_at，一次性任务停用。
pub struct ScheduledTaskWatcher;

impl ScheduledTaskWatcher {
    /// 启动 watcher：持有本 bot 的 registry 与 handler（handler 提供 bot_id 路由）。
    pub fn spawn(
        registry: SessionManager,
        engine: AgentEngine,
        handler: Arc<dyn PlatformHandler>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let bot_id = handler.bot_id().to_string();
            tracing::info!(bot = %bot_id, "scheduled task watcher started");
            if let Err(e) = run(&registry, &engine, &bot_id).await {
                tracing::error!(bot = %bot_id, "scheduled task watcher stopped: {e}");
            }
        })
    }
}

async fn run(registry: &SessionManager, engine: &AgentEngine, bot_id: &str) -> Result<()> {
    loop {
        let now = jiff::Timestamp::now();
        let due = engine.app.db.srv.scheduled_task.due(bot_id, now).await?;

        for task in due {
            let chat_id = ChatId(task.chat_id);
            // 唤醒 session（按需创建），到点任务是寻址事件，抵达即触发 turn
            if let Ok(handle) = registry.get_or_create(chat_id).await {
                handle.wake(WakeEvent::new(WakeReason::Scheduled(
                    TaskPayload::new(task.description.clone()).with_id(task.id),
                )));
            } else {
                tracing::warn!(bot = %bot_id, chat = %task.chat_id, "scheduled task wake failed");
            }
            // 推进：周期 → 下一触发点；一次性 → 停用
            engine.app.db.srv.scheduled_task.advance(&task, now).await?;
        }

        // 休眠到最早到期，或 60s 上限（避免热循环）
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
