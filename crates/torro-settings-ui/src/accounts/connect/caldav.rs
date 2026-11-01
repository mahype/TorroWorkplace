//! Connecting a CalDAV account with a password (task W.6): the calendars and task lists of
//! mailbox.org, Posteo, Fastmail, iCloud or any other CalDAV server, for the setup's calendar and
//! tasks steps.
//!
//! The e-mail address is enough for a known provider: the core finds the server from its domain
//! (`accounts.connect_caldav` without `server`). "Advanced" opens the server address and the user
//! name for a server the core cannot find, or a sign-in that is not the address. The password is a
//! [`Secret`] from the field to the request, so no `Debug` shows it.
//!
//! The add-account dialog connects CalDAV on its mail path (address, search, password, services);
//! this block is the short way for those who only want calendars and tasks.

use iced::Element;
use iced::widget::{button, column, row};
use torro_core::account::Secret;
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, space};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response};

use super::super::Action;
use super::super::wizard::field;

/// A CalDAV sign-in being typed, and whether the core is connecting it.
#[derive(Debug, Default)]
pub struct CalDavSignIn {
    /// The e-mail address of the account.
    pub address: String,
    /// The password or app password; `Debug` never shows it.
    pub password: Secret,
    /// "Advanced": the CalDAV address, empty to find it from the address.
    pub server: String,
    /// "Advanced": the user name, empty for the e-mail address.
    pub login: String,
    /// "Advanced" is open.
    pub advanced: bool,
    connecting: bool,
}

/// What the form's widgets report.
#[derive(Debug, Clone)]
pub enum Message {
    Address(String),
    /// A [`Secret`], so that the derived `Debug` of the message shows no password.
    Password(Secret),
    Server(String),
    Login(String),
    ToggleAdvanced,
}

impl CalDavSignIn {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Address(address) => self.address = address,
            Message::Password(password) => self.password = password,
            Message::Server(server) => self.server = server,
            Message::Login(login) => self.login = login,
            Message::ToggleAdvanced => self.advanced = !self.advanced,
        }
    }

    /// The core is connecting the account.
    pub fn connecting(&self) -> bool {
        self.connecting
    }

    /// Whether Connect can be pressed: an e-mail address (something before and after an `@`) and a
    /// password, and nothing being connected.
    pub fn can_connect(&self) -> bool {
        let address = self.address.trim();
        let valid = address.split_once('@').is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.'));
        valid && !self.password.is_blank() && !self.connecting
    }

    /// Connects the account and shows its `calendars` and `tasks`; the form keeps no copy of the
    /// password.
    pub fn connect(&mut self, calendars: bool, tasks: bool) -> Action {
        self.connecting = true;
        let address = self.address.trim().to_string();
        let server = Some(self.server.trim()).filter(|server| !server.is_empty()).map(str::to_string);
        let login = self.login.trim();
        let login_name = if login.is_empty() { address.clone() } else { login.to_string() };
        let password = std::mem::take(&mut self.password);
        Action::Send(Request::ConnectCalDav { address, server, login_name, password, calendars, tasks })
    }

    /// The answer while connecting: the account's name, or why it failed. `None` when nothing is
    /// being connected, so the answer is not the form's.
    pub fn answered(&mut self, result: &Result<Response, String>) -> Option<Result<String, String>> {
        if !self.connecting {
            return None;
        }
        self.connecting = false;
        Some(match result {
            Ok(Response::Connected { account, .. }) => Ok(account.label.clone()),
            Ok(Response::Error { message, .. }) | Err(message) => Err(message.clone()),
            Ok(_) => Err(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
        })
    }

    /// The form: address, password, the note on app passwords, and "Advanced" with server and user
    /// name. Enter in a field sends `on_submit` once the form can connect.
    pub fn view<'a, M: Clone + 'a>(&'a self, wrap: fn(Message) -> M, on_submit: M) -> Element<'a, M> {
        let submit = self.can_connect().then_some(on_submit);
        let labelled = |label: &'static str, input: Element<'a, M>| -> Element<'a, M> {
            column![typo::text(typo::caption(), label).style(theme::text_2), input].spacing(space::xs()).into()
        };
        let address = field("anna@mailbox.org", &self.address)
            .on_input(move |text| wrap(Message::Address(text)))
            .on_submit_maybe(submit.clone());
        let password = field("", self.password.expose())
            .secure(true)
            .on_input(move |text| wrap(Message::Password(Secret::new(text))))
            .on_submit_maybe(submit.clone());
        let mut form = column![
            labelled(tr("E-mail address"), address.into()),
            labelled(tr("Password"), password.into()),
            typo::text(
                typo::meta(),
                tr("iCloud, Fastmail and a few others need an app password instead of your usual one.")
            )
            .style(theme::text_2),
            advanced_toggle(self.advanced, wrap(Message::ToggleAdvanced)),
        ]
        .spacing(space::m());
        if self.advanced {
            let server = field("https://dav.example.org", &self.server)
                .on_input(move |text| wrap(Message::Server(text)))
                .on_submit_maybe(submit.clone());
            let login = field(tr("The e-mail address"), &self.login)
                .on_input(move |text| wrap(Message::Login(text)))
                .on_submit_maybe(submit);
            form = form
                .push(labelled(tr("Server address"), server.into()))
                .push(labelled(tr("User name"), login.into()))
                .push(
                    typo::text(typo::meta(), tr("Leave the server empty to find it from the address."))
                        .style(theme::text_2),
                );
        }
        form.into()
    }
}

/// "Advanced" with a chevron that points down while it is open.
fn advanced_toggle<'a, M: Clone + 'a>(open: bool, on_press: M) -> Element<'a, M> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let content = row![icon::icon(chevron, icon::SIZE_SM), typo::text(typo::caption(), tr("Advanced"))]
        .spacing(space::xs())
        .align_y(iced::Alignment::Center);
    button(content).padding(style::button_padding()).style(style::button::ghost).on_press(on_press).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::AccountInfo;
    use torro_pim::AccountState;

    fn typed(address: &str, password: &str) -> CalDavSignIn {
        CalDavSignIn { address: address.into(), password: Secret::new(password), ..CalDavSignIn::default() }
    }

    #[test]
    fn an_address_and_a_password_are_enough() {
        assert!(typed(" anna@mailbox.org ", "secret").can_connect());
        for (address, password) in [("anna@mailbox.org", ""), ("anna", "secret"), ("@mailbox.org", "x"), ("a@b", "x")] {
            assert!(!typed(address, password).can_connect(), "{address:?} {password:?}");
        }
    }

    #[test]
    fn the_server_is_found_from_the_address_unless_typed() {
        let mut sign_in = typed("anna@mailbox.org", "secret");
        match sign_in.connect(true, false) {
            Action::Send(Request::ConnectCalDav { address, server, login_name, password, calendars, tasks }) => {
                assert_eq!(
                    (address.as_str(), server, login_name.as_str()),
                    ("anna@mailbox.org", None, "anna@mailbox.org")
                );
                assert_eq!((password.expose(), calendars, tasks), ("secret", true, false));
            }
            other => panic!("{other:?}"),
        }
        assert!(sign_in.password.is_blank() && sign_in.connecting() && !sign_in.can_connect());

        let mut own = typed("anna@example.org", "secret");
        own.update(Message::Server(" https://dav.example.org ".into()));
        own.update(Message::Login("anna".into()));
        match own.connect(true, true) {
            Action::Send(Request::ConnectCalDav { server, login_name, .. }) => {
                assert_eq!((server.as_deref(), login_name.as_str()), (Some("https://dav.example.org"), "anna"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_answer_ends_connecting() {
        let mut sign_in = typed("anna@mailbox.org", "secret");
        assert_eq!(sign_in.answered(&Err("x".into())), None, "nothing being connected");
        sign_in.connect(true, true);
        let account = AccountInfo {
            id: "caldav:anna@dav.mailbox.org".into(),
            kind: "caldav".into(),
            label: "anna@mailbox.org".into(),
            state: AccountState::NotConnected,
            collections: vec![],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        };
        let connected = Ok(Response::Connected { account, calendars: true, tasks: false });
        assert_eq!(sign_in.answered(&connected), Some(Ok("anna@mailbox.org".into())));
        sign_in.password = Secret::new("wrong");
        sign_in.connect(true, true);
        assert_eq!(sign_in.answered(&Err("Password was refused".into())), Some(Err("Password was refused".into())));
        assert!(!sign_in.connecting());
    }
}
