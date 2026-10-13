//! One mail server as a person types it (task M3.5: one place for what the add-account dialog,
//! `wizard.rs`, and the "Server settings" dialog of a mail account's page, `accounts.rs`, share):
//! host, port and security ([`ServerFields`]), the usual ports ([`Server::default_port`]), and the
//! group that shows them ([`server_group`]). The port stays text until it is sent, so a half-typed
//! port is no error while typing.

use iced::Element;
use iced::Length::FillPortion;
use iced::widget::row;
use torro_core::i18n::tr;
use torro_gui::theme::space;
use torro_gui::widgets;
use torro_mail::{Endpoint, Security};

use super::{row_frame, setting_row, wizard};

/// The two mail servers of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    /// Incoming mail.
    Imap,
    /// Outgoing mail (submission).
    Smtp,
}

impl Server {
    /// The usual port for `security`: IMAP 993 (TLS) or 143 (STARTTLS), SMTP 465 or 587.
    pub(super) fn default_port(self, security: Security) -> u16 {
        match (self, security) {
            (Server::Imap, Security::Tls) => 993,
            (Server::Imap, Security::StartTls) => 143,
            (Server::Smtp, Security::Tls) => 465,
            (Server::Smtp, Security::StartTls) => 587,
        }
    }

    /// What most providers use when the search found nothing: IMAP over TLS, SMTP with STARTTLS.
    fn default_security(self) -> Security {
        match self {
            Server::Imap => Security::Tls,
            Server::Smtp => Security::StartTls,
        }
    }
}

/// One mail server as typed: the port stays text until it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServerFields {
    pub(super) host: String,
    pub(super) port: String,
    pub(super) security: Security,
}

impl ServerFields {
    /// No host yet, with the usual port and security of `server`.
    pub(super) fn empty(server: Server) -> ServerFields {
        let security = server.default_security();
        ServerFields { host: String::new(), port: server.default_port(security).to_string(), security }
    }

    /// The fields of a server as it is stored or the search found it.
    pub(super) fn of(endpoint: &Endpoint) -> ServerFields {
        ServerFields { host: endpoint.host.clone(), port: endpoint.port.to_string(), security: endpoint.security }
    }

    /// Nothing was entered for this server.
    pub(super) fn is_blank(&self) -> bool {
        self.host.trim().is_empty()
    }

    /// The server as the request wants it; `None` without a host or with a port that is no port.
    pub(super) fn endpoint(&self) -> Option<Endpoint> {
        let host = self.host.trim();
        let port = self.port.trim().parse::<u16>().ok().filter(|port| *port > 0)?;
        (!host.is_empty()).then(|| Endpoint { host: host.to_string(), port, security: self.security })
    }

    /// Another security: a port that was the usual one of the old security follows to the usual
    /// one of the new, so switching to STARTTLS does not leave port 993 behind.
    pub(super) fn set_security(&mut self, server: Server, security: Security) {
        if self.port.trim() == server.default_port(self.security).to_string() {
            self.port = server.default_port(security).to_string();
        }
        self.security = security;
    }
}

/// How the width of the host and port fields relate: a port is a few digits, a host a name.
const HOST_SHARE: u16 = 3;
const PORT_SHARE: u16 = 1;

/// One mail server as a settings group titled `title`: host and port in one row, the security
/// below. Typing sends `on_host` / `on_port`, a security `on_security`; Enter in a field sends
/// `submit` when there is one (Continue or Save of the dialog).
pub(super) fn server_group<'a, M: Clone + 'a>(
    fields: &'a ServerFields,
    (title, host_placeholder): (&'static str, &'static str),
    on_host: impl Fn(String) -> M + 'a,
    on_port: impl Fn(String) -> M + 'a,
    on_security: impl Fn(Security) -> M,
    submit: Option<M>,
) -> Element<'a, M> {
    let host = wizard::field(host_placeholder, &fields.host)
        .on_input(on_host)
        .on_submit_maybe(submit.clone())
        .width(FillPortion(HOST_SHARE));
    let port = wizard::field(tr("Port"), &fields.port)
        .on_input(on_port)
        .on_submit_maybe(submit)
        .width(FillPortion(PORT_SHARE));
    // TLS and STARTTLS are the protocols' names, the same in every language.
    let security = widgets::segmented(
        [("TLS", on_security(Security::Tls)), ("STARTTLS", on_security(Security::StartTls))],
        usize::from(fields.security == Security::StartTls),
    );
    let rows =
        vec![row_frame(row![host, port].spacing(space::s()), false), setting_row(None, tr("Security"), None, security)];
    widgets::group(title, rows, None)
}

/// The group of the IMAP server ([`server_group`] with its title and placeholder).
pub(super) fn imap_titles() -> (&'static str, &'static str) {
    (tr("Incoming mail (IMAP)"), "imap.example.org")
}

/// The group of the SMTP server ([`server_group`] with its title and placeholder).
pub(super) fn smtp_titles() -> (&'static str, &'static str) {
    (tr("Outgoing mail (SMTP)"), "smtp.example.org")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usual_port_follows_the_security_and_a_typed_one_stays() {
        let mut imap = ServerFields::empty(Server::Imap);
        assert_eq!((imap.port.as_str(), imap.security), ("993", Security::Tls));
        assert!(imap.is_blank() && imap.endpoint().is_none(), "no host yet");
        imap.set_security(Server::Imap, Security::StartTls);
        assert_eq!(imap.port, "143");
        let mut smtp = ServerFields::empty(Server::Smtp);
        assert_eq!((smtp.port.as_str(), smtp.security), ("587", Security::StartTls));
        smtp.set_security(Server::Smtp, Security::Tls);
        assert_eq!(smtp.port, "465");
        smtp.port = "2525".into();
        smtp.set_security(Server::Smtp, Security::StartTls);
        assert_eq!(smtp.port, "2525", "a port the person typed stays");

        smtp.host = " smtp.example.org ".into();
        let endpoint = smtp.endpoint().expect("a server");
        assert_eq!((endpoint.host.as_str(), endpoint.port), ("smtp.example.org", 2525));
        for port in ["0", "70000", "x", ""] {
            smtp.port = port.into();
            assert!(smtp.endpoint().is_none(), "{port:?}");
        }
        assert_eq!(ServerFields::of(&endpoint).endpoint(), Some(endpoint));
    }
}
