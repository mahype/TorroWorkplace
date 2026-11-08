//! Request-only setup and management of mail-document workflows, grants, rules and retrieved files.

use crate::parts::{chevron, field, notice, pressable, setting_row};
use crate::{Action, Data, Page};
use iced::widget::{column, row};
use iced::{Element, Fill};
use std::collections::VecDeque;
use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{Align, Dropdown, DropdownEvent, MenuItem, MenuSection};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::workflows::{
    DraftSpec, Provenance, RuleSpec, RunReason, RunState, Step, Trigger, Workflow, WorkflowPreview, WorkflowRule,
    WorkflowRun,
};
use torro_ipc::{Request, Response};

#[derive(Debug, Default)]
pub struct State {
    tab: usize,
    setup: bool,
    template_mode: bool,
    template: Option<String>,
    mail: String,
    account: Option<String>,
    target: String,
    instruction: String,
    search: String,
    examples: Vec<torro_mail::MessageSummary>,
    workflow: Option<Workflow>,
    edit: Option<DraftSpec>,
    rule: Option<WorkflowRule>,
    rule_form: Option<RuleSpec>,
    run: Option<WorkflowRun>,
    preview: Option<WorkflowPreview>,
    selected: Vec<String>,
    workflows: Option<Vec<Workflow>>,
    rules: Option<Vec<WorkflowRule>>,
    runs: Option<Vec<WorkflowRun>>,
    pending: Option<Request>,
    queued: VecDeque<Request>,
    confirm: Option<Request>,
    discard: bool,
    status: Option<String>,
    problem: Option<String>,
    menus: Dropdown<Menu>,
    cursor: usize,
    keyboard_list: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Menu {
    Actions,
    Workflow,
    Template,
    Target,
    Mail,
    Account,
    Category,
}

#[derive(Debug, Clone)]
pub enum Message {
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    TabFocus(bool),
    ChooseAction,
    OpenAi,
    OpenTargets,
    RepeatRun,
    Tab(usize),
    Back,
    Refresh,
    Poll,
    New { mail: String, account: Option<String>, template: bool },
    ChooseRun { mail: String, account: Option<String> },
    TemplateMode(bool),
    Template(String),
    Target(String),
    Mail(String),
    Search(String),
    Find,
    Instruction(String),
    Create,
    Open(String),
    Edit,
    Name(String),
    Description(String),
    LinkLabel(String),
    Host(String),
    StoreDraft,
    Preview,
    Test,
    Save,
    Run,
    Repeat,
    Duplicate,
    Enabled,
    Remove,
    Authorize,
    Revoke,
    NewRule,
    OpenRule(String),
    RuleWorkflow(String),
    Query(String),
    Account(String),
    Trigger(Trigger),
    Category(Option<torro_mail::Category>),
    PreviewRule,
    StoreRule,
    DuplicateRule,
    RuleEnabled,
    RemoveRule,
    SelectMail(String),
    RunSelected,
    OpenRun(String),
    CancelRun,
    OpenDocument(String),
    ShowDocument(String),
    Confirm,
    Discard,
    Cancel,
    Menus(DropdownEvent<Menu>),
}

fn ask<A>(state: &mut State, request: Request) -> Action<A> {
    if state.pending.is_some() {
        return Action::None;
    }
    state.problem = None;
    state.status = Some(tr("Working…").into());
    state.pending = Some(request.clone());
    Action::Ask(request)
}

fn confirm<A>(state: &mut State, request: Request) -> Action<A> {
    if state.pending.is_none() {
        state.confirm = Some(request);
    }
    Action::None
}

pub fn next_request(state: &mut State) -> Option<Request> {
    if state.pending.is_some() {
        return None;
    }
    let request = state.queued.pop_front()?;
    state.pending = Some(request.clone());
    state.status = Some(tr("Working…").into());
    Some(request)
}

/// Late initial loads fill choices without replacing a draft that is already being edited.
pub fn loaded(state: &mut State, data: &Data) {
    if state.template.is_none() {
        state.template = data.workflow_templates.first().map(|t| t.id.clone());
    }
    if state.target.is_empty() {
        state.target = data.save_targets.first().map(|t| t.id.clone()).unwrap_or_default();
    }
}

/// Keep typed forms when the settings close, but never replay a request whose answer belonged to
/// the old host. The service's durable run list is the authority after reopening.
pub fn keep(mut state: State) -> State {
    if state.pending.take().is_some() {
        state.status = Some(tr("The previous action may still be running. Refresh before trying it again.").into());
    }
    state.queued.clear();
    state.confirm = None;
    state.discard = false;
    state.menus.close();
    state
}

pub fn escape(state: &State) -> Option<Message> {
    if state.menus.open().is_some() {
        return Some(Message::Menus(DropdownEvent::Close));
    }
    if state.confirm.is_some() || state.discard {
        return Some(Message::Cancel);
    }
    (state.setup || state.workflow.is_some() || state.rule_form.is_some() || state.run.is_some())
        .then_some(Message::Back)
}

fn workflows<'a>(state: &'a State, data: &'a Data) -> &'a [Workflow] {
    state.workflows.as_deref().unwrap_or(&data.workflows)
}
fn rules<'a>(state: &'a State, data: &'a Data) -> &'a [WorkflowRule] {
    state.rules.as_deref().unwrap_or(&data.workflow_rules)
}
fn runs<'a>(state: &'a State, data: &'a Data) -> &'a [WorkflowRun] {
    state.runs.as_deref().unwrap_or(&data.workflow_runs)
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    if let Message::Key(key, modifiers) = message {
        return keyboard(state, key, modifiers, data);
    }
    if matches!(message, Message::ChooseAction) {
        let actions = filtered_actions(state, data);
        let Some((_, action)) = actions.get(state.cursor).or_else(|| actions.first()).cloned() else {
            return Action::None;
        };
        state.menus.close();
        return update(state, action, data);
    }
    if let Message::Menus(event) = message {
        state.cursor = 0;
        return if state.menus.update(event) { Action::Focus(widgets::DROPDOWN_SEARCH) } else { Action::None };
    }
    state.menus.close();
    // A late result must never land in a different form. Edits/navigation wait for its answer.
    if state.pending.is_some() {
        return Action::None;
    }
    match message {
        Message::OpenAi => return Action::Open("ai"),
        Message::OpenTargets => return Action::Open(super::ATTACHMENTS_SECTION),
        Message::RepeatRun => {
            if let Some(run) = &state.run {
                return confirm(
                    state,
                    Request::RunWorkflow {
                        id: run.workflow.clone(),
                        revision: run.revision,
                        mail: run.mail.clone(),
                        test: run.test,
                        repeat: true,
                    },
                );
            }
        }
        Message::Back => {
            let dirty = state.edit.as_ref().is_some_and(|edit| state.workflow.as_ref().is_none_or(|w| w.spec != *edit))
                || state
                    .rule_form
                    .as_ref()
                    .is_some_and(|spec| state.rule.as_ref().map_or(!spec.query.is_empty(), |r| r.spec != *spec))
                || state.setup && !state.instruction.is_empty();
            if dirty {
                state.discard = true;
                return Action::None;
            }
            state.setup = false;
            state.workflow = None;
            state.edit = None;
            state.rule = None;
            state.rule_form = None;
            state.run = None;
            state.preview = None;
            state.confirm = None;
            state.selected.clear();
        }
        Message::Discard => {
            state.discard = false;
            state.edit = None;
            state.rule_form = None;
            state.instruction.clear();
            state.setup = false;
            return update(state, Message::Back, data);
        }
        Message::Tab(tab) => {
            state.tab = tab.min(2);
            state.mail.clear();
            state.cursor = 0;
            state.keyboard_list = false;
        }
        Message::Refresh => {
            state.queued.extend([Request::ListWorkflowRules, Request::ListWorkflowRuns { workflow: None, mail: None }]);
            return ask(state, Request::ListWorkflows);
        }
        Message::Poll => {
            if let Some(run) = &state.run
                && active(run.state)
            {
                return ask(state, Request::GetWorkflowRun { id: run.id.clone() });
            }
            return update(state, Message::Refresh, data);
        }
        Message::New { mail, account, template } => {
            state.setup = true;
            state.workflow = None;
            state.edit = None;
            state.preview = None;
            state.mail = mail;
            state.account = account;
            state.template_mode = template;
            state.target = data.save_targets.first().map(|t| t.id.clone()).unwrap_or_default();
            state.template = data.workflow_templates.first().map(|t| t.id.clone());
            state.status = None;
            state.problem = None;
            return ask(state, search_request(String::new()));
        }
        Message::ChooseRun { mail, account } => {
            state.mail = mail;
            state.account = account;
            state.tab = 0;
            state.setup = false;
            state.workflow = None;
            return ask(state, Request::ListWorkflows);
        }
        Message::TemplateMode(on) => state.template_mode = on,
        Message::Template(id) => {
            state.template = Some(id);
            state.template_mode = true;
        }
        Message::Target(id) => {
            state.target = id.clone();
            if let Some(edit) = &mut state.edit {
                edit.save_target = id;
            }
            state.preview = None;
        }
        Message::Mail(id) => {
            state.mail = id;
            state.preview = None;
        }
        Message::Search(query) => state.search = query,
        Message::Find => return ask(state, search_request(state.search.clone())),
        Message::Instruction(text) => state.instruction = text,
        Message::Create if !state.mail.is_empty() && data.save_targets.iter().any(|t| t.id == state.target) => {
            if state.template_mode {
                let Some(template) = data.workflow_templates.iter().find(|t| Some(&t.id) == state.template.as_ref())
                else {
                    return Action::None;
                };
                return ask(
                    state,
                    Request::InstantiateWorkflowTemplate {
                        template_id: template.id.clone(),
                        revision: template.revision,
                        digest: template.digest.clone(),
                        mail: state.mail.clone(),
                        save_target: state.target.clone(),
                    },
                );
            }
            if data.settings.ai_slots.chat.is_none() {
                return Action::None;
            }
            return ask(
                state,
                Request::ProposeWorkflow {
                    mail: state.mail.clone(),
                    save_target: state.target.clone(),
                    instruction: state.instruction.clone(),
                },
            );
        }
        Message::Open(id) => {
            if let Some(workflow) = workflows(state, data).iter().find(|w| w.id == id).cloned() {
                if state.mail.is_empty() {
                    state.mail = workflow.spec.mail.clone();
                }
                state.target = workflow.spec.save_target.clone();
                state.workflow = Some(workflow);
                state.preview = None;
                state.edit = None;
                state.run = None;
                state.rule_form = None;
                state.rule = None;
            } else {
                state.run = None;
                return ask(state, Request::GetWorkflow { id });
            }
        }
        Message::Edit => {
            state.edit = state.workflow.as_ref().map(|w| w.spec.clone());
            state.preview = None;
        }
        Message::Name(text) => {
            if let Some(edit) = &mut state.edit {
                edit.name = text;
            }
        }
        Message::Description(text) => {
            if let Some(edit) = &mut state.edit {
                edit.description = text;
            }
        }
        Message::LinkLabel(text) => {
            if let Some(edit) = &mut state.edit
                && let Some(Step::MailLink { label, .. }) = edit.package.steps.first_mut()
            {
                *label = (!text.is_empty()).then_some(text);
            }
        }
        Message::Host(host) => {
            if let Some(edit) = &mut state.edit
                && direct_package(&edit.package)
            {
                edit.package.hosts = vec![host.clone()];
                if let Some(Step::MailLink { host: current, .. }) = edit.package.steps.first_mut() {
                    *current = host;
                }
            }
        }
        Message::StoreDraft => {
            if let (Some(w), Some(spec)) = (&state.workflow, &state.edit) {
                if spec.package.validate().is_err() || spec.name.trim().is_empty() {
                    state.problem = Some(tr("Enter a name and a supported workflow with exact domains.").into());
                    return Action::None;
                }
                return ask(
                    state,
                    Request::UpdateWorkflowDraft { id: w.id.clone(), revision: w.revision, spec: spec.clone() },
                );
            }
        }
        Message::Preview => {
            if let Some(w) = &state.workflow {
                return ask(
                    state,
                    Request::PreviewWorkflowRun { id: w.id.clone(), revision: w.revision, mail: state.mail.clone() },
                );
            }
        }
        Message::Test | Message::Run | Message::Repeat => {
            if let Some(w) = &state.workflow {
                let test = matches!(message, Message::Test);
                if test && !preview_matches(state, w) {
                    return Action::None;
                }
                return confirm(
                    state,
                    Request::RunWorkflow {
                        id: w.id.clone(),
                        revision: w.revision,
                        mail: state.mail.clone(),
                        test,
                        repeat: matches!(message, Message::Repeat),
                    },
                );
            }
        }
        Message::Save => {
            if let Some(w) = &state.workflow
                && w.tested
            {
                return ask(state, Request::SaveWorkflow { id: w.id.clone(), revision: w.revision });
            }
        }
        Message::Duplicate => {
            if let Some(w) = &state.workflow {
                return ask(state, Request::DuplicateWorkflow { id: w.id.clone(), revision: w.revision });
            }
        }
        Message::Enabled => {
            if let Some(w) = &state.workflow {
                return ask(
                    state,
                    Request::SetWorkflowEnabled { id: w.id.clone(), revision: w.revision, enabled: !w.enabled },
                );
            }
        }
        Message::Remove => {
            if let Some(w) = &state.workflow {
                return confirm(state, Request::RemoveWorkflow { id: w.id.clone(), revision: w.revision });
            }
        }
        Message::Authorize => {
            if let Some(w) = &state.workflow
                && w.saved
                && w.tested
            {
                return confirm(state, Request::AuthorizeWorkflow { id: w.id.clone(), revision: w.revision });
            }
        }
        Message::Revoke => {
            if let Some(w) = &state.workflow {
                return confirm(state, Request::RevokeWorkflowAuthorization { id: w.id.clone() });
            }
        }
        Message::NewRule => {
            if let Some(w) = &state.workflow
                && w.saved
            {
                state.rule = None;
                state.rule_form = Some(RuleSpec {
                    workflow: w.id.clone(),
                    account: w.account.clone(),
                    trigger: Trigger::NewMail,
                    query: String::new(),
                    category: None,
                });
                state.workflow = None;
                state.preview = None;
                state.tab = 1;
            }
        }
        Message::OpenRule(id) => {
            if let Some(rule) = rules(state, data).iter().find(|r| r.id == id).cloned() {
                state.rule_form = Some(rule.spec.clone());
                state.rule = Some(rule);
                state.preview = None;
            }
        }
        Message::RuleWorkflow(id) => {
            if let Some(workflow) = workflows(state, data).iter().find(|w| w.id == id && w.saved).cloned()
                && let Some(spec) = &mut state.rule_form
            {
                spec.workflow = workflow.id;
                spec.account = workflow.account;
                state.preview = None;
                state.selected.clear();
            }
        }
        Message::Query(text) => {
            if let Some(rule) = &mut state.rule_form {
                rule.query = text;
                state.preview = None;
                state.selected.clear();
            }
        }
        Message::Account(account) => {
            if let Some(rule) = &mut state.rule_form {
                rule.account = account;
                state.preview = None;
                state.selected.clear();
            }
        }
        Message::Trigger(trigger) => {
            if let Some(rule) = &mut state.rule_form {
                rule.trigger = trigger;
                state.preview = None;
                state.selected.clear();
            }
        }
        Message::Category(category) => {
            if let Some(rule) = &mut state.rule_form {
                rule.category = category;
                state.preview = None;
                state.selected.clear();
            }
        }
        Message::PreviewRule => {
            if let Some(spec) = &state.rule_form {
                return ask(state, Request::PreviewWorkflowRule { spec: spec.clone() });
            }
        }
        Message::StoreRule => {
            if let Some(spec) = &state.rule_form {
                let request = match &state.rule {
                    Some(rule) => {
                        Request::UpdateWorkflowRule { id: rule.id.clone(), revision: rule.revision, spec: spec.clone() }
                    }
                    None => Request::CreateWorkflowRule { spec: spec.clone() },
                };
                return ask(state, request);
            }
        }
        Message::DuplicateRule => {
            if let Some(r) = &state.rule {
                return ask(state, Request::DuplicateWorkflowRule { id: r.id.clone(), revision: r.revision });
            }
        }
        Message::RuleEnabled => {
            if let Some(r) = &state.rule {
                return confirm(
                    state,
                    Request::SetWorkflowRuleEnabled { id: r.id.clone(), revision: r.revision, enabled: !r.enabled },
                );
            }
        }
        Message::RemoveRule => {
            if let Some(r) = &state.rule {
                return confirm(state, Request::RemoveWorkflowRule { id: r.id.clone(), revision: r.revision });
            }
        }
        Message::SelectMail(id) => {
            if state.preview.as_ref().is_some_and(|p| p.matches.contains(&id)) {
                if state.selected.contains(&id) {
                    state.selected.retain(|held| *held != id);
                } else {
                    state.selected.push(id);
                }
            }
        }
        Message::RunSelected if !state.selected.is_empty() => {
            if let Some(p) = &state.preview {
                state.confirm = Some(Request::RunWorkflow {
                    id: p.workflow.clone(),
                    revision: p.revision,
                    mail: state.selected.first().cloned().unwrap_or_default(),
                    test: false,
                    repeat: false,
                });
            }
        }
        Message::OpenRun(id) => {
            state.run = runs(state, data).iter().find(|r| r.id == id).cloned();
            return ask(state, Request::GetWorkflowRun { id });
        }
        Message::CancelRun => {
            if let Some(run) = &state.run {
                return ask(state, Request::CancelWorkflowRun { id: run.id.clone() });
            }
        }
        Message::OpenDocument(id) => return ask(state, Request::OpenWorkflowDocument { id }),
        Message::ShowDocument(id) => return ask(state, Request::ShowWorkflowDocument { id }),
        Message::Confirm => {
            if let Some(request) = state.confirm.take() {
                if matches!(&request, Request::RunWorkflow { test: false, .. }) && !state.selected.is_empty() {
                    if let Some(p) = &state.preview {
                        for mail in state.selected.iter().skip(1) {
                            state.queued.push_back(Request::RunWorkflow {
                                id: p.workflow.clone(),
                                revision: p.revision,
                                mail: mail.clone(),
                                test: false,
                                repeat: false,
                            });
                        }
                    }
                    state.selected.clear();
                }
                return ask(state, request);
            }
        }
        Message::Cancel => {
            state.confirm = None;
            state.discard = false;
        }
        _ => {}
    }
    Action::None
}

fn preview_matches(state: &State, w: &Workflow) -> bool {
    state.preview.as_ref().is_some_and(|p| {
        p.workflow == w.id && p.revision == w.revision && p.mail == state.mail && p.save_target == w.spec.save_target
    })
}

fn search_request(query: String) -> Request {
    Request::SearchMail {
        account: None,
        folder: None,
        query: (!query.is_empty()).then_some(query),
        unread: None,
        flagged: None,
        since: None,
        before: None,
        limit: 100,
        cursor: None,
        order: None,
        sort: None,
        snoozed: None,
        categories: vec![],
        pile: None,
        caught: None,
    }
}

/// Iced 0.14 buttons have no focus. F6 opens a searchable, keyboard-operated equivalent of every
/// available page action and choice; ordinary text editing keeps captured keys.
pub fn subscription() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, status, _| {
        let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
            return None;
        };
        use iced::keyboard::{Key, key::Named};
        if key == Key::Named(Named::Tab) {
            return Some(Message::TabFocus(modifiers.shift()));
        }
        if key == Key::Named(Named::F6)
            || status == iced::event::Status::Ignored
                && matches!(key, Key::Named(Named::ArrowDown | Named::ArrowUp | Named::Enter | Named::ArrowLeft))
        {
            Some(Message::Key(key, modifiers))
        } else {
            None
        }
    })
}

fn keyboard<A>(
    state: &mut State,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    data: &Data,
) -> Action<A> {
    use iced::keyboard::{Key, key::Named};
    if state.pending.is_some() {
        return Action::None;
    }
    if state.confirm.is_some() || state.discard {
        return if key == Key::Named(Named::Enter) {
            update(state, if state.discard { Message::Discard } else { Message::Confirm }, data)
        } else {
            Action::None
        };
    }
    if key == Key::Named(Named::F6) {
        return update(state, Message::Menus(DropdownEvent::Toggle(Menu::Actions)), data);
    }
    if modifiers.alt() && key == Key::Named(Named::ArrowLeft) {
        return update(state, Message::Back, data);
    }
    let in_actions = state.menus.is_open(&Menu::Actions);
    if state.menus.open().is_some() && !in_actions {
        return Action::None;
    }
    let actions = if in_actions { filtered_actions(state, data) } else { list_actions(state, data) };
    if matches!(key, Key::Named(Named::ArrowUp | Named::ArrowDown)) && !actions.is_empty() {
        state.cursor = if !in_actions && !state.keyboard_list {
            0
        } else if key == Key::Named(Named::ArrowUp) {
            state.cursor.saturating_sub(1)
        } else {
            (state.cursor + 1).min(actions.len().saturating_sub(1))
        };
        state.keyboard_list = true;
    } else if key == Key::Named(Named::Enter) {
        if in_actions {
            return update(state, Message::ChooseAction, data);
        }
        if let Some((_, message)) = actions.get(state.cursor).cloned() {
            return update(state, message, data);
        }
    }
    Action::None
}

fn list_actions(state: &State, data: &Data) -> Vec<(String, Message)> {
    if state.setup || state.workflow.is_some() || state.rule_form.is_some() || state.run.is_some() {
        return vec![];
    }
    match state.tab {
        0 => {
            let mut entries: Vec<_> = workflows(state, data).iter().collect();
            entries.sort_by_key(|w| state.account.as_ref().is_some_and(|a| *a != w.account));
            entries.into_iter().map(|w| (w.spec.name.clone(), Message::Open(w.id.clone()))).collect()
        }
        1 => rules(state, data).iter().map(|r| (r.spec.query.clone(), Message::OpenRule(r.id.clone()))).collect(),
        _ => runs(state, data).iter().map(|r| (run_status(r), Message::OpenRun(r.id.clone()))).collect(),
    }
}

fn filtered_actions(state: &State, data: &Data) -> Vec<(String, Message)> {
    let query = state.menus.query().to_lowercase();
    actions(state, data).into_iter().filter(|(label, _)| label.to_lowercase().contains(&query)).collect()
}

fn actions(state: &State, data: &Data) -> Vec<(String, Message)> {
    if state.pending.is_some() {
        return vec![];
    }
    let mut result = vec![(tr("Refresh").into(), Message::Refresh), (tr("Back").into(), Message::Back)];
    let mut add = |label: &'static str, message: Message| result.push((tr(label).into(), message));
    if state.setup {
        add("AI draft", Message::TemplateMode(false));
        add("Bundled template", Message::TemplateMode(true));
        add("Search mail", Message::Find);
        add("Create draft", Message::Create);
    } else if let Some(spec) = &state.rule_form {
        add("Preview existing mail", Message::PreviewRule);
        add("Save inactive rule", Message::StoreRule);
        add("New mail", Message::Trigger(Trigger::NewMail));
        add("Category assigned", Message::Trigger(Trigger::CategoryAssigned));
        add("Any category", Message::Category(None));
        if !state.selected.is_empty() {
            add("Run on selected mail…", Message::RunSelected);
        }
        if let Some(rule) = &state.rule {
            if rule.spec == *spec
                && (rule.enabled
                    || workflows(state, data).iter().any(|w| w.id == spec.workflow && w.authorized && w.enabled))
            {
                add(if rule.enabled { "Disable rule…" } else { "Activate rule…" }, Message::RuleEnabled);
            }
            add("Duplicate rule", Message::DuplicateRule);
            add("Remove rule…", Message::RemoveRule);
        }
    } else if let Some(run) = &state.run {
        add("Open workflow", Message::Open(run.workflow.clone()));
        if active(run.state) {
            add("Cancel run", Message::CancelRun);
        } else {
            add("Repeat download…", Message::RepeatRun);
        }
        if let Some(d) = &run.document
            && !d.missing
            && !d.changed
        {
            add("Open", Message::OpenDocument(d.id.clone()));
            add("Show in folder", Message::ShowDocument(d.id.clone()));
        }
    } else if let Some(w) = &state.workflow {
        if state.edit.is_some() {
            add("Save draft changes", Message::StoreDraft);
        } else {
            add("Edit draft", Message::Edit);
            add("Duplicate", Message::Duplicate);
            add("Remove…", Message::Remove);
            if w.spec.package.validate().is_ok() && data.save_targets.iter().any(|t| t.id == w.spec.save_target) {
                add("Preview without download", Message::Preview);
                if preview_matches(state, w) {
                    add("Test with real download…", Message::Test);
                }
                if w.saved && w.enabled {
                    add("Run workflow…", Message::Run);
                    add("Repeat download…", Message::Repeat);
                }
            }
            if w.tested && !w.saved {
                add("Save workflow", Message::Save);
            }
            if w.saved {
                add(if w.enabled { "Disable" } else { "Enable" }, Message::Enabled);
                add(
                    if w.authorized { "Revoke authorization…" } else { "Authorize this revision…" },
                    if w.authorized { Message::Revoke } else { Message::Authorize },
                );
                add("Set up automatic execution…", Message::NewRule);
            }
        }
    } else {
        add("Workflows", Message::Tab(0));
        add("Rules", Message::Tab(1));
        add("Runs", Message::Tab(2));
        add(
            "Create workflow with AI…",
            Message::New { mail: state.mail.clone(), account: state.account.clone(), template: false },
        );
        add(
            "New from template…",
            Message::New { mail: state.mail.clone(), account: state.account.clone(), template: true },
        );
    }
    add("AI settings", Message::OpenAi);
    add("Manage save targets…", Message::OpenTargets);
    result.extend(list_actions(state, data));
    if state.setup || state.edit.is_some() {
        result.extend(
            data.save_targets
                .iter()
                .map(|t| (format!("{} · {}", tr("Save target"), t.spec.name), Message::Target(t.id.clone()))),
        );
    }
    if state.setup {
        result.extend(data.workflow_templates.iter().map(|t| (template_name(t), Message::Template(t.id.clone()))));
    }
    if state.setup || state.workflow.is_some() && state.edit.is_none() {
        result.extend(
            state
                .examples
                .iter()
                .map(|m| (format!("{} · {}", tr("Example mail"), m.subject), Message::Mail(m.id.clone()))),
        );
    }
    if state.rule_form.is_some() {
        result.extend(
            workflows(state, data)
                .iter()
                .filter(|w| w.saved)
                .map(|w| (trf("Workflow · {name}", &[("name", &w.spec.name)]), Message::RuleWorkflow(w.id.clone()))),
        );
        result.extend(
            data.accounts
                .iter()
                .filter(|a| a.mail.is_some())
                .map(|a| (a.label.clone(), Message::Account(a.id.clone()))),
        );
        result.extend(data.categories.iter().map(|c| (c.label.clone(), Message::Category(Some(c.category)))));
        if let Some(preview) = &state.preview {
            result.extend(preview.matches.iter().map(|id| (id.clone(), Message::SelectMail(id.clone()))));
        }
    }
    result
}

pub fn answered(state: &mut State, answer: &Result<Response, String>) {
    let Some(request) = state.pending.take() else {
        return;
    };
    state.status = None;
    let answer = match answer {
        Ok(Response::Error { code, message }) => {
            state.problem = Some(format!("{code}: {message}"));
            state.queued.clear();
            return;
        }
        Err(problem) => {
            state.problem = Some(problem.clone());
            state.queued.clear();
            return;
        }
        Ok(answer) => answer,
    };
    match answer {
        Response::WorkflowDraft { workflow } | Response::Workflow { workflow } => {
            state.workflow = Some(workflow.clone());
            state.setup = false;
            state.edit = None;
            state.preview = None;
        }
        Response::Workflows { workflows } => {
            if state.edit.is_none()
                && let Some(w) = &state.workflow
            {
                state.workflow = workflows.iter().find(|held| held.id == w.id).cloned();
            }
            state.workflows = Some(workflows.clone());
        }
        Response::WorkflowRules { rules } => {
            if let Some(rule) = &state.rule
                && state.rule_form.as_ref() == Some(&rule.spec)
            {
                state.rule = rules.iter().find(|held| held.id == rule.id).cloned();
                state.rule_form = state.rule.as_ref().map(|rule| rule.spec.clone());
            }
            state.rules = Some(rules.clone());
        }
        Response::WorkflowRule { rule } => {
            state.rule = Some(rule.clone());
            state.rule_form = Some(rule.spec.clone());
        }
        Response::WorkflowRuns { runs } => {
            if let Some(run) = &state.run {
                state.run = runs.iter().find(|r| r.id == run.id).cloned();
            }
            state.runs = Some(runs.clone());
        }
        Response::WorkflowRun { run } => {
            state.run = Some(run.clone());
            state.tab = 2;
            if run.test && run.state == RunState::Succeeded {
                state.queued.push_back(Request::GetWorkflow { id: run.workflow.clone() });
            }
        }
        Response::WorkflowPreview { preview } => {
            state.preview = Some(preview.clone());
            state.selected.clear();
        }
        Response::MailMessages { messages, .. } => state.examples = messages.clone(),
        Response::Pending { approval } => {
            state.status = Some(trf("Waiting for approval · {id}", &[("id", &approval.id)]));
            // Pending is not success. Never advance a batch before its actual approval.
            state.queued.clear();
            return;
        }
        Response::Ok => {
            state.status = Some(tr("Done").into());
            match request {
                Request::RemoveWorkflow { .. } => {
                    state.workflow = None;
                    state.edit = None;
                }
                Request::RemoveWorkflowRule { .. } => {
                    state.rule = None;
                    state.rule_form = None;
                }
                _ => {}
            }
        }
        _ => {
            state.problem = Some(tr("The service returned an unexpected workflow response.").into());
            state.queued.clear();
            return;
        }
    }
    if !matches!(
        request,
        Request::ListWorkflows
            | Request::ListWorkflowRules
            | Request::ListWorkflowRuns { .. }
            | Request::PreviewWorkflowRun { .. }
            | Request::PreviewWorkflowRule { .. }
            | Request::SearchMail { .. }
            | Request::GetWorkflowRun { .. }
            | Request::OpenWorkflowDocument { .. }
            | Request::ShowWorkflowDocument { .. }
    ) {
        state.queued.extend([
            Request::ListWorkflows,
            Request::ListWorkflowRules,
            Request::ListWorkflowRuns { workflow: None, mail: None },
        ]);
    }
}

fn button(label: &'static str, message: Message, enabled: bool) -> Element<'static, Message> {
    widgets::text_button(None, tr(label), style::button::secondary, enabled.then_some(message))
}
fn note(words: impl Into<String>) -> Element<'static, Message> {
    typo::text(typo::body(), words.into()).style(theme::text_2).width(Fill).into()
}
fn form_field<'a>(label: &'static str, value: &str, input: fn(String) -> Message) -> Element<'a, Message> {
    column![typo::text(typo::caption(), tr(label)).style(theme::text_3), field("", value).on_input(input)]
        .spacing(space::xs())
        .into()
}

pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
    let idle = state.pending.is_none();
    let mut content = column![].spacing(layout::section_gap()).width(Fill);
    if let Some(problem) = &state.problem {
        content = content.push(notice(problem.clone()));
    }
    if let Some(status) = &state.status {
        content = content.push(note(status.clone()));
    }
    let (body, title) = if state.setup {
        (setup(state, data), Some(tr("Set up workflow").to_string()))
    } else if let Some(spec) = &state.rule_form {
        (rule_page(state, data, spec), Some(tr("Automatic execution").to_string()))
    } else if let Some(run) = &state.run {
        (run_page(state, run), Some(tr("Workflow run").to_string()))
    } else if let Some(w) = &state.workflow {
        (workflow_page(state, data, w), Some(w.spec.name.clone()))
    } else {
        let tabs = widgets::segmented(
            [(tr("Workflows"), Message::Tab(0)), (tr("Rules"), Message::Tab(1)), (tr("Runs"), Message::Tab(2))],
            state.tab,
        );
        let mut list = column![tabs].spacing(space::l());
        if state.keyboard_list
            && let Some((name, _)) = list_actions(state, data).get(state.cursor)
        {
            list = list.push(note(trf("Keyboard selection · {name}", &[("name", name)])));
        }
        match state.tab {
            0 => {
                list = list.push(button(
                    "New from template…",
                    Message::New { mail: String::new(), account: None, template: true },
                    idle,
                ));
                if !state.mail.is_empty() {
                    list = list
                        .push(note(tr("Choose a workflow for the selected mail. Matching accounts are listed first.")));
                }
                let mut entries: Vec<_> = workflows(state, data).iter().collect();
                entries.sort_by_key(|w| state.account.as_ref().is_some_and(|a| *a != w.account));
                if entries.is_empty() {
                    list = list.push(note(tr(
                        "No workflows yet. Create an inactive draft from a mail or a bundled template.",
                    )));
                }
                for w in entries {
                    list = list.push(pressable(
                        setting_row(None, w.spec.name.clone(), Some(workflow_status(w)), chevron()),
                        idle.then(|| Message::Open(w.id.clone())),
                    ));
                }
            }
            1 => {
                list = list.push(note(tr(
                    "Create rules from a saved workflow. Saving or enabling a workflow never activates a rule.",
                )));
                for r in rules(state, data) {
                    list = list.push(pressable(
                        setting_row(
                            None,
                            r.spec.query.clone(),
                            Some(format!("{} · {}", r.spec.account, if r.enabled { tr("On") } else { tr("Off") })),
                            chevron(),
                        ),
                        idle.then(|| Message::OpenRule(r.id.clone())),
                    ));
                }
            }
            _ => {
                for run in runs(state, data) {
                    list = list.push(pressable(
                        setting_row(None, run_status(run), Some(format!("{} · {}", run.mail, run.created)), chevron()),
                        idle.then(|| Message::OpenRun(run.id.clone())),
                    ));
                }
                if runs(state, data).is_empty() {
                    list = list.push(note(tr("No workflow runs yet.")));
                }
            }
        }
        (list.into(), None)
    };
    content = content.push(body);
    let page = Page::new(content).max_width(size::content_max()).actions(vec![state.menus.button(
        Menu::Actions,
        tr("Actions · F6"),
        Message::Menus,
    )]);
    match title {
        Some(title) => page.sub_page(title, Message::Back),
        None => page,
    }
}

fn setup<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let idle = state.pending.is_none();
    let modes = row![
        widgets::choice_chip(tr("AI draft"), None, !state.template_mode, Some(Message::TemplateMode(false))),
        widgets::choice_chip(tr("Bundled template"), None, state.template_mode, Some(Message::TemplateMode(true)))
    ]
    .spacing(space::s())
    .wrap();
    let mut page =
        column![note(tr("Download document")), modes, mail_field(state), target_field(state, data)].spacing(space::l());
    if state.template_mode {
        let template = data.workflow_templates.iter().find(|t| Some(&t.id) == state.template.as_ref());
        let label = template.map_or_else(|| tr("Choose template").to_string(), template_name);
        page = page.push(state.menus.button_fill(Menu::Template, label, Message::Menus));
        if let Some(t) = template {
            page = page.push(note(template_description(t))).push(note(trf("Version {n}", &[("n", &t.revision)])));
            if t.id == "direct_pdf" {
                page = page.push(note(tr("The draft preview shows the exact domain from the selected mail.")));
            } else {
                page = page.push(package_view(&t.package));
            }
        }
    } else {
        if data.settings.ai_slots.chat.is_none() {
            page = page.push(note(tr(
                "No chat model is configured. Choose a model in AI settings or use a bundled template.",
            )));
            page = page.push(button("AI settings", Message::OpenAi, idle));
        }
        if let Some(chat) = &data.settings.ai_slots.chat {
            page = page
                .push(note(trf("Chat model · {model}", &[("model", &chat.model.as_deref().unwrap_or(&chat.engine))])));
        }
        page = page.push(form_field("Additional instructions", &state.instruction, Message::Instruction));
    }
    page.push(note(tr("Creating a draft performs no download and grants no execution rights. Login, browser actions, scripts and other tasks are not supported.")))
        .push(button("Create draft", Message::Create, idle && !state.mail.is_empty() && data.save_targets.iter().any(|t| t.id == state.target) && (if state.template_mode { state.template.is_some() } else { data.settings.ai_slots.chat.is_some() }))).into()
}

fn mail_field(state: &State) -> Element<'_, Message> {
    let label =
        state.examples.iter().find(|m| m.id == state.mail).map(|m| m.subject.clone()).unwrap_or_else(|| {
            if state.mail.is_empty() { tr("Choose example mail").into() } else { state.mail.clone() }
        });
    column![
        typo::text(typo::caption(), tr("Example mail")).style(theme::text_3),
        row![
            field(tr("Search mail"), &state.search).on_input(Message::Search).on_submit(Message::Find),
            button("Search", Message::Find, state.pending.is_none())
        ]
        .spacing(space::s()),
        state.menus.button_fill(Menu::Mail, label, Message::Menus)
    ]
    .spacing(space::xs())
    .into()
}

fn target_field<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let target = data.save_targets.iter().find(|t| t.id == state.target);
    let label = target.map_or_else(|| tr("Choose an existing save target").into(), |t| t.spec.name.clone());
    let mut content = column![
        typo::text(typo::caption(), tr("Save target")).style(theme::text_3),
        state.menus.button_fill(Menu::Target, label, Message::Menus)
    ]
    .spacing(space::xs());
    if data.save_targets.is_empty() {
        content = content.push(note(tr("No save target is available. Create one under Attachments first.")));
        content = content.push(button("Manage save targets…", Message::OpenTargets, state.pending.is_none()));
    } else if target.is_none() {
        content = content.push(note(tr("The save target is missing. Choose an existing target before testing.")));
    }
    content.into()
}

fn workflow_page<'a>(state: &'a State, data: &'a Data, w: &'a Workflow) -> Element<'a, Message> {
    let idle = state.pending.is_none();
    let mut page =
        column![note(workflow_status(w)), note(trf("Revision {n}", &[("n", &w.revision)]))].spacing(space::l());
    if let Some(spec) = &state.edit {
        let label = spec
            .package
            .steps
            .first()
            .and_then(|s| match s {
                Step::MailLink { label, .. } => label.as_deref(),
                _ => None,
            })
            .unwrap_or_default();
        page = page
            .push(form_field("Name", &spec.name, Message::Name))
            .push(form_field("Description", &spec.description, Message::Description))
            .push(target_field(state, data))
            .push(form_field("Link label", label, Message::LinkLabel));
        if direct_package(&spec.package)
            && let Some(host) = spec.package.hosts.first()
        {
            page = page.push(form_field("Exact domain", host, Message::Host));
        }
        return page
            .push(note(tr(
                "Editing invalidates the test and authorization. Rules are not updated or enabled automatically.",
            )))
            .push(button("Save draft changes", Message::StoreDraft, idle))
            .into();
    }
    page = page.push(note(w.spec.description.clone())).push(package_view(&w.spec.package));
    if let Some(origin) = &w.template_origin {
        page = page.push(note(trf("Template {id} · version {n}", &[("id", &origin.id), ("n", &origin.revision)])));
    }
    let target = data.save_targets.iter().find(|t| t.id == w.spec.save_target);
    page = page.push(mail_field(state)).push(note(trf(
        "Save target · {target}",
        &[("target", &target.map_or_else(|| tr("Missing save target").to_string(), |t| t.spec.name.clone()))],
    )));
    if let Some(preview) = &state.preview {
        page = page.push(preview_view(preview));
    }
    let supported = w.spec.package.validate().is_ok();
    if !supported {
        page = page.push(note(tr("This workflow version or capability is unsupported. No execution is available.")));
    }
    page = page.push(
        row![
            button("Preview without download", Message::Preview, idle && target.is_some() && supported),
            button(
                "Test with real download…",
                Message::Test,
                idle && target.is_some() && supported && preview_matches(state, w)
            ),
            button("Save workflow", Message::Save, idle && w.tested && !w.saved)
        ]
        .spacing(space::s())
        .wrap(),
    );
    page = page.push(note(tr("Preview is local. Testing downloads and writes a real document. Saving does not authorize execution or activate automation.")));
    if w.saved {
        page = page.push(
            row![
                button("Run workflow…", Message::Run, idle && w.enabled && supported && target.is_some()),
                button("Repeat download…", Message::Repeat, idle && w.enabled && supported && target.is_some())
            ]
            .spacing(space::s())
            .wrap(),
        );
    }
    page = page.push(
        row![
            button("Edit draft", Message::Edit, idle),
            button("Duplicate", Message::Duplicate, idle),
            button(if w.enabled { "Disable" } else { "Enable" }, Message::Enabled, idle && w.saved),
            button("Remove…", Message::Remove, idle)
        ]
        .spacing(space::s())
        .wrap(),
    );
    if w.saved {
        page = page.push(widgets::group(tr("Automatic execution"), vec![note(tr("Authorization and rule activation are separate. Only verified senders and confirmed exact domains can run automatically; initial mail inventories are never processed.")), row![button(if w.authorized { "Revoke authorization…" } else { "Authorize this revision…" }, if w.authorized { Message::Revoke } else { Message::Authorize }, idle && w.tested), button("Set up automatic execution…", Message::NewRule, idle)].spacing(space::s()).wrap().into()], None));
    }
    page.into()
}

fn package_view(package: &torro_ipc::workflows::Package) -> Element<'static, Message> {
    let mut steps = column![note(trf("Exact domains · {hosts}", &[("hosts", &package.hosts.join(", "))])), note(tr("Access: read this mail, HTTPS GET on these domains, write PDF to the selected save target. No login or payment actions."))].spacing(space::s());
    for (index, step) in package.steps.iter().enumerate() {
        let label = match step {
            Step::MailLink { .. } => tr("Select link from mail"),
            Step::Get => tr("Download document"),
            Step::HtmlLink { .. } => tr("Select document link from HTML"),
            Step::ResolveStripeInvoicePdf { .. } => tr("Resolve original Stripe invoice PDF"),
            Step::RenderInvoiceCopy { .. } => tr("Generate marked PDF copy from invoice HTML"),
            Step::VerifyPdf => tr("Verify PDF"),
            Step::SaveToTarget => tr("Save to target"),
        };
        steps = steps.push(note(format!("{}. {label}", index + 1)));
    }
    steps.into()
}
fn direct_package(package: &torro_ipc::workflows::Package) -> bool {
    matches!(package.steps.as_slice(), [Step::MailLink { .. }, Step::Get, Step::VerifyPdf, Step::SaveToTarget])
}
fn preview_view(p: &WorkflowPreview) -> Element<'static, Message> {
    column![
        note(tr("Local preview — no download or file write")),
        note(p.hosts.join(", ")),
        note(trf("Save target · {target}", &[("target", &p.save_target)])),
        note(if p.authorization_required {
            tr("Execution requires authorization.")
        } else {
            tr("Execution is authorized for this revision.")
        })
    ]
    .spacing(space::s())
    .into()
}

fn rule_page<'a>(state: &'a State, data: &'a Data, spec: &'a RuleSpec) -> Element<'a, Message> {
    let idle = state.pending.is_none();
    let account = data
        .accounts
        .iter()
        .find(|a| a.id == spec.account)
        .map_or_else(|| tr("Missing account").to_string(), |a| a.label.clone());
    let category = spec.category.map_or_else(|| tr("Any category").to_string(), |c| data.category_name(c));
    let name = workflows(state, data)
        .iter()
        .find(|w| w.id == spec.workflow)
        .map_or_else(|| spec.workflow.clone(), |w| w.spec.name.clone());
    let workflow = workflows(state, data).iter().find(|w| w.id == spec.workflow);
    let target = workflow.and_then(|w| data.save_targets.iter().find(|t| t.id == w.spec.save_target));
    let authorized = workflow.is_some_and(|w| w.authorized && w.enabled);
    let mut page = column![
        state.menus.button_fill(Menu::Workflow, trf("Workflow · {name}", &[("name", &name)]), Message::Menus),
        note(trf("Save target · {target}", &[("target", &target.map_or_else(|| tr("Missing save target").to_string(), |t| t.spec.name.clone()))])),
        state.menus.button_fill(Menu::Account, account, Message::Menus),
        row![
            widgets::choice_chip(tr("New mail"), None, spec.trigger == Trigger::NewMail, Some(Message::Trigger(Trigger::NewMail))),
            widgets::choice_chip(tr("Category assigned"), None, spec.trigger == Trigger::CategoryAssigned, Some(Message::Trigger(Trigger::CategoryAssigned)))
        ].spacing(space::s()).wrap(),
        form_field("Search condition", &spec.query, Message::Query),
        state.menus.button_fill(Menu::Category, category, Message::Menus),
        note(tr("Rules are saved inactive. Activation never processes existing mail. Catch-up is limited to seven days; unverified senders wait for manual review.")),
        row![button("Preview existing mail", Message::PreviewRule, idle), button("Save inactive rule", Message::StoreRule, idle)].spacing(space::s()).wrap()
    ].spacing(space::l());
    if let Some(preview) = &state.preview {
        page = page.push(preview_view(preview));
        for id in &preview.matches {
            page = page.push(widgets::choice_chip(
                id.clone(),
                None,
                state.selected.contains(id),
                Some(Message::SelectMail(id.clone())),
            ));
        }
        if preview.matches.is_empty() {
            page = page.push(note(tr("No matching existing mail.")));
        }
        page = page.push(button("Run on selected mail…", Message::RunSelected, idle && !state.selected.is_empty()));
    }
    if let Some(rule) = &state.rule {
        page = page.push(note(if rule.enabled { tr("Rule is on") } else { tr("Rule is off") })).push(
            row![
                button(
                    if rule.enabled { "Disable rule…" } else { "Activate rule…" },
                    Message::RuleEnabled,
                    idle && rule.spec == *spec && (rule.enabled || authorized)
                ),
                button("Duplicate rule", Message::DuplicateRule, idle),
                button("Remove rule…", Message::RemoveRule, idle)
            ]
            .spacing(space::s())
            .wrap(),
        );
    }
    if !authorized {
        page = page.push(note(tr("Authorize and enable the workflow before activating this rule.")));
    }
    page.into()
}

fn run_page<'a>(state: &'a State, run: &'a WorkflowRun) -> Element<'a, Message> {
    let mut page =
        column![note(run_status(run)), note(run.mail.clone()), note(trf("Attempt {n}", &[("n", &run.attempts)]))]
            .spacing(space::l());
    if let Some(document) = &run.document {
        page = page.push(document_view(document));
    }
    if active(run.state) {
        page = page.push(button("Cancel run", Message::CancelRun, state.pending.is_none()));
    }
    page = page.push(button("Open workflow", Message::Open(run.workflow.clone()), state.pending.is_none()));
    if !active(run.state) {
        page = page.push(button("Repeat download…", Message::RepeatRun, state.pending.is_none()));
    }
    page.into()
}
pub fn document_view(d: &torro_ipc::workflows::RetrievedDocument) -> Element<'static, Message> {
    let valid = !d.missing && !d.changed;
    let status = if d.missing {
        tr("File missing — not downloaded again automatically")
    } else if d.changed {
        tr("File changed — opening is blocked")
    } else {
        provenance(d.provenance)
    };
    column![
        note(d.relative_path.clone()),
        note(status),
        note(format!("{} · {}", d.source_host, torro_gui::units::format_bytes(d.bytes))),
        row![
            button("Open", Message::OpenDocument(d.id.clone()), valid),
            button("Show in folder", Message::ShowDocument(d.id.clone()), valid)
        ]
        .spacing(space::s())
        .wrap()
    ]
    .spacing(space::s())
    .into()
}
pub fn provenance(value: Provenance) -> &'static str {
    match value {
        Provenance::SupplierPdf => tr("Supplier PDF"),
        Provenance::HtmlCopy => tr("PDF copy generated from HTML"),
    }
}
pub fn active(state: RunState) -> bool {
    matches!(
        state,
        RunState::Queued | RunState::Running | RunState::WaitingBody | RunState::WaitingCategory | RunState::Paused
    )
}
pub fn run_status(run: &WorkflowRun) -> String {
    let status = match run.state {
        RunState::Queued => tr("Queued"),
        RunState::Running => tr("Running"),
        RunState::WaitingBody => tr("Waiting for mail content"),
        RunState::WaitingCategory => tr("Waiting for category"),
        RunState::Paused => tr("Paused"),
        RunState::Succeeded => tr("Succeeded"),
        RunState::Failed => tr("Failed"),
        RunState::Cancelled => tr("Cancelled"),
        RunState::Skipped => tr("Skipped"),
    };
    let reason = run.reason.map(|reason| match reason {
        RunReason::Expired => tr("The invoice link has expired."),
        RunReason::AuthRequired => tr("Sign in is required. Login automation is not supported."),
        RunReason::NotFound => tr("Document not found."),
        RunReason::UnexpectedType => tr("The response is not the expected document type."),
        RunReason::Unsupported => tr("This invoice format is not supported."),
        RunReason::Offline => tr("Offline"),
        RunReason::TargetUnavailable => tr("Save target unavailable."),
        RunReason::PermissionRevoked => tr("Execution permission was revoked."),
        RunReason::Changed => tr("The workflow or target changed. Test and authorize it again."),
        RunReason::Duplicate => tr("Already retrieved. Repeat only with explicit confirmation."),
        RunReason::TooOld => tr("Older than the seven-day catch-up limit."),
        RunReason::SenderUnverified => tr("Sender unverified — manual review required."),
        RunReason::RecoveryUncertain => tr("Recovery is uncertain. Partial results are preserved."),
    });
    match reason {
        Some(reason) => format!("{status} · {reason}"),
        None => status.into(),
    }
}
fn workflow_status(w: &Workflow) -> String {
    [
        if w.saved { tr("Saved") } else { tr("Draft") },
        if w.tested { tr("Tested") } else { tr("Not tested") },
        if w.authorized { tr("Authorized") } else { tr("Not authorized") },
        if w.enabled { tr("On") } else { tr("Off") },
    ]
    .join(" · ")
}
pub fn menu_only(state: &State) -> bool {
    !state.discard && state.confirm.is_none() && state.menus.open().is_some()
}
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if state.discard {
        return Some(crate::parts::confirm(
            tr("Discard changes?"),
            tr("Unsaved workflow or rule changes will be lost.").into(),
            tr("Discard"),
            Message::Discard,
            Message::Cancel,
        ));
    }
    if let Some(request) = &state.confirm {
        let consequence = match request {
            Request::RunWorkflow { test: true, .. } => {
                tr("This test downloads a real document and writes it to the displayed save target. Continue?")
            }
            Request::RunWorkflow { repeat: true, .. } => tr(
                "Download again as a new generation? Existing files remain; this is a real external download and write.",
            ),
            Request::RunWorkflow { .. } => {
                tr("Run on the explicitly selected mail? This downloads and writes real documents.")
            }
            Request::AuthorizeWorkflow { .. } => tr(
                "Authorize this tested revision for its account, exact domains and save target? This grants lasting execution access, but activates no rule.",
            ),
            Request::RevokeWorkflowAuthorization { .. } => {
                tr("Revoke execution access? Subsequent external and writing steps stop; retrieved files remain.")
            }
            Request::SetWorkflowRuleEnabled { enabled: true, .. } => tr(
                "Activate this rule for future matching mail? No existing inventory is processed. Catch-up is limited to seven days.",
            ),
            Request::SetWorkflowRuleEnabled { .. } => tr("Disable this rule? Retrieved files remain."),
            _ => tr("Remove this item? Retrieved documents and deduplication records are preserved."),
        };
        let workflow = match request {
            Request::RunWorkflow { id, revision, .. } | Request::AuthorizeWorkflow { id, revision } => state
                .workflow
                .as_ref()
                .into_iter()
                .chain(workflows(state, data))
                .find(|w| w.id == *id && w.revision == *revision),
            _ => state.workflow.as_ref(),
        };
        let mut facts = column![note(consequence)].spacing(space::s());
        if let Some(w) = workflow {
            let target = data
                .save_targets
                .iter()
                .find(|t| t.id == w.spec.save_target)
                .map_or_else(|| tr("Missing save target").to_string(), |t| t.spec.name.clone());
            facts = facts
                .push(note(w.spec.name.clone()))
                .push(note(trf("Revision {n}", &[("n", &w.revision)])))
                .push(note(w.account.clone()))
                .push(note(trf("Exact domains · {hosts}", &[("hosts", &w.spec.package.hosts.join(", "))])))
                .push(note(trf("Save target · {target}", &[("target", &target)])));
        }
        if let Request::RunWorkflow { mail, .. } = request {
            let label =
                state.examples.iter().find(|m| m.id == *mail).map_or_else(|| mail.clone(), |m| m.subject.clone());
            facts = facts.push(note(label));
            if !state.selected.is_empty() {
                facts = facts.push(note(trf("Selected mail: {n}", &[("n", &state.selected.len())])));
            }
        }
        let facts =
            iced::widget::container(iced::widget::scrollable(facts).style(style::scrollable)).max_height(size::props());
        let body = column![
            typo::text(typo::title(), tr("Confirm workflow action")),
            facts,
            row![button("Cancel", Message::Cancel, true), button("Continue", Message::Confirm, true)]
                .spacing(space::s())
                .wrap()
        ]
        .spacing(space::l());
        return Some(crate::parts::dialog(body, size::form_max(), Message::Cancel));
    }
    let key = state.menus.open()?;
    if *key == Menu::Actions {
        let items = filtered_actions(state, data)
            .into_iter()
            .enumerate()
            .map(|(at, (label, message))| {
                widgets::menu_item(MenuItem::plain(label, message).current(at == state.cursor))
            })
            .collect();
        let menu = widgets::search_menu(
            widgets::DROPDOWN_SEARCH,
            state.menus.query(),
            |query| Message::Menus(DropdownEvent::Query(query)),
            Message::ChooseAction,
            items,
            size::props(),
        );
        return Some(widgets::floating(
            &state.menus.anchor(&Menu::Actions),
            widgets::Placement::Below(Align::End),
            widgets::escape_free(menu),
            Message::Menus(DropdownEvent::Close),
        ));
    }
    let items = match key {
        Menu::Actions => Vec::new(),
        Menu::Workflow => workflows(state, data)
            .iter()
            .filter(|w| w.saved)
            .map(|w| {
                MenuItem::plain(w.spec.name.clone(), Message::RuleWorkflow(w.id.clone()))
                    .current(state.rule_form.as_ref().is_some_and(|r| r.workflow == w.id))
            })
            .collect(),
        Menu::Template => data
            .workflow_templates
            .iter()
            .map(|t| {
                MenuItem::plain(template_name(t), Message::Template(t.id.clone()))
                    .current(state.template.as_ref() == Some(&t.id))
            })
            .collect(),
        Menu::Target => data
            .save_targets
            .iter()
            .map(|t| MenuItem::plain(t.spec.name.clone(), Message::Target(t.id.clone())).current(t.id == state.target))
            .collect(),
        Menu::Mail => state
            .examples
            .iter()
            .map(|m| {
                MenuItem::plain(format!("{} · {}", m.subject, m.date), Message::Mail(m.id.clone()))
                    .current(m.id == state.mail)
            })
            .collect(),
        Menu::Account => data
            .accounts
            .iter()
            .filter(|a| a.mail.is_some())
            .map(|a| {
                MenuItem::plain(a.label.clone(), Message::Account(a.id.clone()))
                    .current(state.rule_form.as_ref().is_some_and(|r| r.account == a.id))
            })
            .collect(),
        Menu::Category => {
            let mut items = vec![
                MenuItem::plain(tr("Any category"), Message::Category(None))
                    .current(state.rule_form.as_ref().is_some_and(|r| r.category.is_none())),
            ];
            items.extend(data.categories.iter().map(|c| {
                MenuItem::plain(c.label.clone(), Message::Category(Some(c.category)))
                    .current(state.rule_form.as_ref().is_some_and(|r| r.category == Some(c.category)))
            }));
            items
        }
    };
    Some(state.menus.view(key, vec![MenuSection::new(items)], size::props(), Align::Start, Message::Menus))
}

fn template_name(t: &torro_ipc::workflows::WorkflowTemplate) -> String {
    match t.id.as_str() {
        "direct_pdf" => tr("Direct PDF").into(),
        "stripe_invoice" => tr("Stripe invoice").into(),
        "paddle_setapp_invoice_copy" => tr("Paddle / Setapp invoice copy").into(),
        _ => t.name.clone(),
    }
}
fn template_description(t: &torro_ipc::workflows::WorkflowTemplate) -> String {
    match t.id.as_str() {
        "direct_pdf" => tr("Uses one unambiguous direct PDF link. The exact domain is taken from the example mail, without downloading.").into(),
        "stripe_invoice" => tr("Retrieves the original Stripe invoice PDF, not a receipt. No payment, login or browser actions.").into(),
        "paddle_setapp_invoice_copy" => tr("Creates a marked PDF copy of supported Paddle invoice HTML. No supplier-layout imitation, external resources or login.").into(),
        _ => t.description.clone(),
    }
}

/// In-memory demo only. It exercises the same UI responses without downloads, file access or grants.
#[derive(Debug)]
pub struct DemoState {
    workflows: Vec<Workflow>,
    rules: Vec<WorkflowRule>,
    runs: Vec<WorkflowRun>,
}

impl Default for DemoState {
    fn default() -> Self {
        let spec = DraftSpec {
            name: tr("Invoice download").into(),
            description: tr("Download document").into(),
            mail: "m:p3".into(),
            save_target: "demo-invoices".into(),
            package: torro_ipc::workflows::Package::direct_pdf("documents.example.org"),
        };
        let workflow = Workflow {
            id: "demo-workflow".into(),
            revision: 1,
            digest: "0".repeat(64),
            account: "mail:anna@mailbox.org".into(),
            spec,
            template_origin: None,
            saved: true,
            enabled: true,
            tested: true,
            authorized: false,
            untrusted: true,
        };
        let run = demo_run(&workflow, "m:p3", false, 0);
        Self { workflows: vec![workflow], rules: vec![], runs: vec![run] }
    }
}

fn demo_run(w: &Workflow, mail: &str, test: bool, index: usize) -> WorkflowRun {
    WorkflowRun {
        id: format!("demo-run-{index}"),
        workflow: w.id.clone(),
        revision: w.revision,
        account: w.account.clone(),
        mail: mail.into(),
        save_target: w.spec.save_target.clone(),
        test,
        generation: 1,
        state: RunState::Succeeded,
        reason: None,
        attempts: 1,
        created: chrono::DateTime::from_timestamp(1_793_620_800, 0).unwrap_or_default(),
        document: Some(torro_ipc::workflows::RetrievedDocument {
            id: format!("demo-document-{index}"),
            save_target: w.spec.save_target.clone(),
            relative_path: "Invoices/2026-11/invoice.pdf".into(),
            sha256: "0".repeat(64),
            bytes: 24576,
            provenance: if w.spec.package.steps.iter().any(|s| matches!(s, Step::RenderInvoiceCopy { .. })) {
                Provenance::HtmlCopy
            } else {
                Provenance::SupplierPdf
            },
            source_host: w.spec.package.hosts.first().cloned().unwrap_or_default(),
            profile_version: None,
            missing: false,
            changed: false,
        }),
        untrusted: true,
    }
}

pub fn demo_templates() -> Vec<torro_ipc::workflows::WorkflowTemplate> {
    use torro_ipc::workflows::{InvoiceProfile, Package, WorkflowTemplate};
    let direct = Package::direct_pdf("documents.example.org");
    let stripe = Package {
        hosts: vec![
            "invoice.stripe.com".into(),
            "invoicedata.stripe.com".into(),
            "stripe-upload-api.s3.us-west-1.amazonaws.com".into(),
        ],
        steps: vec![
            Step::MailLink { host: "invoice.stripe.com".into(), label: None },
            Step::ResolveStripeInvoicePdf { version: 1 },
            Step::VerifyPdf,
            Step::SaveToTarget,
        ],
        ..direct.clone()
    };
    let mut paddle = Package {
        hosts: vec!["my.paddle.com".into()],
        steps: vec![
            Step::MailLink { host: "my.paddle.com".into(), label: None },
            Step::Get,
            Step::RenderInvoiceCopy { profile: InvoiceProfile::PaddleSetapp, version: 1 },
            Step::VerifyPdf,
            Step::SaveToTarget,
        ],
        ..direct.clone()
    };
    paddle.binding.typesetter = 1;
    paddle.binding.fonts_digest = "0".repeat(64);
    [
        ("direct_pdf", "Direct PDF", direct),
        ("stripe_invoice", "Stripe invoice", stripe),
        ("paddle_setapp_invoice_copy", "Paddle / Setapp invoice copy", paddle),
    ]
    .into_iter()
    .map(|(id, name, package)| WorkflowTemplate {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        revision: 1,
        digest: "0".repeat(64),
        package,
    })
    .collect()
}

impl DemoState {
    pub fn answer(&mut self, request: &Request) -> Option<Response> {
        let answer = match request {
            Request::ListWorkflowTemplates => Response::WorkflowTemplates { templates: demo_templates() },
            Request::GetWorkflowTemplate { id } => {
                Response::WorkflowTemplate { template: demo_templates().into_iter().find(|t| t.id == *id)? }
            }
            Request::ListWorkflows => Response::Workflows { workflows: self.workflows.clone() },
            Request::ListWorkflowRules => Response::WorkflowRules { rules: self.rules.clone() },
            Request::ListWorkflowRuns { workflow, mail } => Response::WorkflowRuns {
                runs: self
                    .runs
                    .iter()
                    .filter(|r| {
                        workflow.as_ref().is_none_or(|w| *w == r.workflow) && mail.as_ref().is_none_or(|m| *m == r.mail)
                    })
                    .cloned()
                    .collect(),
            },
            Request::GetWorkflow { id } => {
                Response::Workflow { workflow: self.workflows.iter().find(|w| w.id == *id)?.clone() }
            }
            Request::SearchMail { query, .. } => {
                let now = chrono::DateTime::from_timestamp(1_793_620_800, 0).unwrap_or_default();
                let messages = torro_inbox::demo::mail_sample(now)
                    .into_iter()
                    .map(|m| m.summary)
                    .filter(|m| query.as_ref().is_none_or(|q| m.subject.to_lowercase().contains(&q.to_lowercase())))
                    .collect();
                Response::MailMessages {
                    messages,
                    cursor: None,
                    source: torro_ipc::mail::MailSource::Cache,
                    complete: true,
                    incomplete_reason: None,
                    highlights: vec![],
                    total_estimate: None,
                }
            }
            Request::InstantiateWorkflowTemplate { template_id, mail, save_target, .. }
            | Request::ProposeWorkflow { mail, save_target, instruction: template_id } => {
                let template = demo_templates()
                    .into_iter()
                    .find(|t| t.id == *template_id)
                    .unwrap_or_else(|| demo_templates().remove(0));
                let mut w = self.workflows.first()?.clone();
                w.id = format!("demo-workflow-{}", self.workflows.len());
                w.saved = false;
                w.enabled = false;
                w.tested = false;
                w.authorized = false;
                w.spec.mail = mail.clone();
                w.spec.save_target = save_target.clone();
                w.spec.package = template.package;
                w.template_origin = Some(torro_ipc::workflows::TemplateOrigin {
                    id: template.id,
                    revision: template.revision,
                    digest: template.digest,
                });
                self.workflows.push(w.clone());
                Response::WorkflowDraft { workflow: w }
            }
            Request::UpdateWorkflowDraft { id, spec, .. } => {
                let w = self.workflows.iter_mut().find(|w| w.id == *id)?;
                w.spec = spec.clone();
                w.revision += 1;
                w.tested = false;
                w.authorized = false;
                w.saved = false;
                w.enabled = false;
                Response::WorkflowDraft { workflow: w.clone() }
            }
            Request::DuplicateWorkflow { id, .. } => {
                let mut w = self.workflows.iter().find(|w| w.id == *id)?.clone();
                w.id = format!("demo-workflow-{}", self.workflows.len());
                w.tested = false;
                w.authorized = false;
                w.saved = false;
                w.enabled = false;
                self.workflows.push(w.clone());
                Response::WorkflowDraft { workflow: w }
            }
            Request::PreviewWorkflowRun { id, mail, .. } => {
                let w = self.workflows.iter().find(|w| w.id == *id)?;
                Response::WorkflowPreview {
                    preview: WorkflowPreview {
                        workflow: w.id.clone(),
                        revision: w.revision,
                        mail: mail.clone(),
                        hosts: w.spec.package.hosts.clone(),
                        save_target: w.spec.save_target.clone(),
                        possible_writes: true,
                        authorization_required: !w.authorized,
                        matches: vec![],
                        untrusted: true,
                    },
                }
            }
            Request::RunWorkflow { id, mail, test, .. } => {
                let w = self.workflows.iter_mut().find(|w| w.id == *id)?;
                if *test {
                    w.tested = true;
                }
                let run = demo_run(w, mail, *test, self.runs.len());
                self.runs.push(run.clone());
                Response::WorkflowRun { run }
            }
            Request::SaveWorkflow { id, .. } => {
                let w = self.workflows.iter_mut().find(|w| w.id == *id)?;
                if !w.saved && w.tested {
                    w.enabled = true;
                }
                w.saved = w.tested;
                Response::Workflow { workflow: w.clone() }
            }
            Request::SetWorkflowEnabled { id, enabled, .. } => {
                let w = self.workflows.iter_mut().find(|w| w.id == *id)?;
                w.enabled = *enabled;
                Response::Workflow { workflow: w.clone() }
            }
            Request::AuthorizeWorkflow { id, .. } | Request::RevokeWorkflowAuthorization { id } => {
                let w = self.workflows.iter_mut().find(|w| w.id == *id)?;
                w.authorized = matches!(request, Request::AuthorizeWorkflow { .. });
                Response::Workflow { workflow: w.clone() }
            }
            Request::RemoveWorkflow { id, .. } => {
                self.workflows.retain(|w| w.id != *id);
                Response::Ok
            }
            Request::GetWorkflowRun { id } => {
                Response::WorkflowRun { run: self.runs.iter().find(|r| r.id == *id)?.clone() }
            }
            Request::CancelWorkflowRun { id } => {
                let r = self.runs.iter_mut().find(|r| r.id == *id)?;
                r.state = RunState::Cancelled;
                Response::WorkflowRun { run: r.clone() }
            }
            Request::OpenWorkflowDocument { .. } | Request::ShowWorkflowDocument { .. } => Response::Error {
                code: "unsupported".into(),
                message: tr("Demo documents are samples; no file is opened.").into(),
            },
            Request::CreateWorkflowRule { spec } | Request::UpdateWorkflowRule { spec, .. } => {
                let id = if let Request::UpdateWorkflowRule { id, .. } = request {
                    id.clone()
                } else {
                    format!("demo-rule-{}", self.rules.len())
                };
                let revision = self.rules.iter().find(|r| r.id == id).map_or(1, |r| r.revision + 1);
                let rule =
                    WorkflowRule { id: id.clone(), revision, spec: spec.clone(), enabled: false, activated: None };
                self.rules.retain(|r| r.id != id);
                self.rules.push(rule.clone());
                Response::WorkflowRule { rule }
            }
            Request::DuplicateWorkflowRule { id, .. } => {
                let mut rule = self.rules.iter().find(|r| r.id == *id)?.clone();
                rule.id = format!("demo-rule-{}", self.rules.len());
                rule.enabled = false;
                rule.activated = None;
                self.rules.push(rule.clone());
                Response::WorkflowRule { rule }
            }
            Request::SetWorkflowRuleEnabled { id, enabled, .. } => {
                let rule = self.rules.iter_mut().find(|r| r.id == *id)?;
                rule.enabled = *enabled;
                Response::WorkflowRule { rule: rule.clone() }
            }
            Request::RemoveWorkflowRule { id, .. } => {
                self.rules.retain(|r| r.id != *id);
                Response::Ok
            }
            Request::PreviewWorkflowRule { spec } => {
                let w = self.workflows.iter().find(|w| w.id == spec.workflow)?;
                Response::WorkflowPreview {
                    preview: WorkflowPreview {
                        workflow: w.id.clone(),
                        revision: w.revision,
                        mail: w.spec.mail.clone(),
                        hosts: w.spec.package.hosts.clone(),
                        save_target: w.spec.save_target.clone(),
                        possible_writes: true,
                        authorization_required: !w.authorized,
                        matches: vec![w.spec.mail.clone()],
                        untrusted: true,
                    },
                }
            }
            _ => return None,
        };
        Some(answer)
    }
}

/// Stable render scenes, staged after the settings load. No real backend is called.
pub fn stage(state: &mut State, name: &str, data: &Data) {
    let mut demo = DemoState::default();
    let w = demo.workflows.remove(0);
    state.target = w.spec.save_target.clone();
    state.mail = w.spec.mail.clone();
    state.examples =
        torro_inbox::demo::mail_sample(chrono::DateTime::from_timestamp(1_793_620_800, 0).unwrap_or_default())
            .into_iter()
            .map(|m| m.summary)
            .collect();
    match name {
        "setup" | "template" => {
            state.setup = true;
            state.template_mode = name == "template";
            loaded(state, data);
            if name == "template" {
                state.menus.update(DropdownEvent::Toggle(Menu::Template));
            }
        }
        "test" => {
            state.workflow = Some(w.clone());
            state.preview = Some(WorkflowPreview {
                workflow: w.id.clone(),
                revision: w.revision,
                mail: w.spec.mail.clone(),
                hosts: w.spec.package.hosts.clone(),
                save_target: w.spec.save_target.clone(),
                possible_writes: true,
                authorization_required: true,
                matches: vec![],
                untrusted: true,
            });
            state.confirm = Some(Request::RunWorkflow {
                id: w.id,
                revision: w.revision,
                mail: state.mail.clone(),
                test: true,
                repeat: false,
            });
        }
        "result" | "error" => {
            let mut run = demo.runs.remove(0);
            if name == "error" {
                run.state = RunState::Failed;
                run.reason = Some(RunReason::Unsupported);
                run.document = None;
            } else if let Some(document) = &mut run.document {
                document.provenance = Provenance::HtmlCopy;
                document.source_host = "my.paddle.com".into();
            }
            state.run = Some(run);
        }
        "rule" => {
            state.rule_form = Some(RuleSpec {
                workflow: w.id,
                account: w.account,
                trigger: Trigger::CategoryAssigned,
                query: "from:billing@example.org".into(),
                category: Some(torro_mail::Category::Invoice),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Backend;
    fn workflow() -> Workflow {
        Workflow {
            id: "w1".into(),
            revision: 1,
            digest: "digest".into(),
            account: "mail:a@example.org".into(),
            spec: DraftSpec {
                name: "Invoice".into(),
                description: String::new(),
                mail: "m:example".into(),
                save_target: "target".into(),
                package: torro_ipc::workflows::Package::direct_pdf("documents.example.org"),
            },
            template_origin: None,
            saved: false,
            enabled: false,
            tested: false,
            authorized: false,
            untrusted: true,
        }
    }
    #[test]
    fn preview_test_save_and_authorization_are_separate() {
        let mut state = State { workflow: Some(workflow()), mail: "m:example".into(), ..State::default() };
        let data = Data::default();
        assert!(matches!(update::<()>(&mut state, Message::Save, &data), Action::None));
        let _: Action<()> = update(&mut state, Message::Test, &data);
        assert!(state.confirm.is_none());
        assert!(matches!(
            update::<()>(&mut state, Message::Preview, &data),
            Action::Ask(Request::PreviewWorkflowRun { .. })
        ));
        answered(
            &mut state,
            &Ok(Response::WorkflowPreview {
                preview: WorkflowPreview {
                    workflow: "w1".into(),
                    revision: 1,
                    mail: "m:example".into(),
                    hosts: vec!["documents.example.org".into()],
                    save_target: "target".into(),
                    possible_writes: true,
                    authorization_required: true,
                    matches: vec![],
                    untrusted: true,
                },
            }),
        );
        let _: Action<()> = update(&mut state, Message::Test, &data);
        assert!(matches!(state.confirm, Some(Request::RunWorkflow { test: true, repeat: false, .. })));
        assert!(matches!(
            update::<()>(&mut state, Message::Confirm, &data),
            Action::Ask(Request::RunWorkflow { test: true, .. })
        ));
        assert!(!state.workflow.as_ref().expect("workflow").authorized);
    }
    #[test]
    fn failed_requests_keep_forms_and_stop_the_queue() {
        let mut state = State {
            workflow: Some(workflow()),
            pending: Some(Request::SaveWorkflow { id: "w1".into(), revision: 1 }),
            queued: VecDeque::from([Request::ListWorkflows]),
            ..State::default()
        };
        answered(
            &mut state,
            &Ok(Response::Error { code: "unsupported".into(), message: "unsupported version".into() }),
        );
        assert!(state.problem.as_ref().is_some_and(|p| p.contains("unsupported")));
        assert!(state.workflow.is_some());
        assert!(next_request(&mut state).is_none());
    }

    #[test]
    fn changing_mail_invalidates_the_preview_and_navigation_protects_edits() {
        let mut state = State { workflow: Some(workflow()), mail: "m:example".into(), ..State::default() };
        let data = Data::default();
        state.preview = Some(WorkflowPreview {
            workflow: "w1".into(),
            revision: 1,
            mail: "m:example".into(),
            hosts: vec![],
            save_target: "target".into(),
            possible_writes: true,
            authorization_required: true,
            matches: vec![],
            untrusted: true,
        });
        let _: Action<()> = update(&mut state, Message::Mail("m:other".into()), &data);
        assert!(state.preview.is_none());
        let _: Action<()> = update(&mut state, Message::Test, &data);
        assert!(state.confirm.is_none());
        let _: Action<()> = update(&mut state, Message::Edit, &data);
        let _: Action<()> = update(&mut state, Message::Name("Changed".into()), &data);
        let _: Action<()> = update(&mut state, Message::Back, &data);
        assert!(state.discard);
        assert!(state.edit.is_some());
        let _: Action<()> = update(&mut state, Message::Cancel, &data);
        assert!(!state.discard && state.edit.is_some());
        let _: Action<()> = update(&mut state, Message::Discard, &data);
        assert!(state.edit.is_none() && state.workflow.is_none());
    }

    #[test]
    fn bundled_templates_work_without_ai_and_do_not_test_authorize_or_activate() {
        let demo = crate::demo::Demo::new();
        let response =
            iced::futures::executor::block_on(demo.call(Request::ListSaveTargets { id: None, export_only: false }))
                .expect("targets");
        let mut data = Data { workflow_templates: demo_templates(), ..Data::default() };
        data.take(response);
        let mut state = State::default();
        let _: Action<()> =
            update(&mut state, Message::New { mail: "m:p3".into(), account: None, template: true }, &data);
        answered(
            &mut state,
            &Ok(Response::MailMessages {
                messages: vec![],
                cursor: None,
                source: torro_ipc::mail::MailSource::Cache,
                complete: true,
                incomplete_reason: None,
                highlights: vec![],
                total_estimate: None,
            }),
        );
        let Action::Ask(request @ Request::InstantiateWorkflowTemplate { .. }) =
            update::<()>(&mut state, Message::Create, &data)
        else {
            panic!("template request")
        };
        let mut backend = DemoState::default();
        let response = backend.answer(&request).expect("inactive draft");
        answered(&mut state, &Ok(response));
        let w = state.workflow.as_ref().expect("draft");
        assert!(!w.saved && !w.tested && !w.authorized && !w.enabled);
        assert!(backend.rules.is_empty());
        assert_eq!(backend.runs.len(), 1, "only the pre-existing demo run; copying ran nothing");
    }

    #[test]
    fn every_workflow_scene_builds_with_shared_navigation_and_overlays() {
        let data = Data { workflow_templates: demo_templates(), ..Data::default() };
        for name in ["setup", "template", "test", "result", "error", "rule", "automation"] {
            let mut state = State::default();
            stage(&mut state, name, &data);
            let _ = page(&state, &data);
            let _ = overlay(&state, &data);
            if name != "automation" {
                assert!(page(&state, &data).sub_page.is_some(), "{name}: Back remains reachable");
            }
        }
    }

    #[test]
    fn f6_search_and_enter_offer_keyboard_choices_without_executing_them() {
        use iced::keyboard::{Key, Modifiers, key::Named};
        let data = Data { workflow_templates: demo_templates(), ..Data::default() };
        let mut state = State { setup: true, ..State::default() };
        assert!(matches!(
            update::<()>(&mut state, Message::Key(Key::Named(Named::F6), Modifiers::default()), &data),
            Action::Focus(widgets::DROPDOWN_SEARCH)
        ));
        let _: Action<()> =
            update(&mut state, Message::Menus(DropdownEvent::Query(tr("Stripe invoice").into())), &data);
        let _: Action<()> = update(&mut state, Message::ChooseAction, &data);
        assert_eq!(state.template.as_deref(), Some("stripe_invoice"));
        assert!(state.template_mode && state.pending.is_none());
        assert!(state.workflow.is_none() && state.confirm.is_none());
    }

    #[test]
    fn jobs_poll_by_id_and_successful_tests_reload_the_workflow() {
        let demo = DemoState::default();
        let mut run = demo.runs.first().expect("run").clone();
        run.test = true;
        run.state = RunState::Running;
        let mut state = State { run: Some(run.clone()), ..State::default() };
        assert!(
            matches!(update::<()>(&mut state, Message::Poll, &Data::default()), Action::Ask(Request::GetWorkflowRun { id }) if id == run.id)
        );
        run.state = RunState::Succeeded;
        answered(&mut state, &Ok(Response::WorkflowRun { run: run.clone() }));
        assert_eq!(next_request(&mut state), Some(Request::GetWorkflow { id: run.workflow }));
    }
}
