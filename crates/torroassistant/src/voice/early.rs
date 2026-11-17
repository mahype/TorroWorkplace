//! Speech recognised ahead (issue #833, decision 1): in every short pause of a sentence the listener
//! hands the sentence so far to [`Early`], which recognises it on a thread of its own while the person may
//! still go on. Is the pause the end of the sentence, its text is there already, or soon; does the person
//! go on, the result is not used. Whisper takes almost as long for a piece as for the whole sentence, so the
//! whole sentence so far is recognised each time, never pieces of it.
//!
//! Only the newest sentence counts: an offer that has not begun yet is replaced by the next one, a result
//! of an older offer is dropped by its number. The sound stays in memory, as everywhere in the listener.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::{Transcriber, VoiceError};

/// What the listener and the thread share.
#[derive(Default)]
struct State {
    /// The newest offer not yet begun, with its number.
    waiting: Option<(u64, Vec<i16>)>,
    /// The result of the last offer recognised, with its number.
    done: Option<(u64, Result<String, VoiceError>)>,
    /// The listener is gone: the thread ends.
    closed: bool,
}

/// The state and the signal that it changed.
type Shared = Arc<(Mutex<State>, Condvar)>;

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Recognises sentences ahead on a thread of its own (see the module).
pub struct Early {
    shared: Shared,
    /// The number of the last offer.
    offered: u64,
    /// The offer whose result counts, since the last [`Early::clear`].
    wanted: Option<u64>,
}

impl Early {
    /// Starts the thread, recognising with `transcriber`.
    pub fn new(mut transcriber: Box<dyn Transcriber + Send>) -> Early {
        let shared: Shared = Arc::default();
        let worker = Arc::clone(&shared);
        std::thread::spawn(move || {
            loop {
                let (number, samples) = {
                    let mut state = lock(&worker);
                    loop {
                        if state.closed {
                            return;
                        }
                        if let Some(offer) = state.waiting.take() {
                            break offer;
                        }
                        state = worker.1.wait(state).unwrap_or_else(PoisonError::into_inner);
                    }
                };
                let text = transcriber.transcribe(&samples);
                lock(&worker).done = Some((number, text));
                worker.1.notify_all();
            }
        });
        Early { shared, offered: 0, wanted: None }
    }

    /// Offers the sentence so far: it replaces an offer not yet begun, and its result is the one that counts.
    pub fn offer(&mut self, samples: Vec<i16>) {
        self.offered += 1;
        self.wanted = Some(self.offered);
        lock(&self.shared).waiting = Some((self.offered, samples));
        self.shared.1.notify_all();
    }

    /// The result of the last offer, waiting for it; `None` when nothing was offered since [`Early::clear`]
    /// (or the result was taken).
    pub fn result(&mut self) -> Option<Result<String, VoiceError>> {
        let wanted = self.wanted.take()?;
        let mut state = lock(&self.shared);
        loop {
            if let Some((number, _)) = &state.done
                && *number == wanted
            {
                return state.done.take().map(|(_, text)| text);
            }
            state = self.shared.1.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Forgets the offers: one not yet begun is dropped, the result of one that runs will not count.
    pub fn clear(&mut self) {
        self.wanted = None;
        lock(&self.shared).waiting = None;
    }
}

impl Drop for Early {
    /// Ends the thread once what it recognises now is done; nothing waits for it.
    fn drop(&mut self) {
        lock(&self.shared).closed = true;
        self.shared.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;

    /// Writes the length of what it was given and counts its calls; waits for a go before each one when
    /// it has a `gate`.
    struct Counts {
        calls: Arc<AtomicUsize>,
        gate: Option<mpsc::Receiver<()>>,
    }

    impl Transcriber for Counts {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            if let Some(gate) = &self.gate {
                // A closed gate lets it go on.
                let _ = gate.recv();
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(samples.len().to_string())
        }
    }

    /// A second offer replaces the first one not yet begun; only the last one's result counts.
    #[test]
    fn a_second_offer_replaces_the_first() {
        let (go, gate) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut early = Early::new(Box::new(Counts { calls: Arc::clone(&calls), gate: Some(gate) }));
        // The thread waits at the gate with the first (or second) offer; the later ones wait behind it and
        // replace each other.
        early.offer(vec![0; 1]);
        early.offer(vec![0; 2]);
        early.offer(vec![0; 3]);
        for _ in 0..3 {
            go.send(()).expect("go");
        }
        assert_eq!(early.result(), Some(Ok("3".into())));
        assert!(calls.load(Ordering::SeqCst) <= 2, "an offer not begun was replaced");
        assert_eq!(early.result(), None, "taken");
    }

    /// After `clear` nothing counts until the next offer.
    #[test]
    fn after_clear_there_is_no_result() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut early = Early::new(Box::new(Counts { calls: Arc::clone(&calls), gate: None }));
        assert_eq!(early.result(), None, "nothing offered");
        early.offer(vec![0; 4]);
        early.clear();
        assert_eq!(early.result(), None);
        early.offer(vec![0; 5]);
        assert_eq!(early.result(), Some(Ok("5".into())));
    }
}
