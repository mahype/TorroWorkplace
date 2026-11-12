//! How much a client may send on the person's behalf in `chat` and `presence`
//! (`docs/chat.md`, "Drossel"): at most [`PER_HOUR`] commands an hour and [`PER_MINUTE`] a minute,
//! counted per client id, so a key the assistant lends does not get around it. Even with sending
//! allowed freely, an agent gone wrong cannot flood anyone. The app key is never throttled.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The most sending commands of one client in an hour.
pub const PER_HOUR: usize = 30;

/// The most sending commands of one client in a minute.
pub const PER_MINUTE: usize = 5;

const HOUR: Duration = Duration::from_secs(60 * 60);
const MINUTE: Duration = Duration::from_secs(60);

/// See the module docs.
#[derive(Debug, Default)]
pub struct Throttle {
    sent: Mutex<BTreeMap<String, VecDeque<Instant>>>,
}

impl Throttle {
    /// Whether `client` may send one more command at `now`; counts it when it may.
    pub fn admit(&self, client: &str, now: Instant) -> bool {
        let mut sent = self.sent.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let times = sent.entry(client.to_string()).or_default();
        while times.front().is_some_and(|at| now.duration_since(*at) >= HOUR) {
            times.pop_front();
        }
        let last_minute = times.iter().filter(|at| now.duration_since(**at) < MINUTE).count();
        if times.len() >= PER_HOUR || last_minute >= PER_MINUTE {
            return false;
        }
        times.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_sends_five_a_minute_and_thirty_an_hour() {
        let throttle = Throttle::default();
        let start = Instant::now();
        for n in 0..PER_MINUTE {
            assert!(throttle.admit("agent", start + Duration::from_secs(n as u64)), "{n}");
        }
        assert!(!throttle.admit("agent", start + Duration::from_secs(30)), "the sixth within a minute");
        assert!(throttle.admit("other", start + Duration::from_secs(30)), "counted per client");
        // Spread over the hour: thirty go, the next waits until the first is an hour old.
        let mut at = start + Duration::from_secs(120);
        for n in PER_MINUTE..PER_HOUR {
            assert!(throttle.admit("agent", at), "{n}");
            at += Duration::from_secs(61);
        }
        assert!(!throttle.admit("agent", at));
        assert!(throttle.admit("agent", start + HOUR + Duration::from_secs(1)));
    }
}
