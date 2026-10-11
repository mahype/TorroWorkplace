//! Workspace templates (task S.7): list (name, windows, last updated), rename inline, delete
//! with confirmation (`torro_gui::widgets::modal`), load into the active workspace (Add).
//! Requests: `RenameTemplate`, `DeleteTemplate`, `LoadTemplate { mode: Add }`. Empty: one
//! sentence explaining how to save a workspace from the tray menu.

use chrono::{DateTime, Utc};
use iced::widget::{button, column, container, row, space, text, text_input};
use iced::{Alignment, Background, Element, Length, Theme, border};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime;
use torro_gui::theme;
use torro_gui::widgets::{empty_state, modal, section_title};
use torro_ipc::Request;
use torro_setups::{LoadMode, TemplateSummary};

use super::{Action, Data};

/// Page-local state: at most one row is being renamed, at most one deletion is confirmed.
#[derive(Debug, Default)]
pub struct State {
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
    StartRename(String),
    RenameChanged(String),
    /// "Save" or Enter.
    SaveRename,
    /// "Cancel": the row goes back to showing the name it had.
    CancelRename,
    LoadHere(String),
    /// "Delete" in a row: opens the confirmation.
    AskDelete(String),
    /// "Cancel" in the dialog, or a click next to it.
    CancelDelete,
    /// "Delete template" in the dialog.
    ConfirmDelete,
}

/// What Escape means on this page right now: close the delete dialog, else stop renaming.
pub fn escape(state: &State) -> Option<Message> {
    if state.confirm_delete.is_some() {
        Some(Message::CancelDelete)
    } else if state.renaming.is_some() {
        Some(Message::CancelRename)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::StartRename(id) => {
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
        Message::LoadHere(id) => Action::Send(Request::LoadTemplate { id, mode: LoadMode::Add }),
        Message::AskDelete(id) => {
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
    let mut list = column![].spacing(8);
    for template in &data.templates {
        list = list.push(template_row(state, template, now));
    }
    let footer = text(tr("Save the active workspace from the tray menu: Save workspace.")).size(14).color(theme::MUTED);
    let page: Element<'a, Message> =
        column![section_title::<Message>(tr("Templates")), list, footer].spacing(16).into();
    match state.confirm_delete.as_ref().and_then(|id| find(data, id)) {
        Some(template) => modal(page, delete_dialog(template), 400.0, Message::CancelDelete),
        None => page,
    }
}

/// One template: name, meta line and actions; in rename mode the name becomes an input.
fn template_row<'a>(state: &'a State, template: &'a TemplateSummary, now: DateTime<Utc>) -> Element<'a, Message> {
    let body: Element<'a, Message> = match state.renaming.as_ref().filter(|rename| rename.id == template.id) {
        Some(rename) => rename_body(rename),
        None => {
            let meta = format!(
                "{} · {}",
                trf("{n} windows", &[("n", &template.windows)]),
                trf("Updated {when}", &[("when", &relative_time(template.updated, now))]),
            );
            let actions = row![
                button(text(tr("Rename")).size(14))
                    .style(button::secondary)
                    .on_press(Message::StartRename(template.id.clone())),
                button(text(tr("Load here")).size(14))
                    .style(button::secondary)
                    .on_press(Message::LoadHere(template.id.clone())),
                space().width(Length::Fill),
                button(text(tr("Delete")).size(14))
                    .style(button::text)
                    .on_press(Message::AskDelete(template.id.clone())),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            column![
                text(template.name.clone()).size(16).color(theme::TEXT),
                text(meta).size(12).color(theme::MUTED),
                actions,
            ]
            .spacing(4)
            .into()
        }
    };
    card(body, 8.0, 16)
}

/// The rename row: the input, Enter saves, "Cancel" and "Save" below it.
fn rename_body(rename: &Rename) -> Element<'_, Message> {
    let can_save = !rename.name.trim().is_empty();
    let actions = row![
        space().width(Length::Fill),
        button(text(tr("Cancel")).size(14)).style(button::secondary).on_press(Message::CancelRename),
        button(text(tr("Save")).size(14))
            .style(button::primary)
            .on_press_maybe(can_save.then_some(Message::SaveRename)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    column![
        text_input(tr("Name"), &rename.name)
            .size(14)
            .padding(8)
            .width(Length::Fill)
            .on_input(Message::RenameChanged)
            .on_submit(Message::SaveRename),
        actions,
    ]
    .spacing(8)
    .into()
}

/// The confirmation for deleting one template. Saved backups are not touched.
fn delete_dialog(template: &TemplateSummary) -> Element<'_, Message> {
    let body = column![
        text(tr("Delete this template?")).size(18).color(theme::TEXT),
        text(trf("\"{name}\" will be deleted. Saved backups stay in the data folder.", &[("name", &template.name)]))
            .size(14)
            .color(theme::MUTED),
        row![
            space().width(Length::Fill),
            button(text(tr("Cancel")).size(14)).style(button::secondary).on_press(Message::CancelDelete),
            button(text(tr("Delete template")).size(14)).style(button::danger).on_press(Message::ConfirmDelete),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(16);
    card(body, 12.0, 24)
}

/// A raised surface with rounded corners, for rows and the dialog.
fn card<'a>(body: impl Into<Element<'a, Message>>, radius: f32, padding: u16) -> Element<'a, Message> {
    container(body)
        .padding(padding)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(theme::SURFACE)),
            border: border::rounded(radius),
            ..container::Style::default()
        })
        .into()
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
        TemplateSummary { id: id.into(), name: name.into(), windows: 3, updated }
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
    fn load_here_sends_add_mode() {
        let mut state = State::default();
        let request = sent(update(&mut state, Message::LoadHere("dev".into()), &data()));
        assert_eq!(request, Some(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add }));
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
        let _filled: Element<'_, Message> = view(&state, &data);
    }
}
