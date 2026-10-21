use crate::{agent::process::AgentRuntime, agentcore::Language};

/// The interface block a digest reads instead of `SYSTEM_PROMPT`.
pub(crate) const CONTRACT: &str = r#"## The interface

You are writing a report, not speaking to anyone in the moment. Nothing you call sends anything: the tools you were given only read.

**Your response text is the report**, written in the shape the situation section asks for.
"#;

pub(crate) fn system_prompt(runtime: &AgentRuntime, language: Language) -> String {
    super::default::assemble(
        runtime,
        super::default::Speaking::Artifact,
        language,
        CONTRACT,
    )
}
