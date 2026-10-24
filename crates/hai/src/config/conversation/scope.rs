use serde::{Deserialize, Serialize};
use strum::EnumString;

/// How a channel places a newly created Chat in a Conversation: one shared Conversation for every
/// Chat, or one Conversation per Chat. The chat type is not part of the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ConversationScope {
    Shared,
    Isolated,
}
