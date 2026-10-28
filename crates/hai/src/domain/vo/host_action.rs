use serde::{Deserialize, Serialize};

/// A setting a host-run command changed. The entry names the setting; the current value is read
/// from the Conversation's preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HostAction {
    PersonaChanged,
    /// Carried by stored entries and read as `<host_actions>`; no agent command writes it.
    ModelChanged,
    LanguageChanged,
    ContentPolicyChanged,
}

impl HostAction {
    /// The stable name the prompt uses for this action's element.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::PersonaChanged => "persona",
            Self::ModelChanged => "model",
            Self::LanguageChanged => "language",
            Self::ContentPolicyChanged => "content-policy",
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_host_action_round_trips_through_its_wire_form() {
        for action in [
            HostAction::PersonaChanged,
            HostAction::ModelChanged,
            HostAction::LanguageChanged,
            HostAction::ContentPolicyChanged,
        ] {
            let wire = serde_json::to_value(action).expect("serializable");
            let back: HostAction = serde_json::from_value(wire).expect("deserializable");
            assert_eq!(back, action);
        }
        assert_eq!(
            serde_json::to_value(HostAction::PersonaChanged).expect("serializable"),
            json!({ "action": "persona_changed" })
        );
    }

    #[test]
    fn every_action_has_a_prompt_kind() {
        assert_eq!(HostAction::PersonaChanged.kind(), "persona");
        assert_eq!(HostAction::ModelChanged.kind(), "model");
        assert_eq!(HostAction::LanguageChanged.kind(), "language");
        assert_eq!(HostAction::ContentPolicyChanged.kind(), "content-policy");
    }
}
