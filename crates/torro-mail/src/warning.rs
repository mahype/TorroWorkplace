//! Warning signs of fraud and spam (issue #506, `docs/kategorien.md`, "Betrug abfangen").
//!
//! The core looks at every new mail from someone the person does not know for these signs, without
//! a model and without the network: a sender name that names a company the address does not belong
//! to, a failed sender check, a link whose text names another place than it goes to, an attachment
//! that runs code … ([`WarningSign`]). They go to the decision model as facts, decide whether the
//! narrow "fraud or not?" question is asked at all, and the mail window shows them in the reader
//! (`mail.get_message`, [`crate::Message::warnings`]).
//!
//! **Signs are the suite's words, not the mail's.** A sign names a kind and at most a brand from the
//! core's own fixed table ([`WarningSign::BrandMismatch`]) or the sender check that failed
//! ([`AuthCheck`]); it never carries text of the mail. The weighing lives in the core.
//!
//! The results of the sender checks ([`AuthResults`]) are read from the top-most
//! `Authentication-Results` header, the one the receiving server wrote last
//! ([`crate::MessageTraits::auth`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One of the three sender checks a receiving server writes into `Authentication-Results`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuthCheck {
    /// SPF: the server that sent it may send for the envelope's domain.
    Spf,
    /// DKIM: the mail is signed by its domain and unchanged since.
    Dkim,
    /// DMARC: SPF or DKIM passed for the domain of `From`, the address a person reads.
    Dmarc,
}

impl AuthCheck {
    /// All three, in the order the reader names them.
    pub const ALL: [AuthCheck; 3] = [AuthCheck::Dmarc, AuthCheck::Spf, AuthCheck::Dkim];

    /// The check's name as people know it: `SPF`, `DKIM`, `DMARC`.
    pub fn label(self) -> &'static str {
        match self {
            AuthCheck::Spf => "SPF",
            AuthCheck::Dkim => "DKIM",
            AuthCheck::Dmarc => "DMARC",
        }
    }
}

/// What a sender check said (RFC 8601): `pass`, `fail`, `softfail`, `neutral`, `none`; anything else
/// (`temperror`, `permerror`, `policy` …) is `other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuthResult {
    Pass,
    Fail,
    Softfail,
    Neutral,
    /// The check found nothing to check against: the domain publishes no record, or the mail is
    /// not signed.
    None,
    Other,
}

impl AuthResult {
    /// The result of its word in the header, in any case; `other` for a word RFC 8601 names
    /// besides these, and for anything else.
    pub fn of_word(word: &str) -> AuthResult {
        match word.trim().to_ascii_lowercase().as_str() {
            "pass" => AuthResult::Pass,
            "fail" | "hardfail" => AuthResult::Fail,
            "softfail" => AuthResult::Softfail,
            "neutral" => AuthResult::Neutral,
            "none" => AuthResult::None,
            _ => AuthResult::Other,
        }
    }
}

/// The sender checks of the top-most `Authentication-Results` header. A check the header does not
/// name is `None`; all three `None` when the mail has no such header at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct AuthResults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spf: Option<AuthResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dkim: Option<AuthResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dmarc: Option<AuthResult>,
}

impl AuthResults {
    /// What `check` said; `None` when the header does not name it.
    pub fn of(&self, check: AuthCheck) -> Option<AuthResult> {
        match check {
            AuthCheck::Spf => self.spf,
            AuthCheck::Dkim => self.dkim,
            AuthCheck::Dmarc => self.dmarc,
        }
    }

    /// Whether `check` failed outright: `fail`, or `softfail` for SPF (a domain that asks to treat
    /// strangers with suspicion).
    pub fn failed(&self, check: AuthCheck) -> bool {
        matches!(self.of(check), Some(AuthResult::Fail))
            || (check == AuthCheck::Spf && self.spf == Some(AuthResult::Softfail))
    }
}

/// One warning sign of a mail (issue #506). Each is a fact the core found in the headers, the
/// links, the attachments or the words of the mail, without a model. A `brand` is a name from the
/// core's own table of companies (`PayPal`, `DHL` …), never the mail's text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WarningSign {
    /// The sender's name names `brand`, the address does not belong to it.
    BrandMismatch { brand: String },
    /// The sender's domain only looks like one of `brand` (`paypa1.com`, `rn` for `m`, the brand plus
    /// a word).
    Lookalike { brand: String },
    /// A free mail address (`gmail.com`, `web.de` …) with `brand` in its name.
    FreemailBrand { brand: String },
    /// The sender check `check` failed.
    AuthFailed { check: AuthCheck },
    /// The receiving server ran `check` and found nothing to check: the sender's domain does not
    /// take part.
    AuthNone { check: AuthCheck },
    /// Replies go to another domain than the sender's (`Reply-To`).
    ReplyToDiffers,
    /// The envelope's sender (`Return-Path`) is of another domain than `From`.
    ReturnPathDiffers,
    /// The name of the sender is one of the person's contacts, the address is not.
    ContactNameForeign,
    /// A link names another place in its text than it goes to.
    LinkMismatch,
    /// A link goes to a bare IP address.
    LinkIp,
    /// A link goes through a URL shortener, which hides where it leads.
    LinkShortener,
    /// The sender or a link is of a top-level domain mostly used for abuse (`.zip`, `.top` …).
    UnusualTld,
    /// The sender or a link is written in Punycode (`xn--`): letters of other scripts that look
    /// like Latin ones.
    Punycode,
    /// An attachment that opens a web page or runs code (`.html`, `.iso`, `.exe` …).
    RiskyAttachment,
    /// An attachment with two endings (`invoice.pdf.exe`).
    DoubleExtension,
    /// A packed attachment (`.zip`, `.rar`, `.7z`): its content is not seen.
    ArchiveAttachment,
    /// The mail shows only an image (a QR code, a picture of a letter) and next to no text.
    ImageOnly,
    /// Words that put pressure on: check the account, confirm the access data, a lock, a deadline.
    Pressure,
    /// It asks to pay to a new account or change bank details, or passes as the boss asking for a
    /// payment.
    PaymentChange,
    /// No personal greeting ("Dear customer").
    GenericGreeting,
    /// The first mail from this address.
    FirstContact,
    /// The subject says `Re:`, but the mail answers nothing.
    FakeReply,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_name_their_kind_and_only_a_brand_or_a_check() {
        let brand = WarningSign::BrandMismatch { brand: "PayPal".into() };
        assert_eq!(
            serde_json::to_value(&brand).expect("json"),
            serde_json::json!({"kind": "brand_mismatch", "brand": "PayPal"})
        );
        let check = WarningSign::AuthFailed { check: AuthCheck::Dmarc };
        assert_eq!(
            serde_json::to_value(&check).expect("json"),
            serde_json::json!({"kind": "auth_failed", "check": "dmarc"})
        );
        assert_eq!(
            serde_json::to_value(WarningSign::FakeReply).expect("json"),
            serde_json::json!({"kind": "fake_reply"})
        );
        let back: WarningSign = serde_json::from_value(serde_json::json!({"kind": "link_ip"})).expect("read");
        assert_eq!(back, WarningSign::LinkIp);
    }

    #[test]
    fn results_read_their_words_and_spf_softfail_counts_as_failed() {
        assert_eq!(AuthResult::of_word("PASS"), AuthResult::Pass);
        assert_eq!(AuthResult::of_word("permerror"), AuthResult::Other);
        let results = AuthResults { spf: Some(AuthResult::Softfail), dkim: Some(AuthResult::Pass), dmarc: None };
        assert!(results.failed(AuthCheck::Spf));
        assert!(!results.failed(AuthCheck::Dkim) && !results.failed(AuthCheck::Dmarc));
        let json = serde_json::to_value(results).expect("json");
        assert_eq!(json, serde_json::json!({"spf": "softfail", "dkim": "pass"}));
    }
}
