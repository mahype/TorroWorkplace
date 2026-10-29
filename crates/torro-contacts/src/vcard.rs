//! Reading vCards 2.1, 3.0 and 4.0, and changing them without losing what we do not know.
//!
//! Other clients write properties this crate has never heard of (`X-…`, `RELATED`, parameters of
//! their own). The one rule: **a change touches only the properties whose value changed**; every
//! other line stays in text and order, byte for byte, folding and line endings included
//! ([`patch`]). That is the only way this crate writes an existing vCard.
//!
//! The reader is our own and small (`calcard` was considered: it rebuilds a card from its parsed
//! form, which loses the exact text of the lines we do not change). It understands folding,
//! escapes, `CHARSET` and quoted-printable of vCard 2.1, group prefixes (`item1.EMAIL`) with Apple's
//! labels (`X-ABLabel`), and birthdays without a year (`--0415`, Apple's `1604-04-15`).
//!
//! Groups of contacts are `CATEGORIES`. iCloud keeps groups as cards of their own
//! (`X-ADDRESSBOOKSERVER-KIND:group`); [`group_card`] reads them, writing them is a later stage.

use std::fmt;

use base64::Engine;

use crate::model::{
    Contact, ContactFields, FieldKind, FieldsError, Geo, Labeled, Name, PartialDate, PhotoRef, PostalAddress,
};

/// The vCard version a card is written in. 2.1 is read like 3.0; changes to a 2.1 card are
/// written in 3.0 syntax for the changed lines only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    V3,
    V4,
}

impl Version {
    pub fn as_str(self) -> &'static str {
        match self {
            Version::V3 => "3.0",
            Version::V4 => "4.0",
        }
    }
}

/// Why a text is no vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VCardError {
    /// No `BEGIN:VCARD` … `END:VCARD`.
    NoCard,
}

impl fmt::Display for VCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VCardError::NoCard => f.write_str("not a vCard"),
        }
    }
}

impl std::error::Error for VCardError {}

/// A picture inside a vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    pub data: Vec<u8>,
    /// `image/jpeg`, `image/png`, … as the card names it; empty when it does not.
    pub mime: String,
}

/// One logical line: the exact text with folds and line ending, and what it says.
#[derive(Debug, Clone)]
struct Line {
    raw: String,
    prop: Option<Prop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Prop {
    group: Option<String>,
    /// Upper case.
    name: String,
    /// Names upper case, values as written without quotes.
    params: Vec<(String, Vec<String>)>,
    /// Unfolded, still escaped.
    value: String,
}

impl Prop {
    fn param(&self, name: &str) -> impl Iterator<Item = &String> {
        self.params.iter().filter(move |(n, _)| n == name).flat_map(|(_, values)| values.iter())
    }

    fn has_param_value(&self, name: &str, value: &str) -> bool {
        self.param(name).any(|v| v.eq_ignore_ascii_case(value))
    }

    /// The value with quoted-printable and a foreign `CHARSET` undone; escapes remain.
    fn decoded(&self) -> String {
        if !self.has_param_value("ENCODING", "QUOTED-PRINTABLE") {
            return self.value.clone();
        }
        let bytes = quoted_printable(&self.value);
        let latin = self.param("CHARSET").any(|c| {
            let c = c.to_ascii_lowercase();
            c == "iso-8859-1" || c == "windows-1252" || c == "latin1"
        });
        if latin {
            bytes.iter().map(|b| char::from(*b)).collect()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }

    fn text(&self) -> String {
        unescape(&self.decoded())
    }

    fn components(&self) -> Vec<String> {
        split_unescaped(&self.decoded(), ';').iter().map(|c| unescape(c)).collect()
    }

    fn list(&self) -> Vec<String> {
        split_unescaped(&self.decoded(), ',').iter().map(|c| unescape(c).trim().to_string()).collect()
    }
}

fn quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Undoes vCard escapes: `\n`, `\,`, `\;`, `\\`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Escapes text for a vCard value.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Splits at `sep` where it is not escaped.
fn split_unescaped(text: &str, sep: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            if let Some(part) = parts.last_mut() {
                part.push('\\');
                part.push(c);
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == sep {
            parts.push(String::new());
        } else if let Some(part) = parts.last_mut() {
            part.push(c);
        }
    }
    if escaped && let Some(part) = parts.last_mut() {
        part.push('\\');
    }
    parts
}

/// Splits at `sep` outside double quotes.
fn split_outside_quotes(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

fn parse_prop(text: &str) -> Option<Prop> {
    let mut quoted = false;
    let colon = text.char_indices().find_map(|(i, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        (c == ':' && !quoted).then_some(i)
    })?;
    let (head, value) = (&text[..colon], &text[colon + 1..]);
    let mut parts = split_outside_quotes(head, ';').into_iter();
    let full = parts.next()?.trim();
    let (group, name) = match full.rfind('.') {
        Some(dot) => (Some(full[..dot].to_string()), &full[dot + 1..]),
        None => (None, full),
    };
    if name.is_empty() {
        return None;
    }
    let params = parts
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((n, v)) => (
                n.trim().to_ascii_uppercase(),
                split_outside_quotes(v, ',').into_iter().map(|v| v.trim_matches('"').to_string()).collect(),
            ),
            // vCard 2.1 writes types bare: `TEL;HOME;VOICE:`.
            None => ("TYPE".to_string(), vec![p.trim().to_string()]),
        })
        .collect();
    Some(Prop { group, name: name.to_ascii_uppercase(), params, value: value.to_string() })
}

fn without_eol(line: &str) -> &str {
    line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(line)
}

fn lines(raw: &str) -> Vec<Line> {
    let physical: Vec<&str> = raw.split_inclusive('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < physical.len() {
        let mut text_raw = physical[i].to_string();
        let mut text = without_eol(physical[i]).to_string();
        i += 1;
        while let Some(next) = physical.get(i) {
            if next.starts_with(' ') || next.starts_with('\t') {
                text_raw.push_str(next);
                text.push_str(&without_eol(next)[1..]);
                i += 1;
            } else if text.ends_with('=') && text.to_ascii_uppercase().contains("QUOTED-PRINTABLE") {
                // A soft line break of quoted-printable (vCard 2.1): the next line goes on.
                text_raw.push_str(next);
                text.pop();
                text.push_str(without_eol(next));
                i += 1;
            } else {
                break;
            }
        }
        let prop = parse_prop(&text);
        out.push(Line { raw: text_raw, prop });
    }
    out
}

/// A parsed card with the lines of every field we know.
struct Doc {
    lines: Vec<Line>,
    begin: usize,
    end: usize,
    version: Version,
    eol: &'static str,
}

impl Doc {
    fn parse(raw: &str) -> Result<Doc, VCardError> {
        let lines = lines(raw);
        let is = |line: &Line, name: &str| {
            line.prop.as_ref().is_some_and(|p| p.name == name && p.value.trim().eq_ignore_ascii_case("VCARD"))
        };
        let begin = lines.iter().position(|l| is(l, "BEGIN")).ok_or(VCardError::NoCard)?;
        let end = lines.iter().skip(begin).position(|l| is(l, "END")).map(|e| e + begin).ok_or(VCardError::NoCard)?;
        let version = lines[begin..end]
            .iter()
            .filter_map(|l| l.prop.as_ref())
            .find(|p| p.name == "VERSION")
            .map(|p| if p.value.trim().starts_with('4') { Version::V4 } else { Version::V3 })
            .unwrap_or(Version::V3);
        let eol = if lines[end].raw.ends_with("\r\n") || lines[begin].raw.ends_with("\r\n") { "\r\n" } else { "\n" };
        Ok(Doc { lines, begin, end, version, eol })
    }

    fn props(&self) -> impl Iterator<Item = (usize, &Prop)> {
        (self.begin + 1..self.end).filter_map(|i| self.lines[i].prop.as_ref().map(|p| (i, p)))
    }

    fn first(&self, name: &str) -> Option<(usize, &Prop)> {
        self.props().find(|(_, p)| p.name == name)
    }

    fn label_of(&self, group: &Option<String>) -> Option<String> {
        let group = group.as_ref()?;
        self.props().find(|(_, p)| p.name == "X-ABLABEL" && p.group.as_ref() == Some(group)).map(|(_, p)| p.text())
    }
}

/// The kind a property names with its `TYPE`s, `PREF` and Apple label.
fn kind_of(prop: &Prop, label: Option<String>) -> (FieldKind, bool) {
    // `TYPE="voice,cell"` (vCard 4) names two types in one quoted value.
    let types: Vec<String> =
        prop.param("TYPE").flat_map(|t| t.split(',')).map(|t| t.trim().to_ascii_lowercase()).collect();
    let pref = types.iter().any(|t| t == "pref") || prop.param("PREF").next().is_some();
    if let Some(label) = label {
        let apple = label.strip_prefix("_$!<").and_then(|l| l.strip_suffix(">!$_"));
        let kind = match apple.map(|a| a.to_ascii_lowercase()) {
            Some(a) if a == "home" => FieldKind::Home,
            Some(a) if a == "work" => FieldKind::Work,
            Some(a) if a == "mobile" || a == "iphone" => FieldKind::Mobile,
            Some(a) if a.ends_with("fax") => FieldKind::Fax,
            Some(a) if a == "other" => FieldKind::Other,
            Some(_) => FieldKind::Custom(apple.unwrap_or_default().to_string()),
            None => FieldKind::from_name(&label),
        };
        return (kind, pref);
    }
    let has = |t: &str| types.iter().any(|x| x == t);
    let kind = if has("cell") {
        FieldKind::Mobile
    } else if has("fax") {
        FieldKind::Fax
    } else if has("work") {
        FieldKind::Work
    } else if has("home") {
        FieldKind::Home
    } else {
        FieldKind::Other
    };
    (kind, pref)
}

fn strip_scheme<'a>(value: &'a str, scheme: &str) -> &'a str {
    match value.get(..scheme.len()) {
        Some(head) if head.eq_ignore_ascii_case(scheme) => &value[scheme.len()..],
        _ => value,
    }
}

fn address_of(prop: &Prop) -> PostalAddress {
    let mut parts = prop.components().into_iter();
    let mut next = || parts.next().unwrap_or_default().trim().to_string();
    PostalAddress {
        po_box: next(),
        extended: next(),
        street: next(),
        locality: next(),
        region: next(),
        postal_code: next(),
        country: next(),
        label: prop.param("LABEL").next().map(|l| unescape(l)),
    }
}

fn geo_of(prop: &Prop) -> Option<Geo> {
    let value = strip_scheme(prop.value.trim(), "geo:");
    let (lat, lon) = value.split_once([';', ',']).map(|(a, b)| (a, b.split([';', ',']).next().unwrap_or(b)))?;
    Geo::new(lat.trim().parse().ok()?, lon.trim().parse().ok()?)
}

/// FNV-1a over the bytes, as hex: a short version that changes with the text.
fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn labeled<T>(value: T, (kind, pref): (FieldKind, bool)) -> Labeled<T> {
    Labeled { value, kind, pref }
}

/// Whether `prop` (a `PHOTO`) holds the picture itself: a `data:` address or Base64.
fn inline_photo(prop: &Prop) -> bool {
    !prop.value.trim().is_empty()
        && (prop.value.trim_start().starts_with("data:")
            || prop.has_param_value("ENCODING", "b")
            || prop.has_param_value("ENCODING", "BASE64"))
}

/// The fields a contact holds once; [`patch`] writes them with `set_single` into their first line.
const SINGLE: [&str; 8] = ["UID", "FN", "N", "NICKNAME", "ORG", "TITLE", "NOTE", "BDAY"];

/// Where each value of a field we know sits, for [`patch`].
#[derive(Default)]
struct Index {
    emails: Vec<usize>,
    phones: Vec<usize>,
    addresses: Vec<usize>,
    urls: Vec<usize>,
    impps: Vec<usize>,
}

fn read_doc(doc: &Doc) -> (Contact, Index) {
    let mut contact = Contact { untrusted: true, ..Contact::default() };
    let mut index = Index::default();
    let mut seen: Vec<&str> = Vec::new();
    for (i, prop) in doc.props() {
        // A field that stands once counts by its first line, the one `patch` changes: vCard 4.0
        // allows a second NOTE or TITLE, and reading the last would hide every change.
        if SINGLE.contains(&prop.name.as_str()) {
            if seen.contains(&prop.name.as_str()) {
                continue;
            }
            seen.push(&prop.name);
        }
        let kind = || kind_of(prop, doc.label_of(&prop.group));
        match prop.name.as_str() {
            "UID" => contact.uid = prop.text().trim().to_string(),
            "FN" => contact.display_name = prop.text().trim().to_string(),
            "N" => {
                let mut parts = prop.components().into_iter();
                let mut next = || parts.next().unwrap_or_default().trim().to_string();
                contact.name =
                    Name { family: next(), given: next(), additional: next(), prefix: next(), suffix: next() };
            }
            "NICKNAME" => contact.nickname = Some(prop.text().trim().to_string()).filter(|n| !n.is_empty()),
            "ORG" => {
                contact.org =
                    prop.components().into_iter().next().map(|o| o.trim().to_string()).filter(|o| !o.is_empty())
            }
            "TITLE" => contact.title = Some(prop.text().trim().to_string()).filter(|t| !t.is_empty()),
            "NOTE" => contact.note = Some(prop.text()).filter(|n| !n.trim().is_empty()),
            "BDAY" => contact.birthday = PartialDate::parse(&prop.text()),
            // The standard line wins over Apple's wherever it stands; `patch` changes that one.
            "ANNIVERSARY" => contact.anniversary = PartialDate::parse(&prop.text()).or(contact.anniversary),
            "X-ANNIVERSARY" if contact.anniversary.is_none() || doc.first("ANNIVERSARY").is_none() => {
                contact.anniversary = PartialDate::parse(&prop.text())
            }
            "CATEGORIES" => {
                for group in prop.list().into_iter().filter(|g| !g.is_empty()) {
                    if !contact.groups.contains(&group) {
                        contact.groups.push(group);
                    }
                }
            }
            "GEO" if contact.geo.is_none() => contact.geo = geo_of(prop),
            // Only a picture inside the card: a link to elsewhere is never fetched, so it has none to show.
            "PHOTO" if contact.photo.is_none() && inline_photo(prop) => {
                contact.photo = Some(PhotoRef { version: fingerprint(&prop.value) })
            }
            "EMAIL" => {
                contact.emails.push(labeled(strip_scheme(prop.text().trim(), "mailto:").to_string(), kind()));
                index.emails.push(i);
            }
            "TEL" => {
                contact.phones.push(labeled(strip_scheme(prop.text().trim(), "tel:").to_string(), kind()));
                index.phones.push(i);
            }
            "ADR" => {
                contact.addresses.push(labeled(address_of(prop), kind()));
                index.addresses.push(i);
            }
            "URL" => {
                contact.urls.push(labeled(prop.text().trim().to_string(), kind()));
                index.urls.push(i);
            }
            "IMPP" => {
                contact.impps.push(labeled(prop.text().trim().to_string(), kind()));
                index.impps.push(i);
            }
            _ => {}
        }
    }
    (contact, index)
}

/// Reads the first card in `raw`. The contact has its fields and `uid`; `id`, `book`, `etag` and
/// the rest of the local state are the caller's.
pub fn read(raw: &str) -> Result<Contact, VCardError> {
    Ok(read_doc(&Doc::parse(raw)?).0)
}

/// The version `raw` is written in; 3.0 when it is no card.
pub fn version(raw: &str) -> Version {
    Doc::parse(raw).map(|d| d.version).unwrap_or(Version::V3)
}

/// An iCloud group card (`X-ADDRESSBOOKSERVER-KIND:group`): its name and the `UID`s of its
/// members. `None` for every other card.
pub fn group_card(raw: &str) -> Option<(String, Vec<String>)> {
    let doc = Doc::parse(raw).ok()?;
    let (_, kind) = doc.first("X-ADDRESSBOOKSERVER-KIND")?;
    if !kind.text().trim().eq_ignore_ascii_case("group") {
        return None;
    }
    let name = doc.first("FN").map(|(_, p)| p.text().trim().to_string()).unwrap_or_default();
    let members = doc
        .props()
        .filter(|(_, p)| p.name == "X-ADDRESSBOOKSERVER-MEMBER")
        .map(|(_, p)| strip_scheme(p.text().trim(), "urn:uuid:").to_string())
        .collect();
    Some((name, members))
}

/// The picture inside `raw`, when there is one inline (a link to elsewhere is not fetched).
pub fn photo(raw: &str) -> Option<Photo> {
    let doc = Doc::parse(raw).ok()?;
    let (_, prop) = doc.first("PHOTO")?;
    let value: String = prop.value.chars().filter(|c| !c.is_whitespace()).collect();
    let engine = base64::engine::general_purpose::STANDARD;
    if let Some(rest) = value.strip_prefix("data:") {
        let (head, data) = rest.split_once(',')?;
        let mime = head.split(';').next().unwrap_or_default().to_ascii_lowercase();
        return engine.decode(data).ok().map(|data| Photo { data, mime });
    }
    let base64 = prop.has_param_value("ENCODING", "b") || prop.has_param_value("ENCODING", "BASE64");
    if !base64 {
        return None;
    }
    let mime = prop
        .param("TYPE")
        .next()
        .map(|t| {
            let t = t.to_ascii_lowercase();
            if t.contains('/') { t } else { format!("image/{t}") }
        })
        .unwrap_or_default();
    engine.decode(value).ok().map(|data| Photo { data, mime })
}

/// Folds a line at 75 octets, never inside a character.
fn fold(line: &str, eol: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str(eol);
            out.push(' ');
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out.push_str(eol);
    out
}

fn render_param_value(value: &str) -> String {
    let value = value.replace('\r', "").replace('\n', "\\n");
    if value.contains([':', ';', ',', ' ', '"']) {
        format!("\"{}\"", value.replace('"', "'"))
    } else {
        value.to_string()
    }
}

fn render(group: Option<&str>, name: &str, params: &[(String, Vec<String>)], value: &str, eol: &str) -> String {
    let mut line = String::new();
    if let Some(group) = group {
        line.push_str(group);
        line.push('.');
    }
    line.push_str(name);
    for (param, values) in params {
        line.push(';');
        line.push_str(param);
        line.push('=');
        line.push_str(&values.iter().map(|v| render_param_value(v)).collect::<Vec<_>>().join(","));
    }
    line.push(':');
    line.push_str(value);
    fold(&line, eol)
}

/// The changes to one card: lines kept, replaced or removed, and new lines after a line.
struct Edit {
    doc: Doc,
    body: Vec<Option<String>>,
    after: Vec<Vec<String>>,
    next_item: usize,
}

const KIND_TOKENS: [&str; 6] = ["home", "work", "cell", "fax", "other", "pref"];

impl Edit {
    fn new(doc: Doc) -> Edit {
        let body = doc.lines.iter().map(|l| Some(l.raw.clone())).collect();
        let after = vec![Vec::new(); doc.lines.len()];
        let next_item = doc
            .props()
            .filter_map(|(_, p)| p.group.as_deref()?.strip_prefix("item")?.parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        Edit { doc, body, after, next_item }
    }

    fn finish(self) -> String {
        let mut out = String::new();
        for (body, after) in self.body.into_iter().zip(self.after) {
            if let Some(body) = body {
                out.push_str(&body);
            }
            for line in after {
                out.push_str(&line);
            }
        }
        out
    }

    fn prop(&self, i: usize) -> Option<&Prop> {
        self.doc.lines[i].prop.as_ref()
    }

    fn render(&self, group: Option<&str>, name: &str, params: &[(String, Vec<String>)], value: &str) -> String {
        render(group, name, params, value, self.doc.eol)
    }

    /// Inserts `line` after the last line of `name`, else before `END:VCARD`.
    fn insert(&mut self, name: &str, line: String) {
        let at = self.doc.props().filter(|(_, p)| p.name == name).map(|(i, _)| i).last().unwrap_or(self.doc.end - 1);
        self.after[at].push(line);
    }

    fn remove(&mut self, i: usize) {
        self.body[i] = None;
        // Apple's label and other lines of the same group belong to the removed value.
        if let Some(group) = self.prop(i).and_then(|p| p.group.clone()) {
            let others: Vec<usize> = self
                .doc
                .props()
                .filter(|(j, p)| *j != i && p.group.as_ref() == Some(&group) && p.name.starts_with("X-AB"))
                .map(|(j, _)| j)
                .collect();
            for j in others {
                self.body[j] = None;
            }
        }
    }

    /// Params of the line `i` without encoding, so the plain UTF-8 value we write reads right.
    fn kept_params(&self, i: usize, also_drop: &[&str]) -> Vec<(String, Vec<String>)> {
        self.prop(i)
            .map(|p| {
                p.params
                    .iter()
                    .filter(|(n, _)| n != "ENCODING" && n != "CHARSET" && !also_drop.contains(&n.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sets the single property `name` to `value` (already escaped), removes it for `None`.
    fn set_single(&mut self, name: &str, value: Option<String>, drop_params: &[&str]) {
        let lines: Vec<usize> = self.doc.props().filter(|(_, p)| p.name == name).map(|(i, _)| i).collect();
        match (value, lines.first()) {
            (None, _) => {
                for i in lines {
                    self.remove(i);
                }
            }
            (Some(value), Some(&i)) => {
                let group = self.prop(i).and_then(|p| p.group.clone());
                let params = self.kept_params(i, drop_params);
                self.body[i] = Some(self.render(group.as_deref(), name, &params, &value));
            }
            (Some(value), None) => {
                let line = self.render(None, name, &[], &value);
                self.insert(name, line);
            }
        }
    }

    fn type_params(
        &self,
        kind: &FieldKind,
        pref: bool,
        mut params: Vec<(String, Vec<String>)>,
    ) -> Vec<(String, Vec<String>)> {
        for (name, values) in params.iter_mut() {
            if name == "TYPE" {
                // `TYPE="voice,cell"` (vCard 4) names two types in one quoted value.
                *values = values.iter().flat_map(|v| v.split(',')).map(|v| v.trim().to_string()).collect();
                values.retain(|v| !v.is_empty() && !KIND_TOKENS.contains(&v.to_ascii_lowercase().as_str()));
            }
        }
        params.retain(|(name, values)| !(name == "TYPE" && values.is_empty()) && name != "PREF");
        let v3 = self.doc.version == Version::V3;
        let token = match kind {
            FieldKind::Home => Some("home"),
            FieldKind::Work => Some("work"),
            FieldKind::Mobile => Some("cell"),
            FieldKind::Fax => Some("fax"),
            FieldKind::Other | FieldKind::Custom(_) => None,
        };
        let mut types: Vec<String> =
            token.map(|t| if v3 { t.to_ascii_uppercase() } else { t.to_string() }).into_iter().collect();
        if pref && v3 {
            types.push("PREF".into());
        }
        if !types.is_empty() {
            match params.iter_mut().find(|(n, _)| n == "TYPE") {
                Some((_, values)) => values.extend(types),
                None => params.push(("TYPE".into(), types)),
            }
        }
        if pref && !v3 {
            params.push(("PREF".into(), vec!["1".into()]));
        }
        params
    }

    fn apple_label(kind: &FieldKind) -> String {
        match kind {
            FieldKind::Home => "_$!<Home>!$_".into(),
            FieldKind::Work => "_$!<Work>!$_".into(),
            FieldKind::Mobile => "_$!<Mobile>!$_".into(),
            FieldKind::Fax => "_$!<HomeFAX>!$_".into(),
            FieldKind::Other => "_$!<Other>!$_".into(),
            FieldKind::Custom(text) => escape(text),
        }
    }

    /// Writes a value of a field with several: replaces line `at`, or adds a new line.
    /// The value is written as text (a `VALUE=uri` of the old line goes, so `tel:` needs no prefix),
    /// and an address's `LABEL` is the one of the new value, never the old line's.
    fn write_labeled(
        &mut self,
        name: &str,
        at: Option<usize>,
        kind: &FieldKind,
        pref: bool,
        value: (String, Option<String>),
    ) {
        let (value, label) = value;
        let old_group = at.and_then(|i| self.prop(i).and_then(|p| p.group.clone()));
        let label_line = old_group.as_ref().and_then(|g| {
            self.doc.props().find(|(_, p)| p.name == "X-ABLABEL" && p.group.as_ref() == Some(g)).map(|(i, _)| i)
        });
        let base = at.map(|i| self.kept_params(i, &["VALUE", "LABEL"])).unwrap_or_default();
        let mut params = self.type_params(kind, pref, base);
        if let Some(label) = label {
            params.push(("LABEL".into(), vec![label]));
        }
        let mut group = old_group.clone();
        let mut new_label = None;
        if let Some(label) = label_line {
            // The value has Apple's label: the label says the kind.
            let group = old_group.clone().unwrap_or_default();
            let line = self.render(Some(&group), "X-ABLabel", &[], &Edit::apple_label(kind));
            self.body[label] = Some(line);
        } else if let FieldKind::Custom(_) = kind {
            let item = format!("item{}", self.next_item);
            self.next_item += 1;
            new_label = Some(self.render(Some(&item), "X-ABLabel", &[], &Edit::apple_label(kind)));
            group = Some(item);
        }
        let line = self.render(group.as_deref(), name, &params, &value);
        match at {
            Some(i) => {
                self.body[i] = Some(line);
                if let Some(label) = new_label {
                    self.after[i].push(label);
                }
            }
            None => {
                let line = line + new_label.as_deref().unwrap_or_default();
                self.insert(name, line);
            }
        }
    }

    /// Patches the values of one field with several: removed values go, new ones come after the
    /// last of their kind, values whose kind or preference changed are rewritten. Values stay
    /// matched by what they say, so the order of the others never changes.
    fn patch_labeled<T: PartialEq + Clone>(
        &mut self,
        name: &str,
        lines: &[usize],
        current: &[Labeled<T>],
        before: &[Labeled<T>],
        after: &[Labeled<T>],
        encode: impl Fn(&T, Version) -> (String, Option<String>),
    ) {
        if before == after {
            return;
        }
        let mut used = vec![false; current.len()];
        let find = |value: &T, used: &mut Vec<bool>| {
            let found = current.iter().enumerate().position(|(k, c)| !used[k] && c.value == *value)?;
            used[found] = true;
            Some(found)
        };
        let mut before_used = vec![false; before.len()];
        let mut added = Vec::new();
        for new in after {
            let was = before.iter().enumerate().position(|(k, b)| !before_used[k] && b.value == new.value);
            match was {
                Some(k) => {
                    before_used[k] = true;
                    let old = &before[k];
                    if let Some(c) = find(&new.value, &mut used)
                        && (old.kind != new.kind || old.pref != new.pref)
                    {
                        let value = encode(&new.value, self.doc.version);
                        self.write_labeled(name, Some(lines[c]), &new.kind, new.pref, value);
                    }
                }
                None => added.push(new),
            }
        }
        let mut removed = Vec::new();
        for (k, old) in before.iter().enumerate() {
            if !before_used[k]
                && let Some(c) = find(&old.value, &mut used)
            {
                removed.push(lines[c]);
            }
        }
        // A value that took the place of a removed one is an edit: it stays where the old one was.
        let mut removed = removed.into_iter();
        for new in added {
            let value = encode(&new.value, self.doc.version);
            let at = removed.next();
            self.write_labeled(name, at, &new.kind, new.pref, value);
        }
        for line in removed {
            self.remove(line);
        }
    }
}

#[allow(clippy::ptr_arg)] // `patch_labeled` hands every field's value by reference.
fn encode_text(value: &String, _: Version) -> (String, Option<String>) {
    (escape(value), None)
}

/// The address's parts, and its `LABEL` parameter.
fn encode_address(address: &PostalAddress, _: Version) -> (String, Option<String>) {
    let parts = [
        &address.po_box,
        &address.extended,
        &address.street,
        &address.locality,
        &address.region,
        &address.postal_code,
        &address.country,
    ]
    .iter()
    .map(|p| escape(p))
    .collect::<Vec<_>>()
    .join(";");
    // Read from the `LABEL` parameter in every version, so written back in every version too.
    let label = address.label.clone().filter(|l| !l.trim().is_empty());
    (parts, label)
}

fn date_value(date: &PartialDate, version: Version) -> (String, Vec<(String, Vec<String>)>) {
    match (date.year, version) {
        (Some(year), Version::V4) => (format!("{year:04}{:02}{:02}", date.month, date.day), vec![]),
        (Some(_), Version::V3) => (date.to_string(), vec![]),
        (None, Version::V4) => (format!("--{:02}{:02}", date.month, date.day), vec![]),
        (None, Version::V3) => (
            format!("1604-{:02}-{:02}", date.month, date.day),
            vec![("X-APPLE-OMIT-YEAR".to_string(), vec!["1604".to_string()])],
        ),
    }
}

/// Writes the fields that differ between `before` and `after` into `raw`, and nothing else.
///
/// `before` is the contact as [`read`] gave it for `raw` (or for an older text of the same card);
/// values are found by what they say, so a `before` read from the version before a merge works
/// too. Every line of a field that did not change stays as it was, byte for byte. Local state
/// (`favorite`, `state`, `etag`, the photo) is never written. Not a card: `raw` unchanged.
pub fn patch(raw: &str, before: &Contact, after: &Contact) -> String {
    let Ok(doc) = Doc::parse(raw) else { return raw.to_string() };
    let (current, index) = read_doc(&doc);
    let version = doc.version;
    let mut edit = Edit::new(doc);
    let text = |t: &Option<String>| t.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(escape);

    if before.display_name != after.display_name || (after.display_name.is_empty() && before.name != after.name) {
        // `FN` is required: an empty name shows as what the contact has.
        edit.set_single("FN", Some(escape(&after.shown_name())), &[]);
    }
    if before.name != after.name {
        let n = &after.name;
        let value = [&n.family, &n.given, &n.additional, &n.prefix, &n.suffix]
            .iter()
            .map(|p| escape(p))
            .collect::<Vec<_>>()
            .join(";");
        edit.set_single("N", Some(value), &[]);
    }
    if before.nickname != after.nickname {
        edit.set_single("NICKNAME", text(&after.nickname), &[]);
    }
    if before.org != after.org {
        // Only the company is ours; departments after it stay.
        let rest: Vec<String> = edit
            .doc
            .first("ORG")
            .map(|(_, p)| split_unescaped(&p.decoded(), ';').into_iter().skip(1).collect())
            .unwrap_or_default();
        let value = text(&after.org).map(|org| std::iter::once(org).chain(rest).collect::<Vec<_>>().join(";"));
        edit.set_single("ORG", value, &[]);
    }
    if before.title != after.title {
        edit.set_single("TITLE", text(&after.title), &[]);
    }
    if before.note != after.note {
        let note = after.note.as_deref().filter(|n| !n.trim().is_empty()).map(escape);
        edit.set_single("NOTE", note, &[]);
    }
    // vCard 3.0 has no `ANNIVERSARY`; Apple and others write `X-ANNIVERSARY`. The line there is the
    // one changed; a new one takes the name of the version.
    let anniversary = match (edit.doc.first("ANNIVERSARY"), edit.doc.first("X-ANNIVERSARY"), version) {
        (Some(_), _, _) | (None, None, Version::V4) => "ANNIVERSARY",
        _ => "X-ANNIVERSARY",
    };
    for (name, old, new) in
        [("BDAY", before.birthday, after.birthday), (anniversary, before.anniversary, after.anniversary)]
    {
        if old == new {
            continue;
        }
        match new {
            None if name == "BDAY" => edit.set_single(name, None, &[]),
            // Gone means gone in both lines a card may have.
            None => {
                edit.set_single("ANNIVERSARY", None, &[]);
                edit.set_single("X-ANNIVERSARY", None, &[]);
            }
            Some(date) => {
                let (value, params) = date_value(&date, version);
                match edit.doc.first(name).map(|(i, _)| i) {
                    Some(i) => {
                        let group = edit.prop(i).and_then(|p| p.group.clone());
                        edit.body[i] = Some(edit.render(group.as_deref(), name, &params, &value));
                    }
                    None => {
                        let line = edit.render(None, name, &params, &value);
                        edit.insert(name, line);
                    }
                }
            }
        }
    }
    if before.groups != after.groups {
        let lines: Vec<usize> = edit.doc.props().filter(|(_, p)| p.name == "CATEGORIES").map(|(i, _)| i).collect();
        for &i in lines.iter().skip(1) {
            edit.remove(i);
        }
        let value =
            (!after.groups.is_empty()).then(|| after.groups.iter().map(|g| escape(g)).collect::<Vec<_>>().join(","));
        edit.set_single("CATEGORIES", value, &[]);
    }
    edit.patch_labeled("EMAIL", &index.emails, &current.emails, &before.emails, &after.emails, encode_text);
    edit.patch_labeled("TEL", &index.phones, &current.phones, &before.phones, &after.phones, encode_text);
    edit.patch_labeled(
        "ADR",
        &index.addresses,
        &current.addresses,
        &before.addresses,
        &after.addresses,
        encode_address,
    );
    edit.patch_labeled("URL", &index.urls, &current.urls, &before.urls, &after.urls, encode_text);
    edit.patch_labeled("IMPP", &index.impps, &current.impps, &before.impps, &after.impps, encode_text);
    edit.finish()
}

/// Puts `photo` (`None`: removes the picture) into `raw`; every other line stays.
pub fn set_photo(raw: &str, photo: Option<&Photo>) -> String {
    let Ok(doc) = Doc::parse(raw) else { return raw.to_string() };
    let version = doc.version;
    let mut edit = Edit::new(doc);
    let Some(photo) = photo else {
        edit.set_single("PHOTO", None, &[]);
        return edit.finish();
    };
    let data = base64::engine::general_purpose::STANDARD.encode(&photo.data);
    let (params, value) = match version {
        Version::V4 => (vec![], format!("data:{};base64,{data}", photo.mime)),
        Version::V3 => {
            let kind = photo.mime.strip_prefix("image/").unwrap_or("jpeg").to_ascii_uppercase();
            (vec![("ENCODING".to_string(), vec!["b".to_string()]), ("TYPE".to_string(), vec![kind])], data)
        }
    };
    let lines: Vec<usize> = edit.doc.props().filter(|(_, p)| p.name == "PHOTO").map(|(i, _)| i).collect();
    for &i in lines.iter().skip(1) {
        edit.remove(i);
    }
    match lines.first() {
        Some(&i) => {
            let group = edit.prop(i).and_then(|p| p.group.clone());
            edit.body[i] = Some(edit.render(group.as_deref(), "PHOTO", &params, &value));
        }
        None => {
            let line = edit.render(None, "PHOTO", &params, &value);
            edit.insert("PHOTO", line);
        }
    }
    edit.finish()
}

/// `raw` with the `UID` `uid`, every other line as it was: a copy of a card that must not take the
/// place of the original (`contacts.resolve_conflict` keeping both).
pub fn set_uid(raw: &str, uid: &str) -> String {
    let Ok(doc) = Doc::parse(raw) else { return raw.to_string() };
    let mut edit = Edit::new(doc);
    edit.set_single("UID", Some(escape(uid)), &[]);
    edit.finish()
}

/// A fresh `UID`: a random UUID (version 4), `8-4-4-4-12` hex digits.
pub fn new_uid() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|err| format!("no random numbers for an id: {err}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

/// A new card in `version` with `uid` and the fields sent. Returns the card and the contact as
/// [`read`] gives it back.
pub fn new_vcard(uid: &str, fields: &ContactFields, version: Version) -> Result<(String, Contact), FieldsError> {
    let eol = "\r\n";
    let skeleton = format!(
        "BEGIN:VCARD{eol}VERSION:{}{eol}PRODID:-//TorroWorkplace//Contacts//EN{eol}UID:{}{eol}FN:{eol}N:;;;;{eol}END:VCARD{eol}",
        version.as_str(),
        escape(uid)
    );
    let before = read(&skeleton).map_err(|_| FieldsError::Empty)?;
    let mut after = before.clone();
    fields.apply(&mut after)?;
    // `FN` is never empty: what the contact has, as for a change.
    let mut before_fn = before.clone();
    before_fn.display_name = "\u{0}".into();
    let raw = patch(&skeleton, &before_fn, &after);
    let contact = read(&raw).map_err(|_| FieldsError::Empty)?;
    Ok((raw, contact))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/vcard/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).expect("fixture")
    }

    /// The logical lines of `a` that are not in `b`, by exact text.
    fn only_in(a: &str, b: &str) -> Vec<String> {
        let b: Vec<String> = lines(b).into_iter().map(|l| l.raw).collect();
        lines(a).into_iter().map(|l| l.raw).filter(|l| !b.contains(l)).collect()
    }

    /// Changes one field with `change`, patches, and checks that the lines before and after
    /// differ only where `changed` says (by property name), and that the change reads back.
    fn round_trip(name: &str, changed: &str, change: impl Fn(&mut Contact)) {
        let raw = fixture(name);
        let before = read(&raw).expect("card");
        let mut after = before.clone();
        change(&mut after);
        let patched = patch(&raw, &before, &after);
        for line in only_in(&raw, &patched).iter().chain(only_in(&patched, &raw).iter()) {
            let prop = parse_prop(without_eol(line)).expect("prop");
            assert!(
                prop.name == changed || prop.name == "X-ABLABEL",
                "{name}: line of {} changed although only {changed} should: {line:?}",
                prop.name
            );
        }
        let back = read(&patched).expect("still a card");
        assert_eq!(back, after, "{name}: the change reads back as made");
        let untouched = patch(&raw, &before, &before);
        assert_eq!(untouched, raw, "{name}: no change, no byte changed");
    }

    #[test]
    fn nextcloud_card_reads_and_keeps_unknown_fields() {
        let raw = fixture("nextcloud.vcf");
        let contact = read(&raw).expect("card");
        assert_eq!(contact.uid, "7f3c1a52-9b0e-4c1d-8e2a-5d6f7a8b9c0d");
        assert_eq!(contact.name.family, "Berg");
        assert_eq!(contact.org.as_deref(), Some("Example GmbH"));
        assert_eq!(contact.emails[0], Labeled::new("anna.berg@example.org".to_string(), FieldKind::Work));
        assert_eq!(contact.phones[0].kind, FieldKind::Mobile);
        assert_eq!(contact.addresses[0].value.one_line(), "Hauptstr. 1, 10115 Berlin, Germany");
        assert_eq!(contact.note.as_deref(), Some("Met at the conference, 2024.\nLikes tea."));
        assert_eq!(contact.groups, ["Friends", "Work"]);
        assert_eq!(contact.birthday, PartialDate::parse("1990-04-15"));
        round_trip("nextcloud.vcf", "TEL", |c| c.phones[0].value = "+49 171 9999999".into());
        round_trip("nextcloud.vcf", "ORG", |c| c.org = Some("Other AG".into()));
        round_trip("nextcloud.vcf", "CATEGORIES", |c| c.groups.push("Running".into()));
        round_trip("nextcloud.vcf", "EMAIL", |c| {
            c.emails.push(Labeled::new("third@example.org".into(), FieldKind::Other))
        });
        round_trip("nextcloud.vcf", "ADR", |c| c.addresses.clear());
        round_trip("nextcloud.vcf", "NOTE", |c| c.note = Some("New; note, with\nlines".into()));
        let patched = patch(&raw, &contact, &Contact { org: Some("Other AG".into()), ..contact.clone() });
        assert!(patched.contains("\r\nORG:Other AG;Research\r\n"), "the department stays: {patched}");
        assert!(patched.contains("X-CUSTOM-FIELD;X-PARAM=\"a,b\":keep me exactly\r\n"));
    }

    #[test]
    fn icloud_card_with_labels_photo_and_no_year() {
        let raw = fixture("icloud.vcf");
        let contact = read(&raw).expect("card");
        assert_eq!(
            contact.emails[0],
            Labeled { value: "jonas@example.com".into(), kind: FieldKind::Other, pref: true }
        );
        assert_eq!(contact.phones[1].kind, FieldKind::Custom("Boat".into()));
        assert_eq!(contact.birthday, Some(PartialDate { year: None, month: 7, day: 2 }));
        assert_eq!(contact.org.as_deref(), Some("Keller & Söhne"));
        assert!(contact.photo.is_some());
        let photo = photo(&raw).expect("photo");
        assert_eq!(photo.mime, "image/jpeg");
        assert_eq!(&photo.data[..2], &[0xff, 0xd8]);
        round_trip("icloud.vcf", "TEL", |c| c.phones.remove(1).value.clear());
        round_trip("icloud.vcf", "TEL", |c| c.phones[1].kind = FieldKind::Custom("Car".into()));
        round_trip("icloud.vcf", "BDAY", |c| c.birthday = PartialDate::parse("--12-24"));
        round_trip("icloud.vcf", "TITLE", |c| c.title = Some("Captain".into()));
        // Removing a value with Apple's label takes the label along, and nothing else.
        let mut after = contact.clone();
        after.phones.remove(1);
        let patched = patch(&raw, &contact, &after);
        assert!(!patched.contains("item2."), "{patched}");
        assert!(patched.contains("item3.X-ABADR:ch"));
    }

    #[test]
    fn google_export_with_lf_and_folds() {
        let raw = fixture("google.vcf");
        let contact = read(&raw).expect("card");
        assert_eq!(
            contact.note.as_deref(),
            Some("Long note that is folded because it is longer than seventy-five octets in a single line.")
        );
        assert_eq!(contact.nickname.as_deref(), Some("Mari"));
        round_trip("google.vcf", "URL", |c| c.urls[0].value = "https://example.com/maria".into());
        round_trip("google.vcf", "NICKNAME", |c| c.nickname = None);
        let mut after = contact.clone();
        after.title = Some("Architect".into());
        let patched = patch(&raw, &contact, &after);
        assert!(patched.contains("TITLE:Architect\nEND:VCARD\n"), "new lines take the card's line ending: {patched:?}");
    }

    #[test]
    fn thunderbird_and_version_4() {
        let raw = fixture("thunderbird.vcf");
        let contact = read(&raw).expect("card");
        assert!(contact.emails[0].pref);
        assert_eq!(contact.phones[0].kind, FieldKind::Work);
        round_trip("thunderbird.vcf", "EMAIL", |c| c.emails[0].pref = false);
        round_trip("thunderbird.vcf", "TEL", |c| {
            c.phones.push(Labeled { value: "+33 6 12 34 56 78".into(), kind: FieldKind::Mobile, pref: true })
        });

        let raw = fixture("v4.vcf");
        assert_eq!(version(&raw), Version::V4);
        let contact = read(&raw).expect("card");
        assert_eq!(contact.uid, "urn:uuid:4fbe8971-0bc3-424c-9c26-36c3e1eff6b1");
        assert_eq!(contact.phones[0], Labeled::new("+49-176-1234567".to_string(), FieldKind::Mobile));
        assert_eq!(contact.addresses[0].value.label.as_deref(), Some("Ringstr. 3\n04109 Leipzig"));
        assert_eq!(contact.geo, Some(Geo { lat: 51.3397, lon: 12.3731 }));
        assert_eq!(contact.impps[0].value, "xmpp:lena@example.org");
        assert_eq!(contact.anniversary, PartialDate::parse("2015-06-12"));
        assert_eq!(contact.birthday, PartialDate::parse("--08-02"));
        assert_eq!(photo(&raw).map(|p| p.mime), Some("image/png".into()));
        round_trip("v4.vcf", "BDAY", |c| c.birthday = PartialDate::parse("1985-08-02"));
        round_trip("v4.vcf", "N", |c| c.name.given = "Helena".into());
        round_trip("v4.vcf", "EMAIL", |c| c.emails[0].kind = FieldKind::Home);
    }

    #[test]
    fn version_2_1_with_quoted_printable() {
        let raw = fixture("old-21.vcf");
        let contact = read(&raw).expect("card");
        assert_eq!(contact.name.family, "Müller");
        assert_eq!(contact.display_name, "Jürgen Müller");
        assert_eq!(contact.phones[0].kind, FieldKind::Home);
        assert_eq!(contact.note.as_deref(), Some("First line\r\nsecond line"));
        round_trip("old-21.vcf", "TEL", |c| c.phones[0].value = "+49 89 7654321".into());
    }

    #[test]
    fn group_cards_of_icloud() {
        let (name, members) = group_card(&fixture("icloud-group.vcf")).expect("group");
        assert_eq!(name, "Running club");
        assert_eq!(members, ["8E4F2B1C-3D5A-4E6F-9A8B-7C6D5E4F3A2B", "11111111-2222-3333-4444-555555555555"]);
        assert_eq!(group_card(&fixture("icloud.vcf")), None);
    }

    #[test]
    fn new_cards_and_photos() {
        let uid = new_uid().expect("uid");
        assert_eq!(uid.len(), 36);
        assert_eq!(uid.as_bytes()[14], b'4');
        let fields = ContactFields {
            name: Some(Name { family: "Neu".into(), given: "Max".into(), ..Name::default() }),
            emails: Some(vec![Labeled::new("max@example.org".into(), FieldKind::Custom("Club".into()))]),
            birthday: Some("--03-01".into()),
            groups: Some(vec!["Friends".into()]),
            ..ContactFields::default()
        };
        for version in [Version::V3, Version::V4] {
            let (raw, contact) = new_vcard(&uid, &fields, version).expect("card");
            assert_eq!(contact.uid, uid);
            assert_eq!(contact.display_name, "Max Neu", "{raw}");
            assert_eq!(contact.emails[0].kind, FieldKind::Custom("Club".into()), "{raw}");
            assert_eq!(contact.birthday, PartialDate::parse("--03-01"), "{raw}");
            assert_eq!(contact.groups, ["Friends"]);
            let photo = Photo { data: vec![1, 2, 3], mime: "image/jpeg".into() };
            let with = set_photo(&raw, Some(&photo));
            assert_eq!(super::photo(&with), Some(photo.clone()), "{with}");
            assert!(read(&with).expect("card").photo.is_some());
            let without = set_photo(&with, None);
            assert_eq!(without, raw, "removing the photo restores the card");
        }
        assert_eq!(
            new_vcard(&uid, &ContactFields { birthday: Some("soon".into()), ..ContactFields::default() }, Version::V3),
            Err(FieldsError::BadDate("birthday"))
        );
    }

    /// Review of #351: the old line's parameters never contradict a value written anew.
    #[test]
    fn rewritten_values_take_no_stale_parameters() {
        let raw = fixture("v4.vcf");
        let before = read(&raw).expect("card");
        let mut after = before.clone();
        after.phones[0].kind = FieldKind::Work;
        let patched = patch(&raw, &before, &after);
        assert_eq!(read(&patched).expect("card").phones[0].kind, FieldKind::Work, "{patched}");
        assert!(!patched.contains("VALUE=uri"), "a text value is no uri: {patched}");
        let mut moved = before.clone();
        moved.addresses[0].value.street = "Markt 1".into();
        moved.addresses[0].value.label = Some("Markt 1\n04109 Leipzig".into());
        let patched = patch(&raw, &before, &moved);
        let back = read(&patched).expect("card");
        assert_eq!(back.addresses[0].value.label.as_deref(), Some("Markt 1\n04109 Leipzig"), "{patched}");
        assert!(!patched.contains("Ringstr"), "{patched}");
        let linked = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:x\r\nFN:X\r\nPHOTO:https://example.org/x.jpg\r\nEND:VCARD\r\n";
        assert_eq!(read(linked).expect("card").photo, None, "a picture elsewhere is no picture to show");
    }

    #[test]
    fn an_apple_anniversary_is_changed_and_removed_where_it_stands() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:x\r\nFN:X\r\nX-ANNIVERSARY:2010-01-01\r\nEND:VCARD\r\n";
        let before = read(raw).expect("card");
        let mut after = before.clone();
        after.anniversary = PartialDate::parse("2011-02-02");
        let changed = patch(raw, &before, &after);
        assert_eq!(read(&changed).expect("card").anniversary, PartialDate::parse("2011-02-02"), "{changed}");
        assert_eq!(changed.matches("ANNIVERSARY").count(), 1, "{changed}");
        let both = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:x\r\nFN:X\r\nX-ANNIVERSARY:2010-01-01\r\nANNIVERSARY:2012-03-03\r\nEND:VCARD\r\n";
        let before_both = read(both).expect("card");
        assert_eq!(before_both.anniversary, PartialDate::parse("2012-03-03"), "the standard line wins");
        let mut gone = before_both.clone();
        gone.anniversary = None;
        assert!(!patch(both, &before_both, &gone).contains("ANNIVERSARY"), "both lines go");
        after.anniversary = None;
        let removed = patch(raw, &before, &after);
        assert!(!removed.contains("ANNIVERSARY"), "{removed}");
    }

    #[test]
    fn a_field_written_twice_reads_and_changes_its_first_line() {
        let raw = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:x\r\nFN:X\r\nNOTE:first\r\nTITLE:Boss\r\nNOTE:second\r\nTITLE:Chief\r\nEND:VCARD\r\n";
        let before = read(raw).expect("card");
        assert_eq!((before.note.as_deref(), before.title.as_deref()), (Some("first"), Some("Boss")));
        let mut after = before.clone();
        after.note = Some("changed".into());
        let changed = patch(raw, &before, &after);
        assert_eq!(read(&changed).expect("card").note.as_deref(), Some("changed"), "{changed}");
        assert!(changed.contains("NOTE:second"), "the second line stays as it was: {changed}");
    }

    #[test]
    fn a_copy_gets_its_own_uid_and_nothing_else_changes() {
        let raw = fixture("nextcloud.vcf");
        let copy = set_uid(&raw, "copy-1");
        assert_eq!(read(&copy).expect("card").uid, "copy-1");
        assert_eq!(only_in(&raw, &copy).len(), 1);
    }

    #[test]
    fn long_lines_fold_inside_no_character() {
        let folded = fold(&format!("NOTE:{}", "ü".repeat(60)), "\r\n");
        for line in folded.split("\r\n") {
            assert!(line.len() <= 75, "{line}");
        }
        let doc = lines(&folded);
        assert_eq!(doc[0].prop.as_ref().map(|p| p.text()), Some("ü".repeat(60)));
    }
}
