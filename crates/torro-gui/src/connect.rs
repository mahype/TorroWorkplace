//! The "no account yet" screen every app shows.
//!
//! Task S.10 in `docs/tasks/steuerzentrale.md`. The apps no longer sign in themselves: while
//! the core has no account ([`torro_pim::AccountState::NotConnected`]) the screen names the
//! app and points to the control centre, whose "Open Settings" button starts
//! `torroworkplace ui --section accounts`. While a sign-in runs (`SigningIn`) it only says so.
//! The app keeps polling the core and loads by itself once the account is `Connected`.
//!
//! UI text is English (the base language); German comes through the translation table.

use std::process::Command;

use iced::widget::{center, column, container, row};
use iced::{Element, Fill};

use torro_core::i18n::tr;
use torro_pim::AccountState;

use crate::source::sibling;
use crate::theme::{self, space};
use crate::{style, typo, widgets};

/// The screen keeps no state of its own; the app decides when it is shown.
#[derive(Debug, Clone, Copy, Default)]
pub struct Connect;

#[derive(Debug, Clone)]
pub enum Message {
    /// The user pressed "Open Settings".
    OpenSettings,
}

/// What the app should do after an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Call [`open_settings`].
    OpenSettings,
}

impl Connect {
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::OpenSettings => Action::OpenSettings,
        }
    }

    /// The screen for the current account state (`NotConnected` or `SigningIn`).
    ///
    /// Any state other than `SigningIn` shows the hint and the button, so a `Connected`
    /// account that is still on screen does not show a blank card. The card is a floating card
    /// (`style::container::popover`, as wide as one) centred on the window background: app name
    /// in `TITLE`, the hint in `text_2`, "Open Settings" as the window's one primary button.
    pub fn view<'a>(&'a self, account: &'a AccountState, app_name: &'a str) -> Element<'a, Message> {
        let heading = typo::text(typo::title(), app_name);

        let body: Element<'a, Message> = match account {
            AccountState::SigningIn { .. } => typo::text(typo::body(), tr("Signing in to Nextcloud …")).into(),
            _ => column![
                typo::text(
                    typo::body(),
                    tr("Set up your Nextcloud in Torro Workplace's settings to see events and tasks here.")
                )
                .style(theme::text_2),
                row![widgets::text_button(
                    None,
                    tr("Open Settings"),
                    style::button::primary,
                    Some(Message::OpenSettings)
                )],
            ]
            .spacing(space::l())
            .into(),
        };

        let card = container(column![heading, body].spacing(space::s()).width(Fill))
            .padding(space::xl())
            .width(Fill)
            .max_width(widgets::POPOVER_WIDTH)
            .style(style::container::popover);

        center(card).padding(space::l()).into()
    }
}

/// Opens the control centre on the accounts page. The app does not wait for it; the process
/// is reaped in the background once it exits. Failures are only logged, the app keeps polling.
pub fn open_settings() {
    open_section("accounts");
}

/// Opens the control centre on the page `section`, what `torroworkplace ui --section` takes
/// (`accounts`, `assistant` …), like [`open_settings`]: not waited for, failures only logged.
pub fn open_section(section: &str) {
    let program = sibling(torro_core::AppId::WORKPLACE.binary);
    let mut command = Command::new(&program);
    command
        .args(["ui", "--section", section])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Own process group, so closing the app does not take the settings window with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => eprintln!("cannot open {}: {err}", program.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_settings_button_asks_for_settings() {
        let mut connect = Connect;
        assert_eq!(connect.update(Message::OpenSettings), Action::OpenSettings);
    }

    #[test]
    fn view_builds_for_each_account_state() {
        let connect = Connect;
        let states = [
            AccountState::NotConnected,
            AccountState::SigningIn {
                server: "https://cloud.example.org".into(),
                login_url: "https://cloud.example.org/login/v2/flow/abc".into(),
            },
        ];
        for state in &states {
            let _element = connect.view(state, "Calendar");
        }
    }
}
