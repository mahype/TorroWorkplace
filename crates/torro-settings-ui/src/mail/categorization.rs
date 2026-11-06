//! Settings › Categorization in the mail window (issue #481, before it issue #454, task E12.10 and
//! issue #343, concept `docs/kategorien.md`): one rule for the person – **everything set here holds
//! for every new mail; mail already there is sorted only when they say so**.
//!
//! ```text
//! Categorization                                              [Sort existing mail…]
//! Fixed rules and the AI sort every new mail as it arrives.
//! ▓▓▓▓▓░░░ Sorting existing mail · 412 of 1,284 · about 4 min left   [Pause] [Cancel]
//! Categories 23 │ AI
//! ```
//!
//! - **Categories** ([`categories_tab`], the tab the page opens on): the groups as cards with their
//!   categories in one column, folded and unfolded here. A category's grip moves it within its group
//!   and into another (`inbox.move_category`), a drop line shows where it goes: the order decides
//!   which category wins when fixed rules of several match. A click opens its page with its fixed
//!   rules ([`super::editor`]). "+ New category in …" in its group; then "Other filters" – the mail
//!   filters that set no category (they mark read or go to the trash) –, and at the bottom "Remove
//!   categories from all mail…", which removes only the categories.
//! - **AI** ([`ai_tab`]): "Sort with model", a menu instead of a switch and a list – "Off" first,
//!   then the models on this computer (decision models "Fast", chat models "Slower") and in the
//!   cloud ("Cloud", picking one asks first, issue #424). A line under it says what the choice
//!   means; "Categories the model may choose" leads to the tab Categories.
//! - **Sort existing mail** ([`sort_dialog`]): accounts, folders, time span, what to apply ("Fixed
//!   rules", the model), what to do with mail that has a category, a preview with how long it takes,
//!   "Start sorting"; while a run or a removal goes, a line with its progress stands under the head
//!   ([`progress`]).
//!
//! Everything is a request; the core checks all of it again.

use std::collections::BTreeSet;

use iced::widget::text::Wrapping;
use iced::widget::{checkbox, column, container, mouse_area, row, space as blank, svg};
use iced::{Alignment, Element, Fill, Length, Theme, mouse};
use torro_core::ai::{AiAssignment, AiFunction, AiModel, AiSlot};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{palette, size, space};
use torro_gui::units::{format_count, format_duration};
use torro_gui::widgets::{Align, Anchor, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{layout, style, theme, typo, widgets};
use torro_ipc::inbox::{CategoryInfo, CategoryJobControl, CategoryJobPreview, CategoryJobScope, CategoryJobState};
use torro_ipc::{Request, Response};
use torro_mail::{Category, CategoryGroup};

use super::{editor, filters};
use crate::parts::{choice, pressable, row_frame, setting_row, switch};
use crate::{Action, Data, Page};

/// The tabs of the page, in their order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    /// The categories with their fixed rules (the tab the page opens on, issue #481).
    #[default]
    Categories,
    /// The model that sorts.
    Ai,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Categories, Tab::Ai];

    fn index(self) -> usize {
        Tab::ALL.iter().position(|tab| *tab == self).unwrap_or_default()
    }
}

/// What the page keeps between two frames. The job, the order and the counts always come from the
/// core; the page keeps only what is being chosen.
#[derive(Debug, Default)]
pub struct State {
    pub(super) editor: editor::State,
    pub(super) filters: filters::State,
    tab: Tab,
    /// The menu of "Sort with model" is open.
    model_menu: bool,
    model_anchor: Anchor,
    /// A model in the cloud picked while the question before it sorts is open (issue #424): only
    /// "Use" sets it, "Cancel" changes nothing.
    asking_cloud: Option<CloudPick>,
    /// The groups folded on this page.
    folded: BTreeSet<CategoryGroup>,
    /// The group whose "…" menu is open.
    group_menu: Option<CategoryGroup>,
    group_anchor: Anchor,
    /// A category dragged by its grip, and the slot it is over: a group and the place in it the
    /// category would take, among all of the group's categories (issue #481).
    category_drag: Option<(Category, Option<(CategoryGroup, usize)>)>,
    /// "Remove categories from all mail?" is asked.
    asking_clear: bool,
    /// The dialog "Sort existing mail" while it is open.
    sort: Option<SortForm>,
    preview: Option<CategoryJobPreview>,
    preview_error: Option<String>,
    pending_scope: Option<CategoryJobScope>,
    /// Previews on their way: a refusal counts only for the last one (PR review of #463).
    previews_waiting: u32,
}

/// A model in the cloud waiting for the question before it sorts mail (issue #424).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudPick {
    /// Its AI account (`ai:…`).
    engine: String,
    /// What the question calls it: the model's name, or the account's.
    name: String,
    /// What "Use" sends: what the pick would have sent without the question.
    requests: Vec<Request>,
}

/// What the dialog "Sort existing mail" holds while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SortForm {
    /// The accounts chosen; empty: every account.
    accounts: Vec<String>,
    /// The folders chosen; empty: every folder.
    folders: Vec<String>,
    period: Period,
    /// "Fixed rules": the sorting rules and the mail filters.
    rules: bool,
    /// "Model".
    model: bool,
    /// "Ask the AI again" instead of "Leave as it is".
    again: bool,
    /// The menu of accounts or folders that is open.
    menu: Option<SortMenu>,
}

/// The menus of the dialog "Sort existing mail".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMenu {
    Accounts,
    Folders,
}

/// The time span of "Sort existing mail".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    All,
    Days30,
    Days90,
    Year,
}

impl Period {
    const ALL: [Period; 4] = [Period::All, Period::Days30, Period::Days90, Period::Year];

    fn days(self) -> Option<i64> {
        match self {
            Period::All => None,
            Period::Days30 => Some(30),
            Period::Days90 => Some(90),
            Period::Year => Some(365),
        }
    }
}

impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Period::All => tr("All mail"),
            Period::Days30 => tr("Last 30 days"),
            Period::Days90 => tr("Last 90 days"),
            Period::Year => tr("Last year"),
        })
    }
}

/// What a row, a menu or a dialog of the page reports.
#[derive(Debug, Clone)]
pub enum Message {
    /// One of the tabs, by its index.
    Tab(usize),
    /// "Sort with model": open (`true`) or close the menu.
    ModelMenu(bool),
    /// "Off" in the menu (`false`): the model sorts nothing. `true` sorts again with the slot, or
    /// what sorted last.
    Enabled(bool),
    /// A model picked for sorting mail. One in the cloud asks first (issue #424).
    Model(String, String),
    /// "Use" in the question before a model in the cloud sorts mail.
    UseCloud,
    /// "Cancel" in that question, a click next to it, or Escape: nothing changes.
    CancelCloud,
    /// "Manage models…": which models are on is set in the section AI.
    ManageModels,
    /// Escape during a drag: it ends, nothing moves.
    CancelDrag,
    /// "New filter…" of "Other filters".
    NewFilter,
    /// The filter dialog and the result of applying a filter.
    Filters(filters::Message),
    /// The editor of categories, groups and rules.
    Editor(editor::Message),
    /// The head of a group's card: fold or unfold it.
    Fold(CategoryGroup),
    /// The "…" menu of a group: open it (`Some`) or close it.
    GroupMenu(Option<CategoryGroup>),
    /// The grip of a category was pressed.
    CategoryGrip(Category),
    /// The pointer of a category's drag left this group's card.
    LeaveGroup(CategoryGroup),
    /// The pointer of a category's drag is over this place of a group: before the category at this
    /// index, or at the end with the group's length.
    OverSlot(CategoryGroup, usize),
    /// The pointer was let go: the dragged category goes where the drop line is.
    DropCategory,
    /// One category shown in the sidebar or not.
    Shown(Category, bool),
    /// "Remove categories from all mail…".
    AskClear,
    /// "Cancel" in that question.
    CloseClear,
    /// "Remove from all mail".
    ConfirmClear,
    /// Pause, Resume or Cancel of an unfinished removal.
    ControlClear(String, CategoryJobControl),
    /// "Sort existing mail…" (`rules_only`: the fixed rules alone, without the model).
    OpenSort {
        rules_only: bool,
    },
    /// "Cancel" in the dialog, a click next to it, or Escape.
    CloseSort,
    /// A menu of the dialog: open it (`Some`) or close it.
    SortMenu(Option<SortMenu>),
    /// "All accounts" in the dialog.
    AllAccounts,
    ToggleAccount(String),
    /// "All folders" in the dialog.
    AllFolders,
    ToggleFolder(String),
    Period(Period),
    /// "Fixed rules" ticked or not.
    ApplyRules(bool),
    /// "Model" ticked or not.
    ApplyModel(bool),
    /// "Ask the AI again" (`true`) or "Leave as it is".
    Again(bool),
    /// "Start sorting".
    StartJob,
    /// Pause, Resume or Cancel of the run.
    ControlJob(String, CategoryJobControl),
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::Tab(index) => {
            state.tab = Tab::ALL.get(index).copied().unwrap_or_default();
            close_menus(state);
            Action::None
        }
        Message::ModelMenu(open) => {
            state.model_menu = open;
            Action::None
        }
        Message::Enabled(true) => switched_on(state, data),
        // Off is the exception switched off, with what sorts now: it goes before the slot and keeps
        // sorting off, also while the slot changes. Nothing sorts, nothing to switch off.
        Message::Enabled(false) => {
            state.model_menu = false;
            let held = in_effect(data);
            match held.engine {
                Some(engine) => Action::send(Request::SetAiAssignment {
                    function: AiFunction::InboxCategorize,
                    engine: Some(engine),
                    model: held.model,
                    effort: held.effort,
                    enabled: false,
                }),
                None => Action::None,
            }
        }
        // Picking a model switches the sorting on. A model in the cloud asks first (issue #424) –
        // choosing it is the consent that mail leaves this computer –, unless it sorts already with
        // that consent.
        Message::Model(engine, model) => {
            state.model_menu = false;
            let pick = in_cloud(data, &engine, Some(&model)).filter(|_| !consented(data, &engine, &model));
            let requests = use_model(data, engine, Some(model));
            match pick {
                Some(pick) => {
                    state.asking_cloud = Some(CloudPick { requests, ..pick });
                    Action::None
                }
                None => Action::Send(requests),
            }
        }
        Message::UseCloud => match state.asking_cloud.take() {
            Some(pick) => Action::Send(pick.requests),
            None => Action::None,
        },
        Message::CancelCloud => {
            state.asking_cloud = None;
            Action::None
        }
        Message::ManageModels => {
            state.model_menu = false;
            Action::Open(crate::assistant::AI_SECTION)
        }
        Message::CancelDrag => {
            state.category_drag = None;
            Action::None
        }
        // Only the card it is over lets go: moving fast, the next card may say "over" first.
        Message::LeaveGroup(group) => {
            if let Some((_, over)) = &mut state.category_drag
                && over.is_some_and(|(held, _)| held == group)
            {
                *over = None;
            }
            Action::None
        }
        Message::NewFilter => {
            close_menus(state);
            let new = filters::Message::New { query: String::new(), account: None };
            filters::update(&mut state.filters, new, data)
        }
        Message::Filters(message) => filters::update(&mut state.filters, message, data),
        // A filter's row, its switch and "Filter from a search…" on a category's page (issue #481).
        Message::Editor(editor::Message::Filter(message)) => {
            let _ = editor::update::<A>(&mut state.editor, editor::Message::Popover(None), data);
            filters::update(&mut state.filters, message, data)
        }
        // "Edit group…" and "New category…" of a group's menu: the menu goes with the click.
        Message::Editor(message) => {
            state.group_menu = None;
            editor::update(&mut state.editor, message, data)
        }
        Message::Fold(group) => {
            if !state.folded.remove(&group) {
                state.folded.insert(group);
            }
            Action::None
        }
        Message::GroupMenu(group) => {
            state.group_menu = group;
            Action::None
        }
        Message::CategoryGrip(category) => {
            state.group_menu = None;
            state.category_drag = Some((category, None));
            Action::None
        }
        Message::OverSlot(group, at) => {
            if let Some((_, over)) = &mut state.category_drag {
                *over = Some((group, at));
            }
            Action::None
        }
        Message::DropCategory => {
            let Some((category, Some((group, at)))) = state.category_drag.take() else { return Action::None };
            dropped(data, category, group, at)
        }
        Message::Shown(category, shown) => Action::send(editor::shown_request(data, category, shown)),
        Message::AskClear => {
            state.asking_clear = true;
            Action::None
        }
        Message::CloseClear => {
            state.asking_clear = false;
            Action::None
        }
        // Only the categories go (issue #454): sorting stays as it is, and nothing is sorted anew.
        Message::ConfirmClear => {
            state.asking_clear = false;
            Action::send(Request::ClearCategories { all_accounts: true, disable_automatic: false })
        }
        Message::ControlClear(id, action) => Action::send(Request::ControlClearCategories { id, action }),
        Message::OpenSort { rules_only } => open_sort(state, data, rules_only),
        Message::CloseSort => {
            state.sort = None;
            Action::None
        }
        Message::SortMenu(menu) => {
            if let Some(form) = &mut state.sort {
                form.menu = menu;
            }
            Action::None
        }
        Message::StartJob => match state.sort.take() {
            Some(form) => Action::send(Request::StartCategoryJob { scope: scope(&form) }),
            None => Action::None,
        },
        Message::ControlJob(id, action) => Action::send(Request::ControlCategoryJob { id, action }),
        message => {
            let Some(form) = &mut state.sort else { return Action::None };
            match message {
                Message::AllAccounts => form.accounts.clear(),
                Message::ToggleAccount(account) => toggle(&mut form.accounts, account),
                Message::AllFolders => form.folders.clear(),
                Message::ToggleFolder(folder) => toggle(&mut form.folders, folder),
                Message::Period(period) => form.period = period,
                Message::ApplyRules(on) => form.rules = on,
                Message::ApplyModel(on) => form.model = on,
                Message::Again(again) => form.again = again,
                _ => return Action::None,
            }
            preview_action(state)
        }
    }
}

/// Closes every menu of the page; a drag ends without a move.
fn close_menus(state: &mut State) {
    state.model_menu = false;
    state.group_menu = None;
    state.category_drag = None;
}

/// `inbox.move_category` for `category` let go before the category at `at` of `group` (its length:
/// at the end). The place counts among the others of the group, so one dropped below itself in its
/// own group takes the place above; dropped where it stands, nothing moves.
fn dropped<A>(data: &Data, category: Category, group: CategoryGroup, at: usize) -> Action<A> {
    let members: Vec<Category> =
        data.categories.iter().filter(|info| info.group == group).map(|info| info.category).collect();
    let from = members.iter().position(|held| *held == category);
    let position = match from {
        Some(from) if from < at => at - 1,
        _ => at,
    };
    if from == Some(position) {
        return Action::None;
    }
    Action::send(Request::MoveCategory { category, group, position: position as u32 })
}

/// Opens "Sort existing mail": everything chosen, the model only while one sorts; `rules_only` (a
/// rule's "Apply to existing mail") applies the rules and filters alone. Asks for its preview.
fn open_sort<A>(state: &mut State, data: &Data, rules_only: bool) -> Action<A> {
    close_menus(state);
    let model = !rules_only && model_sorts(data);
    state.sort = Some(SortForm {
        accounts: Vec::new(),
        folders: Vec::new(),
        period: Period::All,
        rules: true,
        model,
        again: false,
        menu: None,
    });
    preview_action(state)
}

fn toggle(selected: &mut Vec<String>, id: String) {
    match selected.iter().position(|held| *held == id) {
        Some(at) => {
            selected.remove(at);
        }
        None => selected.push(id),
    }
}

/// The scope of a run as the dialog holds it.
fn scope(form: &SortForm) -> CategoryJobScope {
    CategoryJobScope {
        accounts: form.accounts.clone(),
        folders: form.folders.clone(),
        since: form.period.days().map(|days| chrono::Utc::now() - chrono::Duration::days(days)),
        again: form.again,
        apply_rules: form.rules,
        apply_model: form.model,
        ..CategoryJobScope::default()
    }
}

/// `inbox.preview_category_job` for the dialog's choice; nothing while it applies nothing.
fn preview_action<A>(state: &mut State) -> Action<A> {
    state.preview = None;
    state.preview_error = None;
    let Some(form) = &state.sort else { return Action::None };
    if !form.rules && !form.model {
        state.pending_scope = None;
        return Action::None;
    }
    let scope = scope(form);
    state.pending_scope = Some(scope.clone());
    state.previews_waiting += 1;
    Action::Ask(Request::PreviewCategoryJob { scope })
}

/// The answer of a preview (or a filter's preview or run, which go to [`filters`]). Only the
/// answer for the choice asked last counts.
pub fn answered(state: &mut State, answer: &Result<Response, String>) {
    if filters::answered(&mut state.filters, answer) {
        return;
    }
    match answer {
        Ok(Response::CategoryJobPreview { preview }) => {
            state.previews_waiting = state.previews_waiting.saturating_sub(1);
            if preview.request_scope == state.pending_scope {
                state.preview = Some(preview.clone());
                state.preview_error = None;
                state.pending_scope = None;
            }
        }
        // A refusal names no preview: it counts only when no newer one is still on its way.
        Err(problem) => {
            state.previews_waiting = state.previews_waiting.saturating_sub(1);
            if state.previews_waiting == 0 {
                state.preview_error = Some(problem.clone());
                state.pending_scope = None;
            }
        }
        _ => {}
    }
}

/// A send of the host came back with `result`: a filter's dialog waiting for its save closes once
/// the filter is stored ([`filters::sent`]), and a group's form of the editor likewise. A filter
/// stored with "Also apply…" comes back as the message that runs it, for the window to send.
pub fn sent(state: &mut State, result: &Result<Data, String>) -> Option<Message> {
    editor::sent(&mut state.editor, result);
    filters::sent(&mut state.filters, result).map(|id| Message::Filters(filters::Message::ApplyNow(id)))
}

/// What Escape means on this page, first what lies on top: the question before a model in the cloud
/// sorts, a dialog, a menu, a drag, then a page of the editor (asking first when something typed
/// would go). `None` leaves it to the window, which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    if state.asking_cloud.is_some() {
        return Some(Message::CancelCloud);
    }
    if let Some(form) = &state.sort {
        return Some(match form.menu {
            Some(_) => Message::SortMenu(None),
            None => Message::CloseSort,
        });
    }
    if state.asking_clear {
        return Some(Message::CloseClear);
    }
    if let Some(message) = filters::escape(&state.filters) {
        return Some(Message::Filters(message));
    }
    if state.model_menu {
        return Some(Message::ModelMenu(false));
    }
    if state.category_drag.is_some() {
        return Some(Message::CancelDrag);
    }
    if state.group_menu.is_some() {
        return Some(Message::GroupMenu(None));
    }
    editor::escape(&state.editor).map(Message::Editor)
}

/// What a drag of the page is (review of #454): the window hears the button let go anywhere and
/// sends [`Message::DropCategory`]; over its list it moves, elsewhere it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dragging {
    /// A category by its grip.
    Category,
}

/// The drag going on, for the window to end it wherever the button is let go.
pub fn dragging(state: &State) -> Option<Dragging> {
    state.category_drag.map(|_| Dragging::Category)
}

/// The question before a model in the cloud sorts mail is open (issue #424).
pub fn asking_cloud(state: &State) -> bool {
    state.asking_cloud.is_some()
}

// ----- the model -----

/// `model` of `engine` goes into the slot Sorting. An exception would stand before it, so it is
/// taken back – first (review of #325): setting the slot can let Sorting's mail out, and meanwhile
/// an exception the person switched off would sort again. The requests go one after the other and
/// stop at the first refused ([`crate::send`]), so a refused take-back sets no slot.
fn use_model(data: &Data, engine: String, model: Option<String>) -> Vec<Request> {
    let effort = data.settings.ai_slots.decide.as_ref().and_then(|slot| slot.effort);
    let mut requests: Vec<Request> = stored_exception(data).map(|_| no_exception()).into_iter().collect();
    requests.push(set_slot(engine, model, effort));
    requests
}

/// The model `id` of `engine` when it computes in the cloud, as the question names it; `None` for one
/// on this computer. A listed model by what its server says ([`stays_here`]); one the list lacks by
/// its account alone. Naming no model ("Automatic"), by the account's seat as the core weighs it.
fn in_cloud(data: &Data, engine: &str, id: Option<&str>) -> Option<CloudPick> {
    let pick = |name: String| CloudPick { engine: engine.to_string(), name, requests: Vec::new() };
    let Some(id) = id else {
        let here = crate::accounts::ai_provider(engine)
            .is_some_and(|provider| provider.seat() == torro_core::ai::Seat::ThisComputer);
        return (!here).then(|| pick(account_label(data, engine)));
    };
    let listed = data
        .decider_models
        .iter()
        .find(|(account, model)| account == engine && crate::accounts::same_model(&model.id, id));
    let model = listed.map_or_else(|| AiModel::named(id), |(_, model)| model.clone());
    (!stays_here(engine, &model)).then(|| pick(model.name))
}

/// `model` of `engine` sorts now with the person's consent that mail leaves this computer.
fn consented(data: &Data, engine: &str, model: &str) -> bool {
    let held = in_effect(data);
    held.is_ready()
        && held.engine.as_deref() == Some(engine)
        && held.model.as_deref().is_some_and(|held| crate::accounts::same_model(held, model))
        && data.settings.mail_to_cloud_in(AiSlot::Decide)
}

/// The exception stored for sorting mail (`ai.set_assignment inbox.categorize`), on or off.
fn stored_exception(data: &Data) -> Option<AiAssignment> {
    data.assignments
        .iter()
        .find(|info| info.assignment.function == AiFunction::InboxCategorize)
        .map(|info| info.assignment.clone())
        .filter(|held| held.engine.is_some())
}

/// What sorts mail now (issue #325), as the core weighs it: the exception while one is stored – one
/// switched off keeps sorting off –, else the slot Sorting as an assignment that is on.
pub fn in_effect(data: &Data) -> AiAssignment {
    let function = AiFunction::InboxCategorize;
    if let Some(own) = stored_exception(data) {
        return own;
    }
    match data.settings.ai_slots.get(AiSlot::Decide) {
        Some(slot) => AiAssignment {
            function,
            engine: Some(slot.engine.clone()),
            model: slot.model.clone(),
            effort: slot.effort,
            enabled: true,
        },
        None => AiAssignment::none(function),
    }
}

/// The exception with a model of its own: stored and on, so it sorts instead of the slot.
fn own_exception(data: &Data) -> Option<AiAssignment> {
    stored_exception(data).filter(|held| held.enabled)
}

/// Sorting on again: the exception switched off is taken back, so the slot sorts again. Without a
/// slot, it gets what sorted last or the first model offered on this computer. What sorted last in
/// the cloud without the consent asks first (issue #424): this never lets mail out by itself.
fn switched_on<A>(state: &mut State, data: &Data) -> Action<A> {
    let exception = stored_exception(data);
    let mut requests = Vec::new();
    let mut cloud = None;
    if data.settings.ai_slots.decide.is_none() {
        if let Some(held) = &exception
            && let Some(engine) = &held.engine
            && !data.settings.mail_to_cloud_in(AiSlot::Decide)
        {
            cloud = in_cloud(data, engine, held.model.as_deref());
        }
        let from_exception = exception
            .as_ref()
            .and_then(|held| held.engine.clone().map(|engine| set_slot(engine, held.model.clone(), held.effort)));
        let first = || {
            offered(data)
                .into_iter()
                .find(|(engine, model)| stays_here(engine, model))
                .map(|(engine, model)| set_slot(engine, Some(model.id), None))
        };
        match from_exception.or_else(first) {
            Some(request) => requests.push(request),
            None => return Action::None,
        }
    }
    requests.extend(exception.map(|_| no_exception()));
    match (requests.is_empty(), cloud) {
        (true, _) => Action::None,
        (false, Some(pick)) => {
            state.asking_cloud = Some(CloudPick { requests, ..pick });
            Action::None
        }
        (false, None) => Action::Send(requests),
    }
}

/// `ai.set_slot decide` with `engine` and `model`.
fn set_slot(engine: String, model: Option<String>, effort: Option<torro_core::ai::Effort>) -> Request {
    Request::SetAiSlot { slot: AiSlot::Decide, engine: Some(engine), model, effort }
}

/// `ai.set_assignment inbox.categorize` with `engine: null`: the exception taken back.
fn no_exception() -> Request {
    Request::SetAiAssignment {
        function: AiFunction::InboxCategorize,
        engine: None,
        model: None,
        effort: None,
        enabled: false,
    }
}

/// The models that may sort mail: every AI account's models that can – decision models, and chat
/// models with tools through the fallback (task MW.3) – and are turned on (task S.26). Those in the
/// cloud too (issue #424): picking one asks first.
pub(super) fn offered(data: &Data) -> Vec<(String, AiModel)> {
    deciders(data).into_iter().filter(|(_, model)| model.enabled).collect()
}

/// The models that can sort mail, turned on or not.
fn deciders(data: &Data) -> Vec<(String, AiModel)> {
    data.decider_models.iter().filter(|(_, model)| model.can_decide()).cloned().collect()
}

/// Whether `model` of the AI account `engine` computes on this computer, as far as the list tells: a
/// provider in the cloud never, whatever a model says; else what its server says, else by the account
/// – LM Studio, or a server of one's own at an address of this computer.
fn stays_here(engine: &str, model: &AiModel) -> bool {
    use torro_core::ai::Seat;
    if crate::accounts::ai_provider(engine)
        .is_some_and(|provider| !matches!(provider.seat(), Seat::ThisComputer | Seat::OwnServer))
    {
        return false;
    }
    if let Some(remote) = model.remote {
        return !remote;
    }
    let (provider, host) = engine.strip_prefix("ai:").and_then(|rest| rest.split_once('@')).unwrap_or_default();
    let lm_studio = torro_core::ai::AiProvider::LmStudio.id();
    let own = torro_core::ai::AiProvider::OpenAiCompatible.id();
    provider == lm_studio || (provider == own && torro_core::ai::is_this_computer(host))
}

/// The model assigned for sorting mail when sorting is on and that model of its AI account is turned
/// off (PR review #242): the core sorts nothing then. Its account and name as listed. `None` while
/// sorting is off, the model is on, "Automatic" is assigned, or the account's models are not in yet.
pub fn assigned_off(data: &Data) -> Option<(String, String)> {
    let held = in_effect(data);
    let (engine, model) = (held.engine.filter(|_| held.enabled)?, held.model?);
    let mut listed = data.decider_models.iter().filter(|(account, _)| *account == engine).peekable();
    listed.peek()?;
    match listed.find(|(_, known)| known.id == model) {
        Some((_, known)) if known.enabled => None,
        Some((_, known)) => Some((engine, known.name.clone())),
        None => Some((engine, model)),
    }
}

/// A model offered for sorting with its AI account (`ai:…`).
pub(super) type Offered = (String, AiModel);

/// The models offered ([`offered`]) in the parts of the menu, as [`grouped`] splits them.
pub(super) type Parts = (Vec<Offered>, Vec<Offered>, Vec<Offered>);

/// The models offered by where they compute (issue #424) and how they sort (issue #421): on this
/// computer the decision models, then the chat models, then those in the cloud, their decision
/// models first. In each part the accounts with a model on this computer first, each by account
/// name, then model name.
pub(super) fn grouped(data: &Data) -> Parts {
    let mut offered = offered(data);
    let here: BTreeSet<String> =
        offered.iter().filter(|(engine, model)| stays_here(engine, model)).map(|(engine, _)| engine.clone()).collect();
    offered.sort_by(|(a_engine, a_model), (b_engine, b_model)| {
        (!here.contains(a_engine))
            .cmp(&!here.contains(b_engine))
            .then_with(|| account_label(data, a_engine).cmp(&account_label(data, b_engine)))
            .then_with(|| a_engine.cmp(b_engine))
            .then_with(|| a_model.name.cmp(&b_model.name))
    });
    let decides = |(_, model): &Offered| model.decision == Some(true);
    let (local, cloud): (Vec<Offered>, Vec<Offered>) =
        offered.into_iter().partition(|(engine, model)| stays_here(engine, model));
    let (fast, other) = local.into_iter().partition(decides);
    let (cloud_fast, cloud_other): (Vec<Offered>, Vec<Offered>) = cloud.into_iter().partition(decides);
    (fast, other, cloud_fast.into_iter().chain(cloud_other).collect())
}

/// What a model says under its name (issue #431): its account and how big it is.
pub(super) fn model_facts(data: &Data, engine: &str, model: &AiModel) -> String {
    let mut facts = vec![account_label(data, engine)];
    facts.extend(crate::parts::size_facts(model));
    facts.join(" · ")
}

/// What the account `id` is called; the id itself when it is gone from the list.
fn account_label(data: &Data, id: &str) -> String {
    data.accounts
        .iter()
        .find(|account| account.id == id)
        .map(|account| account.label.clone())
        .unwrap_or_else(|| id.trim_start_matches("mail:").to_string())
}

/// Whether a model sorts mail now (PR review of #463): one is in effect, on, and not waiting for the
/// consent that mail leaves this computer. Else a run cannot apply it.
fn model_sorts(data: &Data) -> bool {
    assigned_off(data).is_none() && matches!(chosen(data), Chosen::Here(_) | Chosen::Cloud { waiting: false, .. })
}

/// Where the model that sorts stands, as the page says it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Chosen {
    /// Nothing sorts.
    Off,
    /// `name` on this computer.
    Here(String),
    /// `name` in the cloud; `waiting` without the consent, so the core sorts nothing with it yet.
    Cloud { name: String, provider: String, waiting: bool },
}

/// The model in effect as the page names it.
fn chosen(data: &Data) -> Chosen {
    let held = in_effect(data);
    let (true, Some(engine)) = (held.is_ready(), held.engine.clone()) else { return Chosen::Off };
    let listed = held.model.as_deref().and_then(|id| {
        data.decider_models
            .iter()
            .find(|(account, model)| *account == engine && crate::accounts::same_model(&model.id, id))
    });
    let name = match (listed, &held.model) {
        (Some((_, model)), _) => model.name.clone(),
        (None, Some(id)) => id.clone(),
        (None, None) => account_label(data, &engine),
    };
    match in_cloud(data, &engine, held.model.as_deref()) {
        Some(_) => Chosen::Cloud {
            name,
            provider: account_label(data, &engine),
            waiting: !data.settings.mail_to_cloud_in(AiSlot::Decide),
        },
        None => Chosen::Here(name),
    }
}

// ----- the order of rules and filters -----

// ----- the page -----

/// The section's page: a page of the editor (a category, a rule) with "‹ Back" and "Save" while one
/// is open, else the tabs with "Sort existing mail…" in the header bar.
pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
    if let Some((name, body, save)) = editor::page(&state.editor, data) {
        let back = Message::Editor(editor::Message::Back);
        return Page::new(body.map(Message::Editor)).sub_page(name, back).actions(vec![save.map(Message::Editor)]);
    }
    // A run at a time: while one goes or categories are being removed, it waits.
    let busy = data.category_job.as_ref().is_some_and(|job| job.state == CategoryJobState::Running)
        || data.categorization.clear_job.as_ref().is_some_and(|job| job.state == CategoryJobState::Running);
    let sort = widgets::text_button(
        None,
        tr("Sort existing mail…"),
        style::button::secondary,
        (!busy).then_some(Message::OpenSort { rules_only: false }),
    );
    // Every tab spans the whole width (issue #464), so head and tabs never move; the categories take
    // two columns where there is room, the rows of the other tabs keep their text at the reading width.
    Page::new(view(state, data)).actions(vec![sort])
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let subtitle = typo::text(typo::body(), tr("Fixed rules and the AI sort every new mail as it arrives."))
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph);
    let mut page = column![subtitle].spacing(layout::section_gap());
    if let Some(progress) = progress(data) {
        page = page.push(progress);
    }
    let labels = vec![(tr("Categories").to_string(), Some(data.categories.len() as u64)), (tr("AI").to_string(), None)];
    page = page.push(widgets::tabs(labels, state.tab.index(), Message::Tab));
    let tab = match state.tab {
        Tab::Categories => categories_tab(state, data),
        Tab::Ai => ai_tab(state, data),
    };
    page.push(tab).into()
}

/// "Sorting existing mail · 412 of 1,284 · about 4 min left" with its line and Pause/Resume and
/// Cancel, while a run goes, is paused or failed; else the same for an unfinished removal of the
/// categories ("Reset progress"). `None` while nothing is unfinished.
fn progress<'a>(data: &'a Data) -> Option<Element<'a, Message>> {
    let controls = |state: CategoryJobState, send: &dyn Fn(CategoryJobControl) -> Message| -> Element<'a, Message> {
        let (first, control) = match state {
            CategoryJobState::Running => (tr("Pause"), CategoryJobControl::Pause),
            _ => (tr("Resume"), CategoryJobControl::Resume),
        };
        // Cancelled already: only "Resume" is left.
        let cancel = (state != CategoryJobState::Cancelled).then(|| {
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(send(CategoryJobControl::Cancel)))
        });
        row![widgets::text_button(None, first, style::button::secondary, Some(send(control)))]
            .push(cancel)
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into()
    };
    // A cancelled run or removal stays too, with "Resume", as on the page before (review of #454).
    let unfinished = |state: CategoryJobState| {
        matches!(
            state,
            CategoryJobState::Running
                | CategoryJobState::Paused
                | CategoryJobState::Failed
                | CategoryJobState::Cancelled
        )
    };
    // What goes on wins over what was cancelled: a removal that runs, is paused or failed first,
    // then the run of "Sort existing mail", then a removal that was cancelled (review of #454).
    let going = |state: CategoryJobState| unfinished(state) && state != CategoryJobState::Cancelled;
    let clear = data.categorization.clear_job.as_ref();
    let job = data.category_job.as_ref().filter(|job| unfinished(job.state));
    let clear = clear.filter(|held| going(held.state) || (job.is_none() && unfinished(held.state)));
    let (words, done, total, buttons, errors) = if let Some(job) = clear {
        let removed: u64 = job.accounts.iter().map(|account| account.removed).sum();
        let selected: u64 = job.accounts.iter().map(|account| account.categorized).sum();
        let words = trf(
            "Reset progress · {done} of {total} · {state}",
            &[("done", &format_count(removed)), ("total", &format_count(selected)), ("state", &state_word(job.state))],
        );
        let id = job.id.clone();
        (
            words,
            removed,
            selected,
            controls(job.state, &move |action| Message::ControlClear(id.clone(), action)),
            &job.errors,
        )
    } else {
        let job = job?;
        let (done, total) = (job.processed(), job.left() + job.processed());
        let mut words = trf(
            "Sorting existing mail · {done} of {total}",
            &[("done", &format_count(done)), ("total", &format_count(total))],
        );
        match (job.state, job.left_seconds) {
            (CategoryJobState::Running, Some(seconds)) => {
                words = match rough_minutes(seconds) {
                    Some(time) => trf("{progress} · about {time} left", &[("progress", &words), ("time", &time)]),
                    None => trf("{progress} · less than a minute left", &[("progress", &words)]),
                };
            }
            (CategoryJobState::Running, None) => {}
            (state, _) => words = trf("{progress} · {state}", &[("progress", &words), ("state", &state_word(state))]),
        }
        let id = job.id.clone();
        (words, done, total, controls(job.state, &move |action| Message::ControlJob(id.clone(), action)), &job.errors)
    };
    let mut lines = column![
        row![
            container(typo::text(typo::meta().tabular(), words).style(theme::text_2).wrapping(Wrapping::WordOrGlyph))
                .width(Fill),
            buttons
        ]
        .spacing(space::l())
        .align_y(Alignment::Center),
        widgets::progress_line(done, total),
    ]
    .spacing(space::s());
    for error in errors.iter().take(3) {
        lines = lines.push(crate::parts::notice(error.clone()));
    }
    Some(lines.into())
}

/// `n` in words, with the words for one and for many: "1 category", "24 categories".
fn count_words(n: u64, one: &str, many: &'static str) -> String {
    match n {
        1 => one.to_string(),
        n => trf(many, &[("n", &format_count(n))]),
    }
}

/// About how long `seconds` take, in whole minutes ("13 min", "1 h 3 min") – a run's time is a
/// guess, and seconds would tick for nothing; `None` under a minute, which its sentence says.
fn rough_minutes(seconds: u64) -> Option<String> {
    (seconds >= 60).then(|| format_duration((seconds + 30) / 60 * 60_000))
}

/// What a person reads for the state of a run.
fn state_word(state: CategoryJobState) -> String {
    match state {
        CategoryJobState::Running => tr("running"),
        CategoryJobState::Paused => tr("paused"),
        CategoryJobState::Cancelled => torro_core::i18n::trc("job", "cancelled"),
        CategoryJobState::Complete => tr("complete"),
        CategoryJobState::Failed => tr("failed"),
    }
    .to_string()
}

// ----- tab AI -----

/// "Sort with model" with its menu and the line about the choice, the model's special cases, and
/// "Categories the model may choose".
fn ai_tab<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let current = chosen(data);
    let label = match &current {
        Chosen::Off => tr("Off").to_string(),
        Chosen::Here(name) | Chosen::Cloud { name, .. } => name.clone(),
    };
    let pick = widgets::select_button(&state.model_anchor, label, Message::ModelMenu(!state.model_menu));
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    // The model in effect, turned off since: the core sorts nothing with it (PR review #242).
    if let Some((engine, name)) = assigned_off(data) {
        let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) });
        let name = format!("{} · {name}", account_label(data, &engine));
        rows.push(setting_row(Some(alert.into()), name, Some(tr("Turned off in Settings › AI").to_string()), blank()));
    }
    let detail = match (&current, own_exception(data)) {
        (Chosen::Off, _) => None,
        (_, Some(_)) => Some(tr("Only for sorting mail").to_string()),
        (Chosen::Here(_), None) => Some(tr("On this computer").to_string()),
        (Chosen::Cloud { provider, .. }, None) => Some(trf("In the cloud at {provider}", &[("provider", provider)])),
    };
    rows.push(setting_row(None, tr("Sort with model"), detail, pick));
    // Only chat models on this computer (issue #421): the way to a fast one, which Settings › AI
    // offers to download.
    let (fast, other, _) = grouped(data);
    if fast.is_empty() && !other.is_empty() {
        let arrow = icon::icon(Icon::ArrowUpRight, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
        let detail = tr("Sorting runs slower with a chat model. Settings › AI suggests one to download.");
        rows.push(pressable(
            setting_row(None, tr("No decision model"), Some(detail.to_string()), arrow),
            Some(Message::ManageModels),
        ));
    }
    let none = offered(data).is_empty();
    let hint = match &current {
        _ if none && assigned_off(data).is_none() => match deciders(data).is_empty() {
            true => tr("No model can sort mail yet. Add a local AI such as Ollama under AI and load a model that decides."),
            false => tr("No model is turned on that can sort mail. Turn one on in Settings › AI."),
        }
        .to_string(),
        Chosen::Off => {
            tr("New mail gets its category from rules, filters and the headers only. Mail they leave open stays without one.")
                .to_string()
        }
        Chosen::Here(_) => tr(
            "The model runs on this computer. No mail leaves it; nothing is moved or deleted, and you can change any category by hand.",
        )
        .to_string(),
        Chosen::Cloud { waiting: true, .. } => tr("Waiting for your OK – mail stays on this computer until you choose the model again.").to_string(),
        Chosen::Cloud { provider, .. } => trf(
            "The model reads your mail at {provider}. Nothing is moved or deleted, and you can change any category by hand.",
            &[("provider", provider)],
        ),
    };
    let sorting = widgets::group(tr("Sorting"), rows, Some(hint));
    let chosen = data.categories.iter().filter(|info| info.model).count();
    // The row leads to the tab Categories, where "AI may not file mail here" is set per category.
    let may = pressable(
        setting_row(
            None,
            tr("Categories the model may choose"),
            Some(trf("{n} of {total}", &[("n", &chosen), ("total", &data.categories.len())])),
            crate::parts::chevron(),
        ),
        Some(Message::Tab(0)),
    );
    // A jump to Settings › AI, as "Appearance ↗" in the sidebar: no "…", nothing opens here.
    let arrow =
        icon::icon(Icon::ArrowUpRight, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let manage = pressable(setting_row(None, tr("Manage models"), None, arrow), Some(Message::ManageModels));
    column![sorting, widgets::group(tr("Model"), vec![may, manage], None)].spacing(layout::section_gap()).into()
}

/// The menu of "Sort with model": "Off", the models on this computer (decision models "Fast", chat
/// models "Slower"), those in the cloud ("Cloud"), and "More models: Settings › AI".
fn model_menu<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let (fast, other, cloud) = grouped(data);
    let held = in_effect(data);
    let consent = data.settings.mail_to_cloud_in(AiSlot::Decide);
    let items = |models: Vec<Offered>, tag: &'a str, in_cloud: bool| -> Vec<MenuItem<'a, Message>> {
        models
            .into_iter()
            .map(|(engine, model)| {
                let is = held.is_ready()
                    && held.engine.as_deref() == Some(engine.as_str())
                    && held.model.as_deref() == Some(model.id.as_str())
                    && (!in_cloud || consent);
                let item =
                    MenuItem::new(Icon::Check, model.name.clone(), Message::Model(engine.clone(), model.id.clone()))
                        .detail(model_facts(data, &engine, &model))
                        .tag(tag, false)
                        .current(is);
                let item = MenuItem { mark: None, ..item };
                if in_cloud { item.trail(Trail::Icon(Icon::Cloud)) } else { item }
            })
            .collect()
    };
    let off = MenuItem { mark: None, ..MenuItem::new(Icon::Check, tr("Off"), Message::Enabled(false)) }
        .current(!held.is_ready());
    let mut sections = vec![MenuSection::new(vec![off])];
    if !fast.is_empty() {
        sections.push(MenuSection::titled(tr("On this computer · decision models"), items(fast, tr("Fast"), false)));
    }
    if !other.is_empty() {
        sections.push(MenuSection::titled(tr("On this computer · chat models"), items(other, tr("Slower"), false)));
    }
    if !cloud.is_empty() {
        sections
            .push(MenuSection::titled(tr("In the cloud · mail leaves this computer"), items(cloud, tr("Cloud"), true)));
    }
    let more = MenuItem::new(torro_gui::shell::SETTINGS_ICON, tr("More models: Settings › AI"), Message::ManageModels);
    sections.push(MenuSection::new(vec![more]));
    let menu = iced::widget::scrollable(widgets::menu_wide(sections, size::panel()))
        .direction(iced::widget::scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Length::Shrink);
    let menu = container(menu).max_height(size::panel() * 2.0);
    widgets::floating(&state.model_anchor, Placement::Below(Align::End), menu, Message::ModelMenu(false))
}

// ----- tab Categories -----

/// The groups as cards with their categories, "New group…" above them, "Other filters" and
/// "Categories on mail" with "Remove categories from all mail…" below, set apart.
fn categories_tab<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let new = widgets::text_button(
        Some(Icon::Plus),
        tr("New group…"),
        style::button::secondary,
        Some(Message::Editor(editor::Message::NewGroup)),
    );
    // "New group…" belongs to the groups: at the right of their title line.
    let mut page = column![widgets::group_title(tr("Groups"), vec![new])].spacing(layout::section_gap());
    if let Some(result) = filters::apply_view(&state.filters) {
        page = page.push(result.map(Message::Filters));
    }
    if let Some(form) = editor::group_form_view(&state.editor, None) {
        page = page.push(widgets::group(tr("New group"), vec![form.map(Message::Editor)], None));
    }
    let groups = match data.category_groups.is_empty() {
        true => torro_mail::CategoryGroupDef::defaults().iter().map(torro_ipc::inbox::CategoryGroupInfo::of).collect(),
        false => data.category_groups.clone(),
    };
    for group in groups {
        page = page.push(group_card(state, data, group));
    }
    page = page.push(
        typo::text(
            typo::meta(),
            tr("The numbers count mail in Inbox. Where fixed rules of several categories match, the one higher up wins: drag a category by its grip to change the order."),
        )
        .style(theme::text_3)
        .wrapping(Wrapping::WordOrGlyph),
    );
    page = page.push(other_filters(data));
    let count = data.category_clear_preview.as_ref().map(|preview| preview.total).unwrap_or(0);
    let busy = data.categorization.clear_job.as_ref().is_some_and(|job| {
        matches!(job.state, CategoryJobState::Running | CategoryJobState::Paused | CategoryJobState::Failed)
    });
    let remove = widgets::text_button(
        None,
        tr("Remove categories from all mail…"),
        style::button::secondary,
        (count > 0 && !busy).then_some(Message::AskClear),
    );
    let detail = match data.updating(&Request::PreviewClearCategories) && data.category_clear_preview.is_none() {
        true => tr("Updating…").to_string(),
        false => match count {
            1 => tr("1 mail in every folder").to_string(),
            n => trf("{n} mails in every folder", &[("n", &format_count(n))]),
        },
    };
    let on_mail = widgets::group(
        tr("Categories on mail"),
        vec![setting_row(None, tr("Mail with a category"), Some(detail), remove)],
        Some(
            tr("Removing takes only the categories off the mail. Sorting stays as it is, and nothing is sorted again.")
                .to_string(),
        ),
    );
    page.push(blank().height(space::l())).push(on_mail).into()
}

/// A group's card: its head (fold, mark, how many categories – the first names while folded –, how
/// many mails, "…") and, unfolded, its categories in two columns with "+ New category in …".
fn group_card<'a>(
    state: &'a State,
    data: &'a Data,
    group: torro_ipc::inbox::CategoryGroupInfo,
) -> Element<'a, Message> {
    let id = group.group;
    let color = usize::from(group.color.min(4));
    let children: Vec<&'a CategoryInfo> = data.categories.iter().filter(|info| info.group == id).collect();
    let folded = state.folded.contains(&id);
    let mails: u64 = children.iter().map(|info| u64::from(info.messages)).sum();
    let mut head_words = count_words(children.len() as u64, tr("1 category"), tr("{n} categories"));
    if folded && !children.is_empty() {
        let names: Vec<&str> = children.iter().take(3).map(|info| info.label.as_str()).collect();
        let more = if children.len() > 3 { ", …" } else { "" };
        head_words = format!("{head_words} · {}{more}", names.join(", "));
    }
    let chevron = if folded { Icon::ChevronRight } else { Icon::ChevronDown };
    let mark = icon::icon(editor::category_icon(group.icon), icon::SIZE)
        .style(move |t: &Theme, _| svg::Style { color: Some(palette(t).category[color]) });
    let open = state.group_menu == Some(id);
    let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::GroupMenu((!open).then_some(id))));
    let more = widgets::anchored_if(open.then_some(&state.group_anchor), more);
    // The group's name leads its card; how many categories and mails follow, quieter.
    let head = row![
        icon::icon(chevron, icon::SIZE_SM).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) }),
        mark,
        typo::text(typo::body_strong(), group.label.clone()).wrapping(Wrapping::None),
        container(widgets::ellipsized(typo::meta(), head_words, theme::text_3)).width(Fill),
        typo::text(typo::meta().tabular(), count_words(mails, tr("1 mail"), tr("{n} mails"))).style(theme::text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let head = row![pressable(row_frame(head, false), Some(Message::Fold(id))), more]
        .align_y(Alignment::Center)
        .padding(iced::Padding::new(0.0).right(space::l()));
    let mut rows: Vec<Element<'a, Message>> = vec![head.into()];
    if let Some(form) = editor::group_form_view(&state.editor, Some(id)) {
        rows.push(form.map(Message::Editor));
    }
    if !folded {
        let dragging = state.category_drag.map(|(category, _)| category);
        let over = state.category_drag.and_then(|(_, over)| over).filter(|(held, _)| *held == id).map(|(_, at)| at);
        let count = children.len();
        for (at, info) in children.into_iter().enumerate() {
            if over == Some(at) {
                rows.push(container(widgets::drop_line()).padding([0.0, space::l()]).into());
            }
            let cell = category_cell(data, info, color, dragging == Some(info.category));
            rows.push(match dragging {
                Some(_) => mouse_area(cell).on_enter(Message::OverSlot(id, at)).into(),
                None => cell,
            });
        }
        if over == Some(count) {
            rows.push(container(widgets::drop_line()).padding([0.0, space::l()]).into());
        }
        let add = trf("New category in {group}", &[("group", &group.label)]);
        let plus =
            icon::icon(Icon::Plus, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_2) });
        let add = pressable(
            setting_row(Some(plus.into()), add, None, blank()),
            Some(Message::Editor(editor::Message::NewCategoryInGroup(id))),
        );
        // Over the last row of the card the category goes to the end of the group.
        rows.push(match dragging {
            Some(_) => mouse_area(add).on_enter(Message::OverSlot(id, count)).into(),
            None => add,
        });
    } else if state.category_drag.is_some() {
        // A folded group takes a category at its end, over its head.
        let len = data.categories.iter().filter(|info| info.group == id).count();
        if let Some(head) = rows.pop() {
            rows.push(mouse_area(head).on_enter(Message::OverSlot(id, len)).into());
        }
    }
    let card = widgets::group_with(None, rows, None);
    match state.category_drag {
        Some(_) => mouse_area(card).on_exit(Message::LeaveGroup(id)).interaction(mouse::Interaction::Grabbing).into(),
        None => card,
    }
}

/// "Other filters" (issue #481): the mail filters that set no category – they mark mail read or
/// move it to the trash –, each with its switch and a chevron that opens its dialog, and "New
/// filter…". The filters that set a category stand on that category's page.
fn other_filters<'a>(data: &'a Data) -> Element<'a, Message> {
    let new =
        widgets::text_button(Some(Icon::Plus), tr("New filter…"), style::button::secondary, Some(Message::NewFilter));
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for filter in data.mail_filters.iter().filter(|filter| filter.category.is_none()) {
        let id = filter.id.clone();
        let switch = switch(filter.enabled, move |on| Message::Filters(filters::Message::Enabled(id.clone(), on)));
        let title: fn(&Theme) -> iced::widget::text::Style = if filter.enabled { theme::text } else { theme::text_2 };
        let mut then = filters::actions_line(filter, data);
        if !filter.enabled {
            then = trf("Off · {rule}", &[("rule", &then)]);
        }
        let lines = column![
            widgets::ellipsized(typo::body(), filter.query.clone(), title),
            widgets::ellipsized(typo::meta(), then, theme::text_2),
        ];
        let line =
            row![lines.width(Fill), switch, crate::parts::chevron()].spacing(space::m()).align_y(Alignment::Center);
        let open = Message::Filters(filters::Message::Edit(filter.id.clone()));
        rows.push(pressable(row_frame(line, true), Some(open)));
    }
    if rows.is_empty() {
        let none = typo::text(typo::body(), tr("No other filters.")).style(theme::text_2);
        rows.push(row_frame(none, false));
    }
    widgets::group_with(
        Some(widgets::group_title(tr("Other filters"), vec![new])),
        rows,
        Some(tr("Filters that set no category: they mark mail as read or move it to the trash.").to_string()),
    )
}

/// A category's cell: grip, the dot of its group, its name with "From headers" for one the headers
/// decide, how many mails, "Show in the sidebar"; a click opens its page.
fn category_cell<'a>(data: &'a Data, info: &'a CategoryInfo, color: usize, dragged: bool) -> Element<'a, Message> {
    let category = info.category;
    let dot = container(blank().width(space::s()).height(space::s())).style(move |t: &Theme| container::Style {
        background: Some(iced::Background::Color(palette(t).category[color])),
        border: iced::border::rounded(space::s()),
        ..container::Style::default()
    });
    let mut name = row![widgets::ellipsized(typo::body(), info.label.clone(), theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if category.by_rule_only() {
        name = name.push(widgets::state_pill(Icon::FileText, tr("From headers")));
    }
    let shown = !data.settings.hidden_categories.contains(&category);
    let lead = row![widgets::grip(Message::CategoryGrip(category), tr("Drag to change the order")), dot]
        .spacing(space::xs())
        .align_y(Alignment::Center);
    let line = row![
        lead,
        container(name).width(Fill),
        typo::text(typo::meta().tabular(), format_count(u64::from(info.messages))).style(theme::text_3),
        widgets::with_tooltip(switch(shown, move |on| Message::Shown(category, on)), tr("Show in the sidebar")),
    ]
    .spacing(space::m())
    .align_y(Alignment::Center);
    // The one on its way says so where its grip was taken: it opens nothing.
    let open = (!dragged).then_some(Message::Editor(editor::Message::EditCategory(category)));
    // As high as a row of a list, not of a settings group: a group holds many of them.
    let line = container(line).width(Fill).center_y(size::row()).padding([0.0, space::l()]);
    pressable(line.into(), open)
}

/// The "…" menu of the group whose menu is open: "Edit group…".
fn group_menu<'a>(state: &'a State) -> Option<Element<'a, Message>> {
    let group = state.group_menu?;
    let items = vec![
        MenuItem::new(Icon::SquarePen, tr("Edit group…"), Message::Editor(editor::Message::EditGroup(group))),
        MenuItem::new(Icon::Plus, tr("New category…"), Message::Editor(editor::Message::NewCategoryInGroup(group))),
    ];
    let menu = container(widgets::menu(vec![MenuSection::new(items)])).width(widgets::menu_width());
    Some(widgets::floating(&state.group_anchor, Placement::Below(Align::End), menu, Message::GroupMenu(None)))
}

// ----- "Sort existing mail" -----

/// The dialog "Sort existing mail": accounts, folders, time span, what to apply, what to do with
/// mail that has a category, the preview with how long it takes, "Start sorting".
fn sort_dialog<'a>(
    state: &'a State,
    form: &'a SortForm,
    data: &'a Data,
    anchors: &SortAnchors,
) -> Element<'a, Message> {
    let accounts = match form.accounts.as_slice() {
        [] => tr("All accounts").to_string(),
        [one] => account_label(data, one),
        many => trf("{n} accounts", &[("n", &many.len())]),
    };
    let folders = match form.folders.as_slice() {
        [] => tr("All folders").to_string(),
        [one] => folder_label(data, one),
        many => trf("{n} folders", &[("n", &many.len())]),
    };
    let toggle = |menu| Message::SortMenu((form.menu != Some(menu)).then_some(menu));
    let model_ready = model_sorts(data);
    let choices = vec![
        setting_row(
            None,
            tr("Accounts"),
            None,
            choice_box(widgets::select_button_fill(&anchors.accounts, accounts, toggle(SortMenu::Accounts))),
        ),
        setting_row(
            None,
            tr("Folders"),
            None,
            choice_box(widgets::select_button_fill(&anchors.folders, folders, toggle(SortMenu::Folders))),
        ),
        setting_row(
            None,
            tr("Time span"),
            None,
            choice_box(crate::parts::picker_fill(&Period::ALL[..], form.period, Message::Period)),
        ),
    ];
    let model_detail = match chosen(data) {
        Chosen::Off => tr("No model sorts mail now").to_string(),
        Chosen::Here(name) => name,
        Chosen::Cloud { name, .. } => trf("{model} · in the cloud", &[("model", &name)]),
    };
    let apply = vec![
        check_row(
            tr("Fixed rules"),
            Some(tr("Sorting rules and filters; filters look at Inbox only").to_string()),
            form.rules,
            Some(Message::ApplyRules(!form.rules)),
        ),
        check_row(tr("Model"), Some(model_detail), form.model, model_ready.then_some(Message::ApplyModel(!form.model))),
    ];
    let held = vec![
        choice(tr("Leave as it is").to_string(), !form.again, Message::Again(false)),
        choice(tr("Ask the AI again (your own choices stay)").to_string(), form.again, Message::Again(true)),
    ];
    let preview = match (&state.preview, &state.preview_error, form.rules || form.model) {
        (_, _, false) => tr("Choose rules and filters, the model or both.").to_string(),
        (_, Some(problem), _) => problem.clone(),
        (Some(preview), None, _) => match preview.seconds {
            Some(seconds) => match rough_minutes(seconds) {
                Some(time) => {
                    trf("{n} mails · about {time}", &[("n", &format_count(preview.eligible)), ("time", &time)])
                }
                None => trf("{n} mails · less than a minute", &[("n", &format_count(preview.eligible))]),
            },
            None => trf("{n} mails", &[("n", &format_count(preview.eligible))]),
        },
        (None, None, _) => tr("Counting…").to_string(),
    };
    let running = data.category_job.as_ref().is_some_and(|job| job.state == CategoryJobState::Running)
        || data.categorization.clear_job.as_ref().is_some_and(|job| {
            matches!(job.state, CategoryJobState::Running | CategoryJobState::Paused | CategoryJobState::Failed)
        });
    let ready =
        !running && state.preview.as_ref().is_some_and(|preview| preview.eligible > 0) && state.pending_scope.is_none();
    let buttons = row![
        typo::text(typo::body_strong().tabular(), preview).wrapping(Wrapping::WordOrGlyph).width(Fill),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CloseSort)),
        widgets::text_button(None, tr("Start sorting"), style::button::primary, ready.then_some(Message::StartJob)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let content = column![
        typo::text(typo::title(), tr("Sort existing mail")),
        typo::text(typo::body(), tr("New mail is sorted as it arrives. This sorts mail that is already here, once."))
            .style(theme::text_2),
        widgets::group(tr("Which mail"), choices, None),
        widgets::group(tr("Apply"), apply, None),
        widgets::group(tr("Mail that already has a category"), held, None),
        typo::text(
            typo::meta(),
            tr("The preview and progress count all selected mail. Sidebar badges count unread mail only.")
        )
        .style(theme::text_3),
        buttons,
    ]
    .spacing(space::l());
    crate::parts::dialog(
        iced::widget::scrollable(content).style(style::scrollable),
        size::form_max(),
        Message::CloseSort,
    )
}

/// A choice of a dialog's row, as wide as the others: the fields of a dialog line up (design review
/// of #454).
pub(super) fn choice_box<'a, M: 'a>(choice: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(choice).width(size::sidebar()).into()
}

/// The anchors of the dialog's two menus, made in [`overlay`] and shared with them.
#[derive(Debug, Default)]
struct SortAnchors {
    accounts: Anchor,
    folders: Anchor,
}

/// The menu of accounts or folders of the dialog, each with a check while chosen.
fn sort_menu<'a>(form: &'a SortForm, data: &'a Data, menu: SortMenu, anchors: &SortAnchors) -> Element<'a, Message> {
    let item = |label: String, on: bool, message: Message| {
        MenuItem { mark: None, ..MenuItem::new(Icon::Check, label, message) }.current(on)
    };
    let (anchor, items) = match menu {
        SortMenu::Accounts => {
            let mut items = vec![item(tr("All accounts").to_string(), form.accounts.is_empty(), Message::AllAccounts)];
            items.extend(data.mail_stats.iter().map(|stats| {
                let on = form.accounts.contains(&stats.account);
                item(account_label(data, &stats.account), on, Message::ToggleAccount(stats.account.clone()))
            }));
            (&anchors.accounts, items)
        }
        SortMenu::Folders => {
            let mut items = vec![item(tr("All folders").to_string(), form.folders.is_empty(), Message::AllFolders)];
            let several = data.mail_stats.len() > 1;
            for stats in &data.mail_stats {
                if !form.accounts.is_empty() && !form.accounts.contains(&stats.account) {
                    continue;
                }
                for folder in &stats.folders {
                    let label = match several {
                        true => format!("{} · {}", account_label(data, &stats.account), folder.folder.name),
                        false => folder.folder.name.clone(),
                    };
                    let on = form.folders.contains(&folder.folder.id);
                    items.push(item(label, on, Message::ToggleFolder(folder.folder.id.clone())));
                }
            }
            (&anchors.folders, items)
        }
    };
    let menu = iced::widget::scrollable(widgets::menu(vec![MenuSection::new(items)]))
        .style(style::scrollable)
        .height(Length::Shrink);
    let menu = container(menu).max_height(size::panel()).width(widgets::menu_width());
    widgets::floating(anchor, Placement::Below(Align::End), menu, Message::SortMenu(None))
}

/// The name of a folder (`f:…`) by the account statistics; the id while it is not known.
fn folder_label(data: &Data, id: &str) -> String {
    data.mail_stats
        .iter()
        .flat_map(|stats| &stats.folders)
        .find(|folder| folder.folder.id == id)
        .map_or_else(|| id.to_string(), |folder| folder.folder.name.clone())
}

/// A row with a check box: `label`, its `detail`, ticked or not; the whole row ticks it. Without
/// `on_press` it is drawn disabled.
pub(super) fn check_row<'a, M: Clone + 'a>(
    label: impl iced::widget::text::IntoFragment<'a>,
    detail: Option<String>,
    ticked: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut check = checkbox(ticked).size(style::CHECK_SIZE).style(check_style);
    if let Some(message) = on_press.clone() {
        check = check.on_toggle(move |_| message.clone());
    }
    pressable(setting_row(Some(check.into()), label, detail, blank()), on_press)
}

/// `text_2` for the box: a black box would be the heaviest mark of the dialog.
fn check_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    style::checkbox(palette(theme).text_2)(theme, status)
}

/// What lies over the window while it is open, the topmost first: the question before a model in
/// the cloud sorts, "Sort existing mail" with its menus, "Delete …?", "Remove categories from all
/// mail?", the filter's dialog, then a menu of the page.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(pick) = &state.asking_cloud {
        let provider = account_label(data, &pick.engine);
        return Some(crate::parts::cloud_consent(&pick.name, &provider, Message::UseCloud, Message::CancelCloud));
    }
    if let Some(form) = &state.sort {
        let anchors = SortAnchors::default();
        let dialog = sort_dialog(state, form, data, &anchors);
        return Some(match form.menu {
            Some(menu) => iced::widget::stack![dialog, sort_menu(form, data, menu, &anchors)].into(),
            None => dialog,
        });
    }
    if state.asking_clear {
        let count = data.category_clear_preview.as_ref().map(|preview| preview.total).unwrap_or(0);
        return Some(crate::parts::confirm(
            tr("Remove categories from all mail?"),
            trf(
                "{n} mails lose their category. Mail, folders, rules and filters stay, and new mail is sorted as before.",
                &[("n", &format_count(count))],
            ),
            tr("Remove from all mail"),
            Message::ConfirmClear,
            Message::CloseClear,
        ));
    }
    if let Some(dialog) = filters::overlay(&state.filters, data) {
        return Some(dialog.map(Message::Filters));
    }
    if let Some(popover) = editor::overlay(&state.editor, data) {
        return Some(popover.map(Message::Editor));
    }
    if state.model_menu {
        return Some(model_menu(state, data));
    }
    group_menu(state)
}

/// The messages that put the page into a state of `--demo --open categorization-…` (torroinbox):
/// a tab, a dialog or the menu of the model, as a person would click it.
pub fn scene(name: &str) -> Option<Vec<Message>> {
    Some(match name {
        "ai" => vec![Message::Tab(1)],
        "categories" => Vec::new(),
        "category" => vec![Message::Editor(editor::Message::EditCategory(Category::Invoice))],
        "sort" => vec![Message::OpenSort { rules_only: false }],
        "filter" => vec![Message::NewFilter],
        "running" => vec![Message::OpenSort { rules_only: false }, Message::StartJob],
        "model" => vec![Message::ModelMenu(true)],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_ipc::inbox::{CategoryClearJob, CategoryClearProgress, CategoryJob, CategoryJobAccount};
    use torro_mail::CategoryDef;

    use super::*;

    /// The demo's data, as the mail window loads it.
    async fn demo() -> Data {
        let demo: Arc<dyn crate::Backend> = Arc::new(crate::demo::Demo::new());
        let data = crate::load(demo, super::super::wanted()).await;
        assert!(data.error.is_none(), "{:?}", data.error);
        data
    }

    fn sent<A: std::fmt::Debug>(action: Action<A>) -> Vec<Request> {
        match action {
            Action::Send(requests) => requests,
            Action::None => Vec::new(),
            other => panic!("a request or nothing, not {other:?}"),
        }
    }

    /// Issue #481: the page opens on Categories; the tabs are Categories, then AI – "Rules & filters"
    /// is gone; "Sort existing mail…" stands in the header bar; every tab draws over the demo.
    #[tokio::test]
    async fn the_page_has_its_head_and_two_tabs() {
        let data = demo().await;
        let mut state = State::default();
        assert_eq!(state.tab, Tab::Categories);
        assert_eq!(Tab::ALL, [Tab::Categories, Tab::Ai]);
        {
            let page = page(&state, &data);
            assert_eq!(page.actions.len(), 1, "Sort existing mail…");
            assert!(page.sub_page.is_none());
        }
        for (index, tab) in [(1, Tab::Ai), (0, Tab::Categories), (9, Tab::Categories)] {
            let _ = update::<()>(&mut state, Message::Tab(index), &data);
            assert_eq!(state.tab, tab);
            let _ = view(&state, &data);
        }
        assert!(scene("rules").is_none(), "no tab of rules any more");
        assert_eq!(scene("category").map(|messages| messages.len()), Some(1));
        assert!(scene("nope").is_none());
    }

    /// Issue #454: the model is chosen in a menu with "Off" first, no switch; "Off" keeps sorting
    /// off by the exception, and the menu draws with its parts.
    #[tokio::test]
    async fn the_model_is_a_menu_with_off_first() {
        let mut data = demo().await;
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::ModelMenu(true), &data);
        assert!(overlay(&state, &data).is_some(), "the menu");
        assert!(matches!(escape(&state), Some(Message::ModelMenu(false))));
        data.settings.ai_slots.decide = Some(torro_core::ai::SlotModel {
            engine: "ai:ollama@localhost:11434".into(),
            model: Some("nimble".into()),
            effort: None,
        });
        let off = sent(update::<()>(&mut state, Message::Enabled(false), &data));
        assert!(matches!(off.as_slice(), [Request::SetAiAssignment { enabled: false, .. }]), "{off:?}");
        assert!(!state.model_menu, "picking closes the menu");
        let _ = model_menu(&state, &data);
        let _ = view(&state, &data);
    }

    /// Issue #481: the filters without a category stand under "Other filters" with their switch; a
    /// click opens their dialog, "New filter…" a new one, and a new filter's dialog opens over the tab
    /// Categories without changing the tab.
    #[tokio::test]
    async fn filters_without_a_category_stand_under_other_filters() {
        let data = demo().await;
        let mut state = State::default();
        assert!(data.mail_filters.iter().any(|filter| filter.category.is_none()), "the demo has one");
        let _ = view(&state, &data);
        let off =
            sent(update::<()>(&mut state, Message::Filters(filters::Message::Enabled("f4".into(), false)), &data));
        assert!(matches!(off.as_slice(), [Request::UpdateMailFilter { enabled: Some(false), .. }]), "{off:?}");
        let _ = update::<()>(&mut state, Message::Filters(filters::Message::Edit("f4".into())), &data);
        assert!(state.filters.is_open() && overlay(&state, &data).is_some());
        let _ = update::<()>(&mut state, Message::Filters(filters::Message::Close), &data);
        let _ = update::<()>(&mut state, Message::Tab(1), &data);
        let new = filters::Message::New { query: "from:a@b.example".into(), account: None };
        let _ = update::<()>(&mut state, Message::Filters(new), &data);
        assert_eq!(state.tab, Tab::Ai, "the dialog opens where the page stands");
        assert!(state.filters.is_open());
        let _ = update::<()>(&mut state, Message::Filters(filters::Message::Close), &data);
        let _ = update::<()>(&mut state, Message::NewFilter, &data);
        assert!(state.filters.is_open() && overlay(&state, &data).is_some());
    }

    /// Issue #481: a category's page shows its rules and filters; "Filter from a search…" opens a
    /// filter's dialog that sets the category, a filter's row its dialog, over the page.
    #[tokio::test]
    async fn a_categorys_page_opens_its_filters() {
        let data = demo().await;
        let mut state = State::default();
        for message in scene("category").unwrap_or_default() {
            let _ = update::<()>(&mut state, message, &data);
        }
        assert!(page(&state, &data).sub_page.is_some(), "the page of Invoice");
        let new = editor::Message::Filter(filters::Message::NewFor(Category::Invoice));
        let _ = update::<()>(&mut state, Message::Editor(new), &data);
        assert!(state.filters.is_open() && overlay(&state, &data).is_some(), "the dialog over the page");
        let _ = update::<()>(&mut state, Message::Filters(filters::Message::Close), &data);
        let open = editor::Message::Filter(filters::Message::Edit("f2".into()));
        let _ = update::<()>(&mut state, Message::Editor(open), &data);
        assert!(state.filters.is_open());
        assert!(page(&state, &data).sub_page.is_some(), "the page stays below");
    }

    /// Issue #454 and #481: the groups fold and unfold, a category dragged moves to the slot of the drop
    /// line – in its group or into another, its own place counting out –, a new category starts in
    /// its group, and removing the categories removes only them.
    #[tokio::test]
    async fn categories_stand_in_their_groups() {
        let data = demo().await;
        let mut state = State::default();
        let _ = view(&state, &data);
        let group = data.categories[0].group;
        let _ = update::<()>(&mut state, Message::Fold(group), &data);
        assert!(state.folded.contains(&group));
        let _ = view(&state, &data);
        let _ = update::<()>(&mut state, Message::Fold(group), &data);
        assert!(!state.folded.contains(&group));

        let category = data.categories[0].category;
        let other = data.categories.iter().map(|info| info.group).find(|held| *held != group).expect("two groups");
        let drag = |state: &mut State, group, at| {
            let _ = update::<()>(state, Message::CategoryGrip(category), &data);
            let _ = update::<()>(state, Message::OverSlot(group, at), &data);
            let _ = view(state, &data);
            sent(update::<()>(state, Message::DropCategory, &data))
        };
        // On its own place, or right below it: nothing moves.
        assert!(drag(&mut state, group, 0).is_empty(), "its own place");
        assert!(drag(&mut state, group, 1).is_empty(), "right below itself");
        // Further down its group: the place among the others.
        let moved = drag(&mut state, group, 3);
        assert_eq!(moved, [Request::MoveCategory { category, group, position: 2 }]);
        // Into another group, at its slot there.
        let moved = drag(&mut state, other, 1);
        assert_eq!(moved, [Request::MoveCategory { category, group: other, position: 1 }]);

        let _ = update::<()>(&mut state, Message::Editor(editor::Message::NewCategoryInGroup(other)), &data);
        assert!(page(&state, &data).sub_page.is_some(), "the new category's page");
        let _ = update::<()>(&mut state, Message::Editor(editor::Message::Back), &data);
        let _ = update::<()>(&mut state, Message::GroupMenu(Some(other)), &data);
        assert!(overlay(&state, &data).is_some());
        let _ = update::<()>(&mut state, Message::Editor(editor::Message::EditGroup(other)), &data);
        assert!(overlay(&state, &data).is_none(), "the menu goes with its click");
        // A drag let go outside every card ends without a move.
        let _ = update::<()>(&mut state, Message::CategoryGrip(category), &data);
        let _ = update::<()>(&mut state, Message::OverSlot(other, 0), &data);
        let _ = update::<()>(&mut state, Message::LeaveGroup(group), &data);
        assert!(state.category_drag.is_some_and(|(_, over)| over == Some((other, 0))), "another card's leave");
        let _ = update::<()>(&mut state, Message::LeaveGroup(other), &data);
        assert!(sent(update::<()>(&mut state, Message::DropCategory, &data)).is_empty());
        assert!(state.category_drag.is_none());
        let _ = view(&state, &data);

        let _ = update::<()>(&mut state, Message::AskClear, &data);
        assert!(matches!(escape(&state), Some(Message::CloseClear)));
        let cleared = sent(update::<()>(&mut state, Message::ConfirmClear, &data));
        assert_eq!(cleared, [Request::ClearCategories { all_accounts: true, disable_automatic: false }]);
    }

    /// Issue #454: the line of a run or a removal stands under the head while it is unfinished, with
    /// Pause or Resume and Cancel – a failed removal too ("Reset progress"), a cancelled run with
    /// Resume –, and not when done.
    #[test]
    fn unfinished_runs_show_their_progress() {
        let job = |state| CategoryJob {
            id: "job".into(),
            scope: CategoryJobScope::default(),
            state,
            accounts: vec![CategoryJobAccount {
                account: "mail:anna@example.org".into(),
                max_sequence: 10,
                cursor: 4,
                eligible: 10,
                processed: 4,
                changed: 3,
                failed: 0,
            }],
            errors: vec!["mail:anna@example.org: the store is busy".into()],
            started_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            generation: 1,
            model_in_cloud: None,
            sorted_with: None,
            run_millis: 2_000,
            left_seconds: Some(3),
        };
        for state in [CategoryJobState::Running, CategoryJobState::Paused, CategoryJobState::Failed] {
            let data = Data { category_job: Some(job(state)), ..Data::default() };
            assert!(progress(&data).is_some(), "{state:?}");
        }
        let cancelled = Data { category_job: Some(job(CategoryJobState::Cancelled)), ..Data::default() };
        assert!(progress(&cancelled).is_some(), "a cancelled run can be resumed, as before");
        let done = Data { category_job: Some(job(CategoryJobState::Complete)), ..Data::default() };
        assert!(progress(&done).is_none());
        let clearing = CategoryClearJob {
            id: "clear".into(),
            state: CategoryJobState::Failed,
            accounts: vec![CategoryClearProgress {
                account: "mail:anna@example.org".into(),
                max_sequence: 10,
                cursor: 2,
                categorized: 10,
                removed: 2,
            }],
            errors: Vec::new(),
            started_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            generation: 1,
            disable_automatic: false,
            entries_cleared: false,
        };
        let mut data = Data::default();
        data.categorization.clear_job = Some(clearing);
        assert!(progress(&data).is_some(), "a failed removal");
        let _ = view(&State::default(), &data);
    }

    /// Issue #454: the dialog's scope follows its choices; a model is applied only while one sorts,
    /// and only the preview of the choice asked last counts.
    #[test]
    fn the_dialog_asks_for_what_it_shows() {
        let categories = CategoryDef::defaults().iter().map(|def| CategoryInfo::of(def, 0, 0)).collect();
        let data = Data { categories, ..Data::default() };
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::OpenSort { rules_only: false }, &data);
        let _ = update::<()>(&mut state, Message::ToggleAccount("mail:anna@example.org".into()), &data);
        let _ = update::<()>(&mut state, Message::SortMenu(Some(SortMenu::Folders)), &data);
        assert!(matches!(escape(&state), Some(Message::SortMenu(None))));
        assert!(overlay(&state, &data).is_some(), "dialog and menu");
        let asked = update::<()>(&mut state, Message::Period(Period::Days30), &data);
        let Action::Ask(Request::PreviewCategoryJob { scope }) = asked else { panic!("a preview") };
        assert_eq!(scope.accounts, ["mail:anna@example.org"]);
        assert!(scope.since.is_some());
        let old = CategoryJobPreview { eligible: 9, request_scope: None, ..CategoryJobPreview::default() };
        answered(&mut state, &Ok(Response::CategoryJobPreview { preview: old }));
        assert!(state.preview.is_none(), "an answer to another choice");
        let current = CategoryJobPreview {
            eligible: 9,
            seconds: Some(42),
            request_scope: Some(scope),
            ..CategoryJobPreview::default()
        };
        answered(&mut state, &Ok(Response::CategoryJobPreview { preview: current }));
        assert!(state.preview.is_some());
        let _ = overlay(&state, &data);
        assert!(matches!(update::<()>(&mut state, Message::StartJob, &data), Action::Send(_)));

        // PR review of #463: a refusal of an older preview does not hide the newer answer.
        let _ = update::<()>(&mut state, Message::OpenSort { rules_only: false }, &data);
        let _ = update::<()>(&mut state, Message::Period(Period::Year), &data);
        let Some(newer) = state.pending_scope.clone() else { panic!("asked") };
        answered(&mut state, &Err("offline".into()));
        assert!(state.preview_error.is_none(), "the newer one is still out");
        let answer = CategoryJobPreview { eligible: 3, request_scope: Some(newer), ..CategoryJobPreview::default() };
        answered(&mut state, &Ok(Response::CategoryJobPreview { preview: answer }));
        assert!(state.preview.is_some() && state.preview_error.is_none());
        // A model in the cloud that waits for the consent is not applied.
        let mut waiting = data.clone();
        waiting.settings.ai_slots.decide = Some(torro_core::ai::SlotModel {
            engine: "ai:openrouter@openrouter.ai".into(),
            model: Some("qwen".into()),
            effort: None,
        });
        assert!(!model_sorts(&waiting));
        let _ = update::<()>(&mut state, Message::OpenSort { rules_only: false }, &waiting);
        assert!(state.sort.as_ref().is_some_and(|form| !form.model));
    }
}
