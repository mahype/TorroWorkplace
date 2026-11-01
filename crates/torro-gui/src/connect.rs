//! The "no account yet" screen every app shows, and the way into the control centre.
//!
//! Task S.10 in `docs/tasks/steuerzentrale.md`, issue #410: while the core has no account
//! ([`torro_pim::AccountState::NotConnected`]) the screen names the app and offers the providers it
//! uses, one button each ([`Connect::view`]); a button reports [`Action::Add`] with its kind, and the
//! app opens its own settings at Accounts with the add-account dialog of that kind. Nothing starts
//! the control centre for it: an app works without the main application. While a sign-in runs
//! (`SigningIn`) the screen only says so. The app keeps polling the core and loads by itself once
//! the account is `Connected`.
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
    /// The button of a provider was pressed: `nextcloud`, `mail` or `github`.
    Add(String),
}

/// What the app should do after an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open the settings at Accounts with the add-account dialog of this kind of account.
    Add(String),
}

impl Connect {
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Add(kind) => Action::Add(kind),
        }
    }

    /// The screen for the current account state (`NotConnected` or `SigningIn`), with a button per
    /// kind of account in `kinds` (`nextcloud`, `mail`, `github`), the first the window's one primary
    /// button, the others secondary. A kind the screen does not know gets no button.
    ///
    /// Any state other than `SigningIn` shows the hint and the buttons, so a `Connected` account
    /// that is still on screen does not show a blank card. The card is a floating card
    /// (`style::container::popover`, as wide as one) centred on the window background: app name in
    /// `TITLE`, the hint in `text_2`.
    pub fn view<'a>(&'a self, account: &'a AccountState, app_name: &'a str, kinds: &[&str]) -> Element<'a, Message> {
        let heading = typo::text(typo::title(), app_name);

        let body: Element<'a, Message> = match account {
            AccountState::SigningIn { .. } => typo::text(typo::body(), tr("Signing in to Nextcloud …")).into(),
            _ => {
                let buttons = kinds.iter().filter_map(|kind| Some((*kind, button_label(kind)?))).enumerate().map(
                    |(at, (kind, label))| {
                        let look = if at == 0 { style::button::primary } else { style::button::secondary };
                        widgets::text_button(None, label, look, Some(Message::Add(kind.to_string())))
                    },
                );
                column![
                    typo::text(typo::body(), tr("Connect an account to see your calendars and tasks here."))
                        .style(theme::text_2),
                    row(buttons).spacing(space::s()).wrap(),
                ]
                .spacing(space::l())
                .into()
            }
        };

        let card = container(column![heading, body].spacing(space::s()).width(Fill))
            .padding(space::xl())
            .width(Fill)
            .max_width(widgets::POPOVER_WIDTH)
            .style(style::container::popover);

        center(card).padding(space::l()).into()
    }
}

/// The words of the button that adds an account of `kind`; `None` for a kind the screen does not know.
fn button_label(kind: &str) -> Option<&'static str> {
    match kind {
        "nextcloud" => Some(tr("Connect Nextcloud")),
        "mail" => Some(tr("Add mail account")),
        "github" => Some(tr("Connect GitHub")),
        _ => None,
    }
}

/// Opens the control centre on the page `section`, what `torroworkplace ui --section` takes
/// (`appearance`, `connections` …): what a settings section points at that stays the whole suite's.
/// Not waited for, failures only logged.
pub fn open_section(section: &str) {
    spawn_ui(&["ui", "--section", section]);
}

/// Opens the control centre, or brings an open one to the front: what an entry about the suite
/// links to. Not waited for, failures only logged.
pub fn open_control_centre() {
    spawn_ui(&["ui"]);
}

/// Starts the control centre with `args`: not waited for, in a process group of its own, failures
/// only logged.
fn spawn_ui(args: &[&str]) {
    let program = sibling(torro_core::AppId::WORKPLACE.binary);
    let mut command = Command::new(&program);
    command
        .args(args)
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

    /// Issue #410: a provider's button asks to add an account of its kind in the app.
    #[test]
    fn a_button_asks_to_add_its_kind() {
        let mut connect = Connect;
        assert_eq!(connect.update(Message::Add("mail".into())), Action::Add("mail".into()));
        assert!(button_label("nextcloud").is_some() && button_label("github").is_some());
        assert_eq!(button_label("slack"), None, "no button for a kind the screen does not know");
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
            let _element = connect.view(state, "Calendar", &["nextcloud", "mail"]);
            let _element = connect.view(state, "Tasks", &[]);
        }
    }
}
