//! General settings (task S.9 in `docs/tasks/steuerzentrale.md`): two groups, each a card with
//! a radio list or a switch and a footer sentence that explains the consequence. "Language"
//! offers System / English / Deutsch (Deutsch is shown in its own name, like the language
//! picker of any app); "Startup" has "Start at login". There is no Save button: every change
//! sends `Request::UpdateSettings` with the whole `Settings`, the changed field only, and the
//! shell reloads `Data` afterwards. The page shows `data.settings`, so it follows the daemon.

use iced::widget::{column, container, radio, row, space, text, toggler};
use iced::{Alignment, Background, Element, Length, border};
use torro_core::i18n::tr;
use torro_core::settings::Language;
use torro_core::theme::Appearance;
use torro_gui::{theme, widgets};
use torro_ipc::Request;

use super::{Action, Data};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
}

pub fn update(_state: &mut State, message: Message, data: &Data) -> Action {
    // Autosave: copy the stored settings, change the one field, send the whole thing back.
    let mut settings = data.settings.clone();
    match message {
        Message::Language(language) => settings.language = language,
        Message::Appearance(appearance) => settings.appearance = appearance,
        Message::StartAtLogin(start) => settings.start_at_login = start,
    }
    Action::Send(Request::UpdateSettings { settings })
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

    /// Settings that differ from the defaults in every field, so a change to any other field
    /// would show up in the comparison.
    fn data() -> Data {
        Data {
            settings: Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: false,
                hidden_collections: vec!["work".into()],
            },
            ..Data::default()
        }
    }

    /// The settings the page asked the shell to save.
    fn sent(action: Action) -> Settings {
        let Action::Send(Request::UpdateSettings { settings }) = action else {
            panic!("expected UpdateSettings, got {action:?}");
        };
        settings
    }

    #[test]
    fn each_language_choice_sends_only_the_language() {
        let data = data();
        for language in [Language::System, Language::English, Language::German] {
            let action = update(&mut State::default(), Message::Language(language), &data);
            let mut expected = data.settings.clone();
            expected.language = language;
            assert_eq!(sent(action), expected, "choice {language:?}");
        }
    }

    #[test]
    fn each_appearance_choice_sends_only_the_appearance() {
        let data = data();
        for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
            let action = update(&mut State::default(), Message::Appearance(appearance), &data);
            let mut expected = data.settings.clone();
            expected.appearance = appearance;
            assert_eq!(sent(action), expected, "choice {appearance:?}");
        }
    }

    #[test]
    fn start_at_login_sends_only_the_switch() {
        let data = data();
        let action = update(&mut State::default(), Message::StartAtLogin(true), &data);
        let mut expected = data.settings.clone();
        expected.start_at_login = true;
        assert_eq!(sent(action), expected);
    }

    #[test]
    fn start_at_login_off_sends_false() {
        let mut data = data();
        data.settings.start_at_login = true;
        let action = update(&mut State::default(), Message::StartAtLogin(false), &data);
        assert!(!sent(action).start_at_login);
    }

    #[test]
    fn page_builds_with_default_and_stored_settings() {
        let (state, default, stored) = (State::default(), Data::default(), data());
        let _default: Element<'_, Message> = view(&state, &default);
        let _stored: Element<'_, Message> = view(&state, &stored);
    }
}
