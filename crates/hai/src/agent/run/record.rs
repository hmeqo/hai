//! What a session's Step loop reports, written as the audit trail's own vocabulary.

use super::step_loop::{StepEvent, StepObserver};
use crate::{
    agent::events::AgentEventBus,
    domain::vo::{AgentEvent, AgentEventPayload, ChatId, SessionRef, StepOutput, TurnNumber},
};

/// One session's steps, written as events: the session and its turn travel with each record.
pub(crate) struct SessionRecord {
    pub event_bus: AgentEventBus,
    pub chat_id: ChatId,
    pub session: SessionRef,
    pub turn: TurnNumber,
}

impl StepObserver for SessionRecord {
    fn observed(&mut self, event: StepEvent<'_>) {
        self.event_bus
            .emit(event_of(self.chat_id, self.session, self.turn, event));
    }
}

/// The event one reported fact becomes: the audit trail's own vocabulary, with the session and its
/// turn filled in. Pure, so a test reads it without a bus.
pub(crate) fn event_of(
    chat_id: ChatId,
    session: SessionRef,
    turn: TurnNumber,
    event: StepEvent<'_>,
) -> AgentEvent {
    let payload = match event {
        StepEvent::Completed {
            step,
            response,
            reasoning,
            prompt_tokens,
            completion_tokens,
        } => AgentEventPayload::StepCompleted {
            turn,
            step,
            output: StepOutput {
                turn,
                step,
                reasoning: reasoning.map(str::to_owned),
                response: response.to_owned(),
            },
            prompt_tokens,
            completion_tokens,
        },
        StepEvent::ToolCall { step, call } => AgentEventPayload::ToolCall {
            turn,
            step,
            tool: call.fn_name.clone(),
            args: call.fn_arguments.to_string(),
        },
        StepEvent::ToolCallResult { step, result } => AgentEventPayload::ToolCallResult {
            turn,
            step,
            tool: result.tool_name.clone(),
            summary: result.result.to_string(),
            success: result.success,
        },
        StepEvent::Retrying(reason) => AgentEventPayload::ModelRetry { turn, reason },
    };
    AgentEvent {
        chat_id,
        session: Some(session),
        payload,
    }
}

#[cfg(test)]
mod tests {
    use genai::chat::ToolCall;

    use super::*;
    use crate::domain::vo::{ModelRetryReason, SessionId, StepNumber, ToolCallResult};

    fn tool_call(name: &str) -> ToolCall {
        ToolCall {
            call_id: "1".into(),
            fn_name: name.into(),
            fn_arguments: serde_json::json!({ "a": 1 }),
            thought_signatures: None,
        }
    }

    /// What the loop reports becomes the events the audit trail carries, with the session and the
    /// turn it belongs to on each of them.
    #[test]
    fn a_step_becomes_an_event_of_its_session() {
        let chat = ChatId(7);
        let parent = SessionId::new();
        let session = SessionRef::wrap_up(SessionId::new(), parent);
        let turn = TurnNumber::from(1);
        let step = StepNumber::from(2);

        let completed = event_of(
            chat,
            session,
            turn,
            StepEvent::Completed {
                step,
                response: "hi",
                reasoning: Some("because"),
                prompt_tokens: 11,
                completion_tokens: 5,
            },
        );
        assert_eq!(completed.chat_id, chat);
        assert_eq!(completed.session, Some(session));
        assert_eq!(completed.session.and_then(|s| s.parent), Some(parent));
        let AgentEventPayload::StepCompleted {
            turn: reported_turn,
            step: reported_step,
            output,
            prompt_tokens,
            completion_tokens,
        } = completed.payload
        else {
            panic!("expected a completed step; got {completed:?}");
        };
        assert_eq!((reported_turn, reported_step), (turn, step));
        assert_eq!((output.turn, output.step), (turn, step));
        assert_eq!(output.response, "hi");
        assert_eq!(output.reasoning.as_deref(), Some("because"));
        assert_eq!((prompt_tokens, completion_tokens), (11, 5));

        let called = tool_call("read");
        let reported = event_of(
            chat,
            session,
            turn,
            StepEvent::ToolCall {
                step,
                call: &called,
            },
        );
        let AgentEventPayload::ToolCall {
            turn: reported_turn,
            tool,
            args,
            ..
        } = reported.payload
        else {
            panic!("expected a tool call; got {reported:?}");
        };
        assert_eq!(reported_turn, turn);
        assert_eq!(tool, "read");
        assert_eq!(args, "{\"a\":1}");

        let result = ToolCallResult::ok("read", called.fn_arguments, serde_json::json!("done"));
        let reported = event_of(
            chat,
            session,
            turn,
            StepEvent::ToolCallResult {
                step,
                result: &result,
            },
        );
        let AgentEventPayload::ToolCallResult {
            tool,
            summary,
            success,
            ..
        } = reported.payload
        else {
            panic!("expected a tool result; got {reported:?}");
        };
        assert_eq!(tool, "read", "the result names the tool, not the call");
        assert_eq!(summary, "\"done\"");
        assert!(success);

        let reported = event_of(
            chat,
            session,
            turn,
            StepEvent::Retrying(ModelRetryReason::TimeoutRetry),
        );
        assert!(
            matches!(
                reported.payload,
                AgentEventPayload::ModelRetry { turn: reported_turn, reason: ModelRetryReason::TimeoutRetry }
                    if reported_turn == turn
            ),
            "{reported:?}"
        );
    }
}
