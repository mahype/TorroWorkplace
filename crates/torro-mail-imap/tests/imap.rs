//! IMAP sessions and folder roles against a scripted server on 127.0.0.1 (task M2.3): signing
//! in (right and wrong password, STARTTLS), LIST with SPECIAL-USE, XLIST, names only, creating a
//! missing sent folder, two candidates left open, MOVE without the MOVE capability, a changed
//! UIDVALIDITY, and the fetch, store, append and IDLE commands of the sync.

mod imap_support;

use std::collections::BTreeMap;
use std::time::Duration;

use imap_support::{Server, Step, expect, send, serve, trust};
use torro_core::account::Secret;
use torro_mail::{Flag, Role, Security};
use torro_mail_imap::folders::{self, FolderList, OpenRole};
use torro_mail_imap::imap::{AppendUid, IdleEvent, ImapError, Search, Session};

const LOGIN: &str = "anna@example.org";
const PASSWORD: &str = "hunter2";
/// `\0anna@example.org\0hunter2` in base64.
const BLOB: &str = "AGFubmFAZXhhbXBsZS5vcmcAaHVudGVyMg==";

/// Greeting and `AUTHENTICATE PLAIN` as tag `a1`, ending with the capabilities `caps`.
fn signed_in(caps: &str) -> Vec<Step> {
    vec![
        send("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Dovecot ready."),
        expect("a1 AUTHENTICATE PLAIN"),
        send("+ "),
        expect(BLOB),
        send(&format!("a1 OK [CAPABILITY IMAP4rev1 {caps}] Logged in")),
    ]
}

fn signed_in_lines() -> Vec<String> {
    vec!["a1 AUTHENTICATE PLAIN".into(), BLOB.into()]
}

async fn connect(server: &Server) -> Session {
    Session::connect(&server.endpoint, LOGIN, &Secret::new(PASSWORD), &trust()).await.expect("signed in")
}

#[tokio::test]
async fn backup_reads_exact_original_in_read_only_bounded_parts() {
    let mut steps = signed_in("");
    steps.extend([
        expect("a2 EXAMINE \"INBOX\""),
        send("* 1 EXISTS\r\n* OK [UIDVALIDITY 42] valid"),
        send("a2 OK [READ-ONLY] opened"),
        expect("a3 UID SEARCH ALL"),
        send("* SEARCH 7"),
        send("a3 OK searched"),
        expect("a4 UID FETCH 7 (UID FLAGS INTERNALDATE RFC822.SIZE)"),
        send("* 1 FETCH (UID 7 FLAGS (\\Seen $Custom \\Draft) INTERNALDATE \"20-Oct-2026 09:00:00 -0700\" RFC822.SIZE 9)"),
        send("a4 OK metadata"),
        expect("a5 UID FETCH 7 (UID BODY.PEEK[]<0.4>)"),
        send("* 1 FETCH (UID 7 BODY[]<0> {4}\r\nABCD)"),
        send("a5 OK part"),
        expect("a6 UID FETCH 7 (UID BODY.PEEK[]<4.4>)"),
        send("* 1 FETCH (UID 7 BODY[]<4> {4}\r\nEFGH)"),
        send("a6 OK part"),
        expect("a7 UID FETCH 7 (UID BODY.PEEK[]<8.1>)"),
        send("* 1 FETCH (UID 7 BODY[]<8> {1}\r\nI)"),
        send("a7 OK part"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;
    assert_eq!(session.examine("INBOX").await.expect("read-only folder").uid_validity, 42);
    assert_eq!(session.uid_search(&Search::All).await.expect("inventory"), [7]);
    let metadata = session.fetch_backup_metadata(&[7]).await.expect("metadata");
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].uid, 7);
    assert_eq!(metadata[0].bytes, 9);
    assert_eq!(metadata[0].flags, ["\\Seen", "$Custom", "\\Draft"]);
    assert_eq!(metadata[0].internal_date.offset().local_minus_utc(), -7 * 3600);
    let mut original = Vec::new();
    for (offset, length) in [(0, 4), (4, 4), (8, 1)] {
        original.extend(session.fetch_backup_chunk(7, offset, length).await.expect("part").expect("message exists"));
    }
    assert_eq!(original, b"ABCDEFGHI");
    let received = server.received().await;
    assert!(!received.iter().any(|line| line.contains("SELECT ") || line.contains("STORE ")));
}

fn roles(list: &FolderList) -> Vec<(Role, &str, &str)> {
    let mut roles: Vec<_> =
        list.folders.iter().filter_map(|f| Some((f.role?, f.path.as_str(), f.name.as_str()))).collect();
    roles.sort();
    roles
}

#[tokio::test]
async fn signing_in_over_tls_uses_authenticate_plain_and_reads_the_new_capabilities() {
    let mut steps = signed_in("IDLE MOVE");
    steps.extend([expect("a2 LOGOUT"), send("* BYE Logging out"), send("a2 OK Logout completed")]);
    let server = serve(Security::Tls, steps).await;

    let session = connect(&server).await;
    assert!(session.capabilities().has("idle") && session.capabilities().has("MOVE"));
    assert!(!session.capabilities().has("AUTH=PLAIN"), "the capabilities after signing in replace the greeting's");
    assert!(!format!("{session:?}").contains(PASSWORD));
    session.logout().await.expect("logged out");

    let mut lines = signed_in_lines();
    lines.push("a2 LOGOUT".into());
    assert_eq!(server.received().await, lines);
}

#[tokio::test]
async fn without_auth_plain_login_is_used_with_quoted_arguments() {
    let server = serve(
        Security::Tls,
        vec![
            send("* OK ready"),
            expect("a1 CAPABILITY"),
            send("* CAPABILITY IMAP4rev1"),
            send("a1 OK done"),
            expect("a2 LOGIN \"anna@example.org\" \"pa\\\"ss\\\\word\""),
            send("a2 OK Logged in"),
            expect("a3 CAPABILITY"),
            send("* CAPABILITY IMAP4rev1 NAMESPACE"),
            send("a3 OK done"),
        ],
    )
    .await;

    let session =
        Session::connect(&server.endpoint, LOGIN, &Secret::new("pa\"ss\\word"), &trust()).await.expect("signed in");
    assert!(session.capabilities().has("NAMESPACE"));
    assert!(!server.received().await.iter().any(|line| line.starts_with("UNEXPECTED")));
}

#[tokio::test]
async fn starttls_secures_the_connection_before_signing_in() {
    let server = serve(
        Security::StartTls,
        vec![
            send("* OK [CAPABILITY IMAP4rev1 STARTTLS LOGINDISABLED] ready"),
            expect("a1 STARTTLS"),
            send("a1 OK Begin TLS negotiation now"),
            Step::StartTls,
            expect("a2 CAPABILITY"),
            send("* CAPABILITY IMAP4rev1 AUTH=PLAIN"),
            send("a2 OK done"),
            expect("a3 AUTHENTICATE PLAIN"),
            send("+"),
            expect(BLOB),
            send("a3 OK [CAPABILITY IMAP4rev1 UIDPLUS] Logged in"),
        ],
    )
    .await;

    let session = connect(&server).await;
    assert!(session.capabilities().has("UIDPLUS"));
    assert_eq!(server.received().await, ["a1 STARTTLS", "a2 CAPABILITY", "a3 AUTHENTICATE PLAIN", BLOB]);
}

#[tokio::test]
async fn starttls_refuses_a_server_that_sends_data_before_the_handshake() {
    let server = serve(
        Security::StartTls,
        vec![
            send("* OK ready"),
            expect("a1 STARTTLS"),
            send("a1 OK Begin TLS\r\n* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] injected"),
        ],
    )
    .await;

    let error = Session::connect(&server.endpoint, LOGIN, &Secret::new(PASSWORD), &trust()).await.expect_err("refused");
    assert!(matches!(error, ImapError::Protocol(_)), "{error:?}");
    server.received().await;
}

#[tokio::test]
async fn a_certificate_the_client_does_not_trust_is_refused() {
    let server = serve(Security::Tls, vec![send("* OK ready")]).await;
    let error =
        Session::connect(&server.endpoint, LOGIN, &Secret::new(PASSWORD), &torro_mail_imap::imap::Trust::Only(vec![]))
            .await
            .expect_err("not trusted");
    assert!(matches!(error, ImapError::Protocol(_)), "{error:?}");
}

#[tokio::test]
async fn a_wrong_password_is_an_auth_error_that_never_shows_the_password() {
    let wrong = "wrong-pw";
    let blob = "AGFubmFAZXhhbXBsZS5vcmcAd3JvbmctcHc=";
    let server = serve(
        Security::Tls,
        vec![
            send("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ready"),
            expect("a1 AUTHENTICATE PLAIN"),
            send("+"),
            expect(blob),
            // A careless server that echoes what it got.
            send(&format!("a1 NO [AUTHENTICATIONFAILED] Authentication failed for {wrong} ({blob})")),
        ],
    )
    .await;
    let error = Session::connect(&server.endpoint, LOGIN, &Secret::new(wrong), &trust()).await.expect_err("refused");
    assert!(matches!(error, ImapError::Auth(_)), "{error:?}");
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(wrong) && !shown.contains(blob), "{shown}");
    assert!(shown.contains("AUTHENTICATIONFAILED"));

    // LOGIN with a server that answers BAD and repeats the command.
    let server = serve(
        Security::Tls,
        vec![
            send("* OK [CAPABILITY IMAP4rev1] ready"),
            expect(&format!("a1 LOGIN \"{LOGIN}\" \"{wrong}\"")),
            send(&format!("a1 BAD Unknown command LOGIN \"{LOGIN}\" \"{wrong}\"")),
        ],
    )
    .await;
    let error = Session::connect(&server.endpoint, LOGIN, &Secret::new(wrong), &trust()).await.expect_err("refused");
    assert!(matches!(error, ImapError::Protocol(_)), "BAD is not a wrong password: {error:?}");
    assert!(!format!("{error} {error:?}").contains(wrong));
}

#[tokio::test]
async fn special_use_attributes_give_the_roles() {
    let mut steps = signed_in("SPECIAL-USE");
    steps.extend([
        expect("a2 LIST \"\" \"*\" RETURN (SPECIAL-USE)"),
        send("* LIST (\\HasNoChildren) \"/\" INBOX"),
        send("* LIST (\\HasNoChildren \\Sent) \"/\" \"Gesendete Objekte\""),
        send("* LIST (\\HasNoChildren) \"/\" \"Sent\""),
        send("* LIST (\\HasNoChildren \\Drafts) \"/\" \"Entw&APw-rfe\""),
        send("* LIST (\\HasNoChildren \\Trash) \"/\" \"Trash\""),
        send("* LIST (\\HasNoChildren \\Junk) \"/\" \"Spam\""),
        send("* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\""),
        send("* LIST (\\Noselect \\HasChildren) \"/\" \"Projects\""),
        send("a2 OK List completed"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!(
        roles(&list),
        [
            (Role::Inbox, "INBOX", "INBOX"),
            (Role::Drafts, "Entw&APw-rfe", "Entwürfe"),
            (Role::Sent, "Gesendete Objekte", "Gesendete Objekte"),
            (Role::Archive, "Archive", "Archive"),
            (Role::Junk, "Spam", "Spam"),
            (Role::Trash, "Trash", "Trash"),
        ]
    );
    assert!(list.open.is_empty() && list.created.is_empty());
    let projects = list.folders.iter().find(|f| f.path == "Projects").expect("listed");
    assert!(!projects.selectable && projects.role.is_none());
    assert_eq!(list.folders.iter().find(|f| f.path == "Sent").and_then(|f| f.role), None, "the attribute wins");
    assert_eq!(server.received().await.len(), 3, "no STATUS, nothing created");
}

#[tokio::test]
async fn xlist_gives_the_roles_on_servers_without_special_use() {
    let mut steps = signed_in("XLIST");
    steps.extend([
        expect("a2 XLIST \"\" \"*\""),
        send("* XLIST (\\HasNoChildren \\Inbox) \"/\" \"Inbox\""),
        send("* XLIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\""),
        send("* XLIST (\\HasNoChildren \\AllMail) \"/\" \"[Gmail]/All Mail\""),
        send("* XLIST (\\HasNoChildren \\Drafts) \"/\" \"[Gmail]/Drafts\""),
        send("* XLIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\""),
        send("* XLIST (\\HasNoChildren \\Spam) \"/\" \"[Gmail]/Spam\""),
        send("* XLIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\""),
        send("a2 OK Success"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!(
        roles(&list),
        [
            (Role::Inbox, "Inbox", "Inbox"),
            (Role::Drafts, "[Gmail]/Drafts", "Drafts"),
            (Role::Sent, "[Gmail]/Sent Mail", "Sent Mail"),
            (Role::Archive, "[Gmail]/All Mail", "All Mail"),
            (Role::Junk, "[Gmail]/Spam", "Spam"),
            (Role::Trash, "[Gmail]/Trash", "Trash"),
        ]
    );
    assert!(list.created.is_empty());
    server.received().await;
}

#[tokio::test]
async fn known_names_give_the_roles_when_the_server_marks_nothing() {
    let mut steps = signed_in("");
    steps.extend([
        expect("a2 LIST \"\" \"*\""),
        send("* LIST (\\HasChildren) \".\" INBOX"),
        send("* LIST (\\HasNoChildren) \".\" \"INBOX.Entw&APw-rfe\""),
        send("* LIST (\\HasNoChildren) \".\" \"INBOX.Gesendet\""),
        send("* LIST (\\HasNoChildren) \".\" \"INBOX.Papierkorb\""),
        send("* LIST (\\HasNoChildren) \".\" \"INBOX.Junk E-mail\""),
        send("* LIST (\\HasNoChildren) \".\" \"INBOX.Rechnungen\""),
        send("a2 OK done"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!(
        roles(&list),
        [
            (Role::Inbox, "INBOX", "INBOX"),
            (Role::Drafts, "INBOX.Entw&APw-rfe", "Entwürfe"),
            (Role::Sent, "INBOX.Gesendet", "Gesendet"),
            (Role::Junk, "INBOX.Junk E-mail", "Junk E-mail"),
            (Role::Trash, "INBOX.Papierkorb", "Papierkorb"),
        ]
    );
    assert_eq!(list.path_of(Role::Drafts), Some("INBOX.Entw&APw-rfe"), "the path stays as the server sent it");
    assert_eq!(list.path_of(Role::Archive), None, "no archive is created");
    server.received().await;
}

#[tokio::test]
async fn a_missing_sent_folder_is_created_under_the_namespace_and_subscribed() {
    let mut steps = signed_in("NAMESPACE CREATE-SPECIAL-USE");
    steps.extend([
        expect("a2 LIST \"\" \"*\""),
        send("* LIST (\\HasChildren) \".\" INBOX"),
        send("* LIST (\\HasNoChildren) \".\" INBOX.Drafts"),
        send("a2 OK done"),
        expect("a3 NAMESPACE"),
        send("* NAMESPACE ((\"INBOX.\" \".\")) NIL NIL"),
        send("a3 OK done"),
        expect("a4 CREATE \"INBOX.Sent\" (USE (\\Sent))"),
        send("a4 NO [USEATTR] \\Sent is not supported here"),
        expect("a5 CREATE \"INBOX.Sent\""),
        send("a5 OK Create completed"),
        expect("a6 SUBSCRIBE \"INBOX.Sent\""),
        send("a6 NO Subscriptions are switched off"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!(list.created, ["INBOX.Sent"]);
    assert_eq!((list.path_of(Role::Sent), list.path_of(Role::Drafts)), (Some("INBOX.Sent"), Some("INBOX.Drafts")));
    let sent = list.folders.iter().find(|f| f.path == "INBOX.Sent").expect("added");
    assert_eq!((sent.name.as_str(), sent.delimiter, sent.selectable), ("Sent", Some('.'), true));
    let received = server.received().await;
    assert_eq!(received.last().map(String::as_str), Some("a6 SUBSCRIBE \"INBOX.Sent\""), "{received:?}");
}

#[tokio::test]
async fn two_name_candidates_with_content_leave_the_role_open_until_the_person_picks() {
    let script = || {
        let mut steps = signed_in("");
        steps.extend([
            expect("a2 LIST \"\" \"*\""),
            send("* LIST () \"/\" INBOX"),
            send("* LIST () \"/\" Drafts"),
            send("* LIST () \"/\" Sent"),
            send("* LIST () \"/\" \"Gesendete Objekte\""),
            send("a2 OK done"),
        ]);
        steps
    };
    let mut steps = script();
    steps.extend([
        expect("a3 STATUS \"Sent\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"),
        send("* STATUS Sent (MESSAGES 4 UNSEEN 0 UIDNEXT 5 UIDVALIDITY 1)"),
        send("a3 OK done"),
        expect("a4 STATUS \"Gesendete Objekte\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"),
        send("* STATUS \"Gesendete Objekte\" (MESSAGES 9 UNSEEN 1 UIDNEXT 12 UIDVALIDITY 3)"),
        send("a4 OK done"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;
    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!(list.open, [OpenRole { role: Role::Sent, candidates: vec!["Sent".into(), "Gesendete Objekte".into()] }]);
    assert_eq!(list.path_of(Role::Sent), None);
    assert!(list.created.is_empty(), "an open role is never filled by creating a third folder");
    server.received().await;

    // After the person picked: no STATUS, no question.
    let server = serve(Security::Tls, script()).await;
    let mut session = connect(&server).await;
    let chosen = BTreeMap::from([(Role::Sent, "Gesendete Objekte".to_owned())]);
    let list = folders::discover(&mut session, &chosen).await.expect("folders");
    assert_eq!((list.path_of(Role::Sent), list.open.len()), (Some("Gesendete Objekte"), 0));
    server.received().await;

    // One of the two is empty: the one with mail wins.
    let mut steps = script();
    steps.extend([
        expect("a3 STATUS \"Sent\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"),
        send("* STATUS Sent (MESSAGES 0 UNSEEN 0 UIDNEXT 1 UIDVALIDITY 1)"),
        send("a3 OK done"),
        expect("a4 STATUS \"Gesendete Objekte\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"),
        send("* STATUS \"Gesendete Objekte\" (MESSAGES 9 UNSEEN 1 UIDNEXT 12 UIDVALIDITY 3)"),
        send("a4 OK done"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;
    let list = folders::discover(&mut session, &BTreeMap::new()).await.expect("folders");
    assert_eq!((list.path_of(Role::Sent), list.open.len()), (Some("Gesendete Objekte"), 0));
    server.received().await;
}

fn select_inbox(tag: &str, condstore: bool, uid_validity: u32) -> Vec<Step> {
    let command =
        if condstore { format!("{tag} SELECT \"INBOX\" (CONDSTORE)") } else { format!("{tag} SELECT \"INBOX\"") };
    vec![
        expect(&command),
        send("* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)"),
        send("* 2 EXISTS"),
        send("* 0 RECENT"),
        send(&format!("* OK [UIDVALIDITY {uid_validity}] UIDs valid")),
        send("* OK [UIDNEXT 9] Predicted next UID"),
        send(if condstore { "* OK [HIGHESTMODSEQ 130] Highest" } else { "* OK [NOMODSEQ] No mod-sequences" }),
        send(&format!("{tag} OK [READ-WRITE] Select completed")),
    ]
}

#[tokio::test]
async fn moving_without_the_move_capability_copies_marks_and_expunges() {
    let mut steps = signed_in("UIDPLUS");
    steps.extend(select_inbox("a2", false, 7));
    steps.extend([
        expect("a3 UID COPY 4:5,8 \"Archive\""),
        send("a3 OK [COPYUID 3 4:5,8 1:3] Copy completed"),
        expect("a4 UID STORE 4:5,8 +FLAGS.SILENT (\\Deleted)"),
        send("a4 OK Store completed"),
        expect("a5 UID EXPUNGE 4:5,8"),
        send("* 1 EXPUNGE"),
        send("* 1 EXPUNGE"),
        send("a5 OK Expunge completed"),
        expect("a6 UID COPY 1 \"Gone\""),
        send("a6 NO [TRYCREATE] Mailbox doesn't exist: Gone"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    session.select("INBOX").await.expect("selected");
    session.move_messages(&[8, 4, 5], "Archive").await.expect("moved");
    let error = session.move_messages(&[1], "Gone").await.expect_err("no such folder");
    assert!(matches!(error, ImapError::NotFound(_)), "{error:?}");
    let received = server.received().await;
    assert!(!received.iter().any(|line| line.starts_with("UNEXPECTED")), "{received:?}");
}

#[tokio::test]
async fn a_changed_uidvalidity_is_recognised() {
    let mut steps = signed_in("");
    steps.extend(select_inbox("a2", false, 8));
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let selected = session.select("INBOX").await.expect("selected");
    assert_eq!((selected.uid_validity, selected.uid_next, selected.exists), (8, Some(9), 2));
    assert_eq!(selected.highest_modseq, None, "NOMODSEQ");
    assert!(selected.uid_validity_changed(Some(7)), "the cache from UIDVALIDITY 7 is void");
    assert!(!selected.uid_validity_changed(Some(8)) && !selected.uid_validity_changed(None));
    assert_eq!(session.selected(), Some(&selected));
    server.received().await;
}

const HEADER: &str = "Subject: Rechnung =?utf-8?q?f=C3=BCr?= Oktober\r\nFrom: Bob <bob@example.org>\r\n\r\n";
const MESSAGE: &str = "Subject: Hi\r\n\r\nHello Anna\r\n";

#[tokio::test]
async fn the_sync_commands_search_fetch_and_store_with_condstore() {
    let mut steps = signed_in("CONDSTORE");
    steps.extend(select_inbox("a2", true, 8));
    steps.extend([
        expect("a3 UID SEARCH UID 5:*"),
        send("* SEARCH 4 7 5"),
        send("a3 OK Search completed"),
        expect("a4 UID FETCH 5,7 (UID FLAGS INTERNALDATE RFC822.SIZE BODYSTRUCTURE MODSEQ BODY.PEEK[HEADER])"),
        send(&format!(
            "* 1 FETCH (UID 5 FLAGS (\\Seen \\Flagged) INTERNALDATE \"12-Oct-2026 09:00:00 +0200\" RFC822.SIZE 420 \
             BODYSTRUCTURE ((\"text\" \"plain\" NIL NIL NIL \"7bit\" 5 1 NIL NIL NIL)(\"application\" \"pdf\" (\"name\" \"r.pdf\") \
             NIL NIL \"base64\" 100 NIL (\"attachment\" (\"filename\" \"r.pdf\")) NIL) \"mixed\") MODSEQ (120) BODY[HEADER] {{{}}}\r\n{})",
            HEADER.len(),
            HEADER
        )),
        send("* 1 FETCH (FLAGS (\\Seen))"),
        send(
            "* 2 FETCH (UID 7 FLAGS () INTERNALDATE \" 2-Oct-2026 18:30:00 +0000\" RFC822.SIZE 99 BODYSTRUCTURE \
             (\"text\" \"plain\" NIL NIL NIL \"7bit\" 5 1 NIL NIL NIL) MODSEQ (125) BODY[HEADER] \"Subject: x\")",
        ),
        send("a4 OK Fetch completed"),
        expect("a5 UID FETCH 1:* (UID FLAGS MODSEQ) (CHANGEDSINCE 100)"),
        send("* 1 FETCH (UID 5 FLAGS (\\Seen \\Deleted) MODSEQ (128))"),
        send("a5 OK Fetch completed"),
        expect("a6 UID FETCH 5 (UID BODY.PEEK[])"),
        send(&format!("* 1 FETCH (UID 5 BODY[] {{{}}}\r\n{})", MESSAGE.len(), MESSAGE)),
        send("a6 OK Fetch completed"),
        expect("a7 UID STORE 5,7 +FLAGS.SILENT (\\Flagged)"),
        send("a7 OK Store completed"),
        expect("a8 UID STORE 5,7 -FLAGS.SILENT (\\Seen \\Answered)"),
        send("a8 OK Store completed"),
        expect("a9 STATUS \"INBOX\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ)"),
        send("* STATUS INBOX (MESSAGES 2 UNSEEN 1 UIDNEXT 9 UIDVALIDITY 8 HIGHESTMODSEQ 130)"),
        send("a9 OK Status completed"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let selected = session.select("INBOX").await.expect("selected");
    assert_eq!(selected.highest_modseq, Some(130));
    let uids = session.uid_search(&Search::UidsFrom(5)).await.expect("searched");
    assert_eq!(uids, [5, 7], "UID 4 came back for `5:*` and is dropped");

    let headers = session.fetch_headers(&uids).await.expect("fetched");
    assert_eq!(headers.len(), 2, "the unsolicited FETCH without a UID is skipped");
    let first = &headers[0];
    assert_eq!((first.uid, first.size, first.modseq, first.deleted), (5, 420, Some(120), false));
    assert_eq!(first.flags, [Flag::Seen, Flag::Flagged]);
    assert_eq!(first.header, HEADER.as_bytes());
    assert!(first.has_attachments);
    assert_eq!(first.internal_date.map(|d| d.to_rfc3339()).as_deref(), Some("2026-10-12T07:00:00+00:00"));
    assert_eq!((headers[1].uid, headers[1].has_attachments, headers[1].flags.len()), (7, false, 0));
    assert_eq!(headers[1].header, b"Subject: x");

    let changed = session.fetch_flags(Some(100)).await.expect("flags");
    assert_eq!((changed.len(), changed[0].uid, changed[0].deleted, changed[0].modseq), (1, 5, true, Some(128)));
    let message = session.fetch_message(5).await.expect("fetched").expect("still there");
    assert_eq!(message, MESSAGE.as_bytes());
    session.store_flags(&[7, 5], &[Flag::Flagged], &[Flag::Seen, Flag::Answered]).await.expect("stored");
    let status = session.status("INBOX").await.expect("status");
    assert_eq!((status.total, status.unread, status.uid_validity, status.highest_modseq), (2, 1, Some(8), Some(130)));

    let received = server.received().await;
    assert!(!received.iter().any(|line| line.starts_with("UNEXPECTED")), "{received:?}");
}

#[tokio::test]
async fn idle_reports_changes_times_out_and_append_stores_a_draft() {
    let draft = "Subject: Draft\r\n\r\nNot yet\r\n";
    let mut steps = signed_in("IDLE UIDPLUS");
    steps.extend(select_inbox("a2", false, 8));
    steps.extend([
        expect("a3 IDLE"),
        send("+ idling"),
        send("* OK Still here"),
        send("* 3 EXISTS"),
        expect("DONE"),
        send("a3 OK Idle completed"),
        expect("a4 IDLE"),
        send("+ idling"),
        expect("DONE"),
        send("a4 OK Idle completed"),
        expect(&format!("a5 APPEND \"Drafts\" (\\Seen \\Draft) {{{}}}", draft.len())),
        send("+ OK"),
        Step::Literal(draft.len()),
        expect(""),
        send("a5 OK [APPENDUID 3 17] Append completed"),
    ]);
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;
    session.select("INBOX").await.expect("selected");

    // On its own tokio task, as the core's IDLE connection runs: the session and its futures are Send.
    let task = tokio::spawn(async move {
        let first = session.idle(Duration::from_secs(10)).await;
        let second = session.idle(Duration::from_millis(50)).await;
        (session, first, second)
    });
    let (mut session, first, second) = task.await.expect("the task ran");
    assert_eq!(first.expect("idle"), IdleEvent::Changed);
    assert_eq!(second.expect("idle"), IdleEvent::Timeout);

    let appended = session.append("Drafts", draft.as_bytes(), &[Flag::Seen], true).await.expect("appended");
    assert_eq!(appended, Some(AppendUid { uid_validity: 3, uid: 17 }));
    let received = server.received().await;
    assert!(!received.iter().any(|line| line.starts_with("UNEXPECTED")), "{received:?}");
    assert!(received.contains(&format!("<{} bytes: {draft}>", draft.len())));
}

#[tokio::test]
async fn a_dropped_connection_is_offline_and_commands_need_an_open_folder() {
    let mut steps = signed_in("");
    steps.push(expect("a2 NOOP"));
    let server = serve(Security::Tls, steps).await;
    let mut session = connect(&server).await;

    let error = session.uid_search(&Search::All).await.expect_err("nothing selected");
    assert!(matches!(error, ImapError::Protocol(_)), "{error:?}");
    let error = session.noop().await.expect_err("the server hung up");
    assert!(matches!(error, ImapError::Offline(_)), "{error:?}");
    let error = session.noop().await.expect_err("still broken");
    assert!(matches!(error, ImapError::Offline(_)), "{error:?}");
    server.received().await;
}
