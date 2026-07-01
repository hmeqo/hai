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

use std::sync::Arc;

use crate::{agent::runtime::tool_ctx::ToolContext, agentcore::tool::AgentTool};

pub fn get_main_agent_tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
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
