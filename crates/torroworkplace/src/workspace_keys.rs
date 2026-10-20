//! The template keys (issue #243): the modifiers of the settings and a digit load the template that
//! holds the digit, like "Load" in the mode `add`.
//!
//! Two halves, kept apart so macOS and Windows (task WS.4, `global-hotkey` in the tray) reuse the
//! first one:
//!
//! - **What the keys are**, for every system: [`slots`] (which digit loads which template) and
//!   [`label`] (how people read a key, "Super + Alt + 5").
//! - **Hyprland** (task WS.3): [`hyprland_binds`] turns the slots into [`KeyBind`]s, [`BoundKeys`]
//!   remembers what it bound and applies only the difference, and [`follow`] binds everything anew
//!   after Hyprland reloaded its configuration, which drops every bind made at run time.
//!
//! The core decides when ([`crate::core::Core`]): at its start, after a template was saved, captured
//! again or deleted, a key was set and the modifiers changed ([`rebinds_after`]). Without Hyprland it
//! binds nothing here.

use std::slice;

use torro_core::i18n::trf;
use torro_core::settings::ShortcutModifiers;
use torro_ipc::{Request, SettingKey};
use torro_setups::{MAX_SHORTCUT, Template};
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
    let mut slots: Vec<Slot> = Vec::new();
    for template in templates {
        let Some(key) = template.shortcut.filter(|key| *key <= MAX_SHORTCUT) else { continue };
        if slots.iter().all(|slot| slot.key != key) {
            slots.push(Slot { key, template: template.id.clone() });
        }
    }
    slots.sort_by_key(|slot| keyboard_order(slot.key));
    slots
}

/// Where `key` stands on the keyboard's row of digits: 1 first, 0 last.
fn keyboard_order(key: u8) -> u8 {
    (key + MAX_SHORTCUT) % (MAX_SHORTCUT + 1)
}

/// The two modifiers by their Linux names, as people read them ("Super", "Alt"). Key names are not
/// translated anywhere in the suite ("Ctrl K"), so neither are these.
fn modifier_names(modifiers: ShortcutModifiers) -> [&'static str; 2] {
    match modifiers {
        ShortcutModifiers::SuperAlt => ["Super", "Alt"],
        ShortcutModifiers::SuperCtrl => ["Super", "Ctrl"],
        ShortcutModifiers::CtrlAlt => ["Ctrl", "Alt"],
        ShortcutModifiers::CtrlShift => ["Ctrl", "Shift"],
    }
}

/// `key` with `modifiers` as people read it: `Super + Alt + 5`. For messages and the control
/// centre; Hyprland gets [`hyprland_keys`].
pub fn label(modifiers: ShortcutModifiers, key: u8) -> String {
    let [first, second] = modifier_names(modifiers);
    format!("{first} + {second} + {key}")
}

/// `key` with `modifiers` in Hyprland's spelling: `SUPER + ALT + 5`. [`KeyBind::keys`] keeps it, and
/// unbinding needs exactly this text again, so it is made here only.
pub fn hyprland_keys(modifiers: ShortcutModifiers, key: u8) -> String {
    label(modifiers, key).to_uppercase()
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

/// The binds made on the window manager, so a change binds and unbinds only the difference.
///
/// Hyprland runs every bind of a key, so binding a key twice would load two templates; that is why
/// this remembers exactly what went through, and why a key whose old bind could not be removed is
/// not bound again.
#[derive(Debug, Default)]
pub struct BoundKeys {
    /// The binds the window manager took, in the order they were made. Held across the calls to the
    /// window manager, so two changes at once cannot both bind the same key.
    bound: tokio::sync::Mutex<Vec<KeyBind>>,
}

impl BoundKeys {
    /// Makes the window manager's binds what `wanted` says.
    ///
    /// First every remembered bind that is not in `wanted` exactly (another key, or the same key for
    /// another template) is unbound, one call per key; then every bind of `wanted` not remembered yet
    /// is bound, one call per bind. A refused call leaves the others alone: a bind that could not be
    /// made is tried again with the next change, and the key of a bind that could not be removed is
    /// not bound anew, so it never runs two commands. The report names those keys
    /// ("Super + Alt + 5 could not be registered").
    ///
    /// `wanted` is `Err` with the reason when the templates could not be read: the binds stay as they
    /// are and the report says why.
    pub async fn sync(&self, wm: &impl WindowManager, wanted: Result<Vec<KeyBind>, String>) -> Report {
        let wanted = match wanted {
            Ok(wanted) => wanted,
            Err(error) => {
                let problem = trf("The templates could not be read: {error}", &[("error", &error)]);
                return Report { active: false, problem: Some(problem) };
            }
        };
        let mut bound = self.bound.lock().await;
        let mut stuck = Vec::new();
        let gone: Vec<KeyBind> = bound.iter().filter(|old| !wanted.contains(old)).cloned().collect();
        for old in gone {
            match wm.unbind_keys(slice::from_ref(&old.keys)).await {
                Ok(()) => bound.retain(|kept| *kept != old),
                Err(err) => {
                    eprintln!("torroworkplace: cannot release the key {}: {err}", old.keys);
                    stuck.push(old.keys);
                }
            }
        }
        let mut failed = Vec::new();
        let missing: Vec<&KeyBind> = wanted.iter().filter(|new| !bound.contains(new)).collect();
        for new in missing {
            if stuck.contains(&new.keys) {
                failed.push(new.keys.clone());
                continue;
            }
            match wm.bind_keys(slice::from_ref(new)).await {
                Ok(()) => bound.push(new.clone()),
                Err(err) => {
                    eprintln!("torroworkplace: cannot bind the key {}: {err}", new.keys);
                    failed.push(new.keys.clone());
                }
            }
        }
        stuck.retain(|keys| !failed.contains(keys));
        let mut problems = Vec::new();
        if !failed.is_empty() {
            problems.push(trf("{keys} could not be registered", &[("keys", &for_people(&failed))]));
        }
        if !stuck.is_empty() {
            problems.push(trf("{keys} could not be released", &[("keys", &for_people(&stuck))]));
        }
        Report { active: problems.is_empty(), problem: (!problems.is_empty()).then(|| problems.join("; ")) }
    }

    /// Forgets every bind: the window manager dropped them itself ([`WmEvent::ConfigReloaded`]), so the
    /// next [`BoundKeys::sync`] binds everything anew and unbinds nothing.
    pub async fn forget(&self) {
        self.bound.lock().await.clear();
    }
}

/// Hyprland's key texts as people read them, joined: `SUPER + ALT + 5` → `Super + Alt + 5`.
fn for_people(keys: &[String]) -> String {
    let readable = |keys: &String| {
        keys.split(" + ")
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map(|first| first.to_string() + &chars.as_str().to_lowercase()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };
    keys.iter().map(readable).collect::<Vec<_>>().join(", ")
}

/// Keeps the keys bound for as long as `events` runs: binds them now, and anew after every
/// [`WmEvent::ConfigReloaded`] (Hyprland dropped them all; the remembered binds count as none).
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
    report(keys.sync(wm, wanted()).await);
    while let Some(event) = events.recv().await {
        if event == WmEvent::ConfigReloaded {
            keys.forget().await;
            report(keys.sync(wm, wanted()).await);
        }
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
            ShortcutModifiers::ALL.iter().map(|m| (hyprland_keys(*m, 5), label(*m, 5))).collect();
        let expected = [
            ("SUPER + ALT + 5", "Super + Alt + 5"),
            ("SUPER + CTRL + 5", "Super + Ctrl + 5"),
            ("CTRL + ALT + 5", "Ctrl + Alt + 5"),
            ("CTRL + SHIFT + 5", "Ctrl + Shift + 5"),
        ];
        assert_eq!(spelled, expected.map(|(h, p)| (h.to_string(), p.to_string())));
        assert_eq!(
            for_people(&["CTRL + SHIFT + 0".into(), "SUPER + ALT + 1".into()]),
            "Ctrl + Shift + 0, Super + Alt + 1"
        );
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
        let report = keys.sync(&wm, Ok(binds(&before, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(report, Report { active: true, problem: None });

        let after = [template("mail", None), template("dev", Some(1)), template("chat", Some(3))];
        keys.sync(&wm, Ok(binds(&after, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(
            wm.calls()[3..],
            [
                Call::UnbindKeys(vec!["SUPER + ALT + 1".into()]),
                Call::UnbindKeys(vec!["SUPER + ALT + 2".into()]),
                Call::BindKeys(vec![bind("SUPER + ALT + 1", "dev")]),
            ]
        );
        assert_eq!(*keys.bound.lock().await, [bind("SUPER + ALT + 3", "chat"), bind("SUPER + ALT + 1", "dev")]);

        // Nothing changed: nothing is called.
        keys.sync(&wm, Ok(binds(&after, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(wm.calls().len(), 6);
    }

    /// A new modifier releases every old key before it binds the new ones.
    #[tokio::test]
    async fn new_modifiers_release_every_old_key() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        let templates = [template("dev", Some(1)), template("mail", Some(0))];
        keys.sync(&wm, Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        keys.sync(&wm, Ok(binds(&templates, ShortcutModifiers::CtrlShift))).await;
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

    /// After Hyprland reloaded its configuration every key is bound anew and none released (they are
    /// gone already); each result is reported.
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
        assert_eq!(wm.calls(), [all.clone(), all.clone(), all].concat(), "start, then two reloads; no unbind");
        assert_eq!(reports.lock().expect("reports").len(), 3);
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
        let report = keys.sync(&wm, Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        let expected = trf("{keys} could not be registered", &[("keys", &"Super + Alt + 5")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(*keys.bound.lock().await, [bind("SUPER + ALT + 6", "mail")]);

        let wm = Refusing { fake: wm.fake, ..Refusing::default() };
        let report = keys.sync(&wm, Ok(binds(&templates, ShortcutModifiers::SuperAlt))).await;
        assert_eq!(report, Report { active: true, problem: None });
        assert_eq!(wm.fake.calls().last(), Some(&Call::BindKeys(vec![bind("SUPER + ALT + 5", "dev")])));
    }

    /// A key whose old bind stays is not bound for its new template, so it never loads two.
    #[tokio::test]
    async fn a_key_that_cannot_be_released_is_not_bound_twice() {
        let wm = Refusing { stuck: vec!["SUPER + ALT + 1".into()], ..Refusing::default() };
        let keys = BoundKeys::default();
        keys.sync(&wm, Ok(binds(&[template("mail", Some(1))], ShortcutModifiers::SuperAlt))).await;
        let moved = [template("mail", None), template("dev", Some(1)), template("chat", Some(2))];
        let report = keys.sync(&wm, Ok(binds(&moved, ShortcutModifiers::SuperAlt))).await;
        let expected = trf("{keys} could not be registered", &[("keys", &"Super + Alt + 1")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(*keys.bound.lock().await, [bind("SUPER + ALT + 1", "mail"), bind("SUPER + ALT + 2", "chat")]);

        // Without a new template for the key, the report says it could not be released.
        let report = keys.sync(&wm, Ok(Vec::new())).await;
        let expected = trf("{keys} could not be released", &[("keys", &"Super + Alt + 1")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
    }

    #[tokio::test]
    async fn unreadable_templates_leave_the_keys_and_say_why() {
        let wm = FakeWm::new("1");
        let keys = BoundKeys::default();
        keys.sync(&wm, Ok(binds(&[template("dev", Some(1))], ShortcutModifiers::SuperAlt))).await;
        let report = keys.sync(&wm, Err("disk on fire".into())).await;
        let expected = trf("The templates could not be read: {error}", &[("error", &"disk on fire")]);
        assert_eq!(report, Report { active: false, problem: Some(expected) });
        assert_eq!(wm.calls().len(), 1, "nothing released");
    }
}
