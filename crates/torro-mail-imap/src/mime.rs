//! Reading and building messages (task M2.4).
//!
//! **Reading** ([`read`]) with `mail-parser`: headers, the text (the plain part first; an HTML-only
//! message is turned into text, its HTML is never handed out and nothing it names is fetched, so a
//! tracking image stays unseen), attachments without their content, `text/calendar` invitations
//! and the headers that mark list and automatic mail, `X-GitHub-Reason` among them (task E5.6).
//! Parsing is pure: no network, no files.
//! [`ReadMessage::into_message`] makes the `torro_mail::Message` the core hands out, once the core
//! knows the message's id and place. [`attachment`] fetches one attachment's bytes.
//!
//! **Building** ([`build`]) with `mail-builder`: a draft whose references the core has resolved
//! ([`Outgoing`]: addresses, the original's `Message-ID` for a reply, attachment bytes) becomes the
//! MIME bytes to send and the SMTP [`Envelope`]. The core gives the `Message-ID` and the date, so
//! building the same draft twice gives the same bytes (the fingerprint of an approval, task M3.3).
//! Bcc recipients are in the envelope only, never in a header, so neither the recipients nor the
//! Sent copy show them. An answer to an invitation (task E8.3b) carries its iCalendar object as a
//! second form of the text ([`Outgoing::calendar`]): `multipart/alternative` with the plain text
//! and `text/calendar; method=REPLY`, as iMIP (RFC 6047, 2.4) asks, so calendars of every kind
//! read it as an answer and not as a file.
//!
//! Everything read comes from outside: names, subjects, file names and text are data, never
//! instructions, and a file name is never used as a path as it is.

use std::fmt;

use chrono::{DateTime, Utc};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as BuilderAddress;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::message_id::MessageId;
use mail_builder::mime::MimePart;
use mail_parser::{Address, HeaderValue, MessageParser, MessagePart, MimeHeaders, PartType};
use torro_mail::{AttachmentInfo, Flag, MailAddress, Message, MessageSummary, MessageTraits};

use crate::smtp::Envelope;

/// The longest snippet, in characters.
const SNIPPET_MAX: usize = 200;
/// The most message ids a reply's `References:` keeps (the first and the latest), so long threads
/// do not grow the header without end (RFC 5322, 3.6.4).
const REFERENCES_MAX: usize = 20;

/// Why a message could not be read or built. Carries no content of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeError {
    /// The bytes are not a message (no headers found).
    Unreadable,
    /// The message has no attachment with this part.
    NoSuchPart,
    /// The sender or a recipient is not one plain address.
    InvalidAddress,
    /// The message has no recipient.
    NoRecipients,
    /// The `Message-ID` (or one to refer to) is empty or holds spaces, angle brackets or line
    /// breaks.
    InvalidMessageId,
    /// Writing the MIME bytes failed.
    Write,
}

impl fmt::Display for MimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MimeError::Unreadable => "the message could not be read",
            MimeError::NoSuchPart => "the message has no such attachment",
            MimeError::InvalidAddress => "the sender or a recipient is not a valid address",
            MimeError::NoRecipients => "the message has no recipient",
            MimeError::InvalidMessageId => "the message id is not valid",
            MimeError::Write => "the message could not be written",
        })
    }
}

impl std::error::Error for MimeError {}

/// A message as read from its bytes: everything the core keeps and hands out, before it knows the
/// message's id and folder. All of it is untrusted data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadMessage {
    /// `Message-ID:` without angle brackets.
    pub message_id: Option<String>,
    /// `In-Reply-To:` (the first id) without angle brackets.
    pub in_reply_to: Option<String>,
    /// `References:` without angle brackets, oldest first.
    pub references: Vec<String>,
    pub from: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub reply_to: Vec<MailAddress>,
    /// `Subject:`, empty when there is none.
    pub subject: String,
    /// `Date:`; `None` when missing or broken (the core then takes when the server got it).
    pub date: Option<DateTime<Utc>>,
    /// The plain text part, or the HTML part turned into text; line ends `\n`. Empty when the
    /// message has no text (or only its headers were read).
    pub text: String,
    /// `text` was made from HTML.
    pub from_html: bool,
    /// The attachments, inline parts included, without content.
    pub attachments: Vec<AttachmentInfo>,
    pub traits: MessageTraits,
    /// A part is an invitation (`text/calendar` or `application/ics`).
    pub invitation: bool,
}

/// Where a message is and what the core knows of it beyond its bytes: the parts of
/// `torro_mail::MessageSummary` that [`read`] cannot know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The core's id (`m:…`).
    pub id: String,
    /// The account (`mail:<address>`).
    pub account: String,
    /// The folder's id (`f:…`).
    pub folder: String,
    /// The thread, as the core works it out.
    pub thread_id: String,
    /// The flags on the server.
    pub flags: Vec<Flag>,
    /// When the server got the message (`INTERNALDATE`), used when `Date:` is missing or broken.
    pub received: DateTime<Utc>,
}

/// The headers a reply carries to point at the message it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The answered message's `Message-ID` (without angle brackets).
    pub in_reply_to: String,
    /// Its `References:` plus its own id, oldest first, at most 20 (the first and the latest).
    pub references: Vec<String>,
}

/// One attachment with its content, for `mail.get_attachment` and for forwarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The name the message gives it, control and direction characters removed; never use it as
    /// a path as it is.
    pub filename: Option<String>,
    /// `type/subtype` in small letters.
    pub content_type: String,
    /// The decoded content.
    pub bytes: Vec<u8>,
}

/// A message to build, with every reference resolved by the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The account's address and sender name (`From:` and `MAIL FROM`).
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    /// Blind copies: only in the envelope.
    pub bcc: Vec<MailAddress>,
    /// Line breaks become spaces.
    pub subject: String,
    /// The plain text; any line ends.
    pub body: String,
    /// The account's signature, put under the text after the separator `-- ` (RFC 3676); blank
    /// for none.
    pub signature: Option<String>,
    /// Set for a reply ([`ReadMessage::reply`] of the answered message).
    pub reply: Option<Reply>,
    pub attachments: Vec<OutgoingAttachment>,
    /// An iCalendar object to send beside the text (iMIP, task E8.3b); `None` for every other
    /// message.
    pub calendar: Option<CalendarPart>,
    /// The `Message-ID` the core picked, without angle brackets: `<random>@<domain of from>`.
    pub message_id: String,
    /// `Date:`.
    pub date: DateTime<Utc>,
}

/// A file to attach, already read by the core (security rule 1 of M2.1 checked there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingAttachment {
    /// The name shown to the recipients; line breaks and path parts are not checked here.
    pub filename: String,
    /// `type/subtype`; anything else becomes `application/octet-stream`.
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// An iCalendar object sent as a form of the text (iMIP, RFC 6047): `text/calendar` with the
/// `method` of the object and `charset=utf-8`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarPart {
    /// The iTIP method, as the object's `METHOD:` says: `REPLY`. Anything but letters makes
    /// [`build`] fail with [`MimeError::Write`], so it cannot add parameters or headers.
    pub method: String,
    /// The whole iCalendar object (`BEGIN:VCALENDAR` … `END:VCALENDAR`); any line ends.
    pub ics: String,
}

/// A built message: the bytes to send (and to file as the Sent copy) and the SMTP envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// RFC 5322 with CRLF line ends, 7-bit (non-ASCII text is encoded).
    pub bytes: Vec<u8>,
    pub envelope: Envelope,
}

/// Reads a whole message, or only its headers (then `text` is empty and `attachments` too).
/// [`MimeError::Unreadable`] when the bytes hold no headers. Never fetches anything.
pub fn read(raw: &[u8]) -> Result<ReadMessage, MimeError> {
    let message = MessageParser::default().parse(raw).ok_or(MimeError::Unreadable)?;
    let (text, from_html) = body_text(&message);
    Ok(ReadMessage {
        message_id: message.message_id().map(str::to_owned),
        in_reply_to: ids(message.in_reply_to()).into_iter().next(),
        references: ids(message.references()),
        from: message.from().and_then(|from| addresses(from).into_iter().next()),
        to: message.to().map(addresses).unwrap_or_default(),
        cc: message.cc().map(addresses).unwrap_or_default(),
        reply_to: message.reply_to().map(addresses).unwrap_or_default(),
        subject: message.subject().map(|subject| one_line(subject).trim().to_owned()).unwrap_or_default(),
        date: message.date().and_then(|date| DateTime::from_timestamp(date.to_timestamp(), 0)),
        text,
        from_html,
        attachments: message
            .attachments
            .iter()
            .filter_map(|&id| message.parts.get(id as usize).map(|part| attachment_info(id, part)))
            .collect(),
        traits: traits(&message),
        invitation: message.parts.iter().any(is_invitation),
    })
}

impl ReadMessage {
    /// The start of the text on one line (white space collapsed), at most 200 characters.
    pub fn snippet(&self) -> String {
        self.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(SNIPPET_MAX).collect()
    }

    /// It has attachments that are not inline parts.
    pub fn has_attachments(&self) -> bool {
        self.attachments.iter().any(|attachment| !attachment.inline)
    }

    /// The headers for answering this message; `None` when it has no `Message-ID`.
    pub fn reply(&self) -> Option<Reply> {
        let id = self.message_id.clone().filter(|id| valid_message_id(id))?;
        let mut references = if self.references.is_empty() {
            self.in_reply_to.iter().cloned().collect::<Vec<_>>()
        } else {
            self.references.clone()
        };
        references.retain(|reference| valid_message_id(reference) && *reference != id);
        references.push(id.clone());
        if references.len() > REFERENCES_MAX {
            references.drain(1..references.len() - (REFERENCES_MAX - 1));
        }
        Some(Reply { in_reply_to: id, references })
    }

    /// The message as the core hands it out. `include_body: false` leaves the text out (lists,
    /// `mail.get_message` without `include_body`); the snippet stays.
    pub fn into_message(self, place: Place, include_body: bool) -> Message {
        let snippet = self.snippet();
        let has_attachments = self.has_attachments();
        Message {
            summary: MessageSummary {
                id: place.id,
                account: place.account,
                folder: place.folder,
                thread_id: place.thread_id,
                from: self.from,
                to: self.to,
                subject: self.subject,
                date: self.date.unwrap_or(place.received),
                snippet,
                flags: place.flags,
                has_attachments,
                // Read from the calendar part by the core, which keeps it in its store (task E8.3b).
                invite: None,
                untrusted: true,
            },
            cc: self.cc,
            reply_to: self.reply_to,
            text: include_body.then_some(self.text),
            from_html: self.from_html,
            attachments: self.attachments,
            traits: self.traits,
            invitation: self.invitation,
        }
    }
}

/// The attachment `part` (as [`ReadMessage::attachments`] names it) of the message `raw`, with
/// its content. Only attachments are handed out, no other part. [`MimeError::NoSuchPart`] when the
/// message has none by that name.
pub fn attachment(raw: &[u8], part: &str) -> Result<Attachment, MimeError> {
    let message = MessageParser::default().parse(raw).ok_or(MimeError::Unreadable)?;
    let id = part.parse::<u32>().map_err(|_| MimeError::NoSuchPart)?;
    if !message.attachments.contains(&id) {
        return Err(MimeError::NoSuchPart);
    }
    let part = message.parts.get(id as usize).ok_or(MimeError::NoSuchPart)?;
    Ok(Attachment {
        filename: part.attachment_name().map(clean),
        content_type: content_type(part),
        bytes: part.contents().to_vec(),
    })
}

/// Builds `outgoing` into MIME bytes and its SMTP envelope. The same input gives the same bytes.
pub fn build(outgoing: &Outgoing) -> Result<Built, MimeError> {
    for address in std::iter::once(&outgoing.from).chain(&outgoing.to).chain(&outgoing.cc).chain(&outgoing.bcc) {
        if !valid_address(&address.address) {
            return Err(MimeError::InvalidAddress);
        }
    }
    if !valid_message_id(&outgoing.message_id) {
        return Err(MimeError::InvalidMessageId);
    }
    let envelope = envelope(outgoing);
    if envelope.recipients.is_empty() {
        return Err(MimeError::NoRecipients);
    }

    let mut builder = MessageBuilder::new()
        .message_id(outgoing.message_id.as_str())
        .date(outgoing.date.timestamp())
        .from(builder_address(&outgoing.from))
        .subject(one_line(&outgoing.subject));
    if !outgoing.to.is_empty() {
        builder = builder.to(builder_list(&outgoing.to));
    }
    if !outgoing.cc.is_empty() {
        builder = builder.cc(builder_list(&outgoing.cc));
    }
    if let Some(reply) = &outgoing.reply {
        if !valid_message_id(&reply.in_reply_to) || !reply.references.iter().all(|id| valid_message_id(id)) {
            return Err(MimeError::InvalidMessageId);
        }
        builder = builder
            .in_reply_to(reply.in_reply_to.as_str())
            .references(MessageId::new_list(reply.references.iter().map(String::as_str)));
    }
    let text = crlf(&with_signature(&outgoing.body, outgoing.signature.as_deref()));
    // `mail-builder` makes up a random boundary; one from the Message-ID keeps the bytes the same
    // for the same draft. `=_` never occurs in quoted-printable or base64 text, so the boundary
    // cannot appear inside a part.
    let boundary: String = outgoing.message_id.chars().filter(char::is_ascii_alphanumeric).take(48).collect();
    let text_part = match &outgoing.calendar {
        None => MimePart::new("text/plain", text.clone()),
        Some(calendar) => {
            if calendar.method.is_empty() || !calendar.method.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(MimeError::Write);
            }
            let kind = ContentType::new("text/calendar")
                .attribute("method", calendar.method.to_ascii_uppercase())
                .attribute("charset", "utf-8");
            let parts = vec![MimePart::new("text/plain", text.clone()), MimePart::new(kind, crlf(&calendar.ics))];
            let kind =
                ContentType::new("multipart/alternative").attribute("boundary", format!("=_torro_alt_{boundary}"));
            MimePart::new(kind, parts)
        }
    };
    builder = if outgoing.attachments.is_empty() && outgoing.calendar.is_none() {
        builder.text_body(text)
    } else if outgoing.attachments.is_empty() {
        builder.body(text_part)
    } else {
        let mut parts = vec![text_part];
        for file in &outgoing.attachments {
            parts.push(
                MimePart::new(outgoing_content_type(&file.content_type), file.bytes.as_slice())
                    .attachment(one_line(&file.filename)),
            );
        }
        let kind = ContentType::new("multipart/mixed").attribute("boundary", format!("=_torro_{boundary}"));
        builder.body(MimePart::new(kind, parts))
    };
    let bytes = builder.write_to_vec().map_err(|_| MimeError::Write)?;
    Ok(Built { bytes, envelope })
}

/// `From` and every recipient once (addresses compared without case).
fn envelope(outgoing: &Outgoing) -> Envelope {
    let mut recipients: Vec<String> = Vec::new();
    for address in outgoing.to.iter().chain(&outgoing.cc).chain(&outgoing.bcc) {
        let address = address.address.trim();
        if !recipients.iter().any(|known| known.eq_ignore_ascii_case(address)) {
            recipients.push(address.to_owned());
        }
    }
    Envelope { from: outgoing.from.address.trim().to_owned(), recipients }
}

/// The text to show: the first plain text part, else the first HTML part as text.
fn body_text(message: &mail_parser::Message<'_>) -> (String, bool) {
    let body_parts =
        message.text_body.iter().chain(&message.html_body).filter_map(|&id| message.parts.get(id as usize));
    let mut html = None;
    for part in body_parts {
        match &part.body {
            PartType::Text(text) if is_plain(part) => return (lines(text), false),
            PartType::Html(markup) if html.is_none() => html = Some(markup),
            _ => {}
        }
    }
    match html {
        // `html_to_text` only drops tags (and the content of style and script); images, links
        // and stylesheets are never loaded.
        Some(markup) => (lines(&mail_parser::decoders::html::html_to_text(markup)), true),
        None => (String::new(), false),
    }
}

/// A text part that is plain text, not a calendar or another text type.
fn is_plain(part: &MessagePart<'_>) -> bool {
    part.content_type().is_none_or(|kind| {
        kind.c_type.eq_ignore_ascii_case("text")
            && kind.c_subtype.as_deref().is_none_or(|subtype| subtype.eq_ignore_ascii_case("plain"))
    })
}

fn is_invitation(part: &MessagePart<'_>) -> bool {
    part.is_content_type("text", "calendar") || part.is_content_type("application", "ics")
}

fn attachment_info(id: u32, part: &MessagePart<'_>) -> AttachmentInfo {
    let disposition = part.content_disposition();
    let inline = disposition.map_or(part.content_id().is_some(), |disposition| disposition.is_inline());
    AttachmentInfo {
        part: id.to_string(),
        filename: part.attachment_name().map(clean).filter(|name| !name.is_empty()),
        content_type: content_type(part),
        size: part.len() as u64,
        inline,
    }
}

fn content_type(part: &MessagePart<'_>) -> String {
    match part.content_type() {
        Some(kind) => match kind.c_subtype.as_deref() {
            Some(subtype) => format!("{}/{}", kind.c_type, subtype).to_ascii_lowercase(),
            None => kind.c_type.to_ascii_lowercase(),
        },
        None => match part.body {
            PartType::Text(_) => "text/plain".into(),
            PartType::Html(_) => "text/html".into(),
            PartType::Message(_) => "message/rfc822".into(),
            _ => "application/octet-stream".into(),
        },
    }
}

fn traits(message: &mail_parser::Message<'_>) -> MessageTraits {
    let raw = |name: &str| {
        message.header_raw(name).map(|value| one_line(value).trim().to_lowercase()).filter(|value| !value.is_empty())
    };
    MessageTraits {
        list_id: raw("List-Id"),
        list_unsubscribe: message.header_raw("List-Unsubscribe").is_some(),
        precedence: raw("Precedence"),
        // `auto-replied; owner-email=…`: only the keyword counts.
        auto_submitted: raw("Auto-Submitted")
            .and_then(|value| value.split(';').next().map(|keyword| keyword.trim().to_owned()))
            .filter(|keyword| !keyword.is_empty()),
        github: raw("X-GitHub-Reason").filter(|reason| github_reason(reason)),
    }
}

/// A reason as GitHub writes them (`review_requested`, `ci_activity`): small letters and `_`, at
/// most [`GITHUB_REASON_MAX`] of them. Anything else is no reason GitHub sent.
fn github_reason(reason: &str) -> bool {
    reason.len() <= GITHUB_REASON_MAX && reason.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// The longest `X-GitHub-Reason` kept; GitHub's longest is `security_advisory_credit` (24).
const GITHUB_REASON_MAX: usize = 40;

fn addresses(list: &Address<'_>) -> Vec<MailAddress> {
    list.iter()
        .filter_map(|entry| {
            let address = entry.address()?.trim();
            (!address.is_empty()).then(|| MailAddress {
                name: entry.name().map(|name| clean(name).trim().to_owned()).filter(|name| !name.is_empty()),
                address: clean(address),
            })
        })
        .collect()
}

/// Message ids of `In-Reply-To:` or `References:`.
fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(id) => vec![id.to_string()],
        HeaderValue::TextList(list) => list.iter().map(|id| id.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// One plain address: `local@domain`, no spaces, controls, angle brackets, commas or quotes, so
/// nothing can be added to a header or to `RCPT TO`.
fn valid_address(address: &str) -> bool {
    let address = address.trim();
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !address.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | '"' | ';'))
}

/// A message id without angle brackets: not empty, no white space, controls or brackets.
fn valid_message_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 250
        && !id.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
}

fn builder_address(address: &MailAddress) -> BuilderAddress<'_> {
    let name = address.name.as_deref().map(one_line).filter(|name| !name.trim().is_empty());
    BuilderAddress::new_address(name, address.address.trim())
}

fn builder_list(addresses: &[MailAddress]) -> BuilderAddress<'_> {
    BuilderAddress::new_list(addresses.iter().map(builder_address).collect())
}

/// `type/subtype` made of token characters, else `application/octet-stream`.
fn outgoing_content_type(kind: &str) -> String {
    let token =
        |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c));
    match kind.trim().split_once('/') {
        Some((main, sub)) if token(main) && token(sub) => kind.trim().to_ascii_lowercase(),
        _ => "application/octet-stream".into(),
    }
}

fn with_signature(body: &str, signature: Option<&str>) -> String {
    match signature.map(str::trim_end).filter(|signature| !signature.trim().is_empty()) {
        Some(signature) => format!("{}\n\n-- \n{signature}\n", body.trim_end()),
        None => body.to_owned(),
    }
}

/// Line ends as CRLF, whatever came in.
fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// Line ends as `\n`.
fn lines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Line breaks (and other controls) as spaces: for headers.
fn one_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Text from outside without controls and the invisible characters that turn what is shown
/// around (`invoice\u{202E}fdp.exe` reads as `invoiceexe.pdf`).
fn clean(text: &str) -> String {
    text.chars().filter(|&c| !c.is_control() && !is_invisible(c)).collect()
}

fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_are_tokens_or_octet_stream() {
        assert_eq!(outgoing_content_type("Application/PDF"), "application/pdf");
        assert_eq!(outgoing_content_type("text/plain; charset=utf-8"), "application/octet-stream");
        assert_eq!(outgoing_content_type("text\r\nBcc: x"), "application/octet-stream");
        assert_eq!(outgoing_content_type(""), "application/octet-stream");
    }

    #[test]
    fn addresses_and_ids_cannot_carry_more_than_one_value() {
        assert!(valid_address("anna@example.org") && valid_address("a.b+c@sub.example.org"));
        for bad in ["anna", "@example.org", "anna@", "a@b@c", "anna@example.org>, eve@x", "an na@x.org", "a\r\n@x.org"]
        {
            assert!(!valid_address(bad), "{bad:?}");
        }
        assert!(valid_message_id("abc.123@example.org"));
        for bad in ["", "a b@x", "<a@x>", "a\r\n@x"] {
            assert!(!valid_message_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn signatures_follow_the_separator_and_line_ends_become_crlf() {
        assert_eq!(with_signature("Hi\n\n", Some("Anna\n")), "Hi\n\n-- \nAnna\n");
        assert_eq!(with_signature("Hi", Some("  ")), "Hi");
        assert_eq!(crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
        assert_eq!(clean("invoice\u{202E}fdp.exe\n"), "invoicefdp.exe");
    }

    fn answer(calendar: Option<CalendarPart>) -> Outgoing {
        Outgoing {
            from: MailAddress { name: Some("Anna".into()), address: "anna@example.org".into() },
            to: vec![MailAddress { name: None, address: "jonas@example.org".into() }],
            cc: vec![],
            bcc: vec![],
            subject: "Accepted: Planning".into(),
            body: "Anna has accepted.".into(),
            signature: None,
            reply: None,
            attachments: vec![],
            calendar,
            message_id: "reply.1@example.org".into(),
            date: Utc::now(),
        }
    }

    #[test]
    fn an_answer_to_an_invitation_carries_its_calendar_as_a_form_of_the_text() {
        let ics = "BEGIN:VCALENDAR\nMETHOD:REPLY\nBEGIN:VEVENT\nUID:planning-1\nEND:VEVENT\nEND:VCALENDAR\n";
        let built = build(&answer(Some(CalendarPart { method: "REPLY".into(), ics: ics.into() }))).expect("built");
        let raw = String::from_utf8(built.bytes.clone()).expect("ascii");
        assert!(raw.contains("multipart/alternative"), "{raw}");
        assert!(raw.contains("method=REPLY") || raw.contains("method=\"REPLY\""), "{raw}");
        assert!(raw.contains("charset=utf-8") || raw.contains("charset=\"utf-8\""), "{raw}");
        let message = MessageParser::default().parse(&built.bytes).expect("parsed");
        let calendar = message.parts.iter().find(|part| is_invitation(part)).expect("a calendar part");
        let read = std::str::from_utf8(calendar.contents()).expect("text");
        assert!(read.contains("UID:planning-1\r\n"), "line ends are CRLF: {read:?}");
        assert_eq!(calendar.content_type().and_then(|kind| kind.attribute("method")), Some("REPLY"));
        let read = super::read(&built.bytes).expect("read");
        assert_eq!((read.text.trim(), read.invitation), ("Anna has accepted.", true));
        assert_eq!(built.envelope.recipients, ["jonas@example.org"]);

        let plain = build(&answer(None)).expect("built");
        assert!(!String::from_utf8_lossy(&plain.bytes).contains("multipart"), "a plain text stays one part");
        let header = CalendarPart { method: "REPLY\r\nBcc: eve@x".into(), ics: ics.into() };
        assert_eq!(build(&answer(Some(header))), Err(MimeError::Write), "the method carries no header");
    }

    #[test]
    fn a_github_mail_keeps_its_reason_list_and_message_id() {
        let read = read(include_bytes!("../tests/fixtures/mime/github_review_requested.eml")).expect("read");
        assert_eq!(read.from.as_ref().map(|from| from.address.as_str()), Some("notifications@github.com"));
        assert_eq!(read.traits.github.as_deref(), Some("review_requested"));
        assert_eq!(read.traits.list_id.as_deref(), Some("torro/workplace <workplace.torro.github.com>"));
        assert_eq!(read.message_id.as_deref(), Some("torro/workplace/pull/42/issue_event/1234567@github.com"));
        assert_eq!(read.in_reply_to.as_deref(), Some("torro/workplace/pull/42@github.com"));
        assert!(read.text.contains("requested your review"), "{}", read.text);

        let odd = |reason: &str| {
            let raw = format!("From: a@b.example\r\nX-GitHub-Reason: {reason}\r\nSubject: x\r\n\r\nText\r\n");
            super::read(raw.as_bytes()).expect("read").traits.github
        };
        assert_eq!(odd("Mention"), Some("mention".into()), "lowercased like the other traits");
        assert_eq!(odd("review requested"), None);
        assert_eq!(odd(&"a".repeat(GITHUB_REASON_MAX + 1)), None);
        assert_eq!(super::read(b"Subject: x\r\n\r\nText\r\n").expect("read").traits.github, None);
    }

    #[test]
    fn long_reference_chains_keep_the_first_and_the_latest() {
        let read = ReadMessage {
            message_id: Some("new@x".into()),
            in_reply_to: None,
            references: (0..30).map(|n| format!("{n}@x")).collect(),
            from: None,
            to: vec![],
            cc: vec![],
            reply_to: vec![],
            subject: String::new(),
            date: None,
            text: String::new(),
            from_html: false,
            attachments: vec![],
            traits: MessageTraits::default(),
            invitation: false,
        };
        let reply = read.reply().expect("a reply");
        assert_eq!(reply.in_reply_to, "new@x");
        assert_eq!(reply.references.len(), REFERENCES_MAX);
        assert_eq!(reply.references.first().map(String::as_str), Some("0@x"));
        assert_eq!(reply.references[1], "12@x", "31 ids: the first and the latest 19");
        assert_eq!(reply.references.last().map(String::as_str), Some("new@x"));
    }
}
