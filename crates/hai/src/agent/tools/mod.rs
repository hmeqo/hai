use std::sync::Arc;

use autoagents::prelude::ToolT;
use tokio::sync::mpsc;

use crate::{agent::event::BotSignal, domain::service::Services};

pub mod account;
pub mod memory;
pub mod message;
pub mod scratchpad;
pub mod topic;
pub mod util;

pub use util::ToolResult;

pub fn get_main_agent_tools(
    services: Arc<Services>,
    chat_id: i64,
    signal_tx: mpsc::UnboundedSender<BotSignal>,
) -> Vec<Arc<dyn ToolT>> {
    [
        account::tools(Arc::clone(&services)),
        message::get_message_tools(Arc::clone(&services), chat_id, signal_tx),
        topic::get_topic_tools(Arc::clone(&services)),
        memory::tools(Arc::clone(&services)),
        scratchpad::tools(Arc::clone(&services)),
    ]
    .into_iter()
    .flatten()
    .collect()
}
