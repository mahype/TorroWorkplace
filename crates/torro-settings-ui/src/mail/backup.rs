//! Backup and restore controls of one mail account's own settings page (issue #267, task MB.6,
//! `docs/mail-backup.md`).
//!
//! The page only sends `mail.backup_*` and `mail.restore_*` requests and shows their answers; the
//! core reads, writes and checks every archive. The window opens the system's choosers for a
//! folder or an archive ([`Chooser`]) and hands the chosen path back, because a settings section
//! never starts a dialog itself.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, svg};
use iced::{Alignment, Element, Fill, Length, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{palette, space};
use torro_gui::units::{format_bytes, format_count};
use torro_gui::{style, theme, typo, widgets};
use torro_ipc::mail::{
    BackupJob, BackupPhase, BackupScope, BackupVerification, RestoreFolderPlan, RestorePlan, RestoreResult,
};
use torro_ipc::{Request, Response};

use crate::Action;
use crate::parts::{choice, field, row_frame, setting_row};

/// The request that is on its way; at most one per account page, so answers never cross. Verify
/// and Plan remember the archive they asked about: an answer for another file is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Create,
    Poll,
    Cancel,
    Verify(String),
    Plan(String),
    Apply,
}

impl Pending {
    /// Where an error of this request shows: next to the button that sent it.
    fn place(&self) -> Place {
        match self {
            Pending::Create => Place::Create,
            Pending::Poll | Pending::Cancel => Place::Job,
            Pending::Verify(_) | Pending::Plan(_) => Place::Inspect,
            Pending::Apply => Place::Apply,
        }
    }
}

/// The part of the page an error belongs to, so a message never stands under the wrong group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Under "Create backup": a bad date, folder or name, a job that could not start or is lost.
    Create,
    /// Under the running job: refreshing or cancelling failed.
    Job,
    /// Under "Verify archive" and "Run restore test".
    Inspect,
    /// Under "Restore mail".
    Apply,
}

/// An error shown on the page: what went wrong, at the [`Place`] it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Problem {
    place: Place,
    text: String,
}

/// Which system chooser a "Choose…" button asks the window for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chooser {
    /// An existing folder new backups are written to.
    Folder,
    /// A `.zip` archive to verify or restore.
    Archive,
}

/// A filled-in page for `--demo --open backup…` (torroinbox), so each state can be looked at and
/// photographed without a core or a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// A server backup reading mail, with "Refresh progress" and "Cancel backup".
    Running,
    /// A restore test of an incomplete archive with a folder table (one aggregate folder) and
    /// uploads of an earlier restore that were never confirmed.
    Plan,
    /// The job the page waited for is gone (the core was restarted).
    Error,
    /// A complete backup, its verified archive and a finished restore.
    Done,
}

/// What one account's page keeps between presses: the form, the last answers, and the one
/// request on its way. Answers come back through [`answered`], whatever page is shown meanwhile.
#[derive(Debug)]
pub struct State {
    create_open: bool,
    calendar_open: bool,
    calendar_month: NaiveDate,
    pub source: BackupScope,
    pub since: String,
    pub file_name: String,
    pub destination: String,
    pub restore_file: String,
    pub job: Option<BackupJob>,
    pub verification: Option<BackupVerification>,
    pub plan: Option<RestorePlan>,
    pub result: Option<RestoreResult>,
    pub confirm_restore: bool,
    pub allow_incomplete: bool,
    /// "Upload unconfirmed mail again": sends `retry_unconfirmed` with the restore.
    pub retry_unconfirmed: bool,
    problem: Option<Problem>,
    pending: Option<Pending>,
    /// The folder of the running job, so its archive can be named in full for the restore field.
    job_directory: Option<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            create_open: false,
            calendar_open: false,
            calendar_month: chrono::Utc::now().date_naive().with_day(1).expect("first of month"),
            source: BackupScope::Server,
            since: String::new(),
            file_name: String::new(),
            destination: String::new(),
            restore_file: String::new(),
            job: None,
            verification: None,
            plan: None,
            result: None,
            confirm_restore: false,
            allow_incomplete: false,
            retry_unconfirmed: false,
            problem: None,
            pending: None,
            job_directory: None,
        }
    }
}

/// What a row of the backup and restore groups reports.
#[derive(Debug, Clone)]
pub enum Message {
    OpenCreate,
    CloseCreate,
    ToggleCalendar,
    PreviousMonth,
    NextMonth,
    PickDay(NaiveDate),
    Source(BackupScope),
    Since(String),
    FileName(String),
    Destination(String),
    RestoreFile(String),
    /// "Choose…": the window opens the system's chooser and answers with [`Message::Chosen`];
    /// [`update`] itself does nothing with it.
    Choose(Chooser),
    /// The path the person picked in a chooser.
    Chosen(Chooser, PathBuf),
    Create,
    Poll,
    Cancel,
    Verify,
    Plan,
    ConfirmRestore(bool),
    AllowIncomplete(bool),
    RetryUnconfirmed(bool),
    Apply,
}

impl State {
    /// A job that has not ended: it keeps the account busy, so "Create backup" stays off.
    fn active_job(&self) -> bool {
        self.job.as_ref().is_some_and(|job| !job.phase.is_final())
    }

    /// The form as the account stored it last (`MailSettings.backup`): source, day and folder.
    pub fn from_preferences(preferences: &torro_mail::BackupPreferences) -> Self {
        Self {
            source: if preferences.local { BackupScope::Local } else { BackupScope::Server },
            since: preferences.since.map_or_else(String::new, |day| day.format("%Y-%m-%d").to_string()),
            destination: preferences.directory.clone().unwrap_or_default(),
            ..Self::default()
        }
    }

    /// The page of `scene` for `account`, as the demo shows it ([`Scene`]).
    pub fn scene(scene: Scene, account: &str) -> Self {
        let archive = "mail-anna-2026-10-21.zip";
        let job = |phase, archive: Option<&str>| BackupJob {
            id: "demo-job".into(),
            account: account.into(),
            source: BackupScope::Server,
            phase,
            folders: 12,
            messages: if phase == BackupPhase::Complete { 8_412 } else { 1_324 },
            bytes: if phase == BackupPhase::Complete { 1_204_000_000 } else { 186_000_000 },
            archive: archive.map(str::to_string),
            warnings: Vec::new(),
        };
        match scene {
            Scene::Running => Self { job: Some(job(BackupPhase::Reading, None)), ..Self::default() },
            Scene::Error => {
                Self { problem: Some(Problem { place: Place::Create, text: lost_job().into() }), ..Self::default() }
            }
            Scene::Plan => {
                Self { restore_file: archive.into(), plan: Some(demo_plan(account, archive)), ..Self::default() }
            }
            Scene::Done => Self {
                restore_file: archive.into(),
                job: Some(job(BackupPhase::Complete, Some(archive))),
                verification: Some(BackupVerification {
                    source: BackupScope::Server,
                    since: None,
                    started_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                    finished_at: None,
                    complete: true,
                    folders: 12,
                    messages: 8_412,
                    bytes: 1_204_000_000,
                    warnings: Vec::new(),
                }),
                result: Some(RestoreResult { added: 1_726, skipped: 6_686, uncertain: 0, warnings: Vec::new() }),
                ..Self::default()
            },
        }
    }

    /// "Restore mail" is on only for the plan of this account, after "I reviewed the plan", with no
    /// uncertain mail, and with its own confirmation for an incomplete archive and for uploading
    /// unconfirmed mail again.
    fn can_apply(&self, account: &str) -> bool {
        self.pending.is_none()
            && self.confirm_restore
            && self.plan.as_ref().is_some_and(|plan| {
                plan.account == account
                    && plan.uncertain == 0
                    && (plan.complete_archive || self.allow_incomplete)
                    && (plan.unconfirmed == 0 || self.retry_unconfirmed)
            })
    }

    /// A restore test or a verification is on its way: the archive field stays as it was asked.
    fn inspecting(&self) -> bool {
        matches!(self.pending, Some(Pending::Verify(_) | Pending::Plan(_)))
    }

    fn fail(&mut self, place: Place, text: impl Into<String>) {
        self.problem = Some(Problem { place, text: text.into() });
    }

    /// Clears the error of these places; errors elsewhere on the page stay.
    fn clear(&mut self, places: &[Place]) {
        if self.problem.as_ref().is_some_and(|problem| places.contains(&problem.place)) {
            self.problem = None;
        }
    }

    fn problem_at(&self, place: Place) -> Option<&str> {
        self.problem.as_ref().filter(|problem| problem.place == place).map(|problem| problem.text.as_str())
    }
}

/// A press or a typed character on `account`'s page. Typing only changes the form; the buttons
/// return the one request they stand for ([`Action::Ask`]) and mark it pending, and every check the
/// page can make itself (date, folder, name) happens before anything is sent. A press while a
/// request is on its way, or one whose button is off, does nothing.
pub fn update<A>(state: &mut State, account: &str, message: Message) -> Action<A> {
    match message {
        Message::OpenCreate if state.pending.is_none() && !state.active_job() => {
            state.create_open = true;
            state.calendar_open = false;
        }
        Message::CloseCreate => {
            state.create_open = false;
            state.calendar_open = false;
        }
        Message::ToggleCalendar => {
            state.calendar_open = !state.calendar_open;
            if state.calendar_open
                && let Ok(day) = NaiveDate::parse_from_str(&state.since, "%Y-%m-%d")
            {
                state.calendar_month = day.with_day(1).expect("first of month");
            }
        }
        Message::PreviousMonth => {
            let previous = state.calendar_month - chrono::Duration::days(1);
            state.calendar_month = previous.with_day(1).expect("first of month");
        }
        Message::NextMonth => {
            let after = state.calendar_month + chrono::Duration::days(32);
            state.calendar_month = after.with_day(1).expect("first of month");
        }
        Message::PickDay(day) => {
            state.since = day.format("%Y-%m-%d").to_string();
            state.calendar_month = day.with_day(1).expect("first of month");
            state.calendar_open = false;
        }
        Message::Source(source) => state.source = source,
        Message::Since(since) => state.since = since,
        Message::FileName(file_name) => state.file_name = file_name,
        Message::Destination(destination) => state.destination = destination,
        Message::RestoreFile(file) if !state.inspecting() => {
            state.restore_file = file;
            state.verification = None;
            state.plan = None;
            state.result = None;
            state.confirm_restore = false;
            state.allow_incomplete = false;
            state.retry_unconfirmed = false;
            state.clear(&[Place::Inspect, Place::Apply]);
        }
        Message::Chosen(chooser, path) => {
            let Some(path) = path.to_str().map(str::to_string) else {
                let place = if chooser == Chooser::Folder { Place::Create } else { Place::Inspect };
                state.fail(place, tr("This path cannot be used."));
                return Action::None;
            };
            return match chooser {
                Chooser::Folder => update(state, account, Message::Destination(path)),
                Chooser::Archive => update(state, account, Message::RestoreFile(path)),
            };
        }
        Message::ConfirmRestore(confirmed) => state.confirm_restore = confirmed,
        Message::AllowIncomplete(allowed) => state.allow_incomplete = allowed,
        Message::RetryUnconfirmed(retry) => state.retry_unconfirmed = retry,
        Message::Create if state.pending.is_none() && !state.active_job() => return create(state, account),
        Message::Poll if state.pending.is_none() => {
            if let Some(job) = &state.job {
                state.pending = Some(Pending::Poll);
                return Action::Ask(Request::MailBackupStatus { account: account.into(), job: job.id.clone() });
            }
        }
        Message::Cancel if state.pending.is_none() => {
            if let Some(job) = &state.job
                && !job.phase.is_final()
                && job.phase != BackupPhase::Cancelling
            {
                state.pending = Some(Pending::Cancel);
                return Action::Ask(Request::MailBackupCancel { account: account.into(), job: job.id.clone() });
            }
        }
        Message::Verify if state.pending.is_none() => {
            let file = state.restore_file.trim().to_string();
            if !file.is_empty() {
                state.clear(&[Place::Inspect]);
                state.pending = Some(Pending::Verify(file.clone()));
                return Action::Ask(Request::MailBackupVerify { file, account: account.into() });
            }
        }
        Message::Plan if state.pending.is_none() => {
            let file = state.restore_file.trim().to_string();
            if !file.is_empty() {
                // A new test asks every question again: nothing confirmed for the old plan carries over.
                state.plan = None;
                state.result = None;
                state.confirm_restore = false;
                state.allow_incomplete = false;
                state.retry_unconfirmed = false;
                state.clear(&[Place::Inspect, Place::Apply]);
                state.pending = Some(Pending::Plan(file.clone()));
                return Action::Ask(Request::MailRestorePlan { file, account: account.into() });
            }
        }
        Message::Apply if state.can_apply(account) => {
            let Some(plan) = &state.plan else { return Action::None };
            let request = Request::MailRestoreApply {
                plan: plan.id.clone(),
                account: account.into(),
                allow_incomplete: state.allow_incomplete && !plan.complete_archive,
                retry_unconfirmed: state.retry_unconfirmed && plan.unconfirmed > 0,
            };
            state.clear(&[Place::Apply]);
            state.pending = Some(Pending::Apply);
            return Action::Ask(request);
        }
        _ => {}
    }
    Action::None
}

/// "Create backup": checks the form and asks for the job. A chosen folder needs an archive name and
/// is sent as one absolute path; without a folder the core uses its managed folder.
fn create<A>(state: &mut State, account: &str) -> Action<A> {
    let since = match state.since.trim() {
        "" => None,
        day => match NaiveDate::parse_from_str(day, "%Y-%m-%d") {
            Ok(day) => Some(day),
            Err(_) => {
                state.fail(Place::Create, tr("Enter a valid date as YYYY-MM-DD."));
                return Action::None;
            }
        },
    };
    let name = state.file_name.trim();
    if !name.is_empty() && !plain_archive_name(account, name) {
        state.fail(Place::Create, tr("Use a simple .zip archive name without folders."));
        return Action::None;
    }
    let destination = state.destination.trim();
    let file_name = if destination.is_empty() {
        (!name.is_empty()).then(|| name.to_string())
    } else if !Path::new(destination).is_absolute() || name.is_empty() {
        state.fail(Place::Create, tr("Choose an absolute backup folder and an archive name."));
        return Action::None;
    } else {
        Some(Path::new(destination).join(name).to_string_lossy().into_owned())
    };
    state.job_directory = (!destination.is_empty()).then(|| PathBuf::from(destination));
    state.clear(&[Place::Create, Place::Job]);
    state.pending = Some(Pending::Create);
    state.create_open = false;
    state.calendar_open = false;
    Action::Ask(Request::MailBackupCreate { account: account.into(), source: state.source, since, file_name })
}

/// Whether `name` is a plain archive name by the core's own rule (`torro_ipc::mail::limit_refusal`
/// for `mail.backup_create`), so the page never sends a name the core refuses. A path is no name
/// here: the folder has its own row.
fn plain_archive_name(account: &str, name: &str) -> bool {
    let probe = Request::MailBackupCreate {
        account: account.into(),
        source: BackupScope::Server,
        since: None,
        file_name: Some(name.into()),
    };
    !Path::new(name).is_absolute() && torro_ipc::mail::limit_refusal(&probe).is_none()
}

/// The message for a job the core no longer knows (`not-found` on status or cancel), as after a
/// restart of the core: the page lets go of it, so a new backup can start.
fn lost_job() -> &'static str {
    tr(
        "Torro no longer knows this backup job, for example after a restart. Its archive was not finished; start a new backup.",
    )
}

/// The answer to the request that was pending, as the window got it. The core's errors arrive as
/// `Ok(Response::Error)` with their code, failures to reach it as `Err`. An answer without a
/// pending request, or one for an archive the field no longer names, changes nothing.
pub fn answered(state: &mut State, answer: &Result<Response, String>) {
    let Some(pending) = state.pending.take() else { return };
    if let Pending::Verify(file) | Pending::Plan(file) = &pending
        && file != state.restore_file.trim()
    {
        return;
    }
    let place = pending.place();
    match (pending, answer) {
        (Pending::Create | Pending::Poll | Pending::Cancel, Ok(Response::MailBackupJob { job })) => {
            if let Some(archive) = &job.archive
                && state.restore_file.is_empty()
            {
                state.restore_file = state.job_directory.as_ref().map_or_else(
                    || archive.clone(),
                    |directory| directory.join(archive).to_string_lossy().into_owned(),
                );
            }
            state.job = Some(job.clone());
            state.clear(&[Place::Create, Place::Job]);
        }
        (Pending::Poll | Pending::Cancel, Ok(Response::Error { code, .. })) if code == "not-found" => {
            state.job = None;
            state.fail(Place::Create, lost_job());
        }
        (Pending::Verify(_), Ok(Response::MailBackupVerified { verification })) => {
            state.verification = Some(verification.clone());
        }
        (Pending::Plan(file), Ok(Response::MailRestorePlanned { plan })) => {
            let mut plan = plan.clone();
            if plan.archive.is_empty() {
                plan.archive = file;
            }
            state.plan = Some(plan);
        }
        (Pending::Apply, Ok(Response::MailRestored { restore })) => {
            // The core used the plan up: a second press would only meet "plan no longer available".
            // A new restore starts with a new test, and every question is asked again.
            state.result = Some(restore.clone());
            state.plan = None;
            state.confirm_restore = false;
            state.allow_incomplete = false;
            state.retry_unconfirmed = false;
        }
        (_, Ok(Response::Error { message, .. })) => state.fail(place, message.clone()),
        (_, Err(error)) => state.fail(place, error.clone()),
        _ => state.fail(place, tr("The mail backup answer was unexpected. Try again.")),
    }
}

/// Called by the mail window's ordinary ten-second tick while this account page is open: the
/// status request of a job that has not ended, unless another request is on its way.
pub fn poll_request(state: &mut State, account: &str) -> Option<Request> {
    let job = state.job.as_ref()?;
    if state.pending.is_some() || job.phase.is_final() {
        return None;
    }
    state.pending = Some(Pending::Poll);
    Some(Request::MailBackupStatus { account: account.into(), job: job.id.clone() })
}

/// The groups "Backup" and "Restore" of `account`'s page. Status, warnings and errors carry an
/// icon in their colour and wrap, so nothing is cut off in a narrow window.
pub fn view<'a>(state: &'a State, account: &str) -> Element<'a, Message> {
    let create = if state.pending == Some(Pending::Create) {
        widgets::busy_button(tr("Starting backup…"), style::button::secondary)
    } else {
        widgets::text_button(
            None,
            tr("Create backup"),
            style::button::primary,
            (state.pending.is_none() && !state.active_job()).then_some(Message::OpenCreate),
        )
    };
    let mut rows = vec![row_frame(create, false)];
    if let Some(job) = &state.job {
        job_rows(&mut rows, state, job);
    }
    error_row(&mut rows, state, Place::Job);
    let summary = widgets::group(tr("Backup"), rows, None);
    let base = column![summary, restore_group(state, account)].spacing(torro_gui::layout::section_gap());
    if state.create_open { widgets::modal(base, backup_group(state), 700.0, Message::CloseCreate) } else { base.into() }
}

/// Escape dismisses the backup form or its date grid before the account page handles the key.
pub fn escape(state: &State) -> Option<Message> {
    if state.calendar_open {
        Some(Message::ToggleCalendar)
    } else if state.create_open {
        Some(Message::CloseCreate)
    } else {
        None
    }
}

/// How a status line looks: the icon in front and its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// Done and checked: a check in `ok`.
    Ok,
    /// Needs a look (incomplete, uncertain, a warning): an alert in `attention_text`.
    Attention,
    /// Failed: an alert in `error`.
    Error,
    /// Still running: the quiet arrows in `text_3` (they do not move).
    Working,
    /// Ended without a result (cancelled): a cross in `text_3`.
    Quiet,
}

/// One line of state inside a group: icon, `title` in `body` and an optional `detail` in `meta`
/// `text_2`, both wrapping. 44 high for one line of text, growing with it.
fn status_line<'a>(tone: Tone, title: String, detail: Option<String>) -> Element<'a, Message> {
    let (which, color): (Icon, fn(&Theme) -> iced::Color) = match tone {
        Tone::Ok => (Icon::CircleCheck, |t| palette(t).ok),
        Tone::Attention => (Icon::CircleAlert, |t| palette(t).attention_text),
        Tone::Error => (Icon::CircleAlert, |t| palette(t).error),
        Tone::Working => (Icon::RefreshCw, |t| palette(t).text_3),
        Tone::Quiet => (Icon::CircleX, |t| palette(t).text_3),
    };
    let mark = icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(color(t)) });
    let mut lines = column![typo::text(typo::body(), title).wrapping(Wrapping::WordOrGlyph)];
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let mark = container(mark).center_y(typo::line_height(typo::body()));
    wrapped(row![mark, lines.width(Fill)].spacing(space::m()).align_y(Alignment::Start))
}

/// Text that may wrap, inside a group: 16 at the sides, 12 above and below, so one line is as high
/// as a settings row.
fn wrapped<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

/// Buttons side by side; they wrap below each other where the group is too narrow.
fn buttons<'a>(buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(iced::widget::Row::with_children(buttons).spacing(space::s()).wrap().vertical_spacing(space::s()))
        .width(Fill)
        .padding([space::s(), space::l()])
        .into()
}

/// A text button, or its busy look while its request is out.
fn action<'a>(label: &'static str, busy: Option<&'static str>, on_press: Option<Message>) -> Element<'a, Message> {
    match busy {
        Some(working) => widgets::busy_button(working, style::button::secondary),
        None => widgets::text_button(None, label, style::button::secondary, on_press),
    }
}

/// A text field with "Choose…" behind it: the system's chooser, or the path typed by hand.
fn path_control<'a>(field: iced::widget::TextInput<'a, Message>, chooser: Chooser, on: bool) -> Element<'a, Message> {
    let choose =
        widgets::text_button(None, tr("Choose…"), style::button::secondary, on.then_some(Message::Choose(chooser)));
    row![field, choose].spacing(space::s()).align_y(Alignment::Center).into()
}

fn warnings<'a>(rows: &mut Vec<Element<'a, Message>>, warnings: &[String]) {
    rows.extend(warnings.iter().map(|warning| status_line(Tone::Attention, warning.clone(), None)));
}

fn error_row<'a>(rows: &mut Vec<Element<'a, Message>>, state: &State, place: Place) {
    if let Some(text) = state.problem_at(place) {
        rows.push(status_line(Tone::Error, text.to_string(), None));
    }
}

fn backup_group(state: &State) -> Element<'_, Message> {
    let ready = state.pending.is_none() && !state.active_job();
    let submit = || ready.then_some(Message::Create);
    let mut rows = vec![
        row_frame(typo::text(typo::body_strong(), tr("Backup source")), false),
        choice(tr("Server"), state.source == BackupScope::Server, Message::Source(BackupScope::Server)),
        choice(tr("Local"), state.source == BackupScope::Local, Message::Source(BackupScope::Local)),
        setting_row(
            None,
            tr("From date"),
            Some(tr("Leave empty for all mail.").into()),
            row![
                field(tr("YYYY-MM-DD"), &state.since).on_input(Message::Since).on_submit_maybe(submit()),
                widgets::icon_button(Icon::CalendarDays, tr("Choose date"), Some(Message::ToggleCalendar)),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
        ),
        setting_row(
            None,
            tr("Archive name"),
            Some(tr("Saved in the selected folder.").into()),
            field(tr("mail-YYYYMMDD.zip"), &state.file_name).on_input(Message::FileName).on_submit_maybe(submit()),
        ),
        setting_row(
            None,
            tr("Backup folder"),
            Some(tr("Leave empty for the managed folder. Your last choice is saved when you start a backup.").into()),
            path_control(
                field(tr("Managed folder"), &state.destination)
                    .on_input(Message::Destination)
                    .on_submit_maybe(submit()),
                Chooser::Folder,
                true,
            ),
        ),
    ];
    if state.calendar_open {
        rows.insert(4, calendar(state));
    }
    let creating = (state.pending == Some(Pending::Create)).then_some(tr("Starting backup…"));
    rows.push(buttons(vec![action(tr("Create backup"), creating, submit())]));
    error_row(&mut rows, state, Place::Create);
    if let Some(job) = &state.job {
        job_rows(&mut rows, state, job);
    }
    error_row(&mut rows, state, Place::Job);
    widgets::group(tr("Backup"), rows,
        Some(tr("The ZIP is readable without Torro and is not encrypted. Keep it in a private location. Local backup includes only retained original mails.").into()))
}

fn calendar(state: &State) -> Element<'_, Message> {
    let month = state.calendar_month;
    let title = format!("{} {}", torro_gui::datetime::month_name(month.month()), month.year());
    let heading = row![
        widgets::icon_button(Icon::ChevronLeft, tr("Previous month"), Some(Message::PreviousMonth)),
        iced::widget::space().width(Length::Fill),
        typo::text(typo::body_strong(), title),
        iced::widget::space().width(Length::Fill),
        widgets::icon_button(Icon::ChevronRight, tr("Next month"), Some(Message::NextMonth)),
    ]
    .align_y(Alignment::Center);
    let weekdays: Vec<Element<'_, Message>> = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .into_iter()
        .map(|day| {
            container(typo::text(typo::meta(), tr(day)).style(theme::text_2)).width(Length::FillPortion(1)).into()
        })
        .collect();
    let mut grid = column![heading, row(weekdays).spacing(space::xs())].spacing(space::xs());
    for week in torro_gui::datetime::month_grid(month).chunks(7) {
        let cells: Vec<Element<'_, Message>> = week
            .iter()
            .map(|day| {
                let label = day.day().to_string();
                let press = (day.month() == month.month()).then_some(Message::PickDay(*day));
                container(widgets::text_button(None, label, style::button::ghost, press))
                    .width(Length::FillPortion(1))
                    .into()
            })
            .collect();
        grid = grid.push(row(cells).spacing(space::xs()));
    }
    row_frame(grid, true)
}

/// The job: its phase with the counts so far, the archive once there is one, its warnings, and while
/// it runs "Refresh progress" and "Cancel backup" in one row.
fn job_rows<'a>(rows: &mut Vec<Element<'a, Message>>, state: &State, job: &'a BackupJob) {
    let tone = match job.phase {
        BackupPhase::Complete => Tone::Ok,
        BackupPhase::Incomplete => Tone::Attention,
        BackupPhase::Cancelled => Tone::Quiet,
        _ => Tone::Working,
    };
    rows.push(status_line(tone, phase_name(job.phase).into(), Some(counts(job.messages, job.folders, job.bytes))));
    if let Some(archive) = &job.archive {
        rows.push(wrapped(
            typo::text(typo::body_strong(), trf("Archive: {name}", &[("name", archive)]))
                .wrapping(Wrapping::WordOrGlyph),
        ));
    }
    warnings(rows, &job.warnings);
    if job.phase.is_final() {
        return;
    }
    let idle = state.pending.is_none();
    let refreshing = (state.pending == Some(Pending::Poll)).then_some(tr("Refreshing…"));
    let cancelling =
        (state.pending == Some(Pending::Cancel) || job.phase == BackupPhase::Cancelling).then_some(tr("Cancelling…"));
    rows.push(buttons(vec![
        action(tr("Refresh progress"), refreshing, idle.then_some(Message::Poll)),
        action(tr("Cancel backup"), cancelling, idle.then_some(Message::Cancel)),
    ]));
}

/// "Messages: 1,324 · Folders: 12 · Size: 177 MB": one sentence to translate, no plural to get
/// wrong.
fn counts(messages: u64, folders: u64, bytes: u64) -> String {
    trf(
        "Messages: {messages} · Folders: {folders} · Size: {size}",
        &[("messages", &format_count(messages)), ("folders", &format_count(folders)), ("size", &format_bytes(bytes))],
    )
}

fn restore_group<'a>(state: &'a State, account: &str) -> Element<'a, Message> {
    let idle = state.pending.is_none();
    let ready = idle && !state.restore_file.trim().is_empty();
    let archive = field(tr("Archive name or path"), &state.restore_file)
        .on_input_maybe((!state.inspecting()).then_some(Message::RestoreFile))
        .on_submit_maybe(ready.then_some(Message::Verify));
    let mut rows = vec![setting_row(
        None,
        tr("Archive to inspect"),
        None,
        path_control(archive, Chooser::Archive, !state.inspecting()),
    )];
    let checking = matches!(state.pending, Some(Pending::Verify(_))).then_some(tr("Checking…"));
    let testing = matches!(state.pending, Some(Pending::Plan(_))).then_some(tr("Testing…"));
    rows.push(buttons(vec![
        action(tr("Verify archive"), checking, ready.then_some(Message::Verify)),
        action(tr("Run restore test"), testing, ready.then_some(Message::Plan)),
    ]));
    error_row(&mut rows, state, Place::Inspect);
    if let Some(verification) = &state.verification {
        let (tone, title) = match verification.complete {
            true => (Tone::Ok, tr("Archive complete")),
            false => (Tone::Attention, tr("Archive incomplete")),
        };
        let detail = counts(verification.messages, verification.folders, verification.bytes);
        rows.push(status_line(tone, title.into(), Some(detail)));
        warnings(&mut rows, &verification.warnings);
    }
    if let Some(plan) = &state.plan {
        plan_rows(&mut rows, state, plan, account);
    }
    if let Some(result) = &state.result {
        let tone = if result.uncertain == 0 { Tone::Ok } else { Tone::Attention };
        let detail = trf(
            "Added: {added} · Skipped: {skipped} · Uncertain: {uncertain}",
            &[("added", &result.added), ("skipped", &result.skipped), ("uncertain", &result.uncertain)],
        );
        rows.push(status_line(tone, tr("Restore finished").into(), Some(detail)));
        warnings(&mut rows, &result.warnings);
    }
    error_row(&mut rows, state, Place::Apply);
    widgets::group(tr("Restore"), rows,
        Some(tr("The test only reads the target. Restoring adds mail and missing folders; it never deletes existing mail.").into()))
}

/// The restore test: what it found for which archive, the folder table, every question that must be
/// answered before "Restore mail" goes on, and that button.
fn plan_rows<'a>(rows: &mut Vec<Element<'a, Message>>, state: &State, plan: &'a RestorePlan, account: &str) {
    let needs_review = plan.uncertain > 0 || !plan.complete_archive || plan.unconfirmed > 0;
    let summary = trf(
        "To add: {add} · Already present: {present} · Uncertain: {uncertain} · New folders: {folders} · Size: {size}",
        &[
            ("add", &plan.messages_to_add),
            ("present", &plan.already_present),
            ("uncertain", &plan.uncertain),
            ("folders", &plan.folders_to_create),
            ("size", &format_bytes(plan.bytes_to_add)),
        ],
    );
    let title = trf("Restore test of {name}: nothing was changed", &[("name", &plan.archive)]);
    rows.push(status_line(if needs_review { Tone::Attention } else { Tone::Ok }, title, Some(summary)));
    rows.extend(plan.folders.iter().map(folder_row));
    warnings(rows, &plan.warnings);
    if plan.uncertain > 0 {
        let text = tr("Some mail is uncertain. Check the target account first; restoring stays off.");
        rows.push(status_line(Tone::Error, text.into(), None));
    }
    if !plan.complete_archive {
        rows.push(status_line(Tone::Attention, tr("The archive is incomplete.").into(), None));
        rows.push(choice(tr("Do not restore"), !state.allow_incomplete, Message::AllowIncomplete(false)));
        rows.push(choice(tr("Accept incomplete archive"), state.allow_incomplete, Message::AllowIncomplete(true)));
    }
    if plan.unconfirmed > 0 {
        let text = match plan.unconfirmed {
            1 => tr("1 upload of an earlier restore could not be confirmed: it may not have arrived, or arrived changed so that it cannot be recognized. Check the target folder before uploading it again.").to_string(),
            n => trf(
                "{n} uploads of an earlier restore could not be confirmed: they may not have arrived, or arrived changed so that they cannot be recognized. Check the target folders before uploading them again.",
                &[("n", &n)],
            ),
        };
        rows.push(status_line(Tone::Attention, text, None));
        rows.push(choice(tr("Do not upload again"), !state.retry_unconfirmed, Message::RetryUnconfirmed(false)));
        rows.push(choice(
            tr("Upload unconfirmed mail again"),
            state.retry_unconfirmed,
            Message::RetryUnconfirmed(true),
        ));
    }
    rows.push(row_frame(typo::text(typo::body_strong(), tr("Restore confirmation")), false));
    rows.push(choice(tr("Not reviewed"), !state.confirm_restore, Message::ConfirmRestore(false)));
    rows.push(choice(tr("I reviewed the plan"), state.confirm_restore, Message::ConfirmRestore(true)));
    let restoring = (state.pending == Some(Pending::Apply)).then_some(tr("Restoring…"));
    rows.push(buttons(vec![action(tr("Restore mail"), restoring, state.can_apply(account).then_some(Message::Apply))]));
}

/// The archive folder and where it goes, by the names people read (the core decodes the IMAP
/// names, [`RestoreFolderPlan::archive_shown`]).
fn folder_names(folder: &RestoreFolderPlan) -> (&str, String) {
    let target = match folder.create {
        true => trf("Into {target}, new folder", &[("target", &folder.target_shown())]),
        false => trf("Into {target}, existing folder", &[("target", &folder.target_shown())]),
    };
    (folder.archive_shown(), target)
}

/// One folder of the plan: the archive's folder, where it goes ([`folder_names`]), and its counts.
/// An aggregate folder (`\All`) says that it only takes mail found in no other folder.
fn folder_row<'a>(folder: &'a RestoreFolderPlan) -> Element<'a, Message> {
    let (name, target) = folder_names(folder);
    let counts = trf(
        "To add: {add} · Already present: {present} · Uncertain: {uncertain}",
        &[("add", &folder.messages_to_add), ("present", &folder.already_present), ("uncertain", &folder.uncertain)],
    );
    let meta = |text: String| typo::text(typo::meta(), text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
    let mut lines =
        column![typo::text(typo::body_strong(), name).wrapping(Wrapping::WordOrGlyph), meta(target), meta(counts),]
            .spacing(space::xs());
    if folder.aggregate {
        lines = lines.push(meta(trf(
            "Collection folder: only mail that is in no other folder is added here. Copies of mail in other folders: {covered}, not uploaded.",
            &[("covered", &folder.covered)],
        )));
    }
    wrapped(lines)
}

/// The phase as the page and the CLI name it.
fn phase_name(phase: BackupPhase) -> &'static str {
    match phase {
        BackupPhase::Queued => tr("Queued"),
        BackupPhase::Counting => tr("Counting"),
        BackupPhase::Reading => tr("Reading mail"),
        BackupPhase::Verifying => tr("Verifying"),
        BackupPhase::Complete => tr("Complete"),
        BackupPhase::Incomplete => tr("Incomplete"),
        BackupPhase::Cancelling => tr("Cancelling…"),
        BackupPhase::Cancelled => tr("Cancelled"),
    }
}

/// The sample plan of [`Scene::Plan`]: an incomplete archive, an aggregate folder and three
/// unconfirmed uploads, so every question of the page shows.
fn demo_plan(account: &str, archive: &str) -> RestorePlan {
    // Names as IMAP sends them, and as people read them.
    let folder = |(path, shown): (&str, &str), create, add, present, aggregate, covered| RestoreFolderPlan {
        archive_path: path.into(),
        target_path: path.into(),
        archive_display: shown.into(),
        target_display: shown.into(),
        create,
        messages_to_add: add,
        already_present: present,
        uncertain: 0,
        bytes_to_add: add * 140_000,
        aggregate,
        covered,
    };
    RestorePlan {
        id: "demo-plan".into(),
        account: account.into(),
        archive: archive.into(),
        complete_archive: false,
        folders_to_create: 1,
        messages_to_add: 1_903,
        already_present: 6_509,
        uncertain: 0,
        unconfirmed: 3,
        bytes_to_add: 266_420_000,
        folders: vec![
            folder(("INBOX", "INBOX"), false, 1_726, 214, false, 0),
            folder(("Archive/Entw&APw-rfe", "Archive/Entwürfe"), true, 177, 0, false, 0),
            folder(("[Gmail]/All Mail", "[Gmail]/All Mail"), false, 0, 0, true, 6_295),
        ],
        warnings: vec![tr("Folder Spam could not be read during the backup.").into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::mail::{BackupScope, RestorePlan};

    const ACCOUNT: &str = "mail:anna@example.org";

    #[test]
    fn plan_folders_show_the_decoded_names_never_modified_utf7() {
        let plan = demo_plan(ACCOUNT, "anna.zip");
        let drafts = plan.folders.iter().find(|row| row.archive_path == "Archive/Entw&APw-rfe").expect("demo folder");
        let (name, target) = folder_names(drafts);
        assert_eq!(name, "Archive/Entwürfe");
        assert_eq!(target, trf("Into {target}, new folder", &[("target", &"Archive/Entwürfe")]));
        // A core without display names: the wire name rather than nothing.
        let older =
            RestoreFolderPlan { archive_display: String::new(), target_display: String::new(), ..drafts.clone() };
        assert_eq!(folder_names(&older).0, "Archive/Entw&APw-rfe");
    }

    fn job(phase: BackupPhase) -> BackupJob {
        BackupJob {
            id: "first".into(),
            account: ACCOUNT.into(),
            source: BackupScope::Server,
            phase,
            folders: 1,
            messages: 2,
            bytes: 30,
            archive: None,
            warnings: Vec::new(),
        }
    }

    fn plan(complete_archive: bool, unconfirmed: u64) -> RestorePlan {
        RestorePlan {
            id: "plan-1".into(),
            account: ACCOUNT.into(),
            archive: "anna.zip".into(),
            complete_archive,
            folders_to_create: 0,
            messages_to_add: 1,
            already_present: 0,
            uncertain: 0,
            unconfirmed,
            bytes_to_add: 57,
            folders: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn not_found() -> Result<Response, String> {
        Ok(Response::Error { code: "not-found".into(), message: "no such job".into() })
    }

    #[test]
    fn backup_create_uses_the_selected_source_and_inclusive_day() {
        let mut state = State {
            source: BackupScope::Local,
            since: "2026-10-20".into(),
            file_name: "anna.zip".into(),
            ..State::default()
        };
        let action: Action<()> = update(&mut state, ACCOUNT, Message::Create);
        assert_eq!(
            action,
            Action::Ask(Request::MailBackupCreate {
                account: ACCOUNT.into(),
                source: BackupScope::Local,
                since: Some(NaiveDate::from_ymd_opt(2026, 10, 20).expect("date")),
                file_name: Some("anna.zip".into()),
            })
        );
    }

    #[test]
    fn backup_dialog_calendar_sets_the_date_and_closes_after_create() {
        let mut state = State::default();
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::OpenCreate), Action::None);
        assert!(state.create_open);
        let _ = view(&state, ACCOUNT);
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::ToggleCalendar), Action::None);
        assert!(state.calendar_open);
        assert!(matches!(escape(&state), Some(Message::ToggleCalendar)));
        let day = NaiveDate::from_ymd_opt(2026, 10, 20).expect("date");
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::PickDay(day)), Action::None);
        assert_eq!(state.since, "2026-10-20");
        assert!(!state.calendar_open);
        assert!(matches!(
            update::<()>(&mut state, ACCOUNT, Message::Create),
            Action::Ask(Request::MailBackupCreate { since: Some(chosen), .. }) if chosen == day
        ));
        assert!(!state.create_open);
        assert!(escape(&state).is_none());
    }

    #[test]
    fn a_second_create_cannot_replace_the_path_of_a_running_job() {
        let mut state = State {
            file_name: "second.zip".into(),
            destination: "/tmp/new-backups".into(),
            job: Some(job(BackupPhase::Reading)),
            job_directory: Some(PathBuf::from("/tmp/old-backups")),
            ..State::default()
        };
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Create), Action::None);
        assert_eq!(state.job_directory.as_deref(), Some(Path::new("/tmp/old-backups")));
        // Cancelling still keeps the account busy.
        state.job = Some(job(BackupPhase::Cancelling));
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Create), Action::None);
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Cancel), Action::None, "already cancelling");
    }

    #[test]
    fn invalid_date_stops_before_the_daemon_request() {
        let mut state = State { since: "2026-02-30".into(), ..State::default() };
        let action: Action<()> = update(&mut state, ACCOUNT, Message::Create);
        assert_eq!(action, Action::None);
        assert!(state.problem_at(Place::Create).is_some());
    }

    #[test]
    fn selected_folder_is_sent_as_an_absolute_destination() {
        let directory = std::env::temp_dir().join("mail-backups");
        let mut state = State {
            destination: directory.to_string_lossy().into_owned(),
            file_name: "anna.zip".into(),
            ..State::default()
        };
        assert_eq!(
            update::<()>(&mut state, ACCOUNT, Message::Create),
            Action::Ask(Request::MailBackupCreate {
                account: ACCOUNT.into(),
                source: BackupScope::Server,
                since: None,
                file_name: Some(directory.join("anna.zip").to_string_lossy().into_owned()),
            })
        );
        let mut invalid = State { destination: "relative".into(), file_name: "anna.zip".into(), ..State::default() };
        assert_eq!(update::<()>(&mut invalid, ACCOUNT, Message::Create), Action::None);
        assert!(invalid.problem_at(Place::Create).is_some());
        let mut traversal = State {
            destination: directory.to_string_lossy().into_owned(),
            file_name: "../other.zip".into(),
            ..State::default()
        };
        assert_eq!(update::<()>(&mut traversal, ACCOUNT, Message::Create), Action::None);
    }

    #[test]
    fn a_chosen_folder_or_archive_fills_its_field() {
        let mut state = State::default();
        let folder = std::env::temp_dir().join("backups");
        let _: Action<()> = update(&mut state, ACCOUNT, Message::Chosen(Chooser::Folder, folder.clone()));
        assert_eq!(state.destination, folder.to_string_lossy());
        state.plan = Some(plan(true, 0));
        let archive = folder.join("anna.zip");
        let _: Action<()> = update(&mut state, ACCOUNT, Message::Chosen(Chooser::Archive, archive.clone()));
        assert_eq!(state.restore_file, archive.to_string_lossy());
        assert!(state.plan.is_none(), "a new archive drops the old plan");
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Choose(Chooser::Folder)), Action::None);
    }

    #[test]
    fn account_backup_choices_are_loaded_from_persisted_preferences() {
        let preferences = torro_mail::BackupPreferences {
            local: true,
            since: NaiveDate::from_ymd_opt(2020, 1, 2),
            directory: Some("/tmp/backups".into()),
        };
        let state = State::from_preferences(&preferences);
        assert_eq!(state.source, BackupScope::Local);
        assert_eq!(state.since, "2020-01-02");
        assert_eq!(state.destination, "/tmp/backups");
    }

    #[test]
    fn applying_a_restore_requires_the_reviewed_plan_and_explicit_confirmation() {
        let mut state = State { restore_file: "anna.zip".into(), ..State::default() };
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        state.plan = Some(plan(false, 0));
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        state.confirm_restore = true;
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        state.allow_incomplete = true;
        assert_eq!(
            update::<()>(&mut state, ACCOUNT, Message::Apply),
            Action::Ask(Request::MailRestoreApply {
                plan: "plan-1".into(),
                account: ACCOUNT.into(),
                allow_incomplete: true,
                retry_unconfirmed: false,
            })
        );
    }

    #[test]
    fn unconfirmed_uploads_need_their_own_confirmation() {
        let mut state = State {
            restore_file: "anna.zip".into(),
            plan: Some(plan(true, 2)),
            confirm_restore: true,
            ..State::default()
        };
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        let _: Action<()> = update(&mut state, ACCOUNT, Message::RetryUnconfirmed(true));
        assert_eq!(
            update::<()>(&mut state, ACCOUNT, Message::Apply),
            Action::Ask(Request::MailRestoreApply {
                plan: "plan-1".into(),
                account: ACCOUNT.into(),
                allow_incomplete: false,
                retry_unconfirmed: true,
            })
        );
    }

    #[test]
    fn a_finished_restore_uses_up_its_plan_and_confirmations() {
        let mut state = State {
            restore_file: "anna.zip".into(),
            plan: Some(plan(false, 1)),
            confirm_restore: true,
            allow_incomplete: true,
            retry_unconfirmed: true,
            ..State::default()
        };
        assert!(matches!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::Ask(_)));
        let restore = RestoreResult { added: 1, skipped: 0, uncertain: 0, warnings: Vec::new() };
        answered(&mut state, &Ok(Response::MailRestored { restore: restore.clone() }));
        assert_eq!(state.result, Some(restore));
        assert!(state.plan.is_none(), "the core used the plan up");
        assert!(!state.confirm_restore && !state.allow_incomplete && !state.retry_unconfirmed);
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None, "no second press into PlanGone");
    }

    #[test]
    fn a_new_restore_test_asks_every_question_again() {
        let mut state = State {
            restore_file: "anna.zip".into(),
            plan: Some(plan(false, 1)),
            confirm_restore: true,
            allow_incomplete: true,
            retry_unconfirmed: true,
            ..State::default()
        };
        let action: Action<()> = update(&mut state, ACCOUNT, Message::Plan);
        assert!(matches!(action, Action::Ask(Request::MailRestorePlan { .. })));
        assert!(!state.confirm_restore && !state.allow_incomplete && !state.retry_unconfirmed);
    }

    #[test]
    fn a_lost_job_frees_create_backup() {
        let mut state = State { job: Some(job(BackupPhase::Reading)), ..State::default() };
        assert!(poll_request(&mut state, ACCOUNT).is_some());
        answered(&mut state, &not_found());
        assert!(state.job.is_none());
        assert_eq!(state.problem_at(Place::Create), Some(lost_job()));
        assert!(matches!(update::<()>(&mut state, ACCOUNT, Message::Create), Action::Ask(_)));
        // The same for a cancel the core no longer knows.
        let mut cancelled = State { job: Some(job(BackupPhase::Reading)), ..State::default() };
        let _: Action<()> = update(&mut cancelled, ACCOUNT, Message::Cancel);
        answered(&mut cancelled, &not_found());
        assert!(cancelled.job.is_none() && !cancelled.active_job());
        // Other errors keep the job: it may still run.
        let mut failed = State { job: Some(job(BackupPhase::Reading)), ..State::default() };
        let _ = poll_request(&mut failed, ACCOUNT);
        answered(&mut failed, &Err("offline".into()));
        assert!(failed.job.is_some() && failed.problem_at(Place::Job) == Some("offline"));
    }

    #[test]
    fn an_answer_for_another_archive_is_dropped() {
        let mut state = State { restore_file: "first.zip".into(), ..State::default() };
        let _: Action<()> = update(&mut state, ACCOUNT, Message::Plan);
        // The field is locked while the test runs.
        let _: Action<()> = update(&mut state, ACCOUNT, Message::RestoreFile("second.zip".into()));
        assert_eq!(state.restore_file, "first.zip");
        // Should the field change anyway, the answer for the first file does not land on the second.
        state.restore_file = "second.zip".into();
        answered(&mut state, &Ok(Response::MailRestorePlanned { plan: plan(true, 0) }));
        assert!(state.plan.is_none());
        // The plan names its archive; an empty name is the file that was asked for.
        let mut named = State { restore_file: "first.zip".into(), ..State::default() };
        let _: Action<()> = update(&mut named, ACCOUNT, Message::Plan);
        answered(
            &mut named,
            &Ok(Response::MailRestorePlanned { plan: RestorePlan { archive: String::new(), ..plan(true, 0) } }),
        );
        assert_eq!(named.plan.map(|plan| plan.archive), Some("first.zip".into()));
    }

    #[test]
    fn typing_another_archive_keeps_a_backup_error() {
        let mut state = State { since: "nope".into(), ..State::default() };
        let _: Action<()> = update(&mut state, ACCOUNT, Message::Create);
        let _: Action<()> = update(&mut state, ACCOUNT, Message::RestoreFile("anna.zip".into()));
        assert!(state.problem_at(Place::Create).is_some());
    }

    #[test]
    fn every_scene_and_state_builds_its_view() {
        for scene in [Scene::Running, Scene::Plan, Scene::Error, Scene::Done] {
            let state = State::scene(scene, ACCOUNT);
            let _ = view(&state, ACCOUNT);
        }
        let mut busy = State::scene(Scene::Running, ACCOUNT);
        let _: Action<()> = update(&mut busy, ACCOUNT, Message::Cancel);
        let _ = view(&busy, ACCOUNT);
        assert!(State::scene(Scene::Plan, ACCOUNT).plan.is_some_and(|plan| plan.folders.iter().any(|f| f.aggregate)));
    }
}
