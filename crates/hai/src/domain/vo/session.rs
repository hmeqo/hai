//! The trace's top unit: which run an event belongs to.

use serde::{Deserialize, Serialize};
use strum::{EnumIter, EnumString, IntoStaticStr};

use super::SessionId;

/// What kind of run a session is. A `Main` session is one chapter; the other two are single passes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    IntoStaticStr,
    EnumString,
    EnumIter,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SessionKind {
    Main,
    WrapUp,
    Digest,
}

/// The trace's top unit: which run an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef {
    pub id: SessionId,
    pub kind: SessionKind,
    /// The chapter a wrap-up closes; absent for a main session and for a digest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<SessionId>,
}

impl SessionRef {
    pub fn main(id: SessionId) -> Self {
        Self {
            id,
            kind: SessionKind::Main,
            parent: None,
        }
    }

    pub fn wrap_up(id: SessionId, parent: SessionId) -> Self {
        Self {
            id,
            kind: SessionKind::WrapUp,
            parent: Some(parent),
        }
    }

    pub fn digest(id: SessionId) -> Self {
        Self {
            id,
            kind: SessionKind::Digest,
            parent: None,
        }
    }

    /// The first four hex digits: what a log header shows and `session:` matches.
    pub fn short(&self) -> String {
        self.id.0.to_string()[..4].to_owned()
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;
    use crate::test_support::assert_names_agree;

    /// The stored name of a kind is its serde tag and its strum name: the log filter and the JSON
    /// never drift apart.
    #[test]
    fn the_wire_names_agree() {
        assert_names_agree(SessionKind::iter(), |kind| (*kind).into());
    }

    #[test]
    fn only_a_wrap_up_names_a_parent() {
        let id = SessionId::new();
        assert_eq!(SessionRef::main(id).parent, None);
        assert_eq!(SessionRef::digest(id).parent, None);
        assert_eq!(SessionRef::wrap_up(id, id).parent, Some(id));
    }
}
