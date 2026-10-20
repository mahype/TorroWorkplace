//! Workspaces (tasks S.7 and WS.6, draft `docs/prototypes/workspaces/entwurf.html`): the group
//! "Window manager" – which one runs, its version and screens, and the row "Shortcuts" with the
//! choice of modifiers (`settings.set_setting workspace_shortcuts`) and whether the keys work
//! (`Status::shortcuts`); without a window manager it says which one is expected and what to do.
//!
//! Then the templates, sorted by key (1 … 9, 0, then those without a key by name): the key as a
//! `kbd`, the name, windows and age, "At startup · DP-1" when it loads at login, one visible
//! action, "Load" (into the active workspace, mode Add), and the "…" menu with "Edit…" (the
//! template dialog, [`super::template_form`]), "Update from current workspace" and "Delete…" (with
//! confirmation). The header's primary button "Save workspace…" saves the active workspace
//! (`SaveTemplate`) and opens the dialog in the mode `Save` for the new template once the core
//! answers ([`answered`]). Menu, dialog and confirmation are layers over the whole window
//! ([`overlay`]).

use chrono::{DateTime, Utc};
use iced::widget::{container, row};
use iced::{Alignment, Element};
use torro_core::i18n::{tr, trf};
use torro_core::settings::ShortcutModifiers;
use torro_gui::datetime;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, empty_state};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response, SettingKey};
use torro_setups::{LoadMode, TemplateSummary};

use super::template_form::{self, Facts, Form, Mode, Step};
use super::{Action, Data, Header, confirm, dialog, picker, setting_row};
use crate::workspace_keys;

/// Page-local state: at most one row has its menu open, at most one dialog or confirmation is open.
#[derive(Debug, Default)]
pub struct State {
    /// Id of the template whose "…" menu is open.
    menu: Option<String>,
    /// Where the "…" button of the open menu is, for the layer that floats next to it. Only that
    /// one button registers, so a single anchor serves all rows.
    anchor: Anchor,
    /// The template dialog, from "Edit…" or after "Save workspace…".
    form: Option<Form>,
    /// "Save workspace…" was sent; its answer opens the dialog in the mode `Save`.
    saving: bool,
    /// Id of the template whose delete dialog is open.
    confirm_delete: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The "…" button of a row: opens its menu, or closes it when it is open.
    ToggleMenu(String),
    /// A click next to the menu, or Escape.
    CloseMenu,
    /// "Edit…" in the menu: the template dialog in the mode `Edit`.
    Edit(String),
    /// "Update from current workspace" in the menu: captures the active workspace into it again.
    Update(String),
    /// "Load" in a row.
    Load(String),
    /// "Delete…" in the menu: opens the confirmation.
    AskDelete(String),
    /// "Cancel" in the dialog, or a click next to it.
    CancelDelete,
    /// "Delete template" in the dialog.
    ConfirmDelete,
    /// The header's "Save workspace…".
    SaveWorkspace,
    /// The template dialog.
    Form(template_form::Message),
    /// The modifiers of the template keys, chosen in the row "Shortcuts".
    Modifiers(ShortcutModifiers),
}

/// What Escape means on this page right now: close the delete confirmation, else cancel the
/// dialog, else close the menu.
pub fn escape(state: &State) -> Option<Message> {
    if state.confirm_delete.is_some() {
        Some(Message::CancelDelete)
    } else if state.form.is_some() {
        Some(Message::Form(template_form::Message::Cancel))
    } else if state.menu.is_some() {
        Some(Message::CloseMenu)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::ToggleMenu(id) => {
            state.menu = if state.menu.as_deref() == Some(id.as_str()) { None } else { Some(id) };
            Action::None
        }
        Message::CloseMenu => {
            state.menu = None;
            Action::None
        }
        Message::Edit(id) => {
            state.menu = None;
            state.form =
                find(data, &id).map(|template| Form::new(Mode::Edit, template, &data.templates, &data.screens));
            Action::None
        }
        Message::Update(id) => {
            state.menu = None;
            Action::Send(Request::UpdateTemplate { id })
        }
        Message::Load(id) => Action::Send(Request::LoadTemplate { id, mode: LoadMode::Add }),
        Message::AskDelete(id) => {
            state.menu = None;
            state.confirm_delete = Some(id);
            Action::None
        }
        Message::CancelDelete => {
            state.confirm_delete = None;
            Action::None
        }
        Message::ConfirmDelete => match state.confirm_delete.take() {
            Some(id) => Action::Send(Request::DeleteTemplate { id }),
            None => Action::None,
        },
        Message::SaveWorkspace if state.saving || state.form.is_some() || !supported(data) => Action::None,
        Message::SaveWorkspace => {
            state.saving = true;
            Action::Send(Request::SaveTemplate { name: None })
        }
        Message::Form(message) => match state.form.as_mut() {
            Some(form) => {
                let step = form.update(message);
                step_action(state, step)
            }
            None => Action::None,
        },
        Message::Modifiers(modifiers) => {
            let value = serde_json::to_value(modifiers).unwrap_or_default();
            Action::Send(Request::SetSetting { key: SettingKey::WorkspaceShortcuts, value })
        }
    }
}

/// What the shell does for a [`Step`] of the dialog; closing drops it.
fn step_action(state: &mut State, step: Step) -> Action {
    match step {
        Step::Stay => Action::None,
        Step::Send(request) => Action::Send(request),
        Step::Close => {
            state.form = None;
            Action::None
        }
    }
}

/// The core's answer to a request of this page, if it is one the page waits for: the dialog's
/// requests, and "Save workspace…", whose `Saved` opens the dialog in the mode `Save`. `None` for
/// any other answer; a failed save is `None` too, so the shell shows its notice.
pub fn answered(state: &mut State, result: &Result<Response, String>, data: &Data) -> Option<Action> {
    if let Some(step) = state.form.as_mut().and_then(|form| form.answered(result)) {
        return Some(step_action(state, step));
    }
    if !state.saving {
        return None;
    }
    state.saving = false;
    match result {
        Ok(Response::Saved { template }) => {
            state.form = Some(Form::new(Mode::Save, template, &data.templates, &data.screens));
            Some(Action::None)
        }
        _ => None,
    }
}

/// A window manager runs that Torro can work with.
fn supported(data: &Data) -> bool {
    data.status.as_ref().is_some_and(|status| status.backend != "unsupported")
}

/// The header's primary button "Save workspace…" (icon only in narrow windows); off without a window
/// manager, waiting while the save is on its way.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    let label = tr("Save workspace…");
    let on = (supported(data) && state.form.is_none()).then_some(Message::SaveWorkspace);
    let button = match class {
        _ if state.saving => widgets::busy_button(tr("Saving…"), style::button::primary),
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, on)
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, on)
        }
    };
    Header { sub_page: None, actions: vec![button] }
}

pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let narrow = matches!(class, SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip);
    let manager = window_manager(data);
    let templates: Element<'a, Message> = if data.templates.is_empty() {
        empty_state(tr("No templates yet"), tr("Arrange the windows of a workspace, then choose \"Save workspace…\"."))
    } else {
        let now = Utc::now();
        let supported = supported(data);
        let rows = sorted(&data.templates)
            .into_iter()
            .map(|template| template_row(state, template, now, supported, narrow))
            .collect();
        let footer = tr(
            "Load adds the windows to the workspace you are on. Edit a template to give it a key or load it at startup.",
        );
        widgets::group(tr("Templates"), rows, Some(footer.to_string()))
    };
    iced::widget::column![manager, templates].spacing(space::xl()).into()
}

/// The templates in the order of their keys on the keyboard (1 … 9, 0), then those without a key
/// by name.
/// Same order as the tray menu.
pub fn sorted(templates: &[TemplateSummary]) -> Vec<&TemplateSummary> {
    crate::workspace_keys::in_key_order(templates)
}

/// The group "Window manager": the window manager with its version and screens, and the row
/// "Shortcuts".
fn window_manager<'a>(data: &'a Data) -> Element<'a, Message> {
    let Some(status) = data.status.as_ref().filter(|_| supported(data)) else {
        return unsupported();
    };
    let mut detail = Vec::new();
    if let Some(version) = &status.wm_version {
        detail.push(trf("Version {version}", &[("version", version)]));
    }
    if !data.screens.is_empty() {
        detail.push(screens_text(data));
    }
    let detail = (!detail.is_empty()).then(|| detail.join(" · "));
    let manager =
        setting_row(None, manager_name(&status.backend), detail, state_word(widgets::Status::Ok, tr("Detected")));
    let rows = vec![manager, shortcuts_row(data)];
    widgets::group(tr("Window manager"), rows, Some(tr("Torro registers the keys while it runs.").to_string()))
}

/// "2 screens: DP-1 (main), HDMI-A-1".
fn screens_text(data: &Data) -> String {
    let names: Vec<String> = data
        .screens
        .iter()
        .map(|screen| match screen.main {
            true => trf("{screen} (main)", &[("screen", &screen.name)]),
            false => screen.name.clone(),
        })
        .collect();
    let count = match data.screens.len() {
        1 => tr("1 screen").to_string(),
        n => trf("{n} screens", &[("n", &n)]),
    };
    format!("{count}: {}", names.join(", "))
}

/// The row "Shortcuts": what the keys do or why they do not work, and the choice of modifiers.
fn shortcuts_row<'a>(data: &'a Data) -> Element<'a, Message> {
    let modifiers = data.settings.workspaces.modifiers;
    let keys = format!("{} … 0", workspace_keys::label(modifiers, 1));
    let problem =
        data.status.as_ref().and_then(|status| status.shortcuts.as_ref()).and_then(|keys| keys.problem.clone());
    let (mark, detail) = match problem {
        Some(problem) => (Some(widgets::status_dot(widgets::Status::Attention)), problem),
        None => (None, trf("{keys} load the template on that key", &[("keys", &keys)])),
    };
    let choices: Vec<Modifiers> = ShortcutModifiers::ALL.into_iter().map(Modifiers).collect();
    let choice = picker(choices, Modifiers(modifiers), |chosen: Modifiers| Message::Modifiers(chosen.0));
    setting_row(mark, tr("Shortcuts"), Some(detail), choice)
}

/// The group without a window manager: which one this system needs, and what to do.
fn unsupported<'a>() -> Element<'a, Message> {
    let (name, system) = expected();
    let manager = setting_row(
        None,
        trf("{name} not found", &[("name", &name)]),
        Some(trf("Workspaces on {system} need {name}", &[("system", &system), ("name", &name)])),
        state_word(widgets::Status::Attention, tr("Not running")),
    );
    let off = Some(tr("Off until a window manager is found").to_string());
    let keys = setting_row(None, tr("Shortcuts"), off, iced::widget::Space::new());
    let footer = trf("Install and start {name}, then Torro finds it on its own.", &[("name", &name)]);
    widgets::group(tr("Window manager"), vec![manager, keys], Some(footer))
}

/// A row's state as a dot and a word ("● Detected"): the colour only in the dot.
fn state_word<'a>(status: widgets::Status, word: &'a str) -> Element<'a, Message> {
    row![widgets::status_dot(status), typo::text(typo::meta(), word).style(theme::text_2)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// The window manager this system works with, and the system's name: Hyprland on Linux, AeroSpace on
/// macOS, GlazeWM on Windows. Names, not translated.
fn expected() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("AeroSpace", "macOS")
    } else if cfg!(windows) {
        ("GlazeWM", "Windows")
    } else {
        ("Hyprland", "Linux")
    }
}

/// The name of the backend the core reports (`hyprland`) as people write it.
fn manager_name(backend: &str) -> String {
    match backend {
        "hyprland" => "Hyprland".into(),
        "aerospace" => "AeroSpace".into(),
        "glazewm" => "GlazeWM".into(),
        other => other.to_string(),
    }
}

/// A choice of modifiers in the picker, written as this system's keyboard names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Modifiers(ShortcutModifiers);

impl std::fmt::Display for Modifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&workspace_keys::modifiers_label(self.0))
    }
}

/// The page's layer over the whole window: the delete confirmation, else the template dialog, else
/// the "…" menu of a row.
pub fn overlay<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Option<Element<'a, Message>> {
    if let Some(template) = state.confirm_delete.as_ref().and_then(|id| find(data, id)) {
        return Some(confirm(
            tr("Delete this template?"),
            trf("\"{name}\" will be deleted. Saved backups stay in the data folder.", &[("name", &template.name)]),
            tr("Delete template"),
            Message::ConfirmDelete,
            Message::CancelDelete,
        ));
    }
    if let Some(form) = &state.form {
        let narrow = matches!(class, SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip);
        let facts = Facts {
            templates: &data.templates,
            screens: &data.screens,
            modifiers: data.settings.workspaces.modifiers,
            narrow,
        };
        let content = template_form::view(form, facts).map(Message::Form);
        return Some(dialog(content, size::form_max(), Message::Form(template_form::Message::Dismiss)));
    }
    let template = state.menu.as_ref().and_then(|id| find(data, id))?;
    Some(widgets::floating(
        &state.anchor,
        Placement::Below(Align::End),
        row_menu(template, supported(data)),
        Message::CloseMenu,
    ))
}

/// The "…" menu of a row: what a row does not show. Updating needs the window manager.
fn row_menu(template: &TemplateSummary, supported: bool) -> Element<'_, Message> {
    let id = &template.id;
    let mut edit = vec![MenuItem::new(Icon::SquarePen, tr("Edit…"), Message::Edit(id.clone()))];
    if supported {
        edit.push(MenuItem::new(Icon::RefreshCw, tr("Update from current workspace"), Message::Update(id.clone())));
    }
    widgets::menu(vec![
        MenuSection::new(edit),
        MenuSection::new(vec![MenuItem::new(Icon::Trash2, tr("Delete…"), Message::AskDelete(id.clone()))]),
    ])
}

/// "1 window", "3 windows".
pub fn windows_text(windows: usize) -> String {
    match windows {
        1 => tr("1 window").to_string(),
        n => trf("{n} windows", &[("n", &n)]),
    }
}

/// "At startup · DP-1" for a template that loads at login.
fn startup_text(template: &TemplateSummary) -> Option<String> {
    let startup = template.startup.as_ref()?;
    Some(trf("At startup · {screen}", &[("screen", &startup.screen)]))
}

/// One template: its key, name, meta line, "At startup · DP-1", "Load" and the "…" menu button. In
/// narrow windows the startup takes the age's place in the meta line, so the name keeps its room and
/// the startup is not cut off.
fn template_row<'a>(
    state: &'a State,
    template: &'a TemplateSummary,
    now: DateTime<Utc>,
    supported: bool,
    narrow: bool,
) -> Element<'a, Message> {
    let startup = startup_text(template);
    let second = match (&startup, narrow) {
        (Some(startup), true) => startup.clone(),
        _ => trf("Updated {when}", &[("when", &relative_time(template.updated, now))]),
    };
    let meta = [windows_text(template.windows), second];
    let id = &template.id;
    let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(id.clone())));
    // Only the button of the open menu registers its place; the menu floats next to it.
    let more = if state.menu.as_deref() == Some(id.as_str()) { widgets::anchored(&state.anchor, more) } else { more };
    let load =
        widgets::text_button(None, tr("Load"), style::button::secondary, supported.then(|| Message::Load(id.clone())));
    let mut actions = row![].spacing(space::s()).align_y(Alignment::Center);
    if !narrow && let Some(startup) = startup {
        actions = actions.push(widgets::state_pill(Icon::Clock, startup));
    }
    let actions = actions.push(load).push(more);
    // The keys in one column, so the names stand under each other with and without a key.
    let key = template.shortcut.map(|key| widgets::kbd(key.to_string()));
    let key = container(iced::widget::row![].push(key)).center_x(size::control_sm());
    setting_row(Some(key.into()), template.name.as_str(), Some(meta.join(" · ")), actions)
}

fn find<'a>(data: &'a Data, id: &str) -> Option<&'a TemplateSummary> {
    data.templates.iter().find(|template| template.id == id)
}

/// How long ago a template was updated, for its row: "just now", "5 min ago", "2 h ago",
/// "3 d ago"; from a week on, the local date. A time in the future counts as "just now".
pub fn relative_time(updated: DateTime<Utc>, now: DateTime<Utc>) -> String {
    const MINUTE: i64 = 1;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let minutes = (now - updated).num_minutes();
    if minutes < MINUTE {
        tr("just now").to_string()
    } else if minutes < HOUR {
        trf("{n} min ago", &[("n", &minutes)])
    } else if minutes < DAY {
        trf("{n} h ago", &[("n", &(minutes / HOUR))])
    } else if minutes < 7 * DAY {
        trf("{n} d ago", &[("n", &(minutes / DAY))])
    } else {
        datetime::format_date(datetime::to_local(updated).date())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use torro_ipc::{ScreenInfo, ShortcutStatus, Status};

    use super::*;

    fn template(id: &str, name: &str, key: Option<u8>) -> TemplateSummary {
        let updated = Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap();
        TemplateSummary {
            id: id.into(),
            name: name.into(),
            windows: 3,
            updated,
            shortcut: key,
            startup: None,
            workspace: "1".into(),
            monitor: None,
        }
    }

    fn status(backend: &str) -> Status {
        Status {
            version: "1".into(),
            backend: backend.into(),
            workspace: Some("1".into()),
            templates: 2,
            busy: false,
            wm_version: Some("0.56.0".into()),
            shortcuts: Some(ShortcutStatus { modifiers: ShortcutModifiers::SuperAlt, active: true, problem: None }),
        }
    }

    fn data() -> Data {
        Data {
            templates: vec![template("dev", "Dev", None), template("mail", "Mail", Some(2))],
            status: Some(status("hyprland")),
            screens: vec![ScreenInfo { name: "DP-1".into(), width: 2560, height: 1440, scale: 1.0, main: true }],
            ..Data::default()
        }
    }

    fn sent(action: Action) -> Option<Request> {
        match action {
            Action::Send(request) => Some(request),
            _ => None,
        }
    }

    #[test]
    fn relative_time_picks_the_coarsest_unit() {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 12, 0, 0).unwrap();
        assert_eq!(relative_time(now - Duration::seconds(20), now), tr("just now"));
        assert_eq!(relative_time(now + Duration::minutes(5), now), tr("just now"));
        assert_eq!(relative_time(now - Duration::minutes(5), now), trf("{n} min ago", &[("n", &5)]));
        assert_eq!(relative_time(now - Duration::hours(2), now), trf("{n} h ago", &[("n", &2)]));
        assert_eq!(relative_time(now - Duration::days(3), now), trf("{n} d ago", &[("n", &3)]));
        let old = now - Duration::days(10);
        assert_eq!(relative_time(old, now), datetime::format_date(datetime::to_local(old).date()));
    }

    /// Task WS.6: by key in the order of the keyboard, 0 after 9, then without a key by name.
    #[test]
    fn templates_are_sorted_by_key_then_by_name() {
        let templates = vec![
            template("z", "zebra", None),
            template("zero", "Zero", Some(0)),
            template("a", "Alpha", None),
            template("nine", "Nine", Some(9)),
            template("one", "One", Some(1)),
        ];
        let ids: Vec<&str> = sorted(&templates).iter().map(|template| template.id.as_str()).collect();
        assert_eq!(ids, ["one", "nine", "zero", "a", "z"]);
    }

    #[test]
    fn load_sends_add_mode() {
        let mut state = State::default();
        let request = sent(update(&mut state, Message::Load("dev".into()), &data()));
        assert_eq!(request, Some(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add }));
    }

    #[test]
    fn update_captures_the_workspace_into_the_template() {
        let mut state = State::default();
        update(&mut state, Message::ToggleMenu("dev".into()), &data());
        let request = sent(update(&mut state, Message::Update("dev".into()), &data()));
        assert_eq!(request, Some(Request::UpdateTemplate { id: "dev".into() }));
        assert!(state.menu.is_none());
    }

    #[test]
    fn the_modifiers_are_a_setting() {
        let mut state = State::default();
        let request = sent(update(&mut state, Message::Modifiers(ShortcutModifiers::CtrlShift), &data()));
        let value = serde_json::json!("ctrl_shift");
        assert_eq!(request, Some(Request::SetSetting { key: SettingKey::WorkspaceShortcuts, value }));
    }

    #[test]
    fn the_menu_opens_toggles_and_closes() {
        let data = data();
        let mut state = State::default();
        assert!(sent(update(&mut state, Message::ToggleMenu("dev".into()), &data)).is_none());
        assert_eq!(state.menu.as_deref(), Some("dev"));
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert_eq!(state.menu.as_deref(), Some("mail"), "another row takes the menu over");
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert!(state.menu.is_none(), "the same button closes it again");
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        update(&mut state, Message::CloseMenu, &data);
        assert!(state.menu.is_none());
    }

    /// Task WS.6: "Edit…" opens the dialog; choosing a key another template holds and saving sends
    /// only the key, and the dialog closes once the core took it.
    #[test]
    fn edit_opens_the_dialog_and_moving_a_key_sends_only_the_key() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        update(&mut state, Message::Edit("dev".into()), &data);
        assert!(state.menu.is_none());
        assert!(overlay(&state, &data, SizeClass::Wide).is_some(), "the dialog is a window layer");
        update(&mut state, Message::Form(template_form::Message::Key(Some(2))), &data);
        let request = sent(update(&mut state, Message::Form(template_form::Message::Save), &data));
        assert_eq!(request, Some(Request::SetTemplateShortcut { id: "dev".into(), key: Some(2) }));
        assert!(matches!(answered(&mut state, &Ok(Response::Ok), &data), Some(Action::None)));
        assert!(state.form.is_none());
        assert!(answered(&mut state, &Ok(Response::Ok), &data).is_none(), "nothing waits any more");
    }

    #[test]
    fn edit_of_an_unknown_template_opens_nothing() {
        let mut state = State::default();
        update(&mut state, Message::Edit("gone".into()), &data());
        assert!(state.form.is_none());
    }

    /// Task WS.6: "Save workspace…" saves, and the core's `Saved` opens the dialog in the mode
    /// `Save`; its Cancel deletes the template, Escape is Cancel.
    #[test]
    fn save_workspace_opens_the_dialog_and_its_cancel_deletes() {
        let data = data();
        let mut state = State::default();
        let request = sent(update(&mut state, Message::SaveWorkspace, &data));
        assert_eq!(request, Some(Request::SaveTemplate { name: None }));
        assert!(sent(update(&mut state, Message::SaveWorkspace, &data)).is_none(), "one save at a time");
        let saved = template("ws-3", "Workspace 3, 20.10. 10:41", None);
        assert!(answered(&mut state, &Ok(Response::Saved { template: saved }), &data).is_some());
        let form = state.form.as_ref().expect("the dialog");
        assert_eq!((form.id(), form.mode()), ("ws-3", Mode::Save));
        let escape = escape(&state).expect("Escape cancels the dialog");
        let request = sent(update(&mut state, escape, &data));
        assert_eq!(request, Some(Request::DeleteTemplate { id: "ws-3".into() }));
        answered(&mut state, &Ok(Response::Ok), &data);
        assert!(state.form.is_none());
    }

    #[test]
    fn a_failed_save_leaves_the_notice_to_the_shell() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::SaveWorkspace, &data);
        assert!(answered(&mut state, &Err("busy".into()), &data).is_none());
        assert!(!state.saving && state.form.is_none());
    }

    #[test]
    fn without_a_window_manager_nothing_saves_or_loads() {
        let data = Data { status: Some(status("unsupported")), ..data() };
        let mut state = State::default();
        assert!(sent(update(&mut state, Message::SaveWorkspace, &data)).is_none());
        drop(view(&state, &data, SizeClass::Wide));
        drop(header(&state, &data, SizeClass::Narrow));
    }

    #[test]
    fn delete_needs_confirmation() {
        let data = data();
        let mut state = State::default();
        assert!(sent(update(&mut state, Message::AskDelete("mail".into()), &data)).is_none());
        assert!(sent(update(&mut state, Message::CancelDelete, &data)).is_none());
        assert!(state.confirm_delete.is_none());

        update(&mut state, Message::AskDelete("mail".into()), &data);
        let request = sent(update(&mut state, Message::ConfirmDelete, &data));
        assert_eq!(request, Some(Request::DeleteTemplate { id: "mail".into() }));
        assert!(state.confirm_delete.is_none());
        assert!(sent(update(&mut state, Message::ConfirmDelete, &data)).is_none(), "one confirmation, one delete");
    }

    #[test]
    fn escape_closes_the_confirmation_then_the_dialog_then_the_menu() {
        let data = data();
        let mut state = State::default();
        assert!(escape(&state).is_none());
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert!(matches!(escape(&state), Some(Message::CloseMenu)));
        update(&mut state, Message::Edit("mail".into()), &data);
        assert!(matches!(escape(&state), Some(Message::Form(template_form::Message::Cancel))));
        update(&mut state, Message::AskDelete("mail".into()), &data);
        assert!(matches!(escape(&state), Some(Message::CancelDelete)));
    }

    #[test]
    fn views_build_for_empty_and_filled_lists_in_every_class() {
        let mut filled = data();
        filled.templates[1].startup =
            Some(torro_setups::Startup { screen: "DP-1".into(), fallback: Default::default() });
        filled.status.as_mut().and_then(|status| status.shortcuts.as_mut()).expect("keys").problem =
            Some("Super + Alt + 5 could not be registered".into());
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            drop(view(&State::default(), &Data::default(), class));
            let mut state = State::default();
            drop(view(&state, &filled, class));
            update(&mut state, Message::Edit("mail".into()), &filled);
            assert!(overlay(&state, &filled, class).is_some());
            drop(header(&state, &filled, class));
        }
    }

    #[test]
    fn the_open_menu_is_a_window_layer_for_a_known_template_only() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());
        update(&mut state, Message::ToggleMenu("gone".into()), &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_none(), "a menu for a template that is gone shows nothing");
    }

    #[test]
    fn the_screens_are_counted_and_the_main_one_named() {
        let mut data = data();
        assert_eq!(
            screens_text(&data),
            format!("{}: {}", tr("1 screen"), trf("{screen} (main)", &[("screen", &"DP-1")]))
        );
        data.screens.push(ScreenInfo { name: "HDMI-A-1".into(), width: 1920, height: 1080, scale: 1.0, main: false });
        assert!(screens_text(&data).ends_with(", HDMI-A-1"));
        assert_eq!(windows_text(1), tr("1 window"));
    }
}
