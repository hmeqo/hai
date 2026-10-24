//! The Step loop: one run of the model over a message list, the tools it calls, and the decisions
//! that end it. The loop reports what it did and knows nothing about where that goes — `record.rs`
//! turns the reports into events.

use std::sync::Arc;

use genai::{
    Client,
    chat::{ChatMessage, ContentPart, MessageContent, Tool, ToolCall, ToolResponse},
};

use super::Messages;
use crate::{
    agent::{
        events::Doorbell,
        prompt::role::{RoleSpec, StepDecision, StepFacts},
    },
    agentcore::tool::{AgentTool, ToolError, schema, tool_err},
    domain::vo::{ChatId, ModelRetryReason, Step, StepNumber, ToolCallResult},
};

mod retry;

/// How many steps are left when the run is told about its budget; one notice, never repeated.
const BUDGET_NOTICE_AT: usize = 2;

/// The one notice a run gets before its last steps. Operational text, written once in English like the
/// `agents` and `wrap-up` layers.
const BUDGET_NOTICE: &str = "Notice: this run has only a couple of steps left. Finish the work now and answer with what you have; more reading will not fit.";

/// How many times the same call may return the same result before the loop points it out.
const STALL_REPEATS: usize = 3;

/// The nudge a repeated call earns. It never blocks the call: a tool may legitimately poll.
const STALL_NOTICE: &str = "Notice: that call returned the same result three times in a row. Change the approach, or answer with what you have.";

/// One thing the loop did, reported to whoever owns the run. The loop knows no event, no chat and no
/// turn number: a caller that records translates these.
pub(crate) enum StepEvent<'a> {
    /// The model answered, before its tool calls run: what it wrote and what it thought while
    /// writing it.
    Completed {
        step: StepNumber,
        response: &'a str,
        reasoning: Option<&'a str>,
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    ToolCall {
        step: StepNumber,
        call: &'a ToolCall,
    },
    ToolCallResult {
        step: StepNumber,
        result: &'a ToolCallResult,
    },
    Retrying(ModelRetryReason),
}

/// What a run does with the things it did.
pub(crate) trait StepObserver: Send {
    fn observed(&mut self, event: StepEvent<'_>);
}

/// One run: the model, the message list it continues, the role that decides when to stop, and how
/// many Steps it may take.
pub(crate) struct StepLoop<O: StepObserver> {
    pub(crate) client: Client,
    pub(crate) model: String,
    pub(crate) messages: Messages,
    pub(crate) spec: RoleSpec,
    pub(crate) budget: usize,
    /// The turn's arrival check; a pass that nothing may interrupt leaves it `None`.
    pub(crate) steering: Option<Steering>,
    pub(crate) observer: O,
    /// Whether the budget notice has been said, so a run hears it once.
    warned: bool,
    /// The last call's shape, for the repeat nudge.
    stall: StallGuard,
    /// A speaking call has already succeeded in this run: text after it is a closing remark, not an
    /// unsent reply.
    spoke: bool,
    /// Where this run's own injected messages sit, so [`transcript`](Self::transcript) can drop them.
    notices: Vec<usize>,
}

/// Where an interrupting arrival must come from, and the doorbell it rings.
pub(crate) struct Steering {
    pub chat_id: ChatId,
    pub since: u64,
    pub doorbell: Doorbell,
}

pub(crate) struct StepLoopOutput {
    pub steps: Vec<Step>,
    pub messages: Messages,
    /// Whether something new arrived mid-run.
    pub steered: bool,
    /// Whether the loop stopped because the budget was spent rather than because the role stopped:
    /// the last step is then unfinished work, not a decision.
    pub truncated: bool,
}

impl<O: StepObserver> StepLoop<O> {
    /// A run that has not started: no notice said, no repeat seen, nothing injected.
    pub(crate) fn new(
        client: Client,
        model: String,
        messages: Messages,
        spec: RoleSpec,
        budget: usize,
        steering: Option<Steering>,
        observer: O,
    ) -> Self {
        Self {
            client,
            model,
            messages,
            spec,
            budget,
            steering,
            observer,
            warned: false,
            stall: StallGuard::default(),
            spoke: false,
            notices: Vec::new(),
        }
    }

    /// Runs until the role says stop, the budget is spent, or — when the spec asked for it and the
    /// caller handed one over — a new arrival interrupts it.
    pub(crate) async fn run(
        mut self,
        tools: &[Arc<dyn AgentTool>],
    ) -> Result<StepLoopOutput, ToolError> {
        let genai_tools = prepare_genai_tools(tools);
        let mut steps: Vec<Step> = Vec::new();

        loop {
            let remaining = self.budget.saturating_sub(steps.len());
            if !self.warned && remaining <= BUDGET_NOTICE_AT {
                self.warned = true;
                self.notice(ChatMessage::user(BUDGET_NOTICE));
            }
            let step = self.run_step(&genai_tools, tools, steps.len()).await?;
            let hit_step_limit = !step.stop && steps.len() + 1 >= self.budget;
            if hit_step_limit {
                tracing::warn!(step = steps.len() + 1, "run truncated at its step budget");
            }
            steps.push(step.step);
            if step.stalled {
                self.notice(ChatMessage::user(STALL_NOTICE));
            }

            if let Some(reason) = step.retry {
                self.observer.observed(StepEvent::Retrying(reason));
            }

            if step.stop || hit_step_limit {
                return Ok(StepLoopOutput {
                    steps,
                    messages: self.transcript(),
                    steered: false,
                    truncated: hit_step_limit,
                });
            }

            if self
                .steering
                .as_ref()
                .is_some_and(|steering| steering.doorbell.steered(steering.chat_id, steering.since))
            {
                return Ok(StepLoopOutput {
                    steps,
                    messages: self.transcript(),
                    steered: true,
                    truncated: false,
                });
            }
        }
    }

    /// Appends a run-local message for the next Step, remembering where it sits: a later Turn must
    /// not read a spent budget's warning, or a contract guide, as its own.
    fn notice(&mut self, message: ChatMessage) {
        self.notices.push(self.messages.len());
        self.messages.push(message);
    }

    /// The transcript this run leaves: everything it read and wrote, minus its own notices.
    fn transcript(mut self) -> Messages {
        for at in self.notices.iter().rev() {
            self.messages.remove(*at);
        }
        self.messages
    }

    async fn run_step(
        &mut self,
        genai_tools: &[Tool],
        tools: &[Arc<dyn AgentTool>],
        index: usize,
    ) -> Result<StepRun, ToolError> {
        let step = StepNumber::from(index + 1);
        let res = retry::llm_call_with_retry(
            &self.client,
            &self.model,
            &self.spec,
            genai_tools,
            &self.messages,
            &mut self.observer,
        )
        .await?;

        let response_text = res.texts().join("\n");
        let reasoning = res.reasoning_content.clone();
        let prompt_tokens = res.usage.prompt_tokens.unwrap_or(0) as u32;
        let completion_tokens = res.usage.completion_tokens.unwrap_or(0) as u32;
        let active_calls: Vec<ToolCall> = res.into_tool_calls();

        self.messages.push(build_assistant_message(
            &response_text,
            &active_calls,
            reasoning.clone(),
        ));

        self.observer.observed(StepEvent::Completed {
            step,
            response: &response_text,
            reasoning: reasoning.as_deref(),
            prompt_tokens,
            completion_tokens,
        });

        let (tool_calls, stalled) = self.run_tool_calls(&active_calls, tools, step).await;

        let (stop, retry) = match self.spec.decide_step(StepFacts::from_calls(
            step,
            &response_text,
            &active_calls,
            tools,
            self.spoke,
        )) {
            StepDecision::Continue => (false, None),
            StepDecision::Stop => (true, None),
            StepDecision::Retry { reason, guide } => {
                self.notice(guide);
                (false, Some(reason))
            }
        };

        Ok(StepRun {
            step: Step {
                tool_calls,
                response: response_text,
                reasoning,
                prompt_tokens,
                completion_tokens,
            },
            stop,
            retry,
            stalled,
        })
    }

    async fn run_tool_calls(
        &mut self,
        calls: &[ToolCall],
        tools: &[Arc<dyn AgentTool>],
        step: StepNumber,
    ) -> (Vec<ToolCallResult>, bool) {
        let mut results: Vec<ToolCallResult> = Vec::new();
        let mut stalled = false;
        for call in calls {
            self.observer.observed(StepEvent::ToolCall { step, call });
            let speaking = tools
                .iter()
                .any(|tool| tool.name() == call.fn_name && tool.speaks());
            let result = execute_single_tool(call, tools, &self.spec, &mut self.messages).await;
            if speaking && result.success {
                self.spoke = true;
            }
            self.observer.observed(StepEvent::ToolCallResult {
                step,
                result: &result,
            });
            stalled |= self.stall.observe(call, &result);
            results.push(result);
        }
        (results, stalled)
    }
}

struct StepRun {
    step: Step,
    stop: bool,
    retry: Option<ModelRetryReason>,
    /// Whether one call returned what it already did twice in a row.
    stalled: bool,
}

/// What the last tool call was and what it returned; the same pair [`STALL_REPEATS`] times in a row
/// trips the nudge. The key owns its values, which the call already holds in memory.
#[derive(Default)]
pub(crate) struct StallGuard {
    last: Option<(String, serde_json::Value, bool, serde_json::Value)>,
    repeats: usize,
}

impl StallGuard {
    /// Notes one call and its result; `true` when the same call has now returned the same result
    /// [`STALL_REPEATS`] times running. A tripped guard starts its count over.
    fn observe(&mut self, call: &ToolCall, result: &ToolCallResult) -> bool {
        let key = (
            call.fn_name.clone(),
            call.fn_arguments.clone(),
            result.success,
            result.result.clone(),
        );
        self.repeats = if self.last.as_ref() == Some(&key) {
            self.repeats + 1
        } else {
            1
        };
        self.last = Some(key);
        if self.repeats >= STALL_REPEATS {
            self.repeats = 0;
            self.last = None;
            true
        } else {
            false
        }
    }
}

fn prepare_genai_tools(tools: &[Arc<dyn AgentTool>]) -> Vec<Tool> {
    tools
        .iter()
        .map(|t| {
            let mut tool = Tool::new(t.name()).with_description(t.description());
            if let Some(s) = t.schema() {
                tool = tool.with_schema(schema::normalize_root(t.name(), s));
            }
            tool
        })
        .collect()
}

fn build_assistant_message(
    response_text: &str,
    tool_calls: &[ToolCall],
    reasoning_content: Option<String>,
) -> ChatMessage {
    let mut parts = Vec::new();
    if !response_text.is_empty() {
        parts.push(ContentPart::from_text(response_text));
    }
    for call in tool_calls {
        parts.push(ContentPart::ToolCall(call.clone()));
    }
    let mut msg = ChatMessage::assistant(MessageContent::from_parts(parts));
    if let Some(r) = reasoning_content {
        msg = msg.with_reasoning_content(Some(r));
    }
    msg
}

async fn execute_single_tool(
    call: &ToolCall,
    tools: &[Arc<dyn AgentTool>],
    spec: &RoleSpec,
    messages: &mut Messages,
) -> ToolCallResult {
    let tool_name = &call.fn_name;
    let args = &call.fn_arguments;

    let result = match tools.iter().find(|t| t.name() == tool_name) {
        Some(tool) if spec.rejects(tool.as_ref()) => Err(tool_err(format!(
            "{tool_name} is not available in this pass"
        ))),
        Some(tool) => tool.execute(args.clone()).await,
        None => Err(tool_err(format!("Unknown tool: {tool_name}"))),
    };

    match result {
        Ok(val) => {
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                val.to_string(),
            )));
            ToolCallResult::ok(tool_name.clone(), args.clone(), val.clone())
        }
        Err(e) => {
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                format!("Error: {e}"),
            )));
            ToolCallResult::err(tool_name.clone(), args.clone(), format!("{e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_repeated_call_trips_the_stall_guard() {
        let call = ToolCall {
            call_id: "1".into(),
            fn_name: "read".into(),
            fn_arguments: json!({ "a": 1 }),
            thought_signatures: None,
        };
        let same = ToolCallResult::ok("read", json!({ "a": 1 }), json!("same"));

        let mut guard = StallGuard::default();
        assert!(!guard.observe(&call, &same));
        assert!(!guard.observe(&call, &same));
        assert!(guard.observe(&call, &same));

        // A different result starts the count over, and the trip left the guard ready to count again.
        let other = ToolCallResult::ok("read", json!({ "a": 1 }), json!("other"));
        assert!(!guard.observe(&call, &other));
        assert!(!guard.observe(&call, &other));
        assert!(guard.observe(&call, &other));
    }
}
