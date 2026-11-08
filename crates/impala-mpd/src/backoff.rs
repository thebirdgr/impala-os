//! Exponential reconnect backoff. Pure logic, time is injected.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
    delay: Duration,
    next_attempt: Option<Instant>,
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self { initial, max, delay: initial, next_attempt: None }
    }

    /// May we try to connect now?
    pub fn ready(&self, now: Instant) -> bool {
        self.next_attempt.is_none_or(|t| now >= t)
    }

    /// Time until the next attempt is allowed.
    pub fn retry_in(&self, now: Instant) -> Duration {
        self.next_attempt.map(|t| t.saturating_duration_since(now)).unwrap_or_default()
    }

    /// Record a failed attempt; doubles the delay up to `max`.
    pub fn fail(&mut self, now: Instant) {
        self.next_attempt = Some(now + self.delay);
        self.delay = (self.delay * 2).min(self.max);
    }

    pub fn succeed(&mut self) {
        self.delay = self.initial;
        self.next_attempt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_until_capped_and_resets() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut b = Backoff::new(ms(250), ms(1000));
        assert!(b.ready(t0));
        b.fail(t0);
        assert!(!b.ready(t0));
        assert_eq!(b.retry_in(t0), ms(250));
        assert!(b.ready(t0 + ms(250)));
        b.fail(t0);
        assert_eq!(b.retry_in(t0), ms(500));
        b.fail(t0);
        assert_eq!(b.retry_in(t0), ms(1000));
        b.fail(t0);
        assert_eq!(b.retry_in(t0), ms(1000), "capped");
        b.succeed();
        assert!(b.ready(t0));
        b.fail(t0);
        assert_eq!(b.retry_in(t0), ms(250), "reset after success");
    }
}
