use crate::{
    agent::{AttachmentService, event::AgentEvent, link::BotConn},
    app::AppContext,
    domain::service::DbServices,
};

/// 单次 agent 触发的完整轮次上下文
pub struct RoundContext {
    pub ctx: AppContext,
    pub chat_id: i64,
    pub conn: BotConn,
    pub events: Vec<AgentEvent>,
}

impl RoundContext {
    pub fn services(&self) -> DbServices {
        self.ctx.db.srv.clone()
    }

    pub fn attachment(&self) -> AttachmentService {
        self.ctx.agent.attachment.clone()
    }
}
