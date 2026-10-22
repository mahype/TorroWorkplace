//! The regression corpus of the mail search (issue #136), a test module of [`super`]: the mails
//! of `tests/fixtures/mail/search-corpus.json` (German and English, HTML, compounds, umlauts and
//! their spellings, old mail whose text is only in the index) go through `mime::read` and the store
//! as the sync puts them there, among neutral filler mail; every query of the fixture must find
//! exactly the mails it lists, or rank the one it names first. Beside it a time budget: a sanity
//! check that runs always, and the budget of the issue (100 000 mails, 100 ms) to run by hand:
//! `cargo test --release -p torroworkplace -- --ignored search_budget`.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration as Elapsed, Instant};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use torro_mail::{Flag, Order, Role};
use torro_mail_imap::mime;

use super::*;
use crate::mail::MailDirs;
use crate::mail::query::parse;

#[derive(Debug, Deserialize)]
struct Corpus {
    now: DateTime<Utc>,
    mails: Vec<Mail>,
    queries: Vec<Expected>,
}

#[derive(Debug, Deserialize)]
struct Mail {
    key: String,
    folder: String,
    days_ago: i64,
    from: String,
    to: String,
    #[serde(default)]
    cc: Option<String>,
    subject: String,
    #[serde(default)]
    html: bool,
    body: String,
    #[serde(default)]
    attachments: Vec<String>,
    flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    query: String,
    /// Exactly these mails, in any order.
    #[serde(default)]
    expect: Option<Vec<String>>,
    /// `relevance`: [`Expected::first`] is the best match.
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    first: Option<String>,
}

fn corpus() -> Corpus {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mail/search-corpus.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("a corpus")
}

/// The mail as a server would hand it out: headers, the text (plain or HTML) and its attachments.
fn raw(mail: &Mail, date: DateTime<Utc>) -> Vec<u8> {
    let cc = mail.cc.as_ref().map(|cc| format!("Cc: {cc}\r\n")).unwrap_or_default();
    let kind = if mail.html { "text/html" } else { "text/plain" };
    let head = format!(
        "From: {}\r\nTo: {}\r\n{cc}Subject: {}\r\nDate: {}\r\nMessage-ID: <{}@corpus.example>\r\nMIME-Version: 1.0\r\n",
        mail.from,
        mail.to,
        mail.subject,
        date.to_rfc2822(),
        mail.key
    );
    let text =
        format!("Content-Type: {kind}; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{}\r\n", mail.body);
    if mail.attachments.is_empty() {
        return format!("{head}{text}").into_bytes();
    }
    let mut out = format!("{head}Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n{text}");
    for name in &mail.attachments {
        out.push_str(&format!(
            "--b\r\nContent-Type: application/octet-stream; name=\"{name}\"\r\n\
             Content-Disposition: attachment; filename=\"{name}\"\r\n\r\ncontent\r\n"
        ));
    }
    out.push_str("--b--\r\n");
    out.into_bytes()
}

/// A store with the folders of the corpus (and "Lists" for the filler), fast to fill.
fn store_in(dir: &std::path::Path) -> (MailStore, BTreeMap<String, String>) {
    let dirs = MailDirs::new(&dir.join("data"), &dir.join("cache"));
    let store = MailStore::open(&dirs, "mail:anna@example.org").expect("store");
    // Tests only: no waiting for the disk on every message.
    store.db.execute_batch("PRAGMA synchronous = OFF").expect("fast");
    let mut folders = BTreeMap::new();
    for (path, role) in [
        ("INBOX", Some(Role::Inbox)),
        ("Sent", Some(Role::Sent)),
        ("Archive", Some(Role::Archive)),
        ("Junk", Some(Role::Junk)),
        ("Projekte", None),
        ("Lists", None),
    ] {
        let id = store.upsert_folder(path, path, role).expect("folder");
        store.note_uidvalidity(&id, 1).expect("uidvalidity");
        folders.insert(path.to_string(), id);
    }
    (store, folders)
}

/// Stores `raw` as the sync and the background indexing would: headers, then the text, kept to
/// read when recent, else in the index only.
fn put(store: &MailStore, folder: &str, uid: u32, raw: &[u8], flags: Vec<Flag>, now: DateTime<Utc>) -> String {
    let read = mime::read(raw).expect("readable");
    let new = NewMessage {
        id: None,
        uid,
        rfc_message_id: read.message_id.clone(),
        thread_id: format!("t{uid}"),
        from: read.from.clone(),
        to: read.to.clone(),
        cc: read.cc.clone(),
        reply_to: read.reply_to.clone(),
        subject: read.subject.clone(),
        date: read.date.unwrap_or(now),
        snippet: read.snippet(),
        flags,
        has_attachments: read.has_attachments(),
        traits: read.traits.clone(),
        invitation: read.invitation,
    };
    let id = store.upsert_message(folder, 1, &new).expect("stored");
    if new.date >= now - Duration::days(BODY_RETENTION_DAYS) {
        store.set_body(&id, &Body::of(read, None)).expect("text");
    } else {
        let names: Vec<&str> = read.attachments.iter().filter_map(|part| part.filename.as_deref()).collect();
        store.index_text(&id, &read.text, &names).expect("indexed");
    }
    id
}

#[test]
fn original_depot_survives_cache_and_header_removal() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, folders) = store_in(dir.path());
    let now = Utc::now();
    let raw = b"From: a@example.org\r\nSubject: Keep original\r\n\r\nExact bytes\r\n";
    put(&store, &folders["INBOX"], 7, raw, vec![Flag::Seen], now);
    put(&store, &folders["INBOX"], 8, b"From: b@example.org\r\n\r\nNot retained\r\n", vec![], now);
    store
        .save_original("INBOX", 1, 7, now.fixed_offset(), &["\\Seen".into(), "$Work".into()], raw)
        .expect("original saved");
    let before = store.local_originals_page(0, 10).expect("listed");
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].bytes, raw.len() as u64);
    assert_eq!(before[0].flags, vec!["\\Seen", "$Work"]);
    assert_eq!(store.original_bytes(before[0].seq).expect("read"), Some(raw.to_vec()));
    assert_eq!(store.known_missing_originals(None).expect("gap count"), 1);
    let fetch = store.cached_body_missing(None, torro_ipc::mail::MailCacheMode::AllOffline, 10).expect("offline work");
    assert_eq!(fetch.iter().map(|(_, place)| place.uid).collect::<Vec<_>>(), vec![8]);

    store.prune_bodies(now + Duration::days(100)).expect("body cache pruned");
    store.retain_folders(&[]).expect("headers removed");
    let after = store.local_originals_page(0, 10).expect("listed");
    assert_eq!(after.len(), 1, "kept until the retention period ends");
    assert!(after[0].gone, "its folder is no longer on the server");
    assert_eq!(store.original_bytes(before[0].seq).expect("read"), Some(raw.to_vec()));
}

const FILLER_WORDS: [&str; 26] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliett", "kilo", "lima",
    "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango", "uniform", "victor", "whiskey", "xray",
    "yankee", "zulu",
];

/// `count` neutral mails in "Lists", read, made of words no query of the corpus looks for.
fn fill(store: &MailStore, folder: &str, count: u32, now: DateTime<Utc>) {
    let word = |n: u32| FILLER_WORDS[(n as usize) % FILLER_WORDS.len()];
    for n in 0..count {
        let date = now - Duration::days(i64::from(n % 1500)) - Duration::minutes(i64::from(n));
        let subject = format!("{} {} {}", word(n), word(n / 3 + 7), word(n / 7 + 11));
        let body: Vec<&str> = (0..40).map(|k| word(n.wrapping_mul(31).wrapping_add(k * 17) / 5)).collect();
        let raw = format!(
            "From: {} list <{}@lists.example.net>\r\nTo: anna@example.org\r\nSubject: {subject}\r\nDate: {}\r\n\
             Message-ID: <filler{n}@corpus.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
            word(n + 5),
            word(n + 9),
            date.to_rfc2822(),
            body.join(" ")
        );
        put(store, folder, 100_000 + n, raw.as_bytes(), vec![Flag::Seen], now);
    }
}

/// The corpus in a store among `filler` neutral mails, with the texts older than 90 days pruned;
/// the store, the key of each corpus mail's id, and `now`.
fn loaded(dir: &std::path::Path, filler: u32) -> (MailStore, BTreeMap<String, String>, DateTime<Utc>) {
    let corpus = corpus();
    let (store, folders) = store_in(dir);
    let mut keys = BTreeMap::new();
    for (uid, mail) in corpus.mails.iter().enumerate() {
        let date = corpus.now - Duration::days(mail.days_ago);
        let flags = mail
            .flags
            .iter()
            .map(|flag| match flag.as_str() {
                "seen" => Flag::Seen,
                "flagged" => Flag::Flagged,
                other => panic!("unknown flag {other}"),
            })
            .collect();
        let folder = folders.get(&mail.folder).expect("a corpus folder");
        let id = put(&store, folder, u32::try_from(uid).expect("few") + 1, &raw(mail, date), flags, corpus.now);
        keys.insert(id, mail.key.clone());
    }
    fill(&store, &folders["Lists"], filler, corpus.now);
    store.prune_bodies(corpus.now).expect("pruned");
    (store, keys, corpus.now)
}

fn query(text: &str, relevance: Option<DateTime<Utc>>) -> SearchQuery {
    SearchQuery { query: parse(text).expect("a query"), relevance, ..SearchQuery::default() }
}

#[test]
fn the_corpus_finds_what_it_must_and_nothing_else() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, keys, now) = loaded(dir.path(), 400);
    assert!(store.fully_indexed().expect("asked"), "every text is in the index");
    let key = |id: &String| keys.get(id).cloned().unwrap_or_else(|| format!("filler {id}"));
    let mut failures = Vec::new();
    for expected in corpus().queries {
        let relevance = (expected.sort.as_deref() == Some("relevance")).then_some(now);
        let found: Vec<String> = store
            .search(&query(&expected.query, relevance), 50, 0)
            .expect("searched")
            .iter()
            .map(|m| key(&m.id))
            .collect();
        if let Some(want) = &expected.expect {
            let want: BTreeSet<&str> = want.iter().map(String::as_str).collect();
            let got: BTreeSet<&str> = found.iter().map(String::as_str).collect();
            if want != got {
                failures.push(format!("{:?}: want {want:?}, got {got:?}", expected.query));
            }
            let counted = store.count(&query(&expected.query, None)).expect("counted");
            if counted != want.len() as u64 {
                failures.push(format!("{:?}: counted {counted}", expected.query));
            }
        }
        if let Some(first) = &expected.first
            && found.first() != Some(first)
        {
            failures.push(format!("{:?} by relevance: want {first} first, got {found:?}", expected.query));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Old texts are not kept to read, yet found; their headers are highlighted from the row.
#[test]
fn old_texts_live_on_in_the_index_only() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, keys, _) = loaded(dir.path(), 0);
    let id = |key: &str| keys.iter().find(|(_, k)| k.as_str() == key).map(|(id, _)| id.clone()).expect("a mail");
    let old = store.message(&id("mueller_old")).expect("read").expect("there");
    assert_eq!(old.text, None, "400 days old: not kept to read");
    let recent = store.message(&id("strom")).expect("read").expect("there");
    assert!(recent.text.is_some_and(|text| text.contains("Stromrechnung")));
    assert_eq!(store.index_counts().expect("counted"), (20, 20));
}

/// The slowest first page of each sort: every query of the corpus and a few broad ones, by date
/// and by relevance over `store`; per sort the time and the query.
fn slowest(store: &MailStore, now: DateTime<Utc>) -> [(Elapsed, String); 2] {
    let mut slowest = [(Elapsed::ZERO, String::new()), (Elapsed::ZERO, String::new())];
    // `alpha` is in nearly every filler mail: the worst case of a ranking.
    let extra = ["alpha", "a", "rechnung OR invoice OR meeting", "-alpha", "is:unread", "\"bravo charlie\""];
    let texts = corpus().queries.into_iter().map(|expected| expected.query).chain(extra.map(String::from));
    for text in texts {
        for (at, relevance) in [None, Some(now)].into_iter().enumerate() {
            let search = SearchQuery { order: Order::NewestFirst, ..query(&text, relevance) };
            let started = Instant::now();
            store.search(&search, 50, 0).expect("searched");
            let took = started.elapsed();
            if took > slowest[at].0 {
                slowest[at] = (took, text.clone());
            }
        }
    }
    slowest
}

/// A sanity check of the time a search takes, generous for debug builds and busy machines: a
/// first page of every corpus query over 3 000 mails.
#[test]
fn searches_stay_quick() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _, now) = loaded(dir.path(), 3_000);
    for (took, query) in slowest(&store, now) {
        assert!(took < Elapsed::from_millis(500), "{query} took {took:?}");
    }
}

/// The budget of issue #136: a first page over 100 000 mails under 100 ms by date. By relevance
/// a word found in nearly every mail costs more (every match gets a BM25 rank): measured about
/// 100 ms on an M-series Mac, held to 150 ms here; selective words stay far below. Run by hand in
/// a release build (filling the store takes a while): `cargo test --release -p torroworkplace --
/// --ignored search_budget --nocapture`.
#[test]
#[ignore = "fills 100 000 mails; run by hand with --release"]
fn search_budget_of_100_ms_over_100k_mails() {
    let dir = tempfile::tempdir().expect("temp dir");
    let filled = Instant::now();
    let (store, _, now) = loaded(dir.path(), 100_000);
    println!("filled in {:?}", filled.elapsed());
    let [(by_date, date_query), (by_rank, rank_query)] = slowest(&store, now);
    println!("slowest by date: {date_query} in {by_date:?}; by relevance: {rank_query} in {by_rank:?}");
    assert!(by_date < Elapsed::from_millis(100), "{date_query} took {by_date:?}");
    assert!(by_rank < Elapsed::from_millis(150), "{rank_query} took {by_rank:?}");
}
