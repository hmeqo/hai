pub mod knowledge;
pub mod memory;
pub mod multimodal;
pub mod schedule;
pub mod send;
pub mod shell;
pub mod skills;
pub mod skip;
pub mod sleep;
pub mod topic;
pub mod util;

use std::sync::Arc;

use crate::{agent::runtime::context::ToolContext, agentcore::tool::AgentTool};

pub fn get_main_agent_tools(
    ctx: &ToolContext,
    enabled_parsers: &[&str],
    sandbox_image: Option<&str>,
) -> Vec<Arc<dyn AgentTool>> {
    let mut all: Vec<Arc<dyn AgentTool>> = Vec::new();
    all.extend(skip::tools(ctx));
    all.extend(sleep::tools(ctx));
    all.extend(send::tools(ctx));
    all.extend(topic::tools(ctx));
    all.extend(schedule::tools(ctx));
    all.extend(memory::tools(ctx));
    all.extend(knowledge::tools(ctx));
    all.extend(skills::tools(ctx));
    if !enabled_parsers.is_empty() {
        all.extend(multimodal::tools(ctx, enabled_parsers));
    }
    all.extend(shell::tools(ctx, sandbox_image));
    all
}

pub fn get_wrap_up_tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let mut all: Vec<Arc<dyn AgentTool>> = Vec::new();
    all.extend(topic::tools(ctx));
    all.extend(memory::tools(ctx));
    all
}
