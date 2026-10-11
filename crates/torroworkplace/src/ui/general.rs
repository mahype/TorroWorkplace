//! General settings (task S.9 in `docs/tasks/steuerzentrale.md`): two groups, each a card with
//! a radio list or a switch and a footer sentence that explains the consequence. "Language"
//! offers System / English / Deutsch (Deutsch is shown in its own name, like the language
//! picker of any app); "Startup" has "Start at login". There is no Save button: every change
//! sends `Request::SetSetting` with the one key that changed, and the shell reloads `Data`
//! afterwards. The page shows `data.settings`, so it follows the daemon.

use iced::widget::{column, container, radio, row, space, text, toggler};
use iced::{Alignment, Background, Element, Length, border};
use torro_core::i18n::tr;
use torro_core::settings::Language;
use torro_core::theme::Appearance;
use torro_gui::{theme, widgets};
use torro_ipc::{Request, SettingKey};

use super::{Action, Data};

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

    let language = row_group(
        tr("Language"),
        column![
            radio(tr("System"), Language::System, Some(settings.language), Message::Language).text_size(14),
            radio("English", Language::English, Some(settings.language), Message::Language).text_size(14),
            // Shown in its own name on purpose: someone who cannot read the current language
            // must still find their way back.
            radio("Deutsch", Language::German, Some(settings.language), Message::Language).text_size(14),
        ]
        .spacing(8)
        .into(),
        tr("Applies to apps you open afterwards. “System” follows your system language."),
    );

    // The control centre itself follows at once: it reloads the settings after every change.
    let appearance = row_group(
        tr("Appearance"),
        column![
            radio(tr("System"), Appearance::System, Some(settings.appearance), Message::Appearance).text_size(14),
            radio(tr("Light"), Appearance::Light, Some(settings.appearance), Message::Appearance).text_size(14),
            radio(tr("Dark"), Appearance::Dark, Some(settings.appearance), Message::Appearance).text_size(14),
        ]
        .spacing(8)
        .into(),
        tr("Light or dark colours for all Torro apps. “System” follows your desktop."),
    );

    let startup = row_group(
        tr("Startup"),
        row![
            text(tr("Start at login")).size(14),
            space::horizontal(),
            toggler(settings.start_at_login).on_toggle(Message::StartAtLogin),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into(),
        tr("Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready."),
    );

    column![language, appearance, startup].spacing(24).max_width(640.0).into()
}

/// A group: a small heading in capitals, a card with the rows, and a footer sentence in muted
/// text below. Spacing inside the group is 8, between groups 24.
fn row_group<'a>(title: &'static str, rows: Element<'a, Message>, footer: &'static str) -> Element<'a, Message> {
    let card = container(rows).padding(16).width(Length::Fill).style(|_| container::Style {
        background: Some(Background::Color(theme::SURFACE)),
        border: border::rounded(12),
        ..container::Style::default()
    });
    column![widgets::section_title(title), card, text(footer).size(14).color(theme::MUTED)].spacing(8).into()
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
