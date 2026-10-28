//! One ConversationAgent pass at a time: what kind of work holds the agent Busy, the receiver its
//! outcome arrives on, and the values that pass alone owns.

mod digest;
mod purge;
mod turn;
mod wrap_up;

use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

use crate::{
    agent::{
        command::{digest::run::DigestReport, purge::PurgeReport},
        run::TurnOutput,
    },
    config::conversation::Language,
    domain::vo::{ChatId, SessionRef, Stretch},
};

/// What the work behind a Busy state is: a Turn, the chapter wrap-up, a purge, or a digest. A
/// status query and the state's own round trip both carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BusyPass {
    Turn,
    WrapUp,
    Purge,
    Digest,
}

/// One digest a person confirmed: the stretch to summarize, the language its report is written in
/// when one was named, and the reply its report comes back over. Where the report goes is settled by
/// the dialog that asked; nothing about it is stored, so a restart drops it.
pub(crate) struct DigestRequest {
    pub chat_id: ChatId,
    pub stretch: Stretch,
    pub language: Option<Language>,
    pub reply: oneshot::Sender<Result<DigestReport, String>>,
}

/// The completion signal of a Busy state: the outcome of a Turn or a steering interruption, a
/// wrap-up, a purge, a digest, or the process stopping.
pub(crate) enum BusySignal {
    Turn(TurnOutput),
    /// Ended early because something new arrived mid-turn (steering): everything already
    /// processed takes effect, and the arrival starts the next turn right away.
    Steered(TurnOutput),
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

/// What one Busy pass owns beyond its result receiver. The variants are mutually exclusive, so a
/// page reply cannot outlive the digest it belongs to and a purge entry cannot be read by a Turn.
enum BusyWork {
    Turn,
    WrapUp {
        consumed: Vec<Uuid>,
    },
    Purge {
        entry: Uuid,
    },
    Digest {
        reply: Option<oneshot::Sender<Result<DigestReport, String>>>,
    },
}

impl BusyWork {
    fn pass(&self) -> BusyPass {
        match self {
            Self::Turn => BusyPass::Turn,
            Self::WrapUp { .. } => BusyPass::WrapUp,
            Self::Purge { .. } => BusyPass::Purge,
            Self::Digest { .. } => BusyPass::Digest,
        }
    }
}

/// One running pass: the receiver its outcome arrives on, when it started, and the values only that
/// pass owns.
pub(super) struct BusyRun {
    pub(super) result_rx: oneshot::Receiver<BusySignal>,
    started_at: Instant,
    work: BusyWork,
}

impl BusyRun {
    pub(super) fn turn(result_rx: oneshot::Receiver<BusySignal>) -> Self {
        Self {
            result_rx,
            started_at: Instant::now(),
            work: BusyWork::Turn,
        }
    }

    pub(super) fn wrap_up(result_rx: oneshot::Receiver<BusySignal>, consumed: Vec<Uuid>) -> Self {
        Self {
            result_rx,
            started_at: Instant::now(),
            work: BusyWork::WrapUp { consumed },
        }
    }

    pub(super) fn purge(result_rx: oneshot::Receiver<BusySignal>, entry: Uuid) -> Self {
        Self {
            result_rx,
            started_at: Instant::now(),
            work: BusyWork::Purge { entry },
        }
    }

    pub(super) fn digest(
        result_rx: oneshot::Receiver<BusySignal>,
        reply: oneshot::Sender<Result<DigestReport, String>>,
    ) -> Self {
        Self {
            result_rx,
            started_at: Instant::now(),
            work: BusyWork::Digest { reply: Some(reply) },
        }
    }

    pub(super) fn take_digest_reply(
        &mut self,
    ) -> Option<oneshot::Sender<Result<DigestReport, String>>> {
        match &mut self.work {
            BusyWork::Digest { reply } => reply.take(),
            _ => None,
        }
    }

    pub(super) fn take_purge_entry(&self) -> Option<Uuid> {
        match self.work {
            BusyWork::Purge { entry } => Some(entry),
            _ => None,
        }
    }

    pub(super) fn take_wrap_up_consumed(&mut self) -> Vec<Uuid> {
        match &mut self.work {
            BusyWork::WrapUp { consumed } => std::mem::take(consumed),
            _ => Vec::new(),
        }
    }

    pub(super) fn pass(&self) -> BusyPass {
        self.work.pass()
    }

    pub(super) fn elapsed_secs(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }
}

/// The agent's Busy slot. Taking it leaves Idle; restoring it returns the same owned pass.
pub(super) enum PassSlot {
    Idle,
    Busy(BusyRun),
}

impl PassSlot {
    pub(super) fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }

    pub(super) fn begin(&mut self, run: BusyRun) {
        *self = Self::Busy(run);
    }

    pub(super) fn reset(&mut self) {
        *self = Self::Idle;
    }

    #[cfg(test)]
    pub(super) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub(super) fn status(&self) -> (Option<f64>, Option<BusyPass>) {
        match self {
            Self::Idle => (None, None),
            Self::Busy(run) => (Some(run.elapsed_secs()), Some(run.pass())),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::{BusyPass, BusyRun, PassSlot};

    #[test]
    fn taking_the_busy_slot_leaves_it_idle() {
        let (_tx, rx) = oneshot::channel();
        let mut slot = PassSlot::Idle;
        slot.begin(BusyRun::turn(rx));

        match slot.take() {
            PassSlot::Busy(run) => {
                assert_eq!(run.pass(), BusyPass::Turn);
                assert!(slot.is_idle(), "taking the pass leaves Idle");
                slot.begin(run);
            }
            PassSlot::Idle => panic!("a started pass is Busy"),
        }
        assert_eq!(slot.status().1, Some(BusyPass::Turn));
    }

    #[tokio::test]
    async fn dropping_a_digest_pass_releases_its_reply() {
        let (_tx, rx) = oneshot::channel();
        let (reply, report) = oneshot::channel();
        let mut slot = PassSlot::Idle;
        slot.begin(BusyRun::digest(rx, reply));

        drop(slot);

        assert!(
            report.await.is_err(),
            "the page channel closed with the pass"
        );
    }
}
