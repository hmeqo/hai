use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use uuid::Uuid;

use super::{AgentCommand, ChatId, HostAction, MessageId, PurgeSpec, ScheduledTaskId};

/// One change to a chat, in the order it happened. The content is not copied here: the message row
/// holds the current value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEntry {
    pub id: Uuid,
    /// The Chat this change arrived through. A Conversation may hold several; a Turn selects one.
    pub chat_id: ChatId,
    /// When the change happened: a created message carries the platform's send time, an edited one
    /// its edit time, a command or a scheduled task the moment it was enqueued.
    pub at: jiff::Timestamp,
    pub audience: EntryAudience,
    pub kind: EntryKind,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EntryAudience {
    Agent,
    Bystanders,
    /// A change the host made on its own: context for the next Turn, never a reason to start one.
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryKind {
    MessageCreated {
        message_id: MessageId,
    },
    MessageEdited {
        message_id: MessageId,
        edited_at: jiff::Timestamp,
    },
    Command(AgentCommand),
    /// The content policy became stricter; the chapter reopens through the existing wrap-up.
    PolicyChanged,
    /// A host command changed a Conversation setting. The agent reads it with the next Turn: it is
    /// context, not an instruction and not a reason to start one.
    HostAction(HostAction),
    Scheduled {
        task_id: Option<ScheduledTaskId>,
        description: String,
    },
    /// A confirmed purge: not a change to read, an instruction this chat carries out on its own,
    /// outside a Turn.
    Purge(PurgeSpec),
}

impl ChatEntry {
    pub fn new(
        chat_id: ChatId,
        audience: EntryAudience,
        kind: EntryKind,
        at: jiff::Timestamp,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            chat_id,
            at,
            audience,
            kind,
        }
    }

    pub fn message_id(&self) -> Option<MessageId> {
        match &self.kind {
            EntryKind::MessageCreated { message_id }
            | EntryKind::MessageEdited { message_id, .. } => Some(*message_id),
            EntryKind::Command(_)
            | EntryKind::PolicyChanged
            | EntryKind::HostAction(_)
            | EntryKind::Scheduled { .. }
            | EntryKind::Purge(_) => None,
        }
    }

    /// The setting change this entry records, if it records one.
    pub fn host_action(&self) -> Option<&HostAction> {
        match &self.kind {
            EntryKind::HostAction(action) => Some(action),
            EntryKind::MessageCreated { .. }
            | EntryKind::MessageEdited { .. }
            | EntryKind::Command(_)
            | EntryKind::PolicyChanged
            | EntryKind::Scheduled { .. }
            | EntryKind::Purge(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_round_trips_through_its_wire_form() {
        let at = jiff::Timestamp::from_second(1_700_000_000).expect("timestamp");
        for kind in [
            EntryKind::MessageCreated {
                message_id: MessageId(7),
            },
            EntryKind::MessageEdited {
                message_id: MessageId(7),
                edited_at: at,
            },
            EntryKind::Command(AgentCommand::Explain(None)),
            EntryKind::HostAction(HostAction::PersonaChanged),
            EntryKind::Scheduled {
                task_id: Some(ScheduledTaskId(Uuid::from_u128(9))),
                description: "take the medication".into(),
            },
            EntryKind::Purge(PurgeSpec {
                range: crate::domain::vo::Stretch::Last { secs: 86_400 },
                surfaces: crate::domain::vo::PurgeSurfaces::all(),
            }),
        ] {
            let entry = ChatEntry::new(ChatId(1), EntryAudience::Agent, kind.clone(), at);
            let wire = serde_json::to_value(&entry).expect("serializable");
            let back: ChatEntry = serde_json::from_value(wire).expect("deserializable");
            assert_eq!(back.chat_id, ChatId(1));
            assert_eq!(back, entry);
            assert_eq!(back.kind, kind);
        }
    }
}
