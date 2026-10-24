pub(crate) mod prompt;

use std::sync::Arc;

use genai::chat::ChatMessage;

use crate::{
    agent::{
        process::AgentRuntime,
        prompt::role::{StepDecision, StepFacts},
        run::turn::ToolContext,
        tools::{knowledge, memory, read, recall, schedule, send, shell, skip, topic, wait, web},
    },
    agentcore::tool::AgentTool,
    config::conversation::{ContentPolicy, Language},
    domain::{
        model::ChatType,
        vo::{ModelRetryReason, StepNumber},
    },
};

pub(crate) fn system_prompt(
    runtime: &AgentRuntime,
    chat_type: ChatType,
    language: Language,
    persona: Option<&str>,
    content_policy: ContentPolicy,
) -> String {
    assemble(
        runtime,
        Speaking::Chat(chat_type),
        language,
        prompt::SYSTEM_PROMPT,
        persona,
        content_policy,
    )
}

/// Whether a role speaks in the chat. A speaking role reads the conversation's soul, its agency layer
/// and its chat-type rules; a pass whose output is the response text reads none of them, because its
/// task supplies what it writes.
pub(super) enum Speaking {
    /// The role speaks in this kind of chat.
    Chat(ChatType),
    /// The role's output is the response text; it has no conversational layers.
    Artifact,
}

/// Assembles one role's system prompt: the contract, the conversational layers its role carries, and
/// the environment it runs in.
pub(super) fn assemble(
    runtime: &AgentRuntime,
    speaking: Speaking,
    language: Language,
    contract: &str,
    persona: Option<&str>,
    content_policy: ContentPolicy,
) -> String {
    let cfg = &runtime.cfg().agent;
    let prompts = &runtime.prompts;
    let mut builder = prompt::SystemPromptBuilder::new()
        .contract(contract)
        .content_policy(content_policy);
    if let Speaking::Chat(chat_type) = speaking {
        builder = builder.personality(&cfg.personality.name, prompts.soul(language));
        if let Some(persona) = persona {
            builder = builder.persona(persona);
        }
        builder = builder.agents(prompts.agents(language)).chat_type(
            prompts.group(language),
            prompts.private(language),
            chat_type,
        );
    }
    builder
        .environment(crate::agent::prompt::layers::environment(
            runtime.cfg().sandbox.enabled,
        ))
        .language(language)
        .skills(&runtime.skill_manager)
        .build()
}

pub(crate) fn tools(runtime: &AgentRuntime, ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let mut all = Vec::new();
    all.extend(skip::tools(ctx));
    all.extend(wait::tools(ctx));
    all.extend(send::tools(ctx));
    all.extend(topic::tools(ctx));
    all.extend(schedule::tools(ctx));
    all.extend(memory::tools(ctx));
    all.extend(recall::tools(ctx));
    all.extend(knowledge::tools(runtime, ctx));
    all.extend(read::tools(runtime, ctx));
    all.extend(web::tools(runtime));
    all.extend(shell::tools(ctx));
    all
}

const MAX_CONTRACT_RETRIES: u32 = 1;

pub(super) fn decide(facts: StepFacts<'_>, violations: &mut u32) -> StepDecision {
    // Only a run that has not spoken can have written its reply into the response text; after a
    // successful send, text with no call is a closing remark and the turn is already complete.
    let violating = !facts.spoken && !facts.text.trim().is_empty() && facts.non_skip_calls == 0;
    *violations = if violating { *violations + 1 } else { 0 };
    if violating && *violations <= MAX_CONTRACT_RETRIES {
        return StepDecision::Retry {
            reason: ModelRetryReason::ResponseWithText,
            guide: speaking_contract_guide(facts.step, facts.text),
        };
    }
    if facts.skipped || facts.non_skip_calls == 0 {
        return StepDecision::Stop;
    }
    StepDecision::Continue
}

fn speaking_contract_guide(step: StepNumber, text: &str) -> ChatMessage {
    ChatMessage::user(format!(
        "[speaking contract] Step {} wrote text that is not delivered to the chat: {text:?}. If that was meant for the chat, send it with send_message; otherwise call skip to end the turn.",
        step.0
    ))
}

#[cfg(test)]
mod tests {
    use super::{StepDecision, StepFacts, decide};
    use crate::domain::vo::StepNumber;

    fn facts(text_empty: bool, skipped: bool, non_skip_calls: usize) -> StepFacts<'static> {
        StepFacts {
            step: StepNumber::from(1),
            text: if text_empty { "" } else { "reply" },
            calls_empty: non_skip_calls == 0 && !skipped,
            skipped,
            non_skip_calls,
            spoken: false,
        }
    }

    fn after_speaking(facts: StepFacts<'static>) -> StepFacts<'static> {
        StepFacts {
            spoken: true,
            ..facts
        }
    }

    #[test]
    fn skip_with_empty_text_stops() {
        let mut violations = 0;
        assert!(matches!(
            decide(facts(true, true, 0), &mut violations),
            StepDecision::Stop
        ));
        assert_eq!(violations, 0);
    }

    #[test]
    fn skip_with_text_retries_then_stops() {
        let mut violations = 0;
        assert!(matches!(
            decide(facts(false, true, 0), &mut violations),
            StepDecision::Retry { .. }
        ));
        assert_eq!(violations, 1);
        assert!(matches!(
            decide(facts(false, true, 0), &mut violations),
            StepDecision::Stop
        ));
        assert_eq!(violations, 2);
    }

    #[test]
    fn text_only_retries_then_stops() {
        let mut violations = 0;
        assert!(matches!(
            decide(facts(false, false, 0), &mut violations),
            StepDecision::Retry { .. }
        ));
        assert_eq!(violations, 1);
        assert!(matches!(
            decide(facts(false, false, 0), &mut violations),
            StepDecision::Stop
        ));
        assert_eq!(violations, 2);
    }

    /// A Step that worked without speaking keeps the Turn going, and resets the strike count.
    #[test]
    fn a_working_call_with_text_continues_and_resets_strikes() {
        let mut violations = 1;
        assert!(matches!(
            decide(facts(false, false, 1), &mut violations),
            StepDecision::Continue
        ));
        assert_eq!(violations, 0);
    }

    /// Once the run has spoken, text with no call is a closing remark, not an unsent reply; a retry
    /// would make the model send its previous message again.
    #[test]
    fn text_after_speaking_stops_without_a_retry() {
        let mut violations = 0;
        assert!(matches!(
            decide(after_speaking(facts(false, false, 0)), &mut violations),
            StepDecision::Stop
        ));
        assert_eq!(violations, 0);
    }

    #[test]
    fn empty_text_without_calls_stops() {
        let mut violations = 0;
        assert!(matches!(
            decide(facts(true, false, 0), &mut violations),
            StepDecision::Stop
        ));
        assert_eq!(violations, 0);
    }
}
