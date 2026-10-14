//! The pages of the setup's steps (task W.4: Welcome, Components and Done; the steps of tasks
//! W.5–W.8 follow here): each with a headline, one sentence in `text_2` and settings groups, plus
//! what the footer says and what its primary button is called.

use iced::widget::{Space, column, toggler};
use iced::{Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, Language};
use torro_gui::icon::Icon;
use torro_gui::layout;
use torro_gui::theme::{self, Appearance, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo};

use super::{Data, Message, State, Step, app_of};
use crate::ui::setting_row;

/// The page of the current step.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    match state.step {
        Step::Welcome => welcome(state),
        Step::Components => components(state, data),
        Step::Done => done(state, data),
        // Not listed until their tasks build them ([`Step::built`]).
        Step::Nextcloud | Step::Calendar | Step::Tasks | Step::Mail | Step::Inbox | Step::Ai => Space::new().into(),
    }
}

/// What the footer says at its left on the current step, if anything.
pub fn status(state: &State, _data: &Data) -> Option<String> {
    match state.step {
        Step::Components => {
            let on = Component::ALL.len() - state.off.len();
            Some(trf("{on} of {all} on", &[("on", &on), ("all", &Component::ALL.len())]))
        }
        _ => None,
    }
}

/// The word of the primary button on the current step.
pub fn primary(state: &State) -> &'static str {
    match state.step {
        Step::Welcome => tr("Get started"),
        Step::Done => tr("Finish"),
        _ => tr("Continue"),
    }
}

/// The headline and the sentence under it.
fn title<'a>(headline: &'a str, lead: &'a str) -> Element<'a, Message> {
    column![typo::text(typo::headline(), headline), typo::text(typo::body(), lead).style(theme::text_2)]
        .spacing(space::xs())
        .into()
}

/// The parts of a page with the gap between groups.
fn page<'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column(parts).spacing(layout::section_gap()).width(Fill).into()
}

fn welcome(state: &State) -> Element<'_, Message> {
    let languages = [Language::System, Language::English, Language::German];
    let language = widgets::segmented(
        [
            (tr("System"), Message::Language(Language::System)),
            // In their own names on purpose: someone who cannot read the current language must
            // still find theirs.
            ("English", Message::Language(Language::English)),
            ("Deutsch", Message::Language(Language::German)),
        ],
        languages.iter().position(|language| *language == state.language).unwrap_or_default(),
    );
    let looks = [Appearance::System, Appearance::Light, Appearance::Dark];
    let appearance = widgets::segmented(
        [
            (tr("System"), Message::Appearance(Appearance::System)),
            (tr("Light"), Message::Appearance(Appearance::Light)),
            (tr("Dark"), Message::Appearance(Appearance::Dark)),
        ],
        looks.iter().position(|look| *look == state.appearance).unwrap_or_default(),
    );
    let preferences = widgets::group(
        tr("Preferences"),
        vec![setting_row(None, tr("Language"), None, language), setting_row(None, tr("Appearance"), None, appearance)],
        Some(tr("Applies to all apps of the suite. A new language shows when an app starts again.").to_string()),
    );
    page(vec![
        title(
            tr("Set up Torro Workplace"),
            tr(
                "Connect your accounts, choose what you need, and add calendars, mail and AI. Every step can be skipped and changed later in Settings.",
            ),
        ),
        preferences,
    ])
}

/// What a component is for, one line on its card.
fn purpose(component: Component, data: &Data) -> String {
    match component {
        Component::Calendar => tr("Calendars from Nextcloud and other CalDAV servers").into(),
        Component::Tasks => tr("Task lists from CalDAV, issues from GitHub").into(),
        Component::Mail => tr("Mail accounts, or the ones from TorroMail").into(),
        Component::Inbox => tr("Notifications, mentions and reviews in one place").into(),
        Component::Assistant => tr("Chat with a local or a cloud AI").into(),
        Component::Workspaces => match window_manager(data) {
            Some(name) => trf("Save and restore window layouts · {wm} found", &[("wm", &name)]),
            None => tr("Save and restore window layouts · needs a tiling window manager").into(),
        },
    }
}

/// The tiling window manager the core works with, by its product name; `None` without one.
fn window_manager(data: &Data) -> Option<String> {
    let backend = data.status.as_ref()?.backend.as_str();
    let name = match backend {
        "" | "unsupported" => return None,
        "hyprland" => "Hyprland",
        "aerospace" => "AeroSpace",
        "glazewm" => "GlazeWM",
        other => other,
    };
    Some(name.to_string())
}

/// A component's name.
pub fn name(component: Component) -> &'static str {
    match component {
        Component::Calendar => tr("Calendar"),
        Component::Tasks => tr("Tasks"),
        Component::Mail => tr("Mail"),
        Component::Inbox => tr("Notification Hub"),
        Component::Assistant => tr("Assistant"),
        Component::Workspaces => tr("Workspaces"),
    }
}

/// A component's symbol, as the suite shows its app.
fn symbol(component: Component) -> Icon {
    match component {
        Component::Calendar => Icon::Calendar,
        Component::Tasks => Icon::ListChecks,
        Component::Mail => Icon::Mail,
        Component::Inbox => Icon::Inbox,
        Component::Assistant => Icon::User,
        Component::Workspaces => Icon::Layers,
    }
}

fn components<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let rows = Component::ALL
        .into_iter()
        .map(|component| {
            let on = !state.off.contains(&component);
            let switch = toggler(on)
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |on| Message::Component(component, on));
            let tile = widgets::source_tile(TileFace::Icon(symbol(component)), TileSize::Medium, None);
            setting_row(Some(tile), name(component), Some(purpose(component, data)), switch)
        })
        .collect();
    let components = widgets::group(
        tr("Components"),
        rows,
        Some(
            tr("Turned off, a part of the suite leaves the app switcher and the tray, and nothing runs for it in the background. Accounts and data stay.")
                .to_string(),
        ),
    );
    page(vec![
        title(
            tr("What do you want to use?"),
            tr("Turn on what you need; the next steps set up each of them. You can change this later under Apps."),
        ),
        components,
    ])
}

fn done<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let installed = |app: AppId| {
        data.apps.as_ref().is_some_and(|apps| apps.iter().any(|a| a.window_class == app.window_class && a.installed))
    };
    let rows = Component::ALL
        .into_iter()
        .map(|component| {
            let on = !state.off.contains(&component);
            let detail = if on { tr("On") } else { tr("Off") };
            let mark: Element<'a, Message> =
                widgets::source_tile(TileFace::Icon(symbol(component)), TileSize::Medium, None);
            let control: Element<'a, Message> = match app_of(component).filter(|_| on) {
                Some(app) => widgets::text_button(
                    Some(Icon::ArrowUpRight),
                    tr("Open"),
                    style::button::ghost,
                    installed(app).then_some(Message::OpenApp(app.window_class)),
                ),
                None => Space::new().into(),
            };
            setting_row(Some(mark), name(component), Some(detail.to_string()), control)
        })
        .collect();
    let summary = widgets::group(tr("Components"), rows, Some(tr("Change any of this later in Settings.").to_string()));
    page(vec![title(tr("You're set up"), tr("Torro Workplace is ready. Here is what you turned on.")), summary])
}
