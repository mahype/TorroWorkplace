//! The "connect your Nextcloud" screen.
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md`. Every app shows it while the core has no
//! account ([`torro_pim::AccountState::NotConnected`]) or a sign-in is running
//! (`SigningIn`). The user types the server address and presses "Mit Nextcloud verbinden";
//! the app calls `PimSource::connect_nextcloud`, the core opens the browser for Login Flow v2,
//! and the screen shows "Im Browser anmelden …" with the login URL as a fallback link until
//! the account state turns `Connected`.

use iced::Element;
use iced::widget::text;
use torro_pim::AccountState;

/// State of the screen.
#[derive(Debug, Clone, Default)]
pub struct Connect {
    pub server: String,
    /// Shown below the button: errors, "Im Browser anmelden …".
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

impl Connect {
    pub fn update(&mut self, message: Message) -> Action {
        let _ = message;
        Action::None
    }

    /// The screen for the current account state (`NotConnected` or `SigningIn`).
    pub fn view<'a>(&'a self, account: &'a AccountState, app_name: &'a str) -> Element<'a, Message> {
        let _ = (account, app_name);
        text("Mit Nextcloud verbinden").into()
    }
}
