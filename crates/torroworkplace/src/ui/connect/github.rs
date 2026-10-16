//! Signing in to GitHub (task W.6): the two ways without an OAuth app of our own – take over the
//! GitHub CLI's sign-in, or paste a token – and the check that follows. The add-account dialog
//! (`ui/wizard.rs`) and the setup's tasks step (`ui/setup`) both use it.
//!
//! Pressing "Use GitHub CLI sign-in" is the one confirmation: the core runs `gh auth token`, never
//! this window. A pasted token is a [`Secret`] from the field to the request, so no `Debug` shows it.

use iced::Element;
use iced::widget::{column, row};
use torro_core::account::Secret;
use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::theme::{self, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{Request, Response};

use super::super::wizard::field;
use super::super::{Action, chevron, pressable, setting_row};

/// GitHub's page for a new classic token, with the scope the suite needs (`repo`: private
/// repositories, closing issues) and a name already filled in.
pub const NEW_TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo&description=Torro%20Workplace";

/// A GitHub sign-in: the token as pasted, and whether the core is checking one.
#[derive(Debug, Default)]
pub struct GithubSignIn {
    /// The token as pasted; `Debug` never shows it.
    pub token: Secret,
    checking: bool,
}

impl GithubSignIn {
    /// The core checks a sign-in; the way's buttons wait.
    pub fn checking(&self) -> bool {
        self.checking
    }

    /// Whether the pasted token can be sent.
    pub fn has_token(&self) -> bool {
        !self.token.is_blank()
    }

    /// Takes over the GitHub CLI's sign-in.
    pub fn use_cli(&mut self) -> Action {
        self.checking = true;
        Action::Send(Request::ConnectGithub)
    }

    /// Sends the pasted token; the field keeps no copy.
    pub fn send_token(&mut self) -> Action {
        self.checking = true;
        Action::Send(Request::SetGithubToken { token: std::mem::take(&mut self.token) })
    }

    /// Stops waiting for the check (the person went elsewhere); a late answer is not taken.
    pub fn stop(&mut self) {
        self.checking = false;
    }

    /// The answer to the check while one runs: the GitHub login, or why it failed. `None` when no
    /// check runs, so the answer is not the sign-in's.
    pub fn answered(&mut self, result: &Result<Response, String>) -> Option<Result<String, String>> {
        if !self.checking {
            return None;
        }
        self.checking = false;
        Some(match result {
            Ok(Response::Account { account }) => Ok(account.label.clone()),
            Ok(Response::Error { message, .. }) | Err(message) => Err(message.clone()),
            Ok(_) => Err(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
        })
    }
}

/// GitHub's two ways in, as rows to press under `title`, and where the token stays.
pub fn ways<'a, M: Clone + 'a>(title: &'a str, on_cli: M, on_paste: M) -> Element<'a, M> {
    let way = |name: &'static str, summary: &'static str, message| {
        pressable(setting_row(None, name, Some(summary.to_string()), chevron()), Some(message))
    };
    let rows = vec![
        way(tr("Use GitHub CLI sign-in"), tr("Uses the sign-in of the gh command."), on_cli),
        way(tr("Paste a token"), tr("A personal access token from your GitHub settings."), on_paste),
    ];
    let footer = tr("The token stays in the system keychain. Agents never see it.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// What the token needs, the way to GitHub's token page (`on_open`), and the masked field; Enter
/// sends `on_submit` once a token is there.
pub fn token_form<'a, M: Clone + 'a>(
    sign_in: &'a GithubSignIn,
    on_input: fn(String) -> M,
    on_submit: M,
    on_open: M,
) -> Element<'a, M> {
    let create = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Create a token on GitHub"),
        style::button::secondary,
        Some(on_open),
    );
    let input = field("ghp_… / github_pat_…", sign_in.token.expose())
        .secure(true)
        .on_input(on_input)
        .on_submit_maybe(sign_in.has_token().then_some(on_submit));
    column![
        typo::text(
            typo::body(),
            tr("Reading private repositories and closing issues needs the repo scope; GitHub's page has it selected.")
        )
        .style(theme::text_2),
        row![create],
        input,
    ]
    .spacing(space::m())
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::AccountInfo;
    use torro_pim::AccountState;

    fn account(login: &str) -> AccountInfo {
        AccountInfo {
            id: format!("github:{login}"),
            kind: "github".into(),
            label: login.into(),
            state: AccountState::NotConnected,
            collections: vec![],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        }
    }

    #[test]
    fn the_cli_and_a_token_are_checked_and_end_with_the_login() {
        let mut sign_in = GithubSignIn::default();
        assert!(matches!(sign_in.use_cli(), Action::Send(Request::ConnectGithub)));
        assert!(sign_in.checking());
        let ok = Ok(Response::Account { account: account("anna-berg") });
        assert_eq!(sign_in.answered(&ok), Some(Ok("anna-berg".into())));
        assert_eq!(sign_in.answered(&ok), None, "nothing runs any more");

        sign_in.token = Secret::new("ghp_x");
        assert!(sign_in.has_token());
        match sign_in.send_token() {
            Action::Send(Request::SetGithubToken { token }) => assert_eq!(token.expose(), "ghp_x"),
            other => panic!("{other:?}"),
        }
        assert!(!sign_in.has_token(), "the field keeps no copy");
        assert_eq!(sign_in.answered(&Err("refused".into())), Some(Err("refused".into())));
    }

    #[test]
    fn a_stopped_check_takes_no_answer() {
        let mut sign_in = GithubSignIn::default();
        sign_in.use_cli();
        sign_in.stop();
        assert_eq!(sign_in.answered(&Err("late".into())), None);
    }
}
