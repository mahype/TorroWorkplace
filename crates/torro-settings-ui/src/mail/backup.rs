//! Backup and restore controls of one mail account's own settings page.

use chrono::NaiveDate;
use iced::Element;
use iced::widget::{column, row};
use std::path::{Path, PathBuf};
use torro_core::i18n::tr;
use torro_gui::theme::space;
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope, BackupVerification, RestorePlan, RestoreResult};
use torro_ipc::{Request, Response};

use crate::Action;
use crate::parts::{field, row_frame, setting_row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Create,
    Poll,
    Cancel,
    Verify,
    Plan,
    Apply,
}

#[derive(Debug)]
pub struct State {
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
    pub error: Option<String>,
    pending: Option<Pending>,
    job_directory: Option<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        Self {
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
            error: None,
            pending: None,
            job_directory: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Source(BackupScope),
    Since(String),
    FileName(String),
    Destination(String),
    RestoreFile(String),
    Create,
    Poll,
    Cancel,
    Verify,
    Plan,
    ConfirmRestore(bool),
    AllowIncomplete(bool),
    Apply,
}

impl State {
    pub fn from_preferences(preferences: &torro_mail::BackupPreferences) -> Self {
        Self {
            source: if preferences.local { BackupScope::Local } else { BackupScope::Server },
            since: preferences.since.map_or_else(String::new, |day| day.format("%Y-%m-%d").to_string()),
            destination: preferences.directory.clone().unwrap_or_default(),
            ..Self::default()
        }
    }

    fn can_apply(&self, account: &str) -> bool {
        self.pending.is_none()
            && self.confirm_restore
            && self.plan.as_ref().is_some_and(|plan| {
                plan.account == account && plan.uncertain == 0 && (plan.complete_archive || self.allow_incomplete)
            })
    }
}

pub fn update<A>(state: &mut State, account: &str, message: Message) -> Action<A> {
    match message {
        Message::Source(source) => state.source = source,
        Message::Since(since) => state.since = since,
        Message::FileName(file_name) => state.file_name = file_name,
        Message::Destination(destination) => state.destination = destination,
        Message::RestoreFile(file) => {
            state.restore_file = file;
            state.verification = None;
            state.plan = None;
            state.result = None;
            state.confirm_restore = false;
            state.allow_incomplete = false;
        }
        Message::ConfirmRestore(confirmed) => state.confirm_restore = confirmed,
        Message::AllowIncomplete(allowed) => state.allow_incomplete = allowed,
        Message::Create if state.pending.is_none() => {
            let since = if state.since.trim().is_empty() {
                None
            } else {
                match NaiveDate::parse_from_str(state.since.trim(), "%Y-%m-%d") {
                    Ok(day) => Some(day),
                    Err(_) => {
                        state.error = Some(tr("Enter a valid date as YYYY-MM-DD.").into());
                        return Action::None;
                    }
                }
            };
            if !state.file_name.trim().is_empty() && !valid_base_name(state.file_name.trim()) {
                state.error = Some(tr("Use a simple .zip archive name without folders.").into());
                return Action::None;
            }
            state.error = None;
            state.pending = Some(Pending::Create);
            let destination = state.destination.trim();
            let file_name = if destination.is_empty() {
                state.job_directory = None;
                (!state.file_name.trim().is_empty()).then(|| state.file_name.trim().to_string())
            } else {
                if !Path::new(destination).is_absolute() || state.file_name.trim().is_empty() {
                    state.pending = None;
                    state.error = Some(tr("Choose an absolute backup folder and an archive name.").into());
                    return Action::None;
                }
                state.job_directory = Some(PathBuf::from(destination));
                Some(Path::new(destination).join(state.file_name.trim()).to_string_lossy().into_owned())
            };
            return Action::Ask(Request::MailBackupCreate {
                account: account.into(),
                source: state.source,
                since,
                file_name,
            });
        }
        Message::Poll if state.pending.is_none() => {
            if let Some(job) = &state.job {
                state.pending = Some(Pending::Poll);
                return Action::Ask(Request::MailBackupStatus { account: account.into(), job: job.id.clone() });
            }
        }
        Message::Cancel if state.pending.is_none() => {
            if let Some(job) = &state.job
                && !matches!(job.phase, BackupPhase::Complete | BackupPhase::Incomplete | BackupPhase::Cancelled)
            {
                state.pending = Some(Pending::Cancel);
                return Action::Ask(Request::MailBackupCancel { account: account.into(), job: job.id.clone() });
            }
        }
        Message::Verify if state.pending.is_none() => {
            if !state.restore_file.trim().is_empty() {
                state.pending = Some(Pending::Verify);
                state.error = None;
                return Action::Ask(Request::MailBackupVerify {
                    file: state.restore_file.trim().into(),
                    account: account.into(),
                });
            }
        }
        Message::Plan if state.pending.is_none() => {
            if !state.restore_file.trim().is_empty() {
                state.pending = Some(Pending::Plan);
                state.plan = None;
                state.result = None;
                state.confirm_restore = false;
                state.error = None;
                return Action::Ask(Request::MailRestorePlan {
                    file: state.restore_file.trim().into(),
                    account: account.into(),
                });
            }
        }
        Message::Apply if state.can_apply(account) => {
            let Some(plan) = &state.plan else { return Action::None };
            state.pending = Some(Pending::Apply);
            state.error = None;
            return Action::Ask(Request::MailRestoreApply {
                plan: plan.id.clone(),
                account: account.into(),
                allow_incomplete: state.allow_incomplete,
            });
        }
        _ => {}
    }
    Action::None
}

fn valid_base_name(name: &str) -> bool {
    !name.starts_with('.')
        && name.len() <= 120
        && name.ends_with(".zip")
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn answered(state: &mut State, answer: &Result<Response, String>) {
    let Some(pending) = state.pending.take() else { return };
    match answer {
        Ok(Response::MailBackupJob { job }) if matches!(pending, Pending::Create | Pending::Poll | Pending::Cancel) => {
            if let Some(archive) = &job.archive
                && state.restore_file.is_empty()
            {
                state.restore_file = state.job_directory.as_ref().map_or_else(
                    || archive.clone(),
                    |directory| directory.join(archive).to_string_lossy().into_owned(),
                );
            }
            state.job = Some(job.clone());
            state.error = None;
        }
        Ok(Response::MailBackupVerified { verification }) if pending == Pending::Verify => {
            state.verification = Some(verification.clone());
            state.error = None;
        }
        Ok(Response::MailRestorePlanned { plan }) if pending == Pending::Plan => {
            state.plan = Some(plan.clone());
            state.error = None;
        }
        Ok(Response::MailRestored { restore }) if pending == Pending::Apply => {
            state.result = Some(restore.clone());
            state.confirm_restore = false;
            state.error = None;
        }
        Ok(Response::Error { message, .. }) => state.error = Some(message.clone()),
        Err(error) => state.error = Some(error.clone()),
        _ => state.error = Some(tr("The mail backup answer was unexpected. Try again.").into()),
    }
}

/// Called by the mail window's ordinary ten-second tick while this account page is open.
pub fn poll_request(state: &mut State, account: &str) -> Option<Request> {
    let job = state.job.as_ref()?;
    if state.pending.is_some()
        || matches!(job.phase, BackupPhase::Complete | BackupPhase::Incomplete | BackupPhase::Cancelled)
    {
        return None;
    }
    state.pending = Some(Pending::Poll);
    Some(Request::MailBackupStatus { account: account.into(), job: job.id.clone() })
}

pub fn view<'a>(state: &'a State, account: &str) -> Element<'a, Message> {
    column![backup_group(state), restore_group(state, account)].spacing(layout::section_gap()).into()
}

fn backup_group(state: &State) -> Element<'_, Message> {
    let source = widgets::segmented(
        [(tr("Server"), Message::Source(BackupScope::Server)), (tr("Local"), Message::Source(BackupScope::Local))],
        if state.source == BackupScope::Server { 0 } else { 1 },
    );
    let mut rows = vec![
        setting_row(None, tr("Backup source"), None, source),
        setting_row(
            None,
            tr("From date"),
            Some(tr("Leave empty for all mail.").into()),
            field("YYYY-MM-DD", &state.since).on_input(Message::Since),
        ),
        setting_row(
            None,
            tr("Archive name"),
            Some(tr("Saved in the selected folder.").into()),
            field("mail-YYYYMMDD.zip", &state.file_name).on_input(Message::FileName),
        ),
        setting_row(
            None,
            tr("Backup folder"),
            Some(tr("Leave empty for the managed folder. Your last choice is saved when you start a backup.").into()),
            field("/path/to/backups", &state.destination).on_input(Message::Destination),
        ),
    ];
    let start = widgets::text_button(
        None,
        tr("Create backup"),
        style::button::primary,
        state.pending.is_none().then_some(Message::Create),
    );
    rows.push(row_frame(start, false));
    if let Some(job) = &state.job {
        rows.push(row_frame(
            typo::text(
                typo::body(),
                format!(
                    "{} · {} {} · {} {}",
                    phase_name(job.phase),
                    job.messages,
                    tr("messages"),
                    job.bytes,
                    tr("bytes")
                ),
            ),
            false,
        ));
        if let Some(archive) = &job.archive {
            rows.push(row_frame(typo::text(typo::body_strong(), format!("{}: {archive}", tr("Archive"))), false));
        }
        for warning in &job.warnings {
            rows.push(row_frame(typo::text(typo::body(), warning.as_str()), false));
        }
        if !matches!(job.phase, BackupPhase::Complete | BackupPhase::Incomplete | BackupPhase::Cancelled) {
            let refresh = widgets::text_button(
                None,
                tr("Refresh progress"),
                style::button::secondary,
                state.pending.is_none().then_some(Message::Poll),
            );
            let cancel = widgets::text_button(
                None,
                tr("Cancel backup"),
                style::button::secondary,
                state.pending.is_none().then_some(Message::Cancel),
            );
            rows.push(row_frame(row![refresh, cancel].spacing(space::s()), false));
        }
    }
    widgets::group(tr("Backup"), rows,
        Some(tr("The ZIP is readable without Torro and is not encrypted. Keep it in a private location. Local backup includes only retained original mails.").into()))
}

fn restore_group<'a>(state: &'a State, account: &str) -> Element<'a, Message> {
    let mut rows = vec![setting_row(
        None,
        tr("Archive to inspect"),
        None,
        field("backup.zip", &state.restore_file).on_input(Message::RestoreFile),
    )];
    let ready = state.pending.is_none() && !state.restore_file.trim().is_empty();
    let verify =
        widgets::text_button(None, tr("Verify archive"), style::button::secondary, ready.then_some(Message::Verify));
    let plan =
        widgets::text_button(None, tr("Run restore test"), style::button::secondary, ready.then_some(Message::Plan));
    rows.push(row_frame(row![verify, plan].spacing(space::s()), false));
    if let Some(verification) = &state.verification {
        rows.push(row_frame(
            typo::text(
                typo::body(),
                format!(
                    "{} · {} {} · {} {}",
                    if verification.complete { tr("Complete") } else { tr("Incomplete") },
                    verification.folders,
                    tr("Folders"),
                    verification.messages,
                    tr("messages")
                ),
            ),
            false,
        ));
        for warning in &verification.warnings {
            rows.push(row_frame(typo::text(typo::body(), warning.as_str()), false));
        }
    }
    if let Some(plan) = &state.plan {
        rows.push(row_frame(
            typo::text(
                typo::body_strong(),
                format!(
                    "{}: {} · {}: {} · {}: {}",
                    tr("Add"),
                    plan.messages_to_add,
                    tr("Already present"),
                    plan.already_present,
                    tr("Uncertain"),
                    plan.uncertain
                ),
            ),
            false,
        ));
        for folder in &plan.folders {
            let action = if folder.create { tr("Create folder") } else { tr("Use folder") };
            rows.push(row_frame(
                typo::text(
                    typo::body(),
                    format!(
                        "{} → {} · {} · {}: {} · {}: {} · {}: {}",
                        folder.archive_path,
                        folder.target_path,
                        action,
                        tr("Add"),
                        folder.messages_to_add,
                        tr("Already present"),
                        folder.already_present,
                        tr("Uncertain"),
                        folder.uncertain
                    ),
                ),
                false,
            ));
        }
        for warning in &plan.warnings {
            rows.push(row_frame(typo::text(typo::body(), warning.as_str()), false));
        }
        let reviewed = widgets::segmented(
            [
                (tr("Not reviewed"), Message::ConfirmRestore(false)),
                (tr("I reviewed the plan"), Message::ConfirmRestore(true)),
            ],
            usize::from(state.confirm_restore),
        );
        rows.push(setting_row(None, tr("Restore confirmation"), None, reviewed));
        if !plan.complete_archive {
            let incomplete = widgets::segmented(
                [
                    (tr("Do not restore"), Message::AllowIncomplete(false)),
                    (tr("Accept incomplete archive"), Message::AllowIncomplete(true)),
                ],
                usize::from(state.allow_incomplete),
            );
            rows.push(setting_row(None, tr("Incomplete archive"), None, incomplete));
        }
        let apply = widgets::text_button(
            None,
            tr("Restore mail"),
            style::button::primary,
            state.can_apply(account).then_some(Message::Apply),
        );
        rows.push(row_frame(apply, false));
    }
    if let Some(result) = &state.result {
        rows.push(row_frame(
            typo::text(
                typo::body_strong(),
                format!(
                    "{}: {} · {}: {} · {}: {}",
                    tr("Added"),
                    result.added,
                    tr("Skipped"),
                    result.skipped,
                    tr("Uncertain"),
                    result.uncertain
                ),
            ),
            false,
        ));
        for warning in &result.warnings {
            rows.push(row_frame(typo::text(typo::body(), warning.as_str()), false));
        }
    }
    if let Some(error) = &state.error {
        rows.push(row_frame(typo::text(typo::body(), error.as_str()), false));
    }
    widgets::group(tr("Restore"), rows,
        Some(tr("The test only reads the target. Restoring adds mail and missing folders; it never deletes existing mail.").into()))
}

fn phase_name(phase: BackupPhase) -> &'static str {
    match phase {
        BackupPhase::Queued => tr("Queued"),
        BackupPhase::Counting => tr("Counting"),
        BackupPhase::Reading => tr("Reading mail"),
        BackupPhase::Verifying => tr("Verifying"),
        BackupPhase::Complete => tr("Complete"),
        BackupPhase::Incomplete => tr("Incomplete"),
        BackupPhase::Cancelled => tr("Cancelled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::mail::{BackupScope, RestorePlan};

    const ACCOUNT: &str = "mail:anna@example.org";

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
            Action::Ask(torro_ipc::Request::MailBackupCreate {
                account: ACCOUNT.into(),
                source: BackupScope::Local,
                since: Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 20).expect("date")),
                file_name: Some("anna.zip".into()),
            })
        );
    }

    #[test]
    fn invalid_date_stops_before_the_daemon_request() {
        let mut state = State { since: "2026-02-30".into(), ..State::default() };
        let action: Action<()> = update(&mut state, ACCOUNT, Message::Create);
        assert_eq!(action, Action::None);
        assert!(state.error.is_some());
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
        assert!(invalid.error.is_some());
        let mut traversal = State {
            destination: directory.to_string_lossy().into_owned(),
            file_name: "../other.zip".into(),
            ..State::default()
        };
        assert_eq!(update::<()>(&mut traversal, ACCOUNT, Message::Create), Action::None);
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
        state.plan = Some(RestorePlan {
            id: "plan-1".into(),
            account: ACCOUNT.into(),
            complete_archive: false,
            folders_to_create: 0,
            messages_to_add: 1,
            already_present: 0,
            uncertain: 0,
            bytes_to_add: 57,
            folders: Vec::new(),
            warnings: Vec::new(),
        });
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        state.confirm_restore = true;
        assert_eq!(update::<()>(&mut state, ACCOUNT, Message::Apply), Action::None);
        state.allow_incomplete = true;
        assert_eq!(
            update::<()>(&mut state, ACCOUNT, Message::Apply),
            Action::Ask(torro_ipc::Request::MailRestoreApply {
                plan: "plan-1".into(),
                account: ACCOUNT.into(),
                allow_incomplete: true,
            })
        );
    }
}
