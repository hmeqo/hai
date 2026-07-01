use std::sync::Arc;

use async_trait::async_trait;
use genai::{
    Client,
    chat::{ChatMessage, ChatRole},
};

use super::{AgentNode, AgentOutput};
use crate::{
    agent::runtime::react::ReactLoop,
    agentcore::tool::{AgentTool, ToolError},
};

pub struct MainAgent {
    client: Client,
    model: String,
    system_prompt: String,
    max_turns: usize,
}

impl MainAgent {
    pub fn new(client: Client, model: String, system_prompt: String, max_turns: usize) -> Self {
        Self {
            client,
            model,
            system_prompt,
            max_turns,
        }
    }
}

#[async_trait]
impl AgentNode for MainAgent {
    fn name(&self) -> &str {
        "main_agent"
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Arc<dyn AgentTool>>,
    ) -> Result<AgentOutput, ToolError> {
        let full_messages = std::iter::once(ChatMessage::system(&self.system_prompt))
            .chain(messages)
            .collect();

        let output = ReactLoop::new(self.client.clone(), &self.model, self.max_turns)
            .run(full_messages, tools)
            .await?;

        // session 只存非 system 消息
        let mut msgs = output.messages;
        if msgs
            .first()
            .is_some_and(|m| matches!(m.role, ChatRole::System))
        {
            msgs.remove(0);
        }

        Ok(AgentOutput {
            tool_calls: output.tool_calls,
            messages: msgs,
            final_response: output.final_response.trim().to_owned(),
        })
    }
}
