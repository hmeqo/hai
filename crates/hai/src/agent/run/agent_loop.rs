//! One Turn's inputs and entry point: it builds the Step loop and installs the session's recorder
//! (`record.rs`), handing the loop a doorbell only when the role asked to be steered.

use std::sync::Arc;

use genai::Client;

use super::{
    Messages,
    record::SessionRecord,
    step_loop::{Steering, StepConfig, StepLoop, StepLoopOutput},
    turn::SelectedModel,
};
use crate::{
    agent::{
        events::{AgentEventBus, Doorbell},
        model::ModelCapabilities,
        process::AgentRuntime,
        prompt::role::RoleSpec,
    },
    agentcore::tool::{AgentTool, ToolCallContext, ToolError},
    domain::vo::{ChatId, SessionRef, TurnNumber},
};

/// One Turn's inputs, as the chat hands them over.
pub(crate) struct LoopTurn {
    pub client: Client,
    pub model: String,
    pub capabilities: ModelCapabilities,
    pub messages: Messages,
    pub spec: RoleSpec,
    pub steering: Doorbell,
    pub steering_since: u64,
    pub event_bus: AgentEventBus,
    pub chat_id: ChatId,
    pub turn_number: TurnNumber,
    /// The session this run's events belong to.
    pub session: SessionRef,
    /// The ceiling this run may spend, from `[agent] max-steps`.
    pub budget: usize,
}

/// The identity a run's events belong to.
pub(crate) struct TurnIdentity {
    pub chat_id: ChatId,
    pub turn_number: TurnNumber,
    pub session: SessionRef,
}

impl LoopTurn {
    pub(crate) fn new(
        runtime: &AgentRuntime,
        selected: &SelectedModel,
        spec: RoleSpec,
        identity: TurnIdentity,
        messages: Messages,
        steering: Doorbell,
        steering_since: u64,
    ) -> Self {
        Self {
            client: selected.client.clone(),
            model: selected.model.clone(),
            capabilities: selected.capabilities,
            messages,
            spec,
            steering,
            steering_since,
            event_bus: runtime.event_bus().clone(),
            chat_id: identity.chat_id,
            turn_number: identity.turn_number,
            session: identity.session,
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
        capabilities,
        spec,
        messages,
        steering,
        steering_since,
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
        ToolCallContext {
            image_input: capabilities.image_input,
        },
        messages,
        spec,
        StepConfig {
            budget,
            steering: watches_arrivals.then_some(Steering {
                chat_id,
                since: steering_since,
                doorbell: steering,
            }),
        },
        SessionRecord {
            event_bus,
            chat_id,
            session,
            turn: turn_number,
        },
    );

    step_loop.run(tools).await
}
