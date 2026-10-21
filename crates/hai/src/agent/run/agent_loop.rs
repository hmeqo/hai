//! One Turn's inputs and entry point: it builds the Step loop and installs the session's recorder
//! (`record.rs`), handing the loop a doorbell only when the role asked to be steered.

use std::sync::Arc;

use genai::Client;

use super::{
    Messages,
    record::SessionRecord,
    step_loop::{StepLoop, StepLoopOutput},
};
use crate::{
    agent::{
        events::{AgentEventBus, Doorbell},
        prompt::role::RoleSpec,
    },
    agentcore::tool::{AgentTool, ToolError},
    domain::vo::{ChatId, SessionRef, TurnNumber},
};

/// One Turn's inputs, as the chat hands them over.
pub(crate) struct LoopTurn {
    pub client: Client,
    pub model: String,
    pub messages: Messages,
    pub spec: RoleSpec,
    pub steering: Doorbell,
    pub event_bus: AgentEventBus,
    pub chat_id: ChatId,
    pub turn_number: TurnNumber,
    /// The session this run's events belong to.
    pub session: SessionRef,
    /// The ceiling this run may spend, from `[agent] max-steps`.
    pub budget: usize,
}

impl LoopTurn {
    pub(crate) fn new(
        runtime: &crate::agent::process::AgentRuntime,
        spec: RoleSpec,
        chat_id: ChatId,
        messages: Messages,
        steering: Doorbell,
        turn_number: TurnNumber,
        session: SessionRef,
    ) -> Self {
        Self {
            client: runtime.client.clone(),
            model: runtime.model.clone(),
            messages,
            spec,
            steering,
            event_bus: runtime.event_bus().clone(),
            chat_id,
            turn_number,
            session,
            budget: runtime.cfg().agent.max_steps,
        }
    }
}

pub(crate) async fn run_loop(
    turn: LoopTurn,
    tools: &[Arc<dyn AgentTool>],
) -> Result<StepLoopOutput, ToolError> {
    let LoopTurn {
        client,
        model,
        spec,
        messages,
        steering,
        event_bus,
        chat_id,
        turn_number,
        session,
        budget,
    } = turn;
    // A pass nothing may interrupt leaves the arrival check out; the spec is what asks for one.
    let watches_arrivals = spec.steering;
    let step_loop = StepLoop::new(
        client,
        model,
        messages,
        spec,
        budget,
        watches_arrivals.then_some(steering),
        SessionRecord {
            event_bus,
            chat_id,
            session,
            turn: turn_number,
        },
    );

    step_loop.run(tools).await
}
