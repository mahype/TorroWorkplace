//! The page Privacy of the suite's settings (issue #672, `docs/settings.md`, "Privacy (#672)"): whether Chat
//! and Mail load pictures, link previews and videos from servers other than the person's own, in one place
//! for the whole suite.
//!
//! Two groups:
//!
//! - **Content from other providers** – the level as a segment of Always show, Ask and Never show
//!   ([`ForeignContent`]), under it a sentence that says what the chosen level does and that Never show also
//!   stops the sender logos in Mail.
//! - **Providers** – only the providers a person decided on in Chat or Mail ("Always allow …", "Never for
//!   …"), each with its name, a segment Allowed | Blocked and ✕ "Forget provider", after which Chat and Mail
//!   ask again. Empty, one row says where they come from.
//!
//! The control centre shows the page before General, an app without the main application as one of the
//! suite's sections behind its gear, like Appearance ([`crate::host::SUITE_SECTIONS`]).
//!
//! The page keeps no state and never talks to the daemon: [`update`] turns a message into the one
//! `settings.set_setting` it stands for (`privacy_content` or `privacy_provider`), which the window sends as
//! it sends every other change of the settings. Both keys take only the app key: a person decides, never an
//! agent (`docs/api.md`, "Was nie nach außen geht", point 14).

use iced::widget::{column, responsive, row, space as blank};
use iced::{Alignment, Element, Length, Size};
use torro_core::i18n::tr;
use torro_core::privacy::{ForeignContent, Privacy, ProviderChoice, name_of};
use torro_core::settings::Settings;
use torro_gui::icon::Icon;
use torro_gui::theme::{size, space};
use torro_gui::{layout, widgets};
use torro_ipc::{Request, SettingKey};

use crate::parts::{segmented_row, setting_row};

/// What the page's widgets report.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A segment of "Content from other providers": load from every provider, ask, or load nothing.
    Content(ForeignContent),
    /// "Allowed" or "Blocked" at the provider with this key (`youtube`, `mailchimp.com`), or its ✕ "Forget
    /// provider" (`None`): it leaves the list, and Chat and Mail ask about it again.
    Provider(String, Option<ProviderChoice>),
}

/// The one request `message` stands for, to be sent as the window sends every change of the settings
/// (then the settings are loaded again, and the page shows what the core keeps):
///
/// - [`Message::Content`] – `settings.set_setting` with [`SettingKey::PrivacyContent`] and the level's name on
///   the wire (`"always"`, `"ask"`, `"never"`, [`ForeignContent::as_str`]).
/// - [`Message::Provider`] – `settings.set_setting` with [`SettingKey::PrivacyProvider`] and
///   `{"provider": "<key>", "choice": "allowed" | "blocked" | null}` (`torro_core::privacy::ProviderChange`);
///   `null` forgets the provider.
pub fn update(message: Message) -> Request {
    match message {
        Message::Content(level) => {
            Request::SetSetting { key: SettingKey::PrivacyContent, value: serde_json::json!(level.as_str()) }
        }
        Message::Provider(provider, choice) => Request::SetSetting {
            key: SettingKey::PrivacyProvider,
            value: serde_json::json!({ "provider": provider, "choice": choice }),
        },
    }
}

/// The page: "Content from other providers" and "Providers" one under the other, as the core keeps them in
/// `settings.privacy`.
pub fn view<'a>(settings: &'a Settings) -> Element<'a, Message> {
    let privacy = &settings.privacy;
    column![content(privacy.content), providers(privacy)].spacing(layout::section_gap()).into()
}

/// The providers of `--demo`: YouTube and a news site allowed, a newsletter service blocked, so the list has
/// both choices and a known service beside plain domains. The control centre's demo and every app's show
/// them alike.
pub fn demo() -> Privacy {
    let providers = [
        ("youtube", ProviderChoice::Allowed),
        ("mailchimp.com", ProviderChoice::Blocked),
        ("example.org", ProviderChoice::Allowed),
    ];
    Privacy {
        content: ForeignContent::Ask,
        providers: providers.into_iter().map(|(key, choice)| (key.to_string(), choice)).collect(),
    }
}

/// The group "Content from other providers": the level as a segment, the sentence under it.
fn content<'a>(level: ForeignContent) -> Element<'a, Message> {
    let options = ForeignContent::ALL.iter().map(|&each| (level_label(each).to_string(), Message::Content(each)));
    let active = ForeignContent::ALL.iter().position(|&each| each == level).unwrap_or_default();
    let row = segmented_row(tr("Pictures, previews and videos"), None, options.collect(), active);
    widgets::group(tr("Content from other providers"), vec![row], Some(content_footer(level)))
}

/// The word of a level on its segment.
fn level_label(level: ForeignContent) -> &'static str {
    match level {
        ForeignContent::Always => tr("Always show"),
        ForeignContent::Ask => tr("Ask"),
        ForeignContent::Never => tr("Never show"),
    }
}

/// The sentence under the level: what the chosen one does, then that Never show stops the sender logos of
/// Mail too, which a person would not count as "content" (the switch really stops everything).
fn content_footer(level: ForeignContent) -> String {
    let what = match level {
        ForeignContent::Always => {
            tr("Chat and Mail load pictures, previews and videos from every provider without asking.")
        }
        ForeignContent::Ask => {
            tr("Chat and Mail ask before they load pictures, previews or videos from a provider for the first time.")
        }
        ForeignContent::Never => tr("Chat and Mail load nothing from other providers and ask nothing."),
    };
    format!("{what} {}", tr("Never show also stops sender logos in Mail."))
}

/// The providers of the list as the page shows them: key, name and choice, by name.
fn listed(privacy: &Privacy) -> Vec<(String, String, ProviderChoice)> {
    let mut listed: Vec<(String, String, ProviderChoice)> =
        privacy.providers.iter().map(|(key, choice)| (key.clone(), name_of(key), *choice)).collect();
    listed.sort_by_cached_key(|(_, name, _)| name.to_lowercase());
    listed
}

/// The group "Providers": a row per provider of the list, their controls in one column at the right while
/// every name fits beside it, else each under its name (`widgets::segment_column`, as the rights editor
/// does). Empty, one row that says where providers come from.
fn providers<'a>(privacy: &Privacy) -> Element<'a, Message> {
    let listed = listed(privacy);
    if listed.is_empty() {
        let hint = tr("Providers appear here when you allow or block one in Chat or Mail.");
        let row = setting_row(None, tr("No providers yet"), Some(hint.to_string()), blank());
        return widgets::group(tr("Providers"), vec![row], None);
    }
    // The list only counts while Ask is chosen ([`Privacy::decide`]); the sentence says so instead of
    // greying the rows out, which still change what Ask will do.
    let footer = match privacy.content {
        ForeignContent::Ask => tr("Forget a provider, and Chat and Mail ask about it again."),
        ForeignContent::Always | ForeignContent::Never => tr("The list counts only while “Ask” is chosen."),
    };
    responsive(move |room: Size| {
        let names = listed.iter().map(|(_, name, _)| (name.as_str(), None));
        let column = widgets::segment_column(controls_width(), names, room.width);
        let rows = listed.iter().map(|(key, name, choice)| provider_row(key, name.clone(), *choice, column)).collect();
        widgets::group(tr("Providers"), rows, Some(footer.to_string()))
    })
    .height(Length::Shrink)
    .into()
}

/// How wide the controls of a provider's row are: the segment Allowed | Blocked, a small gap and the ✕.
fn controls_width() -> f32 {
    widgets::segmented_width(&[tr("Allowed"), tr("Blocked")]) + space::s() + size::control_sm()
}

/// One provider's row: its `name`, the segment Allowed | Blocked with `choice` chosen and ✕ "Forget
/// provider", beside the name in `column` or under it (`None`).
fn provider_row<'a>(key: &str, name: String, choice: ProviderChoice, column: Option<f32>) -> Element<'a, Message> {
    let active = match choice {
        ProviderChoice::Allowed => 0,
        ProviderChoice::Blocked => 1,
    };
    let pick = |choice| Message::Provider(key.to_string(), Some(choice));
    let segment = widgets::segmented(
        [(tr("Allowed"), pick(ProviderChoice::Allowed)), (tr("Blocked"), pick(ProviderChoice::Blocked))],
        active,
    );
    let forget = widgets::icon_button(Icon::X, tr("Forget provider"), Some(Message::Provider(key.to_string(), None)));
    let controls = row![segment, forget].spacing(space::s()).align_y(Alignment::Center);
    widgets::segment_row(name, None, controls, column)
}

#[cfg(test)]
mod tests {
    use torro_core::privacy::ProviderChange;

    use super::*;

    fn value_of(request: &Request, wanted: SettingKey) -> serde_json::Value {
        match request {
            Request::SetSetting { key, value } if *key == wanted => value.clone(),
            other => panic!("not a {wanted:?}: {other:?}"),
        }
    }

    /// Each segment of the level sends `privacy_content` with the level's name on the wire.
    #[test]
    fn a_level_sends_privacy_content() {
        for (level, wire) in
            [(ForeignContent::Always, "always"), (ForeignContent::Ask, "ask"), (ForeignContent::Never, "never")]
        {
            let request = update(Message::Content(level));
            assert_eq!(value_of(&request, SettingKey::PrivacyContent), serde_json::json!(wire));
        }
    }

    /// Allowed, Blocked and ✕ send `privacy_provider` with the key and the choice, `null` to forget – in the
    /// form the core reads ([`ProviderChange::from_value`]).
    #[test]
    fn a_provider_sends_privacy_provider() {
        let cases = [
            (Some(ProviderChoice::Allowed), serde_json::json!({"provider": "youtube", "choice": "allowed"})),
            (Some(ProviderChoice::Blocked), serde_json::json!({"provider": "youtube", "choice": "blocked"})),
            (None, serde_json::json!({"provider": "youtube", "choice": null})),
        ];
        for (choice, expected) in cases {
            let value = value_of(&update(Message::Provider("youtube".into(), choice)), SettingKey::PrivacyProvider);
            assert_eq!(value, expected);
            let change = ProviderChange::from_value(&value).expect("the core reads it");
            assert_eq!(change, ProviderChange { provider: "youtube".into(), choice });
        }
    }

    /// Every level's sentence says that Never show also stops the sender logos of Mail.
    #[test]
    fn every_sentence_names_the_sender_logos() {
        for level in ForeignContent::ALL {
            let footer = content_footer(level);
            assert!(footer.ends_with(tr("Never show also stops sender logos in Mail.")), "{level:?}: {footer}");
        }
    }

    /// No provider decided: the list is empty and the page shows its hint instead; with providers they are
    /// listed by their names, a known service by its own ("YouTube").
    #[test]
    fn the_list_is_empty_until_a_provider_is_decided() {
        assert!(listed(&Privacy::default()).is_empty());
        let names: Vec<String> = listed(&demo()).into_iter().map(|(_, name, _)| name).collect();
        assert_eq!(names, ["example.org", "mailchimp.com", "YouTube"]);
        for privacy in [Privacy::default(), demo()] {
            let settings = Settings { privacy, ..Settings::default() };
            let _: Element<'_, Message> = view(&settings);
        }
    }
}
