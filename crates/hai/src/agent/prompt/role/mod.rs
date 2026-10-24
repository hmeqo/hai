pub(crate) mod default;
pub(crate) mod digest;
pub(crate) mod wrap_up;

use std::sync::Arc;

use genai::chat::{ChatMessage, ChatOptions, ToolCall};

use crate::{
    agent::process::AgentRuntime,
    agentcore::tool::AgentTool,
    config::{
        conversation::{ContentPolicy, Language},
        schema::ChatConfig,
    },
    domain::{
        model::ChatType,
        vo::{ModelRetryReason, StepNumber},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRole {
    Default,
    WrapUp,
    Digest,
}

pub(crate) enum StepDecision {
    Continue,
    Stop,
    Retry {
        reason: ModelRetryReason,
        guide: ChatMessage,
    },
}

pub(crate) struct StepFacts<'a> {
    pub step: StepNumber,
    /// The Step's own text, as written; a speaking-contract guide quotes it back.
    pub text: &'a str,
    pub calls_empty: bool,
    pub skipped: bool,
    pub non_skip_calls: usize,
    /// Whether a speaking tool already succeeded earlier in this run. Text after that is not an
    /// unsent reply, so the speaking contract has nothing left to recover.
    pub spoken: bool,
}

impl<'a> StepFacts<'a> {
    pub(crate) fn from_calls(
        step: StepNumber,
        text: &'a str,
        calls: &[ToolCall],
        tools: &[Arc<dyn AgentTool>],
        spoken: bool,
    ) -> Self {
        let meta = |name: &str| tools.iter().find(|t| t.name() == name);
        let mut skipped = false;
        let mut non_skip_calls = 0;
        for call in calls {
            match meta(&call.fn_name) {
                Some(tool) if tool.ends_turn() => skipped = true,
                _ => non_skip_calls += 1,
            }
        }
        Self {
            step,
            text,
            calls_empty: calls.is_empty(),
            skipped,
            non_skip_calls,
            spoken,
        }
    }
}

pub(crate) struct RoleSpec {
    role: AgentRole,
    pub system_prompt: String,
    pub options: ChatOptions,
    pub steering: bool,
    contract_violations: u32,
}

impl RoleSpec {
    pub(crate) fn decide_step(&mut self, facts: StepFacts<'_>) -> StepDecision {
        match self.role {
            AgentRole::Default => default::decide(facts, &mut self.contract_violations),
            AgentRole::WrapUp | AgentRole::Digest => stop_when_no_calls(facts),
        }
    }

    /// Whether this role's run is one the chat is waiting on: a Turn is the agent answering the
    /// chat, a wrap-up is bookkeeping the chat should not see.
    pub(crate) fn shows_typing(&self) -> bool {
        matches!(self.role, AgentRole::Default)
    }

    pub(crate) fn rejects(&self, tool: &dyn AgentTool) -> bool {
        match self.role {
            AgentRole::Default => false,
            // A wrap-up is bookkeeping the chat should not see: it must not speak, end the turn, or
            // start a model call of its own.
            AgentRole::WrapUp => tool.speaks() || tool.ends_turn() || tool.produces(),
            // A digest leaves nothing behind, so it may only read.
            AgentRole::Digest => !tool.reads_only(),
        }
    }
}

impl AgentRole {
    pub(crate) fn spec(
        self,
        runtime: &AgentRuntime,
        chat: &ChatConfig,
        chat_type: ChatType,
        language: Language,
        persona: Option<&str>,
        content_policy: ContentPolicy,
    ) -> RoleSpec {
        let cfg = &runtime.cfg().agent;
        match self {
            AgentRole::Default => RoleSpec {
                role: self,
                system_prompt: default::system_prompt(
                    runtime,
                    chat_type,
                    language,
                    persona,
                    content_policy,
                ),
                options: build_chat_options(chat),
                steering: cfg.context.steering,
                contract_violations: 0,
            },
            AgentRole::WrapUp => RoleSpec {
                role: self,
                system_prompt: default::system_prompt(
                    runtime,
                    chat_type,
                    language,
                    None,
                    content_policy,
                ),
                options: build_chat_options(chat),
                steering: false,
                contract_violations: 0,
            },
            AgentRole::Digest => RoleSpec {
                role: self,
                system_prompt: digest::system_prompt(runtime, language, content_policy),
                options: build_chat_options(chat),
                steering: false,
                contract_violations: 0,
            },
        }
    }
}

/// A pass whose work is finished when the model stops calling tools: its summary or report is
/// already the response text.
pub(super) fn stop_when_no_calls(facts: StepFacts<'_>) -> StepDecision {
    if facts.calls_empty {
        StepDecision::Stop
    } else {
        StepDecision::Continue
    }
}

pub(crate) fn build_chat_options(cfg: &ChatConfig) -> ChatOptions {
    let mut opts = ChatOptions::default().with_temperature(cfg.temperature as f64);
    if let Some(maxt) = cfg.max_tokens {
        opts = opts.with_max_tokens(maxt);
    }
    if cfg.reasoning
        && let Some(effort) = cfg.reasoning_effort()
    {
        opts = opts.with_reasoning_effort(effort);
    }
    opts
}

#[cfg(test)]
mod tests {
    use genai::chat::{ChatOptions, ToolCall};

    use super::{AgentRole, RoleSpec, StepDecision, StepFacts, stop_when_no_calls};
    use crate::{agent::test_support::StubTool, domain::vo::StepNumber};

    /// Both passes end when the model asks for nothing more: their output is already written.
    #[test]
    fn a_pass_stops_when_it_stops_calling_tools() {
        let facts = |calls_empty| StepFacts {
            step: StepNumber::from(1),
            text: "report",
            calls_empty,
            skipped: false,
            non_skip_calls: 0,
            spoken: false,
        };

        assert!(matches!(
            stop_when_no_calls(facts(true)),
            StepDecision::Stop
        ));
        assert!(matches!(
            stop_when_no_calls(facts(false)),
            StepDecision::Continue
        ));
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            call_id: "1".into(),
            fn_name: name.into(),
            fn_arguments: serde_json::json!({}),
            thought_signatures: None,
        }
    }

    #[test]
    fn step_facts_come_from_tool_metadata() {
        let tools = vec![
            StubTool::new("skip").ends_turn(true).into_dyn(),
            StubTool::new("send_message").speaks(true).into_dyn(),
            StubTool::new("generate_image").produces(true).into_dyn(),
            StubTool::new("mcp_skip").into_dyn(),
        ];

        let skipped =
            StepFacts::from_calls(StepNumber::from(1), "hi", &[call("skip")], &tools, false);
        assert!(skipped.skipped);
        assert_eq!(skipped.non_skip_calls, 0);

        // Neither call skips the turn: each counts one non-skip call.
        for name in ["send_message", "generate_image"] {
            let facts =
                StepFacts::from_calls(StepNumber::from(1), "hi", &[call(name)], &tools, false);
            assert_eq!(facts.non_skip_calls, 1, "{name}");
        }

        // `ends_turn` comes from tool metadata only: a tool whose name looks like `skip`
        // but does not declare it keeps the turn going
        let lookalike =
            StepFacts::from_calls(StepNumber::from(1), "", &[call("mcp_skip")], &tools, false);
        assert!(!lookalike.skipped);
        assert_eq!(lookalike.non_skip_calls, 1);
    }

    #[test]
    fn a_wrap_up_rejects_what_speaks_ends_or_produces() {
        let spec = RoleSpec {
            role: AgentRole::WrapUp,
            system_prompt: String::new(),
            options: ChatOptions::default(),
            steering: false,
            contract_violations: 0,
        };

        assert!(spec.rejects(&*StubTool::new("send_message").speaks(true).into_dyn()));
        assert!(spec.rejects(&*StubTool::new("skip").ends_turn(true).into_dyn()));
        assert!(spec.rejects(&*StubTool::new("generate_image").produces(true).into_dyn()));
        assert!(!spec.rejects(&*StubTool::new("topic").into_dyn()));
    }

    /// A digest leaves nothing behind, so it keeps the tools that only read and nothing else.
    #[test]
    fn a_digest_rejects_everything_that_is_not_a_read() {
        let spec = RoleSpec {
            role: AgentRole::Digest,
            system_prompt: String::new(),
            options: ChatOptions::default(),
            steering: false,
            contract_violations: 0,
        };

        for name in ["recall", "read", "search_knowledge_base"] {
            assert!(
                !spec.rejects(&*StubTool::new(name).reads_only(true).into_dyn()),
                "{name} only reads"
            );
        }
        for name in ["send_message", "generate_image", "memory", "topic", "bash"] {
            assert!(spec.rejects(&*StubTool::new(name).into_dyn()), "{name}");
        }
        assert!(
            spec.rejects(&*StubTool::new("skip").ends_turn(true).into_dyn()),
            "reading is not the only way a tool is harmless"
        );
    }
}
