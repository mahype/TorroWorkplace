//! The apps of the suite (task S.8 in `docs/tasks/steuerzentrale.md`): a settings group with one
//! row per app the core reports with `Request::ListApps`. A row shows the name with the window
//! class (for window rules) quietly behind it, below it a one-line description, at the right
//! whether the app is installed (dot and word) and its one action, an "Open" button that sends
//! `Request::OpenApp` with the app's window class. Not installed means the button is disabled. Everything in a row stays on
//! its line and is cut off at the end when the window is narrow.
//!
//! The page does not look for the apps itself: the core knows where its apps are, so the control
//! centre and the daemon agree on what they can start. Until the first answer arrives, the group
//! shows a quiet loading line.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{style, typo};
use torro_ipc::{AppInfo, Request};

use super::{Action, Data, one_line, row_frame};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    /// "Open" was pressed for the app with this window class. The class, not the binary: the
    /// inbox's three windows share the binary `torroinbox` (task E8.1).
    Open(String),
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::Open(app) => Action::Send(Request::OpenApp { app, target: None }),
    }
}

pub fn view<'a>(_state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let title = tr("Suite apps");
    match &data.apps {
        // The core has not answered yet: one quiet line, so the page does not jump when the rows come.
        None => column![widgets::section_title(title), typo::text(typo::meta(), tr("Loading …")).style(theme::text_3)]
            .spacing(space::s())
            .into(),
        Some(apps) => widgets::group(
            title,
            apps.iter().map(app_row).collect(),
            Some(tr("Opening an app brings its window to the front when it is already running.").to_string()),
        ),
    }
}

/// One app as a row: name and window class and the description on the left, status and "Open" on
/// the right.
fn app_row<'a>(app: &'a AppInfo) -> Element<'a, Message> {
    let (status, word) =
        if app.installed { (Status::Ok, tr("Installed")) } else { (Status::Inactive, tr("Not installed")) };
    let state =
        row![widgets::status_dot(status), typo::text(typo::meta(), word).style(theme::text_2).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    // A button without a message is drawn disabled, so a missing app cannot be opened.
    let open = widgets::text_button(None, tr("Open"), style::button::secondary, open_message(app));
    let control = row![state, open].spacing(space::l()).align_y(Alignment::Center);

    // The class sits in the name's line, in the metadata colour, so a row stays at two lines.
    let name = row![
        typo::text(typo::body(), app_name(app)).wrapping(Wrapping::None),
        typo::text(typo::meta(), app_class(app)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let mut lines = column![container(name).width(Fill).clip(true)];
    let description = app_description(&app.window_class);
    if !description.is_empty() {
        lines = lines.push(one_line(typo::text(typo::meta(), description).style(theme::text_2)));
    }
    let content = row![lines.width(Fill), control].spacing(space::l()).align_y(Alignment::Center);
    row_frame(content, !description.is_empty())
}

/// The window class for window rules, as shown behind the name.
fn app_class(app: &AppInfo) -> String {
    trf("Window class: {class}", &[("class", &app.window_class)])
}

/// The message "Open" sends for `app`, or `None` for an app that is not installed.
fn open_message(app: &AppInfo) -> Option<Message> {
    app.installed.then(|| Message::Open(app.window_class.clone()))
}

/// The name shown for `app`: translated for the windows the suite ships, else the name the core
/// reports. Chosen by window class, not by binary (task E8.1b): the inbox's three windows share
/// `torroinbox` and each has its own name, the one the app switcher shows.
fn app_name(app: &AppInfo) -> String {
    match app.window_class.as_str() {
        "torro-calendar" => tr("Calendar").to_string(),
        "torro-tasks" => tr("Tasks").to_string(),
        "torro-inbox" => tr("Notification Hub").to_string(),
        "torro-inbox-mail" => tr("Mail").to_string(),
        "torro-inbox-notifications" => tr("App Feed").to_string(),
        _ => app.name.clone(),
    }
}

/// The one-line description of the window with the class `window_class`; empty for one without,
/// which then gets no line.
fn app_description(window_class: &str) -> &'static str {
    match window_class {
        "torro-calendar" => tr("Day, week, month and agenda"),
        "torro-tasks" => tr("Lists, due dates and subtasks"),
        "torro-inbox" => tr("Decide, hand on and see where it all comes from"),
        "torro-inbox-mail" => tr("Your mail, from a quick look to a full client"),
        "torro-inbox-notifications" => tr("What your apps report, one column per app"),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(binary: &str, name: &str, installed: bool) -> AppInfo {
        let window_class = match binary {
            "torrocalendar" => "torro-calendar".to_string(),
            "torrotasks" => "torro-tasks".to_string(),
            other => format!("torro-{other}"),
        };
        AppInfo { binary: binary.into(), name: name.into(), window_class, installed }
    }

    /// One of the inbox's windows as the core lists it: the binary they share, its own class.
    fn inbox(window_class: &str, name: &str) -> AppInfo {
        AppInfo { binary: "torroinbox".into(), name: name.into(), window_class: window_class.into(), installed: true }
    }

    #[test]
    fn open_is_offered_only_for_installed_apps() {
        let installed = app("torrocalendar", "Torro Calendar", true);
        let missing = app("torrotasks", "Torro Tasks", false);
        assert!(matches!(open_message(&installed), Some(Message::Open(class)) if class == installed.window_class));
        assert!(open_message(&missing).is_none());
    }

    /// The inbox's windows share a binary (task E8.1): each row opens its own window by class.
    #[test]
    fn open_names_the_window_class() {
        let mail = AppInfo {
            binary: "torroinbox".into(),
            name: "Torro Workplace Mail".into(),
            window_class: "torro-inbox-mail".into(),
            installed: true,
        };
        assert!(matches!(open_message(&mail), Some(Message::Open(class)) if class == "torro-inbox-mail"));
    }

    #[test]
    fn open_asks_the_daemon_to_open_the_app() {
        let message = Message::Open("torro-calendar".into());
        let action = update(&mut State::default(), message, &Data::default());
        assert!(matches!(action, Action::Send(Request::OpenApp { app, .. }) if app == "torro-calendar"));
    }

    #[test]
    fn names_are_translated_with_a_fallback() {
        assert_eq!(app_name(&app("torrocalendar", "Torro Calendar", true)), tr("Calendar"));
        assert_eq!(app_name(&app("torrotasks", "Torro Tasks", true)), tr("Tasks"));
        assert_eq!(app_name(&app("torroother", "Other", true)), "Other");
        assert_eq!(app_description("torro-torroother"), "");
    }

    /// Task E8.1b: the inbox's three windows share a binary, yet each row has its own name and line.
    #[test]
    fn the_inbox_windows_have_their_own_name_and_description() {
        let rows = [
            inbox("torro-inbox-mail", "Torro Workplace Mail"),
            inbox("torro-inbox-notifications", "Torro App Feed"),
            inbox("torro-inbox", "Torro Notification Hub"),
        ];
        let names: Vec<String> = rows.iter().map(app_name).collect();
        assert_eq!(names, [tr("Mail"), tr("App Feed"), tr("Notification Hub")]);
        let lines: Vec<&str> = rows.iter().map(|row| app_description(&row.window_class)).collect();
        assert_eq!(
            lines,
            [
                tr("Your mail, from a quick look to a full client"),
                tr("What your apps report, one column per app"),
                tr("Decide, hand on and see where it all comes from"),
            ]
        );
        let loaded = Data { apps: Some(rows.to_vec()), ..Data::default() };
        let state = State::default();
        let _page: Element<'_, Message> = view(&state, &loaded);
    }

    #[test]
    fn the_window_class_is_named_and_the_description_stays_apart() {
        let calendar = app("torrocalendar", "Torro Calendar", true);
        assert_eq!(app_class(&calendar), trf("Window class: {class}", &[("class", &"torro-calendar")]));
        assert_eq!(app_description(&calendar.window_class), tr("Day, week, month and agenda"));
        assert!(
            !app_description(&calendar.window_class).contains("torro-"),
            "the class is not part of the description"
        );
    }

    #[test]
    fn view_builds_while_loading_and_with_apps() {
        let state = State::default();
        let loading = Data::default();
        let _page: Element<'_, Message> = view(&state, &loading);

        let loaded = Data {
            apps: Some(vec![app("torrocalendar", "Torro Calendar", true), app("torrotasks", "Torro Tasks", false)]),
            ..Data::default()
        };
        let _page: Element<'_, Message> = view(&state, &loaded);
    }
}
