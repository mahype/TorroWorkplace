//! Where the sync of one mail account stands (issue #422, `mail.sync_status`): the run of all
//! folders under way with its phase and progress, the last run, and every folder with what the last
//! run that reached it did. The sync ([`super::sync`]) reports each step here; the service answers
//! `mail.sync_status` from it.
//!
//! Only in memory, never on disk: after the core started it is empty until the first run begins
//! (right away). The folders' records are kept by id across runs, so a run that ends early still
//! shows what the one before did with the folders it did not reach.
//!
//! Every change raises the account's version and wakes the service's shared [`Notify`]: a waiting
//! `mail.sync_status` answers when the sum of the versions it watches changed. The lock is a plain
//! mutex, taken briefly and never across an `.await`.
//!
//! Each mailbox has its own: an account opened anew (its settings saved, its password replaced)
//! gets one that takes over the folders and the last run of the one before
//! ([`SyncProgress::after`]), while the old mailbox's run, which may still go on for a moment,
//! writes only into its own.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use std::time::Instant;

use chrono::Utc;
use tokio::sync::Notify;
use torro_core::i18n::tr;
use torro_ipc::mail::{
    MailFolderSteps, MailFolderSync, MailFolderSyncState, MailSyncLastRun, MailSyncPhase, MailSyncRun, MailSyncStatus,
    MailSyncTrigger,
};

use super::service::MailError;

/// The sync state of one account; see the module documentation.
pub(crate) struct SyncProgress {
    state: StdMutex<State>,
    /// Raised by every change ([`SyncProgress::change`]).
    version: AtomicU64,
    /// The service's, shared by every account.
    wake: Arc<Notify>,
}

#[derive(Default)]
struct State {
    /// Runs of all folders waiting for the account's one sync at a time.
    queued: u32,
    run: Option<Run>,
    last_run: Option<MailSyncLastRun>,
    /// In the order of the last listing, the inbox first; `stored` is filled in when answering.
    folders: Vec<MailFolderSync>,
    /// The folder being synced now, by a run or by the inbox sync of the IDLE connection.
    current: Option<Current>,
    /// Every phase a folder went into with how many items it had, for the sync's tests.
    #[cfg(test)]
    phases: Vec<(MailSyncPhase, u32)>,
}

/// The run of all folders under way.
struct Run {
    shown: MailSyncRun,
    began: Instant,
    listing_ms: Option<u64>,
    finishing: Option<Instant>,
    folders_done: u32,
    /// Listed folders that vanished from the server during the run: the ones after them count on
    /// from a smaller number.
    gone: u32,
}

/// The folder being synced and the step it is at.
struct Current {
    folder: String,
    began: Instant,
    phase: MailSyncPhase,
    step: Instant,
}

impl SyncProgress {
    pub(crate) fn new(wake: Arc<Notify>) -> SyncProgress {
        SyncProgress { state: StdMutex::default(), version: AtomicU64::new(0), wake }
    }

    /// The state of an account opened anew: the folders and the last run of `before`, the account's
    /// state until now. A run of `before` still under way counts as stopped, its folder as
    /// interrupted and those it did not reach as not reached. The version goes on from `before`'s,
    /// so a waiting `mail.sync_status` sees a change.
    pub(crate) fn after(before: &SyncProgress) -> SyncProgress {
        let taken = SyncProgress::new(Arc::clone(&before.wake));
        {
            let old = before.lock();
            let mut state = taken.lock();
            state.folders = old.folders.clone();
            state.last_run = old.last_run.clone();
            if let Some(run) = &old.run {
                state.current = old.current.as_ref().map(|current| Current {
                    folder: current.folder.clone(),
                    began: current.began,
                    phase: current.phase,
                    step: current.step,
                });
                state.run = Some(Run {
                    shown: run.shown.clone(),
                    began: run.began,
                    listing_ms: run.listing_ms,
                    finishing: run.finishing,
                    folders_done: run.folders_done,
                    gone: run.gone,
                });
            } else if let Some(current) = &old.current {
                // An inbox sync of the IDLE connection was cut off.
                if let Some(record) = state.folders.iter_mut().find(|record| record.folder == current.folder) {
                    record.state = MailFolderSyncState::Interrupted;
                }
            }
        }
        if taken.lock().run.is_some() {
            taken.end(Some(tr("The sync was stopped before it ended.").into()));
        }
        taken.version.store(before.version().wrapping_add(1), Ordering::Release);
        taken
    }

    /// Grows with every change.
    pub(crate) fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// The state for `account`, every folder with `stored` 0 (the store knows that).
    pub(crate) fn status(&self, account: &str) -> MailSyncStatus {
        let state = self.lock();
        MailSyncStatus {
            account: account.to_string(),
            running: state.run.as_ref().map(|run| run.shown.clone()),
            queued: state.queued > 0,
            last_run: state.last_run.clone(),
            folders: state.folders.clone(),
        }
    }

    /// A run of all folders waits for its turn until the guard is dropped.
    pub(crate) fn queue(&self) -> Queued<'_> {
        self.change(|state| state.queued += 1);
        Queued { progress: self }
    }

    /// A run of all folders begins: it lists the folders first. It ends with [`RunGuard::end`];
    /// dropped before (the worker stopped), it ends as stopped.
    pub(crate) fn begin(&self, trigger: MailSyncTrigger) -> RunGuard<'_> {
        let started = Utc::now();
        self.change(|state| {
            state.run = Some(Run {
                shown: MailSyncRun {
                    trigger,
                    started,
                    phase: MailSyncPhase::Listing,
                    folder_index: 0,
                    folder_count: 0,
                    folder: None,
                    done: 0,
                    of: 0,
                },
                began: Instant::now(),
                listing_ms: None,
                finishing: None,
                folders_done: 0,
                gone: 0,
            });
        });
        RunGuard { progress: self, ended: false }
    }

    /// The run listed `folders` (id and name), in the order it syncs them: they wait. Records of
    /// folders no longer listed go; the others keep what the last run did.
    pub(crate) fn listed(&self, folders: &[(String, String)]) {
        self.change(|state| {
            let mut before = std::mem::take(&mut state.folders);
            state.folders = folders
                .iter()
                .map(|(id, name)| {
                    let kept = before.iter().position(|record| record.folder == *id).map(|at| before.swap_remove(at));
                    let mut record = kept.unwrap_or_else(|| MailFolderSync {
                        folder: id.clone(),
                        name: String::new(),
                        on_server: None,
                        stored: 0,
                        last_synced: None,
                        duration_ms: None,
                        steps: MailFolderSteps::default(),
                        state: MailFolderSyncState::Waiting,
                    });
                    record.name = name.clone();
                    record.state = MailFolderSyncState::Waiting;
                    record
                })
                .collect();
            if let Some(run) = state.run.as_mut() {
                run.listing_ms = Some(millis(run.began));
                run.shown.folder_count = count(folders.len());
            }
        });
    }

    /// The sync of `folder` begins, the `index`-th (from 0) of the run, or outside a run (the inbox
    /// sync of the IDLE connection: `None`). Only a folder the runs listed has a record. It ends with
    /// [`FolderGuard::end`] or [`FolderGuard::gone`]; dropped before (the worker stopped), it ends
    /// as interrupted.
    pub(crate) fn folder_begins(&self, index: Option<usize>, folder: &str) -> FolderGuard<'_> {
        self.begin_folder(index, folder);
        FolderGuard { progress: self, ended: false }
    }

    fn begin_folder(&self, index: Option<usize>, folder: &str) {
        self.change(|state| {
            let now = Instant::now();
            state.current =
                Some(Current { folder: folder.to_string(), began: now, phase: MailSyncPhase::Checking, step: now });
            if let Some(record) = state.record(folder) {
                record.state = MailFolderSyncState::Running;
                record.steps = MailFolderSteps::default();
            }
            if let (Some(run), Some(index)) = (state.run.as_mut(), index) {
                run.shown.folder_index = count(index + 1).saturating_sub(run.gone);
                run.shown.folder = Some(folder.to_string());
                run.shown.phase = MailSyncPhase::Checking;
                run.shown.done = 0;
                run.shown.of = 0;
            }
        });
    }

    /// The server reported `exists` messages in the folder being synced.
    pub(crate) fn selected(&self, exists: u32) {
        self.change(|state| {
            if let Some(folder) = state.current.as_ref().map(|current| current.folder.clone())
                && let Some(record) = state.record(&folder)
            {
                record.on_server = Some(exists);
            }
        });
    }

    /// The folder being synced goes on to `phase`, which handles `of` items.
    pub(crate) fn phase(&self, phase: MailSyncPhase, of: usize) {
        self.change(|state| {
            state.close_step();
            if let Some(current) = state.current.as_mut() {
                current.phase = phase;
                current.step = Instant::now();
            }
            if let Some(run) = state.run.as_mut() {
                run.shown.phase = phase;
                run.shown.done = 0;
                run.shown.of = count(of);
            }
            #[cfg(test)]
            state.phases.push((phase, count(of)));
        });
    }

    /// The phases folders went into, with how many items each had.
    #[cfg(test)]
    pub(crate) fn phases(&self) -> Vec<(MailSyncPhase, u32)> {
        self.lock().phases.clone()
    }

    /// `by` more items of the phase are done.
    pub(crate) fn advance(&self, by: usize) {
        self.change(|state| {
            if let Some(run) = state.run.as_mut() {
                run.shown.done = run.shown.done.saturating_add(count(by)).min(run.shown.of);
            }
        });
    }

    /// The folder being synced ended as `outcome`: [`MailFolderSyncState::UpToDate`],
    /// [`MailFolderSyncState::Synced`], [`MailFolderSyncState::Interrupted`] or
    /// [`MailFolderSyncState::Failed`].
    fn folder_ends(&self, outcome: MailFolderSyncState) {
        self.change(|state| {
            state.close_step();
            let Some(current) = state.current.take() else { return };
            let synced = matches!(outcome, MailFolderSyncState::UpToDate | MailFolderSyncState::Synced);
            if let Some(record) = state.record(&current.folder) {
                record.state = outcome;
                record.duration_ms = Some(millis(current.began));
                if synced {
                    record.last_synced = Some(Utc::now());
                }
            }
            if let Some(run) = state.run.as_mut()
                && synced
            {
                run.folders_done += 1;
            }
        });
    }

    /// The folder being synced vanished from the server meanwhile: the run skips it, and it has no
    /// record any more.
    fn folder_gone(&self) {
        self.change(|state| {
            let Some(current) = state.current.take() else { return };
            state.folders.retain(|record| record.folder != current.folder);
            if let Some(run) = state.run.as_mut() {
                run.shown.folder_count = run.shown.folder_count.saturating_sub(1);
                run.shown.folder_index = run.shown.folder_index.min(run.shown.folder_count);
                run.gone += 1;
            }
        });
    }

    /// Every folder is done: the run cleans up and reads the quota.
    pub(crate) fn finishing(&self) {
        self.change(|state| {
            if let Some(run) = state.run.as_mut() {
                run.finishing = Some(Instant::now());
                // Past the last folder, also when the last one vanished: never "3 of 2".
                run.shown.folder_index = run.shown.folder_count;
                run.shown.phase = MailSyncPhase::Finishing;
                run.shown.folder = None;
                run.shown.done = 0;
                run.shown.of = 0;
            }
        });
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the state, raises the version and wakes the waiting answers.
    fn change(&self, edit: impl FnOnce(&mut State)) {
        edit(&mut self.lock());
        self.version.fetch_add(1, Ordering::AcqRel);
        self.wake.notify_waiters();
    }

    /// Ends the run under way with `problem` (`None`: without an error).
    fn end(&self, problem: Option<String>) {
        self.change(|state| {
            let Some(run) = state.run.take() else { return };
            let failed_at = failed_at(&run, problem.is_some());
            // A folder still running was cut off; those the run did not reach wait for the next.
            if state.current.take().is_some() {
                for record in &mut state.folders {
                    if record.state == MailFolderSyncState::Running {
                        record.state = MailFolderSyncState::Interrupted;
                    }
                }
            }
            for record in &mut state.folders {
                if record.state == MailFolderSyncState::Waiting {
                    record.state = MailFolderSyncState::NotReached;
                }
            }
            state.last_run = Some(MailSyncLastRun {
                trigger: run.shown.trigger,
                started: run.shown.started,
                finished: Utc::now(),
                ok: problem.is_none(),
                problem,
                folders_done: run.folders_done,
                folder_count: run.shown.folder_count,
                failed_at,
                listing_ms: run.listing_ms.unwrap_or_else(|| millis(run.began)),
                finishing_ms: run.finishing.map_or(0, millis),
            });
        });
    }
}

impl State {
    fn record(&mut self, folder: &str) -> Option<&mut MailFolderSync> {
        self.folders.iter_mut().find(|record| record.folder == folder)
    }

    /// Adds the time of the current step to the folder's steps.
    fn close_step(&mut self) {
        let Some(current) = self.current.as_mut() else { return };
        let spent = millis(current.step);
        current.step = Instant::now();
        let (folder, phase) = (current.folder.clone(), current.phase);
        let Some(record) = self.record(&folder) else { return };
        let steps = &mut record.steps;
        let slot = match phase {
            MailSyncPhase::Checking => &mut steps.checking_ms,
            MailSyncPhase::Headers => &mut steps.headers_ms,
            MailSyncPhase::Texts => &mut steps.texts_ms,
            MailSyncPhase::Sizes => &mut steps.sizes_ms,
            MailSyncPhase::Listing | MailSyncPhase::Finishing => return,
        };
        *slot = slot.saturating_add(spent);
    }
}

/// A run waiting for its turn ([`SyncProgress::queue`]).
pub(crate) struct Queued<'a> {
    progress: &'a SyncProgress,
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        self.progress.change(|state| state.queued = state.queued.saturating_sub(1));
    }
}

/// A folder being synced ([`SyncProgress::folder_begins`]).
pub(crate) struct FolderGuard<'a> {
    progress: &'a SyncProgress,
    ended: bool,
}

impl FolderGuard<'_> {
    /// The folder's sync ended as `outcome` (see [`SyncProgress::folder_ends`]).
    pub(crate) fn end(mut self, outcome: MailFolderSyncState) {
        self.ended = true;
        self.progress.folder_ends(outcome);
    }

    /// The folder vanished from the server meanwhile (see [`SyncProgress::folder_gone`]).
    pub(crate) fn gone(mut self) {
        self.ended = true;
        self.progress.folder_gone();
    }
}

impl Drop for FolderGuard<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.progress.folder_ends(MailFolderSyncState::Interrupted);
        }
    }
}

/// A run under way ([`SyncProgress::begin`]).
pub(crate) struct RunGuard<'a> {
    progress: &'a SyncProgress,
    ended: bool,
}

impl RunGuard<'_> {
    /// The run ended with `result`.
    pub(crate) fn end(mut self, result: Result<(), &MailError>) {
        self.ended = true;
        self.progress.end(result.err().map(MailError::message));
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.progress.end(Some(tr("The sync was stopped before it ended.").into()));
        }
    }
}

/// The folder a run ending with `problem` was at: none while it listed or finished.
fn failed_at(run: &Run, problem: bool) -> Option<u32> {
    let at = run.shown.folder_index;
    (problem && run.finishing.is_none() && at > 0).then_some(at)
}

fn millis(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress() -> SyncProgress {
        SyncProgress::new(Arc::new(Notify::new()))
    }

    fn listed(progress: &SyncProgress) {
        progress.listed(&[("f:1".into(), "Inbox".into()), ("f:2".into(), "Archive".into())]);
    }

    #[test]
    fn a_run_counts_its_folders_and_steps() {
        let progress = progress();
        let before = progress.version();
        let run = progress.begin(MailSyncTrigger::Manual);
        assert!(progress.version() > before, "a change raises the version");
        listed(&progress);
        let folder = progress.folder_begins(Some(0), "f:1");
        progress.selected(12);
        progress.phase(MailSyncPhase::Headers, 10);
        progress.advance(4);
        let shown = progress.status("mail:a").running.expect("running");
        assert_eq!((shown.folder_index, shown.folder_count, shown.done, shown.of), (1, 2, 4, 10));
        assert_eq!(shown.phase, MailSyncPhase::Headers);
        folder.end(MailFolderSyncState::Synced);
        run.end(Ok(()));
        let status = progress.status("mail:a");
        assert_eq!(status.running, None);
        let last = status.last_run.expect("last run");
        assert!(last.ok);
        assert_eq!((last.folders_done, last.folder_count), (1, 2));
        assert_eq!(status.folders[0].on_server, Some(12));
        assert_eq!(status.folders[0].state, MailFolderSyncState::Synced);
        assert!(status.folders[0].last_synced.is_some());
        assert_eq!(status.folders[1].state, MailFolderSyncState::NotReached);
    }

    #[test]
    fn a_dropped_run_ends_as_stopped() {
        let progress = progress();
        {
            let _run = progress.begin(MailSyncTrigger::Start);
            listed(&progress);
            let _folder = progress.folder_begins(Some(0), "f:1");
        }
        let status = progress.status("mail:a");
        let last = status.last_run.expect("last run");
        assert!(!last.ok);
        assert_eq!(last.problem.as_deref(), Some(tr("The sync was stopped before it ended.")));
        assert_eq!(last.failed_at, Some(1), "stopped in the first folder");
        assert_eq!(status.folders[0].state, MailFolderSyncState::Interrupted);
        assert_eq!(status.folders[1].state, MailFolderSyncState::NotReached);
    }

    #[test]
    fn queued_holds_while_the_guard_lives() {
        let progress = progress();
        let queued = progress.queue();
        assert!(progress.status("mail:a").queued);
        drop(queued);
        assert!(!progress.status("mail:a").queued);
    }

    #[test]
    fn records_outlive_a_run_and_follow_the_listing() {
        let progress = progress();
        let run = progress.begin(MailSyncTrigger::Start);
        listed(&progress);
        progress.folder_begins(Some(0), "f:1").end(MailFolderSyncState::UpToDate);
        run.end(Ok(()));
        let run = progress.begin(MailSyncTrigger::Scheduled);
        progress.listed(&[("f:1".into(), "Inbox".into())]);
        let status = progress.status("mail:a");
        assert_eq!(status.folders.len(), 1, "a folder no longer listed goes");
        assert!(status.folders[0].last_synced.is_some(), "the record keeps what the last run did");
        assert_eq!(status.folders[0].state, MailFolderSyncState::Waiting);
        run.end(Ok(()));
    }

    /// A folder that vanished during the run is skipped: the ones after it count on without a gap,
    /// never past the total.
    #[test]
    fn a_vanished_folder_leaves_no_gap_in_the_count() {
        let progress = progress();
        let run = progress.begin(MailSyncTrigger::Manual);
        progress.listed(&[
            ("f:1".into(), "Inbox".into()),
            ("f:2".into(), "Gone".into()),
            ("f:3".into(), "Last".into()),
        ]);
        progress.folder_begins(Some(0), "f:1").end(MailFolderSyncState::Synced);
        progress.folder_begins(Some(1), "f:2").gone();
        let last = progress.folder_begins(Some(2), "f:3");
        let shown = progress.status("mail:a").running.expect("running");
        assert_eq!((shown.folder_index, shown.folder_count), (2, 2));
        last.end(MailFolderSyncState::Synced);
        progress.finishing();
        let shown = progress.status("mail:a").running.expect("running");
        assert_eq!((shown.folder_index, shown.folder_count), (2, 2));
        run.end(Ok(()));
        let status = progress.status("mail:a");
        assert_eq!(status.folders.len(), 2, "the vanished folder has no record");
        assert_eq!(status.last_run.map(|last| (last.folders_done, last.folder_count)), Some((2, 2)));
    }

    /// A dropped folder guard (the inbox sync of the IDLE connection cut off) leaves no folder
    /// running.
    #[test]
    fn a_dropped_folder_ends_as_interrupted() {
        let progress = progress();
        progress.begin(MailSyncTrigger::Start).end(Ok(()));
        listed(&progress);
        drop(progress.folder_begins(None, "f:1"));
        assert_eq!(progress.status("mail:a").folders[0].state, MailFolderSyncState::Interrupted);
    }

    /// An account opened anew takes over the folders and the last run; a run of the old mailbox
    /// still under way counts as stopped there, and whatever the old one does afterwards stays out
    /// of the new one.
    #[test]
    fn an_account_opened_anew_takes_over_the_state_but_not_the_old_run() {
        let before = progress();
        before.begin(MailSyncTrigger::Start).end(Ok(()));
        let old_run = before.begin(MailSyncTrigger::Scheduled);
        listed(&before);
        let old_folder = before.folder_begins(Some(0), "f:1");
        let after = SyncProgress::after(&before);
        assert!(after.version() > before.version(), "a waiting call sees a change");
        let taken = after.status("mail:a");
        assert_eq!(taken.running, None);
        let last = taken.last_run.clone().expect("a last run");
        assert!(!last.ok && last.trigger == MailSyncTrigger::Scheduled, "{last:?}");
        assert_eq!(taken.folders[0].state, MailFolderSyncState::Interrupted);
        assert_eq!(taken.folders[1].state, MailFolderSyncState::NotReached);

        let new_run = after.begin(MailSyncTrigger::Start);
        drop(old_folder);
        drop(old_run);
        assert!(after.status("mail:a").running.is_some(), "the old mailbox's run ends only its own");
        new_run.end(Ok(()));
    }

    /// The last folder vanishing never shows "3 of 2", and a run failing at its end names no folder.
    #[test]
    fn a_vanished_last_folder_and_a_failed_end_keep_the_count_right() {
        let progress = progress();
        let run = progress.begin(MailSyncTrigger::Manual);
        listed(&progress);
        progress.folder_begins(Some(0), "f:1").end(MailFolderSyncState::Synced);
        progress.folder_begins(Some(1), "f:2").gone();
        let shown = progress.status("mail:a").running.expect("running");
        assert!(shown.folder_index <= shown.folder_count, "{shown:?}");
        progress.finishing();
        run.end(Err(&MailError::NotFound));
        let last = progress.status("mail:a").last_run.expect("a last run");
        assert_eq!((last.ok, last.failed_at, last.folder_count), (false, None, 1));
    }
}
