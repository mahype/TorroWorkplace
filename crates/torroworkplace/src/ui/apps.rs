//! The apps of the suite (task S.8 in `docs/tasks/steuerzentrale.md`): a settings group with one
//! row per app the core reports with `Request::ListApps`. A row shows the name with the window
//! class (for window rules) quietly behind it, below it a one-line description, at the right
//! whether the app is installed (dot and word) and its one action, an "Open" button that sends
//! `Request::OpenApp`. Not installed means the button is disabled. Everything in a row stays on
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
    /// "Open" was pressed for the app with this binary name.
    Open(String),
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::Open(app) => Action::Send(Request::OpenApp { app }),
    }
}

pub fn view<'a>(_state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let title = tr("Suite apps");
    match &data.apps {
        // The core has not answered yet: one quiet line, so the page does not jump when the rows come.
        None => column![widgets::section_title(title), typo::text(typo::META, tr("Loading …")).style(theme::text_3)]
            .spacing(space::S)
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
        row![widgets::status_dot(status), typo::text(typo::META, word).style(theme::text_2).wrapping(Wrapping::None)]
            .spacing(space::S)
            .align_y(Alignment::Center);
    // A button without a message is drawn disabled, so a missing app cannot be opened.
    let open = widgets::text_button(None, tr("Open"), style::button::secondary, open_message(app));
    let control = row![state, open].spacing(space::L).align_y(Alignment::Center);

    // The class sits in the name's line, in the metadata colour, so a row stays at two lines.
    let name = row![
        typo::text(typo::BODY, app_name(app)).wrapping(Wrapping::None),
        typo::text(typo::META, app_class(app)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    let mut lines = column![container(name).width(Fill).clip(true)];
    let description = app_description(&app.binary);
    if !description.is_empty() {
        lines = lines.push(one_line(typo::text(typo::META, description).style(theme::text_2)));
    }
    let content = row![lines.width(Fill), control].spacing(space::L).align_y(Alignment::Center);
    row_frame(content, !description.is_empty())
}

/// The window class for window rules, as shown behind the name.
fn app_class(app: &AppInfo) -> String {
    trf("Window class: {class}", &[("class", &app.window_class)])
}

/// The message "Open" sends for `app`, or `None` for an app that is not installed.
fn open_message(app: &AppInfo) -> Option<Message> {
    app.installed.then(|| Message::Open(app.binary.clone()))
}

/// The name shown for `app`: translated for the apps the suite ships, else the name the core reports.
fn app_name(app: &AppInfo) -> String {
    match app.binary.as_str() {
        "torrocalendar" => tr("Calendar").to_string(),
        "torrotasks" => tr("Tasks").to_string(),
        _ => app.name.clone(),
    }
}

/// The one-line description; empty for an app without one, which then gets no line.
fn app_description(binary: &str) -> &'static str {
    match binary {
        "torrocalendar" => tr("Day, week, month and agenda"),
        "torrotasks" => tr("Lists, due dates and subtasks"),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(binary: &str, name: &str, installed: bool) -> AppInfo {
        AppInfo { binary: binary.into(), name: name.into(), window_class: format!("torro-{binary}"), installed }
    }

    #[test]
    fn open_is_offered_only_for_installed_apps() {
        let installed = app("torrocalendar", "Torro Calendar", true);
        let missing = app("torrotasks", "Torro Tasks", false);
        assert!(matches!(open_message(&installed), Some(Message::Open(binary)) if binary == "torrocalendar"));
        assert!(open_message(&missing).is_none());
    }

    #[test]
    fn open_asks_the_daemon_to_open_the_app() {
        let message = Message::Open("torrocalendar".into());
        let action = update(&mut State::default(), message, &Data::default());
        assert!(matches!(action, Action::Send(Request::OpenApp { app }) if app == "torrocalendar"));
    }

    #[test]
    fn names_are_translated_with_a_fallback() {
        assert_eq!(app_name(&app("torrocalendar", "Torro Calendar", true)), tr("Calendar"));
        assert_eq!(app_name(&app("torrotasks", "Torro Tasks", true)), tr("Tasks"));
        assert_eq!(app_name(&app("torroother", "Other", true)), "Other");
        assert_eq!(app_description("torroother"), "");
    }

    #[test]
    fn the_window_class_is_named_and_the_description_stays_apart() {
        let calendar = app("torrocalendar", "Torro Calendar", true);
        assert_eq!(app_class(&calendar), trf("Window class: {class}", &[("class", &"torro-torrocalendar")]));
        assert_eq!(app_description(&calendar.binary), tr("Day, week, month and agenda"));
        assert!(!app_description(&calendar.binary).contains("torro-"), "the class is not part of the description");
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
