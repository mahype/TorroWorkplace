//! Text as the mail search compares it (issue #136): folding, the words and phrases of a search
//! ([`Needle`]), and where they match in a message's own text ([`find`], [`pieces`], [`passage`]).
//!
//! **Folding** ([`fold`]) makes spellings that people treat as the same word the same text: lower
//! case, accents off (`é` → `e`, `Å` → `a`), `ß` → `ss`, and the umlauts with both their spellings
//! to the bare vowel (`ä`, `ae` → `a`; `ö`, `oe` → `o`; `ü`, `ue` → `u`). So `Müller`, `Mueller`
//! and `Muller` are one word, and so are `Straße` and `Strasse`. The store keeps its two indexes
//! (`mail_fts` and the trigram index `mail_tri`) of folded text and folds every search the same
//! way, so both sides always agree; SQLite's own tokenizers never have to know German.
//!
//! The rule also folds `ae`, `oe`, `ue` that are no umlauts (`queue` → `quu`, `Israel` →
//! `isral`). That is harmless: index and search fold alike, and folding keeps prefixes (the folded
//! start of a word is the start of the folded word), so word-start matching still works. It costs
//! a few extra matches between words that fold alike, which ranking puts behind exact ones.
//!
//! **Highlights** are found here, not by SQLite: the indexes hold folded text, which is not what a
//! person wants to read. [`find`] folds the original text with a map back to its bytes and marks
//! the matches there.

use std::ops::Range;

use torro_ipc::mail::HighlightSpan;

/// A word of at least this many characters (folded) also matches inside longer words, through
/// the trigram index (`rechnung` finds `Stromrechnung`). Shorter ones match from the start of a
/// word only: `art` inside every `Start` and `Karte` would bury what was meant.
pub const INFIX_MIN: usize = 4;

/// At most this many bytes of a text are looked through for highlights: a match further on
/// shows no passage (the message is found all the same).
pub const HIGHLIGHT_TEXT_MAX: usize = 256 * 1024;

/// About this many characters of a text around the first match make a [`passage`].
pub const PASSAGE_CHARS: usize = 200;

/// How far before the first match a [`passage`] starts, in characters.
const PASSAGE_LEAD: usize = 60;

/// `text` folded as the index keeps it; see the module documentation.
pub fn fold(text: &str) -> String {
    mapped(text).chars.into_iter().collect()
}

/// At most the first `max` bytes of `text`, cut at a character boundary.
pub fn head(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Folded text with, for each of its characters, the byte range of the original it came from.
struct Mapped {
    chars: Vec<char>,
    spans: Vec<Range<usize>>,
}

/// Folds `text` and remembers where each folded character came from.
fn mapped(text: &str) -> Mapped {
    let mut chars: Vec<char> = Vec::with_capacity(text.len());
    let mut spans: Vec<Range<usize>> = Vec::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        let span = at..at + c.len_utf8();
        for lower in c.to_lowercase() {
            let plain = plain(lower);
            for folded in plain.chars().chain(plain.is_empty().then_some(lower)) {
                // `ae`, `oe`, `ue` (also from `æ`, `œ`) become the bare vowel.
                if folded == 'e' && matches!(chars.last(), Some('a' | 'o' | 'u')) {
                    if let Some(last) = spans.last_mut() {
                        last.end = span.end;
                    }
                    continue;
                }
                chars.push(folded);
                spans.push(span.clone());
            }
        }
    }
    Mapped { chars, spans }
}

/// A lower-case letter without its accent, or `ß` as `ss`; empty for a character that stays.
fn plain(c: char) -> &'static str {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'þ' => "th",
        _ => "",
    }
}

/// The words of folded characters: runs of letters and digits, as character ranges. Everything
/// else separates words, as for SQLite's `unicode61` tokenizer.
fn words(chars: &[char]) -> Vec<Range<usize>> {
    let mut words = Vec::new();
    let mut start = None;
    for (at, c) in chars.iter().enumerate() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                words.push(from..at);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        words.push(from..chars.len());
    }
    words
}

/// What one word or phrase of a search looks for, folded: its words in order. A word of a search
/// that holds punctuation (`jk@example`) is several words in a row, like a phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needle {
    /// The folded words, never empty ones; no words at all match nothing.
    pub tokens: Vec<String>,
    /// The last word may go on (`Rech` finds `Rechnung`): typed words do, quoted phrases do not.
    pub prefix: bool,
}

impl Needle {
    /// A typed word: its folded words, the last one a prefix.
    pub fn word(text: &str) -> Needle {
        Needle { tokens: tokens(text), prefix: true }
    }

    /// A quoted phrase: its folded words exactly, in this order.
    pub fn phrase(text: &str) -> Needle {
        Needle { tokens: tokens(text), prefix: false }
    }

    /// It also matches inside longer words ([`INFIX_MIN`]): one typed word, long enough. A quoted
    /// phrase stays whole words, so quotes say "exactly this word".
    pub fn infix(&self) -> bool {
        self.prefix && matches!(self.tokens.as_slice(), [one] if one.chars().count() >= INFIX_MIN)
    }
}

/// The folded words of `text`.
fn tokens(text: &str) -> Vec<String> {
    let folded = mapped(text).chars;
    words(&folded).into_iter().map(|word| folded[word].iter().collect()).collect()
}

/// The byte ranges of `text` where any of `needles` matches, sorted and merged. A match from the
/// start of a word marks the whole last word (`Rech` marks `Rechnung`); one inside a word marks
/// from there to the word's end (`rechnung` in `Stromrechnung`).
pub fn find(text: &str, needles: &[Needle]) -> Vec<Range<usize>> {
    let folded = mapped(text);
    let words: Vec<(Range<usize>, String)> =
        words(&folded.chars).into_iter().map(|word| (word.clone(), folded.chars[word].iter().collect())).collect();
    let mut hits: Vec<Range<usize>> = Vec::new();
    for needle in needles.iter().filter(|needle| !needle.tokens.is_empty()) {
        let count = needle.tokens.len();
        for (at, (range, word)) in words.iter().enumerate() {
            let Some(row) = words.get(at..at + count) else { break };
            let last = count - 1;
            let fits = row.iter().zip(&needle.tokens).enumerate().all(|(index, ((_, word), token))| {
                if index == last && needle.prefix { word.starts_with(token.as_str()) } else { word == token }
            });
            if fits {
                hits.push(folded.spans[range.start].start..folded.spans[row[last].0.end - 1].end);
            } else if needle.infix()
                && let Some(inside) = word.find(needle.tokens[0].as_str())
            {
                let from = range.start + word[..inside].chars().count();
                hits.push(folded.spans[from].start..folded.spans[range.end - 1].end);
            }
        }
    }
    merge(hits)
}

/// Sorts `ranges` and joins those that touch or overlap.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// The whole `text` cut at `hits` (sorted byte ranges from [`find`]); empty without hits.
pub fn pieces(text: &str, hits: &[Range<usize>]) -> Vec<HighlightSpan> {
    if hits.is_empty() {
        return Vec::new();
    }
    cut(text, 0..text.len(), hits)
}

/// About [`PASSAGE_CHARS`] characters of `text` around the first of `hits`, cut at the hits in
/// it, line breaks and runs of spaces as one space, with "…" where the text goes on. Empty
/// without hits.
pub fn passage(text: &str, hits: &[Range<usize>]) -> Vec<HighlightSpan> {
    let Some(first) = hits.first() else { return Vec::new() };
    let starts: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let first_char = starts.partition_point(|&at| at < first.start);
    let mut from = first_char.saturating_sub(PASSAGE_LEAD);
    // Start at a word: back to the space before, unless that is far.
    while from > 0 && from + PASSAGE_LEAD + 20 > first_char && !char_at(text, &starts, from - 1).is_whitespace() {
        from -= 1;
    }
    let mut to = (from + PASSAGE_CHARS).min(starts.len());
    while to < starts.len() && to < from + PASSAGE_CHARS + 20 && !char_at(text, &starts, to).is_whitespace() {
        to += 1;
    }
    let byte = |index: usize| starts.get(index).copied().unwrap_or(text.len());
    let window = byte(from)..byte(to);
    let mut out = cut(text, window.clone(), hits);
    tidy(&mut out);
    if window.start > 0
        && let Some(head) = out.first_mut()
    {
        head.text.insert(0, '…');
    }
    if window.end < text.len()
        && let Some(tail) = out.last_mut()
    {
        tail.text.push('…');
    }
    out
}

fn char_at(text: &str, starts: &[usize], index: usize) -> char {
    text[starts[index]..].chars().next().unwrap_or(' ')
}

/// `text[window]` as pieces, `hits` (clipped to the window) marked.
fn cut(text: &str, window: Range<usize>, hits: &[Range<usize>]) -> Vec<HighlightSpan> {
    let mut out = Vec::new();
    let mut at = window.start;
    for hit in hits {
        let (start, end) = (hit.start.max(window.start), hit.end.min(window.end));
        if start >= end {
            continue;
        }
        if at < start {
            out.push(HighlightSpan { text: text[at..start].to_string(), hit: false });
        }
        out.push(HighlightSpan { text: text[start..end].to_string(), hit: true });
        at = end;
    }
    if at < window.end {
        out.push(HighlightSpan { text: text[at..window.end].to_string(), hit: false });
    }
    out
}

/// Line breaks and runs of white space as one space across the pieces, none at the start or the
/// end; pieces left empty go.
fn tidy(pieces: &mut Vec<HighlightSpan>) {
    let mut space = true;
    for piece in pieces.iter_mut() {
        let mut text = String::with_capacity(piece.text.len());
        for c in piece.text.chars() {
            if c.is_whitespace() {
                if !space {
                    text.push(' ');
                }
                space = true;
            } else {
                text.push(c);
                space = false;
            }
        }
        piece.text = text;
    }
    if let Some(last) = pieces.last_mut() {
        let kept = last.text.trim_end().len();
        last.text.truncate(kept);
    }
    pieces.retain(|piece| !piece.text.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(text: &str, needles: &[Needle]) -> Vec<String> {
        find(text, needles).into_iter().map(|range| text[range].to_string()).collect()
    }

    #[test]
    fn umlauts_and_their_spellings_fold_alike() {
        for same in ["Müller", "Mueller", "MULLER", "muller", "MÜLLER"] {
            assert_eq!(fold(same), "muller", "{same}");
        }
        assert_eq!(fold("Straße"), fold("STRASSE"));
        assert_eq!(fold("Grüße"), "grusse");
        assert_eq!(fold("Größe Öl Äpfel"), "grosse ol apfel");
        assert_eq!(fold("Zoë Ångström, Señor Łukasz"), "zo angstrom, senor lukasz", "oe as in Zoe folds too");
        assert_eq!(fold("Œuvre Ærø"), "ouvre aro", "ligatures are spelled out first");
        assert_eq!(fold("Rechnung 2026-10.pdf"), "rechnung 2026-10.pdf", "punctuation stays");
    }

    #[test]
    fn a_head_ends_at_a_character() {
        assert_eq!(head("Grüße", 3), "Gr", "ü takes two bytes");
        assert_eq!(head("Grüße", 4), "Grü");
        assert_eq!(head("kurz", 100), "kurz");
    }

    #[test]
    fn folding_keeps_prefixes() {
        // The folded start of a word is the start of the folded word, so word-start search works.
        for word in ["Mueller", "Grüße", "queue", "Straße", "Aerobic", "Steuererklärung"] {
            let folded = fold(word);
            for end in word.char_indices().map(|(at, _)| at).skip(1) {
                assert!(folded.starts_with(&fold(&word[..end])), "{word} at {end}");
            }
        }
    }

    #[test]
    fn needles_find_word_starts_phrases_and_long_parts_of_words() {
        let text = "Ihre Stromrechnung für Oktober; die Rechnungsnummer folgt. Grüße, Jürgen Müller";
        assert_eq!(marked(text, &[Needle::word("rechnung")]), ["rechnung", "Rechnungsnummer"]);
        assert_eq!(marked(text, &[Needle::word("rec")]), ["Rechnungsnummer"], "short: word starts only");
        assert_eq!(marked(text, &[Needle::word("mueller")]), ["Müller"]);
        assert_eq!(marked(text, &[Needle::word("grusse")]), ["Grüße"]);
        assert_eq!(marked(text, &[Needle::phrase("jurgen muller")]), ["Jürgen Müller"]);
        assert_eq!(marked(text, &[Needle::phrase("Oktober die")]), ["Oktober; die"], "punctuation between words");
        assert!(marked(text, &[Needle::phrase("rechnung")]).is_empty(), "a phrase is whole words");
        assert!(marked(text, &[Needle::phrase("Oktob")]).is_empty());
        assert!(marked(text, &[Needle::word("")]).is_empty() && marked(text, &[Needle::word("--")]).is_empty());
        let address = "From: jk@example.com";
        assert_eq!(marked(address, &[Needle::word("jk@exa")]), ["jk@example"]);
        assert!(Needle::word("rec").tokens == ["rec"] && !Needle::word("rec").infix());
        assert!(Needle::word("Rechnung").infix() && !Needle::word("a b c d").infix());
        assert!(!Needle::phrase("Rechnung").infix());
    }

    #[test]
    fn overlapping_hits_merge_and_pieces_rebuild_the_text() {
        let text = "Strom und Stromrechnung";
        let hits = find(text, &[Needle::word("strom"), Needle::word("stromrechnung"), Needle::word("und")]);
        assert_eq!(
            hits.iter().map(|range| &text[range.clone()]).collect::<Vec<_>>(),
            ["Strom", "und", "Stromrechnung"]
        );
        let pieces = pieces(text, &hits);
        assert_eq!(pieces.iter().map(|piece| piece.text.as_str()).collect::<String>(), text);
        assert_eq!(pieces.iter().filter(|piece| piece.hit).count(), 3);
        assert!(super::pieces(text, &[]).is_empty());
    }

    #[test]
    fn a_passage_shows_the_first_match_with_some_text_around_it() {
        let long = format!("{} Die Stromrechnung\n\n  ist da. {}", "Vorher ".repeat(40), "Danach ".repeat(40));
        let hits = find(&long, &[Needle::word("rechnung")]);
        let passage = passage(&long, &hits);
        let text: String = passage.iter().map(|piece| piece.text.as_str()).collect();
        assert!(text.starts_with('…') && text.ends_with('…'), "{text}");
        assert!(text.contains("Die Stromrechnung ist da."), "white space is tidied: {text}");
        assert!(text.chars().count() <= PASSAGE_CHARS + 25, "{}", text.chars().count());
        assert_eq!(
            passage.iter().filter(|piece| piece.hit).map(|piece| piece.text.as_str()).collect::<Vec<_>>(),
            ["rechnung"]
        );
        let short = "Rechnung anbei";
        let whole = super::passage(short, &find(short, &[Needle::word("rechnung")]));
        assert_eq!(whole.iter().map(|piece| piece.text.as_str()).collect::<String>(), "Rechnung anbei");
        assert!(super::passage(short, &[]).is_empty());
    }
}
