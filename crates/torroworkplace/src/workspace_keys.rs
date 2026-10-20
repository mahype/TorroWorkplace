//! The template keys (issue #243): the modifiers of the settings and a digit load the template that
//! holds the digit, like "Load" in the mode `add`.
//!
//! Three parts; the first is shared by the other two:
//!
//! - **What the keys are**, for every system: [`slots`] (which digit loads which template) and
//!   [`label`] (how people read a key: "Super + Alt + 5" on Linux, "Ctrl + Option + 5" on macOS).
//! - **Hyprland** (task WS.3): [`hyprland_binds`] turns the slots into [`KeyBind`]s, [`BoundKeys`]
//!   remembers what it bound and applies only the difference, and [`follow`] binds everything anew
//!   after Hyprland reloaded its configuration, which drops every bind made at run time. Binds made
//!   at run time outlive the core, so [`Ledger`] writes them down for the next core of the same
//!   Hyprland instance, which releases them first, and the core releases them when it ends
//!   ([`BoundKeys::release`]).
//! - **macOS and Windows** (task WS.4): the tray's event loop registers the keys with the system
//!   (`global-hotkey`, which wants the main thread). It gets [`TemplateKeys`] with every snapshot of
//!   the menu, [`hotkey`] maps a key to the system's, and [`DesktopKeys`] registers only the
//!   difference and says which template a pressed key loads.
//!
//! Next to them, [`in_key_order`] and [`manager_name`] keep the workspaces page and the tray menu
//! in step: the same order of templates, the same name of the window manager.
//!
//! The core decides when on Hyprland ([`crate::core::Core`]): at its start, after a template was
//! saved, captured again or deleted, a key was set and the modifiers changed ([`rebinds_after`]).
//! On macOS and Windows the same requests refresh the menu (`crate::daemon`), and the new snapshot
//! brings the keys. Elsewhere nothing binds them.

use std::slice;

use torro_core::i18n::trf;
use torro_core::settings::ShortcutModifiers;
use torro_ipc::{Request, SettingKey};
use torro_setups::{MAX_SHORTCUT, Template, TemplateSummary};
use torro_wm::{Events, KeyBind, WindowManager, WmEvent};

/// One template key: the digit `key` (`0` to `9`, `0` is the key "0") loads the template `template`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub key: u8,
    /// The template's id ([`Template::id`]).
    pub template: String,
}

/// The keys `templates` hold, in the order of the keyboard: 1 … 9, then 0.
///
/// A key belongs to one template only. The store keeps it that way (`Store::set_shortcut`); a file
/// edited by hand may break it, then the first of `templates` (the store lists them by name) keeps
/// the key. Keys above [`MAX_SHORTCUT`] are not keys and are left out.
pub fn slots(templates: &[Template]) -> Vec<Slot> {
    slots_of(templates.iter().map(|template| (template.id.as_str(), template.shortcut)))
}

/// [`slots`] for the summaries the tray's snapshot holds (`Snapshot::templates`), with the same rules.
#[cfg(any(target_os = "macos", windows))]
pub fn summary_slots(templates: &[TemplateSummary]) -> Vec<Slot> {
    slots_of(templates.iter().map(|template| (template.id.as_str(), template.shortcut)))
}

/// [`slots`] of `(id, key)` pairs, in the order the store lists them.
fn slots_of<'a>(templates: impl Iterator<Item = (&'a str, Option<u8>)>) -> Vec<Slot> {
    let mut slots: Vec<Slot> = Vec::new();
    for (id, key) in templates {
        let Some(key) = key.filter(|key| *key <= MAX_SHORTCUT) else { continue };
        if slots.iter().all(|slot| slot.key != key) {
            slots.push(Slot { key, template: id.to_string() });
        }
    }
    slots.sort_by_key(|slot| keyboard_order(slot.key));
    slots
}

/// `templates` in the order people look for them (task WS.7): by key in the order of the keyboard
/// (1 … 9, then 0), then those without a key by name, ignoring case. The tray lists them so, and the
/// workspaces page sorts the same way. A key above [`MAX_SHORTCUT`] counts as none.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub fn in_key_order(templates: &[TemplateSummary]) -> Vec<&TemplateSummary> {
    let mut sorted: Vec<&TemplateSummary> = templates.iter().collect();
    sorted.sort_by_cached_key(|template| {
        let place = template.shortcut.filter(|key| *key <= MAX_SHORTCUT).map_or(u8::MAX, keyboard_order);
        (place, template.name.to_lowercase())
    });
    sorted
}

/// The name of the window manager the core reports (`Status::backend`, `Snapshot::backend`:
/// `hyprland`) as people write it, for the workspaces page and the tray alike. Names, not
/// translated; one this version does not know stays as reported.
pub fn manager_name(backend: &str) -> String {
    match backend {
        "hyprland" => "Hyprland".into(),
        "aerospace" => "AeroSpace".into(),
        "glazewm" => "GlazeWM".into(),
        other => other.to_string(),
    }
}

/// Where `key` stands on the keyboard's row of digits: 1 first, 0 last.
fn keyboard_order(key: u8) -> u8 {
    (key + MAX_SHORTCUT) % (MAX_SHORTCUT + 1)
}

/// The systems whose keyboards name the modifiers differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum System {
    Linux,
    MacOs,
    Windows,
}

impl System {
    /// The system this runs on; every other one names the keys as Linux does.
    const CURRENT: System = if cfg!(target_os = "macos") {
        System::MacOs
    } else if cfg!(windows) {
        System::Windows
    } else {
        System::Linux
    };
}

/// The two modifiers as the keyboard of `system` names them: Super is Cmd on macOS and Win on
/// Windows, Alt is Option on macOS. Key names are not translated anywhere in the suite ("Ctrl K"), so
/// neither are these.
fn modifier_names(modifiers: ShortcutModifiers, system: System) -> [&'static str; 2] {
    let linux = match modifiers {
        ShortcutModifiers::SuperAlt => ["Super", "Alt"],
        ShortcutModifiers::SuperCtrl => ["Super", "Ctrl"],
        ShortcutModifiers::CtrlAlt => ["Ctrl", "Alt"],
        ShortcutModifiers::CtrlShift => ["Ctrl", "Shift"],
    };
    linux.map(|name| match (system, name) {
        (System::MacOs, "Super") => "Cmd",
        (System::MacOs, "Alt") => "Option",
        (System::Windows, "Super") => "Win",
        _ => name,
    })
}

/// `key` with `modifiers` as people on `system` read it.
fn label_on(system: System, modifiers: ShortcutModifiers, key: u8) -> String {
    let [first, second] = modifier_names(modifiers, system);
    format!("{first} + {second} + {key}")
}

/// `key` with `modifiers` as people read it on this system: `Super + Alt + 5` on Linux,
/// `Ctrl + Option + 5` or `Cmd + Option + 5` on macOS, `Win + Alt + 5` on Windows. For messages and
/// the control centre; Hyprland gets [`hyprland_keys`].
pub fn label(modifiers: ShortcutModifiers, key: u8) -> String {
    label_on(System::CURRENT, modifiers, key)
}

/// The modifiers alone as people read them on this system (`Super + Alt`, macOS `Ctrl + Option`): the
/// control centre's choice of modifiers and the "Shortcut" line of the template dialog (task WS.6).
pub fn modifiers_label(modifiers: ShortcutModifiers) -> String {
    let [first, second] = modifier_names(modifiers, System::CURRENT);
    format!("{first} + {second}")
}

/// `key` with `modifiers` in Hyprland's spelling: `SUPER + ALT + 5`. [`KeyBind::keys`] keeps it, and
/// unbinding needs exactly this text again, so it is made here only.
pub fn hyprland_keys(modifiers: ShortcutModifiers, key: u8) -> String {
    label_on(System::Linux, modifiers, key).to_uppercase()
}

/// The command a template key runs: `program ctl setups load <id> --mode add`, `program` being the
/// core's own binary (`crate::core::sibling`). The same command line as typed in a shell, so a key
/// goes through the socket and the core's checks like every other `ctl` call.
pub fn load_command(program: &str, id: &str) -> Vec<String> {
    [program, "ctl", "setups", "load", id, "--mode", "add"].map(str::to_string).to_vec()
}

/// The Hyprland binds for `slots` with `modifiers`, each running [`load_command`] with `program`.
pub fn hyprland_binds(slots: &[Slot], modifiers: ShortcutModifiers, program: &str) -> Vec<KeyBind> {
    slots
        .iter()
        .map(|slot| KeyBind {
            keys: hyprland_keys(modifiers, slot.key),
            command: load_command(program, &slot.template),
        })
        .collect()
}

/// Whether the keys may have changed once `request` ran without an error: a template saved, captured
/// again or deleted, a key set, the modifiers changed (one setting or all settings at once). Renaming
/// keeps the id the key runs, and loading changes no template, so neither is here.
pub fn rebinds_after(request: &Request) -> bool {
    matches!(
        request,
        Request::SaveTemplate { .. }
            | Request::UpdateTemplate { .. }
            | Request::DeleteTemplate { .. }
            | Request::SetTemplateShortcut { .. }
            | Request::SetSetting { key: SettingKey::WorkspaceShortcuts, .. }
            | Request::UpdateSettings { .. }
    )
}

/// How the keys stand after binding them, for `Core::report_shortcuts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every key that should be bound is, and none that should not be.
    pub active: bool,
    /// Why not, for people: which keys could not be registered or released.
    pub problem: Option<String>,
}

/// The file in the core's state directory where [`Ledger`] writes the bound keys down.
pub const KEYS_FILE: &str = "template-keys.json";

/// Where the keys bound on Hyprland are written down, with the Hyprland instance they were bound on,
/// so the next core of the same session finds them.
///
/// Binds made at run time outlive the core: they stay until Hyprland reloads its configuration or
/// ends. A core that was killed, or ended before it could release them, leaves them behind, and a new
/// core binding the same keys would make each key run twice (Hyprland runs every bind of a key) or
/// keep an old key alive. So every change of the binds is written here, and a new core releases what
/// the file names for its instance before it binds ([`BoundKeys::remembered`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// The file, [`KEYS_FILE`] in the state directory; written 0600 like `startup.json`.
    pub file: std::path::PathBuf,
    /// `HYPRLAND_INSTANCE_SIGNATURE`: new with every start of Hyprland, whose binds go with it.
    pub instance: String,
}

/// What [`KEYS_FILE`] holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Written {
    instance: String,
    /// The keys in Hyprland's spelling ([`hyprland_keys`]), as they have to be released.
    keys: Vec<String>,
}

impl Ledger {
    /// The ledger of this process: [`KEYS_FILE`] in the core's state directory, for the instance
    /// `HYPRLAND_INSTANCE_SIGNATURE` names. `None` without a home directory or outside Hyprland,
    /// where nothing is bound at run time.
    pub fn system() -> Option<Ledger> {
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().filter(|instance| !instance.is_empty())?;
        let state = torro_core::paths::AppPaths::for_app(torro_core::AppId::WORKPLACE)?.state;
        Some(Ledger { file: state.join(KEYS_FILE), instance })
    }

    /// The keys written down for this ledger's instance. None for another instance (Hyprland started
    /// anew, the binds went with the old one) and for a missing or unreadable file.
    fn left_over(&self) -> Vec<String> {
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Written>(&bytes).ok())
            .filter(|written| written.instance == self.instance)
            .map(|written| written.keys)
            .unwrap_or_default()
    }

    /// Writes `keys` down for this instance, creating the folder. A failure is only logged: the keys
    /// work without the file, only a core started after a crash may then bind a key twice.
    fn write(&self, keys: Vec<String>) {
        let written = Written { instance: self.instance.clone(), keys };
        let result = (|| {
            if let Some(folder) = self.file.parent() {
                std::fs::create_dir_all(folder).map_err(|err| format!("{}: {err}", folder.display()))?;
            }
            let bytes = serde_json::to_vec_pretty(&written).map_err(|err| err.to_string())?;
            crate::clients::write_private(&self.file, &bytes).map_err(|err| err.to_string())
        })();
        if let Err(err) = result {
            eprintln!("torroworkplace: cannot write down the bound template keys: {err}");
        }
    }
}

/// The binds made on the window manager, so a change binds and unbinds only the difference.
///
/// Hyprland runs every bind of a key, so binding a key twice would load two templates; that is why
/// this remembers exactly what went through, and why a key whose old bind could not be removed is
/// not bound again. With a [`Ledger`] it remembers across restarts of the core too.
#[derive(Debug, Default)]
pub struct BoundKeys {
    /// What is bound, held across the calls to the window manager, so two changes at once cannot
    /// both bind the same key.
    bound: tokio::sync::Mutex<Bound>,
    /// Where every change is written down; `None` in tests and outside Hyprland.
    ledger: Option<Ledger>,
}

/// The state behind [`BoundKeys`]' lock.
#[derive(Debug, Default)]
struct Bound {
    /// The binds the window manager took, in the order they were made.
    binds: Vec<KeyBind>,
    /// Keys that may be bound without being one of `binds`: what an earlier core of the same Hyprland
    /// instance left ([`Ledger`]), and after a reload every key remembered before it
    /// ([`BoundKeys::rebind_after_reload`]). Released before the next bind.
    left_over: Vec<String>,
}

impl BoundKeys {
    /// Binds that are written down in `ledger`. What an earlier core left there for the same Hyprland
    /// instance counts as still bound: the first [`BoundKeys::sync`] releases it before it binds.
    pub fn remembered(ledger: Ledger) -> BoundKeys {
        let left_over = ledger.left_over();
        BoundKeys { bound: tokio::sync::Mutex::new(Bound { binds: Vec::new(), left_over }), ledger: Some(ledger) }
    }

    /// The binds of this process: [`BoundKeys::remembered`] with [`Ledger::system`], or none written
    /// down where there is no ledger.
    pub fn system() -> BoundKeys {
        Ledger::system().map_or_else(BoundKeys::default, BoundKeys::remembered)
    }

    /// Makes the window manager's binds what `wanted` says, `wanted` being asked only once this holds
    /// the lock, so the last change to run binds what the templates say then and not what an earlier
    /// one read.
    ///
    /// First the keys an earlier core left ([`BoundKeys::remembered`]) are released, once; a release
    /// that fails is only logged, since Hyprland may refuse to release a key it no longer holds, and a
    /// wanted key must be bound anyway. Then every remembered bind that is not in `wanted` exactly
    /// (another key, or the same key for another template) is unbound, one call per key; then every
    /// bind of `wanted` not remembered yet is bound, one call per bind. A refused call leaves the
    /// others alone: a bind that could not be made is tried again with the next change, and the key
    /// of a bind that could not be removed is not bound anew, so it never runs two commands. The
    /// report names those keys ("Super + Alt + 5 could not be registered"). Afterwards the ledger
    /// holds what is bound.
    ///
    /// `wanted` gives `Err` with the reason when the templates could not be read: the binds stay as
    /// they are (left-overs too) and the report says why.
    pub async fn sync(&self, wm: &impl WindowManager, wanted: impl FnOnce() -> Result<Vec<KeyBind>, String>) -> Report {
        let mut bound = self.bound.lock().await;
        self.sync_held(&mut bound, wm, wanted()).await
    }

    /// After Hyprland reloaded its configuration ([`WmEvent::ConfigReloaded`]), which dropped every
    /// bind made at run time: binds `wanted` anew as [`BoundKeys::sync`] does, but first releases
    /// every key remembered, ignoring refusals, all under one hold of the lock.
    ///
    /// The event comes after the reload, so a change that ran in between bound its key on the new
    /// configuration already; forgetting that bind and binding the key again would make it run twice.
    /// Releasing each remembered key first leaves exactly one bind per key, whichever came first.
    pub async fn rebind_after_reload(
        &self,
        wm: &impl WindowManager,
        wanted: impl FnOnce() -> Result<Vec<KeyBind>, String>,
    ) -> Report {
        let mut bound = self.bound.lock().await;
        let remembered: Vec<String> = std::mem::take(&mut bound.binds).into_iter().map(|bind| bind.keys).collect();
        for keys in remembered {
            if !bound.left_over.contains(&keys) {
                bound.left_over.push(keys);
            }
        }
        self.sync_held(&mut bound, wm, wanted()).await
    }

    /// Releases every bind, the left-overs too, when the core ends, so the keys do not outlive it. A
    /// key that cannot be released stays written down, and the next core of this instance releases it.
    pub async fn release(&self, wm: &impl WindowManager) {
        let mut bound = self.bound.lock().await;
        let binds = std::mem::take(&mut bound.binds).into_iter().map(|bind| bind.keys);
        let keys: Vec<String> = std::mem::take(&mut bound.left_over).into_iter().chain(binds).collect();
        for keys in keys {
            if let Err(err) = wm.unbind_keys(slice::from_ref(&keys)).await {
                eprintln!("torroworkplace: cannot release the key {keys} at the end: {err}");
                // Still bound, but no longer this core's: left over for the next one.
                bound.left_over.push(keys);
            }
        }
        self.write_down(&bound);
    }

    /// [`BoundKeys::sync`] with the lock held.
    async fn sync_held(
        &self,
        bound: &mut Bound,
        wm: &impl WindowManager,
        wanted: Result<Vec<KeyBind>, String>,
    ) -> Report {
        let wanted = match wanted {
            Ok(wanted) => wanted,
            Err(error) => {
                let problem = trf("The templates could not be read: {error}", &[("error", &error)]);
                return Report { active: false, problem: Some(problem) };
            }
        };
        for keys in std::mem::take(&mut bound.left_over) {
            if let Err(err) = wm.unbind_keys(slice::from_ref(&keys)).await {
                eprintln!("torroworkplace: cannot release the key {keys} (it may be gone already): {err}");
            }
        }
        let mut stuck = Vec::new();
        let gone: Vec<KeyBind> = bound.binds.iter().filter(|old| !wanted.contains(old)).cloned().collect();
        for old in gone {
            match wm.unbind_keys(slice::from_ref(&old.keys)).await {
                Ok(()) => bound.binds.retain(|kept| *kept != old),
                Err(err) => {
                    eprintln!("torroworkplace: cannot release the key {}: {err}", old.keys);
                    stuck.push(old.keys);
                }
            }
        }
        let mut failed = Vec::new();
        let missing: Vec<&KeyBind> = wanted.iter().filter(|new| !bound.binds.contains(new)).collect();
        for new in missing {
            if stuck.contains(&new.keys) {
                failed.push(new.keys.clone());
                continue;
            }
            match wm.bind_keys(slice::from_ref(new)).await {
                Ok(()) => bound.binds.push(new.clone()),
                Err(err) => {
                    eprintln!("torroworkplace: cannot bind the key {}: {err}", new.keys);
                    failed.push(new.keys.clone());
                }
            }
        }
        self.write_down(bound);
        stuck.retain(|keys| !failed.contains(keys));
        let readable = |keys: &[String]| keys.iter().map(|keys| for_people(keys)).collect::<Vec<_>>();
        report_of(&readable(&failed), &readable(&stuck))
    }

    /// Writes what is bound (and what is left over) to the ledger, if there is one.
    fn write_down(&self, bound: &Bound) {
        if let Some(ledger) = &self.ledger {
            let binds = bound.binds.iter().map(|bind| bind.keys.clone());
            ledger.write(bound.left_over.iter().cloned().chain(binds).collect());
        }
    }
}

/// The report after binding: `failed` are the keys (as people read them) that could not be
/// registered, `stuck` those whose old binding could not be released.
fn report_of(failed: &[String], stuck: &[String]) -> Report {
    let mut problems = Vec::new();
    if !failed.is_empty() {
        problems.push(trf("{keys} could not be registered", &[("keys", &failed.join(", "))]));
    }
    if !stuck.is_empty() {
        problems.push(trf("{keys} could not be released", &[("keys", &stuck.join(", "))]));
    }
    Report { active: problems.is_empty(), problem: (!problems.is_empty()).then(|| problems.join("; ")) }
}

/// A Hyprland key text as people read it: `SUPER + ALT + 5` → `Super + Alt + 5`.
fn for_people(keys: &str) -> String {
    keys.split(" + ")
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map(|first| first.to_string() + &chars.as_str().to_lowercase()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Keeps the keys bound for as long as `events` runs: binds them now, and anew after every
/// [`WmEvent::ConfigReloaded`] (Hyprland dropped them all; [`BoundKeys::rebind_after_reload`]).
/// `wanted` says what the binds should be at that moment (as for [`BoundKeys::sync`]), `report` gets
/// every result.
///
/// The caller subscribes to `events` before it calls this, so a reload right at the start is not
/// missed. Returns when the events end.
pub async fn follow(
    wm: &impl WindowManager,
    keys: &BoundKeys,
    mut events: Events,
    wanted: impl Fn() -> Result<Vec<KeyBind>, String>,
    report: impl Fn(Report),
) {
    report(keys.sync(wm, &wanted).await);
    while let Some(event) = events.recv().await {
        if event == WmEvent::ConfigReloaded {
            report(keys.rebind_after_reload(wm, &wanted).await);
        }
    }
}

/// The template keys as a whole, as the tray on macOS and Windows registers them (task WS.4): which
/// digit loads which template ([`slots`]), all held with `modifiers` (`Settings.workspaces`).
#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateKeys {
    pub modifiers: ShortcutModifiers,
    /// In keyboard order, a key at most once ([`summary_slots`]).
    pub slots: Vec<Slot>,
}

/// The system's key for the digit `key` held with `modifiers`; `None` for a `key` above
/// [`MAX_SHORTCUT`]. `global-hotkey` reads Super as Cmd on macOS and as the Windows key on Windows,
/// and Alt as Option on macOS, so the modifiers map one to one.
#[cfg(any(target_os = "macos", windows))]
pub fn hotkey(modifiers: ShortcutModifiers, key: u8) -> Option<global_hotkey::hotkey::HotKey> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};

    let held = match modifiers {
        ShortcutModifiers::SuperAlt => Modifiers::SUPER | Modifiers::ALT,
        ShortcutModifiers::SuperCtrl => Modifiers::SUPER | Modifiers::CONTROL,
        ShortcutModifiers::CtrlAlt => Modifiers::CONTROL | Modifiers::ALT,
        ShortcutModifiers::CtrlShift => Modifiers::CONTROL | Modifiers::SHIFT,
    };
    let digit = match key {
        0 => Code::Digit0,
        1 => Code::Digit1,
        2 => Code::Digit2,
        3 => Code::Digit3,
        4 => Code::Digit4,
        5 => Code::Digit5,
        6 => Code::Digit6,
        7 => Code::Digit7,
        8 => Code::Digit8,
        9 => Code::Digit9,
        _ => return None,
    };
    Some(HotKey::new(Some(held), digit))
}

/// Where the keys are registered: the system's `GlobalHotKeyManager`, or a fake in the tests. A call
/// fails with the reason as text.
#[cfg(any(target_os = "macos", windows))]
pub trait Registrar {
    fn register(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String>;
    fn unregister(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String>;
}

#[cfg(any(target_os = "macos", windows))]
impl Registrar for global_hotkey::GlobalHotKeyManager {
    fn register(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String> {
        global_hotkey::GlobalHotKeyManager::register(self, key).map_err(|err| err.to_string())
    }

    fn unregister(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String> {
        global_hotkey::GlobalHotKeyManager::unregister(self, key).map_err(|err| err.to_string())
    }
}

/// The template keys registered with the system on macOS and Windows (task WS.4), kept by the tray
/// on its main thread. Like [`BoundKeys`], it changes only the difference: a key that stays but now
/// loads another template needs no call to the system at all.
#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Default)]
pub struct DesktopKeys {
    /// What [`DesktopKeys::apply`] was last asked for; the same again changes nothing.
    wanted: Option<TemplateKeys>,
    /// The keys the system holds for us, each with its label for messages.
    registered: Vec<(global_hotkey::hotkey::HotKey, String)>,
    /// The template a pressed key loads, by the key's id (`GlobalHotKeyEvent::id`). Only registered
    /// keys of the current [`TemplateKeys`] are here, so a key that could not be released loads
    /// nothing.
    targets: Vec<(u32, String)>,
}

#[cfg(any(target_os = "macos", windows))]
impl DesktopKeys {
    /// Registers `wanted` with `system`, only the difference to what it holds, and says how the keys
    /// stand for `Core::report_shortcuts`; `None` when `wanted` is what the last call got, so a new
    /// menu without a change of the keys costs nothing.
    ///
    /// First every held key that `wanted` does not name is released, then every key of `wanted` not
    /// held yet is registered, one call each. A refused call leaves the others alone and is tried
    /// again with the next change of the keys; the report names those keys
    /// ("Ctrl + Option + 5 could not be registered", for example when another app holds it).
    pub fn apply(&mut self, system: &impl Registrar, wanted: &TemplateKeys) -> Option<Report> {
        if self.wanted.as_ref() == Some(wanted) {
            return None;
        }
        self.wanted = Some(wanted.clone());
        let keys: Vec<(global_hotkey::hotkey::HotKey, &Slot)> =
            wanted.slots.iter().filter_map(|slot| hotkey(wanted.modifiers, slot.key).map(|key| (key, slot))).collect();
        let mut stuck = Vec::new();
        let gone: Vec<(global_hotkey::hotkey::HotKey, String)> =
            self.registered.iter().filter(|(held, _)| keys.iter().all(|(key, _)| key != held)).cloned().collect();
        for (held, name) in gone {
            match system.unregister(held) {
                Ok(()) => self.registered.retain(|(kept, _)| *kept != held),
                Err(err) => {
                    eprintln!("torroworkplace: cannot release the key {name}: {err}");
                    stuck.push(name);
                }
            }
        }
        let mut failed = Vec::new();
        for (key, slot) in &keys {
            if self.registered.iter().any(|(held, _)| held == key) {
                continue;
            }
            let name = label(wanted.modifiers, slot.key);
            match system.register(*key) {
                Ok(()) => self.registered.push((*key, name)),
                Err(err) => {
                    eprintln!("torroworkplace: cannot register the key {name}: {err}");
                    failed.push(name);
                }
            }
        }
        self.targets = keys
            .iter()
            .filter(|(key, _)| self.registered.iter().any(|(held, _)| held == key))
            .map(|(key, slot)| (key.id(), slot.template.clone()))
            .collect();
        Some(report_of(&failed, &stuck))
    }

    /// The id of the template the key with the id `pressed` loads; `None` for a key not ours (any
    /// more).
    pub fn template(&self, pressed: u32) -> Option<&str> {
        self.targets.iter().find(|(id, _)| *id == pressed).map(|(_, template)| template.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use torro_wm::fake::{Call, FakeWm};
    use torro_wm::{Split, SplitDirection, WindowId, WmError, WorkspaceId};

    use super::*;

    const PROGRAM: &str = "/opt/torro/torroworkplace";

    /// A template `id` with key `key`, nothing else of interest.
    fn template(id: &str, key: Option<u8>) -> Template {
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
            startup: None,
        }
    }

    fn binds(templates: &[Template], modifiers: ShortcutModifiers) -> Vec<KeyBind> {
        hyprland_binds(&slots(templates), modifiers, PROGRAM)
    }

    fn bind(keys: &str, id: &str) -> KeyBind {
        KeyBind { keys: keys.into(), command: load_command(PROGRAM, id) }
    }

    /// The page and the tray name the window manager alike; an unknown one stays as reported.
    #[test]
    fn the_window_manager_is_named_as_people_write_it() {
        assert_eq!(manager_name("hyprland"), "Hyprland");
        assert_eq!(manager_name("aerospace"), "AeroSpace");
        assert_eq!(manager_name("glazewm"), "GlazeWM");
        assert_eq!(manager_name("sway"), "sway");
    }

    #[test]
    fn the_keys_go_in_keyboard_order_and_a_key_has_one_template() {
        let templates = [
            template("zero", Some(0)),
            template("five", Some(5)),
            template("none", None),
            template("one", Some(1)),
            template("also-five", Some(5)),
            template("eleven", Some(11)),
        ];
        let keys: Vec<(u8, String)> = slots(&templates).into_iter().map(|s| (s.key, s.template)).collect();
        assert_eq!(keys, [(1, "one"), (5, "five"), (0, "zero")].map(|(key, id)| (key, id.to_string())));
    }

    #[test]
    fn the_keys_are_spelled_for_hyprland_and_for_people() {
        let spelled: Vec<(String, String)> =
            ShortcutModifiers::ALL.iter().map(|m| (hyprland_keys(*m, 5), label_on(System::Linux, *m, 5))).collect();
        let expected = [
            ("SUPER + ALT + 5", "Super + Alt + 5"),
            ("SUPER + CTRL + 5", "Super + Ctrl + 5"),
            ("CTRL + ALT + 5", "Ctrl + Alt + 5"),
            ("CTRL + SHIFT + 5", "Ctrl + Shift + 5"),
        ];
        assert_eq!(spelled, expected.map(|(h, p)| (h.to_string(), p.to_string())));
        assert_eq!(for_people("CTRL + SHIFT + 0"), "Ctrl + Shift + 0");
        assert_eq!(label(ShortcutModifiers::CtrlAlt, 5), label_on(System::CURRENT, ShortcutModifiers::CtrlAlt, 5));
        // The modifiers alone are the key's label without its digit (task WS.6).
        for modifiers in ShortcutModifiers::ALL {
            assert_eq!(format!("{} + 7", modifiers_label(modifiers)), label(modifiers, 7));
        }
    }

    /// Task WS.4: macOS and Windows name the modifiers as their keyboards do; Hyprland keeps Linux's.
    #[test]
    fn macos_and_windows_name_the_modifiers_as_their_keyboards_do() {
        let on = |system| ShortcutModifiers::ALL.map(|m| label_on(system, m, 5));
        assert_eq!(on(System::MacOs), ["Cmd + Option + 5", "Cmd + Ctrl + 5", "Ctrl + Option + 5", "Ctrl + Shift + 5"]);
        assert_eq!(on(System::Windows), ["Win + Alt + 5", "Win + Ctrl + 5", "Ctrl + Alt + 5", "Ctrl + Shift + 5"]);
        assert_eq!(hyprland_keys(ShortcutModifiers::SuperAlt, 5), "SUPER + ALT + 5");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn the_snapshot_s_summaries_give_the_same_slots() {
        let templates = [template("zero", Some(0)), template("one", Some(1)), template("also-one", Some(1))];
        let summaries: Vec<TemplateSummary> = templates.iter().map(TemplateSummary::from).collect();
        assert_eq!(summary_slots(&summaries), slots(&templates));
    }

    /// Task WS.7: the tray's order is the page's (task WS.6): by key, 0 after 9, then by name.
    #[test]
    fn templates_go_by_key_then_by_name() {
        let named = |id: &str, name: &str, key| TemplateSummary {
            name: name.into(),
            ..TemplateSummary::from(&template(id, key))
        };
        let templates = [
            named("z", "zebra", None),
            named("zero", "Zero", Some(0)),
            named("a", "Alpha", None),
            named("nine", "Nine", Some(9)),
            named("one", "One", Some(1)),
            named("odd", "Beta", Some(MAX_SHORTCUT + 1)),
        ];
        let ids: Vec<&str> = in_key_order(&templates).iter().map(|template| template.id.as_str()).collect();
        assert_eq!(ids, ["one", "nine", "zero", "a", "odd", "z"], "no key above 9");
    }

    /// The bind runs the command line `ctl` understands: load in the mode `add`.
    #[test]
    fn a_key_loads_its_template_in_the_mode_add() {
        let [bind] = binds(&[template("dev", Some(3))], ShortcutModifiers::SuperAlt).try_into().expect("one bind");
        assert_eq!(bind.keys, "SUPER + ALT + 3");
        let parsed = crate::cli::parse(&bind.command[1..]).expect("ctl understands it");
        assert_eq!(parsed.request, Request::LoadTemplate { id: "dev".into(), mode: torro_setups::LoadMode::Add });
        assert_eq!(bind.command[0], PROGRAM);
    }

    #[test]
    fn the_keys_follow_saves_deletes_keys_and_the_modifiers_only() {
        let setting = |key| Request::SetSetting { key, value: serde_json::json!("ctrl_alt") };
        assert!(rebinds_after(&Request::SaveTemplate { name: None }));
        assert!(rebinds_after(&Request::UpdateTemplate { id: "dev".into() }));
        assert!(rebinds_after(&Request::DeleteTemplate { id: "dev".into() }));
        assert!(rebinds_after(&Request::SetTemplateShortcut { id: "dev".into(), key: None }));
        assert!(rebinds_after(&setting(SettingKey::WorkspaceShortcuts)));
        assert!(!rebinds_after(&setting(SettingKey::Design)));
        assert!(!rebinds_after(&Request::RenameTemplate { id: "dev".into(), name: "Dev".into() }));
        assert!(!rebinds_after(&Request::ListTemplates));
    }

    /// Moving key 1 from `mail` to `dev` (which held 2) releases both old keys and binds 1 for `dev`;
    /// the untouched key 3 is neither released nor bound again.
    #[tokio::test]
    async fn moving_a_key_binds_only_the_difference() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        let before = [template("mail", Some(1)), template("dev", Some(2)), template("chat", Some(3))];
        let report = keys.sync(&wm, || Ok(binds(&before, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(report, Report { active: true, problem: None });

        let after = [template("mail", None), template("dev", Some(1)), template("chat", Some(3))];
        keys.sync(&wm, || Ok(binds(&after, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(
            wm.calls()[3..],
            [
                Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]),
                Call::UnbindKeys(vec!["SUPER + ALT + 2".into()]),
                Call::BindKeys(vec![bind("SUPER + ALT + 1", "dev")]),
            ]
        );
        assert_eq!(keys.bound.lock().await.binds, [bind("SUPER + ALT + 3", "chat"), bind("SUPER + ALT + 1", "dev")]);

        // Nothing changed: nothing is called.
        keys.sync(&wm, || Ok(binds(&after, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(wm.calls().len(), 6);
    }

    /// A new modifier releases every old key before it binds the new ones.
    #[tokio::test]
    async fn new_modifiers_release_every_old_key() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        let templates = [template("dev", Some(1)), template("mail", Some(0))];
        keys.sync(&wm, || Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        keys.sync(&wm, || Ok(binds(&templates, ShortcutModifiers::CtrlShift))).await;
        assert_eq!(
            wm.calls()[2..],
            [
                Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]),
                Call::UnbindKeys(vec!["SUPER + ALT + 0".into()]),
                Call::BindKeys(vec![bind("CTRL + SHIFT + 1", "dev")]),
                Call::BindKeys(vec![bind("CTRL + SHIFT + 0", "mail")]),
            ]
        );
    }

    /// After Hyprland reloaded its configuration every key is released (in case a change bound it on
    /// the new configuration already) and bound anew; each result is reported.
    #[tokio::test]
    async fn a_reload_binds_every_key_anew() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        let mut from_fake = wm.events().await.expect("events");
        let wanted = || Ok(binds(&[template("dev", Some(1)), template("mail", Some(2))], ShortcutModifiers::SuperAlt));
        let reports = Mutex::new(Vec::new());
        let report = |report: Report| reports.lock().expect("reports").push(report);
        let all = vec![
            Call::BindKeys(vec![bind("SUPER + ALT + 1", "dev")]),
            Call::BindKeys(vec![bind("SUPER + ALT + 2", "mail")]),
        ];
        let release =
            vec![Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]), Call::UnbindKeys(vec!["SUPER + ALT + 2".into()])];

        wm.reload_config();
        wm.focus_workspace("2");
        wm.reload_config();
        // The fake's events never end: what it sent goes into a stream that does, so `follow` returns.
        let (sender, events) = tokio::sync::mpsc::channel(8);
        while let Ok(event) = from_fake.try_recv() {
            sender.send(event).await.expect("queued");
        }
        drop(sender);
        follow(&wm, &keys, events, wanted, report).await;
        let reload = [release, all.clone()].concat();
        assert_eq!(wm.calls(), [all, reload.clone(), reload].concat(), "start, then two reloads");
        assert_eq!(reports.lock().expect("reports").len(), 3);
    }

    fn ledger(folder: &std::path::Path, instance: &str) -> Ledger {
        Ledger { file: folder.join("state").join(KEYS_FILE), instance: instance.into() }
    }

    /// Review fix: a core started anew on the same Hyprland releases what the one before left bound,
    /// before it binds, so no key runs twice and an old key or modifier does not stay alive. The
    /// ledger holds what is bound after every change, and nothing once the core released its keys.
    #[tokio::test]
    async fn a_new_core_releases_what_the_last_one_left_bound() {
        let folder = tempfile::tempdir().expect("temp dir");
        let wm = FakeWm::new("1");
        let first = BoundKeys::remembered(ledger(folder.path(), "hypr-a"));
        let old = [template("dev", Some(1)), template("mail", Some(2))];
        first.sync(&wm, || Ok(binds(&old, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(ledger(folder.path(), "hypr-a").left_over(), ["SUPER + ALT + 1", "SUPER + ALT + 2"]);
        // The core ends without releasing (killed): the next one finds the keys and releases them first.
        drop(first);
        let second = BoundKeys::remembered(ledger(folder.path(), "hypr-a"));
        let new = [template("dev", Some(1))];
        let before = wm.calls().len();
        second.sync(&wm, || Ok(binds(&new, ShortcutModifiers::CtrlAlt))).await;
        assert_eq!(
            wm.calls()[before..],
            [
                Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]),
                Call::UnbindKeys(vec!["SUPER + ALT + 2".into()]),
                Call::BindKeys(vec![bind("CTRL + ALT + 1", "dev")]),
            ]
        );
        assert_eq!(ledger(folder.path(), "hypr-a").left_over(), ["CTRL + ALT + 1"]);
        // Released once: the next change does not release them again.
        let before = wm.calls().len();
        second.sync(&wm, || Ok(binds(&new, ShortcutModifiers::CtrlAlt))).await;
        assert_eq!(wm.calls().len(), before);

        // At the end the core releases its keys, and the ledger names none.
        second.release(&wm).await;
        assert_eq!(wm.calls().last(), Some(&Call::UnbindKeys(vec!["CTRL + ALT + 1".into()])));
        assert!(ledger(folder.path(), "hypr-a").left_over().is_empty());
    }

    /// A new Hyprland dropped the old instance's binds: what its ledger names is not released.
    #[tokio::test]
    async fn another_instance_leaves_nothing_to_release() {
        let folder = tempfile::tempdir().expect("temp dir");
        let wm = FakeWm::new("1");
        let wanted = || Ok(binds(&[template("dev", Some(1))], ShortcutModifiers::SuperAlt));
        BoundKeys::remembered(ledger(folder.path(), "hypr-a")).sync(&wm, wanted).await;
        let calls = wm.calls().len();

        let next = BoundKeys::remembered(ledger(folder.path(), "hypr-b"));
        next.sync(&wm, wanted).await;
        assert_eq!(wm.calls()[calls..], [Call::BindKeys(vec![bind("SUPER + ALT + 1", "dev")])], "nothing released");
        // A missing file names nothing either.
        assert!(ledger(&folder.path().join("nowhere"), "hypr-b").left_over().is_empty());
    }

    /// Review fix: a change that binds a key after Hyprland reloaded but before the core handles the
    /// event leaves that key bound once, not twice: the rebind releases every remembered key first.
    #[tokio::test]
    async fn a_key_bound_between_a_reload_and_its_event_is_not_bound_twice() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        keys.sync(&wm, || Ok(binds(&[template("dev", Some(1))], ShortcutModifiers::SuperAlt))).await;
        // Hyprland reloads (key 1 is gone); before the event is handled, key 2 is given to `mail`.
        let now = [template("dev", Some(1)), template("mail", Some(2))];
        keys.sync(&wm, || Ok(binds(&now, ShortcutModifiers::SuperAlt))).await;
        let calls = wm.calls().len();
        keys.rebind_after_reload(&wm, || Ok(binds(&now, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(
            wm.calls()[calls..],
            [
                Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]),
                Call::UnbindKeys(vec!["SUPER + ALT + 2".into()]),
                Call::BindKeys(vec![bind("SUPER + ALT + 1", "dev")]),
                Call::BindKeys(vec![bind("SUPER + ALT + 2", "mail")]),
            ]
        );
        assert_eq!(keys.bound.lock().await.binds, binds(&now, ShortcutModifiers::SuperAlt));
    }

    /// A left-over key that cannot be released is bound anew all the same: Hyprland may refuse to
    /// release a key it no longer holds, and the key must work.
    #[tokio::test]
    async fn a_left_over_that_cannot_be_released_is_bound_anyway() {
        let folder = tempfile::tempdir().expect("temp dir");
        let wanted = || Ok(binds(&[template("dev", Some(1))], ShortcutModifiers::SuperAlt));
        BoundKeys::remembered(ledger(folder.path(), "hypr-a")).sync(&FakeWm::new("1"), wanted).await;
        let wm = Refusing { stuck: vec!["SUPER + ALT + 1".into()], ..Refusing::default() };
        let keys = BoundKeys::remembered(ledger(folder.path(), "hypr-a"));
        let report = keys.sync(&wm, wanted).await;
        assert_eq!(report, Report { active: true, problem: None });
        assert_eq!(keys.bound.lock().await.binds, [bind("SUPER + ALT + 1", "dev")]);
        // One it cannot release at the end stays in the ledger for the next core.
        keys.release(&wm).await;
        assert_eq!(ledger(folder.path(), "hypr-a").left_over(), ["SUPER + ALT + 1"]);
    }

    fn absent() -> WmError {
        WmError::Unsupported("not in this test".into())
    }

    /// A window manager that refuses the keys `refused` and every unbind of `stuck`, and records the
    /// rest in its fake.
    #[derive(Default)]
    struct Refusing {
        fake: FakeWm,
        refused: Vec<String>,
        stuck: Vec<String>,
    }

    /// Only the key binds matter here; everything else is not there.
    impl WindowManager for Refusing {
        fn name(&self) -> &'static str {
            "refusing"
        }
        async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
            Err(absent())
        }
        async fn windows(&self, _: Option<&WorkspaceId>) -> Result<Vec<torro_wm::Window>, WmError> {
            Err(absent())
        }
        async fn monitors(&self) -> Result<Vec<torro_wm::Monitor>, WmError> {
            Err(absent())
        }
        async fn focus_window(&self, _: &WindowId) -> Result<(), WmError> {
            Err(absent())
        }
        async fn move_to_workspace(&self, _: &WindowId, _: &WorkspaceId) -> Result<(), WmError> {
            Err(absent())
        }
        async fn close_window(&self, _: &WindowId) -> Result<(), WmError> {
            Err(absent())
        }
        async fn prepare_insert(&self, _: &WindowId, _: SplitDirection) -> Result<(), WmError> {
            Err(absent())
        }
        async fn place(&self, _: &WindowId, _: &WindowId, _: Split) -> Result<(), WmError> {
            Err(absent())
        }
        async fn events(&self) -> Result<Events, WmError> {
            Err(absent())
        }
        async fn focus_monitor(&self, _: &str) -> Result<(), WmError> {
            Err(absent())
        }
        async fn bind_keys(&self, binds: &[KeyBind]) -> Result<(), WmError> {
            match binds.iter().any(|bind| self.refused.contains(&bind.keys)) {
                true => Err(WmError::Protocol("taken".into())),
                false => self.fake.bind_keys(binds).await,
            }
        }
        async fn unbind_keys(&self, keys: &[String]) -> Result<(), WmError> {
            match keys.iter().any(|keys| self.stuck.contains(keys)) {
                true => Err(WmError::Protocol("no".into())),
                false => self.fake.unbind_keys(keys).await,
            }
        }
    }

    /// A refused key is named for people and tried again with the next change; the others are bound.
    #[tokio::test]
    async fn a_refused_key_is_reported_and_tried_again() {
        let wm = Refusing { refused: vec!["SUPER + ALT + 5".into()], ..Refusing::default() };
        let keys = BoundKeys::default();
        let templates = [template("dev", Some(5)), template("mail", Some(6))];
        let report = keys.sync(&wm, || Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        let expected = trf("{keys} could not be registered", &[("keys", &"Super + Alt + 5")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(keys.bound.lock().await.binds, [bind("SUPER + ALT + 6", "mail")]);

        let wm = Refusing { fake: wm.fake, ..Refusing::default() };
        let report = keys.sync(&wm, || Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(report, Report { active: true, problem: None });
        assert_eq!(wm.fake.calls().last(), Some(&Call::BindKeys(vec![bind("SUPER + ALT + 5", "dev")])));
    }

    /// A key whose old bind stays is not bound for its new template, so it never loads two.
    #[tokio::test]
    async fn a_key_that_cannot_be_released_is_not_bound_twice() {
        let wm = Refusing { stuck: vec!["SUPER + ALT + 1".into()], ..Refusing::default() };
        let keys = BoundKeys::default();
        keys.sync(&wm, || Ok(binds(&[template("mail", Some(1))], ShortcutModifiers::SuperAlt))).await;
        let moved = [template("mail", None), template("dev", Some(1)), template("chat", Some(2))];
        let report = keys.sync(&wm, || Ok(binds(&moved, ShortcutModifiers::SuperAlt))).await;
        let expected = trf("{keys} could not be registered", &[("keys", &"Super + Alt + 1")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(keys.bound.lock().await.binds, [bind("SUPER + ALT + 1", "mail"), bind("SUPER + ALT + 2", "chat")]);

        // Without a new template for the key, the report says it could not be released.
        let report = keys.sync(&wm, || Ok(Vec::new())).await;
        let expected = trf("{keys} could not be released", &[("keys", &"Super + Alt + 1")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
    }

    #[tokio::test]
    async fn unreadable_templates_leave_the_keys_and_say_why() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        keys.sync(&wm, || Ok(binds(&[template("dev", Some(1))], ShortcutModifiers::SuperAlt))).await;
        let report = keys.sync(&wm, || Err("disk on fire".into())).await;
        let expected = trf("The templates could not be read: {error}", &[("error", &"disk on fire")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(wm.calls().len(), 1, "nothing released");
    }

    /// Task WS.4: a slot and the modifiers become the system's key.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn a_slot_and_the_modifiers_become_the_system_s_key() {
        use global_hotkey::hotkey::{Code, HotKey, Modifiers};

        let expected = [
            (ShortcutModifiers::SuperAlt, Modifiers::SUPER | Modifiers::ALT),
            (ShortcutModifiers::SuperCtrl, Modifiers::SUPER | Modifiers::CONTROL),
            (ShortcutModifiers::CtrlAlt, Modifiers::CONTROL | Modifiers::ALT),
            (ShortcutModifiers::CtrlShift, Modifiers::CONTROL | Modifiers::SHIFT),
        ];
        for (modifiers, held) in expected {
            assert_eq!(hotkey(modifiers, 5), Some(HotKey::new(Some(held), Code::Digit5)), "{modifiers:?}");
        }
        let digits = [
            Code::Digit0,
            Code::Digit1,
            Code::Digit2,
            Code::Digit3,
            Code::Digit4,
            Code::Digit5,
            Code::Digit6,
            Code::Digit7,
            Code::Digit8,
            Code::Digit9,
        ];
        for (key, digit) in (0..=MAX_SHORTCUT).zip(digits) {
            assert_eq!(hotkey(ShortcutModifiers::CtrlAlt, key).map(|hotkey| hotkey.key), Some(digit), "{key}");
        }
        assert_eq!(hotkey(ShortcutModifiers::CtrlAlt, MAX_SHORTCUT + 1), None);
    }

    /// The system's keys in a test: records every call, refuses the keys in `refused` and every
    /// release of `stuck`.
    #[cfg(any(target_os = "macos", windows))]
    #[derive(Default)]
    struct FakeSystem {
        calls: Mutex<Vec<(&'static str, global_hotkey::hotkey::HotKey)>>,
        refused: Vec<global_hotkey::hotkey::HotKey>,
        stuck: Vec<global_hotkey::hotkey::HotKey>,
    }

    #[cfg(any(target_os = "macos", windows))]
    impl FakeSystem {
        fn calls(&self) -> Vec<(&'static str, global_hotkey::hotkey::HotKey)> {
            self.calls.lock().expect("calls").clone()
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    impl Registrar for FakeSystem {
        fn register(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String> {
            self.calls.lock().expect("calls").push(("register", key));
            if self.refused.contains(&key) { Err("taken".into()) } else { Ok(()) }
        }

        fn unregister(&self, key: global_hotkey::hotkey::HotKey) -> Result<(), String> {
            self.calls.lock().expect("calls").push(("unregister", key));
            if self.stuck.contains(&key) { Err("no".into()) } else { Ok(()) }
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn desktop(modifiers: ShortcutModifiers, keys: &[(u8, &str)]) -> TemplateKeys {
        let slots = keys.iter().map(|(key, id)| Slot { key: *key, template: id.to_string() }).collect();
        TemplateKeys { modifiers, slots }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn system_key(modifiers: ShortcutModifiers, key: u8) -> global_hotkey::hotkey::HotKey {
        hotkey(modifiers, key).expect("a digit")
    }

    /// Moving key 1 from `mail` to `dev` and dropping key 2 releases 2 only; key 1 stays registered and
    /// now loads `dev`. The same keys again call nothing.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn the_desktop_registers_only_the_difference() {
        let system = FakeSystem::default();
        let mut keys = DesktopKeys::default();
        let ctrl_alt = ShortcutModifiers::CtrlAlt;
        let before = desktop(ctrl_alt, &[(1, "mail"), (2, "dev")]);
        assert_eq!(keys.apply(&system, &before), Some(Report { active: true, problem: None }));
        let one = system_key(ctrl_alt, 1);
        assert_eq!(keys.template(one.id()), Some("mail"));

        let after = desktop(ctrl_alt, &[(1, "dev")]);
        assert_eq!(keys.apply(&system, &after), Some(Report { active: true, problem: None }));
        let two = system_key(ctrl_alt, 2);
        assert_eq!(system.calls(), [("register", one), ("register", two), ("unregister", two)]);
        assert_eq!(keys.template(one.id()), Some("dev"));
        assert_eq!(keys.template(two.id()), None);

        assert_eq!(keys.apply(&system, &after), None);
        assert_eq!(system.calls().len(), 3);
    }

    /// New modifiers release every old key before they register the new ones.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn new_modifiers_release_every_old_desktop_key() {
        let system = FakeSystem::default();
        let mut keys = DesktopKeys::default();
        let (old, new) = (ShortcutModifiers::CtrlAlt, ShortcutModifiers::SuperAlt);
        keys.apply(&system, &desktop(old, &[(1, "dev"), (0, "mail")]));
        keys.apply(&system, &desktop(new, &[(1, "dev"), (0, "mail")]));
        assert_eq!(
            system.calls()[2..],
            [
                ("unregister", system_key(old, 1)),
                ("unregister", system_key(old, 0)),
                ("register", system_key(new, 1)),
                ("register", system_key(new, 0)),
            ]
        );
        assert_eq!(keys.template(system_key(old, 1).id()), None);
        assert_eq!(keys.template(system_key(new, 0).id()), Some("mail"));
    }

    /// A key another program holds is named for people, loads nothing and is tried again with the next
    /// change; a key that cannot be released loads nothing either.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn a_taken_desktop_key_is_reported_and_tried_again() {
        let ctrl_alt = ShortcutModifiers::CtrlAlt;
        let five = system_key(ctrl_alt, 5);
        let system = FakeSystem { refused: vec![five], ..FakeSystem::default() };
        let mut keys = DesktopKeys::default();
        let report = keys.apply(&system, &desktop(ctrl_alt, &[(5, "dev"), (6, "mail")]));
        let expected = trf("{keys} could not be registered", &[("keys", &label(ctrl_alt, 5))]);
        assert_eq!(report, Some(Report { active: false, problem: Some(expected) }));
        assert_eq!(keys.template(five.id()), None);
        assert_eq!(keys.template(system_key(ctrl_alt, 6).id()), Some("mail"));

        let system = FakeSystem { stuck: vec![system_key(ctrl_alt, 6)], ..FakeSystem::default() };
        let report = keys.apply(&system, &desktop(ctrl_alt, &[(5, "dev")]));
        let expected = trf("{keys} could not be released", &[("keys", &label(ctrl_alt, 6))]);
        assert_eq!(report, Some(Report { active: false, problem: Some(expected) }));
        assert_eq!(system.calls(), [("unregister", system_key(ctrl_alt, 6)), ("register", five)]);
        assert_eq!(keys.template(five.id()), Some("dev"));
        assert_eq!(keys.template(system_key(ctrl_alt, 6).id()), None);
    }
}
