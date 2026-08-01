use std::{sync::Arc, time::Duration};

use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

const DEFAULT_SECS: u8 = 3;

fn default_secs() -> u8 {
    DEFAULT_SECS
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SleepArgs {
    #[serde(default = "default_secs")]
    #[schemars(range(min = 1, max = 10))]
    secs: u8,
}

/// 等一会儿。
///
/// 当对方可能还在打字、话还没说完时，可以先等一下，让更多消息到了再一起处理。
/// 不要滥用——大多数时候直接回应就好。
#[hai_macros::tool]
pub struct Sleep;

impl Sleep {
    async fn exec(&self, args: SleepArgs) -> Result<serde_json::Value, ToolError> {
        if args.secs < 1 || args.secs > 10 {
            return Err(ToolError::Msg("secs 必须在 1-10 之间".into()));
        }
        tokio::time::sleep(Duration::from_secs(args.secs as u64)).await;
        tool_ok()
    }
}

pub fn tools(_ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(Sleep)]
}
