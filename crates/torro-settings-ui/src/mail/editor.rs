//! The editor of the mail categories and the sorting rules (task E15.5, issue #255,
//! `docs/kategorien.md`, "Eigene Kategorien und Sortierregeln"), part of Settings ›
//! Categorization in the mail window.
//!
//! ```text
//! Sorting rules   one row per rule, in its order: category, conditions        + New sorting rule
//! Categories      + New category; then one group per group of categories, a row opens its page
//! ```
//!
//! A category's page: name, group, description, "Let the AI choose it" with how many of its group
//! the AI chooses between ("7 of 20"), whether the sidebar shows it, for one of the suite "Reset"
//! and for one of the person's own "Remove…" with the category its mail goes to. A rule's page:
//! "File as", the conditions and the exceptions (field and words each), on/off, its place with
//! "Move up"/"Move down", and "Delete rule". "Save" stands in the header bar.
//!
//! Everything is a request (`inbox.create_category`, `inbox.update_category`,
//! `inbox.remove_category`, `inbox.create_sort_rule`, `inbox.update_sort_rule`,
//! `inbox.remove_sort_rule`, `inbox.move_sort_rule`, `settings.set_setting`); the page checks only
//! what a person must fix before saving – the core checks all of it again. Mistakes show after
//! the first "Save", under the field they are about, and "Save" puts the cursor into the first
//! one it can (`.claude/skills/torro-design`, forms). What would lose something – deleting a rule,
//! resetting a category, leaving a page with changes – asks on the page first.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, space as blank, text_editor};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{palette, space};
use torro_gui::{layout, style, typo, widgets};
use torro_inbox::item::{SORT_CONDITIONS_MAX, SORT_VALUE_MAX};
use torro_inbox::{SortCondition, SortField, SortRule};
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::{Request, SettingKey};
use torro_mail::category::{DESCRIPTION_MAX, MODEL_PER_GROUP_MAX, NAME_MAX};
use torro_mail::{Category, CategoryGroup};

use crate::parts::{chevron, choice, field, picker, pressable, row_frame, setting_row, switch};
use crate::{Action, Data};

/// The id of the name field of a category's page: "Save" with a mistake in it goes there.
const NAME_FIELD: &str = "mail-category-name";

/// The id of the first condition's field of a rule's page.
const CONDITION_FIELD: &str = "mail-rule-condition";

/// What the editor keeps between two frames: the page that is open, and the question it asks.
#[derive(Debug, Default)]
pub struct State {
    form: Option<Form>,
    asking: Option<Ask>,
}

impl State {
    /// A page of the editor is open: the section shows it instead of its list.
    pub fn is_open(&self) -> bool {
        self.form.is_some()
    }
}

/// What the page asks before it does something that loses what was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Leave the page and drop what was typed.
    Discard,
    /// Put back the suite's name and description of a category.
    Reset,
    /// Delete the open rule.
    Delete,
}

/// The page that is open.
#[derive(Debug)]
enum Form {
    Category(CategoryForm),
    Rule(RuleForm),
}

/// A category as it is being typed: a new one (`category: None`) or one of the catalogue.
#[derive(Debug)]
struct CategoryForm {
    category: Option<Category>,
    name: String,
    group: CategoryGroup,
    description: text_editor::Content,
    model: bool,
    /// "Remove…" was pressed: the page asks where the mail goes.
    removing: bool,
    replacement: Option<Category>,
    /// "Save" was pressed once: mistakes show from now on.
    tried: bool,
}

/// A sorting rule as it is being typed: a new one (`id: None`) or one that is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleForm {
    id: Option<String>,
    category: Option<Category>,
    conditions: Vec<(SortField, String)>,
    exceptions: Vec<(SortField, String)>,
    enabled: bool,
    tried: bool,
}

/// What the editor's rows and pages report.
#[derive(Debug, Clone)]
pub enum Message {
    /// "New category".
    NewCategory,
    /// A category's row: open its page.
    EditCategory(Category),
    Name(String),
    Group(CategoryGroup),
    Description(text_editor::Action),
    Model(bool),
    /// "Show in the sidebar" on a category's page: stored at once, as every switch of the list.
    Shown(Category, bool),
    SaveCategory,
    /// "Reset": put back the suite's name and description – after the question.
    Reset,
    /// "Remove…": ask where the mail goes.
    Remove,
    Replacement(Category),
    /// "Remove category" after the replacement was chosen.
    ConfirmRemove,
    /// "Cancel" next to "Remove category".
    CancelRemove,
    /// "New sorting rule".
    NewRule,
    /// A rule's row: open its page.
    EditRule(String),
    RuleCategory(Category),
    /// The field of a condition (`exception: false`) or an exception.
    Field {
        exception: bool,
        at: usize,
        field: SortField,
    },
    Value {
        exception: bool,
        at: usize,
        value: String,
    },
    Add {
        exception: bool,
    },
    Drop {
        exception: bool,
        at: usize,
    },
    RuleEnabled(bool),
    SaveRule,
    /// "Delete rule" – after the question.
    DeleteRule,
    /// "Move up" (`up: true`) or "Move down" on a stored rule's page.
    Move {
        id: String,
        up: bool,
    },
    /// The answer to the question the page asks: go ahead (`true`) or not.
    Answer(bool),
    /// "‹ Back" from a page to the list, and Escape: asks first when something typed would go.
    Back,
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::NewCategory => {
            state.asking = None;
            state.form = Some(Form::Category(CategoryForm {
                category: None,
                name: String::new(),
                group: CategoryGroup::Info,
                description: text_editor::Content::new(),
                model: true,
                removing: false,
                replacement: None,
                tried: false,
            }));
            Action::None
        }
        Message::EditCategory(category) => {
            if let Some(info) = data.categories.iter().find(|info| info.category == category) {
                state.asking = None;
                state.form = Some(Form::Category(CategoryForm::of(info)));
            }
            Action::None
        }
        Message::NewRule => {
            state.asking = None;
            state.form = Some(Form::Rule(RuleForm {
                id: None,
                category: None,
                conditions: vec![(SortField::Sender, String::new())],
                exceptions: Vec::new(),
                enabled: true,
                tried: false,
            }));
            Action::None
        }
        Message::EditRule(id) => {
            if let Some(rule) = data.sort_rules.iter().find(|rule| rule.id == id) {
                state.asking = None;
                state.form = Some(Form::Rule(RuleForm::of(rule)));
            }
            Action::None
        }
        Message::Shown(category, shown) => Action::send(shown_request(data, category, shown)),
        Message::Move { id, up } => {
            let Some(at) = data.sort_rules.iter().position(|rule| rule.id == id) else { return Action::None };
            let position = match up {
                true if at > 0 => at - 1,
                false if at + 1 < data.sort_rules.len() => at + 1,
                _ => return Action::None,
            };
            Action::send(Request::MoveSortRule { id, position: position as u32 })
        }
        Message::Back => {
            // A question open: Escape and "‹ Back" take it back first.
            if state.asking.take().is_some() {
                return Action::None;
            }
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                return Action::None;
            }
            state.form = None;
            Action::None
        }
        Message::Reset => {
            state.asking = Some(Ask::Reset);
            Action::None
        }
        Message::DeleteRule => {
            state.asking = Some(Ask::Delete);
            Action::None
        }
        Message::Answer(go) => {
            let Some(asked) = state.asking.take() else { return Action::None };
            if !go {
                return Action::None;
            }
            let action = match (asked, &state.form) {
                (Ask::Discard, _) => Action::None,
                (Ask::Reset, Some(Form::Category(CategoryForm { category: Some(category), .. }))) => {
                    Action::send(Request::UpdateCategory {
                        category: *category,
                        group: None,
                        name: Some(String::new()),
                        description: Some(String::new()),
                        model: None,
                    })
                }
                (Ask::Delete, Some(Form::Rule(RuleForm { id: Some(id), .. }))) => {
                    Action::send(Request::RemoveSortRule { id: id.clone() })
                }
                _ => return Action::None,
            };
            state.form = None;
            action
        }
        message => {
            let (action, closes) = match &mut state.form {
                Some(Form::Category(form)) => category_update(form, message, data),
                Some(Form::Rule(form)) => rule_update(form, message),
                None => (Action::None, false),
            };
            if closes {
                state.form = None;
            }
            action
        }
    }
}

/// What Escape means while the editor is open: the same as "‹ Back". `None` while the list
/// shows – then the settings close as before.
pub fn escape(state: &State) -> Option<Message> {
    state.is_open().then_some(Message::Back)
}

/// `settings.set_setting` for the sidebar to show `category` or not; only the hidden ones are
/// kept, so a category added later starts shown.
pub fn shown_request(data: &Data, category: Category, shown: bool) -> Request {
    let mut hidden = data.settings.hidden_categories.clone();
    hidden.retain(|held| *held != category);
    if !shown {
        hidden.push(category);
    }
    let value = serde_json::to_value(&hidden).unwrap_or_default();
    Request::SetSetting { key: SettingKey::HiddenCategories, value }
}

/// A message of a category's page: the action, and whether the page closes.
fn category_update<A>(form: &mut CategoryForm, message: Message, data: &Data) -> (Action<A>, bool) {
    match message {
        Message::Name(name) => form.name = name,
        Message::Group(group) => form.group = group,
        Message::Description(action) => form.description.perform(action),
        Message::Model(on) => form.model = on,
        Message::Remove => form.removing = true,
        Message::CancelRemove => {
            form.removing = false;
            form.replacement = None;
        }
        Message::Replacement(category) => form.replacement = Some(category),
        Message::ConfirmRemove => {
            let (Some(category), Some(replacement)) = (form.category, form.replacement) else {
                return (Action::None, false);
            };
            let remove = Request::RemoveCategory { category, replacement: Some(replacement) };
            return (Action::send(remove), true);
        }
        Message::SaveCategory => {
            form.tried = true;
            if !form.mistakes(data).is_empty() {
                // The cursor into the name, where most mistakes are; the others stand at their field.
                let into_name = form.name_mistake(data).is_some();
                return (if into_name { Action::Focus(NAME_FIELD) } else { Action::None }, false);
            }
            return match form.request(data) {
                // A new category is made once; its page closes, so a second "Save" makes no twin.
                Some(request @ Request::CreateCategory { .. }) => (Action::send(request), true),
                Some(request) => (Action::send(request), false),
                None => (Action::None, false),
            };
        }
        _ => {}
    }
    (Action::None, false)
}

/// A message of a rule's page: the action, and whether the page closes.
fn rule_update<A>(form: &mut RuleForm, message: Message) -> (Action<A>, bool) {
    match message {
        Message::RuleCategory(category) => form.category = Some(category),
        Message::Field { exception, at, field } => {
            if let Some(held) = form.list(exception).get_mut(at) {
                held.0 = field;
            }
        }
        Message::Value { exception, at, value } => {
            if let Some(held) = form.list(exception).get_mut(at) {
                held.1 = value;
            }
        }
        Message::Add { exception } => {
            let list = form.list(exception);
            if list.len() < SORT_CONDITIONS_MAX {
                list.push((if exception { SortField::Subject } else { SortField::Sender }, String::new()));
            }
        }
        Message::Drop { exception, at } => {
            let list = form.list(exception);
            if at < list.len() {
                list.remove(at);
            }
        }
        Message::RuleEnabled(on) => form.enabled = on,
        Message::SaveRule => {
            form.tried = true;
            if !form.mistakes().is_empty() {
                let into_condition = form.condition_mistake().is_some();
                return (if into_condition { Action::Focus(CONDITION_FIELD) } else { Action::None }, false);
            }
            let Some(request) = form.request() else { return (Action::None, false) };
            // A new rule is made once; an existing one stays open with its new words.
            let closes = form.id.is_none();
            return (Action::send(request), closes);
        }
        _ => {}
    }
    (Action::None, false)
}

impl Form {
    /// Something typed on the page is not saved yet.
    fn dirty(&self, data: &Data) -> bool {
        match self {
            Form::Category(form) => match form.category {
                None => !form.name.trim().is_empty() || !form.description.text().trim().is_empty(),
                Some(_) => form.request(data).is_some(),
            },
            Form::Rule(form) => match &form.id {
                None => {
                    form.category.is_some()
                        || form.conditions.iter().chain(&form.exceptions).any(|(_, value)| !value.trim().is_empty())
                }
                Some(id) => {
                    data.sort_rules.iter().find(|rule| &rule.id == id).is_some_and(|stored| !form.same_as(stored))
                }
            },
        }
    }
}

impl CategoryForm {
    fn of(info: &CategoryInfo) -> CategoryForm {
        CategoryForm {
            category: Some(info.category),
            name: info.label.clone(),
            group: info.group,
            description: text_editor::Content::with_text(&info.description),
            model: info.model,
            removing: false,
            replacement: None,
            tried: false,
        }
    }

    /// The other categories of the catalogue (a new one has none to be told from).
    fn others<'a>(&self, data: &'a Data) -> impl Iterator<Item = &'a CategoryInfo> {
        let me = self.category;
        data.categories.iter().filter(move |info| Some(info.category) != me)
    }

    /// The category of the suite this page shows, as the catalogue holds it; `None` for one of the
    /// person's own and a new one.
    fn built_in<'a>(&self, data: &'a Data) -> Option<&'a CategoryInfo> {
        data.categories.iter().find(|info| Some(info.category) == self.category && info.built_in)
    }

    /// How many categories of the form's group the AI would choose between with this page saved;
    /// "Other" always counts, it is what "nothing fits" means.
    fn for_model(&self, data: &Data) -> usize {
        let mine = usize::from(self.model || self.category == Some(Category::Other));
        mine + self
            .others(data)
            .filter(|info| info.group == self.group && (info.model || info.category == Category::Other))
            .count()
    }

    /// What is wrong with the name, if anything.
    fn name_mistake(&self, data: &Data) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some(tr("Give the category a name.").to_string());
        }
        if name.chars().count() > NAME_MAX {
            let most = NAME_MAX.to_string();
            return Some(trf("A category's name has at most {most} characters.", &[("most", &most)]));
        }
        self.others(data)
            .any(|info| info.label.to_lowercase() == name.to_lowercase())
            .then(|| trf("There is a category called “{name}” already.", &[("name", &name)]))
    }

    /// What is wrong with the description, if anything.
    fn description_mistake(&self, data: &Data) -> Option<String> {
        let description = self.description.text();
        if description.trim().is_empty() && self.built_in(data).is_none() {
            return Some(tr("Describe what belongs in the category.").to_string());
        }
        (description.trim().chars().count() > DESCRIPTION_MAX).then(|| {
            let most = DESCRIPTION_MAX.to_string();
            trf("A description has at most {most} characters.", &[("most", &most)])
        })
    }

    /// What is wrong with "Let the AI choose it", if anything: one more than its group holds.
    fn model_mistake(&self, data: &Data) -> Option<String> {
        (self.model && self.for_model(data) > MODEL_PER_GROUP_MAX).then(|| {
            trf(
                "{group} would have {n} categories for the AI; at most {most} fit.",
                &[
                    ("group", &torro_core::category::group_label(self.group)),
                    ("n", &self.for_model(data).to_string()),
                    ("most", &MODEL_PER_GROUP_MAX.to_string()),
                ],
            )
        })
    }

    /// Every mistake, in the order of the page.
    fn mistakes(&self, data: &Data) -> Vec<String> {
        [self.name_mistake(data), self.description_mistake(data), self.model_mistake(data)]
            .into_iter()
            .flatten()
            .collect()
    }

    /// The request that saves the page: a new category, or the fields that changed; `None` when
    /// nothing did.
    fn request(&self, data: &Data) -> Option<Request> {
        let name = self.name.trim().to_string();
        let description = self.description.text().replace("\r\n", "\n").trim().to_string();
        let Some(category) = self.category else {
            return Some(Request::CreateCategory { group: self.group, name, description, model: self.model });
        };
        let stored = data.categories.iter().find(|info| info.category == category)?;
        let group = (stored.group != self.group).then_some(self.group);
        let model = (stored.model != self.model).then_some(self.model);
        let name = (stored.label != name).then_some(name);
        let description = (stored.description.trim() != description).then_some(description);
        if group.is_none() && model.is_none() && name.is_none() && description.is_none() {
            return None;
        }
        Some(Request::UpdateCategory { category, group, name, description, model })
    }
}

impl RuleForm {
    fn of(rule: &SortRule) -> RuleForm {
        let pairs = |list: &[SortCondition]| list.iter().map(|held| (held.field, held.value.clone())).collect();
        RuleForm {
            id: Some(rule.id.clone()),
            category: Some(rule.category),
            conditions: pairs(&rule.conditions),
            exceptions: pairs(&rule.exceptions),
            enabled: rule.enabled,
            tried: false,
        }
    }

    /// The page holds what `stored` holds: nothing to save.
    fn same_as(&self, stored: &SortRule) -> bool {
        RuleForm { tried: false, ..self.clone() } == RuleForm::of(stored)
    }

    fn list(&mut self, exception: bool) -> &mut Vec<(SortField, String)> {
        if exception { &mut self.exceptions } else { &mut self.conditions }
    }

    /// What is wrong with "File as", if anything.
    fn category_mistake(&self) -> Option<String> {
        self.category.is_none().then(|| tr("Choose the category the mail is filed as.").to_string())
    }

    /// What is wrong with the conditions, if anything.
    fn condition_mistake(&self) -> Option<String> {
        if self.conditions.iter().all(|(_, value)| value.trim().is_empty()) {
            return Some(tr("A sorting rule needs at least one condition.").to_string());
        }
        too_long(&self.conditions)
    }

    /// What is wrong with the exceptions, if anything.
    fn exception_mistake(&self) -> Option<String> {
        too_long(&self.exceptions)
    }

    /// Every mistake, in the order of the page.
    fn mistakes(&self) -> Vec<String> {
        [self.category_mistake(), self.condition_mistake(), self.exception_mistake()].into_iter().flatten().collect()
    }

    /// The request that saves the page; empty lines are left out.
    fn request(&self) -> Option<Request> {
        let category = self.category?;
        let kept = |list: &[(SortField, String)]| {
            list.iter()
                .filter(|(_, value)| !value.trim().is_empty())
                .map(|(field, value)| SortCondition { field: *field, value: value.trim().to_string() })
                .collect::<Vec<_>>()
        };
        let (conditions, exceptions) = (kept(&self.conditions), kept(&self.exceptions));
        Some(match &self.id {
            None => Request::CreateSortRule { category, conditions, exceptions, position: None, enabled: self.enabled },
            Some(id) => Request::UpdateSortRule {
                id: id.clone(),
                category,
                conditions,
                exceptions: Some(exceptions),
                enabled: Some(self.enabled),
            },
        })
    }
}

/// A value of `list` longer than a condition may be, as the mistake to show.
fn too_long(list: &[(SortField, String)]) -> Option<String> {
    list.iter().any(|(_, value)| value.trim().chars().count() > SORT_VALUE_MAX).then(|| {
        let most = SORT_VALUE_MAX.to_string();
        trf("A condition's value has at most {most} characters.", &[("most", &most)])
    })
}

/// The open page: its name for the path, its body and "Save" for the header bar; `None` while the
/// list shows.
pub fn page<'a>(state: &'a State, data: &'a Data) -> Option<(String, Element<'a, Message>, Element<'a, Message>)> {
    let (name, body, save) = match state.form.as_ref()? {
        Form::Category(form) => {
            let name = match form.category {
                None => tr("New category").to_string(),
                Some(category) => crate::category_name_in(&data.categories, category),
            };
            let ready = form.category.is_none() || form.request(data).is_some();
            (name, category_page(form, data), save_button(ready, Message::SaveCategory))
        }
        Form::Rule(form) => {
            let name = match (&form.id, form.category) {
                (Some(_), Some(category)) => {
                    trf("Rule for {category}", &[("category", &crate::category_name_in(&data.categories, category))])
                }
                _ => tr("New sorting rule").to_string(),
            };
            let ready = match &form.id {
                None => true,
                Some(id) => {
                    data.sort_rules.iter().find(|rule| &rule.id == id).is_none_or(|stored| !form.same_as(stored))
                }
            };
            (name, rule_page(form, data), save_button(ready, Message::SaveRule))
        }
    };
    // The question stands above the page, where it is seen.
    let body = match state.asking {
        Some(asked) => column![question(asked), body].spacing(layout::section_gap()).into(),
        None => body,
    };
    Some((name, body, save))
}

/// "Save" for the header bar: primary while there is something to save.
fn save_button<'a>(ready: bool, message: Message) -> Element<'a, Message> {
    let look: widgets::ButtonStyle = if ready { style::button::primary } else { style::button::secondary };
    widgets::text_button(None, tr("Save"), look, ready.then_some(message))
}

/// The question the page asks, with its two answers.
fn question<'a>(asked: Ask) -> Element<'a, Message> {
    let (title, detail, yes) = match asked {
        Ask::Discard => (tr("Discard changes?"), tr("What you typed on this page is not saved."), tr("Discard")),
        Ask::Reset => (
            tr("Use the suite's name and description?"),
            tr("Your name and description of this category go."),
            tr("Reset"),
        ),
        Ask::Delete => (tr("Delete this rule?"), tr("Mail it filed keeps its category."), tr("Delete rule")),
    };
    let buttons = row![
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Answer(false))),
        widgets::text_button(None, yes, style::button::danger, Some(Message::Answer(true))),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::group(title, vec![setting_row(None, detail, None, buttons)], None)
}

/// "Sorting rules": the rules in their order, each opening its page; "New sorting rule" last.
pub fn rules_group(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = data
        .sort_rules
        .iter()
        .map(|rule| {
            // What the rule files as first: that is what one looks for in the list.
            let category = crate::category_name_in(&data.categories, rule.category);
            let mut detail = conditions_line(&rule.conditions);
            if !rule.exceptions.is_empty() {
                detail = trf(
                    "{conditions}, except {exceptions}",
                    &[("conditions", &detail), ("exceptions", &conditions_line(&rule.exceptions))],
                );
            }
            if !rule.enabled {
                detail = trf("Off · {rule}", &[("rule", &detail)]);
            }
            let row = setting_row(None, category, Some(detail), chevron());
            pressable(row, Some(Message::EditRule(rule.id.clone())))
        })
        .collect();
    rows.push(add_row(tr("New sorting rule"), Message::NewRule));
    widgets::group(
        tr("Sorting rules"),
        rows,
        Some(
            tr("The first rule that fits files new mail, before the headers and the AI. What you filed by hand stays as it is.")
                .to_string(),
        ),
    )
}

/// "Categories": "New category" with what the list is for, then one group per group of
/// categories, each row opening its page. A row says more only where a category is unusual: not
/// for the AI, or hidden from the sidebar.
pub fn categories_groups(data: &Data) -> Vec<Element<'_, Message>> {
    let hidden = &data.settings.hidden_categories;
    let mut groups = vec![widgets::group(
        tr("Categories"),
        vec![add_row(tr("New category"), Message::NewCategory)],
        Some(
            tr("Rename any category, move it to another group, or make your own. The description is what the AI reads.")
                .to_string(),
        ),
    )];
    for group in CategoryGroup::ALL {
        let rows: Vec<Element<'_, Message>> = data
            .categories
            .iter()
            .filter(|info| info.group == group)
            .map(|info| {
                let notes: Vec<&str> = [
                    (!info.model).then(|| tr("Only by rules and by hand")),
                    hidden.contains(&info.category).then(|| tr("Hidden from the sidebar")),
                ]
                .into_iter()
                .flatten()
                .collect();
                let detail = (!notes.is_empty()).then(|| notes.join(" · "));
                let row = setting_row(None, info.label.clone(), detail, chevron());
                pressable(row, Some(Message::EditCategory(info.category)))
            })
            .collect();
        if !rows.is_empty() {
            groups.push(widgets::group(torro_core::category::group_label(group), rows, None));
        }
    }
    groups
}

/// The last row of a list: "+ New …".
fn add_row<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    let plus = torro_gui::icon::icon(Icon::Plus, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    pressable(setting_row(Some(plus.into()), label, None, blank()), Some(message))
}

/// The conditions of a rule in a line: "from billing@shop.example · “invoice” in the subject".
fn conditions_line(conditions: &[SortCondition]) -> String {
    conditions.iter().map(|held| condition_words(held.field, &held.value)).collect::<Vec<_>>().join(" · ")
}

/// One condition in words.
fn condition_words(field: SortField, value: &str) -> String {
    match field {
        SortField::Sender => trf("from {sender}", &[("sender", &value)]),
        SortField::Domain => trf("from anyone at {domain}", &[("domain", &value)]),
        SortField::Subject => trf("“{text}” in the subject", &[("text", &value)]),
        SortField::Text => trf("“{text}” in the text", &[("text", &value)]),
    }
}

/// A group's note: the mistake in the colour of mistakes once "Save" was pressed, else `note`.
fn note_or_mistake<'a>(tried: bool, mistake: Option<String>, note: Option<String>) -> Option<Element<'a, Message>> {
    match (tried, mistake) {
        (true, Some(mistake)) => Some(
            typo::text(typo::meta(), mistake)
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
                .wrapping(Wrapping::WordOrGlyph)
                .into(),
        ),
        _ => note.map(|note| typo::text(typo::meta(), note).style(torro_gui::theme::text_2).into()),
    }
}

/// A group whose note can be a mistake: `widgets::group` without its own note, the line under it.
fn checked_group<'a>(
    title: &'a str,
    rows: Vec<Element<'a, Message>>,
    note: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let group = widgets::group(title, rows, None);
    match note {
        Some(note) => column![group, container(note).padding([0.0, space::l()])].spacing(space::s()).into(),
        None => group,
    }
}

/// A category's page.
fn category_page<'a>(form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
    let name = field(tr("Name"), &form.name).id(NAME_FIELD).on_input(Message::Name).on_submit(Message::SaveCategory);
    let groups: Vec<Element<'a, Message>> = CategoryGroup::ALL
        .into_iter()
        .map(|group| choice(torro_core::category::group_label(group), form.group == group, Message::Group(group)))
        .collect();
    let description = text_editor(&form.description)
        .on_action(Message::Description)
        .placeholder(tr("What belongs here, with examples – and what does not."))
        .font(typo::body().font)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(description_height())
        .style(style::text_editor);
    let counted = trf(
        "{group}: {n} of {most} categories for the AI",
        &[
            ("group", &torro_core::category::group_label(form.group)),
            ("n", &form.for_model(data)),
            ("most", &MODEL_PER_GROUP_MAX),
        ],
    );
    let model = setting_row(None, tr("Let the AI choose it"), Some(counted), switch(form.model, Message::Model));
    let built_in = form.built_in(data);
    let description_note = match built_in {
        Some(_) => tr("Empty, it is the suite's description again. A hard exception is a sorting rule."),
        None => tr("The AI reads it to decide. A hard exception is a sorting rule."),
    };
    let mut page = column![
        checked_group(
            tr("Name"),
            vec![row_frame(name, false)],
            note_or_mistake(form.tried, form.name_mistake(data), None)
        ),
        widgets::group(tr("Group"), groups, Some(tr("The group gives the category its colour and icon.").to_string())),
        checked_group(
            tr("Description"),
            vec![container(description).width(Fill).padding([space::s(), space::l()]).into()],
            note_or_mistake(form.tried, form.description_mistake(data), Some(description_note.to_string())),
        ),
        checked_group(tr("AI"), vec![model], note_or_mistake(form.tried, form.model_mistake(data), None)),
    ]
    .spacing(layout::section_gap());
    // A stored category: the sidebar's switch, stored at once like every switch of a list.
    if let Some(category) = form.category {
        let shown = !data.settings.hidden_categories.contains(&category);
        let switch = switch(shown, move |on| Message::Shown(category, on));
        let sidebar = setting_row(None, tr("Show in the sidebar"), None, switch);
        page = page.push(widgets::group(tr("Sidebar"), vec![sidebar], None));
    }
    if built_in.is_some_and(|info| info.renamed || info.description != suite_description(info.category)) {
        let reset = widgets::text_button(None, tr("Reset"), style::button::secondary, Some(Message::Reset));
        page = page.push(widgets::group(
            tr("Reset"),
            vec![setting_row(None, tr("Use the suite's name and description"), None, reset)],
            None,
        ));
    }
    if form.category.is_some() && built_in.is_none() {
        page = page.push(removal(form, data));
    }
    page.into()
}

/// The suite's description of `category`, as the catalogue gives it while nobody changed it.
fn suite_description(category: Category) -> String {
    torro_core::category::suite_description(category).unwrap_or_default().to_string()
}

/// The room of the description: five lines and the padding of a field.
const DESCRIPTION_LINES: f32 = 5.0;

fn description_height() -> f32 {
    DESCRIPTION_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// A category to choose in a pick list: its name, with its group in front.
#[derive(Debug, Clone)]
struct CategoryChoice {
    category: Category,
    words: String,
}

impl PartialEq for CategoryChoice {
    fn eq(&self, other: &Self) -> bool {
        self.category == other.category
    }
}

impl std::fmt::Display for CategoryChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

/// The categories of the catalogue as choices, by group, "Group · Name".
fn category_choices(data: &Data, leave_out: Option<Category>) -> Vec<CategoryChoice> {
    CategoryGroup::ALL
        .into_iter()
        .flat_map(|group| data.categories.iter().filter(move |info| info.group == group))
        .filter(|info| Some(info.category) != leave_out)
        .map(|info| CategoryChoice {
            category: info.category, words: format!("{} · {}", info.group_label, info.label)
        })
        .collect()
}

/// A pick list of `choices` in the look of a row's choice ([`crate::parts::picker`]); while
/// nothing is chosen it says "Choose a category" – a placeholder, never a choice of its own.
fn category_picker<'a>(
    choices: Vec<CategoryChoice>,
    chosen: Option<Category>,
    on_pick: impl Fn(Category) -> Message + 'a,
) -> Element<'a, Message> {
    let chosen = chosen.and_then(|wanted| choices.iter().find(|held| held.category == wanted).cloned());
    iced::widget::pick_list(choices, chosen, move |choice: CategoryChoice| on_pick(choice.category))
        .placeholder(tr("Choose a category"))
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into()
}

/// "Remove…" for a category of the person's own, and once pressed the category its mail goes to,
/// "Cancel" and "Remove category" in one row.
fn removal<'a>(form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
    if !form.removing {
        let remove =
            widgets::text_button(Some(Icon::Trash2), tr("Remove…"), style::button::danger, Some(Message::Remove));
        return widgets::group(
            tr("Remove"),
            vec![setting_row(None, tr("Remove this category"), None, remove)],
            Some(tr("Its mail, entries and rules move to a category you choose.").to_string()),
        );
    }
    let pick = category_picker(category_choices(data, form.category), form.replacement, Message::Replacement);
    let buttons = row![
        pick,
        widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelRemove)),
        widgets::text_button(
            Some(Icon::Trash2),
            tr("Remove category"),
            style::button::danger,
            form.replacement.map(|_| Message::ConfirmRemove),
        ),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::group(
        tr("Remove"),
        vec![setting_row(None, tr("File its mail as"), None, buttons)],
        Some(tr("Who filed the mail stays as it was: what you filed by hand is still yours.").to_string()),
    )
}

/// A rule's page.
fn rule_page<'a>(form: &'a RuleForm, data: &'a Data) -> Element<'a, Message> {
    let file_as = category_picker(category_choices(data, None), form.category, Message::RuleCategory);
    let file_as = setting_row(None, tr("File as"), None, file_as);
    let enabled = setting_row(None, tr("Rule is on"), None, switch(form.enabled, Message::RuleEnabled));
    let mut page = column![
        // What the rule does first, then what it looks at.
        checked_group(tr("File as"), vec![file_as], note_or_mistake(form.tried, form.category_mistake(), None)),
        checked_group(
            tr("Conditions"),
            condition_rows(&form.conditions, false),
            note_or_mistake(
                form.tried,
                form.condition_mistake(),
                Some(tr("Every condition must fit. Upper and lower case do not count.").to_string()),
            ),
        ),
        checked_group(
            tr("Exceptions"),
            condition_rows(&form.exceptions, true),
            note_or_mistake(
                form.tried,
                form.exception_mistake(),
                Some(tr("If any of these fits, the rule leaves the mail alone.").to_string()),
            ),
        ),
        widgets::group(tr("Rule"), vec![enabled], None),
    ]
    .spacing(layout::section_gap());
    if let Some(id) = &form.id {
        if let Some(at) = data.sort_rules.iter().position(|rule| &rule.id == id) {
            let count = data.sort_rules.len();
            let up = (at > 0).then(|| Message::Move { id: id.clone(), up: true });
            let down = (at + 1 < count).then(|| Message::Move { id: id.clone(), up: false });
            let moves = row![
                widgets::text_button(Some(Icon::ChevronUp), tr("Move up"), style::button::secondary, up),
                widgets::text_button(Some(Icon::ChevronDown), tr("Move down"), style::button::secondary, down),
            ]
            .spacing(space::s());
            let place = trf("Place {n} of {count}", &[("n", &(at + 1)), ("count", &count)]);
            page = page.push(widgets::group(
                tr("Order"),
                vec![setting_row(None, place, None, moves)],
                Some(tr("The first rule that fits decides, so a narrow rule goes before a broad one.").to_string()),
            ));
        }
        let delete = widgets::text_button(
            Some(Icon::Trash2),
            tr("Delete rule"),
            style::button::danger,
            Some(Message::DeleteRule),
        );
        let row = setting_row(None, tr("Delete this rule"), None, delete);
        page = page.push(widgets::group(tr("Delete"), vec![row], None));
    }
    page.into()
}

/// One row per condition (or exception): what it looks at, the words, and ✕; "Add" last.
fn condition_rows(list: &[(SortField, String)], exception: bool) -> Vec<Element<'_, Message>> {
    let mut rows: Vec<Element<'_, Message>> = list
        .iter()
        .enumerate()
        .map(|(at, (field_kind, value))| {
            let kind = picker(FIELD_CHOICES, FieldChoice(*field_kind), move |choice: FieldChoice| Message::Field {
                exception,
                at,
                field: choice.0,
            });
            let mut words = field(placeholder(*field_kind), value)
                .on_input(move |value| Message::Value { exception, at, value })
                .on_submit(Message::SaveRule);
            if !exception && at == 0 {
                words = words.id(CONDITION_FIELD);
            }
            let drop = widgets::icon_button(Icon::X, tr("Remove"), Some(Message::Drop { exception, at }));
            row_frame(row![kind, words, drop].spacing(space::s()).align_y(Alignment::Center), false)
        })
        .collect();
    if list.len() < SORT_CONDITIONS_MAX {
        let label = if exception { tr("Add exception") } else { tr("Add condition") };
        rows.push(add_row(label, Message::Add { exception }));
    }
    rows
}

/// What to type for a condition on `field`.
fn placeholder(field: SortField) -> &'static str {
    match field {
        SortField::Sender => "billing@shop.example",
        SortField::Domain => "shop.example",
        SortField::Subject | SortField::Text => tr("Words"),
    }
}

/// The field of a condition as a choice of a pick list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FieldChoice(SortField);

const FIELD_CHOICES: [FieldChoice; 4] = [
    FieldChoice(SortField::Sender),
    FieldChoice(SortField::Domain),
    FieldChoice(SortField::Subject),
    FieldChoice(SortField::Text),
];

impl std::fmt::Display for FieldChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            SortField::Sender => tr("Sender is"),
            SortField::Domain => tr("Domain is"),
            SortField::Subject => tr("Subject contains"),
            SortField::Text => tr("Text contains"),
        })
    }
}

#[cfg(test)]
mod tests {
    use torro_ipc::inbox::CategoryInfo;
    use torro_mail::CategoryDef;

    use super::*;

    /// The suite's catalogue and one category of the person's own, "Tax" in "Paper trail".
    fn data() -> Data {
        let mut defs = CategoryDef::defaults();
        defs.push(CategoryDef {
            category: tax(),
            group: CategoryGroup::PaperTrail,
            name: Some("Tax".into()),
            description: Some("For the tax return.".into()),
            model: true,
        });
        Data {
            categories: defs.iter().map(|def| CategoryInfo::of(def, 0, 0)).collect(),
            sort_rules: vec![rule("r1", "one.example"), rule("r2", "two.example")],
            ..Data::default()
        }
    }

    fn tax() -> Category {
        Category::Custom(torro_mail::CustomCategory::new(1).expect("a number"))
    }

    fn rule(id: &str, domain: &str) -> SortRule {
        SortRule {
            id: id.into(),
            category: Category::Invoice,
            conditions: vec![SortCondition { field: SortField::Domain, value: domain.into() }],
            exceptions: Vec::new(),
            enabled: true,
            created: chrono::DateTime::from_timestamp(0, 0).expect("a time"),
        }
    }

    /// What the last of `messages` sends, after the messages before it on the same state.
    fn sent(state: &mut State, data: &Data, messages: Vec<Message>) -> Vec<Request> {
        let mut last = Vec::new();
        for message in messages {
            last = match update::<()>(state, message, data) {
                Action::Send(requests) => requests,
                _ => Vec::new(),
            };
        }
        last
    }

    fn typed(text: &str) -> Message {
        Message::Description(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text.into()))))
    }

    /// A new category is made with what was typed, and its page closes.
    #[test]
    fn saving_a_new_category_makes_it() {
        let (data, mut state) = (data(), State::default());
        let requests = sent(
            &mut state,
            &data,
            vec![
                Message::NewCategory,
                Message::Name("  Club  ".into()),
                Message::Group(CategoryGroup::Info),
                typed("The sports club."),
                Message::Model(false),
                Message::SaveCategory,
            ],
        );
        assert_eq!(
            requests,
            vec![Request::CreateCategory {
                group: CategoryGroup::Info,
                name: "Club".into(),
                description: "The sports club.".into(),
                model: false,
            }]
        );
        assert!(!state.is_open(), "made once, so a second Save makes no twin");
    }

    /// Mistakes show only after "Save", "Save" puts the cursor into the name, and nothing is sent
    /// while there are any: no name, a name taken, no description of one's own, more than 20 for
    /// the AI in a group.
    #[test]
    fn a_category_with_mistakes_is_not_sent() {
        let (data, mut state) = (data(), State::default());
        let _ = update::<()>(&mut state, Message::NewCategory, &data);
        assert!(matches!(update::<()>(&mut state, Message::SaveCategory, &data), Action::Focus(NAME_FIELD)));
        let Some(Form::Category(form)) = &state.form else { panic!("still open") };
        assert!(form.tried);
        assert_eq!(form.mistakes(&data).len(), 2, "a name and a description");
        let taken = sent(&mut state, &data, vec![Message::Name("tax".into()), typed("x"), Message::SaveCategory]);
        assert!(taken.is_empty(), "a name is taken whatever its case");
        // "Paper trail" holds 7 of the suite and "Tax" for the AI; twelve more fill it to 20.
        let mut full = data.clone();
        for n in 0..12 {
            let def = CategoryDef {
                category: Category::Custom(torro_mail::CustomCategory::new(n + 2).expect("a number")),
                group: CategoryGroup::PaperTrail,
                name: Some(format!("Topic {n}")),
                description: Some("Something.".into()),
                model: true,
            };
            full.categories.push(CategoryInfo::of(&def, 0, 0));
        }
        let mut state = State::default();
        let one_more = vec![
            Message::NewCategory,
            Message::Name("One more".into()),
            Message::Group(CategoryGroup::PaperTrail),
            typed("Something."),
            Message::SaveCategory,
        ];
        assert!(sent(&mut state, &full, one_more).is_empty());
        let Some(Form::Category(form)) = &state.form else { panic!("still open") };
        assert_eq!(form.for_model(&full), 21);
        assert!(form.model_mistake(&full).is_some(), "said at the switch");
        let off = sent(&mut state, &full, vec![Message::Model(false), Message::SaveCategory]);
        assert_eq!(off.len(), 1, "not for the AI it fits");
    }

    /// Changing a category sends only what changed; the page stays open. Leaving it with changes
    /// asks first; "Cancel" keeps the page, "Discard" leaves it.
    #[test]
    fn changing_a_category_sends_what_changed_and_leaving_asks() {
        let (data, mut state) = (data(), State::default());
        let requests = sent(
            &mut state,
            &data,
            vec![Message::EditCategory(Category::Invoice), Message::Name("Bills".into()), Message::SaveCategory],
        );
        assert_eq!(
            requests,
            vec![Request::UpdateCategory {
                category: Category::Invoice,
                group: None,
                name: Some("Bills".into()),
                description: None,
                model: None,
            }]
        );
        assert!(state.is_open());
        // The data has not caught up: the page still differs, so leaving asks.
        let _ = update::<()>(&mut state, Message::Back, &data);
        assert_eq!(state.asking, Some(Ask::Discard));
        let _ = update::<()>(&mut state, Message::Answer(false), &data);
        assert!(state.is_open() && state.asking.is_none(), "Cancel keeps the page");
        assert!(escape(&state).is_some(), "Escape asks too");
        let _ = update::<()>(&mut state, Message::Back, &data);
        let _ = update::<()>(&mut state, Message::Answer(true), &data);
        assert!(!state.is_open(), "Discard leaves");
        // Nothing changed: leaving does not ask.
        let _ = update::<()>(&mut state, Message::EditCategory(Category::Invoice), &data);
        let _ = update::<()>(&mut state, Message::Back, &data);
        assert!(!state.is_open());
        assert!(escape(&state).is_none(), "with the list, Escape is the settings' own");
    }

    /// "Reset" and "Delete rule" ask first and send nothing before the answer.
    #[test]
    fn resetting_and_deleting_ask_first() {
        let (data, mut state) = (data(), State::default());
        assert!(sent(&mut state, &data, vec![Message::EditCategory(Category::Invoice), Message::Reset]).is_empty());
        assert_eq!(state.asking, Some(Ask::Reset));
        let reset = sent(&mut state, &data, vec![Message::Answer(true)]);
        assert!(matches!(reset.as_slice(), [Request::UpdateCategory { name: Some(name), .. }] if name.is_empty()));
        assert!(sent(&mut state, &data, vec![Message::EditRule("r2".into()), Message::DeleteRule]).is_empty());
        assert!(sent(&mut state, &data, vec![Message::Answer(false)]).is_empty(), "Cancel deletes nothing");
        let deleted = sent(&mut state, &data, vec![Message::DeleteRule, Message::Answer(true)]);
        assert_eq!(deleted, vec![Request::RemoveSortRule { id: "r2".into() }]);
        assert!(!state.is_open());
    }

    /// Removing asks where the mail goes and sends nothing before; "Cancel" takes it back.
    #[test]
    fn removing_a_category_asks_for_its_replacement() {
        let (data, mut state) = (data(), State::default());
        let early =
            sent(&mut state, &data, vec![Message::EditCategory(tax()), Message::Remove, Message::ConfirmRemove]);
        assert!(early.is_empty(), "no replacement chosen yet");
        let _ = sent(&mut state, &data, vec![Message::CancelRemove]);
        let Some(Form::Category(form)) = &state.form else { panic!("open") };
        assert!(!form.removing);
        let requests = sent(
            &mut state,
            &data,
            vec![Message::Remove, Message::Replacement(Category::Document), Message::ConfirmRemove],
        );
        assert_eq!(requests, vec![Request::RemoveCategory { category: tax(), replacement: Some(Category::Document) }]);
        assert!(!state.is_open());
    }

    /// A rule is made with its conditions and exceptions, empty lines left out; with mistakes the
    /// cursor goes into the first condition.
    #[test]
    fn saving_a_rule_sends_its_conditions() {
        let (data, mut state) = (data(), State::default());
        let requests = sent(
            &mut state,
            &data,
            vec![
                Message::NewRule,
                Message::Field { exception: false, at: 0, field: SortField::Domain },
                Message::Value { exception: false, at: 0, value: " rail.example ".into() },
                Message::Add { exception: false },
                Message::Add { exception: true },
                Message::Value { exception: true, at: 0, value: "delay".into() },
                Message::RuleCategory(Category::Booking),
                Message::SaveRule,
            ],
        );
        assert_eq!(
            requests,
            vec![Request::CreateSortRule {
                category: Category::Booking,
                conditions: vec![SortCondition { field: SortField::Domain, value: "rail.example".into() }],
                exceptions: vec![SortCondition { field: SortField::Subject, value: "delay".into() }],
                position: None,
                enabled: true,
            }]
        );
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::NewRule, &data);
        assert!(matches!(update::<()>(&mut state, Message::SaveRule, &data), Action::Focus(CONDITION_FIELD)));
    }

    /// A stored rule: "Move up"/"Move down" by one place, stopping at the ends; the switch on its
    /// page is saved with it.
    #[test]
    fn a_rule_moves_and_switches_on_its_page() {
        let (data, mut state) = (data(), State::default());
        let down = sent(&mut state, &data, vec![Message::Move { id: "r1".into(), up: false }]);
        assert_eq!(down, vec![Request::MoveSortRule { id: "r1".into(), position: 1 }]);
        assert!(sent(&mut state, &data, vec![Message::Move { id: "r1".into(), up: true }]).is_empty(), "first already");
        let off = sent(
            &mut state,
            &data,
            vec![Message::EditRule("r2".into()), Message::RuleEnabled(false), Message::SaveRule],
        );
        assert_eq!(
            off,
            vec![Request::UpdateSortRule {
                id: "r2".into(),
                category: Category::Invoice,
                conditions: data.sort_rules[1].conditions.clone(),
                exceptions: Some(Vec::new()),
                enabled: Some(false),
            }]
        );
    }

    /// The sidebar's switch on a category's page stores at once.
    #[test]
    fn the_sidebar_switch_stores_at_once() {
        let (data, mut state) = (data(), State::default());
        let hidden = sent(&mut state, &data, vec![Message::EditCategory(tax()), Message::Shown(tax(), false)]);
        assert_eq!(
            hidden,
            vec![Request::SetSetting { key: SettingKey::HiddenCategories, value: serde_json::json!(["custom_1"]) }]
        );
    }

    /// Every page and the lists draw.
    #[test]
    fn the_pages_draw() {
        let (data, mut state) = (data(), State::default());
        let _ = categories_groups(&data);
        let _ = rules_group(&data);
        for open in [Message::NewCategory, Message::EditCategory(Category::Invoice), Message::EditCategory(tax())] {
            let _ = update::<()>(&mut state, open, &data);
            let _ = update::<()>(&mut state, Message::Remove, &data);
            let _ = update::<()>(&mut state, Message::Reset, &data);
            assert!(page(&state, &data).is_some());
            state.asking = None;
        }
        let _ = update::<()>(&mut state, Message::EditRule("r1".into()), &data);
        let Some((name, ..)) = page(&state, &data) else { panic!("a page") };
        let invoice = crate::category_name_in(&data.categories, Category::Invoice);
        assert_eq!(name, trf("Rule for {category}", &[("category", &invoice)]));
    }
}
