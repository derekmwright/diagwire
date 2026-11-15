use jiff::civil::Date;
use jiff::{Timestamp, Zoned};

/// The garage's only source of time (G10). Injected so tests never read the
/// wall clock.
pub trait Clock: Send + Sync {
    /// Now, in UTC. Stored as `recorded_at`.
    fn now(&self) -> Timestamp;
    /// The host's local calendar date. Used when a write gives no date.
    fn today(&self) -> Date;
}

/// The host clock and time zone.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn today(&self) -> Date {
        Zoned::now().date()
    }
}
