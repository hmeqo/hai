use uuid::Uuid;

use super::SqlxNullableTimestamp;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScheduledTask {
    pub id: Uuid,
    pub bot_id: String,
    pub chat_id: i64,
    pub description: String,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub fire_at: jiff::Timestamp,
    pub every_secs: Option<i64>,
    pub is_active: bool,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "SqlxNullableTimestamp")]
    pub updated_at: Option<jiff::Timestamp>,
}

impl ScheduledTask {
    /// 周期任务下次触发：跳过已过时刻，取将来首个 fire_at（错过多次只补发一次）。
    pub fn next_fire_after(&self, now: jiff::Timestamp) -> Option<jiff::Timestamp> {
        let every = self.every_secs?;
        let step = jiff::SignedDuration::from_secs(every);
        let mut next = self.fire_at;
        while next <= now {
            next += step;
        }
        Some(next)
    }
}
