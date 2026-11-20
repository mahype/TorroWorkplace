//! The session of a conversation engine in the cloud (issue #838, way B of `docs/sprachchat.md`): while a
//! conversation by voice runs and a person chose an engine (`voice.liveEngine`), the core holds one live connection
//! ([`crate::assistant::live::LiveWire`]) and its key, and everything goes through it:
//!
//! - The listener's microphone comes over its pipe ([`crate::apps::MicPipe`]) and goes on to the engine. A frame the
//!   echo cancellation did not clean is dropped while something is read aloud and for [`QUIET_AFTER`] after it, so
//!   the engine never hears Torro's own voice.
//! - What the person said ([`Incoming::Heard`]) becomes their message in the conversation, and the engine's answer
//!   runs as an answer of the assistant's loop ([`Assistant::send_live`]): its tools go through the core with the
//!   same rights, approvals and log. A closing word ([`torro_ipc::assistant::is_closing`]) ends the conversation.
//! - The engine's sound goes to the speech helper as a job of sound only; it counts as reading aloud, so the
//!   listener, the tray and the windows see it like Weg A. Speaking into it stops it: the answer keeps what was
//!   heard ([`Assistant::interrupt`]), and the engine forgets what was not played.
//! - A connection that closes, fails or comes near its provider's limit (OpenAI 60 minutes, Grok 120) is opened
//!   again in a pause, with the conversation so far as text. When that fails three times, the conversation says so
//!   and ends.
//! - Each round is timed like Weg A's (`settings.voice_timings`), with the engine's model.
//!
//! Nothing here writes text or sound to the disk or the log: only kinds, codes and durations.

use std::collections::VecDeque;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::mpsc;
use torro_core::ai::{AiError, BoxFuture};
use torro_core::i18n::tr;
use torro_ipc::LiveEngine;

use super::{Core, Reading, Speaking, said_so_far, speech_settings, voice_args};
use crate::apps::{MicFrame, SpeechJob, SpeechLine};
use crate::assistant::live::{
    self, ANSWER_RATE, Configured, Incoming, LiveTurn, LiveWire, MIC_RATE, Outgoing, TurnEvent,
};
use crate::assistant::sentences::Sentences;

/// Opens a live connection to an engine: the accounts' ([`Core::connect_live`]), or a test's.
pub(super) type Connect =
    Arc<dyn Fn(LiveEngine) -> BoxFuture<'static, Result<Box<dyn LiveWire>, AiError>> + Send + Sync>;

/// The session that runs (issue #838): its engine and the task to end it with.
#[derive(Debug)]
pub(super) struct LiveRun {
    pub(super) engine: LiveEngine,
    task: tokio::task::AbortHandle,
}

/// How long a frame the echo cancellation did not clean stays dropped after something was read aloud: the echo of
/// the room fades.
const QUIET_AFTER: Duration = Duration::from_millis(300);

/// How long the session waits for what the person said once they stopped: then it counts as not understood.
const HEARD_WAIT: Duration = Duration::from_millis(1500);

/// How long the conversation waits in silence before it ends, as Weg A (`TALK_WAIT_SECONDS` of the listener).
const SILENCE: Duration = Duration::from_secs(300);

/// How often a connection is tried again after it was lost, and how long between the tries.
const RECONNECTS: usize = 3;
const RECONNECT_WAIT: Duration = Duration::from_secs(1);

/// How long before the provider's limit the session opens a new connection, in a pause.
const RENEW_BEFORE: Duration = Duration::from_secs(5 * 60);

/// How often the session looks at its clocks.
const TICK: Duration = Duration::from_millis(250);

/// How much sound sent is remembered with its time, to time the pause at the end of a sentence.
const SENT_KEPT: Duration = Duration::from_secs(30);

impl Core {
    /// Starts the session for the conversation by voice when a person chose an engine and its account has a key;
    /// otherwise the local chain answers as before. One that runs ends first.
    pub(super) async fn start_live(&self) {
        self.end_live();
        let Some(engine) = self.settings.current().voice.live_engine else { return };
        let usable =
            self.ai.live_engines().await.iter().any(|option| option.engine() == engine && option.missing.is_none());
        let Some(core) = self.me.get().and_then(std::sync::Weak::upgrade) else { return };
        if !usable && self.live_connect.is_none() {
            eprintln!("torroworkplace: the conversation engine has no account with a key; the local chain answers");
            return;
        }
        let frames = self.voice.mic().open();
        let task = tokio::spawn({
            let engine = engine.clone();
            async move { core.run_live(engine, frames).await }
        });
        *self.lock_live() = Some(LiveRun { engine, task: task.abort_handle() });
    }

    /// Ends the session at once, if one runs: no frame goes to the engine any more.
    pub(super) fn end_live(&self) {
        if let Some(run) = self.lock_live().take() {
            self.voice.mic().close();
            run.task.abort();
        }
    }

    /// Whether the session answers in `conversation` (issue #838): then nothing else reads its answers aloud.
    pub(super) fn live_answers_in(&self, conversation: &str) -> bool {
        self.lock_live().is_some()
            && self.lock_talk().as_ref().is_some_and(|talk| talk.conversation.as_deref() == Some(conversation))
    }

    /// The engine of the session that runs.
    pub(super) fn live_engine_running(&self) -> Option<LiveEngine> {
        self.lock_live().as_ref().map(|run| run.engine.clone())
    }

    fn lock_live(&self) -> std::sync::MutexGuard<'_, Option<LiveRun>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A live connection to `engine`: the accounts' (its key stays there), or the test's.
    async fn connect_live(&self, engine: &LiveEngine) -> Result<Box<dyn LiveWire>, AiError> {
        if let Some(connect) = &self.live_connect {
            return connect(engine.clone()).await;
        }
        let (provider, _) = self.ai.server(&engine.account).await.ok_or(AiError::NoKey)?;
        let socket = self.ai.live_socket(&engine.account, &engine.model).await?;
        Ok(Box::new(live::openai::OpenAiWire::new(socket, provider)))
    }

    /// The session (see the module) until the conversation by voice ends.
    async fn run_live(self: Arc<Self>, engine: LiveEngine, frames: mpsc::Receiver<MicFrame>) {
        let conversation = self.lock_talk().as_ref().and_then(|talk| talk.conversation.clone());
        let wire = match self.connect_live(&engine).await {
            Ok(wire) => wire,
            Err(err) => {
                eprintln!("torroworkplace: cannot connect the conversation engine: {}", err.code());
                self.close_live(conversation.as_deref(), Some(&live_error(&err))).await;
                return;
            }
        };
        let (commands, orders) = mpsc::unbounded_channel();
        let minutes = self.ai.live_minutes(&engine).await;
        let mut session = Session {
            core: &self,
            engine,
            limit: Duration::from_secs(u64::from(minutes) * 60).saturating_sub(RENEW_BEFORE),
            wire,
            connected: Instant::now(),
            commands,
            orders,
            configured: Configured::default(),
            conversation,
            turn: None,
            pending: Vec::new(),
            answering: false,
            stopped: None,
            last_speech: Instant::now(),
            response: Spoken::default(),
            reader: None,
            round: None,
            sent_ms: 0,
            sent: VecDeque::new(),
            quiet_until: None,
        };
        session.run(frames).await;
    }

    /// Ends the conversation by voice from the session: what is read stops, a lost connection says so in the
    /// conversation (`failed`), and the microphone closes. Ending it ends the session's task too, so this comes last.
    async fn close_live(&self, conversation: Option<&str>, failed: Option<&str>) {
        if let (Some(id), Some(message)) = (conversation, failed) {
            self.assistant.note_error(id, "offline", message).await;
        }
        self.speech.stop().await;
        self.voice.mic().close();
        self.lock_live().take();
        self.forget_talk();
        self.follow_voice();
    }
}

/// The text of a connection that could not be opened, for people.
fn live_error(error: &AiError) -> String {
    match error {
        AiError::Auth => tr("The provider refused the API key of the conversation engine.").into(),
        AiError::NoKey => tr("Connect the account of the conversation engine again: no API key is stored.").into(),
        _ => tr("The connection to the conversation engine was lost.").into(),
    }
}

/// What the engine's current answer said so far: its text and the sentences complete in it.
#[derive(Default)]
struct Spoken {
    text: String,
    sentences: Option<Sentences>,
    said: Vec<String>,
}

impl Spoken {
    /// Takes `piece` of the text; the sentences that became complete join [`Response::said`] once heard.
    fn push(&mut self, piece: &str) {
        self.text.push_str(piece);
        let sentences = self.sentences.get_or_insert_with(Sentences::default);
        self.said.extend(sentences.push(&self.text));
    }

    /// The answer is over: its last sentence is complete.
    fn finish(&mut self) {
        if let Some(sentences) = self.sentences.as_mut() {
            self.said.extend(sentences.finish());
        }
    }
}

/// The engine's sound as one job of the speech helper (issue #838): from its first sound to its end. It counts as
/// reading aloud while it plays.
struct LiveReader<'a> {
    job: SpeechJob<'a>,
    number: u64,
    /// The sentences of the answer that began to sound, for an interruption.
    heard: Arc<std::sync::Mutex<Vec<String>>>,
    speaking: Option<Speaking>,
    /// When its first sound played.
    playing: Option<Instant>,
    /// Its end went to the helper.
    ended: bool,
    done: tokio::sync::watch::Sender<Option<torro_ipc::Response>>,
}

/// When one round began and went on (issue #828): the end of the person's sentence, the pause before it was
/// noticed, the first word and the first sound of the answer.
struct Round {
    stopped: Instant,
    pause_ms: u32,
    first_word: Option<Instant>,
}

/// One session (see the module).
struct Session<'a> {
    core: &'a Core,
    engine: LiveEngine,
    /// How long one connection may last before a new one is opened in a pause.
    limit: Duration,
    wire: Box<dyn LiveWire>,
    connected: Instant,
    /// What the answer's turns send to the engine, and where the session takes it from.
    commands: mpsc::UnboundedSender<Outgoing>,
    orders: mpsc::UnboundedReceiver<Outgoing>,
    configured: Configured,
    /// The conversation by voice's conversation; `None` until the first sentence starts one.
    conversation: Option<String>,
    /// Where the answer's text goes: the turn of the answer that runs.
    turn: Option<mpsc::UnboundedSender<TurnEvent>>,
    /// What the engine said before the person's message was stored: given to the turn once it is.
    pending: Vec<TurnEvent>,
    /// An answer of the engine runs (from its first word to its end without calls).
    answering: bool,
    /// When the person stopped speaking, until what they said comes.
    stopped: Option<Instant>,
    last_speech: Instant,
    response: Spoken,
    reader: Option<LiveReader<'a>>,
    round: Option<Round>,
    /// How much sound went to the engine on this connection, in milliseconds, and when the last of it went.
    sent_ms: u64,
    sent: VecDeque<(u64, Instant)>,
    /// Until when a frame the echo cancellation did not clean is dropped.
    quiet_until: Option<Instant>,
}

/// The next line of `reader`'s helper; never while there is none.
async fn next_line(reader: &mut Option<LiveReader<'_>>) -> Result<SpeechLine, crate::apps::SpeechError> {
    match reader {
        Some(reader) => reader.job.next().await,
        None => std::future::pending().await,
    }
}

impl<'a> Session<'a> {
    async fn run(&mut self, mut frames: mpsc::Receiver<MicFrame>) {
        self.configure().await;
        let mut tick = tokio::time::interval(TICK);
        loop {
            let go_on = tokio::select! {
                incoming = self.wire.next() => self.on_incoming(incoming).await,
                Some(frame) = frames.recv() => {
                    self.on_frame(frame).await;
                    true
                }
                Some(order) = self.orders.recv() => {
                    if self.wire.send(order).await.is_err() {
                        self.reconnect().await
                    } else {
                        true
                    }
                }
                line = next_line(&mut self.reader) => {
                    self.on_line(line);
                    true
                }
                _ = tick.tick() => self.on_tick().await,
            };
            if !go_on {
                return;
            }
        }
    }

    /// The instructions and tools of the assistant, and the conversation so far, for a new connection.
    async fn configure(&mut self) {
        let given = self.configured.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let setup = match given {
            Some(setup) => Some(setup),
            None => {
                let (_, nothing) = mpsc::unbounded_channel();
                let turn = Arc::new(LiveTurn::new(
                    &self.engine.model,
                    self.commands.clone(),
                    nothing,
                    Arc::clone(&self.configured),
                ));
                let setup = self.core.assistant.live_setup(self.conversation.clone(), &self.engine, turn).await;
                *self.configured.lock().unwrap_or_else(PoisonError::into_inner) = setup.clone();
                setup
            }
        };
        if let Some((instructions, tools)) = setup {
            let _ = self.wire.send(Outgoing::Configure { instructions, tools }).await;
        }
        if let Some(id) = &self.conversation
            && let torro_ipc::Response::Conversation { entries, .. } = self.core.assistant.get(id, None).await
        {
            let history = live::history(&entries);
            if !history.is_empty() {
                let _ = self.wire.send(Outgoing::History(history)).await;
            }
        }
    }

    /// Handles what the engine said; `false` once the session is over.
    async fn on_incoming(&mut self, incoming: Incoming) -> bool {
        match incoming {
            Incoming::SpeechStarted => {
                self.last_speech = Instant::now();
                self.interrupt().await;
            }
            Incoming::SpeechStopped { audio_end_ms } => {
                let now = Instant::now();
                self.last_speech = now;
                self.stopped = Some(now);
                let spoken_end =
                    audio_end_ms.and_then(|end| self.sent.iter().find(|(ms, _)| *ms >= end).map(|(_, at)| *at));
                let pause_ms =
                    spoken_end.map_or(0, |at| u32::try_from(now.duration_since(at).as_millis()).unwrap_or(u32::MAX));
                self.round = Some(Round { stopped: now, pause_ms, first_word: None });
            }
            Incoming::Heard(text) => return self.on_heard(&text).await,
            Incoming::AnswerText(piece) => {
                self.answering = true;
                if let Some(round) = self.round.as_mut() {
                    round.first_word.get_or_insert_with(Instant::now);
                }
                self.response.push(&piece);
                self.tell(TurnEvent::Text(piece));
            }
            Incoming::AnswerAudio(samples) => {
                self.answering = true;
                self.play(&samples).await;
            }
            Incoming::AnswerDone { text, calls } => {
                let over = calls.is_empty();
                self.tell(TurnEvent::Done { text, calls });
                if over {
                    self.answering = false;
                    self.response.finish();
                    self.end_reading().await;
                    self.response = Spoken::default();
                    // The answer is told whole: the turn ends with it.
                    self.turn = None;
                }
            }
            Incoming::Closed | Incoming::Failed { .. } => {
                if let Incoming::Failed { code } = &incoming {
                    eprintln!("torroworkplace: conversation engine failed: {code}");
                }
                return self.reconnect().await;
            }
        }
        true
    }

    /// Gives `event` to the turn of the answer, or keeps it until the person's message is stored.
    fn tell(&mut self, event: TurnEvent) {
        match &self.turn {
            Some(turn) => {
                let _ = turn.send(event);
            }
            None => self.pending.push(event),
        }
    }

    /// What the person said (empty: nothing understood): their message in the conversation, answered by the
    /// engine's turn; a closing word ends the conversation. `false` once it ended.
    async fn on_heard(&mut self, text: &str) -> bool {
        self.stopped = None;
        let text = match text.trim() {
            "" => tr("(not understood)").to_string(),
            said => said.to_string(),
        };
        if torro_ipc::assistant::is_closing(&text) {
            let _ = self.wire.send(Outgoing::Cancel { played_ms: 0 }).await;
            self.core.close_live(None, None).await;
            return false;
        }
        let (events, heard) = mpsc::unbounded_channel();
        let turn =
            Arc::new(LiveTurn::new(&self.engine.model, self.commands.clone(), heard, Arc::clone(&self.configured)));
        match self.core.assistant.send_live(self.conversation.clone(), text, &self.engine, turn).await {
            torro_ipc::Response::Conversation { conversation, .. } => {
                if self.conversation.as_deref() != Some(conversation.id.as_str()) {
                    if let Some(talk) = self.core.lock_talk().as_mut() {
                        talk.conversation = Some(conversation.id.clone());
                    }
                    self.core.voice.notify();
                    self.conversation = Some(conversation.id);
                }
                for event in self.pending.drain(..) {
                    let _ = events.send(event);
                }
                self.turn = Some(events);
            }
            // An answer runs there still, or the message could not be stored: what was said is dropped, and so is
            // the engine's answer to it.
            torro_ipc::Response::Error { code, .. } => {
                eprintln!("torroworkplace: what was said to the conversation engine is dropped: {code}");
                self.pending.clear();
                if self.answering {
                    let _ = self.wire.send(Outgoing::Cancel { played_ms: 0 }).await;
                }
            }
            _ => self.pending.clear(),
        }
        true
    }

    /// The person spoke into the answer: it stops where they began, the conversation keeps what was heard, and the
    /// engine forgets what was not played.
    async fn interrupt(&mut self) {
        if !self.answering && self.reader.is_none() {
            return;
        }
        let played_ms = self
            .reader
            .as_ref()
            .and_then(|reader| reader.playing)
            .map_or(0, |at| u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX));
        let heard =
            self.reader.as_ref().map(|reader| reader.heard.lock().unwrap_or_else(PoisonError::into_inner).clone());
        self.stop_reading().await;
        if let Some(id) = self.conversation.clone()
            && (self.turn.is_some() || heard.is_some())
        {
            let heard = heard.as_deref().and_then(said_so_far);
            let answer = self.core.assistant.interrupt(&id, heard, Utc::now()).await;
            // As with a stop: the terminal's commands in the background end with the answer.
            if answer == torro_ipc::Response::Ok
                && let Some(assistant) = self.core.assistant_client()
            {
                self.core.end_terminal_conversation(&assistant.id, &id, false).await;
            }
        }
        (self.turn, self.answering, self.response) = (None, false, Spoken::default());
        self.pending.clear();
        let _ = self.wire.send(Outgoing::Cancel { played_ms }).await;
    }

    /// A frame of the microphone goes to the engine, unless it may hold Torro's own voice.
    async fn on_frame(&mut self, frame: MicFrame) {
        let now = Instant::now();
        if self.reader.as_ref().is_some_and(|reader| reader.speaking.is_some()) {
            self.quiet_until = Some(now + QUIET_AFTER);
        }
        if !frame.clean && self.quiet_until.is_some_and(|until| now < until) {
            return;
        }
        self.sent_ms += frame.samples.len() as u64 * 1000 / u64::from(MIC_RATE);
        self.sent.push_back((self.sent_ms, now));
        while self.sent.front().is_some_and(|(_, at)| now.duration_since(*at) > SENT_KEPT) {
            self.sent.pop_front();
        }
        if self.wire.send(Outgoing::Audio(frame.samples)).await.is_err() {
            eprintln!("torroworkplace: the microphone could not reach the conversation engine");
        }
    }

    /// Plays `samples` of the answer: the first opens a job of sound only at the speech helper.
    async fn play(&mut self, samples: &[i16]) {
        use base64::Engine as _;
        if self.reader.is_none() {
            self.reader = self.begin_reading().await;
        }
        let text = self.response.said.len();
        let Some(reader) = self.reader.as_mut() else { return };
        *reader.heard.lock().unwrap_or_else(PoisonError::into_inner) = self.response.said.clone();
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        for part in bytes.chunks(super::PCM_LINE) {
            let line = serde_json::json!({"pcm": {
                "rate": ANSWER_RATE,
                "data": base64::engine::general_purpose::STANDARD.encode(part),
                "text": text,
            }});
            if reader.job.send(&line.to_string()).await.is_err() {
                self.reader = None;
                return;
            }
        }
    }

    /// The speech helper's job for the answer's sound, registered as the reading of the conversation.
    async fn begin_reading(&mut self) -> Option<LiveReader<'a>> {
        let core: &'a Core = self.core;
        let voice = core.settings.current().voice;
        let number = core.speech.claim().await;
        let mut job = match core.speech.begin_claimed(number, &voice_args("--speak", &speech_settings(&voice))).await {
            Ok(job) => job,
            Err(err) => {
                eprintln!("torroworkplace: the engine's answer cannot be played: {err:?}");
                return None;
            }
        };
        if !matches!(job.send_if_current("{\"stream\": {}}").await, Ok(true)) {
            job.leave();
            return None;
        }
        let (done, ended) = tokio::sync::watch::channel(None);
        let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
        if let Some(conversation) = &self.conversation {
            let after = match core.assistant.get(conversation, None).await {
                torro_ipc::Response::Conversation { conversation, .. } => conversation.last,
                _ => 0,
            };
            *core.reading.lock().await = Some(Reading {
                conversation: conversation.clone(),
                after,
                number,
                done: ended,
                heard: Arc::clone(&heard),
            });
        }
        Some(LiveReader { job, number, heard, speaking: None, playing: None, ended: false, done })
    }

    /// The answer's sound is all there: the helper plays it to its end.
    async fn end_reading(&mut self) {
        if let Some(reader) = self.reader.as_mut()
            && !reader.ended
        {
            reader.ended = true;
            *reader.heard.lock().unwrap_or_else(PoisonError::into_inner) = self.response.said.clone();
            if reader.job.send("\"end\"").await.is_err() {
                self.finish_reading();
            }
        }
    }

    /// Stops what is read now and waits a moment for the helper to say so.
    async fn stop_reading(&mut self) {
        if self.reader.is_none() {
            return;
        }
        self.core.speech.stop().await;
        if let Some(reader) = self.reader.as_mut() {
            let _ = tokio::time::timeout(QUIET_AFTER, async {
                while !matches!(reader.job.next().await, Ok(SpeechLine::Done | SpeechLine::Failed { .. }) | Err(_)) {}
            })
            .await;
        }
        self.finish_reading();
    }

    /// The reading is over: it no longer counts, and the room is quiet after [`QUIET_AFTER`].
    fn finish_reading(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.done.send_replace(Some(torro_ipc::Response::Ok));
            if let Ok(mut reading) = self.core.reading.try_lock()
                && reading.as_ref().is_some_and(|reading| reading.number == reader.number)
            {
                *reading = None;
            }
            self.quiet_until = Some(Instant::now() + QUIET_AFTER);
        }
    }

    /// A line of the speech helper about the answer's sound.
    fn on_line(&mut self, line: Result<SpeechLine, crate::apps::SpeechError>) {
        match line {
            Ok(SpeechLine::Playing) => {
                let core = self.core;
                if let Some(reader) = self.reader.as_mut() {
                    reader.speaking.get_or_insert_with(|| Speaking::new(&core.speaking));
                    reader.playing.get_or_insert_with(Instant::now);
                }
                self.time_round();
            }
            Ok(SpeechLine::Waiting) => {
                if let Some(reader) = self.reader.as_mut() {
                    reader.speaking = None;
                }
            }
            Ok(SpeechLine::Said(said)) => {
                if let Some(reader) = self.reader.as_mut() {
                    let texts: Vec<String> = self.response.said.iter().take(said).cloned().collect();
                    *reader.heard.lock().unwrap_or_else(PoisonError::into_inner) = texts;
                }
            }
            Ok(SpeechLine::Done) => self.finish_reading(),
            Ok(SpeechLine::Failed { code, .. }) => {
                eprintln!("torroworkplace: the engine's answer could not be played: {code}");
                self.finish_reading();
            }
            Err(_) => self.finish_reading(),
            Ok(SpeechLine::Pieces(_) | SpeechLine::Marked) => {}
        }
    }

    /// Keeps the round (issue #828) once its first sound plays: only durations and the engine's model.
    fn time_round(&mut self) {
        let Some(round) = self.round.take() else { return };
        let now = Instant::now();
        let first_word = round.first_word.unwrap_or(now);
        let ms = |from: Instant, to: Instant| {
            u32::try_from(to.saturating_duration_since(from).as_millis()).unwrap_or(u32::MAX)
        };
        let timed = torro_ipc::VoiceRound {
            at: Utc::now(),
            pause_ms: round.pause_ms,
            recognize_ms: 0,
            first_word_ms: ms(round.stopped, first_word),
            first_sound_ms: ms(first_word, now),
            held: false,
            engine: Some(self.engine.model.clone()),
        };
        eprintln!(
            "torroworkplace: voice round: engine, pause {} ms, first word {} ms, first sound {} ms, total {} ms",
            timed.pause_ms,
            timed.first_word_ms,
            timed.first_sound_ms,
            timed.total_ms()
        );
        self.core.keep_round(timed);
    }

    /// The session's clocks: what the person said comes late, a long silence, the provider's limit. `false` once the
    /// session is over.
    async fn on_tick(&mut self) -> bool {
        if self.stopped.is_some_and(|at| at.elapsed() >= HEARD_WAIT) {
            return self.on_heard("").await;
        }
        let busy = self.answering || self.reader.is_some() || self.stopped.is_some() || self.turn.is_some();
        if !busy && self.last_speech.elapsed() >= SILENCE {
            self.core.close_live(None, None).await;
            return false;
        }
        if !busy && self.connected.elapsed() >= self.limit {
            return self.reconnect().await;
        }
        true
    }

    /// A new connection after the old one closed, failed or came near its limit: [`RECONNECTS`] tries, then the
    /// conversation says so and ends. `false` once it ended.
    async fn reconnect(&mut self) -> bool {
        // What ran on the old connection is over: its answer's turn fails and says so in the conversation.
        self.stop_reading().await;
        (self.turn, self.answering, self.stopped, self.round) = (None, false, None, None);
        self.pending.clear();
        self.response = Spoken::default();
        for attempt in 0..RECONNECTS {
            if attempt > 0 {
                tokio::time::sleep(RECONNECT_WAIT).await;
            }
            match self.core.connect_live(&self.engine).await {
                Ok(wire) => {
                    self.wire = wire;
                    self.connected = Instant::now();
                    (self.sent_ms, self.sent) = (0, VecDeque::new());
                    self.configure().await;
                    return true;
                }
                Err(err) => {
                    eprintln!("torroworkplace: the conversation engine cannot be reached again: {}", err.code())
                }
            }
        }
        let lost = tr("The connection to the conversation engine was lost.");
        self.core.close_live(self.conversation.clone().as_deref(), Some(lost)).await;
        false
    }
}

#[cfg(test)]
pub(super) mod tests {
    //! A session with a fake engine ([`FakeWire`]): no network, no listener, no speech helper.

    use std::sync::Mutex;

    use super::*;

    /// What a fake engine was sent, and what it says next.
    #[derive(Default)]
    pub(crate) struct Script {
        pub(crate) sent: Mutex<Vec<Outgoing>>,
        pub(crate) connections: std::sync::atomic::AtomicUsize,
    }

    /// An engine that says what the test sends it and keeps what it is sent.
    pub(crate) struct FakeWire {
        script: Arc<Script>,
        says: mpsc::UnboundedReceiver<Incoming>,
    }

    impl LiveWire for FakeWire {
        fn send(&mut self, out: Outgoing) -> BoxFuture<'_, Result<(), AiError>> {
            self.script.sent.lock().expect("lock").push(out);
            Box::pin(async { Ok(()) })
        }

        fn next(&mut self) -> BoxFuture<'_, Incoming> {
            Box::pin(async { self.says.recv().await.unwrap_or(Incoming::Closed) })
        }
    }

    /// A way to connect that gives a fake engine each time, whose voice is the next sender of `voices`.
    pub(crate) fn fake_engine(
        script: &Arc<Script>,
    ) -> (Connect, mpsc::UnboundedReceiver<mpsc::UnboundedSender<Incoming>>) {
        let (voices, heard) = mpsc::unbounded_channel();
        let script = Arc::clone(script);
        let connect: Connect = Arc::new(move |_| {
            let (say, says) = mpsc::unbounded_channel();
            let _ = voices.send(say);
            script.connections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let wire: Box<dyn LiveWire> = Box::new(FakeWire { script: Arc::clone(&script), says });
            Box::pin(async move { Ok(wire) })
        });
        (connect, heard)
    }
}
