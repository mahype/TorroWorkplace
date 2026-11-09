//! Retrieved workflow documents beside mail, never mixed with original MIME attachments. All
//! opening and revealing uses the service's id-based, current-rights/hash checked requests.

use crate::app::Message as AppMessage;
use iced::widget::{column, container, row, scrollable};
use iced::{Element, Fill};
use std::collections::{HashMap, HashSet};
use torro_core::i18n::tr;
use torro_gui::theme::{self, size, space};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::workflows::WorkflowRun;
use torro_ipc::{Request, Response};
use torro_settings_ui::mail::automation;

const ACTIONS_SCROLL: &str = "workflow-document-actions";

#[derive(Debug, Default)]
pub struct State {
    runs: HashMap<String, Vec<WorkflowRun>>,
    problems: HashMap<String, String>,
    pending: HashSet<String>,
    feedback: HashMap<String, String>,
    menu_mail: Option<String>,
    cursor: usize,
    anchor: widgets::Anchor,
}
#[derive(Debug, Clone)]
pub enum Message {
    Open { mail: String, id: String },
    Show { mail: String, id: String },
    Toggle(String),
    Key(iced::keyboard::key::Named),
    Close,
}
fn key(request: &Request) -> Option<String> {
    match request {
        Request::ListWorkflowRuns { mail: Some(mail), .. } => Some(mail.clone()),
        Request::OpenWorkflowDocument { id } | Request::ShowWorkflowDocument { id } => Some(id.clone()),
        _ => None,
    }
}
impl State {
    pub fn menu_open(&self) -> bool {
        self.menu_mail.is_some()
    }
    pub fn close_menu(&mut self) {
        self.menu_mail = None;
        self.cursor = 0;
    }
    pub fn follow_mail(&mut self, mail: Option<&str>) {
        if self.menu_mail.as_deref() != mail {
            self.close_menu();
        }
    }
    fn actions(&self, mail: &str) -> Vec<(String, Message)> {
        self.runs
            .get(mail)
            .into_iter()
            .flatten()
            .filter(|run| run.mail == mail)
            .filter_map(|run| run.document.as_ref())
            .filter(|d| !d.missing && !d.changed && !self.pending.contains(&d.id))
            .flat_map(|d| {
                let file = d.relative_path.rsplit(['/', '\\']).next().unwrap_or(&d.relative_path);
                [
                    (format!("{} · {file}", tr("Open")), Message::Open { mail: mail.into(), id: d.id.clone() }),
                    (
                        format!("{} · {file}", tr("Show in folder")),
                        Message::Show { mail: mail.into(), id: d.id.clone() },
                    ),
                ]
            })
            .collect()
    }
    /// Both pointer and keyboard choices are checked against the currently visible mail at dispatch.
    pub fn update(&mut self, current: &str, message: Message) -> Option<Request> {
        use iced::keyboard::key::Named;
        let is_open = matches!(&message, Message::Open { .. });
        match message {
            Message::Toggle(mail) if mail == current => {
                if self.menu_mail.as_deref() == Some(current) {
                    self.close_menu();
                } else if !self.actions(current).is_empty() {
                    self.menu_mail = Some(mail);
                    self.cursor = 0;
                }
            }
            Message::Key(Named::F6) => return self.update(current, Message::Toggle(current.into())),
            Message::Key(key) if self.menu_mail.as_deref() == Some(current) => {
                let actions = self.actions(current);
                match key {
                    Named::ArrowUp => self.cursor = self.cursor.saturating_sub(1),
                    Named::ArrowDown => self.cursor = (self.cursor + 1).min(actions.len().saturating_sub(1)),
                    Named::Enter => {
                        if let Some((_, action)) = actions.get(self.cursor).cloned() {
                            return self.update(current, action);
                        }
                    }
                    _ => {}
                }
            }
            Message::Open { mail, id } | Message::Show { mail, id } if mail == current => {
                let open = self
                    .actions(current)
                    .iter()
                    .any(|(_, action)| matches!(action, Message::Open { id: held, .. } if held == &id));
                if !open {
                    return None;
                }
                self.close_menu();
                // The backend still rechecks rights, location and hash; no paths leave this UI.
                return Some(if is_open {
                    Request::OpenWorkflowDocument { id }
                } else {
                    Request::ShowWorkflowDocument { id }
                });
            }
            Message::Close => self.close_menu(),
            _ => {}
        }
        None
    }

    pub fn overlay(&self, mail: &str, anchor: &widgets::Anchor) -> Option<Element<'static, AppMessage>> {
        if self.menu_mail.as_deref() != Some(mail) {
            return None;
        }
        let items = self
            .actions(mail)
            .into_iter()
            .enumerate()
            .map(|(at, (label, action))| {
                widgets::MenuItem::plain(label, AppMessage::WorkflowDocument(action)).current(at == self.cursor)
            })
            .collect();
        Some(widgets::floating(
            &anchor.or(&self.anchor),
            widgets::Placement::Beside,
            container(
                scrollable(widgets::menu(vec![widgets::MenuSection::new(items)]))
                    .id(ACTIONS_SCROLL)
                    .style(style::scrollable),
            )
            .max_height(size::panel()),
            AppMessage::WorkflowDocument(Message::Close),
        ))
    }
    pub fn reveal_selection(&self) -> iced::Task<AppMessage> {
        iced::widget::operation::scroll_to(
            ACTIONS_SCROLL,
            iced::widget::operation::AbsoluteOffset { x: None, y: Some(self.cursor as f32 * style::MENU_ITEM_HEIGHT) },
        )
    }
    pub fn begin(&mut self, request: &Request) -> bool {
        let Some(key) = key(request) else {
            return false;
        };
        if !self.pending.insert(key.clone()) {
            return false;
        }
        self.feedback.remove(&key);
        true
    }
    pub fn answered(&mut self, request: &Request, answer: &Result<Response, String>) {
        let Some(key) = key(request) else {
            return;
        };
        self.pending.remove(&key);
        match answer {
            Ok(Response::WorkflowRuns { runs }) => {
                self.problems.remove(&key);
                self.runs.insert(key, runs.clone());
            }
            Ok(Response::Ok) => {
                self.problems.remove(&key);
                self.feedback.insert(key, tr("Done").into());
            }
            Ok(Response::Error { code, message }) => {
                self.problems.insert(key, format!("{code}: {message}"));
            }
            Err(problem) => {
                self.problems.insert(key, problem.clone());
            }
            _ => {
                self.problems.insert(key, tr("The service returned an unexpected workflow response.").into());
            }
        }
    }
}
pub fn subscription() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, status, _| {
        use iced::keyboard::{Key, key::Named};
        let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key: Key::Named(key), modifiers, .. }) = event
        else {
            return None;
        };
        if !modifiers.is_empty() {
            return None;
        }
        (key == Named::F6
            || status == iced::event::Status::Ignored
                && matches!(key, Named::ArrowDown | Named::ArrowUp | Named::Enter))
        .then_some(Message::Key(key))
    })
}
/// A bounded, independent block; partial results remain visible and changed files are never
/// opened or silently downloaded again.
pub fn documents(state: &State, mail: &str) -> Option<Element<'static, AppMessage>> {
    let runs = state.runs.get(mail).map(Vec::as_slice).unwrap_or_default();
    let problem = state.problems.get(mail);
    if runs.is_empty() && problem.is_none() {
        return None;
    }
    let mut lines = column![].spacing(space::m());
    if let Some(problem) = problem {
        lines = lines.push(typo::text(typo::body(), problem.clone()));
    }
    for run in runs.iter().filter(|run| run.mail == mail) {
        let mut entry =
            column![typo::text(typo::meta(), automation::run_status(run)).style(theme::text_2)].spacing(space::xs());
        if let Some(d) = &run.document {
            let busy = state.pending.contains(&d.id);
            let valid = !d.missing && !d.changed && !busy;
            let status = if d.missing {
                tr("File missing — not downloaded again automatically")
            } else if d.changed {
                tr("File changed — opening is blocked")
            } else {
                automation::provenance(d.provenance)
            };
            entry = entry
                .push(typo::text(typo::body(), d.relative_path.clone()).width(Fill))
                .push(typo::text(typo::meta(), status).style(theme::text_2))
                .push(
                    row![
                        widgets::text_button(
                            None,
                            if busy { tr("Working…") } else { tr("Open") },
                            style::button::secondary,
                            valid.then(|| AppMessage::WorkflowDocument(Message::Open {
                                mail: mail.into(),
                                id: d.id.clone()
                            })),
                        ),
                        widgets::text_button(
                            None,
                            tr("Show in folder"),
                            style::button::ghost,
                            valid.then(|| AppMessage::WorkflowDocument(Message::Show {
                                mail: mail.into(),
                                id: d.id.clone()
                            })),
                        )
                    ]
                    .spacing(space::s())
                    .wrap(),
                );
            if let Some(text) = state.problems.get(&d.id).or_else(|| state.feedback.get(&d.id)) {
                entry = entry.push(typo::text(typo::meta(), text.clone()));
            }
        }
        lines = lines.push(entry);
    }
    let lines = container(scrollable(lines).style(style::scrollable)).max_height(size::control() * 3.0);
    let action = widgets::text_button(
        None,
        tr("Actions · F6"),
        style::button::ghost,
        (!state.actions(mail).is_empty()).then(|| AppMessage::WorkflowDocument(Message::Toggle(mail.into()))),
    );
    let block = widgets::file_block(
        widgets::file_block_head(
            tr("Retrieved documents"),
            String::new(),
            Some(widgets::anchored(&state.anchor, action)),
        ),
        lines,
    );
    Some(container(block).width(Fill).padding(layout::panel_padding()).into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn f6_arrows_enter_and_dismissal_are_scoped_to_the_visible_mail() {
        use iced::keyboard::key::Named;
        let Some(Response::WorkflowRuns { mut runs }) =
            automation::DemoState::default().answer(&Request::ListWorkflowRuns { workflow: None, mail: None })
        else {
            panic!("demo runs")
        };
        let mut run = runs.remove(0);
        run.mail = "m:current".into();
        let document = run.document.as_ref().expect("document").id.clone();
        let mut state = State::default();
        state.runs.insert(run.mail.clone(), vec![run.clone()]);
        assert!(state.update("m:current", Message::Key(Named::F6)).is_none());
        assert!(state.menu_open());
        assert!(state.update("m:current", Message::Key(Named::ArrowDown)).is_none());
        assert_eq!(
            state.update("m:current", Message::Key(Named::Enter)),
            Some(Request::ShowWorkflowDocument { id: document.clone() })
        );
        assert!(!state.menu_open());
        assert!(state.update("m:other", Message::Open { mail: "m:current".into(), id: document.clone() }).is_none());
        assert!(state.update("m:other", Message::Open { mail: "m:other".into(), id: document.clone() }).is_none());
        state.update("m:current", Message::Key(Named::F6));
        state.follow_mail(Some("m:other"));
        assert!(!state.menu_open());
        assert!(state.update("m:other", Message::Key(Named::Enter)).is_none());
        state.update("m:current", Message::Key(Named::F6));
        state.update("m:current", Message::Close);
        assert!(!state.menu_open());
        assert!(state.begin(&Request::OpenWorkflowDocument { id: document.clone() }));
        assert!(state.actions("m:current").is_empty(), "busy documents are absent");
        assert!(state.update("m:current", Message::Show { mail: "m:current".into(), id: document.clone() }).is_none());
        state.pending.clear();
        for (missing, changed) in [(true, false), (false, true)] {
            let d = run.document.as_mut().expect("document");
            d.missing = missing;
            d.changed = changed;
            state.runs.insert(run.mail.clone(), vec![run.clone()]);
            assert!(state.actions("m:current").is_empty());
            assert!(
                state.update("m:current", Message::Open { mail: "m:current".into(), id: document.clone() }).is_none()
            );
        }
        run.mail = "m:foreign".into();
        state.runs.insert("m:current".into(), vec![run]);
        assert!(state.actions("m:current").is_empty(), "even a malformed list cannot cross mail scope");
    }
    #[test]
    fn document_actions_carry_only_an_id_and_errors_are_kept() {
        let request = Request::OpenWorkflowDocument { id: "document1".into() };
        assert_eq!(request, Request::OpenWorkflowDocument { id: "document1".into() });
        let mut state = State::default();
        assert!(state.begin(&request));
        assert!(!state.begin(&request));
        state.answered(&request, &Ok(Response::Error { code: "changed".into(), message: "File changed".into() }));
        assert!(state.problems.contains_key("document1"));
        assert!(state.begin(&request));
    }
}
