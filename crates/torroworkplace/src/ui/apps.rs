//! The apps of the suite (task S.8 in `docs/tasks/steuerzentrale.md`): one row per app the core
//! reports with `Request::ListApps`. A row shows the name, a one-line description, the window
//! class (for window rules), whether the app is installed, and an "Open" button that sends
//! `Request::OpenApp`. Not installed means the button is disabled.
//!
//! The page does not look for the apps itself: the core knows where its apps are, so the control
//! centre and the daemon agree on what they can start. Until the first answer arrives, the list
//! shows a quiet loading line.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Background, Element, Length, border};
use torro_core::i18n::{tr, trf};
use torro_core::theme::STATUS_OK;
use torro_gui::{theme, widgets};
use torro_ipc::{AppInfo, Request};

use super::{Action, Data};

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
    let mut list = column![widgets::section_title(tr("Suite apps"))].spacing(8);
    match &data.apps {
        // The core has not answered yet: one muted line, so the page does not jump when the rows come.
        None => list = list.push(text(tr("Loading …")).size(14).color(theme::MUTED)),
        Some(apps) => {
            for app in apps {
                list = list.push(app_row(app));
            }
        }
    }
    column![
        list,
        text(tr("Opening an app brings its window to the front when it is already running."))
            .size(14)
            .color(theme::MUTED),
    ]
    .spacing(16)
    .into()
}

/// One app as a row: name and description on the left, status and "Open" on the right.
fn app_row<'a>(app: &'a AppInfo) -> Element<'a, Message> {
    let mut details = column![text(app_name(app)).size(16).color(theme::TEXT)].spacing(4).width(Length::Fill);
    let description = app_description(&app.binary);
    if !description.is_empty() {
        details = details.push(text(description).size(14).color(theme::MUTED));
    }
    let class = trf("Window class: {class}", &[("class", &app.window_class)]);
    details = details.push(text(class).size(12).color(theme::MUTED));

    let status = if app.installed {
        row![widgets::color_dot(theme::color(STATUS_OK), 8.0), text(tr("Installed")).size(14).color(theme::MUTED),]
    } else {
        row![widgets::color_dot(theme::MUTED, 8.0), text(tr("Not installed")).size(14).color(theme::MUTED)]
    }
    .spacing(8)
    .align_y(Alignment::Center);

    // A button without a message is drawn disabled, so a missing app cannot be opened.
    let open =
        button(text(tr("Open")).size(14)).style(button::secondary).padding([8, 16]).on_press_maybe(open_message(app));

    let content = row![details, status, open].spacing(16).align_y(Alignment::Center);
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(theme::SURFACE)),
            border: border::rounded(8),
            ..container::Style::default()
        })
        .into()
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
