//! An answer in sentences as the model writes it (issue #747, `assistant.speak_answer`): the core
//! reads the first sentence aloud while the rest is still being written.
//!
//! [`Sentences::push`] takes the whole text written so far and gives the pieces that became complete
//! since the last call; [`Sentences::finish`] gives what is left once the answer is over. A sentence
//! ends at `.`, `!`, `?`, `…` or `:` before a space, and at the end of a line. A full stop after a
//! number ("1. "), after a single letter ("z. ") or after a known abbreviation ("bzw.", "e.g.")
//! ends none. Code blocks are left out: they are not read aloud. At most [`MOST`] characters are
//! given in all, as much as `assistant.speak` reads, or fewer ([`Sentences::with_most`], issue #774).
//! What is given ends at a sentence end within the limit; only a first sentence longer than it is cut.

/// The most characters given in all, as `assistant.speak` reads at most (issue #745).
pub const MOST: usize = 3000;

/// Abbreviations a full stop does not end a sentence after, lower case, without their last dot.
const ABBREVIATIONS: [&str; 10] = ["bzw", "ca", "nr", "dr", "z.b", "e.g", "i.e", "usw", "etc", "vgl"];

/// The sentences of a growing text (see the module).
#[derive(Debug)]
pub struct Sentences {
    /// The text taken so far: the start of every later text.
    taken: String,
    /// What was taken but belongs to no complete sentence yet.
    open: String,
    /// Where in [`Sentences::taken`] [`Sentences::open`] begins: what lies before it was given or dropped.
    open_at: usize,
    /// Inside a code block (between two lines starting with three backticks).
    in_code: bool,
    /// How many characters were given.
    given: usize,
    /// The most characters given in all.
    most: usize,
}

impl Default for Sentences {
    fn default() -> Self {
        Self::with_most(MOST)
    }
}

impl Sentences {
    /// Sentences that give at most `most` characters in all (at most [`MOST`]).
    pub fn with_most(most: usize) -> Self {
        Self { taken: String::new(), open: String::new(), open_at: 0, in_code: false, given: 0, most: most.min(MOST) }
    }

    /// The pieces of `text` – the whole answer written so far – that became complete since the last
    /// call, in order. A `text` that does not go on from the last one (the model's text was replaced)
    /// goes on where both agree.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        let common = common_prefix(&self.taken, text);
        if common < self.taken.len() {
            // Replaced: what was given stays given; what both agree on and was not given yet stays
            // open, and whether a code block is open is counted again up to there.
            self.taken.truncate(common);
            self.open_at = self.open_at.min(common);
            self.open = self.taken[self.open_at..].to_string();
            self.in_code =
                self.taken[..self.open_at].lines().filter(|line| line.trim_start().starts_with("```")).count() % 2 == 1;
        }
        let new = &text[self.taken.len()..];
        self.taken.push_str(new);
        let mut pieces = Vec::new();
        for c in new.chars() {
            self.open.push(c);
            if c == '\n' {
                let line = std::mem::take(&mut self.open);
                self.line(&line, &mut pieces);
            }
        }
        // Within the open line: every sentence that ended, once the next character came.
        if !self.in_code {
            while let Some(end) = sentence_end(&self.open) {
                let rest = self.open.split_off(end);
                let sentence = std::mem::replace(&mut self.open, rest);
                self.give(&sentence, &mut pieces);
            }
        }
        self.open_at = self.taken.len() - self.open.len();
        pieces
    }

    /// What is left once the answer is over: the last sentence without its end.
    pub fn finish(&mut self) -> Vec<String> {
        let line = std::mem::take(&mut self.open);
        let mut pieces = Vec::new();
        if !self.in_code && !line.trim_start().starts_with("```") {
            self.give(&line, &mut pieces);
        }
        pieces
    }

    /// A whole line ended: a fence switches code on or off, other lines outside code are given.
    fn line(&mut self, line: &str, pieces: &mut Vec<String>) {
        if line.trim_start().starts_with("```") {
            self.in_code = !self.in_code;
            return;
        }
        if !self.in_code {
            self.give(line, pieces);
        }
    }

    /// Gives `piece` when it has something to say and the limit is not reached. A piece that does not fit
    /// ends the giving: it is left out, so what was read ends at a sentence end, unless nothing was given
    /// yet; then it is cut after its last sentence end within the limit, or at the limit when it has none.
    fn give(&mut self, piece: &str, pieces: &mut Vec<String>) {
        let piece = piece.trim();
        if !piece.chars().any(char::is_alphanumeric) || self.given >= self.most {
            return;
        }
        let left = self.most - self.given;
        if piece.chars().count() <= left {
            self.given += piece.chars().count();
            pieces.push(piece.to_string());
            return;
        }
        let first = self.given == 0;
        self.given = self.most;
        if !first {
            return;
        }
        let cut: String = piece.chars().take(left).collect();
        let mut end = 0;
        while let Some(found) = sentence_end(&cut[end..]) {
            end += found;
        }
        pieces.push(if end > 0 { cut[..end].trim().to_string() } else { cut });
    }
}

/// The length in bytes both `a` and `b` start with, on a character boundary.
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices().zip(b.chars()).take_while(|((_, x), y)| x == y).last().map_or(0, |((i, x), _)| i + x.len_utf8())
}

/// Where the first sentence of `text` ends (the byte after its mark), when a space follows the mark;
/// `None` while no sentence ended.
fn sentence_end(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (at, &(index, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?' | '…' | ':') {
            continue;
        }
        let &(_, next) = chars.get(at + 1)?;
        if !next.is_whitespace() {
            continue;
        }
        if c == '.' && !ends_with_dot(&text[..index]) {
            continue;
        }
        return Some(index + c.len_utf8());
    }
    None
}

/// Whether a full stop after `before` ends a sentence: not after a number, a single letter or a
/// known abbreviation.
fn ends_with_dot(before: &str) -> bool {
    let word = before.rsplit(char::is_whitespace).next().unwrap_or_default();
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    if word.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    if word.chars().count() == 1 {
        return false;
    }
    !ABBREVIATIONS.contains(&word.to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `text` a few characters at a time, then finishes.
    fn pieces_of(text: &str, step: usize) -> Vec<String> {
        let mut sentences = Sentences::default();
        let chars: Vec<char> = text.chars().collect();
        let mut pieces = Vec::new();
        let mut end = 0;
        while end < chars.len() {
            end = (end + step).min(chars.len());
            let written: String = chars[..end].iter().collect();
            pieces.extend(sentences.push(&written));
        }
        pieces.extend(sentences.finish());
        pieces
    }

    #[test]
    fn the_first_sentence_comes_before_the_answer_is_over() {
        let mut sentences = Sentences::default();
        assert!(sentences.push("Drei Termine").is_empty(), "nothing ended yet");
        assert!(sentences.push("Drei Termine.").is_empty(), "the next character decides");
        assert_eq!(sentences.push("Drei Termine. Um halb"), ["Drei Termine."]);
        assert!(sentences.push("Drei Termine. Um halb zehn").is_empty());
        assert_eq!(sentences.finish(), ["Um halb zehn"]);
    }

    #[test]
    fn numbers_letters_and_abbreviations_end_no_sentence() {
        let text = "Am 3. Mai, z. B. um 9.30 Uhr, bzw. später. Dr. Weber kommt! Wirklich? Ja: gut";
        assert_eq!(
            pieces_of(text, 3),
            ["Am 3. Mai, z. B. um 9.30 Uhr, bzw. später.", "Dr. Weber kommt!", "Wirklich?", "Ja:", "gut"]
        );
    }

    #[test]
    fn lines_end_pieces_and_code_is_left_out() {
        let text = "Heute:\n- Standup\n- Mittagessen\n```rust\nfn main() {}\n```\nFertig.";
        assert_eq!(pieces_of(text, 5), ["Heute:", "- Standup", "- Mittagessen", "Fertig."]);
    }

    #[test]
    fn at_most_the_limit_is_given() {
        let text = "Ein Satz hier. ".repeat(400);
        let pieces = pieces_of(&text, 50);
        let given: usize = pieces.iter().map(|piece| piece.chars().count()).sum();
        assert!(given <= MOST && given > MOST - "Ein Satz hier.".len(), "{given}");
        assert!(pieces.iter().all(|piece| piece == "Ein Satz hier."), "only whole sentences");
    }

    /// Issue #774: a lower limit ends at a sentence end within it; only a first sentence longer than it
    /// is cut, after a sentence end in it when it has one.
    #[test]
    fn a_lower_limit_ends_at_a_sentence_end() {
        let mut sentences = Sentences::with_most(30);
        let text = "Neun Termine. Der Donnerstag ist voll. Am Freitag frei. ";
        assert_eq!(sentences.push(text), ["Neun Termine."], "the second sentence does not fit");
        assert!(sentences.finish().is_empty());
        let mut long = Sentences::with_most(10);
        assert_eq!(long.push("Ein sehr langer Satz ohne Ende"), Vec::<String>::new());
        assert_eq!(long.finish(), ["Ein sehr l"], "a first sentence longer than the limit is cut");
        let mut line = Sentences::with_most(20);
        assert_eq!(line.push("Ja: das passt gut so, alles klar\n"), ["Ja:"], "cut after its own sentence end");
        assert_eq!(Sentences::with_most(MOST + 1).most, MOST, "never more than speak reads");
    }

    #[test]
    fn a_replaced_text_goes_on_where_both_agree() {
        let mut sentences = Sentences::default();
        assert_eq!(sentences.push("Eins. Zwei"), ["Eins."]);
        assert_eq!(sentences.push("Eins. Drei. "), ["Drei."], "the new text from where both agree");
        assert!(sentences.finish().is_empty());
    }

    /// Review of PR #768: a replaced text keeps the start of the sentence both agree on.
    #[test]
    fn a_replaced_text_keeps_the_open_start_both_agree_on() {
        let mut sentences = Sentences::default();
        assert_eq!(sentences.push("Eins. Zwei und"), ["Eins."]);
        assert_eq!(sentences.push("Eins. Zwei oder drei. "), ["Zwei oder drei."], "with its start");
        let mut code = Sentences::default();
        assert!(code.push("```\nfn main()").is_empty());
        assert_eq!(code.push("```\nfn x()\n```\nJa. "), ["Ja."], "the code block is counted again");
    }

    #[test]
    fn marks_without_words_are_not_given() {
        assert_eq!(pieces_of("---\n**Ja.**", 2), ["**Ja.**"]);
    }
}
