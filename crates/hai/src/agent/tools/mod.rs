pub mod account;
pub mod done;
pub mod memory;
pub mod message;
pub mod multimodal;
pub mod shell;
pub mod skills;
pub mod sleep;
pub mod think;
pub mod topic;
pub mod util;
pub mod voice;

use std::sync::Arc;

use crate::{agent::runtime::context::ToolContext, agentcore::tool::AgentTool};

pub fn get_main_agent_tools(
    ctx: &ToolContext,
    tts_enabled: bool,
    enabled_parsers: &[&str],
    sandbox_image: Option<&str>,
) -> Vec<Arc<dyn AgentTool>> {
    let mut all: Vec<Arc<dyn AgentTool>> = Vec::new();
    all.extend(account::tools(ctx));
    all.extend(done::tools(ctx));
    all.extend(sleep::tools(ctx));
    all.extend(message::tools(ctx));
    all.extend(topic::tools(ctx));
    all.extend(memory::tools(ctx));
    all.extend(skills::tools(ctx));
    all.extend(think::tools(ctx));
    if tts_enabled {
        all.extend(voice::tools(ctx));
    }
    if !enabled_parsers.is_empty() {
        all.extend(multimodal::tools(ctx, enabled_parsers));
    }
    all.extend(shell::tools(ctx, sandbox_image));
    all
}
