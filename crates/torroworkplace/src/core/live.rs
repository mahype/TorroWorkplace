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
//! - A connection that closes, fails, comes near its provider's limit (OpenAI 60 minutes, Grok 120, Gemini 10) or
//!   that the provider announces to end ([`Incoming::GoAway`]) is opened
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
use torro_core::ai::{AiError, AiProvider, BoxFuture};
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
    /// Which session it is: one that ends takes only its own run.
    id: u64,
    task: tokio::task::AbortHandle,
    /// Stops the engine's answer from outside (the window's stop): `true` keeps what was heard in the conversation as
    /// an interruption, `false` only silences the engine (the answer was cancelled already).
    stop: mpsc::UnboundedSender<bool>,
}

/// Counts the sessions, for [`LiveRun::id`].
static SESSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long a frame the echo cancellation did not clean stays dropped after something was read aloud: the echo of
/// the room fades.
const QUIET_AFTER: Duration = Duration::from_millis(300);

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
    /// Starts the session for the conversation by voice when a person chose an engine, its account has a key and the
    /// engine answers; otherwise the local chain answers as before (`assistant.talk_status` says which). One that runs
    /// ends first.
    pub(super) async fn start_live(&self) {
        self.end_live();
        let Some(engine) = self.settings.current().voice.live_engine else { return };
        let usable =
            self.ai.live_engines().await.iter().any(|option| option.engine() == engine && option.missing.is_none());
        let Some(core) = self.me.get().and_then(std::sync::Weak::upgrade) else { return };
        if !usable {
            eprintln!("torroworkplace: the conversation engine has no account with a key; the local chain answers");
            return;
        }
        // Connected before the talk says it is live: an engine that does not answer (a refused key, no network) leaves
        // the conversation to this computer instead of a microphone nobody hears.
        let wire = match self.connect_live(&engine).await {
            Ok(wire) => wire,
            Err(err) => {
                eprintln!(
                    "torroworkplace: cannot connect the conversation engine, the local chain answers: {}",
                    err.code()
                );
                return;
            }
        };
        // The talk ended or another engine was chosen while it connected: nothing more goes to this one.
        if self.lock_talk().is_none() || self.settings.current().voice.live_engine.as_ref() != Some(&engine) {
            return;
        }
        let id = SESSIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let (stop, stops) = mpsc::unbounded_channel();
        let mut live = self.lock_live();
        // Another start came meanwhile: the newer session wins, the older ends.
        if let Some(older) = live.take() {
            older.task.abort();
        }
        let frames = self.voice.mic().open();
        let task = tokio::spawn({
            let engine = engine.clone();
            async move { core.run_live(id, engine, wire, frames, stops).await }
        });
        *live = Some(LiveRun { engine, id, task: task.abort_handle(), stop });
    }

    /// Stops the engine's answer in `conversation` when the session answers there (issue #838): the window's stop or
    /// interruption. `interrupt` keeps what was heard in the conversation. `false` when no session answers there.
    pub(super) fn live_stop(&self, conversation: &str, interrupt: bool) -> bool {
        if !self.live_answers_in(conversation) {
            return false;
        }
        self.lock_live().as_ref().is_some_and(|run| run.stop.send(interrupt).is_ok())
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
        // The protocol belongs to the provider: Gemini Live speaks its own, OpenAI Realtime and Grok Voice one.
        Ok(match provider {
            AiProvider::Gemini => Box::new(live::gemini::GeminiWire::new(socket, &engine.model)),
            _ => Box::new(live::openai::OpenAiWire::new(socket, provider)),
        })
    }

    /// The session (see the module) until the conversation by voice ends.
    async fn run_live(
        self: Arc<Self>,
        id: u64,
        engine: LiveEngine,
        wire: Box<dyn LiveWire>,
        frames: mpsc::Receiver<MicFrame>,
        stops: mpsc::UnboundedReceiver<bool>,
    ) {
        let conversation = self.lock_talk().as_ref().and_then(|talk| talk.conversation.clone());
        let (commands, orders) = mpsc::unbounded_channel();
        let minutes = self.ai.live_minutes(&engine).await;
        let mut session = Session {
            core: &self,
            id,
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
            discarding: false,
            person_speaks: false,
            failures: 0,
            late_heard: None,
            awaiting_results: false,
            item_sounds: None,
            renew: false,
        };
        session.run(frames, stops).await;
    }

    /// Ends the conversation by voice from the session `id`: what is read stops, a lost connection says so in the
    /// conversation (`failed`), and the microphone closes. Only while it is still the session that runs: a newer one
    /// keeps its conversation. The session's task returns after this.
    async fn close_live(&self, id: u64, conversation: Option<&str>, failed: Option<&str>) {
        if let (Some(conversation), Some(message)) = (conversation, failed) {
            self.assistant.note_error(conversation, "offline", message).await;
        }
        let ours = {
            let mut live = self.lock_live();
            let ours = live.as_ref().is_some_and(|run| run.id == id);
            if ours {
                live.take();
            }
            ours
        };
        if !ours {
            return;
        }
        // The talk first, without waiting between: a talk started meanwhile is not taken with it.
        self.voice.mic().close();
        self.forget_talk();
        self.follow_voice();
        self.speech.stop().await;
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
    /// Which session it is ([`LiveRun::id`]).
    id: u64,
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
    /// The answer that runs was stopped: what still comes of it is dropped, until the engine begins the next.
    discarding: bool,
    /// The person speaks now (between the engine's start and end of their speech): no new connection meanwhile.
    person_speaks: bool,
    /// Connections lost in a row without a round between them: a fault that comes back every time ends the talk.
    failures: usize,
    /// A transcript that came too late for its sentence, which "(not understood)" stood in for: dropped when it comes
    /// within [`LATE_HEARD`], so it starts no turn of its own that no answer would end.
    late_heard: Option<Instant>,
    /// The turn's answer called tools and waits for their results: an answer the engine begins meanwhile (to speech
    /// during a tool or an approval) is none of this turn's, and is stopped.
    awaiting_results: bool,
    /// When the sound of the answer's current item began, for where the engine cuts it ([`Outgoing::Cancel`]).
    item_sounds: Option<Instant>,
    /// The provider announced the end of this connection ([`Incoming::GoAway`]): a new one opens in the next pause.
    renew: bool,
}

/// How long a transcript that came too late is still taken for the sentence "(not understood)" stood in for.
const LATE_HEARD: Duration = Duration::from_secs(10);

/// The next line of `reader`'s helper; never while there is none.
async fn next_line(reader: &mut Option<LiveReader<'_>>) -> Result<SpeechLine, crate::apps::SpeechError> {
    match reader {
        Some(reader) => reader.job.next().await,
        None => std::future::pending().await,
    }
}

impl<'a> Session<'a> {
    async fn run(&mut self, mut frames: mpsc::Receiver<MicFrame>, mut stops: mpsc::UnboundedReceiver<bool>) {
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
                    if matches!(order, Outgoing::ToolResults(_)) {
                        self.awaiting_results = false;
                    }
                    if self.wire.send(order).await.is_err() {
                        self.reconnect(false).await
                    } else {
                        true
                    }
                }
                Some(keep) = stops.recv() => {
                    self.stop_answer(keep).await;
                    true
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
                self.person_speaks = true;
                // They went on after a pause: their sentence is not over, nothing waits for its words yet.
                self.stopped = None;
                // Only into an answer that sounds now (#835): a word while a tool runs or an approval waits stops
                // nothing.
                if self.reader.as_ref().is_some_and(|reader| reader.speaking.is_some()) {
                    self.stop_answer(true).await;
                }
            }
            Incoming::SpeechStopped { audio_end_ms } => {
                let now = Instant::now();
                self.last_speech = now;
                self.person_speaks = false;
                self.stopped = Some(now);
                let spoken_end =
                    audio_end_ms.and_then(|end| self.sent.iter().find(|(ms, _)| *ms >= end).map(|(_, at)| *at));
                let pause_ms =
                    spoken_end.map_or(0, |at| u32::try_from(now.duration_since(at).as_millis()).unwrap_or(u32::MAX));
                self.round = Some(Round { stopped: now, pause_ms, first_word: None });
            }
            Incoming::Heard(text) => {
                self.failures = 0;
                // The transcript of a sentence "(not understood)" already stood in for: its answer went there.
                if self.late_heard.take().is_some_and(|at| at.elapsed() < LATE_HEARD) {
                    return true;
                }
                return self.on_heard(&text).await;
            }
            // An answer the engine begins while the turn waits for its tools' results answers speech during a tool or an
            // approval, not the turn: it stops, and what it says is dropped. Approvals are given in the window only.
            Incoming::AnswerStarted if self.awaiting_results => {
                self.discarding = true;
                let _ = self.wire.send(Outgoing::Cancel { played_ms: 0 }).await;
            }
            Incoming::AnswerStarted => {
                (self.discarding, self.answering, self.item_sounds) = (false, true, None);
            }
            // The rest of an answer that was stopped.
            Incoming::AnswerText(_) | Incoming::AnswerAudio(_) | Incoming::AnswerDone { .. } if self.discarding => {}
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
                self.item_sounds.get_or_insert_with(Instant::now);
                self.play(&samples).await;
            }
            Incoming::AnswerDone { text, calls } => {
                self.failures = 0;
                let over = calls.is_empty();
                self.awaiting_results = !over && self.turn.is_some();
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
            // An error of one request (OpenAI keeps the connection): a turn that waits for an answer to it gets none, so it
            // ends; the connection goes on. A connection that is gone says `Closed`.
            Incoming::Failed { code } => {
                eprintln!("torroworkplace: conversation engine failed: {code}");
                if self.turn.is_some() && !self.answering {
                    (self.turn, self.awaiting_results) = (None, false);
                }
            }
            // The provider ends the connection soon (issue #918): in the next pause a new one takes over, with the
            // conversation so far as text, before the provider closes this one in the middle of a sentence.
            Incoming::GoAway => self.renew = true,
            Incoming::Closed => return self.reconnect(false).await,
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
            self.core.close_live(self.id, None, None).await;
            return false;
        }
        let (events, heard) = mpsc::unbounded_channel();
        let turn =
            Arc::new(LiveTurn::new(&self.engine.model, self.commands.clone(), heard, Arc::clone(&self.configured)));
        let named = self.named();
        match self.core.assistant.send_live(self.conversation.clone(), text, named, turn).await {
            torro_ipc::Response::Conversation { conversation, .. } => {
                if self.conversation.as_deref() != Some(conversation.id.as_str()) {
                    if let Some(talk) = self.core.lock_talk().as_mut() {
                        talk.conversation = Some(conversation.id.clone());
                    }
                    self.core.voice.notify();
                    self.conversation = Some(conversation.id);
                }
                // The answer may be over before what was said came (OpenAI's transcript comes on its own): then the turn
                // has it all, and no later answer goes to it.
                let over =
                    self.pending.iter().any(|event| matches!(event, TurnEvent::Done { calls, .. } if calls.is_empty()));
                for event in self.pending.drain(..) {
                    let _ = events.send(event);
                }
                self.turn = (!over).then_some(events);
            }
            // An answer runs there still (one that came late after "(not understood)", too), or the message could not be
            // stored: what was said is dropped; the answer that runs goes on.
            torro_ipc::Response::Error { code, .. } => {
                eprintln!("torroworkplace: what was said to the conversation engine is dropped: {code}");
                self.pending.clear();
            }
            _ => self.pending.clear(),
        }
        true
    }

    /// The person spoke into the answer, or stopped it in the window: it stops where they began, the engine forgets
    /// what was not played, and what still comes of it is dropped. With `keep` the conversation keeps what was heard
    /// as an interruption ([`crate::assistant::Assistant::interrupt`]); without, the answer was cancelled already.
    async fn stop_answer(&mut self, keep: bool) {
        if !self.answering && self.reader.is_none() && self.turn.is_none() {
            return;
        }
        // Of the item that sounds now: an answer with tools has one item before them and one after.
        let played_ms = self.item_sounds.map_or(0, |at| u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX));
        let heard =
            self.reader.as_ref().map(|reader| reader.heard.lock().unwrap_or_else(PoisonError::into_inner).clone());
        self.stop_reading().await;
        if let Some(id) = self.conversation.clone()
            && keep
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
        let running = self.answering;
        (self.turn, self.answering, self.response) = (None, false, Spoken::default());
        (self.awaiting_results, self.item_sounds) = (false, None);
        self.pending.clear();
        // Only an answer that still comes has more to drop; one that was over sends nothing more.
        self.discarding = running;
        let _ = self.wire.send(Outgoing::Cancel { played_ms }).await;
    }

    /// The engine and model the person's message names (issue #838): the engine's account, so tools, approvals and the
    /// log name where the data goes; the model is the chat model of the settings when it is one of that account, else
    /// none. A message typed later in the same conversation goes to a chat model, never to the conversation engine.
    fn named(&self) -> (String, Option<String>) {
        let chat = self.core.settings.current().ai_slots.chat;
        let model = chat.filter(|slot| slot.engine == self.engine.account).and_then(|slot| slot.model);
        (self.engine.account.clone(), model)
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
        if self.stopped.is_some_and(|at| at.elapsed() >= self.wire.heard_wait()) {
            self.late_heard = Some(Instant::now());
            return self.on_heard("").await;
        }
        let busy = self.answering
            || self.reader.is_some()
            || self.stopped.is_some()
            || self.turn.is_some()
            || self.person_speaks;
        if !busy && self.last_speech.elapsed() >= SILENCE {
            self.core.close_live(self.id, None, None).await;
            return false;
        }
        if !busy && (self.renew || self.connected.elapsed() >= self.limit) {
            return self.reconnect(true).await;
        }
        true
    }

    /// A new connection after the old one closed or failed, or (`planned`) came near its limit in a pause: [`RECONNECTS`]
    /// tries a second apart. A connection lost again and again without a round between ends the talk too, so a fault
    /// that comes back every time (a field the provider refuses) never loops. `false` once it ended.
    async fn reconnect(&mut self, planned: bool) -> bool {
        let lost = tr("The connection to the conversation engine was lost.");
        // An answer that runs says the loss itself: the conversation does not say it twice.
        let told = self.turn.is_some();
        if !planned {
            self.failures += 1;
            if self.failures > RECONNECTS {
                let failed = (!told).then_some(lost);
                self.core.close_live(self.id, self.conversation.clone().as_deref(), failed).await;
                return false;
            }
        }
        // What ran on the old connection is over: its answer's turn fails and says so in the conversation, and its
        // commands go nowhere: a result of a tool never reaches the new connection, which does not know the call.
        self.stop_reading().await;
        (self.turn, self.answering, self.stopped, self.round) = (None, false, None, None);
        (self.discarding, self.person_speaks) = (false, false);
        (self.awaiting_results, self.item_sounds, self.late_heard) = (false, None, None);
        self.pending.clear();
        self.response = Spoken::default();
        (self.commands, self.orders) = mpsc::unbounded_channel();
        for attempt in 0..RECONNECTS {
            if attempt > 0 || !planned {
                tokio::time::sleep(RECONNECT_WAIT).await;
            }
            match self.core.connect_live(&self.engine).await {
                Ok(wire) => {
                    self.wire = wire;
                    (self.connected, self.renew) = (Instant::now(), false);
                    (self.sent_ms, self.sent) = (0, VecDeque::new());
                    self.configure().await;
                    return true;
                }
                Err(err) => {
                    eprintln!("torroworkplace: the conversation engine cannot be reached again: {}", err.code())
                }
            }
        }
        let failed = (!told).then_some(lost);
        self.core.close_live(self.id, self.conversation.clone().as_deref(), failed).await;
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
