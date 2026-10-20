//! Workspace templates (task S.7): a settings group with a row per template (name, windows,
//! last updated). A row shows one action, "Load" (into the active workspace, mode Add); rename
//! (inline) and delete (with confirmation, a dialog over the whole window, [`overlay`]) sit in the
//! "…" menu of the row, which is a layer over the window like the dialog. Requests:
//! `RenameTemplate`, `DeleteTemplate`, `LoadTemplate { mode: Add }`. Empty: one sentence explaining
//! how to save a workspace from the tray menu.

use chrono::{DateTime, Utc};
use iced::widget::{row, text_input};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime;
use torro_gui::icon::Icon;
use torro_gui::theme::space;
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, empty_state};
use torro_gui::{style, typo};
use torro_ipc::Request;
use torro_setups::{LoadMode, TemplateSummary};

use super::{Action, Data, confirm, row_frame, setting_row};

/// Page-local state: at most one row has its menu open, at most one is being renamed, at most one
/// deletion is confirmed.
#[derive(Debug, Default)]
pub struct State {
    /// Id of the template whose "…" menu is open.
    menu: Option<String>,
    /// Where the "…" button of the open menu is, for the layer that floats next to it. Only that
    /// one button registers, so a single anchor serves all rows.
    anchor: Anchor,
    renaming: Option<Rename>,
    /// Id of the template whose delete dialog is open.
    confirm_delete: Option<String>,
}

/// A row in rename mode: the text typed so far and the name it started with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rename {
    id: String,
    original: String,
    name: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The "…" button of a row: opens its menu, or closes it when it is open.
    ToggleMenu(String),
    /// A click next to the menu, or Escape.
    CloseMenu,
    /// "Rename" in the menu.
    StartRename(String),
    RenameChanged(String),
    /// "Save" or Enter.
    SaveRename,
    /// "Cancel": the row goes back to showing the name it had.
    CancelRename,
    /// "Load" in a row.
    Load(String),
    /// "Delete" in the menu: opens the confirmation.
    AskDelete(String),
    /// "Cancel" in the dialog, or a click next to it.
    CancelDelete,
    /// "Delete template" in the dialog.
    ConfirmDelete,
}

/// What Escape means on this page right now: close the delete dialog, else the menu, else stop
/// renaming.
pub fn escape(state: &State) -> Option<Message> {
    if state.confirm_delete.is_some() {
        Some(Message::CancelDelete)
    } else if state.menu.is_some() {
        Some(Message::CloseMenu)
    } else if state.renaming.is_some() {
        Some(Message::CancelRename)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::ToggleMenu(id) => {
            state.menu = if state.menu.as_deref() == Some(id.as_str()) { None } else { Some(id) };
            Action::None
        }
        Message::CloseMenu => {
            state.menu = None;
            Action::None
        }
        Message::StartRename(id) => {
            state.menu = None;
            state.renaming = find(data, &id).map(|template| Rename {
                id: template.id.clone(),
                original: template.name.clone(),
                name: template.name.clone(),
            });
            Action::None
        }
        Message::RenameChanged(name) => {
            if let Some(rename) = state.renaming.as_mut() {
                rename.name = name;
            }
            Action::None
        }
        Message::SaveRename => match save_rename(state) {
            Some(request) => Action::Send(request),
            None => Action::None,
        },
        Message::CancelRename => {
            state.renaming = None;
            Action::None
        }
        Message::Load(id) => Action::Send(Request::LoadTemplate { id, mode: LoadMode::Add }),
        Message::AskDelete(id) => {
            state.menu = None;
            state.confirm_delete = Some(id);
            Action::None
        }
        Message::CancelDelete => {
            state.confirm_delete = None;
            Action::None
        }
        Message::ConfirmDelete => match state.confirm_delete.take() {
            Some(id) => Action::Send(Request::DeleteTemplate { id }),
            None => Action::None,
        },
    }
}

/// Ends the rename and returns the request for it. An empty name keeps the row open; an
/// unchanged name closes it without a request.
fn save_rename(state: &mut State) -> Option<Request> {
    let rename = state.renaming.as_ref()?;
    let name = rename.name.trim();
    if name.is_empty() {
        return None;
    }
    let request =
        (name != rename.original).then(|| Request::RenameTemplate { id: rename.id.clone(), name: name.to_string() });
    state.renaming = None;
    request
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    if data.templates.is_empty() {
        return empty_state(
            tr("No templates yet"),
            tr("Arrange a workspace, then choose Save workspace in the tray menu."),
        );
    }
    let now = Utc::now();
    let rows = data.templates.iter().map(|template| template_row(state, template, now)).collect();
    let footer = tr("Save the active workspace from the tray menu: Save workspace.");
    widgets::group(tr("Templates"), rows, Some(footer.to_string()))
}

/// The page's layer over the whole window: the delete confirmation while it is open, else the
/// "…" menu of a row.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(template) = state.confirm_delete.as_ref().and_then(|id| find(data, id)) {
        return Some(confirm(
            tr("Delete this template?"),
            trf("\"{name}\" will be deleted. Saved backups stay in the data folder.", &[("name", &template.name)]),
            tr("Delete template"),
            Message::ConfirmDelete,
            Message::CancelDelete,
        ));
    }
    let template = state.menu.as_ref().and_then(|id| find(data, id))?;
    Some(widgets::floating(&state.anchor, Placement::Below(Align::End), row_menu(template), Message::CloseMenu))
}

/// The "…" menu of a row: rename and delete, the actions a row does not show.
fn row_menu(template: &TemplateSummary) -> Element<'_, Message> {
    let id = &template.id;
    widgets::menu(vec![MenuSection::new(vec![
        MenuItem::new(Icon::SquarePen, tr("Rename"), Message::StartRename(id.clone())),
        MenuItem::new(Icon::X, tr("Delete"), Message::AskDelete(id.clone())),
    ])])
}

/// One template: name, meta line, "Load" and the "…" menu button; in rename mode the row becomes
/// an input.
fn template_row<'a>(state: &'a State, template: &'a TemplateSummary, now: DateTime<Utc>) -> Element<'a, Message> {
    if let Some(rename) = state.renaming.as_ref().filter(|rename| rename.id == template.id) {
        return rename_row(rename);
    }
    let meta = format!(
        "{} · {}",
        if template.windows == 1 {
            tr("1 window").to_string()
        } else {
            trf("{n} windows", &[("n", &template.windows)])
        },
        trf("Updated {when}", &[("when", &relative_time(template.updated, now))]),
    );
    let id = &template.id;
    let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(id.clone())));
    // Only the button of the open menu registers its place; the menu floats next to it.
    let more = if state.menu.as_deref() == Some(id.as_str()) { widgets::anchored(&state.anchor, more) } else { more };
    let actions =
        row![widgets::text_button(None, tr("Load"), style::button::secondary, Some(Message::Load(id.clone()))), more]
            .spacing(space::s())
            .align_y(Alignment::Center);
    setting_row(None, template.name.as_str(), Some(meta), actions)
}

/// The rename row: the input (Enter saves), "Cancel" and "Save" next to it.
fn rename_row(rename: &Rename) -> Element<'_, Message> {
    let can_save = !rename.name.trim().is_empty();
    let input = text_input(tr("Name"), &rename.name)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(Message::RenameChanged)
        .on_submit(Message::SaveRename);
    let content = row![
        input,
        widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelRename)),
        widgets::text_button(None, tr("Save"), style::button::primary, can_save.then_some(Message::SaveRename)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    row_frame(content, true)
}

fn find<'a>(data: &'a Data, id: &str) -> Option<&'a TemplateSummary> {
    data.templates.iter().find(|template| template.id == id)
}

/// How long ago a template was updated, for its row: "just now", "5 min ago", "2 h ago",
/// "3 d ago"; from a week on, the local date. A time in the future counts as "just now".
pub fn relative_time(updated: DateTime<Utc>, now: DateTime<Utc>) -> String {
    const MINUTE: i64 = 1;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let minutes = (now - updated).num_minutes();
    if minutes < MINUTE {
        tr("just now").to_string()
    } else if minutes < HOUR {
        trf("{n} min ago", &[("n", &minutes)])
    } else if minutes < DAY {
        trf("{n} h ago", &[("n", &(minutes / HOUR))])
    } else if minutes < 7 * DAY {
        trf("{n} d ago", &[("n", &(minutes / DAY))])
    } else {
        datetime::format_date(datetime::to_local(updated).date())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    fn template(id: &str, name: &str) -> TemplateSummary {
        let updated = Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap();
        TemplateSummary {
            id: id.into(),
            name: name.into(),
            windows: 3,
            updated,
            shortcut: None,
            startup: None,
            workspace: "1".into(),
            monitor: None,
        }
    }

    fn data() -> Data {
        Data { templates: vec![template("dev", "Dev"), template("mail", "Mail")], ..Data::default() }
    }

    fn sent(action: Action) -> Option<Request> {
        match action {
            Action::Send(request) => Some(request),
            _ => None,
        }
    }

    #[test]
    fn relative_time_picks_the_coarsest_unit() {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 12, 0, 0).unwrap();
        assert_eq!(relative_time(now - Duration::seconds(20), now), tr("just now"));
        assert_eq!(relative_time(now + Duration::minutes(5), now), tr("just now"));
        assert_eq!(relative_time(now - Duration::minutes(5), now), trf("{n} min ago", &[("n", &5)]));
        assert_eq!(relative_time(now - Duration::hours(2), now), trf("{n} h ago", &[("n", &2)]));
        assert_eq!(relative_time(now - Duration::days(3), now), trf("{n} d ago", &[("n", &3)]));
        let old = now - Duration::days(10);
        assert_eq!(relative_time(old, now), datetime::format_date(datetime::to_local(old).date()));
    }

    #[test]
    fn rename_saves_trimmed_name_and_closes() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::StartRename("dev".into()), &data);
        update(&mut state, Message::RenameChanged("  Work  ".into()), &data);
        let request = sent(update(&mut state, Message::SaveRename, &data));
        assert_eq!(request, Some(Request::RenameTemplate { id: "dev".into(), name: "Work".into() }));
        assert!(state.renaming.is_none());
    }

    #[test]
    fn rename_with_unchanged_or_empty_name_sends_nothing() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::StartRename("dev".into()), &data);
        update(&mut state, Message::RenameChanged("   ".into()), &data);
        assert!(sent(update(&mut state, Message::SaveRename, &data)).is_none());
        assert!(state.renaming.is_some(), "an empty name keeps the row open");

        update(&mut state, Message::RenameChanged("Dev ".into()), &data);
        assert!(sent(update(&mut state, Message::SaveRename, &data)).is_none());
        assert!(state.renaming.is_none(), "an unchanged name just closes the row");
    }

    #[test]
    fn cancel_rename_restores_the_row() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::StartRename("dev".into()), &data);
        update(&mut state, Message::RenameChanged("Changed".into()), &data);
        assert!(sent(update(&mut state, Message::CancelRename, &data)).is_none());
        assert!(state.renaming.is_none());
        assert_eq!(data.templates[0].name, "Dev");
    }

    #[test]
    fn starting_rename_for_unknown_template_does_nothing() {
        let mut state = State::default();
        update(&mut state, Message::StartRename("gone".into()), &data());
        assert!(state.renaming.is_none());
    }

    #[test]
    fn load_sends_add_mode() {
        let mut state = State::default();
        let request = sent(update(&mut state, Message::Load("dev".into()), &data()));
        assert_eq!(request, Some(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add }));
    }

    #[test]
    fn the_menu_opens_toggles_and_closes() {
        let data = data();
        let mut state = State::default();
        assert!(sent(update(&mut state, Message::ToggleMenu("dev".into()), &data)).is_none());
        assert_eq!(state.menu.as_deref(), Some("dev"));
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert_eq!(state.menu.as_deref(), Some("mail"), "another row takes the menu over");
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert!(state.menu.is_none(), "the same button closes it again");
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        update(&mut state, Message::CloseMenu, &data);
        assert!(state.menu.is_none());
    }

    #[test]
    fn the_menu_entries_close_it_and_go_on() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        update(&mut state, Message::StartRename("dev".into()), &data);
        assert!(state.menu.is_none());
        assert!(state.renaming.is_some());

        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        update(&mut state, Message::AskDelete("mail".into()), &data);
        assert!(state.menu.is_none());
        assert_eq!(state.confirm_delete.as_deref(), Some("mail"));
    }

    #[test]
    fn escape_closes_the_dialog_then_the_menu_then_the_rename() {
        let data = data();
        let mut state = State::default();
        assert!(escape(&state).is_none());
        update(&mut state, Message::StartRename("dev".into()), &data);
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        update(&mut state, Message::AskDelete("mail".into()), &data);
        update(&mut state, Message::ToggleMenu("mail".into()), &data);
        assert!(matches!(escape(&state), Some(Message::CancelDelete)));
        update(&mut state, Message::CancelDelete, &data);
        assert!(matches!(escape(&state), Some(Message::CloseMenu)));
        update(&mut state, Message::CloseMenu, &data);
        assert!(matches!(escape(&state), Some(Message::CancelRename)));
    }

    #[test]
    fn delete_needs_confirmation() {
        let data = data();
        let mut state = State::default();
        assert!(sent(update(&mut state, Message::AskDelete("mail".into()), &data)).is_none());
        assert!(sent(update(&mut state, Message::CancelDelete, &data)).is_none());
        assert!(state.confirm_delete.is_none());

        update(&mut state, Message::AskDelete("mail".into()), &data);
        let request = sent(update(&mut state, Message::ConfirmDelete, &data));
        assert_eq!(request, Some(Request::DeleteTemplate { id: "mail".into() }));
        assert!(state.confirm_delete.is_none());
        assert!(sent(update(&mut state, Message::ConfirmDelete, &data)).is_none(), "one confirmation, one delete");
    }

    #[test]
    fn views_build_for_empty_and_filled_lists() {
        let (empty_state, empty_data) = (State::default(), Data::default());
        let _empty: Element<'_, Message> = view(&empty_state, &empty_data);
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::StartRename("dev".into()), &data);
        update(&mut state, Message::AskDelete("mail".into()), &data);
        drop(view(&state, &data));
        assert!(overlay(&state, &data).is_some(), "the delete confirmation is a window layer");
        update(&mut state, Message::CancelDelete, &data);
        assert!(overlay(&state, &data).is_none());
    }

    #[test]
    fn the_open_menu_is_a_window_layer_for_a_known_template_only() {
        let data = data();
        let mut state = State::default();
        update(&mut state, Message::ToggleMenu("dev".into()), &data);
        assert!(overlay(&state, &data).is_some());
        drop(view(&state, &data));
        update(&mut state, Message::ToggleMenu("gone".into()), &data);
        assert!(overlay(&state, &data).is_none(), "a menu for a template that is gone shows nothing");
    }
}
