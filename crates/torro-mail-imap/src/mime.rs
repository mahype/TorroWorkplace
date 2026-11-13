//! Reading and building messages (task M2.4).
//!
//! **Reading** ([`read`]) with `mail-parser`: headers, the text (the plain part first; an HTML-only
//! message is turned into text, and nothing the HTML names is fetched, so a tracking image stays
//! unseen), attachments without their content, `text/calendar` invitations and the headers that
//! mark list and automatic mail, `X-GitHub-Reason` among them (task E5.6). Parsing is pure: no
//! network, no files.
//!
//! **HTML** (task M8.1): beside the text, the first HTML part is read and cleaned with `ammonia`
//! through an allow list ([`clean_html`]), so the person can see the mail as it was written: text
//! markup, tables, links, images and styles stay; scripts, forms, frames, objects, `svg`, `base`,
//! `meta`, `link` and event attributes go. The cleaned HTML is the only HTML the core hands out; it
//! is kept up to [`HTML_MAX`] bytes. Old layout attributes (`cellpadding`, `bgcolor`, `<font>` …)
//! become inline CSS while cleaning (task M9.5, [`legacy`]). The part as it is ([`ReadMessage::html_source`]) serves the
//! core only to send a stored draft as it was written (issue #125). Cleaning fetches nothing either.
//! [`ReadMessage::into_message`] makes the `torro_mail::Message` the core hands out, once the core
//! knows the message's id and place. [`attachment`] fetches one attachment's bytes.
//!
//! **Building** ([`build`]) with `mail-builder`: a draft whose references the core has resolved
//! ([`Outgoing`]: addresses, the original's `Message-ID` for a reply, attachment bytes) becomes the
//! MIME bytes to send and the SMTP [`Envelope`]. The core gives the `Message-ID` and the date, so
//! building the same draft twice gives the same bytes (the fingerprint of an approval, task M3.3).
//! Bcc recipients are in the envelope only, never in a header, so neither the recipients nor the
//! Sent copy show them. Formatted mail (issue #125, [`Outgoing::html`]) goes as
//! `multipart/alternative`: the plain text made from the HTML, then the HTML, both with the
//! signature in front of the quoted history. An answer to an invitation (task E8.3b) carries its iCalendar object as a
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
use torro_mail::{AttachmentInfo, Flag, MailAddress, Message, MessageSummary, MessageTraits, html};

use crate::smtp::Envelope;

/// The longest snippet, in characters.
const SNIPPET_MAX: usize = 200;
/// The largest cleaned HTML kept and handed out (2 MiB); a larger mail is shown as its text.
pub const HTML_MAX: usize = 2 * 1024 * 1024;
/// The largest HTML part that is cleaned at all (16 MiB): cleaning parses the whole part, and
/// what is left of a larger one would hardly fit into [`HTML_MAX`].
const HTML_INPUT_MAX: usize = 16 * 1024 * 1024;
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
    /// The first HTML part, cleaned ([`clean_html`]); `None` when the message has none (or only
    /// its headers were read), or when the cleaned HTML is larger than [`HTML_MAX`]. The HTML the
    /// core keeps and hands out (`Message::html`).
    pub html: Option<String>,
    /// The first HTML part of the text as it is, uncleaned; `None` when there is none. Only for
    /// the core, to send a stored draft as it was written ([`Outgoing::html`], cleaned there,
    /// issue #125), and to hand a draft back to write on (`mail.get_draft`, issue #502), cleaned
    /// as a saved draft is: never handed out as it is, and nothing it names is fetched.
    pub html_source: Option<String>,
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
    /// The plain text; any line ends. Not used when `html` is given.
    pub body: String,
    /// The text as HTML (issue #125); `None` for plain text only. [`build`] cleans it
    /// (`torro_mail::html::sanitize`), puts the signature in front of the quoted history
    /// (`torro_mail::html::sign`) and sends `multipart/alternative`: the plain text made from that
    /// HTML (`torro_mail::html::to_text`), then the HTML.
    pub html: Option<String>,
    /// The account's signature, put under the text after the separator `-- ` (RFC 3676); blank
    /// for none.
    pub signature: Option<String>,
    /// `signature` uses the compose editor's safe formatting marks.
    pub signature_format: bool,
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
    /// Compose display choice retained independently of image dimensions.
    pub image_size: Option<torro_mail::ImageSize>,
    /// Stable Content-ID, also retained when this image is an ordinary attachment.
    pub content_id: Option<String>,
    pub inline: bool,
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
    let html_source = html_part(&message);
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
        html: html_source.as_deref().and_then(cleaned_html),
        html_source,
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
    /// `mail.get_message` without `include_body`); the snippet stays. The HTML is always left out:
    /// the core adds it only when it is asked for (`include_html`).
    pub fn into_message(self, place: Place, include_body: bool) -> Message {
        let snippet = self.snippet();
        let has_attachments = self.has_attachments();
        Message {
            warnings: Vec::new(),
            // Filled by the core in `mail.get_message` only (issue #672).
            external: None,
            summary: MessageSummary {
                caught: false,
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
                back: None,
                pile: None,
                // Decided by the core when it files the mail, not while the MIME is parsed
                // (task E12.1); the store keeps it.
                category: None,
                untrusted: true,
                pile_since: None,
                logo: None,
                suggested: Vec::new(),
            },
            cc: self.cc,
            reply_to: self.reply_to,
            text: include_body.then_some(self.text),
            from_html: self.from_html,
            html: None,
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

/// Decodes a bounded standalone image entity fetched with `Session::fetch_image_part`. Check the
/// decoded length before copying it, and never interpret a multipart or embedded message as an image.
pub fn image_attachment(raw: &[u8], limit: usize) -> Result<Attachment, MimeError> {
    standalone_attachment(raw, limit, true)
}

/// Decodes an ordinary standalone attachment with the same before-copy byte limit.
pub fn bounded_attachment(raw: &[u8], limit: usize) -> Result<Attachment, MimeError> {
    standalone_attachment(raw, limit, false)
}

fn standalone_attachment(raw: &[u8], limit: usize, image_only: bool) -> Result<Attachment, MimeError> {
    // A standalone leaf is an opaque attachment, not display text. Full message parsing may
    // expand a declared charset to four times the encoded body before we can inspect its size.
    // Parse only the bounded MIME header block and perform transfer decoding, never transcoding.
    let end = (0..raw.len().min(256 << 10))
        .find_map(|offset| {
            let tail = &raw[offset..];
            if tail.starts_with(b"\r\n\r\n") {
                Some(offset + 4)
            } else if tail.starts_with(b"\n\n") {
                Some(offset + 2)
            } else {
                None
            }
        })
        .ok_or(MimeError::Unreadable)?;
    let message = MessageParser::default().parse_headers(&raw[..end]).ok_or(MimeError::Unreadable)?;
    let part = message.parts.first().ok_or(MimeError::NoSuchPart)?;
    let kind = content_type(part);
    if kind.starts_with("multipart/")
        || (image_only && !kind.starts_with("image/"))
        || (!image_only
            && matches!(kind.as_str(), "text/plain" | "text/html")
            && part.content_disposition().is_none_or(|disposition| !disposition.is_attachment()))
    {
        return Err(MimeError::NoSuchPart);
    }
    let body = &raw[end..];
    let encoding = part.content_transfer_encoding().unwrap_or("7bit");
    let bytes = if encoding.eq_ignore_ascii_case("base64") {
        mail_parser::decoders::base64::base64_decode(body).ok_or(MimeError::Unreadable)?
    } else if encoding.eq_ignore_ascii_case("quoted-printable") {
        bounded_quoted_printable(body, limit)?
    } else if ["7bit", "8bit", "binary"].iter().any(|known| encoding.eq_ignore_ascii_case(known)) {
        if body.len() > limit {
            return Err(MimeError::NoSuchPart);
        }
        body.to_vec()
    } else {
        return Err(MimeError::Unreadable);
    };
    if bytes.len() > limit {
        return Err(MimeError::NoSuchPart);
    }
    // Transfer decoders may reserve from encoded length (notably quoted-printable). Keep the
    // final capacity equal to decoded length; raw + decoder + this final copy are pre-budgeted.
    let bytes = bytes.into_boxed_slice().into_vec();
    Ok(Attachment { filename: part.attachment_name().map(clean), content_type: kind, bytes })
}

/// Decode opaque QP bytes without normalizing mixed hard line endings. The general MIME
/// decoder remembers CRLF and can expand every subsequent bare LF, then grow its Vec before
/// a caller checks the decoded limit. This buffer is preallocated once within both limits;
/// every emit is checked before writing, so neither length nor capacity can grow past them.
fn bounded_quoted_printable(body: &[u8], limit: usize) -> Result<Vec<u8>, MimeError> {
    let capacity = body.len().min(limit);
    let mut out = Vec::new();
    out.try_reserve_exact(capacity).map_err(|_| MimeError::Unreadable)?;
    let emit = |out: &mut Vec<u8>, bytes: &[u8]| {
        if bytes.len() > capacity.saturating_sub(out.len()) {
            return Err(MimeError::NoSuchPart);
        }
        out.extend_from_slice(bytes);
        Ok(())
    };
    // Process one borrowed line at a time. RFC 2045 discards unencoded trailing spaces/tabs
    // and soft line breaks; encoded whitespace is literal and must survive.
    for line in body.split_inclusive(|byte| *byte == b'\n') {
        let (mut text, newline) = if let Some(text) = line.strip_suffix(b"\r\n") {
            (text, b"\r\n".as_slice())
        } else if let Some(text) = line.strip_suffix(b"\n") {
            (text, b"\n".as_slice())
        } else {
            (line, b"".as_slice())
        };
        while matches!(text.last(), Some(b' ' | b'\t')) {
            text = &text[..text.len() - 1];
        }
        let soft = !newline.is_empty() && text.ends_with(b"=");
        if soft {
            text = &text[..text.len() - 1];
        }
        let mut remaining = text;
        while let Some((&byte, rest)) = remaining.split_first() {
            if byte == b'=' {
                let Some((&first, rest)) = rest.split_first() else { return Err(MimeError::Unreadable) };
                let Some((&second, rest)) = rest.split_first() else { return Err(MimeError::Unreadable) };
                let decoded = mail_parser::decoders::quoted_printable::quoted_printable_decode_char(first, second)
                    .ok_or(MimeError::Unreadable)?;
                emit(&mut out, &[decoded])?;
                remaining = rest;
            } else {
                emit(&mut out, &[byte])?;
                remaining = rest;
            }
        }
        if !soft {
            emit(&mut out, newline)?;
        }
    }
    Ok(out)
}

/// Builds `outgoing` into MIME bytes and its SMTP envelope. The same input gives the same bytes.
/// Reads the compose size marker of a saved MIME part; unknown values are absent.
pub fn image_size(raw: &[u8], part: &str) -> Option<torro_mail::ImageSize> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.parts.get(part.parse::<usize>().ok()?)?;
    let value =
        part.headers.iter().find(|h| h.name.as_str().eq_ignore_ascii_case("X-Torro-Image-Size"))?.value.as_text()?;
    match value.trim() {
        "small" => Some(torro_mail::ImageSize::Small),
        "medium" => Some(torro_mail::ImageSize::Medium),
        "large" => Some(torro_mail::ImageSize::Large),
        "original" => Some(torro_mail::ImageSize::Original),
        _ => None,
    }
}

fn image_part<'a>(file: &'a OutgoingAttachment) -> MimePart<'a> {
    let mut part = MimePart::new(outgoing_content_type(&file.content_type), file.bytes.as_slice());
    if let Some(size) = file.image_size {
        let size = match size {
            torro_mail::ImageSize::Small => "small",
            torro_mail::ImageSize::Medium => "medium",
            torro_mail::ImageSize::Large => "large",
            torro_mail::ImageSize::Original => "original",
        };
        part = part.header("X-Torro-Image-Size", mail_builder::headers::raw::Raw::new(size));
    }
    part
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
    // Formatted mail: the signature into the HTML, and the plain text made from both, so the two
    // forms say the same.
    let signed = outgoing.html.as_deref().map(|markup| {
        if outgoing.signature_format {
            html::sign_formatted(markup, outgoing.signature.as_deref())
        } else {
            html::sign(markup, outgoing.signature.as_deref())
        }
    });
    let plain_signature = outgoing.signature.as_deref().map(|signature| {
        if outgoing.signature_format {
            html::to_text(&torro_mail::markup::to_html(signature, None))
        } else {
            signature.to_string()
        }
    });
    if let Some(markup) = &signed {
        for cid in html::image_refs(markup) {
            if outgoing.attachments.iter().filter(|a| a.inline && a.content_id.as_deref() == Some(&cid)).count() != 1 {
                return Err(MimeError::Write);
            }
        }
    }
    let text = crlf(&match &signed {
        Some(signed) => html::to_text(signed),
        None => with_signature(&outgoing.body, plain_signature.as_deref()),
    });
    // `mail-builder` makes up a random boundary; one from the Message-ID keeps the bytes the same
    // for the same draft. `=_` never occurs in quoted-printable or base64 text, so the boundary
    // cannot appear inside a part.
    let boundary: String = outgoing.message_id.chars().filter(char::is_ascii_alphanumeric).take(48).collect();
    let mut forms = vec![MimePart::new("text/plain", text.clone())];
    if let Some(signed) = &signed {
        let html_part = MimePart::new("text/html", crlf(&html_document(signed)));
        let mut related = vec![html_part];
        for image in outgoing.attachments.iter().filter(|file| file.inline) {
            let id = image
                .content_id
                .as_deref()
                .filter(|id| torro_mail::html::valid_cid(id))
                .ok_or(MimeError::InvalidMessageId)?;
            related.push(image_part(image).inline().cid(id));
        }
        if related.len() == 1 {
            forms.extend(related);
        } else {
            let kind = ContentType::new("multipart/related").attribute("boundary", format!("=_torro_rel_{boundary}"));
            forms.push(MimePart::new(kind, related));
        }
    }
    if let Some(calendar) = &outgoing.calendar {
        if calendar.method.is_empty() || !calendar.method.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(MimeError::Write);
        }
        let kind = ContentType::new("text/calendar")
            .attribute("method", calendar.method.to_ascii_uppercase())
            .attribute("charset", "utf-8");
        forms.push(MimePart::new(kind, crlf(&calendar.ics)));
    }
    let alternative = forms.len() > 1;
    let text_part = if alternative {
        let kind = ContentType::new("multipart/alternative").attribute("boundary", format!("=_torro_alt_{boundary}"));
        MimePart::new(kind, forms)
    } else {
        MimePart::new("text/plain", text.clone())
    };
    let attached: Vec<_> = outgoing.attachments.iter().filter(|file| !file.inline).collect();
    if signed.is_none() && outgoing.attachments.iter().any(|file| file.inline) {
        return Err(MimeError::Write);
    }
    builder = if attached.is_empty() && !alternative {
        builder.text_body(text)
    } else if attached.is_empty() {
        builder.body(text_part)
    } else {
        let mut parts = vec![text_part];
        for file in attached {
            let mut part = image_part(file).attachment(one_line(&file.filename));
            if let Some(id) = &file.content_id {
                if !html::valid_cid(id) {
                    return Err(MimeError::InvalidMessageId);
                }
                part = part.cid(id.as_str());
            }
            parts.push(part);
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

/// The first HTML part `markup` ([`html_part`]), cleaned ([`clean_html`]); `None` for a part larger
/// than `HTML_INPUT_MAX`, or when the cleaned HTML is empty or larger than [`HTML_MAX`].
fn cleaned_html(markup: &str) -> Option<String> {
    if markup.len() > HTML_INPUT_MAX {
        return None;
    }
    let cleaned = clean_html(markup);
    (!cleaned.trim().is_empty() && cleaned.len() <= HTML_MAX).then_some(cleaned)
}

/// The tags an HTML mail keeps: text markup and structure, lists, tables, links, images and the
/// `style` element. Everything else goes; the text inside an unknown tag stays.
const HTML_TAGS: &[&str] = &[
    "p",
    "div",
    "span",
    "br",
    "hr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "small",
    "sub",
    "sup",
    "font",
    "center",
    "blockquote",
    "pre",
    "code",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "colgroup",
    "col",
    "caption",
    "a",
    "img",
    "style",
];

/// Attributes every kept tag may have: layout and text direction, nothing that runs or loads.
const HTML_ATTRIBUTES: &[&str] = &[
    "style",
    "class",
    "id",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "color",
    "border",
    "cellpadding",
    "cellspacing",
    "alt",
    "title",
    "dir",
    "lang",
];

/// Tags whose content goes with them: what would run, what a frame or object shows, and text
/// that is no part of the mail's body (the document's `title`, a form's choices).
const HTML_DROPPED_WITH_CONTENT: &[&str] = &[
    "script", "noscript", "template", "title", "iframe", "frame", "frameset", "object", "embed", "applet", "svg",
    "math", "select", "textarea",
];

/// Cleans the HTML of a mail through an allow list (task M8.1): the tags of `HTML_TAGS` with the
/// attributes of `HTML_ATTRIBUTES` (and `colspan`/`rowspan` on cells, `face`/`size` on `font`, so
/// the layout of tables and old mail holds), `href` on `a` only to `http`, `https` and `mailto`,
/// `src` on `img` only from `http`, `https`, `cid:` and `data:image/png|jpeg|gif|webp`. Relative
/// links and images, comments (Outlook's conditional ones too), every other tag and attribute
/// (`script`, `iframe`, `object`, `embed`, `form`, `input`, `button`, `base`, `meta`, `link`, `svg`,
/// `on…`) go; links get `rel="noopener noreferrer"`. Fetches nothing.
///
/// Then the old layout attributes become inline CSS (task M9.5, [`legacy::to_css`]): `cellpadding`,
/// `cellspacing`, `border` on tables, `bgcolor`, `width`, `height`, `align`, `valign`, `<center>` and
/// `<font>`, because a renderer that only knows CSS (Blitz) ignores them. The result passes the
/// allow list a second time, so the rewrite can add nothing it does not allow.
pub fn clean_html(markup: &str) -> String {
    let builder = html_cleaner();
    let cleaned = builder.clean(markup).to_string();
    match legacy::to_css(&cleaned) {
        Some(styled) => builder.clean(&styled).to_string(),
        None => cleaned,
    }
}

/// The allow list of [`clean_html`] as an `ammonia` cleaner.
fn html_cleaner() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::empty();
    builder
        .add_tags(HTML_TAGS)
        .clean_content_tags(HTML_DROPPED_WITH_CONTENT.iter().copied().collect())
        .add_generic_attributes(HTML_ATTRIBUTES)
        .add_tag_attributes("a", &["href"])
        .add_tag_attributes("img", &["src"])
        .add_tag_attributes("td", &["colspan", "rowspan"])
        .add_tag_attributes("th", &["colspan", "rowspan"])
        .add_tag_attributes("font", &["face", "size"])
        .url_schemes(["http", "https", "mailto", "cid", "data"].into_iter().collect())
        .url_relative(ammonia::UrlRelative::Deny)
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("a", "href") => allowed_url(value, &["http://", "https://", "mailto:"]),
            ("img", "src") => allowed_url(value, IMAGE_SOURCES),
            _ => Some(value.into()),
        });
    builder
}

/// Where a mail's image may come from: the web, an inline part (`cid:`) or the image itself in
/// one of four picture formats (no SVG, which can carry script).
const IMAGE_SOURCES: &[&str] = &[
    "http://",
    "https://",
    "cid:",
    "data:image/png;",
    "data:image/png,",
    "data:image/jpeg;",
    "data:image/jpeg,",
    "data:image/gif;",
    "data:image/gif,",
    "data:image/webp;",
    "data:image/webp,",
];

/// `value` without the white space around it, when it starts with one of `prefixes` (case
/// ignored); `None` drops the attribute.
fn allowed_url<'a>(value: &str, prefixes: &[&str]) -> Option<std::borrow::Cow<'a, str>> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_control());
    let starts = |prefix: &&str| value.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix));
    prefixes.iter().any(starts).then(|| value.to_owned().into())
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
        content_id: part
            .content_id()
            .map(|id| clean(id.trim().trim_start_matches('<').trim_end_matches('>')))
            .filter(|id| !id.is_empty()),
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
        auth: Some(first_raw(message, "Authentication-Results").map(|value| auth_results(&value)).unwrap_or_default()),
        return_path: first_raw(message, "Return-Path").and_then(|value| return_path(&value)),
        reply_ref: Some(message.header_raw("In-Reply-To").is_some() || message.header_raw("References").is_some()),
    }
}

/// The first header `name` of the message, top-most, unfolded and trimmed (issue #506): the one the
/// last server wrote. `header_raw` gives the last one, which for `Authentication-Results` and
/// `Return-Path` is the one a sender could have put in themselves.
fn first_raw(message: &mail_parser::Message<'_>, name: &str) -> Option<String> {
    message
        .headers_raw()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| one_line(value).trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The sender checks an `Authentication-Results` value names (RFC 8601):
/// `mx.example; spf=pass smtp.mailfrom=…; dkim=fail header.d=…; dmarc=none`. The first part is the
/// server's name and says nothing; comments in parentheses are dropped. Of several results of one
/// check (two DKIM signatures) a `pass` wins, else the first counts.
pub fn auth_results(value: &str) -> torro_mail::AuthResults {
    let mut results = torro_mail::AuthResults::default();
    for part in without_comments(value).split(';').skip(1) {
        let Some((method, rest)) = part.trim().split_once('=') else { continue };
        let result = torro_mail::AuthResult::of_word(rest.split_whitespace().next().unwrap_or_default());
        let slot = match method.trim().to_ascii_lowercase().as_str() {
            "spf" => &mut results.spf,
            "dkim" => &mut results.dkim,
            "dmarc" => &mut results.dmarc,
            _ => continue,
        };
        if slot.is_none() || result == torro_mail::AuthResult::Pass {
            *slot = Some(result);
        }
    }
    results
}

/// `text` without its comments in parentheses (nested ones too), as RFC 5322 allows them in headers.
fn without_comments(text: &str) -> String {
    let mut depth = 0usize;
    text.chars()
        .filter(|c| match c {
            '(' => {
                depth += 1;
                false
            }
            ')' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

/// The address of a `Return-Path` value (`<bounce@mail.example>`), small letters; `None` for the
/// empty path `<>` of bounces and for anything that is not one address.
fn return_path(value: &str) -> Option<String> {
    let address = value.trim().trim_start_matches('<').trim_end_matches('>').trim().to_lowercase();
    valid_address(&address).then_some(address)
}

/// The headers `mail.unsubscribe` acts on (issue #416), as the message carries them: unfolded,
/// controls turned into spaces, trimmed. Both come from outside; the core parses and checks the
/// addresses in them before it uses one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsubscribeHeaders {
    /// `List-Unsubscribe:` (RFC 2369): addresses in angle brackets, separated by commas.
    pub list: String,
    /// `List-Unsubscribe-Post:` (RFC 8058), `List-Unsubscribe=One-Click` for the one click; `None`
    /// when the message has none (or an empty or over-long one).
    pub post: Option<String>,
}

/// The longest `List-Unsubscribe` or `List-Unsubscribe-Post` value taken (bytes); a longer one is
/// left as if it were missing.
pub const UNSUBSCRIBE_HEADER_MAX: usize = 2048;

/// The unsubscribe headers of a message ([`UnsubscribeHeaders`]); `None` when it cannot be read or
/// has no `List-Unsubscribe` of at most [`UNSUBSCRIBE_HEADER_MAX`] bytes. Never fetches anything.
pub fn unsubscribe_of(raw: &[u8]) -> Option<UnsubscribeHeaders> {
    let message = MessageParser::default().parse_headers(raw)?;
    let header = |name: &str| {
        message
            .header_raw(name)
            .map(|value| one_line(value).trim().to_owned())
            .filter(|value| !value.is_empty() && value.len() <= UNSUBSCRIBE_HEADER_MAX)
    };
    Some(UnsubscribeHeaders { list: header("List-Unsubscribe")?, post: header("List-Unsubscribe-Post") })
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

/// The HTML part of a message: `body` (cleaned HTML) in a document of its own.
fn html_document(body: &str) -> String {
    format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>\n{body}\n</body></html>\n")
}

/// The first HTML part of the text of `message`, as it is. A message of plain text alone has none:
/// `mail-parser` lists its text part among the HTML bodies too, but as text, not HTML.
fn html_part(message: &mail_parser::Message<'_>) -> Option<String> {
    message.html_body.iter().filter_map(|&id| message.parts.get(id as usize)).find_map(|part| match &part.body {
        PartType::Html(markup) => Some(markup.to_string()),
        _ => None,
    })
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

/// Old layout attributes as inline CSS (task M9.5, decision 9 in `docs/mail.md`).
///
/// Blitz, which draws mail in the reader, knows CSS but not the layout attributes of old HTML that
/// mail still uses everywhere (`<td bgcolor>`, `<table cellpadding>`, `<font>` …). They are rewritten
/// on HTML that [`clean_html`] has cleaned once: html5ever wrote it, so every element is closed, every
/// attribute value is in double quotes with `"`, `<`, `>` and `&` escaped, and only `style` holds raw
/// text. A small scanner is enough for that; anything else leaves the HTML as it is ([`to_css`] gives
/// `None`).
///
/// The CSS goes in front of an element's own `style`, so what the sender wrote there wins. Every
/// value is checked (colours, numbers, font names); one that does not fit goes with its attribute,
/// so nothing of it reaches the CSS. Like the old attributes in a browser, `cellpadding` and
/// `border` reach the cells of their own table only, and a table inside an element centred with
/// `align` (or `<center>`) is centred as well.
mod legacy {
    /// Elements without an end tag among those the cleaning keeps.
    const VOID: &[&str] = &["br", "hr", "img", "col"];
    /// Elements whose `width` old HTML honours.
    const WIDTH: &[&str] = &["table", "td", "th", "col", "colgroup", "img", "hr"];
    /// Elements whose `height` old HTML honours.
    const HEIGHT: &[&str] = &["table", "td", "th", "tr", "img"];
    /// Elements whose `valign` old HTML honours.
    const VALIGN: &[&str] = &["td", "th", "tr", "thead", "tbody", "tfoot", "col", "colgroup"];
    /// Elements whose `align` old HTML honours: tables, images and rules have their own meaning
    /// ([`Element::align`]), the rest aligns its text.
    const ALIGNED: &[&str] = &[
        "table", "img", "hr", "div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "td", "th", "tr", "thead", "tbody",
        "tfoot", "caption",
    ];
    /// The keywords of `<font size>` 1 to 7, as HTML's table of legacy font sizes has them.
    const FONT_SIZES: [&str; 7] = ["x-small", "small", "medium", "large", "x-large", "xx-large", "xxx-large"];
    /// Attributes of a tag by name, the values as written (still escaped).
    type Attributes<'a> = Vec<(&'a str, &'a str)>;

    /// Centres a block element (a table) in its line.
    const CENTRED: &str = "margin-left:auto;margin-right:auto;";

    /// `cleaned` with its old layout attributes written as inline CSS; `None` when the HTML is not
    /// as html5ever writes it.
    pub(super) fn to_css(cleaned: &str) -> Option<String> {
        let mut out = String::with_capacity(cleaned.len() + cleaned.len() / 8);
        let mut open: Vec<Open<'_>> = Vec::new();
        let mut rest = cleaned;
        while let Some(at) = rest.find('<') {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            if let Some(after) = rest.strip_prefix("</") {
                let end = after.find('>')?;
                let name = &after[..end];
                if let Some(at) = open.iter().rposition(|element| element.name == name) {
                    open.truncate(at);
                }
                out.push_str("</");
                out.push_str(renamed(name));
                out.push('>');
                rest = &after[end + 1..];
                continue;
            }
            let (name, attributes, after) = start_tag(&rest[1..])?;
            let element = Element::new(name, &attributes, &open);
            element.write(&mut out);
            rest = after;
            if name == "style" {
                // Raw text up to its end tag, which the loop then reads like any other.
                let end = rest.find("</style>")?;
                out.push_str(&rest[..end]);
                rest = &rest[end..];
            }
            if !VOID.contains(&name) {
                open.push(Open { name, centred: element.centred, table: element.table });
            }
        }
        out.push_str(rest);
        Some(out)
    }

    /// The name and attributes (values as written, still escaped) of the start tag at the front of
    /// `text`, which begins after its `<`, and the text after its `>`.
    fn start_tag(text: &str) -> Option<(&str, Attributes<'_>, &str)> {
        let name_end = text.find(|c: char| !c.is_ascii_alphanumeric())?;
        let name = &text[..name_end];
        if name.is_empty() {
            return None;
        }
        let mut rest = &text[name_end..];
        let mut attributes = Vec::new();
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if let Some(after) = rest.strip_prefix('>') {
                return Some((name, attributes, after));
            }
            let key_end = rest.find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/' | '"'))?;
            if key_end == 0 {
                return None;
            }
            let key = &rest[..key_end];
            rest = &rest[key_end..];
            let value = match rest.strip_prefix("=\"") {
                Some(quoted) => {
                    let end = quoted.find('"')?;
                    rest = &quoted[end + 1..];
                    &quoted[..end]
                }
                None => "",
            };
            attributes.push((key, value));
        }
    }

    /// What `<center>` and `<font>` become; every other element keeps its name.
    fn renamed(name: &str) -> &str {
        match name {
            "center" => "div",
            "font" => "span",
            other => other,
        }
    }

    /// An element still open, with what its content inherits.
    struct Open<'a> {
        name: &'a str,
        /// A table in it is centred: it is centred with `align` or `<center>`.
        centred: bool,
        /// Set for a table: what its cells get.
        table: Option<Table>,
    }

    /// What a table's `cellpadding` and `border` give its own cells.
    #[derive(Debug, Clone, Copy, Default)]
    struct Table {
        padding: Option<u32>,
        border: bool,
    }

    /// One start tag being rewritten.
    struct Element<'a> {
        name: &'a str,
        /// The declarations made from old attributes, each ending in `;`.
        css: String,
        /// The attributes that stay, as written.
        kept: Attributes<'a>,
        /// Whether a table in this element is centred ([`Open::centred`]).
        centred: bool,
        /// Set for a table.
        table: Option<Table>,
        /// The element has an `align` of its own that fits.
        aligned: bool,
    }

    impl<'a> Element<'a> {
        /// The start tag `name` with `attributes`, inside the elements `open`.
        fn new(name: &'a str, attributes: &[(&'a str, &'a str)], open: &[Open<'_>]) -> Element<'a> {
            let inside_centred = open.last().is_some_and(|parent| parent.centred);
            let mut element = Element {
                name,
                css: String::new(),
                kept: Vec::new(),
                // A table starts afresh: its cells centre nothing unless they say so.
                centred: inside_centred && name != "table",
                table: (name == "table").then(Table::default),
                aligned: false,
            };
            if matches!(name, "td" | "th")
                && let Some(table) = open.iter().rev().find_map(|parent| parent.table)
            {
                if let Some(padding) = table.padding {
                    element.css.push_str(&format!("padding:{padding}px;"));
                }
                if table.border {
                    element.css.push_str("border:1px solid gray;");
                }
            }
            if name == "center" {
                element.css.push_str("text-align:center;");
                element.centred = true;
            }
            for &(key, value) in attributes {
                element.attribute(key, value);
            }
            if name == "table" && inside_centred && !element.aligned {
                element.css.insert_str(0, CENTRED);
            }
            element
        }

        /// Takes one attribute: an old one becomes CSS (or goes, when its value does not fit),
        /// every other stays.
        fn attribute(&mut self, key: &'a str, value: &'a str) {
            let name = self.name;
            let css = match key {
                "bgcolor" => color(value).map(|color| format!("background-color:{color};")),
                "width" if WIDTH.contains(&name) => length(value).map(|length| format!("width:{length};")),
                "height" if HEIGHT.contains(&name) => length(value).map(|length| format!("height:{length};")),
                "valign" if VALIGN.contains(&name) => vertical(value).map(|align| format!("vertical-align:{align};")),
                "align" if ALIGNED.contains(&name) => self.align(value),
                "cellspacing" if name == "table" => pixels(value).map(|n| format!("border-spacing:{n}px;")),
                "cellpadding" if name == "table" => {
                    self.table = Some(Table { padding: pixels(value), ..self.table.unwrap_or_default() });
                    None
                }
                "border" if name == "table" => {
                    let width = pixels(value).filter(|&width| width > 0);
                    self.table = Some(Table { border: width.is_some(), ..self.table.unwrap_or_default() });
                    width.map(|width| format!("border:{width}px solid gray;"))
                }
                "face" if name == "font" => families(value).map(|families| format!("font-family:{families};")),
                "size" if name == "font" => font_size(value).map(|size| format!("font-size:{size};")),
                "color" if name == "font" => color(value).map(|color| format!("color:{color};")),
                _ => {
                    self.kept.push((key, value));
                    return;
                }
            };
            if let Some(css) = css {
                self.css.push_str(&css);
            }
        }

        /// The CSS of `align="value"` on this element: a table is centred or floats, an image
        /// floats or sits on the line, a rule moves; any other element aligns its text, and a
        /// table inside follows a centred one.
        fn align(&mut self, value: &str) -> Option<String> {
            let value = value.trim().to_ascii_lowercase();
            let css = match (self.name, value.as_str()) {
                ("table", "center") => CENTRED.to_owned(),
                ("table" | "img", "left" | "right") => format!("float:{value};"),
                ("img", "top" | "bottom" | "baseline") => format!("vertical-align:{value};"),
                ("img", "middle" | "absmiddle" | "center") => "vertical-align:middle;".to_owned(),
                ("hr", "left") => "margin-left:0;margin-right:auto;".to_owned(),
                ("hr", "right") => "margin-left:auto;margin-right:0;".to_owned(),
                ("hr", "center") => CENTRED.to_owned(),
                ("table" | "img" | "hr", _) => return None,
                (_, "center" | "middle") => {
                    self.centred = true;
                    "text-align:center;".to_owned()
                }
                (_, "left" | "right" | "justify") => {
                    self.centred = false;
                    format!("text-align:{value};")
                }
                _ => return None,
            };
            self.aligned = true;
            Some(css)
        }

        /// Writes the start tag: the new name, the kept attributes, the CSS in front of the
        /// element's own `style`.
        fn write(&self, out: &mut String) {
            out.push('<');
            out.push_str(renamed(self.name));
            let mut styled = false;
            for &(key, value) in &self.kept {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                if key == "style" {
                    out.push_str(&self.css);
                    styled = true;
                }
                out.push_str(value);
                out.push('"');
            }
            if !styled && !self.css.is_empty() {
                out.push_str(" style=\"");
                out.push_str(&self.css);
                out.push('"');
            }
            out.push('>');
        }
    }

    /// A colour as old HTML writes it: `#rgb` or `#rrggbb` (also without `#`, as old mail often
    /// has it), or a name of letters only. Lowercase.
    fn color(value: &str) -> Option<String> {
        let value = value.trim();
        let hex = value.strip_prefix('#').unwrap_or(value);
        if matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(format!("#{}", hex.to_ascii_lowercase()));
        }
        let named = (3..=24).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphabetic());
        named.then(|| value.to_ascii_lowercase())
    }

    /// Whole pixels as in `cellpadding="4"`, `px` allowed after the number.
    fn pixels(value: &str) -> Option<u32> {
        let value = value.trim();
        let digits = value.strip_suffix("px").unwrap_or(value);
        let fits = (1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
        fits.then(|| digits.parse().ok()).flatten()
    }

    /// A width or height: a number is pixels, a percentage stays one.
    fn length(value: &str) -> Option<String> {
        let value = value.trim();
        let Some(number) = value.strip_suffix('%') else {
            return pixels(value).map(|n| format!("{n}px"));
        };
        let (whole, part) = number.split_once('.').unwrap_or((number, "0"));
        let digits = |text: &str| (1..=3).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
        (digits(whole) && digits(part)).then(|| format!("{number}%"))
    }

    /// `valign`: top, middle, bottom or baseline (`center` is middle).
    fn vertical(value: &str) -> Option<&'static str> {
        match value.trim().to_ascii_lowercase().as_str() {
            "top" => Some("top"),
            "middle" | "center" => Some("middle"),
            "bottom" => Some("bottom"),
            "baseline" => Some("baseline"),
            _ => None,
        }
    }

    /// `<font size>`: 1 to 7, or with a sign relative to 3, as the keyword of HTML's table;
    /// beyond the ends it stops at 1 or 7.
    fn font_size(value: &str) -> Option<&'static str> {
        let value = value.trim();
        let (sign, digits) = match value.strip_prefix('+') {
            Some(digits) => (1, digits),
            None => value.strip_prefix('-').map_or((0, value), |digits| (-1, digits)),
        };
        if !(1..=2).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let number: i32 = digits.parse().ok()?;
        let size = if sign == 0 { number } else { 3 + sign * number };
        FONT_SIZES.get(usize::try_from(size.clamp(1, 7) - 1).ok()?).copied()
    }

    /// `<font face>`: font names of letters, digits, spaces, `-`, `_` and `.`, separated by
    /// commas; quotes around a name are taken off, a name with a space is quoted again. A name
    /// that does not fit (`;`, `{`, `(` …) is left out.
    fn families(value: &str) -> Option<String> {
        let value = value.replace("&quot;", "\"");
        let names: Vec<String> = value
            .split(',')
            .filter_map(|name| {
                let name = name.trim().trim_matches(['"', '\'']).trim();
                let fits = (1..=64).contains(&name.len())
                    && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
                let quoted = name.contains(' ') || name.starts_with(|c: char| c.is_ascii_digit());
                fits.then(|| if quoted { format!("'{name}'") } else { name.to_owned() })
            })
            .collect();
        (!names.is_empty()).then(|| names.join(","))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quoted_printable_mixed_line_endings_never_expand_opaque_attachment_bytes() {
        let raw = b"Content-Type: image/png\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n=89PNG\r\n\n\n";
        let attachment = super::image_attachment(raw, 9).expect("transfer bytes fit exactly");
        assert_eq!(attachment.bytes, b"\x89PNG\r\n\n\n");
        assert!(super::image_attachment(raw, 7).is_err(), "reject before emitting an excess byte");
        assert_eq!(
            super::bounded_quoted_printable(b"a=20=\r\nb=09=\nc \t\r\nd\t", 20).expect("soft breaks and whitespace"),
            b"a b\tc\r\nd"
        );
        assert_eq!(super::bounded_quoted_printable(b"a\rb", 3).expect("opaque CR"), b"a\rb");
        assert!(super::bounded_quoted_printable(b"=0", 3).is_err());
        assert!(super::bounded_quoted_printable(b"=GG", 3).is_err());
        assert!(super::bounded_quoted_printable(b"\r\n\n\n", 3).is_err());
        let decoded = super::bounded_quoted_printable(&[b'\n'; 8192], 8192).expect("exact bounded output");
        assert_eq!(decoded.len(), 8192);
        assert_eq!(decoded.capacity(), 8192);
    }

    #[test]
    fn standalone_transfer_encodings_keep_exact_bytes_and_bounded_capacity() {
        for (encoding, body) in [
            ("base64", b"AAECA/7/\r\n".as_slice()),
            ("quoted-printable", b"=00=01=02=03=FE=FF".as_slice()),
            ("binary", b"\0\x01\x02\x03\xfe\xff".as_slice()),
            ("8bit", b"\0\x01\x02\x03\xfe\xff".as_slice()),
        ] {
            let mut raw = format!("Content-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=data.bin\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n").into_bytes();
            raw.extend_from_slice(body);
            let attachment = super::bounded_attachment(&raw, 6).expect("decoded attachment");
            assert_eq!(attachment.bytes, [0, 1, 2, 3, 0xfe, 0xff], "{encoding}");
            assert_eq!(attachment.bytes.capacity(), 6, "no encoded-length capacity retained: {encoding}");
            assert!(super::bounded_attachment(&raw, 5).is_err(), "decoded limit: {encoding}");
        }
        let raw = b"Content-Type: application/octet-stream\nContent-Transfer-Encoding: 7bit\n\nopaque\r\n";
        assert_eq!(super::bounded_attachment(raw, 8).expect("LF headers").bytes, b"opaque\r\n");
        assert!(super::bounded_attachment(b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x--", 100).is_err());
        assert!(
            super::bounded_attachment(b"Content-Type: image/png\r\nContent-Transfer-Encoding: mystery\r\n\r\nx", 100)
                .is_err()
        );
        assert!(
            super::bounded_attachment(b"Content-Type: image/png\r\nContent-Transfer-Encoding: base64\r\n\r\n!", 100)
                .is_err()
        );
    }

    #[test]
    fn bounded_text_attachments_preserve_bytes_without_expanding_the_declared_charset() {
        let raw = b"Content-Type: text/plain; charset=shift_jis\r\nContent-Disposition: attachment; filename=notes.txt\r\nContent-Transfer-Encoding: base64\r\n\r\ngqA=\r\n";
        let attachment = super::bounded_attachment(raw, 2).expect("Shift-JIS attachment");
        assert_eq!(attachment.bytes, [0x82, 0xa0], "transfer decoding only, never whole-body charset expansion");
        assert_eq!(attachment.filename.as_deref(), Some("notes.txt"));
        assert!(super::bounded_attachment(raw, 1).is_err());
        assert!(super::bounded_attachment(b"Content-Type: text/plain\r\n\r\nnot an attachment", 30).is_err());
        assert!(super::image_attachment(raw, 10).is_err(), "a text part is never treated as an image");
    }
    use super::*;

    #[test]
    fn unsubscribe_headers_are_read_unfolded_and_bounded() {
        let raw = b"From: news@shop.example\r\nList-Unsubscribe: <mailto:off@shop.example?subject=stop>,\r\n <https://shop.example/u/1>\r\nList-Unsubscribe-Post:  List-Unsubscribe=One-Click \r\nSubject: Sale\r\n\r\nText\r\n";
        let headers = unsubscribe_of(raw).expect("headers");
        assert_eq!(headers.list, "<mailto:off@shop.example?subject=stop>,   <https://shop.example/u/1>");
        assert_eq!(headers.post.as_deref(), Some("List-Unsubscribe=One-Click"));

        let without_post = b"List-Unsubscribe: <https://shop.example/u/1>\r\n\r\nText\r\n";
        assert_eq!(unsubscribe_of(without_post).and_then(|headers| headers.post), None);
        assert_eq!(unsubscribe_of(b"From: anna@example.org\r\n\r\nHello\r\n"), None, "no header");

        let long = format!("List-Unsubscribe: <https://shop.example/{}>\r\n\r\n", "a".repeat(UNSUBSCRIBE_HEADER_MAX));
        assert_eq!(unsubscribe_of(long.as_bytes()), None, "over-long is as if missing");
    }

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

    #[test]
    fn formatted_signature_has_matching_safe_html_and_plain_text() {
        let mut outgoing = answer(None);
        outgoing.html = Some("<p>Hello</p>".into());
        outgoing.signature = Some("**Anna**\n[Website](https://example.org)".into());
        outgoing.signature_format = true;
        let built = build(&outgoing).expect("built");
        let read = super::read(&built.bytes).expect("read");
        let html = read.html.expect("HTML copy");
        assert!(html.contains("<strong>Anna</strong>"), "{html}");
        assert!(html.contains("href=\"https://example.org\""), "{html}");
        assert!(read.text.contains("Anna") && read.text.contains("Website"), "{}", read.text);
        assert!(!read.text.contains("**Anna**"), "{}", read.text);
    }

    fn answer(calendar: Option<CalendarPart>) -> Outgoing {
        Outgoing {
            from: MailAddress { name: Some("Anna".into()), address: "anna@example.org".into() },
            to: vec![MailAddress { name: None, address: "jonas@example.org".into() }],
            cc: vec![],
            bcc: vec![],
            subject: "Accepted: Planning".into(),
            body: "Anna has accepted.".into(),
            html: None,
            signature: None,
            signature_format: false,
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

    /// The `style` of the first element `tag` in `html`.
    fn style_of<'a>(html: &'a str, tag: &str) -> &'a str {
        let start = html.find(&format!("<{tag}")).unwrap_or_else(|| panic!("no <{tag} in {html}"));
        let element = &html[start..];
        let element = &element[..element.find('>').expect("a start tag")];
        element.split_once("style=\"").map_or("", |(_, style)| style.split('"').next().unwrap_or(""))
    }

    /// Task M9.5: table attributes become CSS, cellpadding and border reach only their own cells.
    #[test]
    fn table_attributes_become_css_for_their_own_cells() {
        let html = clean_html(
            "<table cellpadding=\"4\" cellspacing=\"2\" border=\"1\" width=\"600\" bgcolor=\"#FFFFFF\">\
             <tr><td width=\"50%\" height=\"20\" valign=\"top\" align=\"right\">a\
             <table><tr><th>inner</th></tr></table></td></tr></table>",
        );
        assert_eq!(
            style_of(&html, "table"),
            "border-spacing:2px;border:1px solid gray;width:600px;background-color:#ffffff;",
            "{html}"
        );
        assert_eq!(
            style_of(&html, "td"),
            "padding:4px;border:1px solid gray;width:50%;height:20px;vertical-align:top;text-align:right;",
            "{html}"
        );
        assert_eq!(style_of(&html, "th"), "", "a nested table's cells keep nothing of the outer table: {html}");
        for gone in ["cellpadding", "cellspacing", "border=", "bgcolor", "width=", "height=", "valign", "align="] {
            assert!(!html.contains(gone), "{gone} is CSS now: {html}");
        }
        let plain = clean_html("<table border=\"0\" cellpadding=\"0\"><tr><td>a</td></tr></table>");
        assert_eq!(plain, "<table><tbody><tr><td style=\"padding:0px;\">a</td></tr></tbody></table>");
    }

    /// Task M9.5: align centres a table, floats it or aligns text; a table inside a centred cell
    /// or `<center>` is centred too, unless it aligns itself.
    #[test]
    fn align_centres_tables_and_aligns_text() {
        let html = clean_html(
            "<table width=\"100%\"><tr><td align=\"center\"><table width=\"600\"><tr><td>x</td></tr></table>\
             </td></tr></table>",
        );
        assert!(html.contains("<td style=\"text-align:center;\">"), "{html}");
        assert!(
            html.contains("<table style=\"margin-left:auto;margin-right:auto;width:600px;\">"),
            "the inner table is centred like in a browser: {html}"
        );
        assert!(html.contains("<td>x</td>"), "its cells centre nothing: {html}");

        let html = clean_html("<center><table align=\"left\"><tr><td>x</td></tr></table><p>t</p></center>");
        assert!(html.starts_with("<div style=\"text-align:center;\"><table style=\"float:left;\">"), "{html}");
        assert!(html.ends_with("</div>") && !html.contains("center>"), "{html}");

        let html = clean_html(
            "<table align=\"center\"><tr><td>x</td></tr></table><img src=\"cid:a\" align=\"middle\">\
             <hr align=\"left\" width=\"50%\"><p align=\"justify\">t</p><td align=\"middle\">",
        );
        assert_eq!(style_of(&html, "table"), "margin-left:auto;margin-right:auto;", "{html}");
        assert_eq!(style_of(&html, "img"), "vertical-align:middle;", "{html}");
        assert_eq!(style_of(&html, "hr"), "margin-left:0;margin-right:auto;width:50%;", "{html}");
        assert_eq!(style_of(&html, "p"), "text-align:justify;", "{html}");
    }

    /// Task M9.5: `<font>` becomes a `span` with font, size (HTML's table) and colour.
    #[test]
    fn font_becomes_a_span_with_its_family_size_and_colour() {
        let html = clean_html("<font face=\"'Segoe UI', Arial, sans-serif\" size=\"2\" color=\"336699\">Hi</font>");
        assert_eq!(
            html,
            "<span style=\"font-family:'Segoe UI',Arial,sans-serif;font-size:small;color:#336699;\">Hi</span>"
        );
        let size = |size: &str| style_of(&clean_html(&format!("<font size=\"{size}\">x</font>")), "span").to_owned();
        assert_eq!(size("1"), "font-size:x-small;");
        assert_eq!(size("7"), "font-size:xxx-large;");
        assert_eq!(size("+1"), "font-size:large;");
        assert_eq!(size("-2"), "font-size:x-small;");
        assert_eq!(size("9"), "font-size:xxx-large;", "beyond 7 stops at 7");
        assert_eq!(size("big"), "");
        assert_eq!(style_of(&clean_html("<font color=\"Red\">x</font>"), "span"), "color:red;");
    }

    /// Task M9.5: the sender's own `style` comes after the CSS made from the attributes, so it
    /// wins; the rest of the element stays as it was.
    #[test]
    fn the_senders_own_style_wins() {
        let html = clean_html(
            "<table><tr><td bgcolor=\"#fff\" class=\"x\" style=\"background-color: #000\">a</td></tr></table>",
        );
        assert!(html.contains("<td class=\"x\" style=\"background-color:#fff;background-color: #000\">"), "{html}");
        let untouched = "<p class=\"lead\">Hello <b>Anna</b></p><span align=\"center\">x</span>";
        assert_eq!(clean_html(untouched), untouched, "nothing old, nothing changed");
        let styled = "<style>td { color: red; } /* <td bgcolor=\"red\"> */</style><p>x</p>";
        assert_eq!(clean_html(styled), styled, "a style sheet is no attribute");
    }

    /// Task M9.5: values are checked; one that would smuggle CSS in goes with its attribute.
    #[test]
    fn values_that_do_not_fit_are_dropped() {
        let html = clean_html(
            "<table bgcolor=\"red;background:url(x)\" width=\"100;position:fixed\" cellspacing=\"-1\" \
             cellpadding=\"4 ;x\" border=\"1px;color:red\" align=\"center;float:left\"><tr>\
             <td valign=\"top;x\" height=\"auto\">a</td></tr></table>\
             <font face=\"Arial;}body{x, Georgia, a(b)\" size=\"7;x\" color=\"#12345\">f</font>",
        );
        for gone in
            ["url(", "position", "fixed", "float", ";x", "}", "body{", "a(b)", "#12345", "bgcolor", "size=", "border"]
        {
            assert!(!html.contains(gone), "{gone} is dropped: {html}");
        }
        assert!(html.starts_with("<table><tbody><tr><td>a</td>"), "{html}");
        let border = clean_html("<table border=\"2px\"><tr><td>a</td></tr></table>");
        assert!(border.starts_with("<table style=\"border:2px solid gray;\">"), "px after the number: {border}");
        assert!(html.contains("<span style=\"font-family:Georgia;\">f</span>"), "{html}");
        let quoted = clean_html("<font face=\"&quot;Times New Roman&quot;, serif\">x</font>");
        assert_eq!(style_of(&quoted, "span"), "font-family:'Times New Roman',serif;", "{quoted}");
    }

    /// Review of M9.5: `<` and `>` inside a quoted value of an element that is rewritten end
    /// neither the tag nor the value early. The cleaning escapes them in values, the rewrite keeps
    /// the values as written, and a second cleaning changes nothing.
    #[test]
    fn angle_brackets_in_values_stay_inside_their_attribute() {
        let html = clean_html(
            "<table title=\"a<b>\" bgcolor=\"#fff\"><tr><td align=\"center\" title=\"<script>x</script>\">\
             <img src=\"cid:a\" alt=\"x>y\" align=\"left\" width=\"10\"></td></tr></table>",
        );
        assert_eq!(
            html,
            "<table title=\"a&lt;b&gt;\" style=\"background-color:#fff;\"><tbody><tr>\
             <td title=\"&lt;script&gt;x&lt;/script&gt;\" style=\"text-align:center;\">\
             <img src=\"cid:a\" alt=\"x&gt;y\" style=\"float:left;width:10px;\"></td></tr></tbody></table>"
        );
        assert_eq!(clean_html(&html), html, "stable");
        assert_eq!(legacy::to_css(&html).as_deref(), Some(html.as_str()), "nothing old is left to rewrite");
        // Raw brackets in a value, as a writer other than html5ever might leave them: the tag still
        // ends at its own `>`, not at the one in the value.
        let raw = "<td title=\"a>b\" align=\"right\">x</td>";
        assert_eq!(legacy::to_css(raw).as_deref(), Some("<td title=\"a>b\" style=\"text-align:right;\">x</td>"));
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
            html: None,
            html_source: None,
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

    /// Issue #506: the top-most `Authentication-Results` counts – the one the receiving server
    /// wrote, not one a sender put further down –, comments and the server's name are no checks, of
    /// two DKIM results a `pass` wins; `Return-Path` and the reply reference are read, `<>` is none.
    #[test]
    fn sender_checks_return_path_and_the_reply_reference_are_read() {
        use torro_mail::{AuthResult, AuthResults};
        let raw = "Authentication-Results: mx.example.org; spf=softfail (sender not allowed) smtp.mailfrom=x.example;\r\n \
                   dkim=fail header.d=x.example; dkim=pass header.d=y.example; dmarc=fail (p=reject)\r\n\
                   Authentication-Results: forged.example; spf=pass; dkim=pass; dmarc=pass\r\n\
                   Return-Path: <Bounce@Mail.Example>\r\nFrom: a@b.example\r\nSubject: Re: x\r\n\r\nText\r\n";
        let first = read(raw.as_bytes()).expect("read");
        let expected = AuthResults {
            spf: Some(AuthResult::Softfail),
            dkim: Some(AuthResult::Pass),
            dmarc: Some(AuthResult::Fail),
        };
        assert_eq!(first.traits.auth, Some(expected));
        assert_eq!(first.traits.return_path.as_deref(), Some("bounce@mail.example"));
        assert_eq!(first.traits.reply_ref, Some(false), "no In-Reply-To, no References");

        let raw = "Return-Path: <>\r\nIn-Reply-To: <x@b.example>\r\nFrom: a@b.example\r\n\r\nText\r\n";
        let second = read(raw.as_bytes()).expect("read");
        assert_eq!(second.traits.auth, Some(AuthResults::default()), "read, and no header");
        assert_eq!((second.traits.return_path, second.traits.reply_ref), (None, Some(true)));
        assert_eq!(auth_results("mx.example.org; none"), AuthResults::default());
    }
}
