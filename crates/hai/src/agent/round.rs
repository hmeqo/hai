use crate::{
    agent::{event::WakeEvent, link::BotConn},
    app::AppContext,
    domain::service::DbServices,
};

/// 单次 agent 触发的完整轮次上下文
pub struct RoundContext {
    pub ctx: AppContext,
    pub chat_id: i64,
    pub conn: BotConn,
    pub events: Vec<WakeEvent>,
}

impl RoundContext {
    pub fn services(&self) -> DbServices {
        self.ctx.db.srv.clone()
    }
}
