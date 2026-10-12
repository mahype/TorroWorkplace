//! The send rule through the public API (task M2.4), with a fake server and a fake Sent folder
//! as the core will plug them in (task M3.3): the journal holds only id and status, private
//! files, accepted and unknown are never sent again, a failed copy is retried alone, a crash
//! mid-send reads as unknown. The real SMTP conversation is tested in `src/smtp.rs`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use torro_mail_imap::smtp::{
    self, CopyPlan, CopyStatus, Envelope, Journal, JournalError, SentCopy, SmtpError, Submission, Submit, Submitted,
};

const MESSAGE: &[u8] = b"Subject: Secret plans\r\nTo: ben@example.org\r\n\r\nThe text.\r\n";

fn envelope() -> Envelope {
    Envelope { from: "anna@example.org".into(), recipients: vec!["ben@example.org".into()] }
}

/// A server that answers the same each time and counts how often it is asked.
struct FakeSmtp {
    answer: Submitted,
    calls: AtomicUsize,
}

impl FakeSmtp {
    fn new(answer: Submitted) -> FakeSmtp {
        FakeSmtp { answer, calls: AtomicUsize::new(0) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Submit for FakeSmtp {
    async fn submit(&self, _: &Envelope, _: &[u8]) -> Submitted {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answer
    }
}

/// A server that never answers: the send hangs until it is cancelled (a crash, for the journal).
struct HangingSmtp;

impl Submit for HangingSmtp {
    async fn submit(&self, _: &Envelope, _: &[u8]) -> Submitted {
        std::future::pending().await
    }
}

/// A Sent folder that fails a number of times, then takes the copy.
struct FakeFolder {
    failures: AtomicUsize,
    saved: Mutex<Vec<Vec<u8>>>,
}

impl FakeFolder {
    fn failing(times: usize) -> FakeFolder {
        FakeFolder { failures: AtomicUsize::new(times), saved: Mutex::new(Vec::new()) }
    }

    fn saved(&self) -> Vec<Vec<u8>> {
        self.saved.lock().expect("lock").clone()
    }
}

impl SentCopy for FakeFolder {
    type Error = &'static str;

    async fn append_sent(&self, message: &[u8]) -> Result<(), &'static str> {
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Err("offline");
        }
        self.saved.lock().expect("lock").push(message.to_vec());
        Ok(())
    }
}

#[tokio::test]
async fn an_accepted_send_with_a_failed_copy_retries_only_the_copy() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    let server = FakeSmtp::new(Submitted::Accepted);
    let folder = FakeFolder::failing(1);

    let report = smtp::send(&journal, "s1", &envelope(), MESSAGE, &server, CopyPlan::Append).await.expect("sent");
    assert_eq!((report.submission, report.copy), (Submission::Accepted, CopyStatus::Pending));

    let first = smtp::save_copy(&journal, "s1", MESSAGE, &folder).await.expect("tried");
    assert_eq!((first.status, first.error), (CopyStatus::Pending, Some("offline")));
    assert_eq!(journal.pending_copies().expect("pending"), ["s1"], "the copy waits");

    // Later (the core's minute tick): the copy again, never the mail.
    let again = smtp::send(&journal, "s1", &envelope(), MESSAGE, &server, CopyPlan::Append).await.expect("asked");
    assert_eq!((again.submission, again.attempted), (Submission::Accepted, false));
    for id in journal.pending_copies().expect("pending") {
        let retry = smtp::save_copy(&journal, &id, MESSAGE, &folder).await.expect("retried");
        assert_eq!((retry.status, retry.error), (CopyStatus::Saved, None));
    }
    assert_eq!(server.calls(), 1, "the mail went out once");
    assert_eq!(folder.saved(), [MESSAGE.to_vec()], "one copy, the sent bytes");
    assert!(journal.pending_copies().expect("pending").is_empty());
    let done = smtp::save_copy(&journal, "s1", MESSAGE, &folder).await.expect("asked");
    assert_eq!(done.status, CopyStatus::Saved);
    assert_eq!(folder.saved().len(), 1, "a saved copy is not made again");
}

#[tokio::test]
async fn the_journal_holds_only_id_and_status_in_private_files() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    assert_eq!(journal.dir(), data.path().join("mail").join("send-journal"));
    smtp::send(&journal, "m:abc", &envelope(), MESSAGE, &FakeSmtp::new(Submitted::Accepted), CopyPlan::Append)
        .await
        .expect("sent");

    let files: Vec<_> =
        std::fs::read_dir(journal.dir()).expect("list").map(|item| item.expect("item").path()).collect();
    assert_eq!(files.len(), 1, "no temporary file left: {files:?}");
    let text = std::fs::read_to_string(&files[0]).expect("entry");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json, serde_json::json!({"id": "m:abc", "submission": "accepted", "copy": "pending"}));
    for content in ["Secret", "ben@", "anna@", "The text"] {
        assert!(!text.contains(content), "{content} in the journal");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &std::path::Path| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&files[0]), 0o600);
        assert_eq!(mode(journal.dir()), 0o700);
        assert_eq!(mode(&data.path().join("mail")), 0o700);
    }
}

#[tokio::test]
async fn unknown_is_final_and_not_accepted_may_be_tried_again() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");

    let refused = FakeSmtp::new(Submitted::NotAccepted(SmtpError::Rejected { code: 451 }));
    let report = smtp::send(&journal, "s1", &envelope(), MESSAGE, &refused, CopyPlan::Append).await.expect("tried");
    assert_eq!((report.submission, report.error), (Submission::NotAccepted, Some(SmtpError::Rejected { code: 451 })));
    smtp::send(&journal, "s1", &envelope(), MESSAGE, &refused, CopyPlan::Append).await.expect("tried");
    assert_eq!(refused.calls(), 2, "a refused message may be sent again");

    let lost = FakeSmtp::new(Submitted::Unknown(SmtpError::Offline));
    let report = smtp::send(&journal, "s1", &envelope(), MESSAGE, &lost, CopyPlan::Append).await.expect("tried");
    assert_eq!((report.submission, report.copy), (Submission::Unknown, CopyStatus::NotNeeded));
    for _ in 0..3 {
        let again = smtp::send(&journal, "s1", &envelope(), MESSAGE, &lost, CopyPlan::Append).await.expect("asked");
        assert_eq!((again.submission, again.attempted), (Submission::Unknown, false));
    }
    assert_eq!(lost.calls(), 1, "unknown is never sent again by itself");

    // The person decided: the core forgets the entry, and the id may be sent again.
    journal.forget("s1").expect("forgotten");
    assert_eq!(journal.entry("s1").expect("read"), None);
    smtp::send(&journal, "s1", &envelope(), MESSAGE, &lost, CopyPlan::Append).await.expect("sent");
    assert_eq!(lost.calls(), 2);
}

#[tokio::test]
async fn a_send_cut_off_mid_attempt_reads_as_unknown_and_is_not_repeated() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");

    let cut = tokio::time::timeout(
        Duration::from_millis(50),
        smtp::send(&journal, "s1", &envelope(), MESSAGE, &HangingSmtp, CopyPlan::Append),
    )
    .await;
    assert!(cut.is_err(), "the send hung and was dropped, like a crash");
    assert_eq!(journal.entry("s1").expect("read").map(|entry| entry.submission), Some(Submission::Unknown));

    let server = FakeSmtp::new(Submitted::Accepted);
    let report = smtp::send(&journal, "s1", &envelope(), MESSAGE, &server, CopyPlan::Append).await.expect("asked");
    assert_eq!((report.submission, report.attempted), (Submission::Unknown, false));
    assert_eq!(server.calls(), 0);

    // A journal opened anew after a restart sees the same.
    let reopened = Journal::open(data.path()).expect("journal");
    assert_eq!(reopened.entry("s1").expect("read").map(|entry| entry.submission), Some(Submission::Unknown));
}

#[tokio::test]
async fn an_unreadable_entry_counts_as_unknown() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    let server = FakeSmtp::new(Submitted::Accepted);
    smtp::send(&journal, "s1", &envelope(), MESSAGE, &server, CopyPlan::ProviderKeepsIt).await.expect("sent");
    let file = std::fs::read_dir(journal.dir()).expect("list").next().expect("one").expect("entry").path();
    std::fs::write(&file, b"{broken").expect("break it");

    let entries = journal.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].id.as_str(), entries[0].submission), ("s1", Submission::Unknown));
    let report = smtp::send(&journal, "s1", &envelope(), MESSAGE, &server, CopyPlan::Append).await.expect("asked");
    assert_eq!((report.submission, report.attempted), (Submission::Unknown, false));
    assert_eq!(server.calls(), 1);
}

#[tokio::test]
async fn providers_that_file_sent_mail_get_no_copy() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    let report = smtp::send(
        &journal,
        "s1",
        &envelope(),
        MESSAGE,
        &FakeSmtp::new(Submitted::Accepted),
        CopyPlan::ProviderKeepsIt,
    )
    .await
    .expect("sent");
    assert_eq!((report.submission, report.copy), (Submission::Accepted, CopyStatus::NotNeeded));
    assert!(journal.pending_copies().expect("pending").is_empty());
    let folder = FakeFolder::failing(0);
    assert_eq!(smtp::save_copy(&journal, "s1", MESSAGE, &folder).await.expect("asked").status, CopyStatus::NotNeeded);
    assert!(folder.saved().is_empty());
}

/// A server that waits until it is let go, to hold a send open.
struct GatedSmtp(tokio::sync::Notify);

impl Submit for GatedSmtp {
    async fn submit(&self, _: &Envelope, _: &[u8]) -> Submitted {
        self.0.notified().await;
        Submitted::Accepted
    }
}

#[tokio::test]
async fn the_same_id_is_never_sent_twice_at_once() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    let server = GatedSmtp(tokio::sync::Notify::new());

    let envelope = envelope();
    let first = smtp::send(&journal, "s1", &envelope, MESSAGE, &server, CopyPlan::Append);
    let second = async {
        // Wait until the first send is with the server; its entry then says "submitting".
        while journal.entry("s1").expect("read").map(|entry| entry.submission) != Some(Submission::Submitting) {
            tokio::task::yield_now().await;
        }
        let answer = smtp::send(&journal, "s1", &envelope, MESSAGE, &server, CopyPlan::Append).await;
        let forget = journal.forget("s1");
        server.0.notify_one();
        (answer, forget)
    };
    let (first, (second, forget)) = tokio::join!(first, second);
    assert_eq!(first.expect("sent").submission, Submission::Accepted);
    assert_eq!(second, Err(JournalError::Busy));
    assert_eq!(forget, Err(JournalError::Busy), "not forgotten while it runs");
}

#[tokio::test]
async fn bad_ids_and_missing_entries_are_refused() {
    let data = tempfile::tempdir().expect("temp dir");
    let journal = Journal::open(data.path()).expect("journal");
    let server = FakeSmtp::new(Submitted::Accepted);
    for id in [String::new(), "x".repeat(101)] {
        let answer = smtp::send(&journal, &id, &envelope(), MESSAGE, &server, CopyPlan::Append).await;
        assert_eq!(answer.map(|report| report.submission), Err(JournalError::InvalidId));
    }
    assert_eq!(server.calls(), 0);
    let folder = FakeFolder::failing(0);
    assert!(matches!(smtp::save_copy(&journal, "nothing", MESSAGE, &folder).await, Err(JournalError::NotFound)));
}

#[cfg(unix)]
#[test]
fn a_journal_directory_that_is_a_link_is_refused() {
    let data = tempfile::tempdir().expect("temp dir");
    let elsewhere = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(data.path().join("mail")).expect("mail dir");
    std::os::unix::fs::symlink(elsewhere.path(), data.path().join("mail").join("send-journal")).expect("link");
    assert!(matches!(Journal::open(data.path()), Err(JournalError::Unsafe)));
}
