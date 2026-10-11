//! The apps of the suite (task S.8 in `docs/tasks/steuerzentrale.md`): one row per app from
//! `torro_core::AppId::ALL` except the core itself. A row shows the name, a one-line
//! description, the window class (for window rules), whether the app is installed, and an
//! "Open" button that sends `Request::OpenApp`. Not installed means the button is disabled.
//!
//! "Installed" is looked up next to this executable and on `PATH`, so the control centre
//! and the daemon agree on what they can start.

use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Background, Element, Length, border};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::theme::STATUS_OK;
use torro_gui::{theme, widgets};
use torro_ipc::Request;

use super::{Action, Data};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    /// "Open" was pressed for the app with this binary name.
    Open(&'static str),
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::Open(binary) => Action::Send(Request::OpenApp { app: binary.into() }),
    }
}

pub fn view<'a>(_state: &'a State, _data: &'a Data) -> Element<'a, Message> {
    // Looked up on every redraw: a handful of file checks, and the page reloads every few seconds
    // anyway, so an app installed meanwhile shows up without reopening the window.
    let dirs = search_dirs();
    let mut list = column![widgets::section_title(tr("Suite apps"))].spacing(8);
    for app in listed_apps() {
        list = list.push(app_row(app, is_installed(app.binary, &dirs)));
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

/// The apps that get a row: every suite app except the core, which is the control centre itself.
fn listed_apps() -> impl Iterator<Item = &'static AppId> {
    AppId::ALL.iter().filter(|app| **app != AppId::WORKPLACE)
}

/// One app as a row: name and description on the left, status and "Open" on the right.
fn app_row<'a>(app: &AppId, installed: bool) -> Element<'a, Message> {
    let mut details = column![text(app_name(app)).size(16).color(theme::TEXT)].spacing(4).width(Length::Fill);
    let description = app_description(app);
    if !description.is_empty() {
        details = details.push(text(description).size(14).color(theme::MUTED));
    }
    let class = trf("Window class: {class}", &[("class", &app.window_class)]);
    details = details.push(text(class).size(12).color(theme::MUTED));

    let status = if installed {
        row![widgets::color_dot(theme::color(STATUS_OK), 8.0), text(tr("Installed")).size(14).color(theme::MUTED),]
    } else {
        row![widgets::color_dot(theme::MUTED, 8.0), text(tr("Not installed")).size(14).color(theme::MUTED)]
    }
    .spacing(8)
    .align_y(Alignment::Center);

    // A button without a message is drawn disabled, so a missing app cannot be opened.
    let open = button(text(tr("Open")).size(14))
        .style(button::secondary)
        .padding([8, 16])
        .on_press_maybe(installed.then_some(Message::Open(app.binary)));

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

/// The name shown for `app`: translated for the apps the suite ships, else the app's own name.
fn app_name(app: &AppId) -> &'static str {
    match app.binary {
        "torrocalendar" => tr("Calendar"),
        "torrotasks" => tr("Tasks"),
        _ => app.display_name,
    }
}

/// The one-line description; empty for an app without one, which then gets no line.
fn app_description(app: &AppId) -> &'static str {
    match app.binary {
        "torrocalendar" => tr("Day, week, month and agenda"),
        "torrotasks" => tr("Lists, due dates and subtasks"),
        _ => "",
    }
}

/// The folders an app binary is looked for in: the folder of this executable, then `PATH`.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// Whether a binary named `binary` exists in one of `dirs`. Takes the folders as an argument so
/// tests can use temporary ones.
fn is_installed(binary: &str, dirs: &[PathBuf]) -> bool {
    let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().any(|dir| dir.join(&file).is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detects_installed_apps_in_the_given_dirs() {
        let next_to_exe = tempfile::tempdir().expect("temp dir");
        let on_path = tempfile::tempdir().expect("temp dir");
        let empty = tempfile::tempdir().expect("temp dir");
        let binary = format!("torrocalendar{}", std::env::consts::EXE_SUFFIX);
        fs::write(on_path.path().join(&binary), b"").expect("write binary");
        // A directory with the right name is not an installed binary.
        fs::create_dir(next_to_exe.path().join("torrotasks")).expect("create dir");

        let dirs = vec![next_to_exe.path().to_path_buf(), empty.path().to_path_buf(), on_path.path().to_path_buf()];
        assert!(is_installed("torrocalendar", &dirs));
        assert!(!is_installed("torrotasks", &dirs));
        assert!(!is_installed("torrocalendar", &[empty.path().to_path_buf()]));
        assert!(!is_installed("torrocalendar", &[]));
    }

    #[test]
    fn rows_exclude_the_core() {
        let rows: Vec<&AppId> = listed_apps().collect();
        assert_eq!(rows.len(), AppId::ALL.len() - 1);
        assert!(rows.iter().all(|app| app.binary != AppId::WORKPLACE.binary));
        assert!(rows.iter().any(|app| app.binary == AppId::CALENDAR.binary));
        assert!(rows.iter().any(|app| app.binary == AppId::TASKS.binary));
    }

    #[test]
    fn names_are_translated_with_a_fallback() {
        assert_eq!(app_name(&AppId::CALENDAR), tr("Calendar"));
        assert_eq!(app_name(&AppId::TASKS), tr("Tasks"));
        let other = AppId { window_class: "torro-other", binary: "torroother", dir: "other", display_name: "Other" };
        assert_eq!(app_name(&other), "Other");
        assert_eq!(app_description(&other), "");
    }

    #[test]
    fn open_asks_the_daemon_to_open_the_app() {
        let action = update(&mut State::default(), Message::Open("torrocalendar"), &Data::default());
        assert!(matches!(action, Action::Send(Request::OpenApp { app }) if app == "torrocalendar"));
    }

    #[test]
    fn view_builds_for_every_state() {
        let state = State::default();
        let data = Data::default();
        let _page: Element<'_, Message> = view(&state, &data);
        let _row: Element<'_, Message> = app_row(&AppId::TASKS, false);
    }
}
