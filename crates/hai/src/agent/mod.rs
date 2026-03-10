pub mod handler;
pub mod multimodal;
pub mod node;
pub mod openrouter;
pub mod prompts;
pub mod render;
pub mod tools;

pub use handler::*;
pub use node::*;
pub use prompts::*;

use anyhow::Result;

#[async_trait::async_trait]
pub trait MessageSender: Send + Sync {
    async fn send_message(
        &self,
        chat_id: i64,
        content: String,
        reply_to_id: Option<i64>,
    ) -> Result<()>;
}
