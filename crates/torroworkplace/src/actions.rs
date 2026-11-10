//! The kinds of action Torro looks for in mail, and the queue that reads them out (issue #603,
//! `docs/vorschlaege.md`).
//!
//! **One kind, one entry.** A kind brings everything the three steps need: the question the
//! decision model answers while it sorts a mail (*detect*, [`KindDef::question`], asked in
//! [`crate::categorize`]), the form whose fields a chat model reads out of the mail (*extract*,
//! [`KindDef::form`], [`crate::suggest`]) and the dialog the apps open (*act*). A new kind is a new
//! entry in [`KINDS`]: sorting asks about it, `mail.list_action_kinds` lists it, and the inbox app
//! shows it among its actions, without any of them changing.
//!
//! **Two steps.** Detecting rides along with the sorting call that runs for every new mail anyway –
//! a yes/no question more. Only a mail where it said yes is read out, by a chat model, in the
//! background, one mail at a time ([`Readings`]); what is still missing when the person opens the
//! mail is read out then (`mail.extract_action`).

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use torro_core::settings::Settings;
use torro_ipc::actions::{ActionForm, ActionKind};

/// One kind of action ([`KINDS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindDef {
    /// Its id: what suggestions, settings and commands name it by.
    pub id: &'static str,
    /// What a person reads for it, in English; the apps translate it.
    pub label: &'static str,
    /// The Lucide icon's name.
    pub icon: &'static str,
    /// The dialog it fills.
    pub form: ActionForm,
    /// The yes/no question the decision model answers about a mail. English: it is for the model.
    pub question: &'static str,
}

/// The kinds of action, in the order the apps show them.
pub const KINDS: &[KindDef] = &[
    KindDef {
        id: "event",
        label: "Event",
        icon: "calendar-plus",
        form: ActionForm::Event,
        question: "Does this mail name a meeting, call or appointment at a date or time the person who received it \
                   may take part in or should keep free?",
    },
    KindDef {
        id: "task",
        label: "Task",
        icon: "list-todo",
        form: ActionForm::Task,
        question: "Does this mail ask the person who received it to do something specific, such as sending, \
                   preparing, checking or deciding something, beyond simply answering?",
    },
];

/// The prefix of a kind's question in a sorting call: `action:<id>`.
pub const QUESTION_PREFIX: &str = "action:";

/// The kind with the id `id`, `None` for one there is none of.
pub fn kind(id: &str) -> Option<&'static KindDef> {
    KINDS.iter().find(|kind| kind.id == id)
}

/// Every kind as `mail.list_action_kinds` lists it, with whether `settings` looks for it.
pub fn kinds(settings: &Settings) -> Vec<ActionKind> {
    KINDS
        .iter()
        .map(|kind| ActionKind {
            id: kind.id.to_string(),
            label: kind.label.to_string(),
            icon: kind.icon.to_string(),
            form: kind.form,
            enabled: !settings.mail_actions_off.contains(kind.id),
        })
        .collect()
}

/// The kinds `settings` looks for in new mail: what the sorting call asks about.
pub fn looked_for(settings: &Settings) -> Vec<&'static KindDef> {
    KINDS.iter().filter(|kind| !settings.mail_actions_off.contains(kind.id)).collect()
}

/// The mails waiting to be read out (issue #603, decision 3), shared by the decider that finds the
/// suggestions and the core's reader that fills them: each mail once, one at a time. Cheap to clone.
#[derive(Clone, Default)]
pub struct Readings {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The mails waiting, as `(account, message)`, oldest first.
    queue: Mutex<VecDeque<(String, String)>>,
    /// The mails being read out now.
    running: Mutex<HashSet<(String, String)>>,
    /// Wakes the reader when a mail comes.
    arrived: tokio::sync::Notify,
    /// Wakes those waiting for a mail being read out ([`Readings::wait_for`]).
    finished: tokio::sync::Notify,
}

impl Readings {
    /// Puts the mail `message` of `account` in the queue, unless it waits already.
    pub fn enqueue(&self, account: &str, message: &str) {
        let key = (account.to_string(), message.to_string());
        let mut queue = self.inner.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if !queue.contains(&key) {
            queue.push_back(key);
            self.inner.arrived.notify_one();
        }
    }

    /// The next mail to read out, waiting until there is one; marked as being read out until
    /// [`Readings::finish`]. Taken from the queue and marked under the queue's lock, so
    /// [`Readings::claim`] always sees it in one of the two.
    ///
    /// A mail read out by hand right now ([`Readings::claim`]) stays in the queue until that ends.
    pub async fn next(&self) -> (String, String) {
        loop {
            let arrived = self.inner.arrived.notified();
            let finished = self.inner.finished.notified();
            {
                let mut queue = self.inner.queue.lock().unwrap_or_else(PoisonError::into_inner);
                let mut running = self.inner.running.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(at) = queue.iter().position(|key| !running.contains(key))
                    && let Some(key) = queue.remove(at)
                {
                    running.insert(key.clone());
                    return key;
                }
            }
            tokio::select! {
                () = arrived => {}
                () = finished => {}
            }
        }
    }

    /// Takes the mail `message` of `account` for a reading asked for by hand (`mail.extract_action`):
    /// out of the queue, and – once a reading under way has ended – marked as being read out until
    /// [`Readings::finish`], so the reader in the background never asks a second model about it at
    /// the same time.
    pub async fn claim(&self, account: &str, message: &str) -> (String, String) {
        let key = (account.to_string(), message.to_string());
        loop {
            let finished = self.inner.finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            {
                let mut queue = self.inner.queue.lock().unwrap_or_else(PoisonError::into_inner);
                queue.retain(|held| *held != key);
                let mut running = self.inner.running.lock().unwrap_or_else(PoisonError::into_inner);
                if running.insert(key.clone()) {
                    return key;
                }
            }
            finished.await;
        }
    }

    /// The mail is read out.
    pub fn finish(&self, key: &(String, String)) {
        self.inner.running.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
        self.inner.finished.notify_waiters();
    }

    /// Waits while the mail `message` of `account` is being read out.
    #[cfg(test)]
    pub async fn wait_for(&self, account: &str, message: &str) {
        let key = (account.to_string(), message.to_string());
        loop {
            let finished = self.inner.finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            if !self.inner.running.lock().unwrap_or_else(PoisonError::into_inner).contains(&key) {
                return;
            }
            finished.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn every_kind_has_an_id_of_its_own_and_a_question() {
        let ids: HashSet<&str> = KINDS.iter().map(|kind| kind.id).collect();
        assert_eq!(ids.len(), KINDS.len());
        assert!(KINDS.iter().all(|kind| !kind.question.trim().is_empty() && !kind.label.is_empty()));
        assert_eq!(kind("event").map(|kind| kind.form), Some(ActionForm::Event));
        assert_eq!(kind("github"), None);
    }

    #[test]
    fn a_kind_switched_off_is_listed_but_not_looked_for() {
        let mut settings = Settings::default();
        assert!(kinds(&settings).iter().all(|kind| kind.enabled), "every kind starts on");
        settings.mail_actions_off.insert("task".into());
        let listed = kinds(&settings);
        assert_eq!(listed.len(), KINDS.len(), "still listed");
        assert!(!listed.iter().find(|kind| kind.id == "task").expect("task").enabled);
        assert_eq!(looked_for(&settings).iter().map(|kind| kind.id).collect::<Vec<_>>(), ["event"]);
    }

    /// A fixture of `tests/fixtures/actions/` (issue #603): a mail and what a person expects in it.
    struct Fixture {
        name: String,
        content: crate::categorize::Content,
        /// The ids of the kinds expected.
        expect: Vec<String>,
    }

    /// Every fixture of `tests/fixtures/actions/`, each checked to read.
    fn fixtures() -> Vec<Fixture> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actions");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("the fixtures") {
            let path = entry.expect("an entry").path();
            if path.extension().and_then(|end| end.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable");
            let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|err| panic!("{path:?}: {err}"));
            let expect = value["expect"].as_object().unwrap_or_else(|| panic!("{path:?} expects nothing"));
            for (id, wanted) in expect {
                assert!(kind(id).is_some(), "{path:?}: no kind {id:?}");
                assert!(wanted.is_boolean(), "{path:?}: {id} is yes or no");
            }
            let subject = value["subject"].as_str().unwrap_or_default().to_string();
            let text = value["text"].as_str().unwrap_or_default().to_string();
            assert!(!subject.is_empty() && !text.is_empty(), "{path:?} has no subject or text");
            found.push(Fixture {
                name: path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_string(),
                content: crate::categorize::Content {
                    subject,
                    from: value["from"].as_str().map(str::to_string),
                    list: value["list"].as_str().map(str::to_string),
                    bulk: value["bulk"].as_bool().unwrap_or(false),
                    date: None,
                    text,
                },
                expect: expect
                    .iter()
                    .filter(|(_, yes)| yes.as_bool() == Some(true))
                    .map(|(id, _)| id.clone())
                    .collect(),
            });
        }
        found
    }

    /// Issue #603: enough mails to measure the detection with, of every sort the plan names –
    /// events, tasks, both, and mail with neither (a newsletter with a date among them).
    #[test]
    fn the_fixtures_for_measuring_the_detection_are_sound() {
        let all = fixtures();
        assert!(all.len() >= 12, "too few fixtures to measure anything: {}", all.len());
        let with = |id: &str| all.iter().filter(|fixture| fixture.expect.iter().any(|kind| kind == id)).count();
        assert!(with("event") >= 3 && with("task") >= 3, "events and tasks to find");
        assert!(all.iter().filter(|fixture| fixture.expect.len() == 2).count() >= 2, "mail with both");
        assert!(all.iter().filter(|fixture| fixture.expect.is_empty()).count() >= 4, "mail with neither");
    }

    /// Issue #603, decision 1: how well a decision model finds the kinds of action in the fixtures,
    /// asked as the sorting of new mail asks them ([`crate::categorize::detect_request`] with every
    /// kind), at `TORRO_EVAL_BASE` (an Ollama `…/v1`) with `TORRO_EVAL_MODEL`. Prints hits, false
    /// alarms and misses of each kind and the time a mail took; the numbers go to
    /// `docs/vorschlaege.md`. `cargo test --release -p torroworkplace -- --ignored
    /// action_detection_quality --nocapture`.
    #[tokio::test]
    #[ignore = "needs a running decision model"]
    async fn action_detection_quality() {
        let base = std::env::var("TORRO_EVAL_BASE").expect("TORRO_EVAL_BASE, e.g. http://localhost:11434/v1");
        let model = std::env::var("TORRO_EVAL_MODEL").expect("TORRO_EVAL_MODEL, e.g. nimble");
        let http = reqwest::Client::new();
        let every: Vec<&KindDef> = KINDS.iter().collect();
        let (mut hits, mut alarms, mut missed) = (HashMap::new(), HashMap::new(), HashMap::new());
        let mut took = std::time::Duration::ZERO;
        let all = fixtures();
        for fixture in &all {
            let request = crate::categorize::detect_request(&model, &fixture.content, &every);
            let body = torro_core::ai::systemone::decide_body_for(&request).expect("a legal call");
            let started = std::time::Instant::now();
            let answer = http
                .post(format!("{}/systemone", base.trim_end_matches('/')))
                .json(&body)
                .send()
                .await
                .expect("the model answers");
            let bytes = answer.bytes().await.expect("an answer");
            took += started.elapsed();
            let reply = torro_core::ai::systemone::read_decision(&bytes).expect("a decision");
            let found = crate::categorize::detected(&reply);
            for kind in KINDS.iter().map(|kind| kind.id) {
                let (expected, said) = (fixture.expect.iter().any(|id| id == kind), found.iter().any(|id| id == kind));
                let counter = match (expected, said) {
                    (true, true) => &mut hits,
                    (false, true) => &mut alarms,
                    (true, false) => &mut missed,
                    (false, false) => continue,
                };
                *counter.entry(kind).or_insert(0u32) += 1;
                if expected != said {
                    println!("action_detection_quality: {}: {kind} expected {expected}, said {said}", fixture.name);
                }
            }
        }
        for kind in KINDS.iter().map(|kind| kind.id) {
            let count = |counter: &HashMap<&str, u32>| counter.get(kind).copied().unwrap_or(0);
            println!(
                "action_detection_quality: {kind}: {} hits, {} false alarms, {} missed",
                count(&hits),
                count(&alarms),
                count(&missed)
            );
        }
        let mails = u32::try_from(all.len()).unwrap_or(u32::MAX).max(1);
        println!("action_detection_quality: {} mails, {:?} a mail with {model}", all.len(), took / mails);
    }

    #[tokio::test]
    async fn each_mail_waits_once_and_a_reading_is_waited_for() {
        let readings = Readings::default();
        readings.enqueue("mail:a@x", "m:1");
        readings.enqueue("mail:a@x", "m:1");
        readings.enqueue("mail:a@x", "m:2");
        let first = readings.next().await;
        assert_eq!(first, ("mail:a@x".to_string(), "m:1".to_string()));
        let waiting = {
            let readings = readings.clone();
            tokio::spawn(async move { readings.wait_for("mail:a@x", "m:1").await })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "waits while it is read out");
        readings.finish(&first);
        waiting.await.expect("done waiting");
        // Claimed by hand while it waits: out of the queue, and the reader gets the next one.
        readings.enqueue("mail:a@x", "m:3");
        let claimed = readings.claim("mail:a@x", "m:3").await;
        assert_eq!(readings.next().await.1, "m:2", "the same mail never twice");
        readings.finish(&claimed);
        readings.wait_for("mail:a@x", "m:9").await;
        // Read by hand: queued again meanwhile, the reader leaves it until the reading ends.
        let by_hand = readings.claim("mail:a@x", "m:4").await;
        readings.enqueue("mail:a@x", "m:4");
        readings.enqueue("mail:a@x", "m:5");
        assert_eq!(readings.next().await.1, "m:5", "not the mail read by hand");
        readings.finish(&("mail:a@x".to_string(), "m:5".to_string()));
        readings.finish(&by_hand);
        assert_eq!(readings.next().await.1, "m:4", "once the reading by hand ended");
    }
}
