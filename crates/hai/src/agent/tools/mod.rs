use std::sync::Arc;

use autoagents::prelude::ToolT;
use tokio::sync::mpsc;

use crate::{
    agent::{AttachmentService, event::BotSignal},
    app::AppContext,
    domain::service::DbServices,
};

pub mod account;
pub mod memory;
pub mod message;
pub mod multimodal;
pub mod scratchpad;
pub mod skills;
pub mod topic;
pub mod util;
pub mod voice;

pub struct ToolContext {
    pub ctx: AppContext,
    pub chat_id: i64,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
}

impl ToolContext {
    pub fn services(&self) -> DbServices {
        self.ctx.db.srv.clone()
    }

    pub fn attachment(&self) -> AttachmentService {
        self.ctx.agent.attachment.clone()
    }
}

pub fn get_main_agent_tools(ctx: ToolContext) -> Vec<Arc<dyn ToolT>> {
    let tools: Vec<Arc<dyn ToolT>> = [
        account::tools(&ctx),
        message::get_message_tools(&ctx),
        topic::get_topic_tools(&ctx),
        memory::tools(&ctx),
        scratchpad::tools(&ctx),
        multimodal::multimodal_tools(&ctx),
        voice::get_voice_tools(&ctx),
    ]
    .into_iter()
    .flatten()
    .collect();

    tools
}
