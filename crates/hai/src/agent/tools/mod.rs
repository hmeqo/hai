use std::sync::Arc;

use autoagents::prelude::ToolT;

use crate::agent::runtime::ctx::RoundCtx;

pub mod account;
pub mod memory;
pub mod message;
pub mod multimodal;
pub mod scratchpad;
pub mod shell;
pub mod skills;
pub mod topic;
pub mod util;
pub mod voice;

pub fn get_main_agent_tools(ctx: &RoundCtx) -> Vec<Arc<dyn ToolT>> {
    let tools: Vec<Arc<dyn ToolT>> = [
        account::tools(ctx),
        message::get_message_tools(ctx),
        topic::get_topic_tools(ctx),
        memory::tools(ctx),
        scratchpad::tools(ctx),
        multimodal::multimodal_tools(ctx),
        voice::get_voice_tools(ctx),
        shell::tools(ctx),
    ]
    .into_iter()
    .flatten()
    .collect();

    tools
}
