//! The Step loop: one run of the model over a message list, the tools it calls, and the decisions
//! that end it. The loop reports what it did and knows nothing about where that goes — `record.rs`
//! turns the reports into events.

use std::sync::Arc;

use base64::{Engine as _, prelude::BASE64_STANDARD};
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
    agentcore::tool::{AgentTool, ToolCallContext, ToolError, ToolMedia, schema, tool_err},
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
    pub(crate) tool_context: ToolCallContext,
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
    /// Where this run's own notices sit, so [`into_messages`](Self::into_messages) can drop them.
    /// Images are message content and are never listed here.
    transient: Vec<usize>,
}

/// Where an interrupting arrival must come from, and the doorbell it rings.
pub(crate) struct Steering {
    pub chat_id: ChatId,
    pub since: u64,
    pub doorbell: Doorbell,
}

/// The run's budget and its optional arrival check.
pub(crate) struct StepConfig {
    pub budget: usize,
    pub steering: Option<Steering>,
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
        tool_context: ToolCallContext,
        messages: Messages,
        spec: RoleSpec,
        config: StepConfig,
        observer: O,
    ) -> Self {
        Self {
            client,
            model,
            tool_context,
            messages,
            spec,
            budget: config.budget,
            steering: config.steering,
            observer,
            warned: false,
            stall: StallGuard::default(),
            spoke: false,
            transient: Vec::new(),
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
                    messages: self.into_messages(),
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
                    messages: self.into_messages(),
                    steered: true,
                    truncated: false,
                });
            }
        }
    }

    /// Appends a run-local message for the next Step, remembering where it sits: a later Turn must
    /// not read a spent budget's warning, or a contract guide, as its own.
    fn notice(&mut self, message: ChatMessage) {
        self.transient.push(self.messages.len());
        self.messages.push(message);
    }

    /// The messages this run leaves: everything it read and wrote, minus its own notices.
    fn into_messages(mut self) -> Messages {
        for at in self.transient.iter().rev() {
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
        let prompt_tokens = res.usage.prompt_tokens.unwrap_or(0) as u32;
        let completion_tokens = res.usage.completion_tokens.unwrap_or(0) as u32;
        let active_calls: Vec<ToolCall> = res.tool_calls().into_iter().cloned().collect();

        // The Step and the assistant message both need the reasoning text, and the tool-use builder
        // folds its own copy into the content: one clone is the cost of keeping both.
        let (assistant, reasoning) = if active_calls.is_empty() {
            let reasoning = res.reasoning_content;
            let mut message = ChatMessage::assistant(res.content);
            let carries_reasoning = message
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::ReasoningContent(_)));
            if !carries_reasoning && let Some(text) = &reasoning {
                message = message.with_reasoning_content(Some(text.clone()));
            }
            (message, reasoning)
        } else {
            let reasoning = res.reasoning_content.clone();
            let message = res
                .into_assistant_message_for_tool_use()
                .expect("a response with tool calls builds a tool-use message");
            (message, reasoning)
        };
        self.messages.push(assistant);

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
            let ExecutedTool { result, media } = self.execute_tool(call, tools).await;
            if speaking && result.success {
                self.spoke = true;
            }
            self.observer.observed(StepEvent::ToolCallResult {
                step,
                result: &result,
            });
            stalled |= self.stall.observe(call, &result);
            if !media.is_empty() {
                self.messages.push(image_message(call, media));
            }
            results.push(result);
        }
        (results, stalled)
    }

    async fn execute_tool(
        &mut self,
        call: &ToolCall,
        tools: &[Arc<dyn AgentTool>],
    ) -> ExecutedTool {
        let tool_name = &call.fn_name;
        let args = &call.fn_arguments;

        let result = match tools.iter().find(|t| t.name() == tool_name) {
            Some(t) if self.spec.rejects(t.as_ref()) => Err(tool_err(format!(
                "{tool_name} is not available in this pass"
            ))),
            Some(tool) => tool.execute(args.clone(), &self.tool_context).await,
            None => Err(tool_err(format!("Unknown tool: {tool_name}"))),
        };

        match result {
            Ok(output) => {
                self.messages
                    .push(ChatMessage::from(ToolResponse::from_tool_call(
                        call,
                        output.data.to_string(),
                    )));
                ExecutedTool {
                    result: ToolCallResult::ok(
                        tool_name.clone(),
                        args.clone(),
                        output.data.clone(),
                    ),
                    media: output.media,
                }
            }
            Err(e) => {
                self.messages
                    .push(ChatMessage::from(ToolResponse::from_tool_call(
                        call,
                        format!("Error: {e}"),
                    )));
                ExecutedTool {
                    result: ToolCallResult::err(tool_name.clone(), args.clone(), format!("{e}")),
                    media: Vec::new(),
                }
            }
        }
    }
}

struct StepRun {
    step: Step,
    stop: bool,
    retry: Option<ModelRetryReason>,
    /// Whether one call returned what it already did twice in a row.
    stalled: bool,
}

struct ExecutedTool {
    result: ToolCallResult,
    media: Vec<ToolMedia>,
}

/// The user-content message that carries tool media. genai's OpenAI and Anthropic tool roles do not
/// accept image parts, so pixels travel as a user message beside the text-only tool result.
fn image_message(call: &ToolCall, media: Vec<ToolMedia>) -> ChatMessage {
    let focus = media.iter().find_map(|media| match media {
        ToolMedia::Image { focus, .. } => focus
            .as_deref()
            .map(str::trim)
            .filter(|focus| !focus.is_empty()),
    });
    let mut text = format!("Image from {}({})", call.fn_name, call.fn_arguments);
    if let Some(focus) = focus {
        text.push_str("\nFocus: ");
        text.push_str(focus);
    }
    let mut parts = vec![ContentPart::from_text(text)];
    for media in media {
        let ToolMedia::Image { mime, data, .. } = media;
        parts.push(ContentPart::from_binary_base64(
            mime,
            BASE64_STANDARD.encode(data),
            None,
        ));
    }
    ChatMessage::user(MessageContent::from_parts(parts))
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
            Tool::new(t.name())
                .with_description(t.description())
                .with_schema(schema::normalize_root(t.name(), t.schema()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::{prompt::role::AgentRole, test_support::StubTool};

    struct Noop;

    impl StepObserver for Noop {
        fn observed(&mut self, _: StepEvent<'_>) {}
    }

    #[test]
    fn an_image_message_stays_in_the_messages_and_carries_no_text_base64() {
        let call = ToolCall {
            call_id: "1".into(),
            fn_name: "read".into(),
            fn_arguments: json!({ "path": "attachment://a" }),
            thought_signatures: None,
        };
        let image = image_message(
            &call,
            vec![ToolMedia::Image {
                mime: "image/png".into(),
                data: vec![1, 2, 3],
                focus: Some("what is this".into()),
            }],
        );
        let parts: Vec<ContentPart> = image.content.clone().into_iter().collect();
        assert!(
            parts
                .iter()
                .any(|part| matches!(part, ContentPart::Binary(_))),
            "the image is a binary part"
        );
        assert!(parts.iter().all(|part| !matches!(
            part,
            ContentPart::Text(text) if text.contains("AQID") || text.contains("base64")
        )));

        let mut step_loop = StepLoop::new(
            crate::agent::test_support::genai_client(),
            "model".into(),
            ToolCallContext::default(),
            Messages::new(vec![ChatMessage::user("start")]),
            RoleSpec::stub(AgentRole::Default),
            StepConfig {
                budget: 3,
                steering: None,
            },
            Noop,
        );
        step_loop
            .messages
            .push(ChatMessage::from(ToolResponse::from_tool_call(
                &call,
                "{\"ok\":true}",
            )));
        step_loop.messages.push(image);

        let messages = step_loop.into_messages().to_vec();
        assert_eq!(messages.len(), 3, "start, tool result, image");
        assert!(
            messages.iter().any(|message| message
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::Binary(_)))),
            "the image remains message content"
        );
    }

    #[tokio::test]
    async fn a_tool_image_is_appended_beside_its_result_and_kept() {
        let call = ToolCall {
            call_id: "1".into(),
            fn_name: "read".into(),
            fn_arguments: json!({ "path": "attachment://a" }),
            thought_signatures: None,
        };
        let tool = StubTool::new("read")
            .media(ToolMedia::Image {
                mime: "image/png".into(),
                data: vec![1, 2, 3],
                focus: None,
            })
            .into_dyn();
        let mut step_loop = StepLoop::new(
            crate::agent::test_support::genai_client(),
            "model".into(),
            ToolCallContext::default(),
            Messages::new(vec![ChatMessage::user("start")]),
            RoleSpec::stub(AgentRole::Default),
            StepConfig {
                budget: 3,
                steering: None,
            },
            Noop,
        );

        let (results, stalled) = step_loop
            .run_tool_calls(&[call], &[tool], StepNumber::from(1))
            .await;
        assert!(!stalled);
        assert_eq!(results.len(), 1);
        assert_eq!(step_loop.messages.len(), 3, "start, tool result, image");

        let messages = step_loop.into_messages().to_vec();
        assert_eq!(messages.len(), 3, "start, tool result, image");
        assert!(
            messages.iter().any(|message| message
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::Binary(_)))),
            "the image is kept"
        );
    }

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

    /// A provider validates tool parameters as an object, so a tool that takes no arguments still
    /// goes out with an empty object schema rather than `null`.
    #[test]
    fn a_tool_without_arguments_carries_an_object_schema() {
        let skip: Arc<dyn AgentTool> = Arc::new(crate::agent::tools::skip::Skip);
        let tools = prepare_genai_tools(&[skip]);
        assert_eq!(
            tools[0].schema,
            Some(json!({ "type": "object", "properties": {} }))
        );
    }
}
