use std::time::{Duration, Instant};

pub(super) const DEBOUNCE: Duration = Duration::from_millis(500);
pub(super) const FULL_INTERVAL: Duration = Duration::from_secs(15 * 60);

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

    pub fn completed(&mut self, now: Instant) {
        self.full_at = now + FULL_INTERVAL;
        self.hinted_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_coalesce_but_periodic_pass_cannot_be_starved() {
        let now = Instant::now();
        let mut schedule = Schedule::new(now);
        assert_eq!(schedule.due(), now);
        schedule.completed(now);
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
        schedule.completed(now);
        assert_eq!(schedule.due(), now + FULL_INTERVAL);
        schedule.force(now + Duration::from_secs(1));
        assert_eq!(schedule.due(), now + Duration::from_secs(1));
    }
}
