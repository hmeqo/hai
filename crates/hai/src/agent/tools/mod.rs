use std::sync::Arc;

use autoagents::prelude::ToolT;

use crate::agent::runtime::ctx::RoundContext;

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

pub fn get_main_agent_tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    [
        account::tools,
        message::tools,
        topic::tools,
        memory::tools,
        scratchpad::tools,
        multimodal::tools,
        voice::tools,
        shell::tools,
    ]
    .into_iter()
    .flat_map(|f| f(ctx))
    .collect()
}
