//! The warning signs of fraud and spam of one mail (issue #506, `docs/kategorien.md`, "Betrug
//! abfangen"), found without a model and without the network.
//!
//! [`signs`] reads what the core holds of a mail ([`SignFacts`]): the sender's name and address,
//! `Reply-To`, `Return-Path`, the sender checks of `Authentication-Results`, the links of its HTML
//! (or of its text), its attachments and its words. Each sign is a [`WarningSign`]: the suite's own
//! word for what looks wrong, at most with a brand of the fixed table below, never text of the mail.
//! [`score`] weighs them; from [`SUSPICION`] on the feed asks the decision model the narrow question
//! "fraud or not?" before it files the mail (`inbox_feed`).
//!
//! **Weights** (3 strong, 2 middle, 1 weak): a sender name naming a company the address does not
//! belong to, a look-alike domain, a failed DMARC check, a link whose text names another place, a
//! contact's name on a foreign address and attachments that run code weigh 3 – any one of them is
//! enough to ask. SPF or DKIM failed, replies to another domain, a free mail address with a brand
//! in its name, links to an IP address or through a shortener, Punycode, pressure and a changed bank
//! account weigh 2. The weak ones only count together: no sender check, another envelope sender,
//! an unusual top-level domain, a packed attachment, a mail that is only an image, no personal
//! greeting, the first mail of the address, `Re:` without a reply.
//!
//! **Fehlende Fakten geben kein Zeichen:** what the store did not read (mail kept before the update
//! has no sender checks and no reply reference), or what a caller does not know, says nothing.
//!
//! Pure: nothing is looked up, nothing is logged.

use std::collections::BTreeSet;

use torro_mail::{AttachmentInfo, AuthCheck, AuthResult, MailAddress, MessageTraits, WarningSign};

use super::fold::fold;

/// From this score on a mail of a stranger is suspicious enough for the narrow question to the model.
pub const SUSPICION: u32 = 3;

/// What [`signs`] reads of one mail.
#[derive(Debug, Clone, Copy, Default)]
pub struct SignFacts<'a> {
    /// `From:`.
    pub from: Option<&'a MailAddress>,
    /// `Reply-To:`.
    pub reply_to: &'a [MailAddress],
    pub subject: &'a str,
    /// The text, plain or made from HTML; empty when not at hand.
    pub text: &'a str,
    /// The cleaned HTML; `None` when the mail has none or it is not at hand: the links are then read
    /// from `text`.
    pub html: Option<&'a str>,
    pub attachments: &'a [AttachmentInfo],
    /// `Authentication-Results`, `Return-Path`, the reply reference.
    pub traits: Option<&'a MessageTraits>,
    /// The sender's name is that of a contact whose addresses do not hold this one; the caller asks
    /// the address books (`ContactsService::knows_name`).
    pub contact_name_foreign: bool,
    /// No earlier mail of this address is in the account; the caller asks the store.
    pub first_contact: bool,
}

impl<'a> SignFacts<'a> {
    /// The facts of `message` as the store holds it, with its cleaned `html` when at hand; the two
    /// that need the person's own data stay `false`.
    pub fn of(message: &'a torro_mail::Message, html: Option<&'a str>) -> SignFacts<'a> {
        SignFacts {
            from: message.summary.from.as_ref(),
            reply_to: &message.reply_to,
            subject: &message.summary.subject,
            text: message.text.as_deref().unwrap_or(&message.summary.snippet),
            html: html.or(message.html.as_deref()),
            attachments: &message.attachments,
            traits: Some(&message.traits),
            contact_name_foreign: false,
            first_contact: false,
        }
    }
}

/// The warning signs of the mail of `facts`, strongest first, each once.
pub fn signs(facts: &SignFacts<'_>) -> Vec<WarningSign> {
    let mut found: Vec<WarningSign> = Vec::new();
    let sender = facts.from.map(|from| from.address.trim().to_lowercase()).unwrap_or_default();
    let domain = domain_of(&sender);
    let name = facts.from.and_then(|from| from.name.as_deref()).map(fold).unwrap_or_default();
    sender_signs(&name, domain, &mut found);
    if let Some(traits) = facts.traits {
        header_signs(traits, domain, facts.subject, &mut found);
    }
    if let Some(domain) = domain {
        let replies = facts.reply_to.iter().filter_map(|to| domain_of_owned(&to.address));
        if replies.into_iter().any(|reply| registrable(&reply) != registrable(domain)) {
            found.push(WarningSign::ReplyToDiffers);
        }
    }
    if facts.contact_name_foreign {
        found.push(WarningSign::ContactNameForeign);
    }
    link_signs(facts, &mut found);
    attachment_signs(facts, &mut found);
    word_signs(facts, &mut found);
    if facts.first_contact {
        found.push(WarningSign::FirstContact);
    }
    ordered(found)
}

/// How much one sign weighs: 3 strong, 2 middle, 1 weak (module docs).
pub fn weight(sign: &WarningSign) -> u32 {
    match sign {
        WarningSign::BrandMismatch { .. }
        | WarningSign::Lookalike { .. }
        | WarningSign::AuthFailed { check: AuthCheck::Dmarc }
        | WarningSign::LinkMismatch
        | WarningSign::ContactNameForeign
        | WarningSign::RiskyAttachment
        | WarningSign::DoubleExtension => 3,
        WarningSign::AuthFailed { .. }
        | WarningSign::ReplyToDiffers
        | WarningSign::FreemailBrand { .. }
        | WarningSign::LinkIp
        | WarningSign::LinkShortener
        | WarningSign::Punycode
        | WarningSign::Pressure
        | WarningSign::PaymentChange => 2,
        WarningSign::AuthNone { .. }
        | WarningSign::ReturnPathDiffers
        | WarningSign::UnusualTld
        | WarningSign::ArchiveAttachment
        | WarningSign::ImageOnly
        | WarningSign::GenericGreeting
        | WarningSign::FirstContact
        | WarningSign::FakeReply => 1,
    }
}

/// The weight of `signs` together.
pub fn score(signs: &[WarningSign]) -> u32 {
    signs.iter().map(weight).sum()
}

/// `signs` with the two that need the person's own data (`contact_name_foreign`, `first_contact`)
/// added, strongest first: `mail.get_message` reads the mail without them and adds them only for the
/// app key, which may see the address books and every account.
pub fn with_acquaintance(
    mut signs: Vec<WarningSign>,
    contact_name_foreign: bool,
    first_contact: bool,
) -> Vec<WarningSign> {
    if contact_name_foreign {
        signs.push(WarningSign::ContactNameForeign);
    }
    if first_contact {
        signs.push(WarningSign::FirstContact);
    }
    ordered(signs)
}

/// Each sign once, the heaviest first; equal weights keep the order they were found in.
fn ordered(found: Vec<WarningSign>) -> Vec<WarningSign> {
    let mut seen = BTreeSet::new();
    let mut unique: Vec<WarningSign> =
        found.into_iter().filter(|sign| seen.insert(serde_json::to_string(sign).unwrap_or_default())).collect();
    unique.sort_by_key(|sign| std::cmp::Reverse(weight(sign)));
    unique
}

// ----- the sender -----

/// A company the suite knows: the words its name is written with (folded) and the domains its mail
/// comes from (with every domain below them).
struct Brand {
    name: &'static str,
    words: &'static [&'static str],
    domains: &'static [&'static str],
}

/// The brands whose names fraud uses most in Germany, Austria and Switzerland, and worldwide.
const BRANDS: &[Brand] = &[
    Brand {
        name: "PayPal",
        words: &["paypal"],
        domains: &["paypal.com", "paypal.de", "paypal.me", "paypal-communication.com", "paypal.at", "paypal.ch"],
    },
    Brand {
        name: "Amazon",
        words: &["amazon"],
        domains: &[
            "amazon.com",
            "amazon.de",
            "amazon.at",
            "amazon.co.uk",
            "amazon.fr",
            "amazon.it",
            "amazon.es",
            "amazon.nl",
            "amazonaws.com",
            "amazonses.com",
        ],
    },
    Brand {
        name: "Apple",
        words: &["apple", "icloud", "itunes"],
        domains: &["apple.com", "icloud.com", "me.com", "mac.com", "itunes.com"],
    },
    Brand {
        name: "Microsoft",
        words: &["microsoft", "outlook", "office 365", "office365", "onedrive"],
        domains: &[
            "microsoft.com",
            "office.com",
            "office365.com",
            "outlook.com",
            "live.com",
            "hotmail.com",
            "onedrive.com",
            "microsoftonline.com",
            "sharepoint.com",
        ],
    },
    Brand {
        name: "Google",
        words: &["google", "gmail"],
        domains: &["google.com", "google.de", "gmail.com", "googlemail.com", "youtube.com"],
    },
    Brand { name: "Netflix", words: &["netflix"], domains: &["netflix.com", "netflix.net"] },
    Brand {
        name: "DHL",
        words: &["dhl"],
        domains: &["dhl.com", "dhl.de", "dhl.at", "dhl.ch", "deutschepost.de", "dpdhl.com"],
    },
    Brand { name: "Deutsche Post", words: &["deutsche post"], domains: &["deutschepost.de", "dhl.de", "dpdhl.com"] },
    Brand { name: "DPD", words: &["dpd"], domains: &["dpd.com", "dpd.de", "dpd.at", "dpd.ch"] },
    Brand { name: "Hermes", words: &["hermes"], domains: &["hermesworld.com", "myhermes.de", "hermes-europe.eu"] },
    Brand { name: "UPS", words: &["ups"], domains: &["ups.com"] },
    Brand { name: "FedEx", words: &["fedex"], domains: &["fedex.com"] },
    Brand {
        name: "Sparkasse",
        words: &["sparkasse"],
        domains: &["sparkasse.de", "sparkasse.at", "s-finanzgruppe.de", "sparkassen.de"],
    },
    Brand {
        name: "Volksbank",
        words: &["volksbank", "raiffeisenbank"],
        domains: &["vr.de", "volksbank.de", "volksbank.at", "raiffeisen.at", "raiffeisen.ch"],
    },
    Brand { name: "Deutsche Bank", words: &["deutsche bank"], domains: &["db.com", "deutsche-bank.de"] },
    Brand { name: "Commerzbank", words: &["commerzbank"], domains: &["commerzbank.de", "commerzbank.com"] },
    Brand { name: "ING", words: &["ing diba", "ing-diba"], domains: &["ing.de", "ing.com", "ing-diba.de"] },
    Brand { name: "N26", words: &["n26"], domains: &["n26.com"] },
    Brand { name: "Postbank", words: &["postbank"], domains: &["postbank.de"] },
    Brand { name: "comdirect", words: &["comdirect"], domains: &["comdirect.de"] },
    Brand { name: "Erste Bank", words: &["erste bank"], domains: &["erstebank.at", "sparkasse.at", "george.at"] },
    Brand { name: "UBS", words: &["ubs"], domains: &["ubs.com"] },
    Brand { name: "PostFinance", words: &["postfinance"], domains: &["postfinance.ch"] },
    Brand { name: "Visa", words: &["visa"], domains: &["visa.com", "visa.de"] },
    Brand { name: "Mastercard", words: &["mastercard"], domains: &["mastercard.com", "mastercard.de"] },
    Brand {
        name: "American Express",
        words: &["american express", "amex"],
        domains: &["americanexpress.com", "aexp.com", "amex.de"],
    },
    Brand { name: "Telekom", words: &["telekom", "t-online"], domains: &["telekom.de", "t-online.de", "telekom.com"] },
    Brand { name: "Vodafone", words: &["vodafone"], domains: &["vodafone.de", "vodafone.com", "vodafone.at"] },
    Brand { name: "1&1", words: &["1&1", "1und1"], domains: &["1und1.de", "ionos.de", "1and1.com"] },
    Brand {
        name: "eBay",
        words: &["ebay"],
        domains: &["ebay.com", "ebay.de", "ebay.at", "ebay.ch", "ebay-kleinanzeigen.de", "kleinanzeigen.de"],
    },
    Brand { name: "Zalando", words: &["zalando"], domains: &["zalando.de", "zalando.at", "zalando.ch", "zalando.com"] },
    Brand {
        name: "Facebook",
        words: &["facebook", "instagram", "whatsapp"],
        domains: &["facebook.com", "facebookmail.com", "meta.com", "instagram.com", "whatsapp.com"],
    },
    Brand { name: "Elster", words: &["finanzamt", "bundeszentralamt"], domains: &["elster.de", "bzst.de", "bund.de"] },
    Brand { name: "Booking.com", words: &["booking.com"], domains: &["booking.com"] },
    Brand { name: "Steam", words: &["steam"], domains: &["steampowered.com", "steamcommunity.com"] },
];

/// Free mail providers: anyone gets an address there, so a brand in the name means nothing good.
const FREEMAIL: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "yahoo.com",
    "yahoo.de",
    "outlook.com",
    "outlook.de",
    "hotmail.com",
    "hotmail.de",
    "live.com",
    "live.de",
    "msn.com",
    "aol.com",
    "aol.de",
    "gmx.de",
    "gmx.net",
    "gmx.at",
    "gmx.ch",
    "gmx.com",
    "web.de",
    "t-online.de",
    "freenet.de",
    "posteo.de",
    "mailbox.org",
    "icloud.com",
    "me.com",
    "mail.com",
    "proton.me",
    "protonmail.com",
    "yandex.com",
    "yandex.ru",
    "mail.ru",
    "zoho.com",
    "bluewin.ch",
    "gmx.ch",
];

/// URL shorteners: the link hides where it goes.
const SHORTENERS: &[&str] = &[
    "bit.ly",
    "tinyurl.com",
    "t.co",
    "goo.gl",
    "ow.ly",
    "is.gd",
    "buff.ly",
    "rebrand.ly",
    "cutt.ly",
    "shorturl.at",
    "rb.gy",
    "tiny.cc",
    "s.id",
    "t.ly",
    "bl.ink",
    "short.io",
    "lnkd.in",
    "qrco.de",
    "v.gd",
];

/// Top-level domains mostly met in abuse (Spamhaus' and Interisle's lists of the worst ones).
const UNUSUAL_TLDS: &[&str] = &[
    "zip", "mov", "top", "xyz", "click", "link", "gq", "tk", "ml", "cf", "ga", "work", "rest", "bar", "cam", "icu",
    "buzz", "monster", "cyou", "sbs", "cfd", "lol", "quest", "surf", "support", "country", "kim", "loan", "men",
];

/// The domains below a country code that are sold like a top-level domain (`example.co.uk`).
const TWO_PART_SUFFIXES: &[&str] = &["co.uk", "com.au", "co.at", "or.at", "gv.at"];

/// The part after `@` of `address`, in small letters; `None` without one.
fn domain_of(address: &str) -> Option<&str> {
    address.rsplit_once('@').map(|(_, domain)| domain.trim_end_matches('.')).filter(|domain| !domain.is_empty())
}

fn domain_of_owned(address: &str) -> Option<String> {
    domain_of(&address.trim().to_lowercase()).map(str::to_string)
}

/// The domain a person registers, as `docs/kategorien.md` says it: the last two labels, three below
/// [`TWO_PART_SUFFIXES`] (`mail.shop.co.uk` → `shop.co.uk`).
pub fn registrable(domain: &str) -> String {
    let domain = domain.trim_end_matches('.').to_lowercase();
    let labels: Vec<&str> = domain.split('.').filter(|label| !label.is_empty()).collect();
    let two = labels.len() >= 2 && TWO_PART_SUFFIXES.contains(&labels[labels.len() - 2..].join(".").as_str());
    let keep = if two { 3 } else { 2 };
    labels[labels.len().saturating_sub(keep)..].join(".")
}

/// `domain` is one of `domains` or below one of them.
fn within(domain: &str, domains: &[&str]) -> bool {
    domains.iter().any(|own| domain == *own || domain.ends_with(&format!(".{own}")))
}

/// The brands `name` (folded) names, as whole words.
fn named_brands(name: &str) -> Vec<&'static Brand> {
    BRANDS.iter().filter(|brand| brand.words.iter().any(|word| has_word(name, &fold(word)))).collect()
}

/// `text` holds `word` with no letter or digit right before or after it.
fn has_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[at + word.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
}

/// The signs of the sender's name and domain.
fn sender_signs(name: &str, domain: Option<&str>, found: &mut Vec<WarningSign>) {
    let Some(domain) = domain else { return };
    let freemail = FREEMAIL.contains(&registrable(domain).as_str());
    for brand in named_brands(name) {
        // A free mail domain of the brand itself (`outlook.com`, `icloud.com`) is anyone's address:
        // a company writes from its own domains, never from a box everybody can get.
        if within(domain, brand.domains) && !freemail {
            continue;
        }
        found.push(WarningSign::BrandMismatch { brand: brand.name.into() });
        if freemail {
            found.push(WarningSign::FreemailBrand { brand: brand.name.into() });
        }
    }
    if let Some(brand) = lookalike(domain) {
        found.push(WarningSign::Lookalike { brand: brand.name.into() });
    }
    if domain.split('.').any(|label| label.starts_with("xn--")) || !domain.is_ascii() {
        found.push(WarningSign::Punycode);
    }
    if unusual_tld(domain) {
        found.push(WarningSign::UnusualTld);
    }
}

/// The brand `domain` only looks like (module docs), while it is none of the brand's: a label of
/// it, or a part of one between hyphens, is the brand's word after folding look-alike letters
/// (`paypa1`, `rn` for `m`, also the word itself below a foreign domain, `paypal.com.login.example`),
/// one letter swapped or added (`netfliix`), one left out of a long word (`amazn`), or holds a long
/// brand word with more letters on (`paypalsecure`). The top-level domain is no label here. Words that
/// are ordinary words too ([`ORDINARY_WORDS`]) never count in a domain: `apple-farm.de` is no Apple.
fn lookalike(domain: &str) -> Option<&'static Brand> {
    let labels: Vec<&str> = domain.split('.').collect();
    let labels = &labels[..labels.len().saturating_sub(1)];
    BRANDS.iter().find(|brand| {
        if within(domain, brand.domains) {
            return false;
        }
        let words: Vec<String> = brand
            .words
            .iter()
            .map(|word| confusables(&word.replace([' ', '-', '.', '&'], "")))
            .filter(|word| word.len() >= 3)
            .collect();
        labels.iter().flat_map(|label| label.split('-').chain(std::iter::once(*label))).any(|part| {
            let folded = confusables(part);
            words.iter().any(|word| {
                let ordinary = ORDINARY_WORDS.contains(&word.as_str());
                let exact = folded == *word && !ordinary;
                // A letter swapped or one more; one less only of a long word (`amazn`, `paypl` for
                // words of six letters), never of a short one: `mail` is no `gmail`.
                let shorter = folded.len() + 1 == word.len() && word.len() >= 6;
                let near = !ordinary
                    && word.len() >= 5
                    && (folded.len() >= word.len() || shorter)
                    && folded != *word
                    && distance(&folded, word) <= 1;
                let joined =
                    !ordinary && word.len() >= 6 && folded.len() > word.len() && folded.contains(word.as_str());
                exact || near || joined
            })
        })
    })
}

/// Brand words that are ordinary words or names as well: in a domain they say nothing.
const ORDINARY_WORDS: &[&str] = &["apple", "steam", "hermes", "visa", "outlook"];

/// `label` with the letters fraud swaps for look-alikes put back: `0` → `o`, `1` → `l`, `rn` → `m`,
/// `vv` → `w`.
fn confusables(label: &str) -> String {
    label.replace("rn", "m").replace("vv", "w").replace('0', "o").replace('1', "l")
}

/// How many letters two words are apart (Levenshtein).
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut last = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (row[j + 1] + 1).min(row[j] + 1).min(last + usize::from(ca != cb));
            last = old;
        }
    }
    row[b.len()]
}

fn unusual_tld(host: &str) -> bool {
    host.rsplit('.').next().is_some_and(|tld| UNUSUAL_TLDS.contains(&tld))
}

// ----- the headers -----

/// The signs of the sender checks, the envelope's sender and a reply that answers nothing.
fn header_signs(traits: &MessageTraits, domain: Option<&str>, subject: &str, found: &mut Vec<WarningSign>) {
    if let Some(auth) = &traits.auth {
        for check in AuthCheck::ALL {
            if auth.failed(check) {
                found.push(WarningSign::AuthFailed { check });
            }
        }
        // A domain that takes part in no check at all is common: one sign, however many say none.
        if let Some(check) = AuthCheck::ALL.into_iter().find(|check| auth.of(*check) == Some(AuthResult::None)) {
            found.push(WarningSign::AuthNone { check });
        }
    }
    if let (Some(path), Some(domain)) = (traits.return_path.as_deref(), domain)
        && let Some(envelope) = domain_of(path)
        && registrable(envelope) != registrable(domain)
    {
        found.push(WarningSign::ReturnPathDiffers);
    }
    let subject = subject.trim().to_lowercase();
    let says_reply = ["re:", "aw:", "antw:", "sv:", "wg: re:"].iter().any(|prefix| subject.starts_with(prefix));
    if says_reply && traits.reply_ref == Some(false) {
        found.push(WarningSign::FakeReply);
    }
}

// ----- the links -----

/// One link of the mail: the text it shows (empty in plain text) and where it goes.
struct Link {
    text: String,
    href: String,
}

/// The `http` and `https` links of the mail: of its HTML (`a[href]`), else of its text.
fn links(facts: &SignFacts<'_>) -> Vec<Link> {
    match facts.html {
        Some(html) => {
            let document = dom_query::Document::fragment(html);
            document
                .select("a[href]")
                .iter()
                .filter_map(|anchor| {
                    let href = anchor.attr("href")?.trim().to_string();
                    Some(Link { text: anchor.text().trim().to_string(), href })
                })
                .filter(|link| is_web(&link.href))
                .take(LINKS_MAX)
                .collect()
        }
        None => facts
            .text
            .split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']'))
            .filter(|word| is_web(word))
            .map(|word| Link {
                text: String::new(),
                href: word.trim_end_matches(['.', ',', ';', ':', '!', '?']).to_string(),
            })
            .take(LINKS_MAX)
            .collect(),
    }
}

/// The most links of one mail that are looked at: a mail of thousands of links stays cheap.
const LINKS_MAX: usize = 200;

fn is_web(href: &str) -> bool {
    let lower = href.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The host a link goes to, in small letters, Punycode for letters beyond ASCII (`url` writes them so).
fn host_of(href: &str) -> Option<url::Host<String>> {
    url::Url::parse(href.trim()).ok()?.host().map(|host| host.to_owned())
}

/// The host a link's text names, when it reads like an address (`www.paypal.com`,
/// `https://paypal.com/login`, `paypal.de`); `None` for words, and for a file name like
/// `Rechnung.pdf`: without a scheme or `www.` the text must end in a top-level domain the suite
/// knows ([`TEXT_TLDS`], [`UNUSUAL_TLDS`]).
fn host_in_text(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('/');
    if text.contains(char::is_whitespace) || !text.contains('.') {
        return None;
    }
    let marked = is_web(text) || text.to_ascii_lowercase().starts_with("www.");
    let with_scheme = if is_web(text) { text.to_string() } else { format!("https://{text}") };
    let url::Host::Domain(domain) = host_of(&with_scheme)? else { return None };
    let tld = domain.rsplit('.').next().unwrap_or_default();
    (marked || TEXT_TLDS.contains(&tld) || UNUSUAL_TLDS.contains(&tld)).then_some(domain)
}

/// Top-level domains a link's text without a scheme is read as an address with.
const TEXT_TLDS: &[&str] = &[
    "com", "de", "at", "ch", "net", "org", "eu", "info", "io", "uk", "fr", "it", "es", "nl", "be", "pl", "ru", "me",
    "co", "us", "biz", "app", "online", "shop",
];

fn link_signs(facts: &SignFacts<'_>, found: &mut Vec<WarningSign>) {
    for link in links(facts) {
        let Some(host) = host_of(&link.href) else { continue };
        match &host {
            url::Host::Ipv4(_) | url::Host::Ipv6(_) => found.push(WarningSign::LinkIp),
            url::Host::Domain(domain) => {
                if SHORTENERS.contains(&registrable(domain).as_str()) {
                    found.push(WarningSign::LinkShortener);
                }
                if domain.split('.').any(|label| label.starts_with("xn--")) {
                    found.push(WarningSign::Punycode);
                }
                if unusual_tld(domain) {
                    found.push(WarningSign::UnusualTld);
                }
                if let Some(shown) = host_in_text(&link.text)
                    && registrable(&shown) != registrable(domain)
                {
                    found.push(WarningSign::LinkMismatch);
                }
            }
        }
    }
}

// ----- the attachments -----

/// Endings of files that open a web page or run code when opened.
const RISKY: &[&str] = &[
    "html", "htm", "shtml", "xhtml", "svg", "iso", "img", "vhd", "exe", "scr", "js", "jse", "vbs", "vbe", "lnk", "msi",
    "bat", "cmd", "com", "pif", "hta", "ps1", "jar", "wsf", "cpl", "one",
];

/// Endings of packed files, whose content is not seen.
const ARCHIVES: &[&str] = &["zip", "rar", "7z", "gz", "tgz", "tar", "ace", "cab", "arj", "xz", "bz2"];

/// Endings a file shows to look harmless in front of the real one (`invoice.pdf.exe`).
const DOCUMENTS: &[&str] =
    &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "jpg", "jpeg", "png", "txt", "odt", "rtf", "csv"];

fn attachment_signs(facts: &SignFacts<'_>, found: &mut Vec<WarningSign>) {
    for part in facts.attachments.iter().filter(|part| !part.inline) {
        let Some(name) = part.filename.as_deref() else { continue };
        let name = name.trim().trim_end_matches(['.', ' ']).to_lowercase();
        let parts: Vec<&str> = name.split('.').collect();
        let Some(ending) = parts.last().filter(|_| parts.len() >= 2) else { continue };
        if RISKY.contains(ending) || part.content_type.eq_ignore_ascii_case("text/html") {
            found.push(WarningSign::RiskyAttachment);
        }
        if ARCHIVES.contains(ending) {
            found.push(WarningSign::ArchiveAttachment);
        }
        let inner = parts.len() >= 3 && DOCUMENTS.contains(&parts[parts.len() - 2]);
        if inner && !DOCUMENTS.contains(ending) {
            found.push(WarningSign::DoubleExtension);
        }
    }
    let image = facts.attachments.iter().any(|part| part.content_type.to_ascii_lowercase().starts_with("image/"));
    let words = facts.text.chars().filter(|c| c.is_alphanumeric()).count();
    if image && words < IMAGE_ONLY_TEXT {
        found.push(WarningSign::ImageOnly);
    }
}

/// Fewer letters and digits than this beside an image make a mail that is only an image.
const IMAGE_ONLY_TEXT: usize = 40;

// ----- the words -----

/// Words that put pressure on the reader (folded on use).
const PRESSURE: &[&str] = &[
    "verify your account",
    "confirm your account",
    "confirm your identity",
    "verify your identity",
    "update your account",
    "account has been suspended",
    "account will be suspended",
    "account will be closed",
    "account has been locked",
    "account is locked",
    "unusual activity",
    "unusual sign-in",
    "suspicious activity",
    "within 24 hours",
    "within 48 hours",
    "final notice",
    "last reminder",
    "confirm your password",
    "your password expires",
    "konto verifizieren",
    "konto bestatigen",
    "konto uberprufen",
    "konto prufen",
    "zugangsdaten bestatigen",
    "zugangsdaten prufen",
    "daten bestatigen",
    "daten aktualisieren",
    "identitat bestatigen",
    "identitat verifizieren",
    "konto wurde gesperrt",
    "konto wird gesperrt",
    "sperrung ihres",
    "sperrung deines",
    "vorubergehend gesperrt",
    "innerhalb von 24 stunden",
    "innerhalb von 48 stunden",
    "letzte mahnung",
    "letzte erinnerung",
    "ungewohnliche aktivitat",
    "verdachtige aktivitat",
    "sicherheitsuberprufung",
];

/// Words of a changed bank account or of a boss asking for money (the "Chef-Masche").
const PAYMENT: &[&str] = &[
    "new bank details",
    "changed our bank",
    "change of bank details",
    "updated bank details",
    "new bank account",
    "new account details",
    "wire transfer",
    "urgent payment",
    "urgent transfer",
    "gift card",
    "gift cards",
    "are you at your desk",
    "are you available",
    "neue bankverbindung",
    "bankverbindung geandert",
    "bankverbindung hat sich geandert",
    "anderung der bankverbindung",
    "neues konto",
    "neue iban",
    "iban geandert",
    "dringende uberweisung",
    "uberweisung tatigen",
    "gutscheinkarte",
    "geschenkkarte",
    "bist du gerade erreichbar",
    "sind sie gerade erreichbar",
    "vertraulich behandeln",
];

/// Greetings that name nobody.
const GENERIC_GREETINGS: &[&str] = &[
    "dear customer",
    "dear client",
    "dear user",
    "dear member",
    "dear valued customer",
    "dear account holder",
    "dear sir or madam",
    "dear sir/madam",
    "hello customer",
    "sehr geehrter kunde",
    "sehr geehrte kundin",
    "sehr geehrte/r kunde",
    "sehr geehrte(r) kunde",
    "lieber kunde",
    "liebe kundin",
    "hallo kunde",
    "sehr geehrter nutzer",
    "sehr geehrte nutzerin",
    "lieber nutzer",
    "sehr geehrte damen und herren",
    "geschatzter kunde",
];

/// How much of the start of the text a greeting is looked for in.
const GREETING_HEAD: usize = 300;

fn word_signs(facts: &SignFacts<'_>, found: &mut Vec<WarningSign>) {
    let text = fold(&format!("{}\n{}", facts.subject, super::fold::head(facts.text, 20_000)));
    let holds = |phrases: &[&str]| phrases.iter().any(|phrase| text.contains(&fold(phrase)));
    if holds(PRESSURE) {
        found.push(WarningSign::Pressure);
    }
    if holds(PAYMENT) {
        found.push(WarningSign::PaymentChange);
    }
    let start = fold(super::fold::head(facts.text.trim_start(), GREETING_HEAD));
    if GENERIC_GREETINGS.iter().any(|greeting| start.contains(&fold(greeting))) {
        found.push(WarningSign::GenericGreeting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_mail::AuthResults;

    fn address(name: &str, address: &str) -> MailAddress {
        MailAddress { name: (!name.is_empty()).then(|| name.to_string()), address: address.into() }
    }

    fn signs_of(from: &MailAddress, text: &str) -> Vec<WarningSign> {
        signs(&SignFacts { from: Some(from), text, ..SignFacts::default() })
    }

    #[test]
    fn a_brand_in_the_name_on_a_foreign_domain_is_a_mismatch_and_its_own_domain_is_none() {
        let fake = address("PayPal Service", "service@secure-mail.example");
        assert!(signs_of(&fake, "").contains(&WarningSign::BrandMismatch { brand: "PayPal".into() }));
        let real = address("PayPal", "service@mail.paypal.de");
        assert!(signs_of(&real, "").is_empty(), "{:?}", signs_of(&real, ""));
        // A brand's own free mail domain is anyone's address (review of #506).
        let outlook = signs_of(&address("Microsoft Account Team", "ms.security123@outlook.com"), "");
        assert!(outlook.contains(&WarningSign::FreemailBrand { brand: "Microsoft".into() }), "{outlook:?}");
        let free = address("Amazon Kundendienst", "amazon.hilfe@gmail.com");
        let found = signs_of(&free, "");
        assert!(found.contains(&WarningSign::FreemailBrand { brand: "Amazon".into() }), "{found:?}");
        // A word inside another word names no brand: "Upstairs" is not UPS.
        assert!(signs_of(&address("Upstairs Cafe", "hi@cafe.example"), "").is_empty());
    }

    #[test]
    fn domains_that_only_look_like_a_brand_are_lookalikes() {
        for domain in
            ["paypa1.com", "paypal-secure.com", "arnazon.de", "paypal.com.login-check.ru", "netfliix.com", "amazn.com"]
        {
            let found = signs_of(&address("", &format!("info@{domain}")), "");
            assert!(found.iter().any(|sign| matches!(sign, WarningSign::Lookalike { .. })), "{domain}: {found:?}");
        }
        for domain in ["paypal.com", "mail.amazon.de", "dpd.de", "shop.example", "github.com", "upsilon.example"] {
            let found = signs_of(&address("", &format!("info@{domain}")), "");
            assert!(!found.iter().any(|sign| matches!(sign, WarningSign::Lookalike { .. })), "{domain}: {found:?}");
        }
    }

    #[test]
    fn registrable_domains_keep_two_labels_and_three_below_known_suffixes() {
        assert_eq!(registrable("mail.shop.example"), "shop.example");
        assert_eq!(registrable("news.shop.co.uk"), "shop.co.uk");
        assert_eq!(registrable("example"), "example");
    }

    #[test]
    fn failed_checks_and_headers_give_their_signs_and_unread_ones_none() {
        let from = address("", "info@bank.example");
        let traits = MessageTraits {
            auth: Some(AuthResults {
                spf: Some(AuthResult::Fail),
                dkim: Some(AuthResult::None),
                dmarc: Some(AuthResult::Fail),
            }),
            return_path: Some("bounce@other.example".into()),
            reply_ref: Some(false),
            ..MessageTraits::default()
        };
        let reply_to = [address("", "pay@elsewhere.example")];
        let facts = SignFacts {
            from: Some(&from),
            reply_to: &reply_to,
            subject: "Re: Ihre Rechnung",
            traits: Some(&traits),
            ..SignFacts::default()
        };
        let found = signs(&facts);
        for sign in [
            WarningSign::AuthFailed { check: AuthCheck::Dmarc },
            WarningSign::AuthFailed { check: AuthCheck::Spf },
            WarningSign::AuthNone { check: AuthCheck::Dkim },
            WarningSign::ReturnPathDiffers,
            WarningSign::ReplyToDiffers,
            WarningSign::FakeReply,
        ] {
            assert!(found.contains(&sign), "{sign:?} in {found:?}");
        }
        assert_eq!(found[0], WarningSign::AuthFailed { check: AuthCheck::Dmarc }, "strongest first");
        // Mail kept before the update: nothing read, nothing said.
        let old = MessageTraits::default();
        let facts =
            SignFacts { from: Some(&from), subject: "Re: Ihre Rechnung", traits: Some(&old), ..SignFacts::default() };
        assert!(signs(&facts).is_empty());
    }

    #[test]
    fn links_say_where_they_really_go() {
        let from = address("", "info@shop.example");
        let html = r#"<p><a href="https://login.evil.example/paypal">www.paypal.com</a>
            <a href="http://192.0.2.7/x">Login</a> <a href="https://bit.ly/abc">here</a>
            <a href="https://xn--pypal-4ve.com/">PayPal</a> <a href="https://shop.example/a">shop.example</a></p>"#;
        let facts = SignFacts { from: Some(&from), html: Some(html), ..SignFacts::default() };
        let found = signs(&facts);
        for sign in [WarningSign::LinkMismatch, WarningSign::LinkIp, WarningSign::LinkShortener, WarningSign::Punycode]
        {
            assert!(found.contains(&sign), "{sign:?} in {found:?}");
        }
        let plain = signs_of(&from, "Bitte hier: http://203.0.113.4/login.");
        assert!(plain.contains(&WarningSign::LinkIp), "{plain:?}");
        let honest = SignFacts {
            from: Some(&from),
            html: Some(r#"<a href="https://www.shop.example/">shop.example</a>"#),
            ..SignFacts::default()
        };
        assert!(signs(&honest).is_empty());
    }

    #[test]
    fn attachments_that_run_code_or_hide_their_ending_are_signs() {
        let part = |name: &str, kind: &str| AttachmentInfo {
            part: "2".into(),
            filename: Some(name.into()),
            content_type: kind.into(),
            size: 10,
            inline: false,
            content_id: None,
        };
        let parts = [
            part("Rechnung.pdf.exe", "application/octet-stream"),
            part("sendung.html", "text/html"),
            part("docs.zip", "application/zip"),
        ];
        let found = signs(&SignFacts {
            attachments: &parts,
            text: "Anbei die Unterlagen zu Ihrer Bestellung, bitte öffnen.",
            ..SignFacts::default()
        });
        for sign in [WarningSign::DoubleExtension, WarningSign::RiskyAttachment, WarningSign::ArchiveAttachment] {
            assert!(found.contains(&sign), "{sign:?} in {found:?}");
        }
        let harmless = [part("Rechnung.pdf", "application/pdf")];
        assert!(
            signs(&SignFacts {
                attachments: &harmless,
                text: "Anbei die Rechnung zu Ihrer Bestellung vom Montag.",
                ..SignFacts::default()
            })
            .is_empty()
        );
        let qr = [part("code.png", "image/png")];
        assert!(
            signs(&SignFacts { attachments: &qr, text: "Scannen!", ..SignFacts::default() })
                .contains(&WarningSign::ImageOnly)
        );
    }

    #[test]
    fn words_of_pressure_payment_and_a_greeting_to_nobody_are_signs() {
        let from = address("", "a@b.example");
        let found = signs_of(&from, "Sehr geehrter Kunde,\nIhr Konto wurde gesperrt. Bitte Zugangsdaten bestätigen.");
        assert!(found.contains(&WarningSign::Pressure) && found.contains(&WarningSign::GenericGreeting), "{found:?}");
        let boss = signs_of(&from, "Hallo, bist du gerade erreichbar? Ich brauche eine dringende Überweisung.");
        assert!(boss.contains(&WarningSign::PaymentChange), "{boss:?}");
        assert!(signs_of(&from, "Hallo Anna, bis morgen beim Essen!").is_empty());
    }

    /// The signs of a fixture under `tests/fixtures/fraud/`, read as the sync reads mail.
    fn fixture_signs(name: &str) -> Vec<WarningSign> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fraud").join(name);
        let raw = std::fs::read(&path).expect("the fixture");
        let read = torro_mail_imap::mime::read(&raw).expect("a mail");
        signs(&SignFacts {
            from: read.from.as_ref(),
            reply_to: &read.reply_to,
            subject: &read.subject,
            text: &read.text,
            html: read.html.as_deref(),
            attachments: &read.attachments,
            traits: Some(&read.traits),
            contact_name_foreign: false,
            first_contact: false,
        })
    }

    /// Issue #506: the typical patterns of fraud reach the suspicion on their own, each with the sign
    /// that gives it away.
    #[test]
    fn typical_fraud_reaches_the_suspicion() {
        for (name, telling) in [
            ("fraud-paypal-lookalike.eml", WarningSign::Lookalike { brand: "PayPal".into() }),
            ("fraud-paypal-lookalike.eml", WarningSign::LinkMismatch),
            ("fraud-dhl-attachment.eml", WarningSign::RiskyAttachment),
            ("fraud-dhl-attachment.eml", WarningSign::AuthFailed { check: AuthCheck::Dmarc }),
            ("fraud-boss.eml", WarningSign::PaymentChange),
            ("fraud-boss.eml", WarningSign::ReplyToDiffers),
            ("fraud-bank-dmarc.eml", WarningSign::AuthFailed { check: AuthCheck::Dmarc }),
            ("fraud-bank-dmarc.eml", WarningSign::LinkIp),
        ] {
            let found = fixture_signs(name);
            assert!(score(&found) >= SUSPICION, "{name}: {found:?}");
            assert!(found.contains(&telling), "{name}: {telling:?} in {found:?}");
        }
    }

    /// Issue #506: mail of people the person knows can look suspicious – a private address with
    /// `Reply-To`, a forwarder that breaks SPF, a newsletter service in a colleague's name. That is
    /// why acquaintance comes before the signs (`inbox_feed::acquainted`); the signs of plain mail
    /// without anything odd stay below the suspicion.
    #[test]
    fn mail_of_acquaintances_can_carry_signs_and_plain_mail_stays_below() {
        assert!(score(&fixture_signs("known-newsletter-service.eml")) >= SUSPICION, "only acquaintance saves it");
        let forwarded = fixture_signs("known-forwarded-no-spf.eml");
        assert!(forwarded.contains(&WarningSign::AuthFailed { check: AuthCheck::Spf }), "{forwarded:?}");
        let private = fixture_signs("known-private-reply-to.eml");
        assert!(private.contains(&WarningSign::ReplyToDiffers), "{private:?}");
        let plain = fixture_signs("known-no-spf.eml");
        assert!(score(&plain) < SUSPICION, "{plain:?}");
    }

    #[test]
    fn scores_add_up_and_one_strong_sign_reaches_suspicion() {
        assert_eq!(score(&[WarningSign::BrandMismatch { brand: "PayPal".into() }]), SUSPICION);
        assert!(score(&[WarningSign::FirstContact, WarningSign::GenericGreeting]) < SUSPICION);
        let added = with_acquaintance(vec![WarningSign::FirstContact], true, false);
        assert_eq!(added, [WarningSign::ContactNameForeign, WarningSign::FirstContact]);
    }
}
