//! General settings (task S.9 in `docs/tasks/steuerzentrale.md`): settings groups, each with a
//! footer sentence that explains the consequence. "Language" and "Appearance" are lists of choices
//! – one row each, the whole row is the button, a check mark at the chosen one (Deutsch is shown in
//! its own name, like the language picker of any app); "Interface" has the zoom of the suite with
//! −, the value, + and "Reset" (task Z.1); "Calendar" has the night with its two times "From" and
//! "To" on the quarter hour (task K.27); "Startup" has the "Start at login" switch. There is no
//! Save button: every change sends `Request::SetSetting` with the one key that changed, and the
//! shell reloads `Data` afterwards. The zoom goes through the window's own zoom instead
//! (`Action::Zoom`), which saves it and applies it at once, also in the demo. The page shows
//! `data.settings`, so it follows the daemon.

use iced::widget::{column, container, pick_list, row, svg, toggler};
use iced::{Alignment, Element, Theme};
use torro_core::i18n::tr;
use torro_core::settings::{Language, Night, Quarter, Zoom};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{palette, space};
use torro_gui::zoom::{self, Step};
use torro_gui::{style, typo, widgets};
use torro_ipc::{Request, SettingKey};

use super::{Action, Data, pressable, setting_row};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    /// −, + or "Reset" of the zoom.
    Zoom(Step),
    /// The night's start, picked in "From".
    NightFrom(Quarter),
    /// The night's end, picked in "To".
    NightTo(Quarter),
}

pub fn update(_state: &mut State, message: Message, data: &Data) -> Action {
    // The core takes the same names the settings file uses (`"german"`, `"dark"`, `true`), so the
    // values come from the enums' own serialisation. A value that cannot serialise (none here)
    // would become `null`, which the core refuses.
    let (key, value) = match message {
        Message::Language(language) => (SettingKey::Language, serde_json::to_value(language).unwrap_or_default()),
        Message::Appearance(appearance) => {
            (SettingKey::Appearance, serde_json::to_value(appearance).unwrap_or_default())
        }
        Message::StartAtLogin(start) => (SettingKey::StartAtLogin, serde_json::Value::Bool(start)),
        Message::Zoom(step) => return Action::Zoom(step),
        // The whole night goes as one value, with the other end as the page shows it.
        Message::NightFrom(from) => (SettingKey::Night, Night { from, ..data.settings.night }.to_value()),
        Message::NightTo(to) => (SettingKey::Night, Night { to, ..data.settings.night }.to_value()),
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
        Some(tr("Light or dark colors for all Torro apps. “System” follows your desktop.").to_string()),
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

    let interface = widgets::group(
        tr("Interface"),
        vec![setting_row(None, tr("Zoom"), None, zoom_control(settings.zoom))],
        Some(
            tr("Makes all Torro apps larger or smaller, like zooming in a browser: Ctrl + Plus and Ctrl + Minus, Ctrl + 0 resets.")
                .to_string(),
        ),
    );

    let calendar = widgets::group(
        tr("Calendar"),
        vec![setting_row(None, tr("Night"), None, night_control(settings.night))],
        Some(
            tr("The timeline folds the night away and the radar shades it; neither counts it as free time. The same time twice means no night.")
                .to_string(),
        ),
    );

    column![language, appearance, interface, calendar, startup].spacing(space::XL).into()
}

/// "From" and "To" of the night, each a list of the quarter hours of a day.
fn night_control<'a>(night: Night) -> Element<'a, Message> {
    let label = |text: &'a str| typo::text(typo::META, text).style(torro_gui::theme::text_2);
    row![
        label(tr("From")),
        time_field(night.from, Message::NightFrom),
        label(tr("To")),
        time_field(night.to, Message::NightTo),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center)
    .into()
}

/// A time on the quarter hour as a pick list in the look of a secondary button.
fn time_field<'a>(time: Quarter, on_pick: fn(Quarter) -> Message) -> Element<'a, Message> {
    pick_list(Quarter::all().collect::<Vec<_>>(), Some(time), on_pick)
        .padding(style::PICK_LIST_PADDING)
        .text_size(typo::CAPTION.size)
        .text_line_height(typo::CAPTION.line_height)
        .font(typo::CAPTION.font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into()
}

/// −, the level, + and "Reset". The level keeps the width of the widest one ("200%"), so the
/// buttons stay where they are; a button that would change nothing is off.
fn zoom_control<'a>(level: Zoom) -> Element<'a, Message> {
    let press = |step: Step| (step.apply(level) != level).then_some(Message::Zoom(step));
    let width = typo::text_width(&zoom::label(Zoom::MAX), typo::BODY.tabular());
    let value = container(typo::text(typo::BODY.tabular(), zoom::label(level))).center_x(width);
    row![
        widgets::icon_button(Icon::Minus, tr("Zoom out"), press(Step::Out)),
        value,
        widgets::icon_button(Icon::Plus, tr("Zoom in"), press(Step::In)),
        widgets::text_button(None, tr("Reset"), style::button::secondary, press(Step::Reset)),
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center)
    .into()
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
    fn the_zoom_buttons_zoom_the_window() {
        for step in [Step::In, Step::Out, Step::Reset] {
            let action = update(&mut State::default(), Message::Zoom(step), &Data::default());
            assert!(matches!(action, Action::Zoom(sent) if sent == step), "{step:?}: {action:?}");
        }
    }

    #[test]
    fn a_time_of_the_night_sets_the_whole_night() {
        let quarter = |hour, minute| Quarter::new(hour, minute).expect("quarter");
        let from = update(&mut State::default(), Message::NightFrom(quarter(23, 30)), &Data::default());
        assert_eq!(sent(from), (SettingKey::Night, serde_json::json!({"from": "23:30", "to": "06:00"})));
        let to = update(&mut State::default(), Message::NightTo(quarter(7, 0)), &Data::default());
        assert_eq!(sent(to), (SettingKey::Night, serde_json::json!({"from": "22:00", "to": "07:00"})));
    }

    #[test]
    fn page_builds_with_default_and_stored_settings() {
        let stored = Data {
            settings: Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: false,
                hidden_collections: vec!["work".into()],
                zoom: torro_core::settings::Zoom::MAX,
                night: Night::none_at(Quarter::new(6, 0).expect("quarter")),
            },
            ..Data::default()
        };
        let (state, default) = (State::default(), Data::default());
        let _default: Element<'_, Message> = view(&state, &default);
        let _stored: Element<'_, Message> = view(&state, &stored);
    }
}
