//! One run of the model: the loop, the record layer around it, the context it acts with, and what
//! it hands back.

mod agent_loop;
pub(crate) mod engine;
pub(crate) mod record;
pub(crate) mod step_loop;
pub(crate) mod turn;
pub(crate) mod wire_messages;

use std::sync::Arc;

use genai::chat::ChatMessage;
pub(crate) use turn::{SelectedModel, ToolContext, TurnContext};

use crate::{agentcore::tool::AgentTool, domain::vo::Step};

/// The message list one run reads, shared until something writes to it.
#[derive(Clone)]
pub struct Messages(Arc<Vec<ChatMessage>>);

impl Messages {
    pub fn new(msgs: Vec<ChatMessage>) -> Self {
        Self(Arc::new(msgs))
    }

    pub fn push(&mut self, msg: ChatMessage) {
        Arc::make_mut(&mut self.0).push(msg);
    }

    /// Removes the message at `index`; a run's own notice uses it so it never outlives the run.
    pub fn remove(&mut self, index: usize) -> ChatMessage {
        Arc::make_mut(&mut self.0).remove(index)
    }

    pub fn extend(&mut self, msgs: impl IntoIterator<Item = ChatMessage>) {
        Arc::make_mut(&mut self.0).extend(msgs);
    }

    pub fn to_vec(&self) -> Vec<ChatMessage> {
        (*self.0).clone()
    }

    /// Takes the messages out without copying when this is the only handle; a snapshot is the
    /// owner at the end of a Turn.
    pub fn into_vec(self) -> Vec<ChatMessage> {
        Arc::unwrap_or_clone(self.0)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// What one run leaves behind: the messages it ended with, the steps it took, and whether anything
/// it wrote reached the chat.
pub struct TurnOutput {
    pub messages: Messages,
    pub steps: Vec<Step>,
    pub has_spoken: bool,
}

pub(crate) fn has_spoken(steps: &[Step], tools: &[Arc<dyn AgentTool>]) -> bool {
    steps
        .iter()
        .flat_map(|s| &s.tool_calls)
        .any(|tc| tc.success && tools.iter().any(|t| t.name() == tc.tool_name && t.speaks()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::has_spoken;
    use crate::{
        agent::test_support::StubTool,
        agentcore::tool::AgentTool,
        domain::vo::{Step, ToolCallResult},
    };

    fn step(tool: &str) -> Step {
        Step {
            tool_calls: vec![ToolCallResult::ok(tool, json!({}), json!({"ok": true}))],
            response: String::new(),
            reasoning: None,
            prompt_tokens: 0,
            completion_tokens: 0,
        }
    }

    #[test]
    fn has_spoken_follows_tool_metadata() {
        let tools: Vec<Arc<dyn AgentTool>> = vec![
            StubTool::new("send_message").speaks(true).into_dyn(),
            StubTool::new("topic").into_dyn(),
        ];

        assert!(has_spoken(&[step("send_message")], &tools));
        assert!(!has_spoken(&[step("topic")], &tools));
        assert!(!has_spoken(&[step("unknown_tool")], &tools));
    }
}
