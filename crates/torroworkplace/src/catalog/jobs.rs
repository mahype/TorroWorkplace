//! The app jobs of this run (issue #411, step 6 of the plan): installing, updating, uninstalling, one at
//! a time, in memory. A job is a list of steps ([`AppJobStep`]) the runner in [`super`] walks through
//! ([`Job`]); `apps.app_job` reads where it stands, `apps.cancel_app_job` stops it while it downloads.
//! Once a job checks or installs it runs to its end, so nothing is left half installed.
//!
//! The ended jobs stay listed (the last [`KEEP_ENDED`]), so a window that asks twice a second still sees
//! how one ended.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;
use torro_ipc::{AppJobInfo, AppJobKind, AppJobState, AppJobStep, AppStep, StepStatus};

use super::CatalogError;

/// How many ended jobs stay listed.
const KEEP_ENDED: usize = 20;

/// How much of the percent a step is: a download is most of the time a job takes.
fn weight(step: AppStep) -> u32 {
    if step == AppStep::Download { 6 } else { 1 }
}

#[derive(Debug)]
struct Running {
    info: AppJobInfo,
    /// How far each step is, 0 to 1, for the percent.
    fractions: Vec<f32>,
    cancel: watch::Sender<bool>,
}

impl Running {
    fn percent(&self) -> u8 {
        let total: u32 = self.info.steps.iter().map(|step| weight(step.step)).sum::<u32>().max(1);
        let done: f32 =
            self.info.steps.iter().zip(&self.fractions).map(|(step, part)| weight(step.step) as f32 * part).sum();
        ((done / total as f32) * 100.0).clamp(0.0, 100.0) as u8
    }
}

#[derive(Debug, Default)]
struct Board {
    running: Option<Running>,
    ended: VecDeque<AppJobInfo>,
}

/// The jobs of this run. Cheap to clone: every clone holds the same jobs.
#[derive(Debug, Clone, Default)]
pub struct Jobs {
    board: Arc<Mutex<Board>>,
}

impl Jobs {
    /// Starts the job of `kind` for `apps` with `steps` (window class and step), all to do; `busy`
    /// while another one has not ended.
    pub fn begin(
        &self,
        kind: AppJobKind,
        apps: Vec<String>,
        steps: Vec<(String, AppStep)>,
    ) -> Result<Job, CatalogError> {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        if board.running.is_some() {
            return Err(CatalogError::Busy);
        }
        let id = new_id();
        let steps: Vec<AppJobStep> = steps
            .into_iter()
            .map(|(app, step)| AppJobStep { app, step, status: StepStatus::Todo, detail: None })
            .collect();
        let fractions = vec![0.0; steps.len()];
        let (cancel, signal) = watch::channel(false);
        let info = AppJobInfo {
            id: id.clone(),
            apps,
            kind,
            steps,
            percent: 0,
            state: AppJobState::Running,
            error: None,
            restart: false,
        };
        board.running = Some(Running { info, fractions, cancel });
        Ok(Job { id, board: Arc::clone(&self.board), signal })
    }

    /// The job `id`, running or ended.
    pub fn get(&self, id: &str) -> Option<AppJobInfo> {
        let board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        let running =
            board.running.as_ref().filter(|running| running.info.id == id).map(|running| running.info.clone());
        running.or_else(|| board.ended.iter().find(|info| info.id == id).cloned())
    }

    /// Cancels the job `id` when it downloads (or has not begun); answers it as it stands after.
    pub fn cancel(&self, id: &str) -> Option<AppJobInfo> {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = board.running.as_mut().filter(|running| running.info.id == id) {
            let now = running.info.steps.iter().find(|step| step.status == StepStatus::Now).map(|step| step.step);
            let begun = running.info.steps.iter().any(|step| step.status != StepStatus::Todo);
            let downloading = now == Some(AppStep::Download) || !begun;
            if running.info.state == AppJobState::Running && downloading {
                running.info.state = AppJobState::Cancelled;
                running.info.error = Some("cancelled".into());
                for step in running.info.steps.iter_mut().filter(|step| step.status == StepStatus::Now) {
                    step.status = StepStatus::Todo;
                }
                let _ = running.cancel.send(true);
            }
            return Some(running.info.clone());
        }
        board.ended.iter().find(|info| info.id == id).cloned()
    }

    /// The id of the job that runs now.
    pub fn running(&self) -> Option<String> {
        let board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        board
            .running
            .as_ref()
            .filter(|running| running.info.state == AppJobState::Running)
            .map(|running| running.info.id.clone())
    }

    /// Whether no runner is at work (a cancelled job's runner may still be ending).
    pub fn idle(&self) -> bool {
        self.board.lock().unwrap_or_else(PoisonError::into_inner).running.is_none()
    }
}

/// A random id of 16 hex digits (`audit::is_random_id` takes it as such).
fn new_id() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        // Without the system's randomness the time still tells jobs of one run apart.
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        bytes = (nanos as u64).to_be_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The runner's side of one job: it moves the steps along. Once the job is cancelled or failed, the
/// calls change nothing any more.
#[derive(Debug, Clone)]
pub struct Job {
    id: String,
    board: Arc<Mutex<Board>>,
    signal: watch::Receiver<bool>,
}

impl Job {
    /// The job as it stands.
    pub fn info(&self) -> AppJobInfo {
        let board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        let running =
            board.running.as_ref().filter(|running| running.info.id == self.id).map(|running| running.info.clone());
        running.or_else(|| board.ended.iter().find(|info| info.id == self.id).cloned()).unwrap_or_else(|| AppJobInfo {
            id: self.id.clone(),
            apps: Vec::new(),
            kind: AppJobKind::Install,
            steps: Vec::new(),
            percent: 0,
            state: AppJobState::Failed,
            error: Some("error".into()),
            restart: false,
        })
    }

    /// Turns true when the job is cancelled; for the download to stop at once.
    pub fn cancel_signal(&self) -> watch::Receiver<bool> {
        self.signal.clone()
    }

    /// `change` on the step `step` of `app`, while the job runs; `false` when it no longer does.
    fn with(&self, app: &str, step: AppStep, change: impl FnOnce(&mut Running, usize)) -> bool {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(running) = board.running.as_mut().filter(|running| running.info.id == self.id) else { return false };
        if running.info.state != AppJobState::Running {
            return false;
        }
        if let Some(index) = running.info.steps.iter().position(|known| known.app == app && known.step == step) {
            change(running, index);
            running.info.percent = running.percent();
        }
        true
    }

    /// The step begins; `false` when the job was cancelled meanwhile (the runner stops).
    pub fn start(&self, app: &str, step: AppStep) -> bool {
        self.with(app, step, |running, index| running.info.steps[index].status = StepStatus::Now)
    }

    /// How far the step is, 0 to 1.
    pub fn progress(&self, app: &str, step: AppStep, fraction: f32) {
        self.with(app, step, |running, index| running.fractions[index] = fraction.clamp(0.0, 1.0));
    }

    /// The step is done, with a short fact for it.
    pub fn done(&self, app: &str, step: AppStep, detail: Option<String>) {
        self.with(app, step, |running, index| {
            let shown = &mut running.info.steps[index];
            shown.status = StepStatus::Done;
            shown.detail = detail;
            running.fractions[index] = 1.0;
        });
    }

    /// The step failed with `code`, and so did the job.
    pub fn fail(&self, app: &str, step: AppStep, code: &str) {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(running) = board.running.as_mut().filter(|running| running.info.id == self.id) else { return };
        if running.info.state != AppJobState::Running {
            return;
        }
        if let Some(shown) = running.info.steps.iter_mut().find(|known| known.app == app && known.step == step) {
            shown.status = StepStatus::Failed;
        }
        running.info.state = AppJobState::Failed;
        running.info.error = Some(code.to_string());
    }

    /// Every step is done; `restart` when the main application was updated and the service hands over
    /// to its new copy now.
    pub fn finish(&self, restart: bool) {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(running) = board.running.as_mut().filter(|running| running.info.id == self.id) else { return };
        if running.info.state != AppJobState::Running {
            return;
        }
        running.info.state = AppJobState::Done;
        running.info.percent = 100;
        running.info.restart = restart;
    }

    /// The runner is through: the next job may begin, this one stays listed.
    pub fn end(&self) {
        let mut board = self.board.lock().unwrap_or_else(PoisonError::into_inner);
        if board.running.as_ref().is_some_and(|running| running.info.id == self.id)
            && let Some(running) = board.running.take()
        {
            board.ended.push_back(running.info);
            while board.ended.len() > KEEP_ENDED {
                board.ended.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<(String, AppStep)> {
        [AppStep::Download, AppStep::Verify, AppStep::Install, AppStep::Connect]
            .into_iter()
            .map(|step| ("torro-tasks".to_string(), step))
            .collect()
    }

    #[test]
    fn one_job_at_a_time_and_the_ended_stay_listed() {
        let jobs = Jobs::default();
        let job = jobs.begin(AppJobKind::Install, vec!["torro-tasks".into()], steps()).expect("job");
        assert_eq!(job.id.len(), 16);
        assert!(job.id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(jobs.running(), Some(job.id.clone()));
        assert_eq!(jobs.begin(AppJobKind::Install, Vec::new(), steps()).err(), Some(CatalogError::Busy));
        assert!(job.start("torro-tasks", AppStep::Download));
        job.progress("torro-tasks", AppStep::Download, 0.5);
        assert_eq!(jobs.get(&job.id).map(|info| info.percent), Some(33), "half of 6 of 9");
        job.done("torro-tasks", AppStep::Download, Some("1000000".into()));
        job.finish(false);
        job.end();
        assert_eq!(jobs.running(), None);
        let ended = jobs.get(&job.id).expect("listed");
        assert_eq!((ended.state, ended.percent), (AppJobState::Done, 100));
        assert_eq!(ended.steps[0].detail.as_deref(), Some("1000000"));
        assert!(jobs.begin(AppJobKind::Update, Vec::new(), steps()).is_ok(), "the next one may begin");
        assert_eq!(jobs.get("0000"), None);
    }

    #[test]
    fn a_job_is_cancelled_only_while_it_downloads() {
        let jobs = Jobs::default();
        let job = jobs.begin(AppJobKind::Install, vec!["torro-tasks".into()], steps()).expect("job");
        job.start("torro-tasks", AppStep::Download);
        job.done("torro-tasks", AppStep::Download, None);
        job.start("torro-tasks", AppStep::Verify);
        let checking = jobs.cancel(&job.id).expect("job");
        assert_eq!(checking.state, AppJobState::Running, "it checks: it runs to its end");
        job.fail("torro-tasks", AppStep::Verify, "checksum");
        job.end();
        assert_eq!(
            jobs.get(&job.id).map(|info| (info.state, info.error)),
            Some((AppJobState::Failed, Some("checksum".into())))
        );

        let job = jobs.begin(AppJobKind::Install, vec!["torro-tasks".into()], steps()).expect("job");
        let signal = job.cancel_signal();
        job.start("torro-tasks", AppStep::Download);
        let cancelled = jobs.cancel(&job.id).expect("job");
        assert_eq!((cancelled.state, cancelled.error.as_deref()), (AppJobState::Cancelled, Some("cancelled")));
        assert!(*signal.borrow(), "the download hears it");
        assert!(!job.start("torro-tasks", AppStep::Verify), "the runner stops");
        assert_eq!(jobs.running(), None, "not running any more");
        assert_eq!(
            jobs.begin(AppJobKind::Install, Vec::new(), steps()).err(),
            Some(CatalogError::Busy),
            "until it ended"
        );
        job.end();
        assert!(jobs.begin(AppJobKind::Install, Vec::new(), steps()).is_ok());
    }
}
