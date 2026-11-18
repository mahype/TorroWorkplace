//! The text of an attachment for `mail.read_attachment` (issue #893): what the assistant reads when a
//! person asks about a file a mail brought.
//!
//! - **Text files**: `text/*`, `application/json|xml|csv`, types ending in `+json` or `+xml`, and for
//!   `application/octet-stream` (what many mailers send for any file) only the names ending in `txt csv md
//!   json xml log ics vcf`. UTF-8 as it is; otherwise the `charset` the type names, else Windows-1252, the
//!   usual older encoding of mail from Western Europe (a Latin-1 CSV of a bank, say). A NUL byte means binary.
//! - **PDF** (`application/pdf` or a name ending in `.pdf`): the text layer of every page through `lopdf`,
//!   with a bound on how far a stream may inflate, so a small file cannot make the core use gigabytes. A
//!   protected PDF and one without text (a scan) are not readable: there is no text recognition here.
//! - **Everything else** (pictures, Word, Excel …) is [`Unreadable::Unsupported`] with a sentence the model
//!   can pass on, so it tells the person instead of guessing.
//!
//! At most [`MAX_TEXT_FILE_BYTES`] of text are kept; the rest is cut and said so. Nothing here writes a file
//! or logs a name or text. What an attachment holds is data from outside, never instructions.

use encoding_rs::{Encoding, WINDOWS_1252};
use torro_core::i18n::{tr, trf};
use torro_ipc::mail::AttachmentKind;
use torro_ipc::tools::MAX_TEXT_FILE_BYTES;

/// How far one stream of a PDF may inflate while it is loaded or its text read: 64 MiB.
const PDF_INFLATE_LIMIT: usize = 64 * 1024 * 1024;

/// The name endings read as text when a mail sends a file as `application/octet-stream`.
const TEXT_ENDINGS: [&str; 8] = ["txt", "csv", "md", "json", "xml", "log", "ics", "vcf"];

/// The text taken out of an attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// What it was read as.
    pub kind: AttachmentKind,
    /// How many pages a PDF has.
    pub pages: Option<u32>,
    /// The text, at most [`MAX_TEXT_FILE_BYTES`].
    pub text: String,
    /// The text was longer and ends at [`MAX_TEXT_FILE_BYTES`].
    pub cut: bool,
}

/// Why an attachment gives no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// A kind of file Torro Workplace does not read, or a PDF it cannot read (protected, a scan); the sentence
    /// says which, for the person.
    Unsupported(String),
    /// A file of a kind it reads, but damaged.
    Broken,
}

/// The text of the attachment `bytes`, sent as `content_type` (`type/subtype`, perhaps with parameters) under the
/// file name `name`. Runs `lopdf` for a PDF: call it on a blocking thread.
pub fn extract(content_type: &str, name: Option<&str>, bytes: &[u8]) -> Result<Extracted, Unreadable> {
    let essence = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let ending = name.and_then(|name| name.rsplit_once('.')).map(|(_, end)| end.to_ascii_lowercase());
    if essence == "application/pdf" || ending.as_deref() == Some("pdf") {
        return pdf(bytes);
    }
    if is_text(&essence, ending.as_deref()) {
        let text = decode(bytes, charset(content_type))?;
        let (text, cut) = bounded(text);
        return Ok(Extracted { kind: AttachmentKind::Text, pages: None, text, cut });
    }
    let kind = if essence.is_empty() { "unknown" } else { essence.as_str() };
    Err(Unreadable::Unsupported(trf(
        "Torro Workplace cannot read this kind of file yet ({type}); it reads text and PDF attachments.",
        &[("type", &kind)],
    )))
}

/// Whether a file of the type `essence` with the name ending `ending` is read as text.
fn is_text(essence: &str, ending: Option<&str>) -> bool {
    let named_text = ending.is_some_and(|end| TEXT_ENDINGS.contains(&end));
    essence.starts_with("text/")
        || matches!(essence, "application/json" | "application/xml" | "application/csv")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || (named_text && matches!(essence, "application/octet-stream" | ""))
}

/// The `charset` parameter of `content_type`, when it names one.
fn charset(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        key.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches('"'))
    })
}

/// `bytes` as text: UTF-8 (a byte order mark left out), else in `charset`, else Windows-1252. A NUL byte, which no
/// text file holds, makes it unsupported.
fn decode(bytes: &[u8], charset: Option<&str>) -> Result<String, Unreadable> {
    if bytes.contains(&0) {
        return Err(Unreadable::Unsupported(tr("This attachment is not a text file Torro Workplace can read.").into()));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_string());
    }
    let encoding = charset.and_then(|label| Encoding::for_label(label.as_bytes())).unwrap_or(WINDOWS_1252);
    Ok(encoding.decode_without_bom_handling(bytes).0.into_owned())
}

/// The text of the PDF `bytes`, page after page.
fn pdf(bytes: &[u8]) -> Result<Extracted, Unreadable> {
    let protected =
        || Unreadable::Unsupported(tr("This PDF is protected with a password; Torro Workplace cannot read it.").into());
    let options = lopdf::LoadOptions::with_max_decompressed_size(PDF_INFLATE_LIMIT);
    // `lopdf` opens a PDF protected only against changes with the empty password itself; one that needs a
    // password to open fails to load or stays encrypted.
    let document = match lopdf::Document::load_mem_with_options(bytes, options) {
        Ok(document) => document,
        Err(_) if contains(bytes, b"/Encrypt") => return Err(protected()),
        Err(_) => return Err(Unreadable::Broken),
    };
    if document.is_encrypted() {
        return Err(protected());
    }
    let numbers: Vec<u32> = document.get_pages().keys().copied().collect();
    let text = document.extract_text_with_limit(&numbers, PDF_INFLATE_LIMIT).map_err(|_| Unreadable::Broken)?;
    if text.trim().is_empty() {
        return Err(Unreadable::Unsupported(
            tr("This PDF has no text layer, likely a scan; Torro Workplace cannot read it.").into(),
        ));
    }
    let (text, cut) = bounded(text);
    let pages = u32::try_from(numbers.len()).unwrap_or(u32::MAX);
    Ok(Extracted { kind: AttachmentKind::Pdf, pages: Some(pages), text, cut })
}

/// Whether `needle` occurs in `bytes`.
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// `text` within [`MAX_TEXT_FILE_BYTES`], cut at a character, and whether it was cut.
fn bounded(mut text: String) -> (String, bool) {
    let limit = usize::try_from(MAX_TEXT_FILE_BYTES).unwrap_or(usize::MAX);
    if text.len() <= limit {
        return (text, false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture of `tests/fixtures/attachments/`.
    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments").join(name);
        std::fs::read(&path).unwrap_or_else(|err| panic!("{path:?}: {err}"))
    }

    fn unsupported(result: Result<Extracted, Unreadable>) -> String {
        match result {
            Err(Unreadable::Unsupported(message)) => message,
            other => panic!("expected unsupported, got {other:?}"),
        }
    }

    #[test]
    fn a_pdf_with_text_is_read_page_after_page() {
        let read = extract("application/pdf", Some("Rechnung.pdf"), &fixture("invoice.pdf")).expect("read");
        assert_eq!((read.kind, read.pages, read.cut), (AttachmentKind::Pdf, Some(2), false));
        assert!(read.text.contains("Invoice 2026-118"), "{}", read.text);
        assert!(read.text.contains("Total 119.00 EUR"), "{}", read.text);
        // Many mailers send a PDF as octet-stream: the name decides.
        let named = extract("application/octet-stream", Some("Rechnung.PDF"), &fixture("invoice.pdf")).expect("read");
        assert_eq!(named.kind, AttachmentKind::Pdf);
    }

    #[test]
    fn a_scan_a_picture_and_word_are_not_read() {
        let scan = unsupported(extract("application/pdf", Some("scan.pdf"), &fixture("scan.pdf")));
        assert_eq!(scan, tr("This PDF has no text layer, likely a scan; Torro Workplace cannot read it."));
        let picture = unsupported(extract("image/png", Some("photo.png"), &fixture("pixel.png")));
        assert!(picture.contains("image/png"), "{picture}");
        let docx = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        assert!(unsupported(extract(docx, Some("letter.docx"), b"PK\x03\x04")).contains(docx));
        // A binary file named like text, and a PNG sent as octet-stream without a text name.
        assert_eq!(
            unsupported(extract("application/octet-stream", Some("notes.txt"), &fixture("pixel.png"))),
            tr("This attachment is not a text file Torro Workplace can read.")
        );
        assert!(unsupported(extract("application/octet-stream", Some("photo.png"), b"x")).contains("octet-stream"));
        assert_eq!(extract("application/pdf", None, b"%PDF-1.4 broken"), Err(Unreadable::Broken));
    }

    #[test]
    fn text_is_read_as_utf8_its_charset_or_windows_1252() {
        let csv = fixture("statement-latin1.csv");
        let read = extract("text/csv", Some("Umsätze.csv"), &csv).expect("read");
        assert_eq!(read.kind, AttachmentKind::Text);
        assert!(read.text.contains("Überweisung;Müller;-42,50 €"), "{}", read.text);
        let named = extract("application/octet-stream", Some("umsatz.csv"), &csv).expect("read");
        assert_eq!(named.text, read.text);
        let utf8 = extract("text/plain; charset=utf-8", None, "\u{feff}Grüße\n".as_bytes()).expect("read");
        assert_eq!(utf8.text, "Grüße\n");
        let latin2 = extract("text/plain; charset=\"iso-8859-2\"", None, b"\xb3").expect("read");
        assert_eq!(latin2.text, "ł");
        for kind in ["application/json", "application/ld+json", "application/xml", "image/svg+xml", "text/calendar"] {
            assert!(extract(kind, None, b"{}").is_ok(), "{kind}");
        }
    }

    #[test]
    fn a_long_text_is_cut_at_a_character() {
        let limit = usize::try_from(MAX_TEXT_FILE_BYTES).expect("fits");
        let long = format!("{}ü", "a".repeat(limit - 1));
        let read = extract("text/plain", None, long.as_bytes()).expect("read");
        assert_eq!((read.text.len(), read.cut), (limit - 1, true));
    }
}
