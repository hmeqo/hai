use std::collections::HashMap;

use kameo::actor::ActorRef;
use tokio::sync::RwLock;

use super::AgentEngine;
use crate::{
    agent::{
        link::BotHandle,
        runtime::actor::{ChatActor, spawn_chat_actor},
    },
    domain::vo::ChatId,
};

/// 集中管理所有 ChatActor 的生命周期
pub struct ChatActorManager {
    actors: RwLock<HashMap<ChatId, ActorRef<ChatActor>>>,
    bot: BotHandle,
    engine: AgentEngine,
}

impl ChatActorManager {
    pub fn new(bot: BotHandle, engine: AgentEngine) -> Self {
        Self {
            actors: RwLock::new(HashMap::new()),
            bot,
            engine,
        }
    }

    /// 获取或创建指定 chat 的会话
    pub async fn get_or_create(&self, chat_id: ChatId) -> ActorRef<ChatActor> {
        if let Some(actor) = self.actors.read().await.get(&chat_id) {
            return actor.clone();
        }
        let actor = spawn_chat_actor(chat_id, self.bot.clone(), self.engine.clone()).await;
        self.actors.write().await.insert(chat_id, actor.clone());
        actor
    }

    /// 查询存在的会话（不创建）
    pub async fn get(&self, chat_id: ChatId) -> Option<ActorRef<ChatActor>> {
        self.actors.read().await.get(&chat_id).cloned()
    }
}
