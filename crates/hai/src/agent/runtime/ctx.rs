use kameo::actor::ActorRef;

use crate::{
    agent::{event::WakeEvent, link::BotHandle, runtime::actor::ChatActor},
    agentcore::skills::SkillManager,
    app::AppContext,
    domain::{entity::ChatType, service::DbServices, vo::ChatId},
};

/// 一轮 task 执行的上下文环境。
///
/// 由 ChatActor 创建，贯穿 prepare → RoundTask 整轮生命周期。
pub struct RoundCtx {
    pub app: AppContext,
    pub chat_id: ChatId,
    pub chat_type: ChatType,
    pub bot: BotHandle,
    /// 本轮唤醒事件
    pub events: Vec<WakeEvent>,
    pub skill_manager: SkillManager,
    pub session: ActorRef<ChatActor>,
}

impl RoundCtx {
    pub fn services(&self) -> DbServices {
        self.app.db.srv.clone()
    }
}
