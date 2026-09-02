use uuid::Uuid;

use crate::{
    domain::{model::ScheduledTask, repo::Repos, vo::ChatId},
    error::Result,
};

#[derive(Debug)]
pub struct ScheduledTaskService {
    repos: Repos,
}

impl ScheduledTaskService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// 创建任务。`every_secs` = Some 周期 / None 一次性。
    pub async fn create(
        &self,
        bot_id: &str,
        chat_id: ChatId,
        description: &str,
        fire_at: jiff::Timestamp,
        every_secs: Option<i64>,
    ) -> Result<ScheduledTask> {
        self.repos
            .scheduled_task
            .create(bot_id, chat_id.0, description, fire_at, every_secs)
            .await
    }

    pub async fn list_active(&self, bot_id: &str, chat_id: ChatId) -> Result<Vec<ScheduledTask>> {
        self.repos
            .scheduled_task
            .list_active_by_chat(bot_id, chat_id.0)
            .await
    }

    /// 列出全部（含停用）。
    pub async fn list_all(&self, bot_id: &str, chat_id: ChatId) -> Result<Vec<ScheduledTask>> {
        self.repos
            .scheduled_task
            .list_all_by_chat(bot_id, chat_id.0)
            .await
    }

    /// 取消 = 置 is_active=false（DB 行保留，审计留痕）。
    pub async fn cancel(&self, bot_id: &str, chat_id: ChatId, id: Uuid) -> Result<()> {
        self.repos
            .scheduled_task
            .deactivate(bot_id, chat_id.0, id)
            .await
    }

    /// 到点任务（watcher 用）。
    pub async fn due(&self, bot_id: &str, now: jiff::Timestamp) -> Result<Vec<ScheduledTask>> {
        self.repos.scheduled_task.due(bot_id, now).await
    }

    /// 触发后推进：周期任务挪到下一触发点，一次性任务停用。
    pub async fn advance(&self, task: &ScheduledTask, now: jiff::Timestamp) -> Result<()> {
        let next = task.next_fire_after(now);
        self.repos
            .scheduled_task
            .advance_fire_at(task.id, next)
            .await
    }
}
