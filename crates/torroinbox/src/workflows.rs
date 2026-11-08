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

#[derive(Debug, Default)]
pub struct State {
    runs: HashMap<String, Vec<WorkflowRun>>,
    problems: HashMap<String, String>,
    pending: HashSet<String>,
    feedback: HashMap<String, String>,
}
#[derive(Debug, Clone)]
pub enum Message {
    Open(String),
    Show(String),
}
impl Message {
    pub fn request(self) -> Request {
        match self {
            Self::Open(id) => Request::OpenWorkflowDocument { id },
            Self::Show(id) => Request::ShowWorkflowDocument { id },
        }
    }
}
fn key(request: &Request) -> Option<String> {
    match request {
        Request::ListWorkflowRuns { mail: Some(mail), .. } => Some(mail.clone()),
        Request::OpenWorkflowDocument { id } | Request::ShowWorkflowDocument { id } => Some(id.clone()),
        _ => None,
    }
}
impl State {
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
    for run in runs {
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
                            valid.then(|| AppMessage::WorkflowDocument(Message::Open(d.id.clone()))),
                        ),
                        widgets::text_button(
                            None,
                            tr("Show in folder"),
                            style::button::ghost,
                            valid.then(|| AppMessage::WorkflowDocument(Message::Show(d.id.clone()))),
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
    let block = widgets::file_block(widgets::file_block_head(tr("Retrieved documents"), String::new(), None), lines);
    Some(container(block).width(Fill).padding(layout::panel_padding()).into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_actions_carry_only_an_id_and_errors_are_kept() {
        let request = Message::Open("document1".into()).request();
        assert_eq!(request, Request::OpenWorkflowDocument { id: "document1".into() });
        let mut state = State::default();
        assert!(state.begin(&request));
        assert!(!state.begin(&request));
        state.answered(&request, &Ok(Response::Error { code: "changed".into(), message: "File changed".into() }));
        assert!(state.problems.contains_key("document1"));
        assert!(state.begin(&request));
    }
}
