mod prompt;

pub use self::prompt::SystemPromptBuilder;
use crate::{
    agent::runtime::{AgentEngine, react::ReactLoopConfig},
    domain::model::ChatType,
};

pub fn build_react_config(engine: &AgentEngine, chat_type: ChatType) -> ReactLoopConfig {
    let cfg = &engine.app.cfg.agent;
    let system_prompt = SystemPromptBuilder::new()
        .personality(cfg)
        .system_prompt(cfg)
        .chat_type(cfg, chat_type)
        .skills(&engine.skill_manager)
        .build();
    ReactLoopConfig {
        system_prompt,
        options: ReactLoopConfig::build_chat_options(cfg),
    }
}
