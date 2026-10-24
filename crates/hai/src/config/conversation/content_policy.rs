use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// How mature the material the agent may engage with can be. Prompt policy only; there is no
/// output moderation.
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
pub enum MaturityLevel {
    #[default]
    Safe,
    Standard,
    Mature,
}

/// Whether the prompt restricts political stances. `Unrestricted` removes that restriction; it
/// does not override a provider's or platform's rules.
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
pub enum PoliticalRestriction {
    #[default]
    Restricted,
    Unrestricted,
}

impl MaturityLevel {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

impl PoliticalRestriction {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// The named presets `/content-policy` and its picker offer. A preset is only a spelling for a
/// `ContentPolicy`; the axes are what the prompt reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentPolicyPreset {
    Safe,
    Standard,
    Mature,
    Unrestricted,
}

impl ContentPolicyPreset {
    pub const ALL: [Self; 4] = [Self::Safe, Self::Standard, Self::Mature, Self::Unrestricted];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Standard => "standard",
            Self::Mature => "mature",
            Self::Unrestricted => "unrestricted",
        }
    }

    pub const fn policy(self) -> ContentPolicy {
        match self {
            Self::Safe => ContentPolicy::SAFE,
            Self::Standard => ContentPolicy::STANDARD,
            Self::Mature => ContentPolicy::MATURE,
            Self::Unrestricted => ContentPolicy::UNRESTRICTED,
        }
    }
}

impl std::str::FromStr for ContentPolicyPreset {
    type Err = &'static str;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value)
            .ok_or("unknown content policy preset")
    }
}

/// The two prompt-only axes a Conversation runs with. `UNRESTRICTED` is the one preset that adds
/// no restriction of its own; providers and platforms still enforce theirs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ContentPolicy {
    pub maturity: MaturityLevel,
    pub politics: PoliticalRestriction,
}

impl ContentPolicy {
    pub const PRESETS: [&'static str; 4] = [
        ContentPolicyPreset::Safe.as_str(),
        ContentPolicyPreset::Standard.as_str(),
        ContentPolicyPreset::Mature.as_str(),
        ContentPolicyPreset::Unrestricted.as_str(),
    ];

    pub const SAFE: Self = Self {
        maturity: MaturityLevel::Safe,
        politics: PoliticalRestriction::Restricted,
    };
    pub const STANDARD: Self = Self {
        maturity: MaturityLevel::Standard,
        politics: PoliticalRestriction::Restricted,
    };
    pub const MATURE: Self = Self {
        maturity: MaturityLevel::Mature,
        politics: PoliticalRestriction::Restricted,
    };
    pub const UNRESTRICTED: Self = Self {
        maturity: MaturityLevel::Mature,
        politics: PoliticalRestriction::Unrestricted,
    };

    /// Whether this is the unrestricted preset; providers and platforms still enforce their own
    /// rules.
    pub fn is_unrestricted(self) -> bool {
        self == Self::UNRESTRICTED
    }

    pub fn preset(name: &str) -> Option<Self> {
        name.parse::<ContentPolicyPreset>()
            .ok()
            .map(ContentPolicyPreset::policy)
    }

    /// Whether this policy is stricter than `other` on either axis. A stricter policy reopens the
    /// chapter through the existing wrap-up; a loosening one does not.
    pub fn is_stricter_than(self, other: Self) -> bool {
        self.maturity < other.maturity || self.politics < other.politics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_name_their_two_axes() {
        assert_eq!(ContentPolicy::preset("safe"), Some(ContentPolicy::SAFE));
        assert_eq!(
            ContentPolicy::preset("standard"),
            Some(ContentPolicy::STANDARD)
        );
        assert_eq!(ContentPolicy::preset("mature"), Some(ContentPolicy::MATURE));
        assert_eq!(
            ContentPolicy::preset("unrestricted"),
            Some(ContentPolicy::UNRESTRICTED)
        );
        assert_eq!(ContentPolicy::preset("anything"), None);
    }

    /// A typed preset is the only spelling the command layer parses, and each one names exactly the
    /// axes the preset list holds.
    #[test]
    fn every_typed_preset_spells_and_resolves_itself() {
        assert_eq!(
            ContentPolicyPreset::ALL.map(ContentPolicyPreset::as_str),
            ContentPolicy::PRESETS
        );
        for preset in ContentPolicyPreset::ALL {
            assert_eq!(preset.as_str().parse::<ContentPolicyPreset>(), Ok(preset));
            assert_eq!(
                ContentPolicy::preset(preset.as_str()),
                Some(preset.policy())
            );
        }
        assert_eq!(
            ContentPolicyPreset::Unrestricted.policy(),
            ContentPolicy::UNRESTRICTED
        );
        assert!("Anything".parse::<ContentPolicyPreset>().is_err());
        assert!("".parse::<ContentPolicyPreset>().is_err());
    }

    #[test]
    fn only_a_tightened_axis_counts_as_stricter() {
        assert!(ContentPolicy::SAFE.is_stricter_than(ContentPolicy::MATURE));
        assert!(ContentPolicy::MATURE.is_stricter_than(ContentPolicy::UNRESTRICTED));
        assert!(!ContentPolicy::MATURE.is_stricter_than(ContentPolicy::SAFE));
        assert!(!ContentPolicy::MATURE.is_stricter_than(ContentPolicy::MATURE));
    }
}
