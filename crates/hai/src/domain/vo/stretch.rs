//! A stretch of time as a person names it.

use serde::{Deserialize, Serialize};

use super::TimeRange;

/// The stretch a command covers: everything, the time before now, or a fixed window. Both ends of
/// `Between` are inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stretch {
    All,
    Last {
        secs: i64,
    },
    Between {
        from: jiff::Timestamp,
        to: jiff::Timestamp,
    },
}

impl Stretch {
    /// The window this stretch stands for at `now`: `Last` starts there and is open above, `All` is
    /// open on both sides.
    pub fn resolve(&self, now: jiff::Timestamp) -> TimeRange {
        match *self {
            Self::All => TimeRange::new(None, None),
            Self::Last { secs } => {
                TimeRange::new(Some(now - jiff::SignedDuration::from_secs(secs)), None)
            }
            Self::Between { from, to } => TimeRange::new(Some(from), Some(to)),
        }
    }
}

/// The unit a span is named in: the largest one that divides it exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Days,
    Hours,
    Minutes,
    Seconds,
}

impl TimeUnit {
    /// The one-letter form an inspector reads.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Days => "d",
            Self::Hours => "h",
            Self::Minutes => "m",
            Self::Seconds => "s",
        }
    }
}

/// A span as a whole number of the largest unit that divides it exactly. Anything under a minute
/// reads in seconds, so a span is never rounded down to a zero count of a larger unit.
pub fn whole_units(secs: i64) -> (i64, TimeUnit) {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;

    let (size, unit) = match secs {
        secs if secs < MINUTE => (1, TimeUnit::Seconds),
        secs if secs % DAY == 0 => (DAY, TimeUnit::Days),
        secs if secs % HOUR == 0 => (HOUR, TimeUnit::Hours),
        secs if secs % MINUTE == 0 => (MINUTE, TimeUnit::Minutes),
        _ => (1, TimeUnit::Seconds),
    };
    (secs / size, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> jiff::Timestamp {
        jiff::Timestamp::from_second(secs).expect("a valid second")
    }

    #[test]
    fn a_stretch_resolves_to_its_window() {
        let now = at(1_700_000_000);

        assert_eq!(
            Stretch::All.resolve(now),
            TimeRange::new(None, None),
            "all time is open on both sides"
        );
        assert_eq!(
            Stretch::Last { secs: 86_400 }.resolve(now),
            TimeRange::new(Some(now - jiff::SignedDuration::from_secs(86_400)), None),
            "a relative stretch starts before now and is open above"
        );
        assert_eq!(
            Stretch::Between {
                from: at(1),
                to: at(2)
            }
            .resolve(now),
            TimeRange::new(Some(at(1)), Some(at(2))),
            "a fixed window keeps both of its ends"
        );
    }

    #[test]
    fn a_span_reads_in_the_largest_unit_that_divides_it() {
        assert_eq!(whole_units(300), (5, TimeUnit::Minutes));
        assert_eq!(whole_units(1_800), (30, TimeUnit::Minutes));
        assert_eq!(whole_units(3_600), (1, TimeUnit::Hours));
        assert_eq!(whole_units(90 * 60), (90, TimeUnit::Minutes));
        assert_eq!(whole_units(86_400), (1, TimeUnit::Days));
        assert_eq!(whole_units(7 * 86_400), (7, TimeUnit::Days));
        assert_eq!(
            whole_units(90),
            (90, TimeUnit::Seconds),
            "a span under a minute never rounds down to zero"
        );
    }
}
