//! The SystemOne protocol of decision models (task E12.3, `docs/kategorien.md`): a model that
//! does not write but **decides**.
//!
//! One call carries a state and named questions and answers each with probabilities, the option
//! chosen and a confidence – no text, nothing to parse. `POST <base>/systemone`, the same base as
//! the OpenAI-compatible API (`http://localhost:11434/v1` → `…/v1/systemone`).
//!
//! Like [`super::openai`] this module is the wire format only: pure, without a network. The core
//! sends it (`torroworkplace::account::ai`).
//!
//! **The limits are the narrower of the two published sets.** Ollama documents up to 64 questions
//! and 2–26 options for a `choice`; the Jev reference (jevmodel.org) 1–8 questions, 2–20 options,
//! 2–10 levels and 8 000 characters of state. The narrower ones are kept ([`QUESTIONS_MAX`] …),
//! so the same call also works against a Jev server and later against `jeff-serve`, without being
//! rebuilt.
//!
//! Reading an answer is deliberately strict: a reply that answers none of the questions is
//! [`AiError::Format`], never a silent fallback. What a model says is data, never instructions.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::AiError;
use crate::i18n::{tr, trf};

/// The most questions one call may carry (the Jev reference; Ollama allows 64).
pub const QUESTIONS_MAX: usize = 8;

/// The most options a `choice` question may offer (the Jev reference; Ollama allows 26). The
/// catalogue of mail categories has exactly 20 options, so it fits.
pub const CHOICE_OPTIONS_MAX: usize = 20;

/// The fewest options a `choice` question must offer.
pub const CHOICE_OPTIONS_MIN: usize = 2;

/// The most levels a `score` question may have.
pub const SCORE_LEVELS_MAX: usize = 10;

/// The fewest levels a `score` question must have.
pub const SCORE_LEVELS_MIN: usize = 2;

/// The most characters of state one call may carry. Longer state is cut at a character boundary
/// by [`decide_body_for`]: a mail that is too long is still decided, just on its beginning.
pub const STATE_MAX: usize = 8_000;

/// The most characters a question's instructions may have.
pub const INSTRUCTIONS_MAX: usize = 1_800;

/// What is asked of a decision model in one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    /// The model's [`super::AiModel::id`].
    pub model: String,
    /// What the model decides about. Cut to [`STATE_MAX`] characters when it is sent.
    pub state: String,
    /// The questions by name; a name is letters, digits, `_`, `.` or `-`. At most
    /// [`QUESTIONS_MAX`].
    pub questions: BTreeMap<String, Question>,
}

/// One question of a [`DecisionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Pick one of the named options. `criteria` maps each option's key to what it means, in
    /// English: the model is prompted in English.
    Choice { instructions: String, criteria: Vec<(String, String)> },
    /// Yes or no. The answer is a **probability**, not a truth value ([`Answer::Noul`]).
    Noul { instructions: String },
    /// Where on an ordered scale, lowest level first. The answer is a **number**, which may fall
    /// between two levels ([`Answer::Score`]).
    Score { instructions: String, levels: Vec<String> },
}

impl Question {
    /// Why the question cannot be sent, as a sentence for people; `None` when it fits the limits.
    fn refused(&self, name: &str) -> Option<String> {
        // Two sentences, not one with the word slotted in: "options" and "levels" are words a
        // person reads, and a translation cannot inflect a word it never sees.
        let options = |n: usize| {
            (!(CHOICE_OPTIONS_MIN..=CHOICE_OPTIONS_MAX).contains(&n)).then(|| {
                trf(
                    "The question “{name}” has {n} options; it needs {least} to {most}.",
                    &[("name", &name), ("n", &n), ("least", &CHOICE_OPTIONS_MIN), ("most", &CHOICE_OPTIONS_MAX)],
                )
            })
        };
        let levels = |n: usize| {
            (!(SCORE_LEVELS_MIN..=SCORE_LEVELS_MAX).contains(&n)).then(|| {
                trf(
                    "The question “{name}” has {n} levels; it needs {least} to {most}.",
                    &[("name", &name), ("n", &n), ("least", &SCORE_LEVELS_MIN), ("most", &SCORE_LEVELS_MAX)],
                )
            })
        };
        let instructions = match self {
            Question::Choice { instructions, .. }
            | Question::Noul { instructions }
            | Question::Score { instructions, .. } => instructions,
        };
        if instructions.chars().count() > INSTRUCTIONS_MAX {
            return Some(trf(
                "The instructions of the question “{name}” are longer than {most} characters.",
                &[("name", &name), ("most", &INSTRUCTIONS_MAX)],
            ));
        }
        match self {
            Question::Choice { criteria, .. } => options(criteria.len()),
            Question::Score { levels: held, .. } => levels(held.len()),
            Question::Noul { .. } => None,
        }
    }

    /// The question as the protocol writes it.
    fn body(&self) -> Value {
        match self {
            Question::Choice { instructions, criteria } => {
                let criteria: serde_json::Map<String, Value> =
                    criteria.iter().map(|(key, means)| (key.clone(), Value::String(means.clone()))).collect();
                json!({ "type": "choice", "instructions": instructions, "criteria": criteria })
            }
            // Ollama writes the two sides of a yes/no question as the keys `false` and `true`.
            Question::Noul { instructions } => json!({
                "type": "noul",
                "instructions": instructions,
                "criteria": { "false": "No", "true": "Yes" },
            }),
            Question::Score { instructions, levels } => {
                json!({ "type": "score", "instructions": instructions, "criteria": levels })
            }
        }
    }
}

/// What one answer says.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// A `choice`: the option chosen, a probability for every option, and how sure the model is
    /// (0 to 1).
    Choice { choice: String, probabilities: BTreeMap<String, f32>, confidence: f32 },
    /// A `noul`: the **probability** that the answer is yes, 0 to 1. Not a truth value – the
    /// caller decides where it turns into one.
    Noul(f32),
    /// A `score`: where on the scale, as the index of a level, possibly between two of them, with
    /// how sure the model is.
    Score { score: f32, confidence: f32 },
}

/// What a decision model answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DecisionReply {
    /// The answers by the name of their question; a question the model left out is missing.
    pub answers: BTreeMap<String, Answer>,
    /// What the server counted, when it says.
    pub usage: Option<super::Usage>,
}

impl DecisionReply {
    /// The `choice` answered under `name`; `None` when the model left it out or answered another
    /// kind.
    pub fn choice(&self, name: &str) -> Option<(&str, f32)> {
        match self.answers.get(name) {
            Some(Answer::Choice { choice, confidence, .. }) => Some((choice.as_str(), *confidence)),
            _ => None,
        }
    }

    /// The probability of the `noul` answered under `name`.
    pub fn noul(&self, name: &str) -> Option<f32> {
        match self.answers.get(name) {
            Some(Answer::Noul(probability)) => Some(*probability),
            _ => None,
        }
    }

    /// The number of the `score` answered under `name`.
    pub fn score(&self, name: &str) -> Option<f32> {
        match self.answers.get(name) {
            Some(Answer::Score { score, .. }) => Some(*score),
            _ => None,
        }
    }
}

/// The body of `POST <base>/systemone` for `request`, or why it cannot be sent
/// ([`AiError::Invalid`] with a sentence for people). The state is cut to [`STATE_MAX`]
/// characters at a character boundary; everything else must fit the limits.
pub fn decide_body_for(request: &DecisionRequest) -> Result<Value, AiError> {
    if request.questions.is_empty() || request.questions.len() > QUESTIONS_MAX {
        return Err(AiError::Invalid(trf(
            "A decision takes 1 to {most} questions, not {n}.",
            &[("most", &QUESTIONS_MAX), ("n", &request.questions.len())],
        )));
    }
    for (name, question) in &request.questions {
        if let Some(refused) = question.refused(name) {
            return Err(AiError::Invalid(refused));
        }
    }
    let questions: serde_json::Map<String, Value> =
        request.questions.iter().map(|(name, question)| (name.clone(), question.body())).collect();
    Ok(json!({ "model": request.model, "state": cut(&request.state, STATE_MAX), "questions": questions }))
}

/// `text` cut to at most `most` characters, at a character boundary.
fn cut(text: &str, most: usize) -> &str {
    match text.char_indices().nth(most) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

/// The answers in a server's reply, or why it cannot be read. A reply that answers nothing is
/// [`AiError::Format`]: a decision nobody can read must not pass as an empty one.
pub fn read_decision(body: &[u8]) -> Result<DecisionReply, AiError> {
    let unreadable =
        || AiError::Format(tr("The AI server's answer is no decision. Try again or pick another model.").to_string());
    let value: Value = serde_json::from_slice(body).map_err(|_| unreadable())?;
    let answers = value.get("answers").and_then(Value::as_object).ok_or_else(unreadable)?;
    let mut read = BTreeMap::new();
    for (name, answer) in answers {
        if let Some(answer) = answer_of(answer) {
            read.insert(name.clone(), answer);
        }
    }
    if read.is_empty() {
        return Err(unreadable());
    }
    Ok(DecisionReply { answers: read, usage: usage_of(&value) })
}

/// One answer, by what its `type` says; `None` for one this version cannot read, so a reply with
/// several answers is not lost because of one of them.
fn answer_of(answer: &Value) -> Option<Answer> {
    let number = |name: &str| answer.get(name).and_then(Value::as_f64).map(|value| value as f32);
    match answer.get("type").and_then(Value::as_str)? {
        "choice" => {
            let choice = answer.get("choice").and_then(Value::as_str)?.to_string();
            let probabilities = answer
                .get("probabilities")
                .and_then(Value::as_object)
                .map(|held| {
                    held.iter()
                        .filter_map(|(key, value)| Some((key.clone(), value.as_f64()? as f32)))
                        .collect::<BTreeMap<String, f32>>()
                })
                .unwrap_or_default();
            // Without a confidence of its own the chosen option's probability says how sure it is.
            let confidence = number("confidence").or_else(|| probabilities.get(&choice).copied())?;
            Some(Answer::Choice { choice, probabilities, confidence })
        }
        "noul" => Some(Answer::Noul(number("noul")?)),
        "score" => Some(Answer::Score { score: number("score")?, confidence: number("confidence").unwrap_or(0.0) }),
        _ => None,
    }
}

/// What the server counted, when it says (`input_tokens`, `output_tokens`).
fn usage_of(value: &Value) -> Option<super::Usage> {
    let usage = value.get("usage")?;
    let count = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    Some(super::Usage { prompt_tokens: count("input_tokens"), completion_tokens: count("output_tokens") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice() -> Question {
        Question::Choice {
            instructions: "Which queue should own this ticket?".into(),
            criteria: vec![
                ("billing".into(), "Payments, refunds, invoices".into()),
                ("technical".into(), "Product errors and usage problems".into()),
            ],
        }
    }

    fn request(questions: Vec<(&str, Question)>) -> DecisionRequest {
        DecisionRequest {
            model: "nimble".into(),
            state: "The customer has been billed twice.".into(),
            questions: questions.into_iter().map(|(name, question)| (name.to_string(), question)).collect(),
        }
    }

    #[test]
    fn a_call_is_written_as_the_protocol_says() {
        let asked = request(vec![
            ("route", choice()),
            ("escalate", Question::Noul { instructions: "Does this need a human?".into() }),
            (
                "urgency",
                Question::Score {
                    instructions: "How urgent is this?".into(),
                    levels: vec!["routine".into(), "soon".into(), "urgent".into()],
                },
            ),
        ]);
        let body = decide_body_for(&asked).expect("a body");
        assert_eq!(body["model"], "nimble");
        assert_eq!(body["state"], "The customer has been billed twice.");
        assert_eq!(body["questions"]["route"]["type"], "choice");
        assert_eq!(body["questions"]["route"]["criteria"]["billing"], "Payments, refunds, invoices");
        assert_eq!(body["questions"]["escalate"]["type"], "noul");
        assert_eq!(body["questions"]["escalate"]["criteria"]["true"], "Yes");
        assert_eq!(body["questions"]["urgency"]["criteria"], json!(["routine", "soon", "urgent"]));
    }

    #[test]
    fn a_state_that_is_too_long_is_cut_and_the_other_limits_are_refused() {
        let long = request(vec![("route", choice())]);
        let long = DecisionRequest { state: "ä".repeat(STATE_MAX + 500), ..long };
        let body = decide_body_for(&long).expect("a body");
        assert_eq!(body["state"].as_str().expect("text").chars().count(), STATE_MAX, "cut, not refused");

        let too_many = (0..CHOICE_OPTIONS_MAX + 1)
            .map(|n| (format!("o{n}"), "something".to_string()))
            .collect::<Vec<(String, String)>>();
        let wide = request(vec![("route", Question::Choice { instructions: "Pick".into(), criteria: too_many })]);
        assert!(matches!(decide_body_for(&wide), Err(AiError::Invalid(_))), "21 options");

        let one = request(vec![(
            "route",
            Question::Choice { instructions: "Pick".into(), criteria: vec![("only".into(), "one".into())] },
        )]);
        assert!(matches!(decide_body_for(&one), Err(AiError::Invalid(_))), "one option");

        let questions: Vec<(&str, Question)> =
            ["a", "b", "c", "d", "e", "f", "g", "h", "i"].into_iter().map(|name| (name, choice())).collect();
        assert!(matches!(decide_body_for(&request(questions)), Err(AiError::Invalid(_))), "nine questions");
        assert!(matches!(decide_body_for(&request(Vec::new())), Err(AiError::Invalid(_))), "no question");

        let wordy = request(vec![(
            "route",
            Question::Choice { instructions: "x".repeat(INSTRUCTIONS_MAX + 1), criteria: choice_criteria() },
        )]);
        assert!(matches!(decide_body_for(&wordy), Err(AiError::Invalid(_))), "long instructions");
    }

    fn choice_criteria() -> Vec<(String, String)> {
        match choice() {
            Question::Choice { criteria, .. } => criteria,
            _ => unreachable!(),
        }
    }

    #[test]
    fn an_answer_of_each_kind_is_read() {
        let body = br#"{
            "model": "nimble",
            "answers": {
                "route": {"type": "choice", "choice": "billing",
                          "probabilities": {"billing": 0.93, "technical": 0.07}, "confidence": 0.93},
                "escalate": {"type": "noul", "noul": 0.12},
                "urgency": {"type": "score", "score": 1.4, "confidence": 0.88}
            },
            "usage": {"input_tokens": 136, "output_tokens": 18}
        }"#;
        let reply = read_decision(body).expect("a decision");
        let (choice, confidence) = reply.choice("route").expect("a choice");
        assert_eq!(choice, "billing");
        assert!((confidence - 0.93).abs() < 0.001);
        assert!((reply.noul("escalate").expect("a noul") - 0.12).abs() < 0.001);
        assert!((reply.score("urgency").expect("a score") - 1.4).abs() < 0.001);
        assert_eq!(reply.usage.expect("counted").prompt_tokens, 136);
        assert_eq!(reply.choice("escalate"), None, "a noul is no choice");
        assert_eq!(reply.score("nope"), None);
    }

    #[test]
    fn a_choice_without_a_confidence_falls_back_to_its_probability() {
        let body = br#"{"answers": {"c": {"type": "choice", "choice": "a", "probabilities": {"a": 0.8, "b": 0.2}}}}"#;
        let reply = read_decision(body).expect("a decision");
        let (choice, confidence) = reply.choice("c").expect("a choice");
        assert_eq!(choice, "a");
        assert!((confidence - 0.8).abs() < 0.001);
    }

    #[test]
    fn an_answer_that_is_no_decision_is_refused() {
        for wrong in [
            &b"not json"[..],
            br#"{"model": "nimble"}"#,
            br#"{"answers": {}}"#,
            br#"{"answers": {"c": {"type": "nonsense"}}}"#,
            br#"{"answers": {"c": {"type": "choice", "probabilities": {"a": 1.0}}}}"#,
        ] {
            assert!(matches!(read_decision(wrong), Err(AiError::Format(_))), "{:?}", String::from_utf8_lossy(wrong));
        }
        // One answer this version cannot read does not lose the others.
        let mixed = br#"{"answers": {"a": {"type": "nonsense"}, "b": {"type": "noul", "noul": 0.9}}}"#;
        let reply = read_decision(mixed).expect("a decision");
        assert_eq!(reply.answers.len(), 1);
        assert!(reply.noul("b").is_some());
    }
}
