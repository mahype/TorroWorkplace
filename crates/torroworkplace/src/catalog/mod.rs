//! The app catalog (issue #411, sub-issue #479): the main application and every app of the suite with
//! the version installed and the newest on GitHub, and the jobs that install, update and uninstall them
//! (`apps.list_catalog`, `apps.check_updates`, `apps.install_app`, `apps.update_apps`,
//! `apps.uninstall_app`, `apps.app_job`, `apps.cancel_app_job`).
//!
//! - [`releases`]: the newest release on GitHub, asked daily ([`watch`]) and on `apps.check_updates`,
//!   kept in `<state>/apps-catalog.json`, so a failed check leaves the last state with its code.
//! - [`installed`]: what is installed where, in which version, and whether a package manager looks
//!   after it (decision 1 of the plan: then the catalog names its command instead of acting itself).
//! - [`install`]: the work on this system: download, checksum and signature, put in place, launcher.
//! - [`jobs`]: one job at a time with its steps, in memory.
//!
//! Every rule takes the system it is for as a value ([`Platform`]), so the rules of all three systems
//! are tested on each. What leaves the core are fixed codes, sizes, paths and versions, never what a
//! program the core ran printed.
//!
//! After a job the service may hand over to a newer copy of itself (decision 5, issue #477): the
//! hand-over and the search for the newest copy live in the daemon; the catalog takes both as hooks
//! ([`Catalog::set_hand_over`], [`Catalog::set_newest_service`]).

mod install;
mod installed;
mod jobs;
mod releases;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use chrono::Utc;
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::version::Version;
use torro_ipc::{AppCatalog, AppJobInfo, AppJobKind, AppStep, CatalogApp, CatalogState, Response};

use install::Fail;
pub use installed::Env;
use installed::{Found, Scan};
use jobs::{Job, Jobs};
use releases::{Asset, Releases, Stored};

/// Hands the service over to the program at the path (issue #477, `daemon::hand_over`): starts it as
/// `daemon --takeover` and ends this service. Wired by the daemon; by default it only logs.
pub type HandOver = Arc<dyn Fn(PathBuf) + Send + Sync>;

/// The newest copy of the service on this computer with its version (issue #477): on macOS the
/// service bundles inside the apps' bundles. Wired by the daemon; by default none is known.
pub type NewestService = Arc<dyn Fn() -> Option<(PathBuf, Version)> + Send + Sync>;

/// How long after the start the first check runs (decision 2): late enough not to slow the start.
const FIRST_CHECK: Duration = Duration::from_secs(60);

/// How often [`watch`] looks whether a check is due: hourly, so a computer that slept still checks
/// about once a day.
const WATCH_TICK: Duration = Duration::from_secs(60 * 60);

/// How old the last check may be before [`watch`] checks again (decision 2): a day.
const CHECK_EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// How long a done job waits before the service hands over, so the window following it reads the
/// end (`restart: true`) before the service goes.
const HAND_OVER_DELAY: Duration = Duration::from_millis(1500);

/// The systems the catalog knows how to install on; a value, so every rule is tested everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code, reason = "a build constructs only its own system; tests build all three"))]
pub enum Platform {
    /// Bundles (`Torro Calendar.app`), installed from signed `.app.zip` archives.
    MacOs,
    /// Programs beside the service, from `.tar.xz` archives, with `.desktop` entries.
    Linux,
    /// Programs beside the service, from `.zip` archives, with Start menu shortcuts.
    Windows,
}

impl Platform {
    #[cfg(target_os = "macos")]
    pub const CURRENT: Platform = Platform::MacOs;
    #[cfg(windows)]
    pub const CURRENT: Platform = Platform::Windows;
    #[cfg(not(any(target_os = "macos", windows)))]
    pub const CURRENT: Platform = Platform::Linux;

    /// The target triple the release's archives carry for this system on `arch` (`x86_64`,
    /// `aarch64`, as [`std::env::consts::ARCH`] names them); `None` where the release builds nothing.
    pub fn target(self, arch: &str) -> Option<String> {
        match (self, arch) {
            (Platform::MacOs, "x86_64" | "aarch64") => Some(format!("{arch}-apple-darwin")),
            (Platform::Linux, "x86_64" | "aarch64") => Some(format!("{arch}-unknown-linux-gnu")),
            (Platform::Windows, "x86_64") => Some("x86_64-pc-windows-msvc".into()),
            _ => None,
        }
    }

    /// The file name of the program `name` on this system: `torrocalendar.exe` on Windows.
    pub fn program_file(self, name: &str) -> String {
        match self {
            Platform::Windows => format!("{name}.exe"),
            _ => name.to_string(),
        }
    }
}

/// One file of the release an app is installed from: its program and, on macOS, the title of its
/// bundle. Most apps are one part; the calendar on macOS is two (its reminder is a bundle of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    /// The program file (`torrocalendar`, `torro-reminder`), also the first word of the archive's name.
    pub program: &'static str,
    /// The bundle's name without `.app` (`Torro Calendar`).
    pub title: &'static str,
}

impl Part {
    /// The archive of this part in the release for `platform` and `target`:
    /// `torrocalendar-aarch64-apple-darwin.app.zip`, `torrocalendar-x86_64-unknown-linux-gnu.tar.xz`,
    /// `torrocalendar-x86_64-pc-windows-msvc.zip`.
    pub fn asset(self, platform: Platform, target: &str) -> String {
        let program = self.program;
        match platform {
            Platform::MacOs => format!("{program}-{target}.app.zip"),
            Platform::Linux => format!("{program}-{target}.tar.xz"),
            Platform::Windows => format!("{program}-{target}.zip"),
        }
    }

    /// `Torro Calendar.app`.
    pub fn bundle(self) -> String {
        format!("{}.app", self.title)
    }
}

/// The reminder over the whole screen on macOS (task ER2.2): the calendar's binary in an agent bundle
/// of its own, installed and removed with the calendar (decision 4).
pub const REMINDER: Part = Part { program: "torro-reminder", title: "Torro Reminder" };

/// The parts `app` is installed from on `platform`.
pub fn parts(app: AppId, platform: Platform) -> Vec<Part> {
    let own = Part { program: app.program_file(), title: app.display_name };
    match (platform, app == AppId::CALENDAR) {
        (Platform::MacOs, true) => vec![own, REMINDER],
        _ => vec![own],
    }
}

/// The apps `app` brings along (decision 4): Mail and App Feed need the Notification Hub, whose
/// program writes their mails and answers.
pub fn requires(app: AppId) -> Vec<AppId> {
    if app == AppId::INBOX_MAIL || app == AppId::INBOX_NOTIFICATIONS { vec![AppId::INBOX] } else { Vec::new() }
}

/// Why an app command was refused; [`CatalogError::code`] is the code of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// No such app or job. `not-found`.
    NotFound(String),
    /// Installed already, not installed, looked after by a package manager, needed by another app, no
    /// update. `conflict`.
    Conflict(String),
    /// Another app job runs. `busy`.
    Busy,
    /// GitHub was not reached. `offline`.
    Offline,
    /// A development build, or the release has nothing for this computer. `unsupported`.
    Unsupported(String),
}

impl CatalogError {
    /// `not-found`, `conflict`, `busy`, `offline` or `unsupported`.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogError::NotFound(_) => "not-found",
            CatalogError::Conflict(_) => "conflict",
            CatalogError::Busy => "busy",
            CatalogError::Offline => "offline",
            CatalogError::Unsupported(_) => "unsupported",
        }
    }

    /// The error answer of a command.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.to_string() }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::NotFound(message) | CatalogError::Conflict(message) | CatalogError::Unsupported(message) => {
                f.write_str(message)
            }
            CatalogError::Busy => f.write_str(tr("Another app is being installed, updated or uninstalled.")),
            CatalogError::Offline => f.write_str(tr("Couldn't reach GitHub.")),
        }
    }
}

/// The hooks of the daemon ([`HandOver`], [`NewestService`]).
struct Hooks {
    hand_over: HandOver,
    newest_service: NewestService,
}

impl Default for Hooks {
    fn default() -> Hooks {
        Hooks {
            hand_over: Arc::new(|program: PathBuf| {
                eprintln!("torroworkplace: a newer service is at {}; no hand-over is wired", program.display());
            }),
            newest_service: Arc::new(|| None),
        }
    }
}

struct Inner {
    releases: Releases,
    env: Env,
    jobs: Jobs,
    versions: installed::Versions,
    hooks: RwLock<Hooks>,
    /// A development build installs, updates and uninstalls nothing (`unsupported`): it would put
    /// release programs beside `target/debug`. Tests switch it off.
    dev_build: bool,
    /// [`HAND_OVER_DELAY`]; none in tests.
    hand_over_delay: Duration,
}

/// The app catalog of the service. Cheap to clone: every clone is the same catalog.
#[derive(Clone)]
pub struct Catalog {
    inner: Arc<Inner>,
}

impl Catalog {
    /// The catalog of this computer: GitHub's releases, the state in the core's state directory, the
    /// apps where the service finds them.
    pub fn system() -> Catalog {
        Catalog::new(Releases::system(), Env::system(), cfg!(debug_assertions), HAND_OVER_DELAY)
    }

    /// A catalog that asks no server, keeps no file and finds no app: the one of a core in tests
    /// unless they bring theirs ([`crate::core::Core::with_catalog`]).
    pub fn none() -> Catalog {
        Catalog::new(Releases::none(), Env::empty(Platform::CURRENT), true, Duration::ZERO)
    }

    fn new(releases: Releases, env: Env, dev_build: bool, hand_over_delay: Duration) -> Catalog {
        Catalog {
            inner: Arc::new(Inner {
                releases,
                env,
                jobs: Jobs::default(),
                versions: installed::Versions::default(),
                hooks: RwLock::new(Hooks::default()),
                dev_build,
                hand_over_delay,
            }),
        }
    }

    /// Sets how the service hands over to another copy of itself after a job (issue #477).
    pub fn set_hand_over(&self, hand_over: HandOver) {
        self.inner.hooks.write().unwrap_or_else(PoisonError::into_inner).hand_over = hand_over;
    }

    /// Sets how the newest copy of the service on this computer is found (issue #477).
    pub fn set_newest_service(&self, newest_service: NewestService) {
        self.inner.hooks.write().unwrap_or_else(PoisonError::into_inner).newest_service = newest_service;
    }

    /// `apps.list_catalog`: the stored release against what is installed now.
    pub async fn list(&self) -> AppCatalog {
        let stored = self.inner.releases.stored();
        let scan = installed::scan(&self.inner.env, &self.inner.versions).await;
        build(&self.inner.env, &scan, &stored, self.inner.jobs.running())
    }

    /// `apps.check_updates`: asks GitHub ([`Releases::check`], at most every five minutes), then lists.
    pub async fn check(&self) -> AppCatalog {
        self.inner.releases.check(Utc::now()).await;
        self.list().await
    }

    /// `apps.app_job`.
    pub fn job(&self, id: &str) -> Result<AppJobInfo, CatalogError> {
        self.inner.jobs.get(id).ok_or_else(|| no_job(id))
    }

    /// `apps.cancel_app_job`: cancels while the job downloads, else answers it as it stands.
    pub fn cancel(&self, id: &str) -> Result<AppJobInfo, CatalogError> {
        self.inner.jobs.cancel(id).ok_or_else(|| no_job(id))
    }

    /// `apps.install_app`: installs `name` (and what it brings along) from the newest release.
    pub async fn install(&self, name: &str) -> Result<AppJobInfo, CatalogError> {
        self.refuse_in_development()?;
        let app = known(name)?;
        let scan = installed::scan(&self.inner.env, &self.inner.versions).await;
        if scan.found(app).is_some() {
            return Err(CatalogError::Conflict(trf("{app} is installed already.", &[("app", &app.display_name)])));
        }
        refuse_managed(app, &scan)?;
        let stored = self.fresh_release().await?;
        let mut apps: Vec<AppId> = requires(app).into_iter().filter(|needed| scan.found(*needed).is_none()).collect();
        apps.push(app);
        let items = apps.iter().map(|app| self.item(*app, None, &stored)).collect::<Result<Vec<_>, _>>()?;
        self.start(AppJobKind::Install, items, app, false)
    }

    /// `apps.update_apps`: updates `names` in one job, the main application last.
    pub async fn update(&self, names: &[String]) -> Result<AppJobInfo, CatalogError> {
        self.refuse_in_development()?;
        let mut apps: Vec<AppId> = Vec::new();
        for name in names {
            let app = known(name)?;
            if !apps.contains(&app) {
                apps.push(app);
            }
        }
        if apps.is_empty() {
            return Err(CatalogError::NotFound(tr("Name at least one app to update.").into()));
        }
        // The main application restarts the service, so it goes last.
        apps.sort_by_key(|app| (*app == AppId::WORKPLACE, AppId::ALL.iter().position(|known| known == app)));
        let scan = installed::scan(&self.inner.env, &self.inner.versions).await;
        let stored = self.fresh_release().await?;
        let mut items = Vec::new();
        for app in &apps {
            let Some(found) = scan.found(*app) else {
                return Err(CatalogError::Conflict(trf("{app} is not installed.", &[("app", &app.display_name)])));
            };
            refuse_managed(*app, &scan)?;
            let entry = entry(&self.inner.env, *app, &scan, &stored);
            if entry.state != CatalogState::Update {
                return Err(CatalogError::Conflict(trf("{app} has no update.", &[("app", &app.display_name)])));
            }
            // What an app brings along and is missing comes too (decision 4).
            for needed in requires(*app) {
                let listed = apps.contains(&needed) || items.iter().any(|item: &Item| item.app == needed);
                if scan.found(needed).is_none() && !listed {
                    items.push(self.item(needed, None, &stored)?);
                }
            }
            items.push(self.item(*app, Some(found.clone()), &stored)?);
        }
        let last = apps.last().copied().unwrap_or(AppId::WORKPLACE);
        self.start(AppJobKind::Update, items, last, false)
    }

    /// `apps.uninstall_app`: moves `name` to the trash, with `remove_data` its own files too.
    pub async fn uninstall(&self, name: &str, remove_data: bool) -> Result<AppJobInfo, CatalogError> {
        self.refuse_in_development()?;
        let app = known(name)
            .ok()
            .filter(|app| *app != AppId::WORKPLACE)
            .ok_or_else(|| CatalogError::NotFound(trf("There is no app {app} to uninstall.", &[("app", &name)])))?;
        let scan = installed::scan(&self.inner.env, &self.inner.versions).await;
        let Some(found) = scan.found(app) else {
            return Err(CatalogError::Conflict(trf("{app} is not installed.", &[("app", &app.display_name)])));
        };
        refuse_managed(app, &scan)?;
        if let Some(user) =
            AppId::ALL.iter().find(|other| requires(**other).contains(&app) && scan.found(**other).is_some())
        {
            return Err(CatalogError::Conflict(trf(
                "{app} is needed by {other}; uninstall that first.",
                &[("app", &app.display_name), ("other", &user.display_name)],
            )));
        }
        let item = Item { app, parts: Vec::new(), found: Some(found.clone()), shared: Vec::new() };
        // Its own files, but none another installed app keeps using (the inbox's three share one).
        let shared = AppId::ALL
            .iter()
            .filter(|other| **other != app && scan.found(**other).is_some())
            .flat_map(|other| (self.inner.env.app_dirs)(*other))
            .collect();
        let item = Item { shared, ..item };
        self.start(AppJobKind::Uninstall, vec![item], app, remove_data)
    }

    /// Checks on GitHub every [`WATCH_TICK`] whether the last check is a day old (decision 2), first
    /// [`FIRST_CHECK`] after the start; on Windows it first deletes what an update left (`.old`).
    pub async fn watch(self) {
        if self.inner.env.platform == Platform::Windows
            && let Some(dir) = self.inner.env.exe.as_deref().and_then(Path::parent)
        {
            install::sweep_old(dir);
        }
        tokio::time::sleep(FIRST_CHECK).await;
        self.inner.releases.check(Utc::now()).await;
        loop {
            tokio::time::sleep(WATCH_TICK).await;
            let checked = self.inner.releases.stored().checked;
            if checked.is_none_or(|checked| Utc::now() - checked >= CHECK_EVERY) {
                self.inner.releases.check(Utc::now()).await;
            }
        }
    }

    /// `unsupported` in a development build, `busy` while a job runs (checked again when the job
    /// begins, [`Jobs::begin`]).
    fn refuse_in_development(&self) -> Result<(), CatalogError> {
        if self.inner.dev_build {
            return Err(CatalogError::Unsupported(
                tr("A development build does not install, update or uninstall apps.").into(),
            ));
        }
        if !self.inner.jobs.idle() {
            return Err(CatalogError::Busy);
        }
        Ok(())
    }

    /// The release to install from: asked again unless asked within five minutes; `offline` when
    /// GitHub was not reached now.
    async fn fresh_release(&self) -> Result<Stored, CatalogError> {
        let stored = self.inner.releases.check(Utc::now()).await;
        match stored.error.as_deref() {
            Some(releases::OFFLINE) => Err(CatalogError::Offline),
            _ => Ok(stored),
        }
    }

    /// What a job does with `app`: the archives of its parts in `stored` for this system.
    fn item(&self, app: AppId, found: Option<Found>, stored: &Stored) -> Result<Item, CatalogError> {
        let nothing = || {
            CatalogError::Unsupported(trf(
                "The newest release has no {app} for this computer.",
                &[("app", &app.display_name)],
            ))
        };
        let parts = release_parts(&self.inner.env, app, stored).ok_or_else(nothing)?;
        Ok(Item { app, parts, found, shared: Vec::new() })
    }

    /// Starts the job in the background and answers it as it starts.
    fn start(
        &self,
        kind: AppJobKind,
        items: Vec<Item>,
        connect: AppId,
        remove_data: bool,
    ) -> Result<AppJobInfo, CatalogError> {
        let platform = self.inner.env.platform;
        let mut steps = Vec::new();
        for item in &items {
            let class = item.app.window_class.to_string();
            let own: &[AppStep] = match kind {
                AppJobKind::Install | AppJobKind::Update => &[AppStep::Download, AppStep::Verify, AppStep::Install],
                AppJobKind::Uninstall => &[AppStep::Remove],
            };
            steps.extend(own.iter().map(|step| (class.clone(), *step)));
            if platform != Platform::MacOs {
                steps.push((class, AppStep::Launcher));
            }
        }
        steps.push((connect.window_class.to_string(), AppStep::Connect));
        let apps = items.iter().map(|item| item.app.window_class.to_string()).collect();
        let job = self.inner.jobs.begin(kind, apps, steps)?;
        let info = job.info();
        let inner = Arc::clone(&self.inner);
        let work = Work { kind, items, connect, remove_data };
        tokio::spawn(run(inner, work, job));
        Ok(info)
    }
}

fn no_job(id: &str) -> CatalogError {
    CatalogError::NotFound(trf("There is no app job {id}.", &[("id", &id)]))
}

/// The entry of [`AppId::ALL`] that `name` names, window class or binary.
fn known(name: &str) -> Result<AppId, CatalogError> {
    AppId::find(name).ok_or_else(|| CatalogError::NotFound(trf("There is no app {app}.", &[("app", &name)])))
}

/// `conflict` when a package manager looks after `app`: the one that installed it, or, for an app
/// not installed, the one the service came with (decision 1).
fn refuse_managed(app: AppId, scan: &Scan) -> Result<(), CatalogError> {
    match scan.manager(app) {
        Some(manager) => Err(CatalogError::Conflict(trf(
            "{manager} looks after {app}; use its command.",
            &[("manager", &manager.name()), ("app", &app.display_name)],
        ))),
        None => Ok(()),
    }
}

/// The archives of `app`'s parts in `stored` for the system of `env`, each with its `.sha256`; `None`
/// when one is missing (or the system has no target).
fn release_parts(env: &Env, app: AppId, stored: &Stored) -> Option<Vec<(Part, Asset, Option<Asset>)>> {
    let target = env.platform.target(&env.arch)?;
    parts(app, env.platform)
        .into_iter()
        .map(|part| {
            let name = part.asset(env.platform, &target);
            let archive = stored.asset(&name)?.clone();
            let sum = stored.asset(&format!("{name}.sha256")).cloned();
            // Elsewhere than on macOS the checksum is all that is checked: an archive without one is
            // not offered.
            (sum.is_some() || env.platform == Platform::MacOs).then_some((part, archive, sum))
        })
        .collect()
}

/// The catalog from what is installed (`scan`) and the release as stored.
fn build(env: &Env, scan: &Scan, stored: &Stored, job: Option<String>) -> AppCatalog {
    AppCatalog {
        checked: stored.checked,
        latest: stored.latest.clone(),
        release_url: stored.release_url.clone(),
        error: stored.error.clone(),
        main: entry(env, AppId::WORKPLACE, scan, stored),
        apps: AppId::ALL
            .iter()
            .filter(|app| **app != AppId::WORKPLACE)
            .map(|app| entry(env, *app, scan, stored))
            .collect(),
        job,
    }
}

/// One entry of the catalog.
fn entry(env: &Env, app: AppId, scan: &Scan, stored: &Stored) -> CatalogApp {
    let found = scan.found(app);
    let release = release_parts(env, app, stored);
    let available = release.as_ref().and(stored.latest.clone());
    let size = release.as_ref().map(|parts| parts.iter().map(|(_, archive, _)| archive.size).sum());
    let installed = found.map(|found| found.version.clone().unwrap_or_else(|| "unknown".into()));
    let manager = scan.manager(app);
    let newer = |installed: &str| match (installed.parse::<Version>(), available.as_deref().map(str::parse::<Version>))
    {
        (Ok(installed), Some(Ok(available))) => available > installed,
        _ => false,
    };
    let state = match (&installed, manager) {
        (_, Some(_)) => CatalogState::Managed,
        (Some(installed), None) if newer(installed) => CatalogState::Update,
        (Some(_), None) => CatalogState::Installed,
        (None, None) if available.is_some() => CatalogState::Available,
        (None, None) => CatalogState::Unavailable,
    };
    CatalogApp {
        window_class: app.window_class.to_string(),
        program: app.program_file().to_string(),
        name: app.display_name.to_string(),
        installed,
        available,
        state,
        managed: manager.map(|manager| manager.commands(app)),
        size,
        requires: requires(app).iter().map(|app| app.window_class.to_string()).collect(),
    }
}

/// What one job does with one app.
struct Item {
    app: AppId,
    /// Install and update: the archives of its parts with their checksums.
    parts: Vec<(Part, Asset, Option<Asset>)>,
    /// Update and uninstall: what is installed now.
    found: Option<Found>,
    /// Uninstall: folders of other installed apps, which its data removal leaves alone.
    shared: Vec<PathBuf>,
}

/// A whole job ([`run`]).
struct Work {
    kind: AppJobKind,
    items: Vec<Item>,
    /// The app whose window follows the job; the step [`AppStep::Connect`] is its.
    connect: AppId,
    remove_data: bool,
}

/// What the steps of a job left for the step [`AppStep::Connect`].
#[derive(Default)]
struct Done {
    /// Bundles and programs moved away or replaced: a service running from one of them hands over.
    removed: Vec<PathBuf>,
    /// The service program of a main application just updated or installed (the hand-over goes there
    /// unless a newer copy is known).
    main_service: Option<PathBuf>,
}

/// Runs `work` as `job` to its end, then hands the service over when that is due.
async fn run(inner: Arc<Inner>, work: Work, job: Job) {
    let outcome = run_steps(&inner, &work, &job).await;
    let hand_over = match outcome {
        Ok(target) => {
            job.finish(target.is_some());
            target
        }
        Err((app, step, fail)) => {
            job.fail(app.window_class, step, fail.code());
            None
        }
    };
    job.end();
    if let Some(program) = hand_over {
        tokio::time::sleep(inner.hand_over_delay).await;
        let hook = Arc::clone(&inner.hooks.read().unwrap_or_else(PoisonError::into_inner).hand_over);
        hook(program);
    }
}

type StepFail = (AppId, AppStep, Fail);

/// The steps of `work`; `Ok` with the program to hand over to, if one is due.
async fn run_steps(inner: &Arc<Inner>, work: &Work, job: &Job) -> Result<Option<PathBuf>, StepFail> {
    let scratch = tempfile::Builder::new()
        .prefix("torro-apps-")
        .tempdir()
        .map_err(|err| (work.connect, AppStep::Download, install::io_fail(&err)))?;
    let mut done = Done::default();
    for item in &work.items {
        match work.kind {
            AppJobKind::Install | AppJobKind::Update => {
                install_item(inner, item, job, scratch.path(), &mut done).await?;
            }
            AppJobKind::Uninstall => uninstall_item(inner, item, job, work.remove_data, &mut done).await?,
        }
    }
    let connect = work.connect;
    if !job.start(connect.window_class, AppStep::Connect) {
        return Err((connect, AppStep::Connect, Fail::Cancelled));
    }
    let newest = {
        let hook = Arc::clone(&inner.hooks.read().unwrap_or_else(PoisonError::into_inner).newest_service);
        hook()
    };
    let target = hand_over_target(inner.env.exe.as_deref(), &done, newest, &Version::current());
    let detail = target.as_ref().and_then(|(_, version)| version.as_ref().map(Version::to_string));
    job.done(connect.window_class, AppStep::Connect, detail);
    Ok(target.map(|(program, _)| program))
}

/// Where the service hands over after a job (decision 5): to the main application's new service
/// when it was updated (always), else to the newest copy when this one ran from something the job
/// removed, or when the newest copy is newer than `current`.
fn hand_over_target(
    exe: Option<&Path>,
    done: &Done,
    newest: Option<(PathBuf, Version)>,
    current: &Version,
) -> Option<(PathBuf, Option<Version>)> {
    let newest = newest.map(|(program, version)| (program, Some(version)));
    if let Some(main) = &done.main_service {
        return newest.filter(|(_, version)| version.as_ref() > Some(current)).or(Some((main.clone(), None)));
    }
    let removed = exe.is_some_and(|exe| done.removed.iter().any(|gone| exe.starts_with(gone)));
    match removed {
        true => newest,
        false => newest.filter(|(_, version)| version.as_ref() > Some(current)),
    }
}

/// Download, check, put in place and (not on macOS) the launcher entry of one app.
async fn install_item(
    inner: &Arc<Inner>,
    item: &Item,
    job: &Job,
    scratch: &Path,
    done: &mut Done,
) -> Result<(), StepFail> {
    let env = &inner.env;
    let app = item.app;
    let class = app.window_class;
    let fail = |step: AppStep| move |fail: Fail| (app, step, fail);
    let cancelled = |step: AppStep| (app, step, Fail::Cancelled);
    let folder = scratch.join(class);
    std::fs::create_dir_all(&folder).map_err(|err| (app, AppStep::Download, install::io_fail(&err)))?;

    // Download every part with its checksum.
    if !job.start(class, AppStep::Download) {
        return Err(cancelled(AppStep::Download));
    }
    let http = inner.releases.http().ok_or((app, AppStep::Download, Fail::Error))?;
    let total: u64 = item.parts.iter().map(|(_, archive, _)| archive.size).sum::<u64>().max(1);
    let mut loaded = 0u64;
    let mut fetched = Vec::new();
    for (part, archive, sum) in &item.parts {
        let file = folder.join(&archive.name);
        let base = loaded;
        let mut signal = job.cancel_signal();
        let progress = |bytes: u64| job.progress(class, AppStep::Download, (base + bytes) as f32 / total as f32);
        install::download(http, archive, &file, &mut signal, progress).await.map_err(fail(AppStep::Download))?;
        loaded += archive.size;
        let expected = match sum {
            Some(sum) => Some(install::fetch_sum(http, sum).await.map_err(fail(AppStep::Download))?),
            None => None,
        };
        fetched.push((*part, file, expected));
    }
    job.done(class, AppStep::Download, Some(install::megabytes(loaded)));

    // Check: the checksum everywhere it is given, on macOS also the signature.
    if !job.start(class, AppStep::Verify) {
        return Err(cancelled(AppStep::Verify));
    }
    let platform = env.platform;
    let mut unpacked = Vec::new();
    for (part, file, expected) in fetched {
        if let Some(expected) = expected {
            let file = file.clone();
            blocking(move || install::check_sum(&file, &expected)).await.map_err(fail(AppStep::Verify))?;
        }
        let into = folder.join(format!("{}.unpacked", part.program));
        let program = platform.program_file(part.program);
        let new = blocking(move || match platform {
            Platform::MacOs => install::unpack_bundle(&file, &into, &part.bundle(), part.program),
            Platform::Linux => install::unpack_tar(&file, &into, &program),
            Platform::Windows => install::unpack_zip(&file, &into, &program),
        })
        .await
        .map_err(fail(AppStep::Verify))?;
        if platform == Platform::MacOs {
            let bundle = new.clone();
            blocking(move || install::check_signature(&bundle)).await.map_err(fail(AppStep::Verify))?;
        }
        unpacked.push((part, new));
    }
    let checked = if platform == Platform::MacOs { "Developer ID" } else { "SHA-256" };
    job.done(class, AppStep::Verify, Some(checked.into()));

    // Put in place: beside what is there, else where the suite lies.
    if !job.start(class, AppStep::Install) {
        return Err(cancelled(AppStep::Install));
    }
    let mut placed = Vec::new();
    for (part, new) in unpacked {
        let (target, old) = match platform {
            Platform::MacOs => {
                let old = existing_bundle(env, item, part);
                let folder = old
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .or_else(|| install::bundle_folder(env));
                let folder = folder.ok_or((app, AppStep::Install, Fail::Permission))?;
                (folder.join(part.bundle()), old)
            }
            Platform::Linux | Platform::Windows => {
                let old = item.found.as_ref().map(|found| found.path.clone());
                let dir = old.as_deref().and_then(Path::parent).or_else(|| env.exe.as_deref().and_then(Path::parent));
                let dir = dir.ok_or((app, AppStep::Install, Fail::Error))?;
                (dir.join(platform.program_file(part.program)), old)
            }
        };
        let discard = Arc::clone(&env.discard);
        let into = target.clone();
        let old_bundle = old.clone();
        blocking(move || match platform {
            Platform::MacOs => install::place_bundle(&new, &into, old_bundle.as_deref(), &*discard),
            Platform::Linux | Platform::Windows => install::place_program(&new, &into, platform),
        })
        .await
        .map_err(fail(AppStep::Install))?;
        if let Some(old) = old {
            done.removed.push(old);
        }
        placed.push(target);
    }
    let Some(program) = placed.first().cloned() else {
        return Err((app, AppStep::Install, Fail::Error));
    };
    if app == AppId::WORKPLACE {
        done.main_service = Some(match platform {
            Platform::MacOs => program
                .parent()
                .map_or_else(|| program.clone(), |folder| folder.join(torro_gui::source::SERVICE_PROGRAM)),
            Platform::Linux | Platform::Windows => program.clone(),
        });
    }
    let shown = match platform {
        Platform::MacOs => program.parent().unwrap_or(&program).to_path_buf(),
        _ => program.clone(),
    };
    job.done(class, AppStep::Install, Some(shown.display().to_string()));

    // The launcher entry; on macOS the bundle is one.
    if platform != Platform::MacOs {
        if !job.start(class, AppStep::Launcher) {
            return Err(cancelled(AppStep::Launcher));
        }
        let (data_home, start_menu) = (env.data_home.clone(), env.start_menu.clone());
        let entry = blocking(move || match platform {
            Platform::Linux => install::write_desktop(app, &program, data_home.as_deref()),
            _ => install::make_shortcut(app, &program, start_menu.as_deref()),
        })
        .await
        .map_err(fail(AppStep::Launcher))?;
        job.done(class, AppStep::Launcher, Some(entry.display().to_string()));
    }
    Ok(())
}

/// The bundle of `part` as installed now: the app's own ([`Found::path`]) or, for the reminder, the one
/// beside it or where the suite's bundles are looked for.
fn existing_bundle(env: &Env, item: &Item, part: Part) -> Option<PathBuf> {
    let found = item.found.as_ref()?;
    if found.path.file_name().is_some_and(|name| name == part.bundle().as_str()) {
        return Some(found.path.clone());
    }
    let beside = found.path.parent().map(|folder| folder.join(part.bundle())).filter(|bundle| bundle.is_dir());
    beside.or_else(|| installed::bundle_path(env, part.title, part.program))
}

/// Moves one app to the trash, with its data when asked, and removes its launcher entry.
async fn uninstall_item(
    inner: &Arc<Inner>,
    item: &Item,
    job: &Job,
    remove_data: bool,
    done: &mut Done,
) -> Result<(), StepFail> {
    let env = &inner.env;
    let app = item.app;
    let class = app.window_class;
    let platform = env.platform;
    let Some(found) = item.found.clone() else {
        return Err((app, AppStep::Remove, Fail::Error));
    };
    if !job.start(class, AppStep::Remove) {
        return Err((app, AppStep::Remove, Fail::Cancelled));
    }
    let mut gone = vec![found.path.clone()];
    if platform == Platform::MacOs {
        // The calendar's reminder goes with it (decision 4).
        let others = parts(app, platform).into_iter().skip(1);
        gone.extend(others.filter_map(|part| existing_bundle(env, item, part)));
    }
    let data: Vec<PathBuf> = match remove_data {
        true => (env.app_dirs)(app).into_iter().filter(|dir| !item.shared.contains(dir)).collect(),
        false => Vec::new(),
    };
    let discard = Arc::clone(&env.discard);
    let removing = gone.clone();
    blocking(move || install::remove(&removing, &data, platform, &*discard))
        .await
        .map_err(|fail| (app, AppStep::Remove, fail))?;
    done.removed.extend(gone);
    job.done(class, AppStep::Remove, Some(found.path.display().to_string()));
    if platform != Platform::MacOs {
        if !job.start(class, AppStep::Launcher) {
            return Err((app, AppStep::Launcher, Fail::Cancelled));
        }
        let (data_home, start_menu) = (env.data_home.clone(), env.start_menu.clone());
        let entry = blocking(move || match platform {
            Platform::Linux => install::remove_desktop(app, data_home.as_deref()),
            _ => install::remove_shortcut(app, start_menu.as_deref()),
        })
        .await
        .map_err(|fail| (app, AppStep::Launcher, fail))?;
        job.done(class, AppStep::Launcher, entry.map(|path| path.display().to_string()));
    }
    Ok(())
}

/// `work` on a thread for blocking work: file copies, archives, `codesign`.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, Fail> + Send + 'static) -> Result<T, Fail> {
    tokio::task::spawn_blocking(work).await.unwrap_or(Err(Fail::Error))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use httpmock::MockServer;
    use installed::tests::{env_in, put_program};
    use std::sync::Mutex;
    use torro_ipc::{AppJobState, StepStatus};

    /// A catalog over `env` that asks `server` and installs for real (no development build).
    pub(crate) fn catalog_at(env: Env, server: &MockServer) -> Catalog {
        let releases = Releases::new(Some(server.base_url()), server.url("/download/"), None);
        Catalog::new(releases, env, false, Duration::ZERO)
    }

    /// One file of a test release: its name, contents and, to test a wrong one, the checksum it names.
    pub(crate) struct File {
        pub name: String,
        pub body: Vec<u8>,
        pub sum: Option<String>,
        /// How long the archive takes to answer.
        pub delay: Option<Duration>,
    }

    /// Serves the release `version` with `files` (each with its `.sha256`) on `server`.
    pub(crate) async fn serve(server: &MockServer, version: &str, files: &[File]) {
        let mut assets = Vec::new();
        for file in files {
            let sum_name = format!("{}.sha256", file.name);
            let sum = match &file.sum {
                Some(sum) => sum.clone(),
                None => {
                    let dir = tempfile::tempdir().expect("dir");
                    std::fs::write(dir.path().join("f"), &file.body).expect("write");
                    install::sha256_of(&dir.path().join("f")).expect("sum")
                }
            };
            let sum_text = format!("{sum}  {}\n", file.name);
            let parts = [(file.name.clone(), file.body.clone(), file.delay), (sum_name, sum_text.into_bytes(), None)];
            for (name, body, delay) in parts {
                assets.push(serde_json::json!({
                    "name": name, "size": body.len(), "browser_download_url": server.url(format!("/download/{name}"))
                }));
                let path = format!("/download/{name}");
                server
                    .mock_async(move |when, then| {
                        when.method("GET").path(path);
                        then.status(200).delay(delay.unwrap_or_default()).body(body);
                    })
                    .await;
            }
        }
        let release = serde_json::json!({
            "tag_name": format!("v{version}"),
            "html_url": format!("https://github.com/mahype/TorroWorkplace/releases/tag/v{version}"),
            "assets": assets,
        });
        server
            .mock_async(move |when, then| {
                when.method("GET").path(releases::LATEST);
                then.status(200).json_body(release);
            })
            .await;
    }

    /// An [`Env`] for Windows in `dir` with the main application (the service in `dir/bin`).
    pub(crate) fn windows_env(dir: &Path) -> Env {
        env_in(dir, Platform::Windows, &[], true)
    }

    /// A Windows `.zip` of `program` for the release.
    pub(crate) fn windows_file(program: &str, body: &[u8]) -> File {
        let dir = tempfile::tempdir().expect("dir");
        let archive = dir.path().join("a.zip");
        install::tests::zip_with(&archive, &format!("{program}.exe"), body);
        File {
            name: format!("{program}-x86_64-pc-windows-msvc.zip"),
            body: std::fs::read(archive).expect("zip"),
            sum: None,
            delay: None,
        }
    }

    /// Waits until the job `id` ended and its runner is through.
    pub(crate) async fn ended(catalog: &Catalog, id: &str) -> AppJobInfo {
        for _ in 0..500 {
            let job = catalog.job(id).expect("job");
            if job.state != AppJobState::Running && catalog.inner.jobs.idle() {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the job did not end: {:?}", catalog.job(id));
    }

    fn state_of(catalog: &AppCatalog, class: &str) -> CatalogState {
        catalog.apps.iter().find(|app| app.window_class == class).map(|app| app.state).expect("listed")
    }

    fn code(result: Result<AppJobInfo, CatalogError>) -> Option<&'static str> {
        result.err().map(|err| err.code())
    }

    #[tokio::test]
    async fn the_catalog_lists_versions_and_what_can_be_done() {
        let dir = tempfile::tempdir().expect("dir");
        let versions = [("torrocalendar", "0.1.0"), ("torroworkplace", "0.2.0")];
        let env = env_in(dir.path(), Platform::Windows, &versions, true);
        put_program(&env, "torroworkplace");
        put_program(&env, "torrocalendar");
        let server = MockServer::start_async().await;
        serve(&server, "0.2.0", &[windows_file("torrocalendar", b"MZ cal"), windows_file("torrotasks", b"MZ tasks")])
            .await;
        let catalog = catalog_at(env, &server);
        let before = catalog.list().await;
        assert_eq!((before.checked, before.latest.clone()), (None, None), "nothing asked yet");
        assert_eq!(state_of(&before, "torro-tasks"), CatalogState::Unavailable);
        let checked = catalog.check().await;
        assert_eq!(checked.latest.as_deref(), Some("0.2.0"));
        assert!(checked.checked.is_some() && checked.error.is_none());
        let calendar = checked.apps.iter().find(|app| app.window_class == "torro-calendar").expect("calendar");
        assert_eq!((calendar.installed.as_deref(), calendar.available.as_deref()), (Some("0.1.0"), Some("0.2.0")));
        assert_eq!(calendar.state, CatalogState::Update);
        assert_eq!(calendar.program, "torrocalendar");
        assert!(calendar.size.is_some_and(|size| size > 0));
        assert_eq!(state_of(&checked, "torro-tasks"), CatalogState::Available);
        assert_eq!(state_of(&checked, "torro-assistant"), CatalogState::Unavailable, "the release has none");
        let mail = checked.apps.iter().find(|app| app.window_class == "torro-inbox-mail").expect("mail");
        assert_eq!(mail.requires, ["torro-inbox"]);
        assert_eq!(checked.main.window_class, "torro-workplace");
        assert_eq!((checked.main.installed.as_deref(), checked.main.state), (Some("0.2.0"), CatalogState::Installed));
        assert_eq!(checked.apps.len(), 6, "every app but the main application");
        assert_eq!(checked.job, None);
    }

    #[tokio::test]
    async fn apps_of_a_package_manager_name_its_commands_and_are_left_alone() {
        let dir = tempfile::tempdir().expect("dir");
        let mut env = env_in(dir.path(), Platform::Windows, &[("torrocalendar", "0.1.0")], true);
        // The service came with Scoop.
        let scoop = dir.path().join("scoop/apps/torroworkplace/current");
        env.exe = Some(scoop.join("torroworkplace.exe"));
        env.dirs = vec![scoop];
        put_program(&env, "torrocalendar");
        let server = MockServer::start_async().await;
        serve(&server, "0.2.0", &[windows_file("torrotasks", b"MZ tasks")]).await;
        let catalog = catalog_at(env, &server);
        let listed = catalog.check().await;
        let calendar = listed.apps.iter().find(|app| app.window_class == "torro-calendar").expect("calendar");
        assert_eq!(calendar.state, CatalogState::Managed);
        let update = calendar.managed.as_ref().map(|managed| managed.update.as_str());
        assert_eq!(update, Some("scoop update torrocalendar"));
        let tasks = listed.apps.iter().find(|app| app.window_class == "torro-tasks").expect("tasks");
        assert_eq!((tasks.state, tasks.installed.as_deref()), (CatalogState::Managed, None), "as the service came");
        let install = tasks.managed.as_ref().map(|managed| managed.install.as_str());
        assert_eq!(install, Some("scoop install torrotasks"));
        assert_eq!(code(catalog.install("torrotasks").await), Some("conflict"));
        assert_eq!(code(catalog.uninstall("torrocalendar", false).await), Some("conflict"));
    }

    #[tokio::test]
    async fn installing_mail_brings_the_hub_along_checked_and_in_place() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[], true);
        put_program(&env, "torroworkplace");
        let server = MockServer::start_async().await;
        let files = [windows_file("torroinbox", b"MZ hub"), windows_file("torro-inbox-mail", b"MZ mail")];
        serve(&server, "0.2.0", &files).await;
        let catalog = catalog_at(env, &server);
        let started = catalog.install("torro-inbox-mail").await.expect("started");
        assert_eq!(started.apps, ["torro-inbox", "torro-inbox-mail"]);
        assert_eq!(started.kind, AppJobKind::Install);
        assert!(started.id.len() == 16 && started.id.chars().all(|c| c.is_ascii_hexdigit()));
        let job = ended(&catalog, &started.id).await;
        assert_eq!((job.state, job.error.as_deref(), job.percent, job.restart), (AppJobState::Done, None, 100, false));
        assert!(job.steps.iter().all(|step| step.status == StepStatus::Done), "{:?}", job.steps);
        let kinds: Vec<_> =
            job.steps.iter().filter(|step| step.app == "torro-inbox-mail").map(|step| step.step).collect();
        assert_eq!(kinds, [AppStep::Download, AppStep::Verify, AppStep::Install, AppStep::Launcher, AppStep::Connect]);
        let verify = job.steps.iter().find(|step| step.step == AppStep::Verify).and_then(|step| step.detail.clone());
        assert_eq!(verify.as_deref(), Some("SHA-256"));
        let bin = dir.path().join("bin");
        assert_eq!(std::fs::read(bin.join("torroinbox.exe")).expect("hub"), b"MZ hub");
        assert_eq!(std::fs::read(bin.join("torro-inbox-mail.exe")).expect("mail"), b"MZ mail");
        assert_eq!(code(catalog.install("torro-inbox-mail").await), Some("conflict"), "installed");
        assert_eq!(code(catalog.uninstall("torroinbox", false).await), Some("conflict"), "Mail needs the hub");
    }

    #[tokio::test]
    async fn a_wrong_checksum_fails_the_job_and_installs_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[], true);
        let server = MockServer::start_async().await;
        let mut file = windows_file("torrotasks", b"MZ tasks");
        file.sum = Some("0".repeat(64));
        serve(&server, "0.2.0", &[file]).await;
        let catalog = catalog_at(env, &server);
        let started = catalog.install("torro-tasks").await.expect("started");
        let job = ended(&catalog, &started.id).await;
        assert_eq!((job.state, job.error.as_deref()), (AppJobState::Failed, Some("checksum")));
        let failed = job.steps.iter().find(|step| step.status == StepStatus::Failed).map(|step| step.step);
        assert_eq!(failed, Some(AppStep::Verify));
        assert!(!dir.path().join("bin/torrotasks.exe").exists());
    }

    #[tokio::test]
    async fn a_job_is_cancelled_while_it_downloads_and_the_next_waits_for_its_end() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[], true);
        let server = MockServer::start_async().await;
        // The archive answers slowly.
        let file = File { delay: Some(Duration::from_secs(5)), ..windows_file("torrotasks", b"MZ tasks") };
        serve(&server, "0.2.0", &[file]).await;
        let catalog = catalog_at(env, &server);
        let started = catalog.install("torro-tasks").await.expect("started");
        assert_eq!(catalog.install("torro-calendar").await.err(), Some(CatalogError::Busy));
        assert_eq!(catalog.list().await.job.as_deref(), Some(started.id.as_str()));
        // Until the download step has begun.
        for _ in 0..100 {
            let now = catalog.job(&started.id).expect("job").steps.iter().any(|step| step.status == StepStatus::Now);
            if now {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let cancelled = catalog.cancel(&started.id).expect("cancelled");
        assert_eq!((cancelled.state, cancelled.error.as_deref()), (AppJobState::Cancelled, Some("cancelled")));
        let job = ended(&catalog, &started.id).await;
        assert_eq!(job.state, AppJobState::Cancelled);
        assert!(!dir.path().join("bin/torrotasks.exe").exists());
        assert_eq!(catalog.cancel("ffff").err().map(|err| err.code()), Some("not-found"));
    }

    #[tokio::test]
    async fn updating_the_main_application_hands_the_service_over() {
        let dir = tempfile::tempdir().expect("dir");
        let versions = [("torroworkplace", "0.1.0"), ("torrotasks", "0.1.0")];
        let env = env_in(dir.path(), Platform::Windows, &versions, true);
        let service = put_program(&env, "torroworkplace");
        put_program(&env, "torrotasks");
        let server = MockServer::start_async().await;
        let files = [windows_file("torroworkplace", b"MZ new core"), windows_file("torrotasks", b"MZ t")];
        serve(&server, "0.2.0", &files).await;
        let catalog = catalog_at(env, &server);
        let handed: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let record = Arc::clone(&handed);
        catalog.set_hand_over(Arc::new(move |program| record.lock().expect("lock").push(program)));
        let names = ["torroworkplace".to_string(), "torrotasks".to_string(), "torro-tasks".to_string()];
        let started = catalog.update(&names).await.expect("started");
        assert_eq!(started.apps, ["torro-tasks", "torro-workplace"], "once each, the main application last");
        let job = ended(&catalog, &started.id).await;
        assert_eq!((job.state, job.restart), (AppJobState::Done, true), "{job:?}");
        assert_eq!(std::fs::read(&service).expect("core"), b"MZ new core");
        assert!(dir.path().join("bin/torroworkplace.exe.old").exists(), "the running one renamed aside");
        for _ in 0..100 {
            if !handed.lock().expect("lock").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*handed.lock().expect("lock"), [service]);
    }

    #[tokio::test]
    async fn a_newer_copy_of_the_service_is_handed_over_to() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[("torrotasks", "0.1.0")], true);
        put_program(&env, "torrotasks");
        let server = MockServer::start_async().await;
        serve(&server, "9.0.0", &[windows_file("torrotasks", b"MZ t")]).await;
        let catalog = catalog_at(env, &server);
        let newer = dir.path().join("newer/torroworkplace.exe");
        let found = newer.clone();
        catalog.set_newest_service(Arc::new(move || Some((found.clone(), "9.0.0".parse().expect("version")))));
        let handed: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let record = Arc::clone(&handed);
        catalog.set_hand_over(Arc::new(move |program| record.lock().expect("lock").push(program)));
        let started = catalog.update(&["torrotasks".into()]).await.expect("started");
        let job = ended(&catalog, &started.id).await;
        assert!(job.restart, "{job:?}");
        let connect = job.steps.iter().find(|step| step.step == AppStep::Connect).and_then(|step| step.detail.clone());
        assert_eq!(connect.as_deref(), Some("9.0.0"));
        for _ in 0..100 {
            if !handed.lock().expect("lock").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*handed.lock().expect("lock"), [newer]);
    }

    #[tokio::test]
    async fn updates_are_refused_for_what_has_none() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[("torrotasks", "0.2.0")], true);
        put_program(&env, "torrotasks");
        let server = MockServer::start_async().await;
        serve(&server, "0.2.0", &[windows_file("torrotasks", b"MZ t")]).await;
        let catalog = catalog_at(env, &server);
        assert_eq!(code(catalog.update(&["torrotasks".into()]).await), Some("conflict"), "up to date");
        assert_eq!(code(catalog.update(&["torrocalendar".into()]).await), Some("conflict"), "not installed");
        assert_eq!(code(catalog.update(&["torro-compose".into()]).await), Some("not-found"));
        assert_eq!(code(catalog.update(&[]).await), Some("not-found"));
        assert_eq!(code(catalog.install("torro-assistant").await), Some("unsupported"), "not in the release");
    }

    #[tokio::test]
    async fn uninstalling_moves_the_app_away_with_its_own_data_only() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Linux, &[], true);
        let tasks = put_program(&env, "torrotasks");
        put_program(&env, "torroinbox");
        let mail = put_program(&env, "torro-inbox-mail");
        let data = dir.path().join("data");
        for app in ["torrotasks", "torroinbox"] {
            std::fs::create_dir_all(data.join(app)).expect("data");
        }
        install::write_desktop(AppId::TASKS, &tasks, env.data_home.as_deref()).expect("desktop");
        let server = MockServer::start_async().await;
        let catalog = catalog_at(env, &server);
        let started = catalog.uninstall("torro-tasks", true).await.expect("started");
        assert_eq!(started.kind, AppJobKind::Uninstall);
        let job = ended(&catalog, &started.id).await;
        assert_eq!((job.state, job.restart), (AppJobState::Done, false), "{job:?}");
        let kinds: Vec<_> = job.steps.iter().map(|step| step.step).collect();
        assert_eq!(kinds, [AppStep::Remove, AppStep::Launcher, AppStep::Connect]);
        assert!(!tasks.exists() && !data.join("torrotasks").exists());
        assert!(!dir.path().join("share/applications/torro-tasks.desktop").exists());
        // Mail's data is the hub's too, and the hub stays.
        let started = catalog.uninstall("torro-inbox-mail", true).await.expect("started");
        assert_eq!(ended(&catalog, &started.id).await.state, AppJobState::Done);
        assert!(!mail.exists());
        assert!(data.join("torroinbox").exists(), "shared with the hub");
        assert_eq!(code(catalog.uninstall("torro-tasks", false).await), Some("conflict"), "not installed");
        assert_eq!(code(catalog.uninstall("torroworkplace", false).await), Some("not-found"), "the main application");
        assert_eq!(code(catalog.uninstall("torro-compose", false).await), Some("not-found"));
    }

    #[tokio::test]
    async fn uninstalling_on_macos_takes_the_reminder_along() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::MacOs, &[], false);
        let calendar = installed::tests::put_bundle(&env, "Torro Calendar", "torrocalendar", "0.1.0");
        let reminder = installed::tests::put_bundle(&env, "Torro Reminder", "torro-reminder", "0.1.0");
        let server = MockServer::start_async().await;
        let catalog = catalog_at(env, &server);
        let started = catalog.uninstall("torro-calendar", false).await.expect("started");
        let job = ended(&catalog, &started.id).await;
        assert_eq!(job.state, AppJobState::Done, "{job:?}");
        let kinds: Vec<_> = job.steps.iter().map(|step| step.step).collect();
        assert_eq!(kinds, [AppStep::Remove, AppStep::Connect], "no launcher entry on macOS");
        assert!(!calendar.exists() && !reminder.exists());
    }

    #[tokio::test]
    async fn a_development_build_installs_nothing() {
        let catalog = Catalog::none();
        for refused in [
            catalog.install("torro-tasks").await,
            catalog.update(&["torro-tasks".into()]).await,
            catalog.uninstall("torro-tasks", false).await,
        ] {
            assert_eq!(code(refused), Some("unsupported"));
        }
        let listed = catalog.check().await;
        assert_eq!(listed.error.as_deref(), Some("offline"));
        assert!(listed.apps.iter().all(|app| app.state == CatalogState::Unavailable));
        assert_eq!(catalog.job("abc").err().map(|err| err.code()), Some("not-found"));
    }

    #[tokio::test]
    async fn without_github_installing_is_offline() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[], true);
        let releases = Releases::new(Some(releases::tests::nowhere()), releases::DOWNLOADS.into(), None);
        let catalog = Catalog::new(releases, env, false, Duration::ZERO);
        assert_eq!(catalog.install("torro-tasks").await.err(), Some(CatalogError::Offline));
    }

    /// Linux, the whole way: `tar`, checksum, the program beside the service, its `.desktop` entry.
    #[cfg(unix)]
    #[tokio::test]
    async fn installing_on_linux_adds_the_program_and_its_launcher() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Linux, &[], true);
        let server = MockServer::start_async().await;
        let archive = dir.path().join("a.tar.xz");
        install::tests::tar_with(&archive, "torrocalendar", b"ELF calendar");
        let body = std::fs::read(&archive).expect("archive");
        let file = File { name: "torrocalendar-x86_64-unknown-linux-gnu.tar.xz".into(), body, sum: None, delay: None };
        serve(&server, "0.2.0", &[file]).await;
        let catalog = catalog_at(env, &server);
        let started = catalog.install("torrocalendar").await.expect("started");
        let job = ended(&catalog, &started.id).await;
        assert_eq!(job.state, AppJobState::Done, "{job:?}");
        let program = dir.path().join("bin/torrocalendar");
        assert_eq!(std::fs::read(&program).expect("program"), b"ELF calendar");
        let entry = dir.path().join("share/applications/torro-calendar.desktop");
        let desktop = std::fs::read_to_string(entry).expect("entry");
        assert!(desktop.contains(&format!("Exec={}\n", program.display())), "{desktop}");
        let launcher =
            job.steps.iter().find(|step| step.step == AppStep::Launcher).and_then(|step| step.detail.clone());
        assert!(launcher.is_some_and(|path| path.ends_with("torro-calendar.desktop")));
    }

    #[test]
    fn the_service_hands_over_only_when_due() {
        let current: Version = "0.2.0".parse().expect("version");
        let copy =
            |version: &str| Some((PathBuf::from("/Applications/Torro Tasks.app/s"), version.parse().expect("version")));
        let exe = Path::new(
            "/Applications/Torro Calendar.app/Contents/Library/LoginItems/S.app/Contents/MacOS/torroworkplace",
        );
        let nothing = Done::default();
        assert!(hand_over_target(Some(exe), &nothing, copy("0.2.0"), &current).is_none(), "no newer copy");
        assert!(hand_over_target(Some(exe), &nothing, copy("0.3.0"), &current).is_some(), "a newer copy");
        assert!(hand_over_target(Some(exe), &nothing, None, &current).is_none());
        let removed = Done { removed: vec![PathBuf::from("/Applications/Torro Calendar.app")], main_service: None };
        assert!(
            hand_over_target(Some(exe), &removed, copy("0.2.0"), &current).is_some(),
            "ran from the removed bundle"
        );
        assert!(hand_over_target(Some(exe), &removed, None, &current).is_none(), "nowhere to go");
        let main = Done { removed: Vec::new(), main_service: Some(PathBuf::from("/opt/torro/torroworkplace")) };
        let target = hand_over_target(Some(exe), &main, copy("0.2.0"), &current).map(|(program, _)| program);
        assert_eq!(target, Some(PathBuf::from("/opt/torro/torroworkplace")), "always after the main application");
        let target = hand_over_target(Some(exe), &main, copy("0.3.0"), &current).map(|(program, _)| program);
        assert_eq!(target, copy("0.3.0").map(|(program, _)| program), "unless an even newer copy is there");
    }

    #[test]
    fn the_release_names_its_archives_per_system() {
        let calendar = parts(AppId::CALENDAR, Platform::MacOs);
        assert_eq!(calendar, [Part { program: "torrocalendar", title: "Torro Calendar" }, REMINDER]);
        assert_eq!(
            calendar[1].asset(Platform::MacOs, "aarch64-apple-darwin"),
            "torro-reminder-aarch64-apple-darwin.app.zip"
        );
        let mail = parts(AppId::INBOX_MAIL, Platform::Linux);
        let linux = "torro-inbox-mail-x86_64-unknown-linux-gnu.tar.xz";
        assert_eq!(mail[0].asset(Platform::Linux, "x86_64-unknown-linux-gnu"), linux);
        assert_eq!(mail[0].bundle(), "Torro Workplace Mail.app");
        let tasks = parts(AppId::TASKS, Platform::Windows);
        assert_eq!(
            tasks[0].asset(Platform::Windows, "x86_64-pc-windows-msvc"),
            "torrotasks-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(Platform::Windows.target("aarch64"), None, "no Windows build for ARM");
        assert_eq!(Platform::Linux.target("aarch64").as_deref(), Some("aarch64-unknown-linux-gnu"));
        assert_eq!(Platform::MacOs.target("x86_64").as_deref(), Some("x86_64-apple-darwin"));
        assert_eq!(requires(AppId::INBOX_NOTIFICATIONS), [AppId::INBOX]);
        assert!(requires(AppId::INBOX).is_empty());
    }

    /// The fixture release (as GitHub answers it) offers every app on every system.
    #[test]
    fn the_fixture_release_has_every_app_for_every_system() {
        let text = include_str!("../../tests/fixtures/catalog/release.json");
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        let assets = json["assets"]
            .as_array()
            .expect("assets")
            .iter()
            .filter_map(|asset| asset["name"].as_str())
            .map(|name| Asset { name: name.into(), size: 1, url: format!("{}{name}", releases::DOWNLOADS) })
            .collect();
        let stored = Stored { latest: Some("0.2.0".into()), assets, ..Stored::default() };
        for platform in [Platform::MacOs, Platform::Linux, Platform::Windows] {
            let env = Env { arch: "x86_64".into(), ..Env::empty(platform) };
            for app in AppId::ALL {
                assert!(release_parts(&env, *app, &stored).is_some(), "{} on {platform:?}", app.window_class);
            }
        }
    }
}
