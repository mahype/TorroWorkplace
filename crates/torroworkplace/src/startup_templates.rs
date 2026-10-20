//! Loading templates at login (issue #243, task WS.5): every template with a
//! [`torro_setups::Startup`] is loaded once per session, in the mode `add`, onto the workspace its
//! screen shows.
//!
//! The core starts [`run`] in the background when the daemon starts (`Core::start`). It:
//!
//! 1. compares the session ([`Session::current`]) with the marker [`FILE`] in the state directory and
//!    stops when they are the same: the daemon was only restarted, the windows are there already;
//! 2. waits until the window manager reports the same screens [`SAME_IN_A_ROW`] times in a row
//!    ([`settled_monitors`]), because right after a login screens still come up one by one;
//! 3. loads the templates in the order of their keys, then by name ([`in_order`]): each onto its
//!    screen, else as its [`torro_setups::ScreenFallback`] says
//!    ([`torro_setups::Startup::monitor_in`]), by focusing the screen and loading as the menu's "Add"
//!    does; a failing template does not stop the others ([`load_all`]);
//! 4. gives the focus back to the screen that had it and writes the marker.
//!
//! The core runs the pass holding the turn of saves and loads (`Core::load_at_login`), after any one
//! that runs is over: no other save or load moves the focus between focusing a screen and loading onto
//! it, and the pass moves none while another one runs.
//!
//! The marker is written once the pass ran, also when single templates failed: a failure is reported
//! (stderr and a notification, [`report`]) and would only fail again, so trying anew at every restart
//! of the daemon in the same session would load the templates that did work a second time. It is not
//! written when the pass never ran (no screens, the templates unreadable): then nothing was loaded, and
//! the next start of the daemon in this session may try again.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use torro_core::i18n::trf;
use torro_ipc::Response;
use torro_setups::{SetupsError, Template};
use torro_wm::{Monitor, WindowManager, WmError};

/// The marker in the core's state directory: the session the templates were last loaded in.
pub const FILE: &str = "startup.json";

/// How many equal answers in a row count as settled screens.
pub const SAME_IN_A_ROW: usize = 3;

/// Two boot times this close (in seconds) are the same boot. Windows computes the boot time from the
/// clock and the uptime, so it moves by a second, and by the clock's correction after the start; a
/// second boot within a minute of the first, with templates loaded in between, does not happen.
const BOOT_SLACK: u64 = 60;

/// How [`settled_monitors`] asks for the screens: every `every`, `tries` times at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    pub every: Duration,
    pub tries: u32,
}

impl Pace {
    /// At login: once a second, for a minute at most.
    pub const LOGIN: Pace = Pace { every: Duration::from_secs(1), tries: 60 };
}

/// One session of the person: what tells a login from a restart of the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Session {
    /// Hyprland's `HYPRLAND_INSTANCE_SIGNATURE`: new with every start of Hyprland, so with every login.
    Hyprland(String),
    /// Elsewhere the system's start, in seconds since the epoch (`sysinfo::System::boot_time`). A log
    /// out and in again without a restart is the same session there.
    Boot(u64),
}

impl Session {
    /// The session this process runs in: Hyprland's instance when it is set, else the system's start.
    pub fn current() -> Session {
        Session::of(std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(), sysinfo::System::boot_time())
    }

    /// [`Session::current`] from its parts: an empty signature counts as none.
    fn of(signature: Option<String>, boot: u64) -> Session {
        match signature.filter(|signature| !signature.is_empty()) {
            Some(signature) => Session::Hyprland(signature),
            None => Session::Boot(boot),
        }
    }

    /// Whether `self` and `other` are one session; boot times within [`BOOT_SLACK`] are.
    pub fn is(&self, other: &Session) -> bool {
        match (self, other) {
            (Session::Hyprland(a), Session::Hyprland(b)) => a == b,
            (Session::Boot(a), Session::Boot(b)) => a.abs_diff(*b) <= BOOT_SLACK,
            _ => false,
        }
    }
}

/// What [`FILE`] holds.
#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    session: Session,
}

/// Whether the marker in `file` names `session`. A missing or unreadable marker names none.
pub fn already_loaded(file: &Path, session: &Session) -> bool {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
        .is_some_and(|marker| marker.session.is(session))
}

/// Writes the marker for `session` to `file` (0600), creating the folder.
pub fn mark(file: &Path, session: &Session) -> Result<(), String> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|err| format!("{}: {err}", folder.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&Marker { session: session.clone() }).map_err(|err| err.to_string())?;
    crate::clients::write_private(file, &bytes).map_err(|err| err.to_string())
}

/// The screens once `read` gave the same ones [`SAME_IN_A_ROW`] times in a row, asking every
/// `pace.every` and `pace.tries` times at most.
///
/// Only the layout counts (names, places, sizes, scales), not the workspace each one shows, which the
/// person may switch meanwhile. An error and an empty list are no answer: right after a login the
/// window manager may not know any screen yet, and loading onto none would skip every template.
/// When the screens never settle, the last answer is taken (they are there, just changing); `None`
/// when there was none at all.
pub async fn settled_monitors<F, Fut>(mut read: F, pace: Pace) -> Option<Vec<Monitor>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<Monitor>, WmError>>,
{
    let mut last: Option<Vec<Monitor>> = None;
    let mut same = 0;
    for attempt in 0..pace.tries {
        if attempt > 0 {
            tokio::time::sleep(pace.every).await;
        }
        match read().await {
            Ok(monitors) if !monitors.is_empty() => {
                same = match &last {
                    Some(before) if layout(before) == layout(&monitors) => same + 1,
                    _ => 1,
                };
                last = Some(monitors);
                if same >= SAME_IN_A_ROW {
                    break;
                }
            }
            _ => same = 0,
        }
    }
    last
}

/// What [`settled_monitors`] compares of the screens.
fn layout(monitors: &[Monitor]) -> Vec<(&str, &torro_wm::Rect, f32)> {
    monitors.iter().map(|monitor| (monitor.name.as_str(), &monitor.rect, monitor.scale)).collect()
}

/// The templates of `templates` that load at login, in the order they load: those with a key in the
/// order of the keyboard (1 … 9, 0, as [`crate::workspace_keys::slots`] gives them), then the others
/// by name.
pub fn in_order(templates: Vec<Template>) -> Vec<Template> {
    let slots = crate::workspace_keys::slots(&templates);
    let rank = |template: &Template| slots.iter().position(|slot| slot.template == template.id).unwrap_or(slots.len());
    let mut loading: Vec<Template> = templates.into_iter().filter(|template| template.startup.is_some()).collect();
    loading.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
    loading
}

/// How loading one template at login went.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The load ran, or its screen could not be focused (then an `error`): the answer, as the menu's
    /// "Add" gets it (`Response::Loaded`, or an error such as `not-found`). Boxed: an answer is large.
    Ran(Box<Response>),
    /// Its screen is missing and its [`ScreenFallback::Skip`](torro_setups::ScreenFallback::Skip) says
    /// not to load it elsewhere.
    Skipped,
}

/// One template's [`Outcome`], with what [`report`] names.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// The template's id ([`Template::id`]).
    pub id: String,
    /// Its name, for people.
    pub name: String,
    /// The screen it was meant for ([`torro_setups::Startup::screen`]).
    pub screen: String,
    pub outcome: Outcome,
}

/// Loads each of `templates` (in this order) onto its screen among `monitors`: focuses the screen
/// [`torro_setups::Startup::monitor_in`] picks, then `load` with the template's id, which must load it
/// like `Request::LoadTemplate { mode: Add }` does (the core loads it the same way, `Core::load_template`).
/// A template with no screen is [`Outcome::Skipped`], one whose screen cannot be focused fails without
/// loading, so it never lands on the wrong screen; neither stops the others. The caller holds the turn
/// of saves and loads, so `load` never answers `busy` and nothing else moves the focus in between.
///
/// Afterwards the screen that showed the active workspace before gets the focus back, so the person
/// finds the screen they were on; when none did, the focus stays where the last load left it.
/// Templates without a startup are left out.
pub async fn load_all<L, Fut>(
    wm: &impl WindowManager,
    templates: &[Template],
    monitors: &[Monitor],
    mut load: L,
) -> Vec<Done>
where
    L: FnMut(String) -> Fut,
    Fut: Future<Output = Response>,
{
    let active = wm.active_workspace().await.ok();
    let before = monitors
        .iter()
        .find(|monitor| active.is_some() && monitor.active_workspace == active)
        .map(|monitor| monitor.name.clone());
    let mut done = Vec::new();
    for template in templates {
        let Some(startup) = &template.startup else { continue };
        let outcome = match startup.monitor_in(monitors) {
            None => Outcome::Skipped,
            Some(monitor) => match wm.focus_monitor(&monitor.name).await {
                Ok(()) => Outcome::Ran(Box::new(load(template.id.clone()).await)),
                Err(err) => Outcome::Ran(Box::new(Response::Error {
                    code: "error".into(),
                    message: trf(
                        "Screen {screen} could not be focused: {error}",
                        &[("screen", &monitor.name as &dyn std::fmt::Display), ("error", &err)],
                    ),
                })),
            },
        };
        done.push(Done {
            id: template.id.clone(),
            name: template.name.clone(),
            screen: startup.screen.clone(),
            outcome,
        });
    }
    if let Some(before) = before
        && let Err(err) = wm.focus_monitor(&before).await
    {
        eprintln!("torroworkplace: cannot give the focus back to screen {before} after loading at login: {err}");
    }
    done
}

/// Where [`run`] keeps its marker and which session it runs in.
#[derive(Debug, Clone)]
pub struct Marking<'a> {
    /// The marker [`FILE`]; `None` without a home directory, then the templates load at every start.
    pub file: Option<&'a Path>,
    pub session: Session,
}

/// The whole pass at the daemon's start (see the module): nothing when `marking` says this session
/// loaded already; else waits for the screens ([`settled_monitors`] with `pace`), loads the templates
/// that load at login ([`in_order`], [`load_all`] with `load`) and writes the marker. The caller holds
/// the turn of saves and loads for the whole pass (see the module).
///
/// `templates` is what the store listed. Returns what was done, `None` when the pass did not run:
/// loaded already in this session, the templates unreadable, or no screens. Without templates to load
/// the marker is written all the same, so a template set to load at login later in the session waits
/// for the next login instead of loading when the daemon restarts.
pub async fn run<L, Fut>(
    wm: &impl WindowManager,
    templates: Result<Vec<Template>, SetupsError>,
    marking: Marking<'_>,
    pace: Pace,
    load: L,
) -> Option<Vec<Done>>
where
    L: FnMut(String) -> Fut,
    Fut: Future<Output = Response>,
{
    if marking.file.is_some_and(|file| already_loaded(file, &marking.session)) {
        return None;
    }
    let templates = match templates {
        Ok(templates) => in_order(templates),
        Err(err) => {
            eprintln!("torroworkplace: cannot read the templates to load at login: {err}");
            return None;
        }
    };
    let done = match templates.is_empty() {
        true => Vec::new(),
        false => {
            let Some(monitors) = settled_monitors(|| wm.monitors(), pace).await else {
                eprintln!("torroworkplace: no screens to load the templates onto at login");
                return None;
            };
            load_all(wm, &templates, &monitors, load).await
        }
    };
    if let Some(file) = marking.file
        && let Err(err) = mark(file, &marking.session)
    {
        eprintln!("torroworkplace: cannot remember that the templates were loaded at login: {err}");
    }
    Some(done)
}

/// Tells the person how loading `done` went, as the daemon reports a command of the menu: every
/// template on stderr; a notification only for one that failed (in part), with its name in front, since
/// loads that worked show on the screens themselves and a login brings no row of "Template loaded".
/// stderr names the template by its id, never by its name: the log holds no contents a person wrote
/// (`docs/api.md`), the notification is for that person and names it.
pub fn report(done: &[Done]) {
    for one in done {
        let what = format!("load template {} at login", one.id);
        let response = match &one.outcome {
            Outcome::Skipped => {
                eprintln!("torroworkplace: {what}: skipped, screen {} is missing", one.screen);
                continue;
            }
            Outcome::Ran(response) => &**response,
        };
        match response {
            Response::Error { message, .. } => eprintln!("torroworkplace: {what} failed: {message}"),
            Response::Loaded { restored, failed } => {
                eprintln!("torroworkplace: {what}: {restored} windows restored, {} failed", failed.len());
                for (window, reason) in failed {
                    eprintln!("  {window}: {reason}");
                }
            }
            _ => eprintln!("torroworkplace: {what}: ok"),
        }
        if failed(response)
            && let Some((summary, body)) = crate::notify::message(&what, response)
        {
            crate::notify::show(summary, format!("{}\n{body}", one.name));
        }
    }
}

/// Whether `response` to a load says something went wrong: an error, or windows that did not come.
fn failed(response: &Response) -> bool {
    match response {
        Response::Error { .. } => true,
        Response::Loaded { failed, .. } => !failed.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use torro_setups::{ScreenFallback, Startup};
    use torro_wm::fake::{Call, FakeWm};
    use torro_wm::{Rect, WorkspaceId};

    use super::*;

    /// No waiting in tests: the screens are asked for one after the other.
    const QUICK: Pace = Pace { every: Duration::ZERO, tries: 60 };

    fn monitor(name: &str, x: i32, workspace: &str) -> Monitor {
        Monitor {
            name: name.into(),
            rect: Rect { x, y: 0, width: 1920, height: 1080 },
            scale: 1.0,
            active_workspace: Some(WorkspaceId(workspace.into())),
        }
    }

    /// `DP-1` at 0,0 (the main screen) shows workspace 1 and has the focus, `HDMI-A-1` shows 7.
    fn two_screens() -> FakeWm {
        let wm = FakeWm::new("1");
        wm.set_monitors(vec![monitor("DP-1", 0, "1"), monitor("HDMI-A-1", 1920, "7")]);
        wm
    }

    /// A template `id` (also its name) with key `key`, loading at login onto `screen` unless `None`.
    fn template(id: &str, key: Option<u8>, screen: Option<(&str, ScreenFallback)>) -> Template {
        let now = chrono::Utc::now();
        Template {
            schema_version: torro_setups::SCHEMA_VERSION,
            id: id.into(),
            name: id.into(),
            wm: "hyprland".into(),
            created: now,
            updated: now,
            source: torro_setups::Source { workspace: "1".into(), monitor: None, monitor_rect: None },
            tree: None,
            windows: vec![],
            shortcut: key,
            startup: screen.map(|(screen, fallback)| Startup { screen: screen.into(), fallback }),
        }
    }

    fn loaded() -> Response {
        Response::Loaded { restored: 2, failed: vec![] }
    }

    /// Records which template loaded on which workspace (the active one when `load` ran).
    struct Loads<'a> {
        wm: &'a FakeWm,
        seen: Mutex<Vec<(String, WorkspaceId)>>,
    }

    impl<'a> Loads<'a> {
        fn new(wm: &'a FakeWm) -> Loads<'a> {
            Loads { wm, seen: Mutex::new(Vec::new()) }
        }

        async fn load(&self, id: String) -> Response {
            let workspace = self.wm.active_workspace().await.expect("active workspace");
            self.seen.lock().expect("loads").push((id, workspace));
            loaded()
        }

        fn seen(&self) -> Vec<(String, WorkspaceId)> {
            self.seen.lock().expect("loads").clone()
        }
    }

    fn ws(name: &str) -> WorkspaceId {
        WorkspaceId(name.into())
    }

    #[tokio::test]
    async fn a_second_start_in_the_same_session_loads_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("state").join(FILE);
        let wm = two_screens();
        let loads = Loads::new(&wm);
        let session = Session::Hyprland("abc_123".into());
        let templates = || Ok(vec![template("dev", None, Some(("HDMI-A-1", ScreenFallback::Main)))]);
        let marking = || Marking { file: Some(&file), session: session.clone() };

        let first = run(&wm, templates(), marking(), QUICK, |id| loads.load(id)).await;
        assert_eq!(first.map(|done| done.len()), Some(1));
        assert!(already_loaded(&file, &session));
        let calls = wm.calls().len();

        let second = run(&wm, templates(), marking(), QUICK, |id| loads.load(id)).await;
        assert_eq!(second, None);
        assert_eq!(loads.seen().len(), 1, "loaded once");
        assert_eq!(wm.calls().len(), calls, "the second start touched no screen");

        // A new login (Hyprland started anew) loads again.
        let next = Marking { file: Some(&file), session: Session::Hyprland("def_456".into()) };
        assert!(run(&wm, templates(), next, QUICK, |id| loads.load(id)).await.is_some());
        assert_eq!(loads.seen().len(), 2);
    }

    #[tokio::test]
    async fn a_missing_screen_falls_back_to_the_main_one_or_is_skipped() {
        let wm = two_screens();
        wm.focus_workspace("7");
        let loads = Loads::new(&wm);
        let templates = vec![
            template("main", Some(1), Some(("DP-9", ScreenFallback::Main))),
            template("skip", Some(2), Some(("DP-9", ScreenFallback::Skip))),
            template("there", Some(3), Some(("HDMI-A-1", ScreenFallback::Skip))),
        ];
        let monitors = wm.monitors().await.expect("monitors");

        let done = load_all(&wm, &templates, &monitors, |id| loads.load(id)).await;

        let outcomes: Vec<(&str, &Outcome)> = done.iter().map(|one| (one.id.as_str(), &one.outcome)).collect();
        assert_eq!(
            outcomes,
            vec![
                ("main", &Outcome::Ran(Box::new(loaded()))),
                ("skip", &Outcome::Skipped),
                ("there", &Outcome::Ran(Box::new(loaded())))
            ]
        );
        assert_eq!(loads.seen(), vec![("main".to_string(), ws("1")), ("there".to_string(), ws("7"))]);
        assert_eq!(done[1].screen, "DP-9");
    }

    #[tokio::test]
    async fn templates_load_by_key_then_by_name_and_the_focus_goes_back() {
        let wm = two_screens();
        let loads = Loads::new(&wm);
        let on = |screen| Some((screen, ScreenFallback::Main));
        let templates = vec![
            template("beta", None, on("DP-1")),
            template("alpha", None, on("HDMI-A-1")),
            template("zero", Some(0), on("HDMI-A-1")),
            template("two", Some(2), on("DP-1")),
            template("no-startup", Some(1), None),
        ];
        let marking = Marking { file: None, session: Session::Boot(1) };

        let done = run(&wm, Ok(templates), marking, QUICK, |id| loads.load(id)).await.expect("ran");

        let order: Vec<&str> = done.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(order, ["two", "zero", "alpha", "beta"]);
        assert_eq!(
            loads.seen(),
            vec![
                ("two".to_string(), ws("1")),
                ("zero".to_string(), ws("7")),
                ("alpha".to_string(), ws("7")),
                ("beta".to_string(), ws("1")),
            ]
        );
        // DP-1 had the focus before; it gets it back after the last load.
        assert_eq!(wm.calls().last(), Some(&Call::FocusMonitor("DP-1".into())));
        assert_eq!(wm.active_workspace().await, Ok(ws("1")));
    }

    #[tokio::test]
    async fn the_focus_goes_back_to_the_screen_that_had_it() {
        let wm = two_screens();
        wm.focus_workspace("7");
        let loads = Loads::new(&wm);
        let templates = vec![template("dev", None, Some(("DP-1", ScreenFallback::Main)))];
        let monitors = wm.monitors().await.expect("monitors");

        load_all(&wm, &templates, &monitors, |id| loads.load(id)).await;

        assert_eq!(wm.calls(), vec![Call::FocusMonitor("DP-1".into()), Call::FocusMonitor("HDMI-A-1".into())]);
        assert_eq!(wm.active_workspace().await, Ok(ws("7")));
    }

    /// A screen that cannot be focused would put the windows elsewhere: that template fails unloaded.
    #[tokio::test]
    async fn a_screen_that_cannot_be_focused_loads_nothing_there() {
        let wm = two_screens();
        let mut blank = monitor("DP-2", 3840, "9");
        blank.active_workspace = None;
        wm.set_monitors(vec![monitor("DP-1", 0, "1"), blank]);
        let loads = Loads::new(&wm);
        let templates = vec![
            template("lost", Some(1), Some(("DP-2", ScreenFallback::Main))),
            template("dev", Some(2), Some(("DP-1", ScreenFallback::Main))),
        ];
        let monitors = wm.monitors().await.expect("monitors");

        let done = load_all(&wm, &templates, &monitors, |id| loads.load(id)).await;

        assert!(
            matches!(&done[0].outcome, Outcome::Ran(answer) if matches!(**answer, Response::Error { ref code, .. } if code == "error")),
            "{:?}",
            done[0].outcome
        );
        assert_eq!(loads.seen(), vec![("dev".to_string(), ws("1"))]);
    }

    /// A failing template stops neither the others nor the marker: trying again at the next restart
    /// would load the others a second time.
    #[tokio::test]
    async fn a_failing_template_stops_neither_the_others_nor_the_marker() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        let wm = two_screens();
        let broken = Response::Error { code: "not-found".into(), message: "no template first".into() };
        let templates = vec![
            template("first", Some(1), Some(("DP-1", ScreenFallback::Main))),
            template("second", Some(2), Some(("DP-1", ScreenFallback::Main))),
        ];
        let seen = Mutex::new(Vec::new());
        let load = |id: String| {
            seen.lock().expect("seen").push(id.clone());
            let answer = if id == "first" { broken.clone() } else { loaded() };
            async move { answer }
        };
        let session = Session::Boot(1_700_000_000);
        let marking = Marking { file: Some(&file), session: session.clone() };

        let done = run(&wm, Ok(templates), marking, QUICK, load).await.expect("ran");

        assert_eq!(done[0].outcome, Outcome::Ran(Box::new(broken)));
        assert_eq!(done[1].outcome, Outcome::Ran(Box::new(loaded())));
        assert_eq!(*seen.lock().expect("seen"), ["first", "second"], "one failing does not stop the next");
        assert!(already_loaded(&file, &session));
    }

    /// Without screens nothing loads and nothing is marked, so the next start may try again.
    #[tokio::test]
    async fn without_screens_nothing_loads_and_nothing_is_marked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        let wm = FakeWm::new("1");
        wm.set_monitors(vec![]);
        let loads = Loads::new(&wm);
        let templates = vec![template("dev", None, Some(("DP-1", ScreenFallback::Main)))];
        let session = Session::Boot(5);
        let marking = Marking { file: Some(&file), session: session.clone() };

        let pace = Pace { every: Duration::ZERO, tries: 3 };
        assert_eq!(run(&wm, Ok(templates), marking, pace, |id| loads.load(id)).await, None);
        assert!(loads.seen().is_empty());
        assert!(!file.exists());
    }

    /// No template to load: the marker is written all the same, and no screen is asked for.
    #[tokio::test]
    async fn without_templates_to_load_the_session_is_marked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        let wm = two_screens();
        let loads = Loads::new(&wm);
        let session = Session::Hyprland("abc".into());
        let marking = Marking { file: Some(&file), session: session.clone() };

        let done = run(&wm, Ok(vec![template("dev", Some(1), None)]), marking, QUICK, |id| loads.load(id)).await;

        assert_eq!(done, Some(vec![]));
        assert!(already_loaded(&file, &session));
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn screens_count_once_the_same_layout_comes_three_times_in_a_row() {
        let a = monitor("DP-1", 0, "1");
        let b = monitor("HDMI-A-1", 1920, "7");
        // An error and an empty list are no answer; a workspace switch is no change of layout.
        let mut switched = b.clone();
        switched.active_workspace = Some(ws("8"));
        let answers: Vec<Result<Vec<Monitor>, WmError>> = vec![
            Err(WmError::NotFound("not yet".into())),
            Ok(vec![]),
            Ok(vec![a.clone()]),
            Ok(vec![a.clone(), b.clone()]),
            Ok(vec![a.clone(), b.clone()]),
            Ok(vec![a.clone(), switched.clone()]),
            Ok(vec![a.clone()]),
        ];
        let asked = Mutex::new(answers.into_iter());
        let read = || {
            let next = asked.lock().expect("answers").next().unwrap_or(Ok(vec![]));
            async move { next }
        };

        let settled = settled_monitors(read, QUICK).await;

        assert_eq!(settled, Some(vec![a.clone(), switched]));
        assert_eq!(asked.lock().expect("answers").len(), 1, "stopped after the third equal answer");
    }

    #[tokio::test]
    async fn screens_that_never_settle_give_the_last_answer_and_none_give_none() {
        let a = monitor("DP-1", 0, "1");
        let b = monitor("HDMI-A-1", 1920, "7");
        let turn = Mutex::new(0);
        let read = || {
            let mut turn = turn.lock().expect("turn");
            *turn += 1;
            let answer = if *turn % 2 == 0 { vec![a.clone()] } else { vec![a.clone(), b.clone()] };
            async move { Ok(answer) }
        };
        let pace = Pace { every: Duration::ZERO, tries: 5 };
        assert_eq!(settled_monitors(read, pace).await, Some(vec![a.clone(), b.clone()]));
        assert_eq!(*turn.lock().expect("turn"), 5);

        let nothing = || async { Err::<Vec<Monitor>, _>(WmError::NotFound("no".into())) };
        assert_eq!(settled_monitors(nothing, pace).await, None);
    }

    #[test]
    fn the_session_is_hyprland_s_instance_or_else_the_boot() {
        assert_eq!(Session::of(Some("abc".into()), 7), Session::Hyprland("abc".into()));
        assert_eq!(Session::of(Some(String::new()), 7), Session::Boot(7));
        assert_eq!(Session::of(None, 7), Session::Boot(7));

        assert!(Session::Boot(1_000).is(&Session::Boot(1_001)), "the boot time moves by a second");
        assert!(Session::Boot(1_000).is(&Session::Boot(1_000 + BOOT_SLACK)));
        assert!(!Session::Boot(1_000).is(&Session::Boot(1_001 + BOOT_SLACK)));
        assert!(!Session::Hyprland("a".into()).is(&Session::Hyprland("b".into())));
        assert!(!Session::Hyprland("1000".into()).is(&Session::Boot(1_000)));
    }

    #[test]
    fn a_missing_or_broken_marker_names_no_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(FILE);
        let session = Session::Boot(1);
        assert!(!already_loaded(&file, &session));
        std::fs::write(&file, "{ not json").expect("damage");
        assert!(!already_loaded(&file, &session));
        mark(&file, &session).expect("mark");
        assert!(already_loaded(&file, &session));
    }

    #[test]
    fn only_failures_count_as_failed() {
        assert!(!failed(&loaded()));
        assert!(failed(&Response::Loaded { restored: 1, failed: vec![("kitty".into(), "did not start".into())] }));
        assert!(failed(&Response::Error { code: "busy".into(), message: "busy".into() }));
    }
}
