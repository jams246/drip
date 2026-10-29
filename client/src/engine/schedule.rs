use std::time::{Duration, Instant};

pub(super) const DEBOUNCE: Duration = Duration::from_millis(500);
pub(super) const FULL_INTERVAL: Duration = Duration::from_secs(15 * 60);

pub(super) fn backoff(attempt: u32) -> Result<Duration, super::EngineError> {
    let seconds = 1u64.checked_shl(attempt.min(6)).unwrap_or(60).min(60);
    let jitter = crate::source::random_id()?;
    let jitter = u8::from_str_radix(&jitter[..2], 16)
        .map_err(|_| super::EngineError::InvalidResponse("invalid random retry identifier"))?;
    Ok(
        (Duration::from_secs(seconds) + Duration::from_millis(u64::from(jitter)))
            .min(Duration::from_secs(60)),
    )
}

pub(super) struct Schedule {
    full_at: Instant,
    hinted_at: Option<Instant>,
}

impl Schedule {
    pub fn new(now: Instant) -> Self {
        Self {
            full_at: now,
            hinted_at: None,
        }
    }

    pub fn hint(&mut self, now: Instant) {
        self.hinted_at = Some(now + DEBOUNCE);
    }

    pub fn force(&mut self, now: Instant) {
        self.full_at = now;
    }

    pub fn due(&self) -> Instant {
        self.hinted_at
            .map_or(self.full_at, |hint| hint.min(self.full_at))
    }

    pub fn begin(&mut self, now: Instant) -> bool {
        let periodic = now >= self.full_at;
        if periodic {
            self.full_at = now + FULL_INTERVAL;
        }
        self.hinted_at = None;
        periodic
    }

    pub fn retry(&mut self, now: Instant) {
        self.hinted_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backoff_starts_at_one_second_and_stays_bounded() {
        for attempt in 0..100 {
            let delay = backoff(attempt).unwrap();
            assert!(delay >= Duration::from_secs(1));
            assert!(delay <= Duration::from_secs(60));
            if attempt == 0 {
                assert!(delay < Duration::from_millis(1256));
            }
        }
    }

    #[test]
    fn hints_coalesce_but_periodic_pass_cannot_be_starved() {
        let now = Instant::now();
        let mut schedule = Schedule::new(now);
        assert_eq!(schedule.due(), now);
        schedule.begin(now);
        schedule.hint(now + Duration::from_millis(1));
        schedule.hint(now + Duration::from_millis(400));
        assert_eq!(schedule.due(), now + Duration::from_millis(900));
        schedule.hint(now + FULL_INTERVAL);
        assert_eq!(schedule.due(), now + FULL_INTERVAL);
    }

    #[test]
    fn dropped_hints_still_get_periodic_reconciliation() {
        let now = Instant::now();
        let mut schedule = Schedule::new(now);
        schedule.begin(now);
        assert_eq!(schedule.due(), now + FULL_INTERVAL);
        schedule.force(now + Duration::from_secs(1));
        assert_eq!(schedule.due(), now + Duration::from_secs(1));
    }

    #[test]
    fn targeted_passes_and_retries_do_not_postpone_the_periodic_deadline() {
        let now = Instant::now();
        let mut schedule = Schedule::new(now);
        assert!(schedule.begin(now));
        schedule.hint(now + Duration::from_secs(10));
        assert!(!schedule.begin(now + Duration::from_secs(11)));
        schedule.retry(now + Duration::from_secs(12));
        assert!(!schedule.begin(now + Duration::from_secs(12)));
        assert_eq!(schedule.due(), now + FULL_INTERVAL);
        assert!(schedule.begin(now + FULL_INTERVAL));
    }
}
