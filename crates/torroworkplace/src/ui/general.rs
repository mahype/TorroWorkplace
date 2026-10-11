//! General settings (task S.9 in `docs/tasks/steuerzentrale.md`): three settings groups, each
//! with a footer sentence that explains the consequence. "Language" and "Appearance" are lists
//! of choices – one row each, the whole row is the button, a check mark at the chosen one (Deutsch
//! is shown in its own name, like the language picker of any app); "Startup" has the "Start at
//! login" switch. There is no Save button: every change sends `Request::SetSetting` with the one
//! key that changed, and the shell reloads `Data` afterwards. The page shows `data.settings`, so
//! it follows the daemon.

use iced::widget::{column, svg, toggler};
use iced::{Element, Theme};
use torro_core::i18n::tr;
use torro_core::settings::Language;
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{palette, space};
use torro_gui::{style, widgets};
use torro_ipc::{Request, SettingKey};

use super::{Action, Data, pressable, setting_row};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
    // The core takes the same names the settings file uses (`"german"`, `"dark"`, `true`), so the
    // values come from the enums' own serialisation. A value that cannot serialise (none here)
    // would become `null`, which the core refuses.
    let (key, value) = match message {
        Message::Language(language) => (SettingKey::Language, serde_json::to_value(language).unwrap_or_default()),
        Message::Appearance(appearance) => {
            (SettingKey::Appearance, serde_json::to_value(appearance).unwrap_or_default())
        }
        Message::StartAtLogin(start) => (SettingKey::StartAtLogin, serde_json::Value::Bool(start)),
    };
    Action::Send(Request::SetSetting { key, value })
}

pub fn view<'a>(_state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    let language = |label: &'static str, language: Language| {
        choice(label, settings.language == language, Message::Language(language))
    };
    let language = widgets::group(
        tr("Language"),
        vec![
            language(tr("System"), Language::System),
            language("English", Language::English),
            // Shown in its own name on purpose: someone who cannot read the current language
            // must still find their way back.
            language("Deutsch", Language::German),
        ],
        Some(tr("Applies to apps you open afterwards. “System” follows your system language.").to_string()),
    );

    // The control centre itself follows at once: it reloads the settings after every change.
    let appearance = |label: &'static str, appearance: Appearance| {
        choice(label, settings.appearance == appearance, Message::Appearance(appearance))
    };
    let appearance = widgets::group(
        tr("Appearance"),
        vec![
            appearance(tr("System"), Appearance::System),
            appearance(tr("Light"), Appearance::Light),
            appearance(tr("Dark"), Appearance::Dark),
        ],
        Some(tr("Light or dark colours for all Torro apps. “System” follows your desktop.").to_string()),
    );

    let switch =
        toggler(settings.start_at_login).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::StartAtLogin);
    let startup = widgets::group(
        tr("Startup"),
        vec![setting_row(None, tr("Start at login"), None, switch)],
        Some(
            tr("Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready.")
                .to_string(),
        ),
    );

    column![language, appearance, startup].spacing(space::XL).into()
}

/// One choice of a list: the whole row is the button; the chosen one has a check mark at the end,
/// neutral like the current item of a menu.
fn choice<'a>(label: &'a str, chosen: bool, on_press: Message) -> Element<'a, Message> {
    let check: Option<Element<'a, Message>> = chosen.then(|| {
        icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
    });
    pressable(setting_row(None, label, None, check), Some(on_press))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::settings::Settings;

    /// The key and value the page asked the shell to set.
    fn sent(action: Action) -> (SettingKey, serde_json::Value) {
        let Action::Send(Request::SetSetting { key, value }) = action else {
            panic!("expected SetSetting, got {action:?}");
        };
        (key, value)
    }

    #[test]
    fn each_language_choice_sets_only_the_language() {
        let cases = [(Language::System, "system"), (Language::English, "english"), (Language::German, "german")];
        for (language, name) in cases {
            let action = update(&mut State::default(), Message::Language(language), &Data::default());
            assert_eq!(sent(action), (SettingKey::Language, serde_json::json!(name)), "choice {language:?}");
        }
    }

    #[test]
    fn each_appearance_choice_sets_only_the_appearance() {
        let cases = [(Appearance::System, "system"), (Appearance::Light, "light"), (Appearance::Dark, "dark")];
        for (appearance, name) in cases {
            let action = update(&mut State::default(), Message::Appearance(appearance), &Data::default());
            assert_eq!(sent(action), (SettingKey::Appearance, serde_json::json!(name)), "choice {appearance:?}");
        }
    }

    #[test]
    fn start_at_login_sets_only_the_switch() {
        let on = update(&mut State::default(), Message::StartAtLogin(true), &Data::default());
        assert_eq!(sent(on), (SettingKey::StartAtLogin, serde_json::json!(true)));
        let off = update(&mut State::default(), Message::StartAtLogin(false), &Data::default());
        assert_eq!(sent(off), (SettingKey::StartAtLogin, serde_json::json!(false)));
    }

    #[test]
    fn page_builds_with_default_and_stored_settings() {
        let stored = Data {
            settings: Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: false,
                hidden_collections: vec!["work".into()],
            },
            ..Data::default()
        };
        let (state, default) = (State::default(), Data::default());
        let _default: Element<'_, Message> = view(&state, &default);
        let _stored: Element<'_, Message> = view(&state, &stored);
    }
}
