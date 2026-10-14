//! An answer that appears letter by letter (issue #135), like in Claude or Codex: calm, without
//! jumps, however the text comes.
//!
//! The core hands the window the text in pieces – every follow of the window (`app::FOLLOW`) a few
//! dozen letters, sometimes more after a pause. [`Smoother`] keeps what came and lets it appear
//! on the window's frames:
//!
//! - **The pace follows the backlog**: what came and does not show yet, spread over [`LAG`]
//!   ([`LAG_DONE`] once the answer is complete), at least [`MIN_RATE`]. A steady model makes a
//!   steady pace; a large piece is caught up quickly instead of queueing; the last letters come one
//!   a frame, so nothing trickles in after the model is done.
//! - **Letters are graphemes**, not bytes or chars: an umlaut written with a combining mark or an
//!   emoji sequence appears whole. The last grapheme of a text that still grows waits, since the
//!   next piece may extend it.
//! - **Markdown does not jump** ([`display`]): what shows is read as Markdown only in a form that
//!   stays the same as it grows – a heading, list or code fence waits for its first word, open bold
//!   and inline code are closed for the drawing.
//!
//! Pure and without a clock: the window passes the time between its frames, so the tests can play
//! any pace.

use std::borrow::Cow;
use std::time::Duration;

use unicode_segmentation::UnicodeSegmentation;

/// How far the shown text trails what came while the model still writes: the backlog is spread
/// over this time. Long enough to even out the pieces of every follow, short enough that the
/// answer keeps up with the model.
const LAG: f64 = 0.25;

/// The same once the answer is complete: what is left comes out quickly.
const LAG_DONE: f64 = 0.12;

/// The slowest pace in graphemes a second: the end of a backlog comes one letter a frame, never
/// slower.
const MIN_RATE: f64 = 60.0;

/// The longest time one frame counts: after a stall (window hidden, machine busy) the text goes on
/// where it was instead of jumping ahead.
const MAX_STEP: Duration = Duration::from_millis(50);

/// The text of one answer as far as it came, and how much of it shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Smoother {
    /// The answer as the core has it.
    target: String,
    /// The bytes of `target` that show; always at a grapheme boundary.
    shown: usize,
    /// Graphemes owed from earlier frames, below one.
    carry: f64,
    /// The answer is stored and will not grow any more.
    complete: bool,
}

impl Smoother {
    /// A smoother that shows nothing yet.
    pub fn new() -> Smoother {
        Smoother::default()
    }

    /// The answer as the core has it now; `complete` once it is stored and will not grow.
    ///
    /// Usually `text` only grew. Text that changed behind what shows (the model started its step
    /// anew) takes the shown part back to where both agree, at a grapheme boundary, and goes on
    /// from there.
    pub fn set(&mut self, text: &str, complete: bool) {
        if !text.starts_with(self.shown()) {
            self.shown = common_prefix(self.shown(), text);
            self.carry = 0.0;
        }
        if self.target != text {
            text.clone_into(&mut self.target);
        }
        self.complete = complete;
    }

    /// What shows now: the start of the answer, whole graphemes only.
    pub fn shown(&self) -> &str {
        &self.target[..self.shown]
    }

    /// Something came that does not show yet: the window asks for frames only then.
    pub fn revealing(&self) -> bool {
        self.shown < self.limit()
    }

    /// The whole answer shows and it will not grow.
    pub fn done(&self) -> bool {
        self.complete && self.shown == self.target.len()
    }

    /// Shows at once everything that may show.
    pub fn finish(&mut self) {
        self.shown = self.shown.max(self.limit());
        self.carry = 0.0;
    }

    /// One frame, `elapsed` after the one before: shows the next letters at the pace of the
    /// backlog. `true` when more shows now.
    pub fn advance(&mut self, elapsed: Duration) -> bool {
        let limit = self.limit();
        if self.shown >= limit {
            self.carry = 0.0;
            return false;
        }
        let ahead = &self.target[self.shown..limit];
        let backlog = ahead.graphemes(true).count();
        let lag = if self.complete { LAG_DONE } else { LAG };
        let rate = (backlog as f64 / lag).max(MIN_RATE);
        self.carry += rate * elapsed.min(MAX_STEP).as_secs_f64();
        if self.carry < 1.0 {
            return false;
        }
        let steps = self.carry.floor();
        self.carry -= steps;
        // `steps` is a small whole number: at most the backlog over `LAG_DONE` for one `MAX_STEP`.
        let step = ahead.grapheme_indices(true).nth(steps as usize).map_or(ahead.len(), |(at, _)| at);
        self.shown += step;
        if self.shown >= limit {
            self.carry = 0.0;
        }
        true
    }

    /// The end of what may show now: all of a complete answer; of a growing one everything but its
    /// last grapheme, which the next piece may still extend (a combining mark, the rest of an emoji
    /// sequence).
    fn limit(&self) -> usize {
        if self.complete {
            return self.target.len();
        }
        self.target.grapheme_indices(true).next_back().map_or(0, |(at, _)| at)
    }
}

/// The bytes at the start of `a` and `b` that are the same graphemes.
fn common_prefix(a: &str, b: &str) -> usize {
    a.graphemes(true).zip(b.graphemes(true)).take_while(|(x, y)| x == y).map(|(x, _)| x.len()).sum()
}

/// What of `shown` – the start of an answer that grows – to read as Markdown, so the drawing never
/// jumps while it grows. `settled`: the whole answer shows; it is drawn as it is, the same as an
/// answer that never streamed.
///
/// - A last line that only starts a block – `#`, `-`, `*`, `+`, `>`, `1.`, a rule, a code fence –
///   waits for its first word (a fence for its line), so it never shows as a paragraph first and
///   then turns into a heading, a list or a code block.
/// - `*` and `` ` `` at the very end wait for what follows: they may open bold or code.
/// - Bold and inline code that are open in the last paragraph are closed for the drawing, so the
///   letters show bold or as code at once and the markers never flash up. Inside an open code
///   fence nothing is closed: the parser already draws the open block as code.
pub fn display(shown: &str, settled: bool) -> Cow<'_, str> {
    if settled {
        return Cow::Borrowed(shown);
    }
    let mut text = shown;
    let line = text.rfind('\n').map_or(0, |at| at + 1);
    if opens_block(&text[line..]) {
        text = &text[..line];
    }
    text = text.trim_end_matches(['*', '`']);
    if in_open_fence(text) {
        return Cow::Borrowed(text);
    }
    let paragraph = text.rfind("\n\n").map_or(0, |at| at + 2);
    match closers(&text[paragraph..]) {
        "" => Cow::Borrowed(text),
        // A closing marker after a space closes nothing in Markdown: it goes right after the word.
        closers => Cow::Owned(format!("{}{closers}", text.trim_end())),
    }
}

/// Whether `line` (the last, unfinished line) only starts a block so far: the start of a code
/// fence, or nothing but the marker of a heading, a list item, a quote or a rule.
fn opens_block(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with("```") || line.starts_with("~~~") {
        return true;
    }
    let marker = line.trim_end();
    if marker.is_empty() {
        return false;
    }
    let numbered = marker.trim_end_matches(['.', ')']);
    let all = |c: char| marker.chars().all(|m| m == c);
    (all('#') && marker.len() <= 6)
        || ['-', '*', '+', '_', '='].into_iter().any(all)
        || marker == ">"
        || (numbered.len() + 1 >= marker.len() && numbered.chars().all(|c| c.is_ascii_digit()))
}

/// Whether `text` ends inside a fenced code block: an odd number of fence lines.
fn in_open_fence(text: &str) -> bool {
    let fences = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("```") || line.starts_with("~~~")
        })
        .count();
    fences % 2 == 1
}

/// The markers that close what is open at the end of `paragraph`: inline code (`` ` ``), bold
/// (`**`), both (inline code first, it is inside), or nothing.
fn closers(paragraph: &str) -> &'static str {
    let bytes = paragraph.as_bytes();
    let (mut code, mut bold) = (false, false);
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            b'`' => code = !code,
            b'*' if !code && bytes.get(at + 1) == Some(&b'*') => {
                bold = !bold;
                at += 1;
            }
            _ => {}
        }
        at += 1;
    }
    match (code, bold) {
        (true, true) => "`**",
        (true, false) => "`",
        (false, true) => "**",
        (false, false) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame at 60 Hz.
    const FRAME: Duration = Duration::from_micros(16_667);

    /// The byte offsets at which `text` may be cut between graphemes.
    fn boundaries(text: &str) -> Vec<usize> {
        let mut at: Vec<usize> = text.grapheme_indices(true).map(|(at, _)| at).collect();
        at.push(text.len());
        at
    }

    fn graphemes(text: &str) -> usize {
        text.graphemes(true).count()
    }

    /// A text with letters that are more than one char: umlauts written with a combining mark, a
    /// family emoji, flags, a skin tone.
    fn mixed() -> String {
        let decomposed = "Gru\u{0308}\u{00DF}e an Mu\u{0308}ller";
        format!(
            "{decomposed} 👨‍👩‍👧‍👦 und 🇩🇪🇫🇷 – ok 👍🏽. **Fett** und `code`.\n\n- eins\n- zwei\n\n```rust\nfn main() {{}}\n```\nEnde."
        )
    }

    #[test]
    fn a_steady_model_gives_a_steady_pace() {
        // A model at 400 letters a second, followed every 50 ms: 20 letters a piece.
        let text: String = "lorem ipsum dolor sit amet ".repeat(40);
        let mut smoother = Smoother::new();
        let (mut came, mut time) = (0, Duration::ZERO);
        let mut per_frame = Vec::new();
        while came < text.len() {
            if time.as_millis() / 50 >= (came / 20) as u128 {
                came = (came + 20).min(text.len());
                smoother.set(&text[..came], false);
            }
            let before = graphemes(smoother.shown());
            smoother.advance(FRAME);
            per_frame.push(graphemes(smoother.shown()) - before);
            time += FRAME;
        }
        // After the first half second the pace has settled: every frame shows some letters, about
        // the model's 6.7, and the gaps between pieces never stop the text.
        let settled = &per_frame[30..];
        assert!(settled.iter().all(|n| (4..=11).contains(n)), "even: {settled:?}");
        let lag = graphemes(&text) - graphemes(smoother.shown());
        assert!(lag <= 200, "it keeps up with the model ({lag} behind)");
    }

    #[test]
    fn a_large_piece_is_caught_up_quickly() {
        let text = "word ".repeat(600);
        let mut smoother = Smoother::new();
        smoother.set(&text, false);
        let mut frames = 0;
        while graphemes(&text) - graphemes(smoother.shown()) > 20 {
            smoother.advance(FRAME);
            frames += 1;
            assert!(frames < 120, "3000 letters are caught up within two seconds");
        }
        assert!(frames > 10, "and still appear, not all at once");
    }

    #[test]
    fn a_complete_answer_ends_exactly_with_the_full_text() {
        let text = mixed();
        let cuts = boundaries(&text);
        let mut smoother = Smoother::new();
        // The pieces come cut anywhere between chars, even inside a grapheme.
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        for piece in chars.chunks(7) {
            let end = piece.last().map_or(0, |(at, c)| at + c.len_utf8());
            smoother.set(&text[..end], false);
            for _ in 0..2 {
                smoother.advance(FRAME);
                assert!(cuts.contains(&smoother.shown().len()), "whole graphemes only: {:?}", smoother.shown());
                assert!(text.starts_with(smoother.shown()));
            }
        }
        smoother.set(&text, true);
        let mut frames = 0;
        while smoother.revealing() {
            smoother.advance(FRAME);
            assert!(cuts.contains(&smoother.shown().len()));
            frames += 1;
            assert!(frames < 60, "the end comes within a second");
        }
        assert_eq!(smoother.shown(), text);
        assert!(smoother.done());
        assert!(!smoother.advance(FRAME), "nothing more to show");
    }

    #[test]
    fn the_last_grapheme_of_a_growing_text_waits_for_the_next_piece() {
        let mut smoother = Smoother::new();
        smoother.set("Mu", false);
        smoother.finish();
        assert_eq!(smoother.shown(), "M", "the u may still get its umlaut");
        smoother.set("Mu\u{0308}l", false);
        smoother.finish();
        assert_eq!(smoother.shown(), "Mu\u{0308}", "now it is whole");
        smoother.set("Mu\u{0308}l", true);
        assert!(smoother.revealing());
        smoother.finish();
        assert!(smoother.done());
    }

    #[test]
    fn letters_come_one_at_a_time_at_the_end_of_a_backlog() {
        let mut smoother = Smoother::new();
        smoother.set("Hi there", true);
        smoother.advance(FRAME);
        assert_eq!(graphemes(smoother.shown()), 1, "60 a second is one a frame");
        smoother.advance(FRAME);
        assert_eq!(smoother.shown(), "Hi");
    }

    #[test]
    fn a_stall_does_not_make_the_text_jump() {
        let text = "x".repeat(400);
        let mut smoother = Smoother::new();
        smoother.set(&text, false);
        smoother.advance(Duration::from_secs(3));
        assert!(graphemes(smoother.shown()) <= 100, "a long pause counts as one short step");
    }

    #[test]
    fn text_that_changed_goes_back_to_where_both_agree() {
        let mut smoother = Smoother::new();
        smoother.set("Hello world", true);
        smoother.finish();
        smoother.set("Hello there", false);
        assert_eq!(smoother.shown(), "Hello ");
        assert!(smoother.revealing());
        smoother.set("Hello there!", true);
        smoother.finish();
        assert_eq!(smoother.shown(), "Hello there!");
    }

    #[test]
    fn markdown_that_grows_does_not_jump() {
        // Open bold and inline code are closed for the drawing; the markers wait at the end.
        assert_eq!(display("Some **bo", false), "Some **bo**");
        assert_eq!(display("Some **bold ", false), "Some **bold**");
        assert_eq!(display("Some **", false), "Some ");
        assert_eq!(display("Run `cargo", false), "Run `cargo`");
        assert_eq!(display("**Run `cargo", false), "**Run `cargo`**");
        assert_eq!(display("**Done** and `x` too", false), "**Done** and `x` too");
        assert_eq!(display("not \\*\\* bold", false), "not \\*\\* bold");
        // Only the last paragraph counts.
        assert_eq!(display("**a\n\nb", false), "**a\n\nb");
        // Block markers wait for their first word.
        assert_eq!(display("Intro\n## ", false), "Intro\n");
        assert_eq!(display("Intro\n- ", false), "Intro\n");
        assert_eq!(display("Intro\n12.", false), "Intro\n");
        assert_eq!(display("Intro\n> ", false), "Intro\n");
        assert_eq!(display("Intro\n---", false), "Intro\n");
        assert_eq!(display("Intro\n- one", false), "Intro\n- one");
        assert_eq!(display("Intro\n12 apples", false), "Intro\n12 apples");
        // A fence waits for its line; inside an open fence nothing is closed.
        assert_eq!(display("Intro\n```ru", false), "Intro\n");
        assert_eq!(display("```rust\nlet s = **x", false), "```rust\nlet s = **x");
        assert_eq!(display("```rust\nfn main() {}\n```", false), "```rust\nfn main() {}\n");
        // Settled text is drawn as it is.
        assert_eq!(display("Some **", true), "Some **");
    }
}
