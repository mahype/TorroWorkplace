//! The query language of the mail search (issue #136): what a person or an agent types into
//! `query` of `mail.search_messages`, read into a [`Query`] the store turns into SQL.
//!
//! ```text
//! stromrechnung from:stadtwerke -mahnung "zweite erinnerung" has:attachment after:2026-01
//! rechnung OR invoice in:archive is:unread subject:"Q3 report" filename:pdf before:2026-03-01
//! ```
//!
//! - **Words** are separated by spaces and all required. Each matches from the start of a word;
//!   long ones also inside words (see `fold::INFIX_MIN`).
//! - **`"a phrase"`** matches these words in this order, whole words only. An unclosed quote
//!   runs to the end.
//! - **`-word`**, `-"phrase"`, `-from:x` exclude what they would match.
//! - **`OR`** (upper case) between two terms takes either: `a OR b c` is `(a or b) and c`.
//! - **Operators** `name:value` (the name in any case, the value a word or a quoted phrase):
//!   `from:`, `to:` (also cc), `subject:`, `filename:`, `has:attachment`, `in:` a folder's name,
//!   path or role (`inbox`, `drafts`, `sent`, `archive`, `junk` or `spam`, `trash`), `before:` and
//!   `after:` a day, month or year (`2026-03-01`, `2026-03`, `2026`; UTC, before is exclusive, after
//!   inclusive), `is:unread`, `is:read`, `is:flagged` (or `starred`), `is:unflagged`.
//!
//! An unknown name is no operator: `re:` or `https://…` are words like any other. An operator
//! without a value (`from:` while typing) is left out. A known operator with a value it cannot
//! read is an error ([`QueryError`]), so a search never silently means something else.

use chrono::{DateTime, NaiveDate, Utc};
use torro_core::i18n::trf;

/// A parsed search: every clause must hold. Empty: everything matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub clauses: Vec<Clause>,
}

/// Terms joined by `OR`: one of them must hold. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub any: Vec<Term>,
}

/// One test, or its opposite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// `-` before it: the term must not hold.
    pub negated: bool,
    pub test: Test,
}

/// What one term asks of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// Words (`phrase: false`) or a phrase in `field`, as typed (folding is the store's).
    Text { field: Field, text: String, phrase: bool },
    /// `has:attachment`.
    HasAttachment,
    /// `is:unread` (`true`) or `is:read` (`false`).
    Unread(bool),
    /// `is:flagged` (`true`) or `is:unflagged` (`false`).
    Flagged(bool),
    /// `in:`: a folder's name, path or role, as typed.
    In(String),
    /// `before:`: sent before this instant.
    Before(DateTime<Utc>),
    /// `after:`: sent at or after this instant.
    After(DateTime<Utc>),
}

/// Where the words of a [`Test::Text`] are looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Subject, sender, recipients, text and attachment names.
    Any,
    /// `from:`: the sender's name and address.
    From,
    /// `to:`: the names and addresses in To and Cc.
    To,
    /// `subject:`.
    Subject,
    /// `filename:`: the names of the attachments.
    Filename,
}

/// A query that cannot be read. The texts are for people: `mail.search_messages` answers `error`
/// with [`QueryError::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// `before:` or `after:` with something that is no day, month or year.
    Date { operator: String, value: String },
    /// `is:` with something else than `unread`, `read`, `flagged`, `unflagged`.
    Is(String),
    /// `has:` with something else than `attachment`.
    Has(String),
}

impl QueryError {
    /// What went wrong and how to write it instead, in the person's language.
    pub fn message(&self) -> String {
        match self {
            QueryError::Date { operator, value } => trf(
                "“{operator}:{value}” is no date. Write a day, month or year, like {operator}:2026-03-01.",
                &[("operator", operator), ("value", value)],
            ),
            QueryError::Is(value) => trf(
                "“is:{value}” is not known. Use is:unread, is:read, is:flagged or is:unflagged.",
                &[("value", value)],
            ),
            QueryError::Has(value) => trf("“has:{value}” is not known. Use has:attachment.", &[("value", value)]),
        }
    }
}

impl Query {
    /// Nothing to test: every message matches.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// Every term, in order.
    pub fn terms(&self) -> impl Iterator<Item = &Term> {
        self.clauses.iter().flat_map(|clause| clause.any.iter())
    }

    /// It looks for words or phrases (anything [`Test::Text`]), so matches can be ranked and
    /// highlighted.
    pub fn has_words(&self) -> bool {
        self.terms().any(|term| matches!(term.test, Test::Text { .. }))
    }

    /// The query without the clauses that only ask for folders (`in:` alone, or several joined by
    /// `OR`), every other clause as it is (task E19.1). For the mail back from a follow-up with
    /// `snoozed: false`: it stands on top of "All" (`in:inbox`) also when it lies elsewhere, a sent
    /// mail nobody answered. A clause mixing `in:` with another test stays whole, and so does one
    /// that leaves a folder out (`-in:trash`): that narrows, it never asks for a place.
    pub fn without_folders(&self) -> Query {
        let folders_only =
            |clause: &Clause| clause.any.iter().all(|term| !term.negated && matches!(term.test, Test::In(_)));
        Query { clauses: self.clauses.iter().filter(|clause| !folders_only(clause)).cloned().collect() }
    }

    /// It needs the texts of messages (words anywhere, or attachment names), which are indexed
    /// after the headers: while the index is not complete, it may miss matches.
    pub fn needs_texts(&self) -> bool {
        self.terms().any(|term| matches!(term.test, Test::Text { field: Field::Any | Field::Filename, .. }))
    }

    /// The words and phrases to mark in a message's `field` (`Field::Subject`, or `Field::Any` for
    /// its text): the terms that must hold, not excluded ones, of that field or of every field.
    pub fn marked(&self, field: Field) -> Vec<(&str, bool)> {
        self.terms()
            .filter(|term| !term.negated)
            .filter_map(|term| match &term.test {
                Test::Text { field: of, text, phrase } if *of == Field::Any || *of == field => {
                    Some((text.as_str(), *phrase))
                }
                _ => None,
            })
            .collect()
    }
}

/// One piece of the typed text, before `OR` joins pieces into clauses.
#[derive(Debug)]
enum Piece {
    Or,
    Term(Term),
}

/// Reads `text` as a query; see the module documentation. Blank text is the empty query.
pub fn parse(text: &str) -> Result<Query, QueryError> {
    let mut pieces = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let (lexeme, after) = lexeme(rest);
        rest = after.trim_start();
        if let Some(piece) = piece(lexeme)? {
            pieces.push(piece);
        }
    }
    Ok(Query { clauses: clauses(pieces) })
}

/// One lexeme as typed: `-`, an operator name, and a word or quoted phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lexeme<'a> {
    negated: bool,
    name: Option<&'a str>,
    value: &'a str,
    quoted: bool,
}

/// The first lexeme of `text` (which starts with no white space) and what follows it.
fn lexeme(text: &str) -> (Lexeme<'_>, &str) {
    let (negated, body) = match text.strip_prefix('-') {
        Some(after) if after.chars().next().is_some_and(|c| !c.is_whitespace()) => (true, after),
        _ => (false, text),
    };
    if let Some(inner) = body.strip_prefix('"') {
        let (value, after) = quoted(inner);
        return (Lexeme { negated, name: None, value, quoted: true }, after);
    }
    let end = body.find(char::is_whitespace).unwrap_or(body.len());
    // `name:"a phrase"`: the quote may hold spaces.
    if let Some(colon) = body[..end].find(':')
        && body[colon + 1..].starts_with('"')
    {
        let (value, after) = quoted(&body[colon + 2..]);
        return (Lexeme { negated, name: Some(&body[..colon]), value, quoted: true }, after);
    }
    let word = &body[..end];
    let lexeme = match word.split_once(':') {
        Some((name, value)) if !name.is_empty() => Lexeme { negated, name: Some(name), value, quoted: false },
        _ => Lexeme { negated, name: None, value: word, quoted: false },
    };
    (lexeme, &body[end..])
}

/// The text up to the closing quote (or the end), and what follows the quote.
fn quoted(text: &str) -> (&str, &str) {
    match text.find('"') {
        Some(end) => (&text[..end], &text[end + 1..]),
        None => (text, ""),
    }
}

/// The piece a lexeme stands for; `None` for an operator without a value.
fn piece(lexeme: Lexeme<'_>) -> Result<Option<Piece>, QueryError> {
    let Lexeme { negated, name, value, quoted } = lexeme;
    let term = |test| Ok(Some(Piece::Term(Term { negated, test })));
    let text = |field, text: &str| term(Test::Text { field, text: text.to_string(), phrase: quoted });
    let Some(name) = name else {
        if value == "OR" && !negated && !quoted {
            return Ok(Some(Piece::Or));
        }
        return text(Field::Any, value);
    };
    let operator = name.to_lowercase();
    let known = ["from", "to", "subject", "filename", "has", "in", "is", "before", "after"];
    if !known.contains(&operator.as_str()) {
        // Not an operator: the whole lexeme is words (`re:`, `https://…`).
        return text(Field::Any, &format!("{name}:{value}"));
    }
    if value.trim().is_empty() {
        return Ok(None);
    }
    let lower = value.to_lowercase();
    match operator.as_str() {
        "from" => text(Field::From, value),
        "to" => text(Field::To, value),
        "subject" => text(Field::Subject, value),
        "filename" => text(Field::Filename, value),
        "in" => term(Test::In(value.trim().to_string())),
        "has" => match lower.as_str() {
            "attachment" | "attachments" => term(Test::HasAttachment),
            _ => Err(QueryError::Has(value.to_string())),
        },
        "is" => match lower.as_str() {
            "unread" => term(Test::Unread(true)),
            "read" => term(Test::Unread(false)),
            "flagged" | "starred" => term(Test::Flagged(true)),
            "unflagged" | "unstarred" => term(Test::Flagged(false)),
            _ => Err(QueryError::Is(value.to_string())),
        },
        _ => {
            let at = start_of(value.trim())
                .ok_or_else(|| QueryError::Date { operator: operator.clone(), value: value.to_string() })?;
            term(if operator == "before" { Test::Before(at) } else { Test::After(at) })
        }
    }
}

/// The start (midnight UTC) of the day `2026-03-01`, the month `2026-03` or the year `2026`.
fn start_of(value: &str) -> Option<DateTime<Utc>> {
    let parts: Vec<&str> = value.split('-').collect();
    let number = |text: &str, digits: usize| (text.len() == digits).then(|| text.parse::<u32>().ok()).flatten();
    let (year, month, day) = match parts.as_slice() {
        [year] => (number(year, 4)?, 1, 1),
        [year, month] => (number(year, 4)?, number(month, 2)?, 1),
        [year, month, day] => (number(year, 4)?, number(month, 2)?, number(day, 2)?),
        _ => return None,
    };
    let date = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc())
}

/// Joins the terms around each `OR` into one clause; every other term is a clause of its own. An
/// `OR` without a term on both sides is left out.
fn clauses(pieces: Vec<Piece>) -> Vec<Clause> {
    let mut clauses: Vec<Clause> = Vec::new();
    let mut joined = false;
    for piece in pieces {
        match piece {
            Piece::Or => joined = !clauses.is_empty(),
            Piece::Term(term) => match (joined, clauses.last_mut()) {
                (true, Some(last)) => {
                    last.any.push(term);
                    joined = false;
                }
                _ => clauses.push(Clause { any: vec![term] }),
            },
        }
    }
    clauses
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_core::i18n::tr;

    use super::*;

    fn word(text: &str) -> Term {
        Term { negated: false, test: Test::Text { field: Field::Any, text: text.into(), phrase: false } }
    }

    fn phrase(field: Field, text: &str) -> Term {
        Term { negated: false, test: Test::Text { field, text: text.into(), phrase: true } }
    }

    fn not(term: Term) -> Term {
        Term { negated: true, ..term }
    }

    fn of(field: Field, text: &str) -> Term {
        Term { negated: false, test: Test::Text { field, text: text.into(), phrase: false } }
    }

    fn test(test: Test) -> Term {
        Term { negated: false, test }
    }

    /// The clauses of `text`, each as its terms.
    fn read(text: &str) -> Vec<Vec<Term>> {
        parse(text).expect("parses").clauses.into_iter().map(|clause| clause.any).collect()
    }

    fn day(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single().expect("a day")
    }

    #[test]
    fn words_are_all_required_and_blank_is_everything() {
        assert_eq!(read("Stromrechnung  Oktober"), [vec![word("Stromrechnung")], vec![word("Oktober")]]);
        assert!(parse("").expect("empty").is_empty() && parse("  \t ").expect("blank").is_empty());
        assert_eq!(read("jk@example.com"), [vec![word("jk@example.com")]], "punctuation stays in the word");
        assert_eq!(read("or and"), [vec![word("or")], vec![word("and")]], "only OR in capitals joins");
    }

    #[test]
    fn phrases_are_quoted_and_an_open_quote_runs_to_the_end() {
        assert_eq!(
            read(r#""zweite Erinnerung" bitte"#),
            [vec![phrase(Field::Any, "zweite Erinnerung")], vec![word("bitte")]]
        );
        assert_eq!(
            read(r#"bitte "zweite Erinnerung"#),
            [vec![word("bitte")], vec![phrase(Field::Any, "zweite Erinnerung")]]
        );
        assert_eq!(read(r#""""#), [vec![phrase(Field::Any, "")]], "an empty phrase matches nothing (the store's rule)");
        assert_eq!(read(r#"a"b"#), [vec![word(r#"a"b"#)]], "a quote inside a word is part of it");
    }

    #[test]
    fn a_dash_excludes_and_a_lone_dash_is_a_word() {
        assert_eq!(read("rechnung -mahnung"), [vec![word("rechnung")], vec![not(word("mahnung"))]]);
        assert_eq!(read(r#"-"zweite Erinnerung""#), [vec![not(phrase(Field::Any, "zweite Erinnerung"))]]);
        assert_eq!(read("-from:bank"), [vec![not(of(Field::From, "bank"))]]);
        assert_eq!(read("-is:unread"), [vec![not(test(Test::Unread(true)))]]);
        assert_eq!(read("a - b"), [vec![word("a")], vec![word("-")], vec![word("b")]]);
        assert_eq!(read("e-mail"), [vec![word("e-mail")]], "a dash inside a word is no exclusion");
    }

    #[test]
    fn or_joins_its_neighbours_and_stray_ors_are_left_out() {
        assert_eq!(read("rechnung OR invoice april"), [vec![word("rechnung"), word("invoice")], vec![word("april")]]);
        assert_eq!(read("a OR b OR c"), [vec![word("a"), word("b"), word("c")]]);
        assert_eq!(read("OR a"), [vec![word("a")]]);
        assert_eq!(read("a OR"), [vec![word("a")]]);
        assert_eq!(read("a OR OR b"), [vec![word("a"), word("b")]]);
        assert_eq!(read(r#"from:anna OR -"x y""#), [vec![of(Field::From, "anna"), not(phrase(Field::Any, "x y"))]]);
        assert_eq!(read(r#""OR""#), [vec![phrase(Field::Any, "OR")]], "a quoted OR is a word");
    }

    #[test]
    fn field_operators_take_a_word_or_a_phrase() {
        assert_eq!(read("from:stadtwerke"), [vec![of(Field::From, "stadtwerke")]]);
        assert_eq!(
            read("FROM:Anna to:bob@example.org"),
            [vec![of(Field::From, "Anna")], vec![of(Field::To, "bob@example.org")]]
        );
        assert_eq!(read(r#"subject:"Q3 report" pdf"#), [vec![phrase(Field::Subject, "Q3 report")], vec![word("pdf")]]);
        assert_eq!(read("filename:rechnung.pdf"), [vec![of(Field::Filename, "rechnung.pdf")]]);
        assert_eq!(read("from:a:b"), [vec![of(Field::From, "a:b")]], "the first colon names the operator");
    }

    #[test]
    fn flags_attachments_and_folders() {
        assert_eq!(read("has:attachment"), [vec![test(Test::HasAttachment)]]);
        assert_eq!(read("has:Attachments"), [vec![test(Test::HasAttachment)]]);
        assert_eq!(read("is:unread is:READ"), [vec![test(Test::Unread(true))], vec![test(Test::Unread(false))]]);
        assert_eq!(read("is:flagged is:starred"), [vec![test(Test::Flagged(true))], vec![test(Test::Flagged(true))]]);
        assert_eq!(read("is:unflagged"), [vec![test(Test::Flagged(false))]]);
        assert_eq!(read("in:archive"), [vec![test(Test::In("archive".into()))]]);
        assert_eq!(read(r#"in:"Old projects""#), [vec![test(Test::In("Old projects".into()))]]);
    }

    #[test]
    fn dates_are_days_months_or_years_in_utc() {
        assert_eq!(read("before:2026-03-01"), [vec![test(Test::Before(day(2026, 3, 1)))]]);
        assert_eq!(read("after:2026-03"), [vec![test(Test::After(day(2026, 3, 1)))]]);
        assert_eq!(read("after:2025"), [vec![test(Test::After(day(2025, 1, 1)))]]);
        assert_eq!(read("-before:2026-01-31"), [vec![not(test(Test::Before(day(2026, 1, 31))))]]);
        for bad in [
            "before:soon",
            "after:2026-13",
            "after:2026-02-30",
            "before:26-01-01",
            "after:2026-1-1",
            "before:2026/03/01",
        ] {
            assert!(matches!(parse(bad), Err(QueryError::Date { .. })), "{bad}");
        }
    }

    #[test]
    fn unknown_operators_are_words_and_empty_ones_are_left_out() {
        assert_eq!(read("re:offer"), [vec![word("re:offer")]]);
        assert_eq!(read("https://example.org/x"), [vec![word("https://example.org/x")]]);
        assert_eq!(read(":colon"), [vec![word(":colon")]]);
        assert_eq!(read("from: anna"), [vec![word("anna")]], "typing an operator filters nothing yet");
        assert_eq!(read("rechnung from:"), [vec![word("rechnung")]]);
        assert_eq!(read(r#"subject:"""#), Vec::<Vec<Term>>::new());
    }

    #[test]
    fn values_an_operator_cannot_read_are_errors_with_a_hint() {
        assert_eq!(parse("is:banana"), Err(QueryError::Is("banana".into())));
        assert_eq!(parse("has:pdf"), Err(QueryError::Has("pdf".into())));
        let message = parse("before:soon").expect_err("no date").message();
        let hint = trf(
            "“{operator}:{value}” is no date. Write a day, month or year, like {operator}:2026-03-01.",
            &[("operator", &"before"), ("value", &"soon")],
        );
        assert_eq!(message, hint);
        assert!(!tr("“has:{value}” is not known. Use has:attachment.").is_empty());
    }

    #[test]
    fn a_query_knows_what_it_needs_and_what_to_mark() {
        let query = parse(r#"rechnung subject:"Q3 report" -mahnung from:anna is:unread"#).expect("parses");
        assert!(query.has_words() && query.needs_texts());
        assert_eq!(query.marked(Field::Any), [("rechnung", false)]);
        assert_eq!(query.marked(Field::Subject), [("rechnung", false), ("Q3 report", true)]);
        let headers = parse("from:anna subject:offer is:flagged").expect("parses");
        assert!(headers.has_words() && !headers.needs_texts(), "headers are indexed from the start");
        assert!(parse("filename:pdf").expect("parses").needs_texts());
        assert!(!parse("is:unread in:inbox").expect("parses").has_words());
    }

    /// Task E19.1: only clauses that ask for folders go; the rest stays as it was.
    #[test]
    fn without_folders_drops_only_what_asks_for_a_folder() {
        let query = parse("in:inbox offer in:sent OR in:archive -in:trash in:inbox OR is:unread").expect("parses");
        assert_eq!(
            query.without_folders(),
            Query {
                clauses: vec![
                    Clause { any: vec![word("offer")] },
                    Clause { any: vec![not(test(Test::In("trash".into())))] },
                    Clause { any: vec![test(Test::In("inbox".into())), test(Test::Unread(true))] },
                ]
            }
        );
        assert!(parse("in:inbox").expect("parses").without_folders().is_empty());
    }
}
