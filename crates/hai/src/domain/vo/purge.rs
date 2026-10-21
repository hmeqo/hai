//! What one purge covers: the stretch of time it wipes and which of the four surfaces it touches.

use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoStaticStr};

use super::Stretch;

/// One surface a purge may touch; the dialog toggles it and the command grammar spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr, Serialize, Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PurgeSurface {
    Agent,
    Files,
    Telegram,
    Derived,
}

impl PurgeSurface {
    /// The order the grammar and the dialog walk.
    pub const ALL: [Self; 4] = [Self::Agent, Self::Files, Self::Telegram, Self::Derived];

    /// The wire spelling: `agent` / `files` / `telegram` / `derived`, written once by `strum`.
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// The four independent switches one purge carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeSurfaces {
    /// Message and attachment rows, memory, topics with their notes, scheduled tasks created in the
    /// window, and the conversation snapshot (reset whole) with the changes it consumed.
    pub agent: bool,
    /// The chat workspace's files whose modification time falls in the window.
    pub files: bool,
    /// The messages themselves, on the platform.
    pub telegram: bool,
    /// Event rows, perception transcripts, and the cached bytes of the window's attachments.
    pub derived: bool,
}

impl PurgeSurfaces {
    pub fn all() -> Self {
        Self {
            agent: true,
            files: true,
            telegram: true,
            derived: true,
        }
    }

    /// Nothing selected: a state a person can reach by switching everything off, and what the
    /// grammar builds a list up from.
    pub fn none() -> Self {
        Self {
            agent: false,
            files: false,
            telegram: false,
            derived: false,
        }
    }

    /// The same switches with one surface turned on.
    pub fn with(self, surface: PurgeSurface) -> Self {
        match surface {
            PurgeSurface::Agent => Self {
                agent: true,
                ..self
            },
            PurgeSurface::Files => Self {
                files: true,
                ..self
            },
            PurgeSurface::Telegram => Self {
                telegram: true,
                ..self
            },
            PurgeSurface::Derived => Self {
                derived: true,
                ..self
            },
        }
    }

    pub fn get(self, surface: PurgeSurface) -> bool {
        match surface {
            PurgeSurface::Agent => self.agent,
            PurgeSurface::Files => self.files,
            PurgeSurface::Telegram => self.telegram,
            PurgeSurface::Derived => self.derived,
        }
    }

    pub fn toggled(self, surface: PurgeSurface) -> Self {
        match surface {
            PurgeSurface::Agent => Self {
                agent: !self.agent,
                ..self
            },
            PurgeSurface::Files => Self {
                files: !self.files,
                ..self
            },
            PurgeSurface::Telegram => Self {
                telegram: !self.telegram,
                ..self
            },
            PurgeSurface::Derived => Self {
                derived: !self.derived,
                ..self
            },
        }
    }
}

/// Everything one purge does: a range and the surfaces it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeSpec {
    pub range: Stretch,
    pub surfaces: PurgeSurfaces,
}

/// What one purge removed, counted per item; the receipt and the audit event share this one value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeCounts {
    pub messages: u64,
    pub memories: u64,
    pub topics: u64,
    pub scheduled: u64,
    pub events: u64,
    pub perceptions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_one_surface_leaves_the_others_alone() {
        let surfaces = PurgeSurfaces::all();

        let off = surfaces.toggled(PurgeSurface::Telegram);
        assert!(!off.telegram);
        for surface in [
            PurgeSurface::Agent,
            PurgeSurface::Files,
            PurgeSurface::Derived,
        ] {
            assert!(off.get(surface), "{surface:?} must not move");
        }

        let back = off.toggled(PurgeSurface::Telegram);
        assert_eq!(back, surfaces, "toggling twice is the identity");
    }

    /// Building a list one name at a time is what the grammar does; a name repeated is still one
    /// surface on.
    #[test]
    fn surfaces_are_built_up_from_the_names_that_were_given() {
        let named = PurgeSurfaces::none()
            .with(PurgeSurface::Agent)
            .with(PurgeSurface::Agent)
            .with(PurgeSurface::Derived);

        assert!(named.agent && named.derived);
        assert!(!named.files && !named.telegram);
        assert!(
            PurgeSurfaces::none()
                .with(PurgeSurface::Agent)
                .get(PurgeSurface::Agent)
        );
    }
}
