//! Retrieval: the entry point that runs the legs, the two ranking policies over its candidates and
//! the join back to the entities.

pub(crate) mod fuse;
pub(crate) mod hydrate;
pub(crate) mod rank;
pub mod recall;

pub(crate) use rank::ScorerErrorPolicy;
pub use recall::{RecallItem, RecallQuery, RecallScope, recall};
