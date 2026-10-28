use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// The one prompt-only content level a Conversation runs with; there is no output moderation.
/// `Unrestricted` adds no restriction of any form and tells the model not to refuse; a provider's
/// or platform's own block remains outside the application.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ContentPolicy {
    #[default]
    Safe,
    Standard,
    Unrestricted,
}

impl ContentPolicy {
    pub const ALL: [Self; 3] = [Self::Safe, Self::Standard, Self::Unrestricted];

    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// `Safe` < `Standard` < `Unrestricted`. A stricter change reopens the chapter through the
    /// existing wrap-up; a loosening one does not.
    pub fn is_stricter_than(self, other: Self) -> bool {
        self < other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_spells_and_parses_itself() {
        assert_eq!(
            ContentPolicy::ALL.map(ContentPolicy::as_str),
            ["safe", "standard", "unrestricted"]
        );
        for level in ContentPolicy::ALL {
            assert_eq!(level.as_str().parse::<ContentPolicy>(), Ok(level));
            assert_eq!(
                serde_json::to_value(level).expect("serializes"),
                serde_json::json!(level.as_str())
            );
        }
        assert!("anything".parse::<ContentPolicy>().is_err());
        assert!("".parse::<ContentPolicy>().is_err());
    }

    #[test]
    fn only_a_tighter_level_counts_as_stricter() {
        assert!(ContentPolicy::Safe.is_stricter_than(ContentPolicy::Standard));
        assert!(ContentPolicy::Standard.is_stricter_than(ContentPolicy::Unrestricted));
        assert!(!ContentPolicy::Unrestricted.is_stricter_than(ContentPolicy::Safe));
        assert!(!ContentPolicy::Standard.is_stricter_than(ContentPolicy::Standard));
    }
}
