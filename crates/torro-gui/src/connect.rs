//! The "connect your Nextcloud" screen.
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md`. Every app shows it while the core has no
//! account ([`torro_pim::AccountState::NotConnected`]) or a sign-in is running
//! (`SigningIn`). The user types the server address and presses "Connect to Nextcloud";
//! the app calls `PimSource::connect_nextcloud`, the core opens the browser for Login Flow v2,
//! and the screen shows "Sign in in your browser …" with the login URL as a fallback link until
//! the account state turns `Connected`.
//!
//! UI text is English (the base language); German comes later through a translation table.

use iced::font::Weight;
use iced::widget::{button, center, column, container, text, text_input};
use iced::{Element, Fill, Font, border};

use torro_core::theme::STATUS_ERROR;
use torro_pim::AccountState;

use crate::theme;

/// State of the screen.
#[derive(Debug, Clone, Default)]
pub struct Connect {
    pub server: String,
    /// Shown below the button: errors, "Sign in in your browser …".
    pub status: Option<String>,
    /// A connect request is on its way.
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    ServerChanged(String),
    /// The user pressed the button or Enter.
    Submit,
}

/// What the app should do after an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    /// Call `connect_nextcloud(server)`.
    Connect(String),
}

const TAGLINE: &str = "Sign in with your Nextcloud to see events and tasks.";
const EMPTY_SERVER: &str = "Please enter the address of your Nextcloud";

impl Connect {
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::ServerChanged(server) => {
                self.server = server;
                self.status = None;
                Action::None
            }
            Message::Submit => {
                let server = self.server.trim();
                if server.is_empty() {
                    self.status = Some(EMPTY_SERVER.to_string());
                    return Action::None;
                }
                let server = server.to_string();
                self.busy = true;
                self.status = None;
                Action::Connect(server)
            }
        }
    }

    /// The screen for the current account state (`NotConnected` or `SigningIn`).
    ///
    /// Any state other than `SigningIn` shows the form, so a `Connected` account that is
    /// still on screen does not show a blank card.
    pub fn view<'a>(&'a self, account: &'a AccountState, app_name: &'a str) -> Element<'a, Message> {
        let heading = text(app_name).size(26).font(Font { weight: Weight::Bold, ..Font::DEFAULT }).color(theme::TEXT);

        let body: Element<'a, Message> = match account {
            AccountState::SigningIn { login_url, .. } => column![
                text("Sign in in your browser …").size(16).color(theme::TEXT),
                text("This window updates itself once you have signed in.").size(14).color(theme::MUTED),
                text("If the browser does not open, copy this link:").size(14).color(theme::MUTED),
                // A read-only input: it cannot be edited, but the text can be selected and copied.
                text_input("", login_url).size(13).width(Fill).style(login_url_style),
            ]
            .spacing(12)
            .into(),
            _ => self.form(),
        };

        let card = container(column![heading, body].spacing(16).width(Fill))
            .padding(28)
            .width(Fill)
            .max_width(420)
            .style(card_style);

        center(card).padding(16).into()
    }

    /// The server input, the connect button and the status line.
    fn form(&self) -> Element<'_, Message> {
        let can_submit = !self.busy;
        let mut form = column![
            text(TAGLINE).size(15).color(theme::MUTED),
            text_input("cloud.example.org", &self.server)
                .on_input(Message::ServerChanged)
                .on_submit_maybe(can_submit.then_some(Message::Submit))
                .padding(12)
                .size(16)
                .width(Fill),
            button(text("Connect to Nextcloud").width(Fill).align_x(iced::Center))
                .style(button::primary)
                .padding([10, 16])
                .width(Fill)
                .on_press_maybe(can_submit.then_some(Message::Submit)),
        ]
        .spacing(14);

        if let Some(status) = &self.status {
            form = form.push(text(status.as_str()).size(14).color(theme::color(STATUS_ERROR)));
        }
        form.into()
    }
}

/// The card the form sits on.
fn card_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        text_color: Some(theme::TEXT),
        background: Some(iced::Background::Color(theme::SURFACE)),
        border: border::rounded(12).color(theme::LINE).width(1),
        ..Default::default()
    }
}

/// The login URL field: looks like plain text, not like a disabled input.
fn login_url_style(theme: &iced::Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style { value: theme::TEXT, ..text_input::default(theme, text_input::Status::Active) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(server: &str) -> Connect {
        let mut connect = Connect::default();
        connect.update(Message::ServerChanged(server.to_string()));
        connect
    }

    #[test]
    fn server_changed_sets_server_and_clears_status() {
        let mut connect = Connect { status: Some("alt".into()), ..Default::default() };
        assert_eq!(connect.update(Message::ServerChanged("cloud.example.org".into())), Action::None);
        assert_eq!(connect.server, "cloud.example.org");
        assert_eq!(connect.status, None);
        assert!(!connect.busy);
    }

    #[test]
    fn submit_with_empty_server_sets_status() {
        let mut connect = Connect::default();
        assert_eq!(connect.update(Message::Submit), Action::None);
        assert_eq!(connect.status.as_deref(), Some("Please enter the address of your Nextcloud"));
        assert!(!connect.busy);
    }

    #[test]
    fn submit_with_only_spaces_counts_as_empty() {
        let mut connect = typed("   \t ");
        assert_eq!(connect.update(Message::Submit), Action::None);
        assert!(connect.status.is_some());
        assert!(!connect.busy);
    }

    #[test]
    fn submit_connects_with_trimmed_server() {
        let mut connect = typed("  cloud.example.org \n");
        connect.status = Some("alt".into());
        assert_eq!(connect.update(Message::Submit), Action::Connect("cloud.example.org".into()));
        assert!(connect.busy);
        assert_eq!(connect.status, None);
        assert_eq!(connect.server, "  cloud.example.org \n", "the field keeps what the user typed");
    }

    #[test]
    fn view_builds_for_each_account_state() {
        let connect = typed("cloud.example.org");
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
