//! Reading and building messages (task M2.4): a built message reads back as written (umlauts,
//! attachment, reply headers), HTML becomes text without anything being fetched, list and
//! automatic mail is marked, invitations and attachments are found. No network.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use torro_mail::{Flag, MailAddress};
use torro_mail_imap::mime::{self, MimeError, Outgoing, OutgoingAttachment, Place, Reply};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mime").join(name);
    std::fs::read(&path).expect("fixture")
}

fn address(name: Option<&str>, address: &str) -> MailAddress {
    MailAddress { name: name.map(str::to_owned), address: address.to_owned() }
}

fn when() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 12, 9, 30, 0).single().expect("a date")
}

fn outgoing() -> Outgoing {
    Outgoing {
        from: address(Some("Jürgen Müller"), "juergen@example.de"),
        to: vec![address(Some("Anna Äpfelbaum"), "anna@example.org")],
        cc: vec![address(None, "carla@example.org")],
        bcc: vec![address(None, "secret@example.net"), address(None, "ANNA@example.org")],
        subject: "Grüße aus Köln – Größe ✓".into(),
        body: "Hallo Anna,\n\nwie geht’s? Ünïcödé und ein sehr langer Satz, der weit über die übliche Zeilenlänge von \
               achtundsiebzig Zeichen hinausgeht.\n.Zeile mit Punkt\n"
            .into(),
        html: None,
        signature: Some("Jürgen\nKöln".into()),
        reply: Some(Reply {
            in_reply_to: "docs-2@example.de".into(),
            references: vec!["docs-1@example.org".into(), "docs-2@example.de".into()],
        }),
        attachments: vec![OutgoingAttachment {
            filename: "Präsentation.pdf".into(),
            content_type: "application/pdf".into(),
            bytes: (0u8..=255).collect(),
        }],
        calendar: None,
        message_id: "t1.abc@example.de".into(),
        date: when(),
    }
}

/// Issue #125: formatted mail goes as `multipart/alternative` (plain text, then HTML) beside its
/// attachment; both forms carry the signature above the quoted history, the HTML is cleaned, and
/// the plain text is made from it.
#[test]
fn formatted_mail_carries_html_and_the_plain_text_made_from_it() {
    let html = concat!(
        "<p>Hallo <strong>Anna</strong>,<br>siehe <a href=\"https://example.org/q4\">Tabelle</a>.</p>",
        "<script>steal()</script><img src=\"https://tracker.example/p.gif\">",
        "<div class=\"torro-quote\"><p>Am Montag schrieb Jonas:</p><blockquote type=\"cite\">Hi</blockquote></div>",
    );
    let formatted = Outgoing { html: Some(html.into()), body: "not used".into(), ..outgoing() };
    let built = mime::build(&formatted).expect("built");
    let raw = String::from_utf8(built.bytes.clone()).expect("ascii");
    assert!(raw.contains("multipart/mixed") && raw.contains("multipart/alternative"), "{raw}");
    let plain_at = raw.find("text/plain").expect("a plain part");
    let html_at = raw.find("text/html").expect("an HTML part");
    assert!(plain_at < html_at, "the richer form comes last");

    let read = mime::read(&built.bytes).expect("read back");
    assert!(!read.from_html, "the plain form is read first");
    assert_eq!(
        read.text.trim_end(),
        "Hallo Anna,\nsiehe Tabelle <https://example.org/q4>.\n\n-- \nJürgen\nKöln\n\nAm Montag schrieb Jonas:\n\n> Hi"
    );
    let sent = read.html.expect("the HTML part");
    assert!(sent.contains("<strong>Anna</strong>") && sent.contains("href=\"https://example.org/q4\""), "{sent}");
    assert!(!sent.contains("script") && !sent.contains("tracker"), "cleaned: {sent}");
    let signature = sent.find("torro-signature").expect("signed");
    assert!(signature < sent.find("torro-quote").expect("the history"), "the signature is above the history");
    assert_eq!(read.attachments.len(), 1, "the attachment is still there");
    assert_eq!(mime::build(&formatted).expect("again").bytes, built.bytes, "the same draft, the same bytes");

    // Without attachments the alternative is the whole message.
    let alone = Outgoing { attachments: vec![], ..formatted };
    let raw = String::from_utf8(mime::build(&alone).expect("built").bytes).expect("ascii");
    assert!(raw.contains("multipart/alternative") && !raw.contains("multipart/mixed"), "{raw}");
    // A plain message has no HTML to read back.
    assert_eq!(mime::read(&mime::build(&outgoing()).expect("built").bytes).expect("read").html, None);
}

#[test]
fn a_built_message_reads_back_with_umlauts_attachment_and_reply_headers() {
    let built = mime::build(&outgoing()).expect("built");
    assert!(built.bytes.is_ascii(), "7-bit: non-ASCII text is encoded");
    assert!(built.bytes.windows(2).all(|pair| pair[1] != b'\n' || pair[0] == b'\r'), "CRLF only");

    let read = mime::read(&built.bytes).expect("read back");
    assert_eq!(read.subject, "Grüße aus Köln – Größe ✓");
    assert_eq!(read.from, Some(address(Some("Jürgen Müller"), "juergen@example.de")));
    assert_eq!(read.to, [address(Some("Anna Äpfelbaum"), "anna@example.org")]);
    assert_eq!(read.cc, [address(None, "carla@example.org")]);
    assert_eq!(read.message_id.as_deref(), Some("t1.abc@example.de"));
    assert_eq!(read.in_reply_to.as_deref(), Some("docs-2@example.de"));
    assert_eq!(read.references, ["docs-1@example.org", "docs-2@example.de"]);
    assert_eq!(read.date, Some(when()));
    assert!(!read.from_html);
    assert_eq!(read.text.trim_end(), format!("{}\n\n-- \nJürgen\nKöln", outgoing().body.trim_end()));

    assert_eq!(read.attachments.len(), 1);
    let info = &read.attachments[0];
    assert_eq!(
        (info.filename.as_deref(), info.content_type.as_str(), info.size, info.inline),
        (Some("Präsentation.pdf"), "application/pdf", 256, false)
    );
    let file = mime::attachment(&built.bytes, &info.part).expect("the attachment");
    assert_eq!(file.bytes, (0u8..=255).collect::<Vec<_>>());
    assert!(read.has_attachments());
}

#[test]
fn bcc_is_only_in_the_envelope_and_the_same_draft_builds_the_same_bytes() {
    let built = mime::build(&outgoing()).expect("built");
    assert_eq!(built.envelope.from, "juergen@example.de");
    assert_eq!(built.envelope.recipients, ["anna@example.org", "carla@example.org", "secret@example.net"], "each once");
    let text = String::from_utf8_lossy(&built.bytes).to_lowercase();
    assert!(!text.contains("secret@example.net") && !text.contains("bcc:"), "no trace of the blind copy");
    assert_eq!(mime::build(&outgoing()).expect("again").bytes, built.bytes, "deterministic");
}

#[test]
fn nothing_sneaks_into_headers_or_the_envelope() {
    let mut sneaky = outgoing();
    sneaky.subject = "Hi\r\nBcc: eve@example.net".into();
    let built = mime::build(&sneaky).expect("built");
    let text = String::from_utf8_lossy(&built.bytes);
    assert!(!text.lines().any(|line| line.to_ascii_lowercase().starts_with("bcc:")), "no header from the subject");
    assert_eq!(mime::read(&built.bytes).expect("read").subject, "Hi  Bcc: eve@example.net");

    let mut bad = outgoing();
    bad.to.push(address(None, "anna@example.org>, eve@example.net"));
    assert_eq!(mime::build(&bad), Err(MimeError::InvalidAddress));
    let mut bad = outgoing();
    bad.message_id = "<x@y>\r\nX: 1".into();
    assert_eq!(mime::build(&bad), Err(MimeError::InvalidMessageId));
    let mut none = outgoing();
    (none.to, none.cc, none.bcc) = (vec![], vec![], vec![]);
    assert_eq!(mime::build(&none), Err(MimeError::NoRecipients));
}

#[test]
fn a_reply_points_at_the_message_it_answers() {
    let read = mime::read(&fixture("attachments.eml")).expect("read");
    let reply = read.reply().expect("it has a Message-ID");
    assert_eq!(reply.in_reply_to, "docs-2@example.de");
    assert_eq!(reply.references, ["docs-0@example.de", "docs-1@example.org", "docs-2@example.de"]);
}

#[test]
fn html_mail_becomes_text_and_its_tracking_image_is_never_fetched() {
    // A tracker on this computer: if reading fetched the image, it would get a connection.
    let tracker = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    tracker.set_nonblocking(true).expect("non-blocking");
    let port = tracker.local_addr().expect("address").port();
    let raw = format!(
        "From: news@example.com\r\nTo: anna@example.org\r\nSubject: News\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <b>Anna</b></p>\
         <img src=\"http://127.0.0.1:{port}/pixel.gif\"><link rel=\"stylesheet\" href=\"http://127.0.0.1:{port}/s.css\">\r\n"
    );
    let read = mime::read(raw.as_bytes()).expect("read");
    assert!(read.from_html);
    assert!(read.text.contains("Hello") && read.text.contains("Anna"), "{:?}", read.text);
    assert!(!read.text.contains('<') && !read.text.contains("127.0.0.1"), "{:?}", read.text);
    let message = mime::read(raw.as_bytes()).expect("read").into_message(place(), true);
    assert!(message.text.is_some_and(|text| !text.contains("pixel")));
    match tracker.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        other => panic!("the tracker was contacted: {other:?}"),
    }
}

#[test]
fn newsletters_are_marked_and_their_html_is_only_text() {
    let read = mime::read(&fixture("newsletter_html.eml")).expect("read");
    assert_eq!(read.subject, "Neuigkeiten für dich");
    assert!(read.from_html);
    assert!(read.text.contains("Hallo Anna") && read.text.contains("Weiterlesen"), "{:?}", read.text);
    for hidden in ["tracker", "pixel", "track()", "color", "https://"] {
        assert!(!read.text.contains(hidden), "{hidden} in {:?}", read.text);
    }
    assert_eq!(read.traits.list_id.as_deref(), Some("example news <news.example.com>"));
    assert!(read.traits.list_unsubscribe);
    assert_eq!(read.traits.precedence.as_deref(), Some("bulk"));
    assert_eq!(read.traits.auto_submitted.as_deref(), Some("auto-generated"));
    assert!(!read.invitation && !read.has_attachments());
}

#[test]
fn invitations_are_found_and_the_text_is_not_the_calendar() {
    let read = mime::read(&fixture("invitation.eml")).expect("read");
    assert!(read.invitation);
    assert_eq!(read.text.trim(), "Du bist zur Planung eingeladen.");
    assert!(!read.from_html);
    assert_eq!(read.cc, [address(None, "carla@example.org")]);
    assert_eq!(read.traits, Default::default(), "a person's mail");
}

#[test]
fn attachments_are_listed_cleaned_and_fetched_one_by_one() {
    let raw = fixture("attachments.eml");
    let read = mime::read(&raw).expect("read");
    assert_eq!(read.from, Some(address(Some("Jürgen Müller"), "juergen@example.de")));
    assert_eq!(read.reply_to, [address(None, "team@example.de")]);
    assert!(read.text.starts_with("Hallo Anna,\n\nanbei die Präsentation."), "{:?}", read.text);

    let names: Vec<_> =
        read.attachments.iter().map(|a| (a.filename.clone(), a.content_type.clone(), a.inline)).collect();
    assert_eq!(
        names,
        [
            (None, "image/png".to_owned(), true),
            (Some("Präsentation.pdf".to_owned()), "application/pdf".to_owned(), false),
            (Some("invoicefdp.exe".to_owned()), "application/octet-stream".to_owned(), false),
        ],
        "the direction override that disguises .exe as .pdf is gone"
    );
    let pdf = mime::attachment(&raw, &read.attachments[1].part).expect("the pdf");
    assert!(pdf.bytes.starts_with(b"%PDF-1.4"));
    assert_eq!(pdf.filename.as_deref(), Some("Präsentation.pdf"));
    assert_eq!(mime::attachment(&raw, "0"), Err(MimeError::NoSuchPart), "the root is no attachment");
    assert_eq!(mime::attachment(&raw, "../1"), Err(MimeError::NoSuchPart));
}

fn place() -> Place {
    Place {
        id: "m:1".into(),
        account: "mail:anna@example.org".into(),
        folder: "f:1".into(),
        thread_id: "t:1".into(),
        flags: vec![Flag::Seen],
        received: when(),
    }
}

#[test]
fn a_read_message_becomes_the_core_s_untrusted_message() {
    let read = mime::read(&fixture("attachments.eml")).expect("read");
    let message = read.clone().into_message(place(), false);
    assert_eq!((message.summary.id.as_str(), message.summary.folder.as_str()), ("m:1", "f:1"));
    assert!(message.summary.untrusted && message.summary.has_attachments);
    assert_eq!(message.text, None, "no body asked for");
    assert!(message.summary.snippet.starts_with("Hallo Anna, anbei die Präsentation."));
    assert_eq!(message.summary.date, Utc.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).single().expect("date"));
    assert_eq!(message.summary.flags, [Flag::Seen]);

    let headless = mime::read(b"From: a@example.org\r\nSubject: x\r\n\r\n").expect("headers only");
    assert_eq!(headless.into_message(place(), true).summary.date, when(), "no Date: falls back to received");
    assert_eq!(mime::read(b""), Err(MimeError::Unreadable));
}
