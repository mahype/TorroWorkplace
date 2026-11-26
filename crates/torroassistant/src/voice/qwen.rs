//! Qwen3-TTS (issue #764, `docs/messung-qwen3-tts.md`): the 0.6B Base model on an NVIDIA graphics card,
//! without Python. Since issue #808 a program of its own computes it, `torro-qwen-tts`
//! (`programs/torro-qwen-tts`, a workspace of its own, so Crane never touches the versions of the suite);
//! [`Qwen`] starts it as a child process and talks to it over JSON lines. It clones the voice templates
//! of the app ([`TEMPLATES`], "Warm woman" and "Calm man") with their transcripts, piece by piece, so the
//! first sound comes once the first piece is computed. Only builds with the feature `qwen-cuda` read with
//! it; everything here is in every build, so it is tested without a graphics card.
//!
//! [`Qwen`] runs behind the same [`Synth`] as Pocket TTS, on a thread of its own in the reading helper
//! ([`super::speech`]). Its program holds about 8 GB of graphics memory while it runs; the helper drops
//! [`Qwen`], and with it the program, when another provider reads.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};
use torro_core::settings::{QWEN_RECOMMENDED, QWEN_VOICES};
use torro_core::voice_models::QWEN;
use torro_ipc::{VoiceMissing, VoicePart};

use super::VoiceError;
use super::pocket::TEMPLATES;
use super::pocket::{Chunk, Delivery, Synth};
use super::speech::pieces;

/// The name Qwen3-TTS goes by in messages.
const NAME: &str = "Qwen3-TTS";

/// The program that computes Qwen3-TTS (issue #808), without the suffix of the system.
pub const PROGRAM: &str = "torro-qwen-tts";

/// The longest piece computed at once, in characters: as for Pocket TTS, a sentence or two, so the
/// first sound comes after about two seconds (measured 2.5 s for 184 characters) and a stop takes
/// effect between two pieces at the latest.
pub const PIECE: usize = 200;

/// What the templates say, for cloning in context: Qwen3-TTS hears how the words sound in the
/// voice. Recorded for issue #745.
pub const TRANSCRIPTS: [(&str, &str); 2] = [
    (
        "warm_woman",
        "Hallo, schön, dass du da bist. Ich lese dir deine Antworten vor, ruhig und deutlich, damit du ganz \
         entspannt zuhören kannst.",
    ),
    (
        "calm_man",
        "Guten Tag. Ich bin dein Assistent und lese dir die Antworten vor, in aller Ruhe, deutlich und gut \
         verständlich.",
    ),
];

/// The template and its transcript for the voice `voice`; one that is no Qwen voice reads as
/// [`QWEN_RECOMMENDED`].
pub fn template(voice: &str) -> Option<(&'static [u8], &'static str)> {
    let voice = if QWEN_VOICES.contains(&voice) { voice } else { QWEN_RECOMMENDED };
    let bytes = TEMPLATES.iter().find(|(id, _)| *id == voice).map(|(_, bytes)| *bytes)?;
    let words = TRANSCRIPTS.iter().find(|(id, _)| *id == voice).map(|(_, words)| *words)?;
    Some((bytes, words))
}

/// The language as Qwen3-TTS names it, for the language code `code` of the suite (`"de"`, `"en"`);
/// `"auto"` for one it does not know.
pub fn language(code: &str) -> &'static str {
    match code {
        "de" => "german",
        "en" => "english",
        "fr" => "french",
        "es" => "spanish",
        "it" => "italian",
        "pt" => "portuguese",
        "ru" => "russian",
        "ja" => "japanese",
        "ko" => "korean",
        "zh" => "chinese",
        _ => "auto",
    }
}

/// The most frames of sound computed for a piece of `chars` characters: twice what is spoken at a
/// calm pace (12 frames a second, about 15 characters a second), so a piece the model does not end
/// by itself (it happens, `docs/messung-qwen3-tts.md`) stops after seconds, not minutes.
pub fn most_frames(chars: usize) -> usize {
    48 + chars.saturating_mul(2)
}

/// How quiet the start of a piece may be and still count as silence (about -40 dB).
const QUIET: f32 = 0.01;

/// How much of the silence before the first word is kept, in seconds: the word does not start
/// abruptly.
const LEAD: f32 = 0.05;

/// `samples` at `rate` without the silence before the first word but [`LEAD`]: Qwen3-TTS often starts
/// a piece with half a second of it, once with eight (`docs/messung-qwen3-tts.md`). Sound that is all
/// silence stays as it is.
pub fn without_lead(samples: &[f32], rate: u32) -> &[f32] {
    let Some(first) = samples.iter().position(|sample| sample.abs() > QUIET) else { return samples };
    // The lead is a few thousand samples at most, far below what loses precision in an `f32`.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let lead = (rate as f32 * LEAD) as usize;
    &samples[first.saturating_sub(lead)..]
}

/// Crane reads the speech tokenizer from `<model>/speech_tokenizer/`; the catalogue keeps its files flat
/// as `speech_tokenizer.<name>` (`torro_core::voice_models::QWEN`). Links each into place, as a hard
/// link (no copy of 680 MB, and it works on every system), or copies it where the file system has none
/// (FAT, some network shares); one that is there with the size of its file is kept. The folder of the
/// model in the voice folder `folder`.
pub fn place_tokenizer(folder: &Path) -> Result<PathBuf, VoiceError> {
    let model = torro_core::voice_models::model(QWEN)
        .ok_or_else(|| VoiceError::program("Qwen3-TTS", "this build has no Qwen3-TTS"))?;
    let home = model.folder(folder);
    let inner = home.join("speech_tokenizer");
    let failed = |err: std::io::Error| VoiceError::program("Qwen3-TTS", format!("{}: {err}", inner.display()));
    std::fs::create_dir_all(&inner).map_err(failed)?;
    for file in model.files {
        let Some(name) = file.name.strip_prefix("speech_tokenizer.") else { continue };
        let (from, to) = (home.join(file.name), inner.join(name));
        if std::fs::metadata(&to).is_ok_and(|meta| meta.len() == file.bytes) {
            continue;
        }
        // A link of a file downloaded again (or cut short) is replaced.
        let _ = std::fs::remove_file(&to);
        std::fs::hard_link(&from, &to).or_else(|_| std::fs::copy(&from, &to).map(|_| ())).map_err(failed)?;
    }
    Ok(home)
}

/// Where `torro-qwen-tts` is: beside the assistant's program, where `docs/install.md` puts it, else
/// looked for on `PATH`.
pub fn program() -> PathBuf {
    let file = format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&file)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(file))
}

/// The NVIDIA card Qwen3-TTS computes on, as the settings show it (`NVIDIA GeForce RTX 4090, 24 GB`),
/// asked of `torro-qwen-tts --card`; `None` when there is none, its libraries do not answer or the
/// program is missing.
#[cfg_attr(all(not(feature = "qwen-cuda"), test), expect(dead_code, reason = "it asks a real program"))]
pub fn graphics_card() -> Option<String> {
    let program = program();
    let mut child = Command::new(&program)
        .arg("--card")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| eprintln!("torroassistant: {NAME}: {} did not start: {err}", program.display()))
        .ok()?;
    // The status waits for this answer: a program that hangs (a driver that does not answer) counts as
    // no card after a while instead of holding the settings. Its one short line fits in the pipe.
    let started = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > CARD_WAIT {
            eprintln!("torroassistant: {NAME}: {} --card did not answer", program.display());
            end(&mut child);
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut answer = String::new();
    child.stdout.take()?.read_to_string(&mut answer).ok()?;
    card_of(&answer)
}

/// How long `torro-qwen-tts --card` may take: starting CUDA takes a second or two.
const CARD_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// The card in the answer of `torro-qwen-tts --card`.
pub fn card_of(answer: &str) -> Option<String> {
    let line = answer.lines().next()?;
    serde_json::from_str::<Value>(line).ok()?.get("card")?.as_str().map(str::to_string)
}

/// Qwen3-TTS loaded once on the graphics card by its program, with the templates it was given.
pub struct Qwen {
    /// The program; `None` in the tests, which answer for it.
    child: Option<Child>,
    /// The lines to the program.
    input: Box<dyn Write>,
    /// The lines from the program.
    output: Box<dyn BufRead>,
    /// The rate of the sound it makes.
    rate: u32,
    /// The rate the templates must have.
    speaker_rate: u32,
    language: &'static str,
    /// The voices whose templates the program has.
    voices: HashSet<&'static str>,
    /// The voice folder and language it was started with, to start it again; `None` in the tests.
    again: Option<(PathBuf, String)>,
    /// The program ended or no longer reads or writes lines: the next job starts it again.
    broken: bool,
}

impl Qwen {
    /// Starts `torro-qwen-tts` ([`program`]) with the model from the voice folder `folder` on the first
    /// NVIDIA card, speaking the language of the suite `code` ([`language`]). The model must be
    /// downloaded (the reading helper looks first); without a card it is [`VoiceError::Missing`] with
    /// [`VoiceMissing::Gpu`], never a crash. Its messages go to the helper's error output.
    pub fn load(folder: &Path, code: &str) -> Result<Qwen, VoiceError> {
        let home = place_tokenizer(folder)?;
        let program = program();
        let mut child = Command::new(&program)
            .arg(&home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| VoiceError::program(NAME, format!("{} did not start: {err}", program.display())))?;
        let started = match (child.stdin.take(), child.stdout.take()) {
            (Some(input), Some(output)) => Qwen::over(Box::new(input), Box::new(BufReader::new(output)), code),
            _ => Err(VoiceError::program(NAME, "no pipes to its program")),
        };
        match started {
            Ok(mut qwen) => {
                qwen.child = Some(child);
                qwen.again = Some((folder.to_path_buf(), code.to_string()));
                Ok(qwen)
            }
            Err(err) => {
                end(&mut child);
                Err(err)
            }
        }
    }

    /// Qwen3-TTS answering on `output` what is written to `input`, once it said it is ready.
    fn over(input: Box<dyn Write>, mut output: Box<dyn BufRead>, code: &str) -> Result<Qwen, VoiceError> {
        let first = read_line(output.as_mut())?;
        if let Some(error) = first.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
            if error.get("gpu").and_then(Value::as_bool) == Some(true) {
                eprintln!("torroassistant: {NAME}: no graphics card: {message}");
                return Err(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Gpu));
            }
            return Err(VoiceError::program(NAME, message));
        }
        let ready = first.get("ready").ok_or_else(|| VoiceError::program(NAME, "its program is not ready"))?;
        let rate_of = |key: &str| {
            ready
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|rate| u32::try_from(rate).ok())
                .filter(|rate| *rate > 0)
                .ok_or_else(|| VoiceError::program(NAME, format!("its program names no `{key}`")))
        };
        Ok(Qwen {
            child: None,
            input,
            output,
            rate: rate_of("rate")?,
            speaker_rate: rate_of("speaker_rate")?,
            language: language(code),
            voices: HashSet::new(),
            again: None,
            broken: false,
        })
    }

    /// Writes `request` to the program and reads its answer; one that says what failed is an error.
    fn ask(&mut self, request: &Value) -> Result<Value, VoiceError> {
        let gone = |err: std::io::Error| VoiceError::program(NAME, format!("its program does not read: {err}"));
        let answer = writeln!(self.input, "{request}")
            .and_then(|()| self.input.flush())
            .map_err(gone)
            .and_then(|()| read_line(self.output.as_mut()))
            .inspect_err(|_| self.broken = true)?;
        match answer.get("error") {
            Some(error) => Err(VoiceError::program(NAME, error.as_str().unwrap_or("failed"))),
            None => Ok(answer),
        }
    }

    /// Starts the program again after it broke (it crashed, or was ended from outside): the old one is
    /// ended first, so its graphics memory is free. Without a program to start (the tests) it stays broken.
    fn start_again(&mut self) -> Result<(), VoiceError> {
        let Some((folder, code)) = self.again.clone() else {
            return Err(VoiceError::program(NAME, "its program ended"));
        };
        if let Some(child) = self.child.as_mut() {
            end(child);
        }
        self.child = None;
        *self = Qwen::load(&folder, &code)?;
        Ok(())
    }

    /// The id of `voice` ([`template`]), its template given to the program once.
    fn voice(&mut self, voice: &str) -> Result<&'static str, VoiceError> {
        let id = QWEN_VOICES.iter().copied().find(|id| *id == voice).unwrap_or(QWEN_RECOMMENDED);
        if self.voices.contains(id) {
            return Ok(id);
        }
        let (bytes, words) = template(id).ok_or_else(|| VoiceError::program(NAME, "no voice template"))?;
        let wave = super::audio::decode_wav(bytes)?;
        if wave.rate != self.speaker_rate {
            return Err(VoiceError::program(NAME, format!("the template is at {} Hz", wave.rate)));
        }
        self.ask(&json!({ "voice": { "id": id, "words": words, "samples": wave.samples } }))?;
        self.voices.insert(id);
        Ok(id)
    }
}

/// The next line of the program, read as JSON.
fn read_line(output: &mut dyn BufRead) -> Result<Value, VoiceError> {
    let mut line = String::new();
    let read = output.read_line(&mut line).map_err(|err| VoiceError::program(NAME, format!("its program: {err}")))?;
    if read == 0 {
        return Err(VoiceError::program(NAME, "its program ended"));
    }
    serde_json::from_str(&line).map_err(|err| VoiceError::program(NAME, format!("its program wrote no JSON: {err}")))
}

/// Ends the program at once, and waits until it has: its graphics memory is free afterwards.
fn end(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for Qwen {
    // The thread of Qwen ends once the model is off the card (`speech::start_worker`), so the program
    // ends with it; a piece still being computed is lost, as the next job would not hear it anyway.
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            end(child);
        }
    }
}

impl Synth for Qwen {
    fn rate(&self) -> u32 {
        self.rate
    }

    // Speed and pause are Pocket's (issue #782): Qwen3-TTS speaks at its own pace.
    fn speak(&mut self, text: &str, voice: &str, _delivery: Delivery, mut chunk: Chunk) -> Result<(), VoiceError> {
        if self.broken {
            self.start_again()?;
        }
        let voice = self.voice(voice)?;
        for piece in pieces(text, PIECE) {
            let most = most_frames(piece.chars().count());
            let request =
                json!({ "speak": { "text": piece, "language": self.language, "voice": voice, "most": most } });
            let answer = self.ask(&request)?;
            let samples: Vec<f32> = answer
                .get("sound")
                .and_then(Value::as_array)
                .ok_or_else(|| VoiceError::program(NAME, "made no sound"))?
                .iter()
                // Sound is in [-1, 1], written from an f32; a sample that is no number is silence.
                .map(|sample| sample.as_f64().unwrap_or_default() as f32)
                .collect();
            // A stop is heard between two pieces: computing one cannot be interrupted.
            if !chunk(without_lead(&samples, self.rate)) {
                return Ok(());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #764: each Qwen voice has its template and transcript; another id reads as the recommended.
    #[test]
    fn every_voice_has_its_template_and_words() {
        for voice in QWEN_VOICES {
            let (bytes, words) = template(voice).expect("a template");
            assert_eq!(&bytes[..4], b"RIFF", "{voice}");
            assert!(words.ends_with('.'), "{voice}");
        }
        assert_eq!(template("alba"), template(QWEN_RECOMMENDED), "a Pocket voice is no Qwen voice");
        assert!(template("warm_woman").is_some_and(|(_, words)| words.starts_with("Hallo")));
        assert!(template("calm_man").is_some_and(|(_, words)| words.starts_with("Guten Tag")));
    }

    #[test]
    fn languages_and_frames() {
        assert_eq!(
            (language("de"), language("en"), language("auto"), language("xx")),
            ("german", "english", "auto", "auto")
        );
        assert_eq!(most_frames(0), 48);
        // 200 characters are spoken in about 13 seconds, 160 frames: the most is well above.
        assert!(most_frames(PIECE) > 2 * 160);
    }

    /// Issue #764: the silence before the first word goes but 50 ms; sound without any stays whole.
    #[test]
    fn the_silence_before_the_first_word_is_cut() {
        let mut samples = vec![0.0; 24_000];
        samples.extend([0.5, -0.5, 0.2]);
        let cut = without_lead(&samples, 24_000);
        assert_eq!(cut.len(), 1_200 + 3, "50 ms at 24 kHz before the first sound");
        assert_eq!(cut[1_200], 0.5);
        let quiet = vec![0.001; 10];
        assert_eq!(without_lead(&quiet, 24_000), &quiet[..], "all silence stays");
        assert_eq!(without_lead(&[0.3, 0.0], 24_000), &[0.3, 0.0], "nothing before the first word");
        assert!(without_lead(&[], 24_000).is_empty());
    }

    /// Issue #764: the tokenizer's flat files are linked where Crane looks, once; with the feature only,
    /// since the catalogue lists the model only then.
    #[cfg(feature = "qwen-cuda")]
    #[test]
    fn the_tokenizer_is_linked_into_its_folder() {
        let dir = tempfile::tempdir().expect("voice folder");
        let model = torro_core::voice_models::model(QWEN).expect("in the catalogue");
        std::fs::create_dir_all(model.folder(dir.path())).expect("folder");
        for file in model.files.iter().filter(|file| file.name.starts_with("speech_tokenizer.")) {
            let made = std::fs::File::create(model.path(dir.path(), file.name)).expect("file");
            made.set_len(file.bytes).expect("size");
        }
        let home = place_tokenizer(dir.path()).expect("linked");
        for name in ["config.json", "model.safetensors"] {
            assert!(home.join("speech_tokenizer").join(name).is_file(), "{name}");
        }
        place_tokenizer(dir.path()).expect("again, kept");
    }

    /// Issue #764, by hand on a computer with an NVIDIA card: starts `torro-qwen-tts` (issue #808, built
    /// as in `docs/install.md` and on `PATH`) with the downloaded model from the voice folder
    /// `TORRO_QWEN_VOICE` and computes a sentence in each voice, without playing it. Ignored, since tests
    /// run without a graphics card and a 2.5 GB model:
    /// `TORRO_QWEN_VOICE=<voice folder> cargo test -p torroassistant --features qwen-cuda -- --ignored real_model`.
    #[cfg(feature = "qwen-cuda")]
    #[test]
    #[ignore = "needs an NVIDIA card and the downloaded model"]
    fn reads_with_the_real_model() {
        let folder = PathBuf::from(std::env::var("TORRO_QWEN_VOICE").expect("TORRO_QWEN_VOICE"));
        assert!(graphics_card().is_some(), "an NVIDIA card");
        let mut qwen = Qwen::load(&folder, "de").expect("loaded");
        for voice in QWEN_VOICES {
            let sound = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let heard = std::rc::Rc::clone(&sound);
            let chunk: Chunk = Box::new(move |samples| {
                heard.borrow_mut().extend_from_slice(samples);
                true
            });
            qwen.speak("Gern, ich habe den Termin für morgen eingetragen.", voice, Delivery::default(), chunk)
                .expect("spoken");
            let seconds = sound.borrow().len() as f32 / qwen.rate() as f32;
            assert!((1.5..12.0).contains(&seconds), "{voice}: {seconds} s");
            let first = sound.borrow().iter().position(|sample| sample.abs() > QUIET).unwrap_or(usize::MAX);
            assert!(first as f32 <= qwen.rate() as f32 * LEAD + 1.0, "{voice}: the lead is cut");
        }
    }

    /// The lines written to the program, for the tests that answer for it.
    #[derive(Clone, Default)]
    struct Written(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

    impl Write for Written {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Written {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8_lossy(&self.0.borrow())
                .lines()
                .map(|line| serde_json::from_str(line).expect("a JSON line"))
                .collect()
        }
    }

    /// Qwen with a program that answers `answers`, one line each, and what was written to it.
    fn answering(answers: &[Value]) -> (Result<Qwen, VoiceError>, Written) {
        let text: String = answers.iter().map(|answer| format!("{answer}\n")).collect();
        let written = Written::default();
        let output = Box::new(std::io::Cursor::new(text.into_bytes()));
        (Qwen::over(Box::new(written.clone()), output, "de"), written)
    }

    /// Issue #808: the template goes to the program once, at its rate, each piece is asked for with the
    /// language and the most frames, and the silence before the first word is cut from what comes back.
    #[test]
    fn talks_to_its_program_in_json_lines() {
        let rate = super::super::audio::decode_wav(TEMPLATES[0].1).expect("a template").rate;
        let mut sound = vec![0.0; 24_000];
        sound.push(0.5);
        let answers = [
            json!({ "ready": { "rate": 24_000, "speaker_rate": rate } }),
            json!({ "done": true }),
            json!({ "sound": sound }),
            json!({ "sound": [0.25] }),
        ];
        let (qwen, written) = answering(&answers);
        let mut qwen = qwen.expect("ready");
        assert_eq!(qwen.rate(), 24_000);
        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let into = std::rc::Rc::clone(&heard);
        let chunk: Chunk = Box::new(move |samples| {
            into.borrow_mut().push(samples.len());
            true
        });
        qwen.speak("Hallo.", "calm_man", Delivery::default(), chunk).expect("spoken");
        let chunk: Chunk = Box::new(|_| true);
        qwen.speak("Noch einmal.", "calm_man", Delivery::default(), chunk).expect("spoken again");
        let lines = written.lines();
        assert_eq!(lines.len(), 3, "the template once, then one piece each: {lines:?}");
        assert_eq!(lines[0]["voice"]["id"], "calm_man");
        assert!(lines[0]["voice"]["words"].as_str().is_some_and(|words| words.starts_with("Guten Tag")));
        assert!(lines[0]["voice"]["samples"].as_array().is_some_and(|samples| samples.len() > rate as usize));
        assert_eq!(
            lines[1],
            json!({ "speak": { "text": "Hallo.", "language": "german", "voice": "calm_man", "most": most_frames(6) } })
        );
        assert_eq!(lines[2]["speak"]["text"], "Noch einmal.");
        assert_eq!(*heard.borrow(), vec![1_200 + 1], "50 ms before the first sound");
    }

    /// Issue #808: a program without a card is the missing graphics card, as before; one that fails or ends
    /// says so, and an answer with an error fails the job.
    #[test]
    fn says_what_its_program_lacks() {
        let (no_card, _) = answering(&[json!({ "error": { "gpu": true, "message": "no CUDA" } })]);
        assert!(matches!(no_card, Err(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Gpu))));
        let (failed, _) = answering(&[json!({ "error": { "gpu": false, "message": "did not load: x" } })]);
        assert!(matches!(failed, Err(VoiceError::Program { message, .. }) if message == "did not load: x"));
        let (ended, _) = answering(&[]);
        assert!(matches!(ended, Err(VoiceError::Program { message, .. }) if message == "its program ended"));
        let (no_rate, _) = answering(&[json!({ "ready": { "rate": 24_000 } })]);
        assert!(matches!(no_rate, Err(VoiceError::Program { .. })));

        let rate = super::super::audio::decode_wav(TEMPLATES[0].1).expect("a template").rate;
        let ready = json!({ "ready": { "rate": 24_000, "speaker_rate": rate } });
        let (qwen, _) = answering(&[ready.clone(), json!({ "done": true }), json!({ "error": "made no sound: x" })]);
        let failed = qwen.expect("ready").speak("Hallo.", "warm_woman", Delivery::default(), Box::new(|_| true));
        assert!(matches!(failed, Err(VoiceError::Program { message, .. }) if message == "made no sound: x"));
        // A template at another rate than the speaker encoder's is never sent.
        let (qwen, written) = answering(&[json!({ "ready": { "rate": 24_000, "speaker_rate": rate + 1 } })]);
        let wrong = qwen.expect("ready").speak("Hallo.", "warm_woman", Delivery::default(), Box::new(|_| true));
        assert!(matches!(wrong, Err(VoiceError::Program { .. })));
        assert!(written.lines().is_empty());
    }

    /// Review of #808: a program that ended is not asked again; the next job starts it anew (here, where
    /// none can be started, it fails at once without writing).
    #[test]
    fn a_program_that_ended_is_started_again() {
        let rate = super::super::audio::decode_wav(TEMPLATES[0].1).expect("a template").rate;
        let (qwen, written) = answering(&[json!({ "ready": { "rate": 24_000, "speaker_rate": rate } })]);
        let mut qwen = qwen.expect("ready");
        let ended = qwen.speak("Hallo.", "warm_woman", Delivery::default(), Box::new(|_| true));
        assert!(matches!(ended, Err(VoiceError::Program { message, .. }) if message == "its program ended"));
        assert!(qwen.broken, "it ended while asked");
        let again = qwen.speak("Hallo.", "warm_woman", Delivery::default(), Box::new(|_| true));
        assert!(matches!(again, Err(VoiceError::Program { .. })));
        assert_eq!(written.lines().len(), 1, "only the first job wrote to it");
        // An answer with an error is no broken program.
        let ready = json!({ "ready": { "rate": 24_000, "speaker_rate": rate } });
        let (qwen, _) = answering(&[ready, json!({ "error": "x" })]);
        let mut qwen = qwen.expect("ready");
        assert!(qwen.speak("Hallo.", "warm_woman", Delivery::default(), Box::new(|_| true)).is_err());
        assert!(!qwen.broken);
    }

    #[test]
    fn reads_the_card_of_its_program() {
        assert_eq!(
            card_of("{\"card\": \"NVIDIA GeForce RTX 4090, 24 GB\"}\n").as_deref(),
            Some("NVIDIA GeForce RTX 4090, 24 GB")
        );
        assert_eq!(card_of("{\"card\": null}\n"), None);
        assert_eq!(card_of(""), None);
        assert_eq!(card_of("Segmentation fault"), None);
    }

    #[cfg(not(feature = "qwen-cuda"))]
    #[test]
    fn without_the_feature_nothing_is_linked() {
        let dir = tempfile::tempdir().expect("voice folder");
        assert!(matches!(place_tokenizer(dir.path()), Err(VoiceError::Program { .. })));
        // Nothing is started either.
        assert!(matches!(Qwen::load(dir.path(), "de"), Err(VoiceError::Program { .. })));
    }

    /// Issue #808: the program is looked for beside the assistant, else by its name on `PATH`.
    #[test]
    fn the_program_goes_by_its_name() {
        let name = format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(program().file_name().and_then(|name| name.to_str()), Some(name.as_str()));
    }
}
