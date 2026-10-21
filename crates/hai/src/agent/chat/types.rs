//! What a chat's agent carries while it works: the request that started a pass, and the outcome it
//! reports back.

use crate::{
    agent::command::{digest::run::DigestReport, purge::PurgeReport},
    agentcore::Language,
    domain::vo::{SessionRef, Stretch},
};

/// One digest a person confirmed: the stretch to summarize, the language its report is written in
/// when one was named, and the reply its report comes back over. Where the report goes is settled by
/// the dialog that asked; nothing about it is stored, so a restart drops it.
pub(crate) struct DigestRequest {
    pub stretch: Stretch,
    pub language: Option<Language>,
    pub reply: tokio::sync::oneshot::Sender<Result<DigestReport, String>>,
}

/// The completion signal of a Busy state: the outcome of a Turn or a steering interruption, a
/// wrap-up, a purge, a digest, or the process stopping.
pub(crate) enum BusySignal {
    Turn(crate::agent::run::TurnOutput),
    /// Ended early because something new arrived mid-turn (steering): everything already
    /// processed takes effect, and the arrival starts the next turn right away.
    Steered(crate::agent::run::TurnOutput),
    WrapUp {
        summary: String,
        context_tokens: u32,
        session: SessionRef,
    },
    TurnFailed,
    WrapUpFailed {
        error: String,
        session: SessionRef,
    },
    /// A purge ended. `Err` is the failure's own text: the transaction rolled back, so none of the
    /// agent surface was deleted — while the platform and workspace surfaces may have been partly
    /// done, which is what a best-effort surface means.
    Purged(Result<PurgeReport, String>),
    /// A digest ended: the report it wrote, or the failure's own words. Nothing was written either
    /// way.
    Digested(Result<DigestReport, String>),
    /// Cut short because the process is stopping: nothing is consumed, so the next start reads the
    /// same accumulation again.
    Cancelled,
}
