//! Accounts and AI of the control centre (tasks S.5, S.30, issue #325): the pages themselves live in
//! `torro_settings_ui::accounts` since issue #410, so every app shows the same ones behind its gear.
//! Here they get the control centre's data ([`Data`] as their [`Seen`]) and their actions become the
//! shell's ([`act`]); everything else is theirs as it was.
//!
//! "Used by …" on an account's page names the installed apps that use it: here what the core lists
//! in `apps.list_apps` ([`Data::apps`]), every app while that answer has not come.

use torro_core::AppId;
use torro_core::settings::Settings;
use torro_gui::layout::SizeClass;
use torro_inbox::SourceInfo;
use torro_ipc::AccountInfo;
pub use torro_settings_ui::accounts::*;

use super::{Data, Header};

impl Seen for Data {
    fn accounts(&self) -> &[AccountInfo] {
        &self.accounts
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }

    fn sources(&self) -> &[SourceInfo] {
        &self.sources
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn installed(&self, app: AppId) -> bool {
        match &self.apps {
            Some(apps) => apps.iter().any(|known| known.installed && known.window_class == app.window_class),
            None => true,
        }
    }
}

/// What a page of the accounts, the add-account dialog or a sign-in wants, as the shell's action.
pub fn act(action: Action) -> super::Action {
    match action {
        Action::None => super::Action::None,
        Action::Send(request) => super::Action::Send(request),
        Action::AskAi(asks) => super::Action::AskAi(asks),
        Action::OpenUrl(url) => super::Action::OpenUrl(url),
        Action::Focus(field) => super::Action::Focus(field.into()),
        // Only an app window's AI section opens the control centre (issue #547); this is it.
        Action::ControlCentre(_) => super::Action::None,
    }
}

/// The header bar of the page: on an account's detail its label with a back button to the list
/// ([`sub_page`]); no button of its own since task S.30.
pub fn header<'a>(state: &'a State, data: &'a Data, _class: SizeClass) -> Header<'a, Message> {
    Header { sub_page: sub_page(state, data), actions: Vec::new() }
}
