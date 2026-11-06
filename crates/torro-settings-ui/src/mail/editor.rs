//! The editor of the mail categories and the sorting rules (task E15.5, issue #255,
//! `docs/kategorien.md`, "Eigene Kategorien und Sortierregeln"), part of Settings ›
//! Categorization in the mail window.
//!
//! The list stands in [`super::categorization`] (tab Categories). This module holds the pages it
//! opens and the form of a group.
//!
//! A category's page (issue #481), compact so four rules fit without scrolling:
//!
//! ```text
//! [▣ ▾] [Invoice                         ] [● Paper trail ▾]
//! Description for the AI  [ Text …                         ]
//!   ✦ If the AI is on, it reads the description … Fixed rules always come first.
//! Fixed rules · go before the AI                         [+ Rule ▾]
//!   Domain is telekom.de                                  (●)  ›
//!   [Search] from:shop.example has:attachment             (●)  ›
//! ┌ Show in the sidebar (●) ┐ ┌ AI may not file mail here ( ) ┐   (narrow: one below the other)
//! Use the suite's name, description and icon             Remove…
//! ```
//!
//! The symbol and the group open as popovers ([`overlay`]). The fixed rules are the sorting rules
//! and the mail filters that set this category, and for Personal the built-in rule "From someone in
//! your address books"; "+ Rule" offers a rule or a filter from a search. A filter's dialog belongs
//! to [`super::filters`]: its messages go out as [`Message::Filter`]. A rule's page: "File as", the
//! conditions and the exceptions (field and words each), on/off and "Delete rule"; it goes back to
//! the category's page it came from. "Save" stands in the header bar.
//!
//! Everything is a request (`inbox.create_category`, `inbox.update_category`,
//! `inbox.remove_category`, `inbox.create_sort_rule`, `inbox.update_sort_rule`,
//! `inbox.remove_sort_rule`, `settings.set_setting`); the page checks only
//! what a person must fix before saving – the core checks all of it again. Mistakes show after
//! the first "Save", under the field they are about, and "Save" puts the cursor into the first
//! one it can (`.claude/skills/torro-design`, forms). What would lose something – deleting a rule,
//! resetting a category, leaving a page with changes – asks on the page first.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, responsive, row, space as blank, text_editor};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{palette, size, space};
use torro_gui::widgets::{Align, Anchor, MenuItem, MenuSection, Placement};
use torro_gui::{layout, style, typo, widgets};
use torro_inbox::item::{SORT_CONDITIONS_MAX, SORT_VALUE_MAX};
use torro_inbox::{SortComparison, SortCondition, SortField, SortRule};
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::{Request, SettingKey};
use torro_mail::category::{DESCRIPTION_MAX, MODEL_PER_GROUP_MAX, NAME_MAX};
use torro_mail::{Category, CategoryGroup, CategoryIcon};

use super::filters;
use crate::parts::{field, picker, pressable, row_frame, setting_row, switch};
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
    group_form: Option<GroupForm>,
    /// The category whose page opened the rule's page that is open: its page comes back once the
    /// rule's page closes (issue #481).
    back_to: Option<Category>,
    /// The popover open on a category's page.
    popover: Option<Popover>,
    /// What "Discard" goes on with: the rule a person opened from a category's page with changes.
    then: Option<Box<Message>>,
    icon_anchor: Anchor,
    group_anchor: Anchor,
    rule_anchor: Anchor,
}

/// The popovers of a category's page (issue #481).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    /// The symbols, as a grid.
    Icon,
    /// The groups.
    Group,
    /// "+ Rule": a rule, or a filter from a search.
    Rule,
}

impl State {
    /// A page of the editor is open: the section shows it instead of its list.
    pub fn is_open(&self) -> bool {
        self.form.is_some() || self.group_form.is_some()
    }
}

#[derive(Debug)]
struct GroupForm {
    group: Option<CategoryGroup>,
    name: String,
    icon: CategoryIcon,
    color: u8,
    tried: bool,
    sending: bool,
    refused: Option<String>,
}

/// What the page asks before it does something that loses what was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Leave the page and drop what was typed.
    Discard,
    /// Put back the suite's name, description and icon of a category.
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
    icon: CategoryIcon,
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
    conditions: Vec<SortCondition>,
    exceptions: Vec<SortCondition>,
    enabled: bool,
    tried: bool,
    /// The category a new rule started with on a category's page (issue #481): no change yet.
    preset: Option<Category>,
}

/// What the editor's rows and pages report.
#[derive(Debug, Clone)]
pub enum Message {
    NewGroup,
    EditGroup(CategoryGroup),
    GroupName(String),
    GroupIcon(CategoryIcon),
    GroupColor(u8),
    SaveGroup,
    CancelGroup,
    NewCategoryInGroup(CategoryGroup),
    /// "New category".
    NewCategory,
    /// A category's row: open its page.
    EditCategory(Category),
    Name(String),
    Group(CategoryGroup),
    Icon(CategoryIcon),
    Description(text_editor::Action),
    Model(bool),
    /// "Show in the sidebar" on a category's page: stored at once, as every switch of the list.
    Shown(Category, bool),
    /// A popover of a category's page: open it (`Some`) or close it.
    Popover(Option<Popover>),
    /// "Rule…" of "+ Rule" on a category's page: a new rule that files mail there.
    NewRuleIn(Category),
    /// The switch of a rule's row on a category's page: stored at once.
    RuleSwitched(String, bool),
    /// "From someone in your address books" on Personal's page (issue #390): stored at once.
    ContactsPersonal(bool),
    /// "Move up" (`up: true`) or "Move down" on a category's page: one place in its group, sent at
    /// once.
    Shift {
        category: Category,
        up: bool,
    },
    /// For the filter dialog ([`super::filters`]): a filter's row, its switch, "Filter from a
    /// search…". [`super::categorization`] hands it on.
    Filter(filters::Message),
    SaveCategory,
    /// "Reset": put back the suite's name, description and icon – after the question.
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
    Comparison {
        exception: bool,
        at: usize,
        comparison: SortComparison,
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
    /// The answer to the question the page asks: go ahead (`true`) or not.
    Answer(bool),
    /// "‹ Back" from a page to the list, and Escape: asks first when something typed would go.
    Back,
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::NewGroup => {
            state.form = None;
            state.group_form = Some(GroupForm {
                group: None,
                name: String::new(),
                icon: CategoryIcon::Tag,
                color: 0,
                tried: false,
                sending: false,
                refused: None,
            });
            Action::None
        }
        Message::EditGroup(group) => {
            state.form = None;
            state.group_form = data.category_groups.iter().find(|held| held.group == group).map(|held| GroupForm {
                group: Some(group),
                name: held.label.clone(),
                icon: held.icon,
                color: held.color,
                tried: false,
                sending: false,
                refused: None,
            });
            Action::None
        }
        Message::GroupName(name) => {
            if let Some(form) = &mut state.group_form {
                form.name = name;
                form.refused = None;
            }
            Action::None
        }
        Message::GroupIcon(icon) => {
            if let Some(form) = &mut state.group_form {
                form.icon = icon;
                form.refused = None;
            }
            Action::None
        }
        Message::GroupColor(color) => {
            if let Some(form) = &mut state.group_form {
                form.color = color;
                form.refused = None;
            }
            Action::None
        }
        Message::SaveGroup => {
            let Some(form) = &mut state.group_form else { return Action::None };
            if form.sending {
                return Action::None;
            }
            form.tried = true;
            let name = form.name.trim();
            if name.is_empty()
                || name.chars().count() > NAME_MAX
                || name.chars().any(char::is_control)
                || form.color >= 5
            {
                return Action::None;
            }
            let request = match form.group {
                Some(group) => {
                    Request::UpdateCategoryGroup { group, name: name.to_string(), icon: form.icon, color: form.color }
                }
                None => Request::CreateCategoryGroup { name: name.to_string(), icon: form.icon, color: form.color },
            };
            form.sending = true;
            Action::send(request)
        }
        Message::CancelGroup => {
            state.group_form = None;
            Action::None
        }
        Message::NewCategoryInGroup(group) => {
            let action = update(state, Message::NewCategory, data);
            if let Some(Form::Category(form)) = &mut state.form {
                form.group = group;
            }
            action
        }
        Message::NewCategory => {
            state.group_form = None;
            state.asking = None;
            state.back_to = None;
            state.popover = None;
            state.form = Some(Form::Category(CategoryForm {
                category: None,
                name: String::new(),
                group: CategoryGroup::Info,
                icon: CategoryIcon::Tag,
                description: text_editor::Content::new(),
                model: true,
                removing: false,
                replacement: None,
                tried: false,
            }));
            Action::None
        }
        Message::EditCategory(category) => {
            state.group_form = None;
            state.back_to = None;
            state.popover = None;
            if let Some(info) = data.categories.iter().find(|info| info.category == category) {
                state.asking = None;
                state.form = Some(Form::Category(CategoryForm::of(info)));
            }
            Action::None
        }
        Message::NewRule => {
            state.group_form = None;
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                return Action::None;
            }
            state.asking = None;
            state.form = Some(Form::Rule(RuleForm {
                id: None,
                category: None,
                conditions: vec![SortCondition {
                    field: SortField::Sender,
                    comparison: SortComparison::Is,
                    value: String::new(),
                }],
                exceptions: Vec::new(),
                enabled: true,
                tried: false,
                preset: None,
            }));
            Action::None
        }
        Message::NewRuleIn(category) => {
            state.popover = None;
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                state.then = Some(Box::new(Message::NewRuleIn(category)));
                return Action::None;
            }
            let back = state.open_category();
            let action = update(state, Message::NewRule, data);
            if let Some(Form::Rule(form)) = &mut state.form {
                form.category = Some(category);
                form.preset = Some(category);
                state.back_to = back;
            }
            action
        }
        Message::EditRule(id) => {
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                state.then = Some(Box::new(Message::EditRule(id)));
                return Action::None;
            }
            let back = state.open_category();
            let action = edit_rule(state, id, data);
            if matches!(state.form, Some(Form::Rule(_))) && back.is_some() {
                state.back_to = back;
            }
            action
        }
        Message::Popover(popover) => {
            state.popover = popover;
            Action::None
        }
        Message::RuleSwitched(id, on) => match data.sort_rules.iter().find(|rule| rule.id == id) {
            Some(rule) => Action::send(rule_switched(rule, on)),
            None => Action::None,
        },
        Message::ContactsPersonal(on) => {
            Action::send(Request::SetSetting { key: SettingKey::MailContactsPersonal, value: serde_json::json!(on) })
        }
        Message::Shift { category, up } => match shifted(data, category, up) {
            Some(request) => Action::send(request),
            None => Action::None,
        },
        // Handed on by the categorization before it gets here.
        Message::Filter(_) => Action::None,
        Message::Shown(category, shown) => Action::send(shown_request(data, category, shown)),
        Message::Icon(icon) => {
            state.popover = None;
            if let Some(Form::Category(form)) = &mut state.form {
                form.icon = icon;
            }
            Action::None
        }
        Message::Group(group) => {
            state.popover = None;
            if let Some(Form::Category(form)) = &mut state.form {
                form.group = group;
            }
            Action::None
        }
        Message::Back => {
            state.popover = None;
            if state.group_form.take().is_some() {
                return Action::None;
            }
            // A question open: Escape and "‹ Back" take it back first.
            state.then = None;
            if state.asking.take().is_some() {
                return Action::None;
            }
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                return Action::None;
            }
            close(state, data);
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
            let then = state.then.take();
            let Some(asked) = state.asking.take() else { return Action::None };
            if !go {
                return Action::None;
            }
            // Discarded to open a rule from a category's page: the rule opens, and goes back there.
            if let (Ask::Discard, Some(then)) = (asked, then) {
                let back = state.open_category();
                state.form = None;
                let action = update(state, *then, data);
                if matches!(state.form, Some(Form::Rule(_))) {
                    state.back_to = back;
                }
                return action;
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
                        icon: Some(category.default_icon()),
                    })
                }
                (Ask::Delete, Some(Form::Rule(RuleForm { id: Some(id), .. }))) => {
                    Action::send(Request::RemoveSortRule { id: id.clone() })
                }
                _ => return Action::None,
            };
            close(state, data);
            action
        }
        message => {
            let (action, closes) = match &mut state.form {
                Some(Form::Category(form)) => category_update(form, message, data),
                Some(Form::Rule(form)) => rule_update(form, message),
                None => (Action::None, false),
            };
            if closes {
                close(state, data);
            }
            action
        }
    }
}

/// "‹ Back", a question answered or a page that closes itself: a rule's page opened from a
/// category's page goes back there (issue #481), every other page to the list.
fn close(state: &mut State, data: &Data) {
    state.popover = None;
    let back = match state.form {
        Some(Form::Rule(_)) => state.back_to.take(),
        _ => None,
    };
    state.back_to = None;
    state.form = back
        .and_then(|category| data.categories.iter().find(|info| info.category == category))
        .map(|info| Form::Category(CategoryForm::of(info)));
}

/// `inbox.update_sort_rule` that switches `rule` on or off and keeps everything else.
pub(super) fn rule_switched(rule: &SortRule, on: bool) -> Request {
    Request::UpdateSortRule {
        id: rule.id.clone(),
        category: rule.category,
        conditions: rule.conditions.clone(),
        exceptions: Some(rule.exceptions.clone()),
        enabled: Some(on),
    }
}

impl State {
    /// The stored category whose page is open, if one is.
    fn open_category(&self) -> Option<Category> {
        match &self.form {
            Some(Form::Category(form)) => form.category,
            _ => None,
        }
    }
}

/// Opens the stored rule `id`'s page (a second click on the open one closes it), asking first when
/// something typed on the open page would go.
fn edit_rule<A>(state: &mut State, id: String, data: &Data) -> Action<A> {
    if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
        state.asking = Some(Ask::Discard);
        return Action::None;
    }
    if matches!(state.form.as_ref(), Some(Form::Rule(form)) if form.id.as_deref() == Some(id.as_str())) {
        state.form = None;
        state.asking = None;
        return Action::None;
    }
    if let Some(rule) = data.sort_rules.iter().find(|rule| rule.id == id) {
        state.asking = None;
        state.form = Some(Form::Rule(RuleForm::of(rule)));
    }
    Action::None
}

/// A send came back while a main category is being saved (issue #397). The form closes once the
/// reloaded catalogue holds the main category as the form had it, so another section's send that
/// returns first does not close it; a refusal shows in the form, which takes a second try. A send
/// carries no name of its request, so a refusal of another section's send at the same moment would
/// show there too.
pub fn sent(state: &mut State, result: &Result<Data, String>) {
    let Some(form) = state.group_form.as_mut().filter(|form| form.sending) else { return };
    match result {
        Ok(data) => {
            let name = form.name.trim();
            let saved = data.category_groups.iter().any(|held| {
                let looks = held.icon == form.icon && held.color == form.color;
                match form.group {
                    Some(group) => held.group == group && looks && (name.is_empty() || held.label == name),
                    None => !held.built_in && looks && held.label == name,
                }
            });
            if saved {
                state.group_form = None;
            }
        }
        Err(message) => {
            form.sending = false;
            form.refused = Some(message.clone());
        }
    }
}

/// What Escape means while the editor is open: the same as "‹ Back". `None` while the list
/// shows – then the settings close as before.
pub fn escape(state: &State) -> Option<Message> {
    if state.popover.is_some() {
        return Some(Message::Popover(None));
    }
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
                held.field = field;
                held.comparison = field.default_comparison();
                if field == SortField::Attachment {
                    held.value.clear();
                }
            }
        }
        Message::Comparison { exception, at, comparison } => {
            if let Some(held) = form.list(exception).get_mut(at)
                && held.field.allows(comparison)
            {
                held.comparison = comparison;
            }
        }
        Message::Value { exception, at, value } => {
            if let Some(held) = form.list(exception).get_mut(at) {
                held.value = value;
            }
        }
        Message::Add { exception } => {
            let list = form.list(exception);
            if list.len() < SORT_CONDITIONS_MAX {
                let field = if exception { SortField::Subject } else { SortField::Sender };
                list.push(SortCondition { field, comparison: field.default_comparison(), value: String::new() });
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
                    form.category != form.preset
                        || form.conditions.iter().chain(&form.exceptions).any(|condition| {
                            !condition.value.trim().is_empty() || condition.field == SortField::Attachment
                        })
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
            icon: info.icon,
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

    /// What is wrong with the AI tile ("AI may not file mail here" off), if anything: one more than its group holds.
    fn model_mistake(&self, data: &Data) -> Option<String> {
        (self.model && self.for_model(data) > MODEL_PER_GROUP_MAX).then(|| {
            trf(
                "{group} would have {n} categories for the AI; at most {most} fit.",
                &[
                    ("group", &group_words(data, self.group)),
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
            return Some(Request::CreateCategory {
                group: self.group,
                name,
                description,
                model: self.model,
                icon: self.icon,
            });
        };
        let stored = data.categories.iter().find(|info| info.category == category)?;
        let group = (stored.group != self.group).then_some(self.group);
        let model = (stored.model != self.model).then_some(self.model);
        let name = (stored.label != name).then_some(name);
        let description = (stored.description.trim() != description).then_some(description);
        let icon = (stored.icon != self.icon).then_some(self.icon);
        if group.is_none() && model.is_none() && name.is_none() && description.is_none() && icon.is_none() {
            return None;
        }
        Some(Request::UpdateCategory { category, group, name, description, model, icon })
    }
}

impl RuleForm {
    fn of(rule: &SortRule) -> RuleForm {
        RuleForm {
            id: Some(rule.id.clone()),
            category: Some(rule.category),
            conditions: rule.conditions.clone(),
            exceptions: rule.exceptions.clone(),
            enabled: rule.enabled,
            tried: false,
            preset: None,
        }
    }

    /// The page holds what `stored` holds: nothing to save.
    fn same_as(&self, stored: &SortRule) -> bool {
        RuleForm { tried: false, ..self.clone() } == RuleForm::of(stored)
    }

    fn list(&mut self, exception: bool) -> &mut Vec<SortCondition> {
        if exception { &mut self.exceptions } else { &mut self.conditions }
    }

    /// What is wrong with "File as", if anything.
    fn category_mistake(&self) -> Option<String> {
        self.category.is_none().then(|| tr("Choose the category the mail is filed as.").to_string())
    }

    /// What is wrong with the conditions, if anything.
    fn condition_mistake(&self) -> Option<String> {
        if self
            .conditions
            .iter()
            .all(|condition| condition.value.trim().is_empty() && condition.field != SortField::Attachment)
        {
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
        let kept = |list: &[SortCondition]| {
            list.iter()
                .filter(|condition| condition.field == SortField::Attachment || !condition.value.trim().is_empty())
                .map(|condition| SortCondition {
                    field: condition.field,
                    comparison: condition.comparison,
                    value: condition.value.trim().to_string(),
                })
                .collect::<Vec<_>>()
        };
        let (conditions, exceptions) = (kept(&self.conditions), kept(&self.exceptions));
        Some(match &self.id {
            None => {
                Request::CreateSortRule { category, conditions, exceptions, position: Some(0), enabled: self.enabled }
            }
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
fn too_long(list: &[SortCondition]) -> Option<String> {
    if list.iter().any(|condition| {
        condition.field == SortField::Size
            && !condition.value.trim().is_empty()
            && condition.value.trim().parse::<f64>().ok().is_none_or(|mb| !mb.is_finite() || mb < 0.0)
    }) {
        return Some(tr("Enter the message size in megabytes.").into());
    }
    list.iter().any(|condition| condition.value.trim().chars().count() > SORT_VALUE_MAX).then(|| {
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
            (name, category_page(state, form, data), save_button(ready, Message::SaveCategory))
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
            tr("Use the suite's name, description and icon?"),
            tr("Your name, description and icon of this category go."),
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

/// The form of a group – a new one for `group: None`, else that group's – when it is open, for the
/// tab Categories (issue #454): name, icon, colour, "Cancel" and "Save".
pub(super) fn group_form_view(state: &State, group: Option<CategoryGroup>) -> Option<Element<'_, Message>> {
    state.group_form.as_ref().filter(|form| form.group == group)?;
    Some(group_editor(state))
}

fn group_editor<'a>(state: &'a State) -> Element<'a, Message> {
    let Some(form) = &state.group_form else { return blank().into() };
    let name = field(tr("Name"), &form.name).on_input(Message::GroupName).on_submit(Message::SaveGroup);
    // The five colours of the group palette (`Palette::category`), named as the standard design
    // draws them; chips, so the check stays at the name.
    let colors = widgets::chip_row(
        [tr("Blue"), tr("Amber"), tr("Teal"), tr("Violet"), tr("Rose")]
            .into_iter()
            .zip(0u8..)
            .map(|(name, at)| {
                widgets::category_choice_chip(name, usize::from(at), form.color == at, Some(Message::GroupColor(at)))
            })
            .collect(),
    );
    let mut body = column![
        typo::text(typo::caption(), tr("Main category")),
        name,
        typo::text(typo::meta(), tr("Icon")),
        icon_picker(form.icon, Message::GroupIcon, form.group.unwrap_or(CategoryGroup::Info)),
        typo::text(typo::meta(), tr("Colour")),
        colors,
    ]
    .spacing(space::s());
    if form.tried && form.name.trim().is_empty() {
        body = body.push(typo::text(typo::meta(), tr("Give the category a name.")));
    }
    if let Some(refused) = &form.refused {
        body = body.push(typo::text(typo::meta(), refused.clone()));
    }
    body = body.push(
        row![
            widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CancelGroup)),
            widgets::text_button(
                None,
                if form.sending { tr("Saving…") } else { tr("Save") },
                style::button::primary,
                (!form.sending).then_some(Message::SaveGroup)
            ),
        ]
        .spacing(space::s()),
    );
    container(body).width(Fill).padding([space::m(), space::l()]).into()
}

/// A row "+ New …" of a list, as in Filters and Attachments.
fn add_row<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    let plus = torro_gui::icon::icon(Icon::Plus, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    pressable(setting_row(Some(plus.into()), label, None, blank()), Some(message))
}

/// The conditions of a rule in one compact line, including each comparison.
pub(super) fn conditions_line(conditions: &[SortCondition]) -> String {
    conditions
        .iter()
        .map(|held| {
            let comparison =
                if held.comparison == SortComparison::Auto { held.field.default_comparison() } else { held.comparison };
            if held.field == SortField::Attachment {
                format!("{} {}", FieldChoice(held.field), ComparisonChoice(comparison))
            } else {
                format!("{} {} {}", FieldChoice(held.field), ComparisonChoice(comparison), held.value)
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
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
        // The sentence keeps the reading width in a wide page, as under `widgets::group` (issue #464).
        Some(note) => {
            column![group, container(widgets::reading_cell(note)).padding([0.0, space::l()])].spacing(space::s()).into()
        }
        None => group,
    }
}

/// A category's page (issue #481): the head in one line, the description with what the AI does
/// with it, the fixed rules, the two tiles, "Reset" and "Remove…" at the end.
fn category_page<'a>(state: &'a State, form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
    let color = group_color(data, form.group);
    let toggle = |popover| Message::Popover((state.popover != Some(popover)).then_some(popover));
    let symbol = widgets::anchored(
        &state.icon_anchor,
        widgets::boxed_button(category_icon(form.icon), tr("Icon"), Some(toggle(Popover::Icon))),
    );
    let name = field(tr("Name"), &form.name).id(NAME_FIELD).on_input(Message::Name).on_submit(Message::SaveCategory);
    let group = widgets::select_button_marked(
        &state.group_anchor,
        widgets::category_dot(color),
        group_words(data, form.group),
        toggle(Popover::Group),
    );
    // A short field is never stretched: the name at most a form wide, the group right beside it.
    let head = row![symbol, container(name).width(Fill).max_width(size::form_max()), group, blank().width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let mut head = column![head].spacing(space::s());
    if let Some(mistake) = note_or_mistake(form.tried, form.name_mistake(data), None) {
        head = head.push(mistake);
    }
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
    let hint = row![
        torro_gui::icon::icon(Icon::Sparkles, torro_gui::icon::SIZE_SM)
            .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).accent) }),
        typo::text(
            typo::meta(),
            tr(
                "If the AI is on, it reads the description and files matching mail here. Fixed rules always come first."
            ),
        )
        .style(torro_gui::theme::text_2)
        .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Start);
    let hint = widgets::reading_cell(hint);
    let mut about = column![typo::text(typo::caption(), tr("Description for the AI")).style(torro_gui::theme::text_3)]
        .spacing(space::s())
        .push(description);
    about = about.push(match note_or_mistake(form.tried, form.description_mistake(data), None) {
        Some(mistake) => mistake,
        None => hint,
    });
    let mut page = column![head, about].spacing(layout::section_gap());
    // A new category has no rules yet: they name it by its key, which it gets when it is saved.
    if let Some(category) = form.category {
        page = page.push(fixed_rules(state, data, category));
    }
    page = page.push(tiles(form, data));
    if let Some(mistake) = note_or_mistake(form.tried, form.model_mistake(data), None) {
        page = page.push(mistake);
    }
    let built_in = form.built_in(data);
    let changed = built_in.is_some_and(|info| {
        info.renamed
            || info.description != suite_description(info.category)
            || info.icon != info.category.default_icon()
    });
    if form.removing {
        page = page.push(removal(form, data));
    } else {
        let mut end = row![].spacing(space::s()).align_y(Alignment::Center);
        // The way without a mouse to the place a grip drags to (design review of #481): one up or
        // down in its group, which decides between the rules of several categories.
        if let Some(category) = form.category {
            let up = shifted(data, category, true).map(|_| Message::Shift { category, up: true });
            let down = shifted(data, category, false).map(|_| Message::Shift { category, up: false });
            end = end.push(widgets::text_button(Some(Icon::ChevronUp), tr("Move up"), style::button::ghost, up));
            end = end.push(widgets::text_button(Some(Icon::ChevronDown), tr("Move down"), style::button::ghost, down));
        }
        if changed {
            end = end.push(widgets::text_button(
                Some(Icon::History),
                tr("Use the suite's name, description and icon"),
                style::button::ghost,
                Some(Message::Reset),
            ));
        }
        end = end.push(blank().width(Fill));
        if form.category.is_some() && built_in.is_none() {
            end = end.push(widgets::text_button(
                Some(Icon::Trash2),
                tr("Remove…"),
                style::button::danger,
                Some(Message::Remove),
            ));
        }
        if changed || form.category.is_some() {
            page = page.push(end);
        }
    }
    page.into()
}

/// `inbox.move_category` one place up or down within the stored group of `category`; `None` at
/// the end it would leave.
fn shifted(data: &Data, category: Category, up: bool) -> Option<Request> {
    let group = data.categories.iter().find(|info| info.category == category)?.group;
    let members: Vec<Category> =
        data.categories.iter().filter(|info| info.group == group).map(|info| info.category).collect();
    let at = members.iter().position(|held| *held == category)?;
    let position = match up {
        true => at.checked_sub(1)?,
        false if at + 1 < members.len() => at + 1,
        false => return None,
    };
    Some(Request::MoveCategory { category, group, position: position as u32 })
}

/// The colour of `group` in the group palette, 0–4.
fn group_color(data: &Data, group: CategoryGroup) -> usize {
    data.category_groups.iter().find(|held| held.group == group).map_or(0, |held| usize::from(held.color.min(4)))
}

/// "Fixed rules · go before the AI" with "+ Rule": the sorting rules and the mail filters that file
/// mail as `category`, each with its switch and a chevron that opens it; for Personal also the
/// built-in "From someone in your address books" with its switch alone. No hint about rules of
/// other categories that might match too: the searches of filters cannot be compared reliably.
fn fixed_rules<'a>(state: &'a State, data: &'a Data, category: Category) -> Element<'a, Message> {
    let open = state.popover == Some(Popover::Rule);
    let add = widgets::anchored(
        &state.rule_anchor,
        widgets::text_button(
            Some(Icon::Plus),
            tr("Rule"),
            style::button::secondary,
            Some(Message::Popover((!open).then_some(Popover::Rule))),
        ),
    );
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for rule in data.sort_rules.iter().filter(|rule| rule.category == category) {
        let mut when = conditions_line(&rule.conditions);
        if !rule.exceptions.is_empty() {
            when = trf(
                "{conditions}, except {exceptions}",
                &[("conditions", &when), ("exceptions", &conditions_line(&rule.exceptions))],
            );
        }
        let id = rule.id.clone();
        let switch = switch(rule.enabled, move |on| Message::RuleSwitched(id.clone(), on));
        rows.push(fixed_line(None, when, rule.enabled, switch, Some(Message::EditRule(rule.id.clone()))));
    }
    for filter in data.mail_filters.iter().filter(|filter| filter.category == Some(category)) {
        let id = filter.id.clone();
        let switch = switch(filter.enabled, move |on| Message::Filter(filters::Message::Enabled(id.clone(), on)));
        // "Search" the noun, as the chip of a saved search: "Suche", not "Suchen".
        let chip = widgets::state_pill(Icon::Search, torro_core::i18n::trc("noun", "Search"));
        let open = Message::Filter(filters::Message::Edit(filter.id.clone()));
        rows.push(fixed_line(Some(chip), filter.query.clone(), filter.enabled, switch, Some(open)));
    }
    if category == Category::Personal {
        let on = data.settings.mail_contacts_personal;
        let switch = switch(on, Message::ContactsPersonal);
        rows.push(fixed_line(None, tr("From someone in your address books").to_string(), on, switch, None));
    }
    if rows.is_empty() {
        let none = typo::text(typo::body(), tr("No fixed rules yet.")).style(torro_gui::theme::text_2);
        rows.push(container(none).width(Fill).center_y(size::row()).padding([0.0, space::l()]).into());
    }
    widgets::group_with(Some(widgets::group_title(tr("Fixed rules · go before the AI"), vec![add])), rows, None)
}

/// A row of the fixed rules: `lead` (the chip of a filter), what it looks at – quieter and with
/// "Off" when switched off, so an off row stands out by word and colour –, its switch, and a
/// chevron when it opens (`open`).
fn fixed_line<'a>(
    lead: Option<Element<'a, Message>>,
    when: String,
    enabled: bool,
    control: Element<'a, Message>,
    open: Option<Message>,
) -> Element<'a, Message> {
    let tone: fn(&Theme) -> iced::widget::text::Style =
        if enabled { torro_gui::theme::text } else { torro_gui::theme::text_2 };
    let mut text = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(lead) = lead {
        text = text.push(lead);
    }
    text = text.push(container(widgets::ellipsized(typo::body(), when, tone)).width(Fill));
    if !enabled {
        text = text.push(typo::text(typo::meta(), tr("Off")).style(torro_gui::theme::text_3));
    }
    let mut line = row![text.width(Fill), control].spacing(space::m()).align_y(Alignment::Center);
    let chevron: Element<'a, Message> = match open {
        Some(_) => crate::parts::chevron(),
        None => blank().width(torro_gui::icon::SIZE).into(),
    };
    line = line.push(chevron);
    // As high as a category's row, so four rules fit on the page without scrolling.
    let line = container(line).width(Fill).center_y(size::row()).padding([0.0, space::l()]);
    pressable(line.into(), open)
}

/// "Show in the sidebar" (stored at once) and "AI may not file mail here" (saved with the page) as
/// two tiles, side by side from a form's width, else one below the other. A category that follows
/// from the headers has its AI tile without a switch: the AI never files mail there.
fn tiles<'a>(form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
    let category = form.category;
    let by_rule_only = category.is_some_and(Category::by_rule_only);
    let model = form.model;
    let counted = trf(
        "{group}: {n} of {most} categories for the AI",
        &[("group", &group_words(data, form.group)), ("n", &form.for_model(data)), ("most", &MODEL_PER_GROUP_MAX)],
    );
    let shown = category.map(|category| !data.settings.hidden_categories.contains(&category));
    responsive(move |room| {
        let ai: Element<'a, Message> = match by_rule_only {
            true => widgets::switch_tile(
                tr("AI may not file mail here"),
                Some(tr("The headers decide this category; the AI never files mail here.").to_string()),
                true,
                None,
            ),
            false => widgets::switch_tile(
                tr("AI may not file mail here"),
                Some(counted.clone()),
                !model,
                Some(Box::new(|off: bool| Message::Model(!off))),
            ),
        };
        let mut tiles: Vec<Element<'a, Message>> = Vec::new();
        if let (Some(category), Some(shown)) = (category, shown) {
            tiles.push(widgets::switch_tile(
                tr("Show in the sidebar"),
                None,
                shown,
                Some(Box::new(move |on: bool| Message::Shown(category, on))),
            ));
        }
        tiles.push(ai);
        if room.width >= size::form_max() {
            let line = tiles.into_iter().map(|tile| container(tile).width(Fill).into());
            row(line.collect::<Vec<Element<'a, Message>>>()).spacing(space::m()).into()
        } else {
            column(tiles).spacing(space::m()).into()
        }
    })
    .into()
}

/// The popover of a category's page that is open (issue #481): the symbols as a grid, the groups,
/// or "+ Rule" with a rule or a filter from a search.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let Some(Form::Category(form)) = &state.form else { return None };
    let dismiss = Message::Popover(None);
    Some(match state.popover? {
        Popover::Icon => {
            let width = ICON_COLUMNS * (size::control_sm() + space::s()) + space::m() * 2.0;
            let grid = container(icon_picker(form.icon, Message::Icon, form.group))
                .width(width)
                .padding(space::m())
                .style(style::container::menu);
            widgets::floating(&state.icon_anchor, Placement::Below(Align::Start), grid, dismiss)
        }
        Popover::Group => {
            let items = data
                .category_groups
                .iter()
                .map(|held| MenuItem::new(category_icon(held.icon), held.label.clone(), Message::Group(held.group)))
                .collect();
            let menu = container(widgets::menu(vec![MenuSection::new(items)])).width(widgets::menu_width());
            widgets::floating(&state.group_anchor, Placement::Below(Align::End), menu, dismiss)
        }
        Popover::Rule => {
            let category = form.category?;
            let items = vec![
                MenuItem::new(Icon::ListFilter, tr("Rule…"), Message::NewRuleIn(category)),
                MenuItem::new(
                    Icon::Search,
                    tr("Filter from a search…"),
                    Message::Filter(filters::Message::NewFor(category)),
                ),
            ];
            let menu = container(widgets::menu(vec![MenuSection::new(items)])).width(widgets::menu_width());
            widgets::floating(&state.rule_anchor, Placement::Below(Align::End), menu, dismiss)
        }
    })
}

/// The symbols of the popover in one line, so the grid is as wide as eight buttons.
const ICON_COLUMNS: f32 = 8.0;

/// The suite's description of `category`, as the catalogue gives it while nobody changed it.
fn suite_description(category: Category) -> String {
    torro_core::category::suite_description(category).unwrap_or_default().to_string()
}

/// The room of the description: three lines and the padding of a field, so four fixed rules fit
/// on the page without scrolling (issue #481).
const DESCRIPTION_LINES: f32 = 3.0;

fn description_height() -> f32 {
    DESCRIPTION_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// A category to choose in a pick list: its name, with its group in front.
#[derive(Debug, Clone)]
struct CategoryChoice {
    category: Category,
    words: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CategoryIconChoice(CategoryIcon);

impl std::fmt::Display for CategoryIconChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            CategoryIcon::MessageCircle => tr("Message"),
            CategoryIcon::ListTodo => tr("Task"),
            CategoryIcon::CircleCheck => tr("Approval"),
            CategoryIcon::UserCheck => tr("Assigned"),
            CategoryIcon::ShieldAlert => tr("Security"),
            CategoryIcon::User => tr("Person"),
            CategoryIcon::CalendarPlus => tr("New event"),
            CategoryIcon::TextQuote => tr("Quote"),
            CategoryIcon::Paperclip => tr("Attachment"),
            CategoryIcon::SquareCheck => tr("Checked box"),
            CategoryIcon::Send => tr("Send"),
            CategoryIcon::CalendarDays => tr("Calendar"),
            CategoryIcon::ChartColumn => tr("Statistics"),
            CategoryIcon::AlignLeft => tr("Text"),
            CategoryIcon::Mail => tr("Mail"),
            CategoryIcon::Flag => tr("Flag"),
            CategoryIcon::Bell => tr("Notification"),
            CategoryIcon::Users => tr("People"),
            CategoryIcon::Video => tr("Video"),
            CategoryIcon::MailOpen => tr("Read mail"),
            CategoryIcon::Reply => tr("Reply"),
            CategoryIcon::Trash2 => tr("Trash"),
            CategoryIcon::Eye => tr("View"),
            CategoryIcon::Ellipsis => tr("More"),
            CategoryIcon::CircleDashed => tr("Dashed circle"),
            CategoryIcon::Tag => tr("Tag"),
            CategoryIcon::Megaphone => tr("Megaphone"),
            CategoryIcon::Bot => tr("Robot"),
            CategoryIcon::Receipt => tr("Receipt"),
            CategoryIcon::Package => tr("Package"),
            CategoryIcon::Skull => tr("Skull"),
            CategoryIcon::Newspaper => tr("Newspaper"),
            CategoryIcon::Building2 => tr("Building"),
            CategoryIcon::Landmark => tr("Bank"),
            CategoryIcon::TriangleAlert => tr("Warning"),
        })
    }
}

fn icon_picker<'a>(
    selected: CategoryIcon,
    on_pick: fn(CategoryIcon) -> Message,
    _group: CategoryGroup,
) -> Element<'a, Message> {
    responsive(move |room| {
        // A visible palette lets people recognise the symbol before choosing it. Keep each
        // button's spoken name and tooltip; narrow windows simply use fewer columns.
        let columns = ((room.width / (size::control_sm() + space::s())).floor() as usize).clamp(3, 8);
        let mut lines = column![].spacing(space::s());
        for icons in CategoryIcon::ALL.chunks(columns) {
            let mut line = row![].spacing(space::s());
            for &icon in icons {
                let name = CategoryIconChoice(icon).to_string();
                let button = if icon == selected {
                    widgets::compact_button(category_icon(icon), name, style::button::icon_chosen, Some(on_pick(icon)))
                } else {
                    widgets::boxed_button(category_icon(icon), name, Some(on_pick(icon)))
                };
                line = line.push(button);
            }
            lines = lines.push(line);
        }
        lines.into()
    })
    .into()
}

pub(super) fn category_icon(icon: CategoryIcon) -> Icon {
    match icon {
        CategoryIcon::MessageCircle => Icon::MessageCircle,
        CategoryIcon::ListTodo => Icon::ListTodo,
        CategoryIcon::CircleCheck => Icon::CircleCheck,
        CategoryIcon::UserCheck => Icon::UserCheck,
        CategoryIcon::ShieldAlert => Icon::ShieldAlert,
        CategoryIcon::User => Icon::User,
        CategoryIcon::CalendarPlus => Icon::CalendarPlus,
        CategoryIcon::TextQuote => Icon::TextQuote,
        CategoryIcon::Paperclip => Icon::Paperclip,
        CategoryIcon::SquareCheck => Icon::SquareCheck,
        CategoryIcon::Send => Icon::Send,
        CategoryIcon::CalendarDays => Icon::CalendarDays,
        CategoryIcon::ChartColumn => Icon::ChartColumn,
        CategoryIcon::AlignLeft => Icon::AlignLeft,
        CategoryIcon::Mail => Icon::Mail,
        CategoryIcon::Flag => Icon::Flag,
        CategoryIcon::Bell => Icon::Bell,
        CategoryIcon::Users => Icon::Users,
        CategoryIcon::Video => Icon::Video,
        CategoryIcon::MailOpen => Icon::MailOpen,
        CategoryIcon::Reply => Icon::Reply,
        CategoryIcon::Trash2 => Icon::Trash2,
        CategoryIcon::Eye => Icon::Eye,
        CategoryIcon::Ellipsis => Icon::Ellipsis,
        CategoryIcon::CircleDashed => Icon::CircleDashed,
        CategoryIcon::Tag => Icon::Tag,
        CategoryIcon::Megaphone => Icon::Megaphone,
        CategoryIcon::Bot => Icon::Bot,
        CategoryIcon::Receipt => Icon::Receipt,
        CategoryIcon::Package => Icon::Package,
        CategoryIcon::Skull => Icon::Skull,
        CategoryIcon::Newspaper => Icon::Newspaper,
        CategoryIcon::Building2 => Icon::Building2,
        CategoryIcon::Landmark => Icon::Landmark,
        CategoryIcon::TriangleAlert => Icon::TriangleAlert,
    }
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
    data.categories
        .iter()
        .filter(|info| Some(info.category) != leave_out)
        .map(|info| CategoryChoice {
            category: info.category, words: format!("{} · {}", info.group_label, info.label)
        })
        .collect()
}

fn group_words(data: &Data, group: CategoryGroup) -> String {
    data.category_groups
        .iter()
        .find(|held| held.group == group)
        .map_or_else(|| torro_core::category::group_label(group).to_string(), |held| held.label.clone())
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

/// Once "Remove…" was pressed on a category of the person's own: the category its mail goes to,
/// "Cancel" and "Remove category" in one row.
fn removal<'a>(form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
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
    if form.id.is_some() {
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
fn condition_rows(list: &[SortCondition], exception: bool) -> Vec<Element<'_, Message>> {
    let mut rows: Vec<Element<'_, Message>> = list
        .iter()
        .enumerate()
        .map(|(at, condition)| {
            responsive(move |room| {
                let kind = picker(FIELD_CHOICES, FieldChoice(condition.field), move |choice: FieldChoice| {
                    Message::Field { exception, at, field: choice.0 }
                });
                let comparison = if condition.comparison == SortComparison::Auto {
                    condition.field.default_comparison()
                } else {
                    condition.comparison
                };
                let comparisons: Vec<ComparisonChoice> =
                    COMPARISON_CHOICES.iter().copied().filter(|choice| condition.field.allows(choice.0)).collect();
                let comparison = picker(comparisons, ComparisonChoice(comparison), move |choice: ComparisonChoice| {
                    Message::Comparison { exception, at, comparison: choice.0 }
                });
                let drop = widgets::icon_button(Icon::X, tr("Remove"), Some(Message::Drop { exception, at }));
                let value: Element<'_, Message> = if condition.field == SortField::Attachment {
                    blank::horizontal().into()
                } else {
                    let mut words = field(placeholder(condition.field), &condition.value)
                        .on_input(move |value| Message::Value { exception, at, value })
                        .on_submit(Message::SaveRule);
                    if !exception && at == 0 {
                        words = words.id(CONDITION_FIELD);
                    }
                    words.into()
                };
                if room.width >= 700.0 {
                    row_frame(row![kind, comparison, value, drop].spacing(space::s()).align_y(Alignment::Center), false)
                } else {
                    row_frame(
                        column![row![kind, comparison].spacing(space::s()), row![value, drop].spacing(space::s())]
                            .spacing(space::s()),
                        false,
                    )
                }
            })
            .into()
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
        SortField::ReplyTo | SortField::Recipient => "name@example.org",
        SortField::Subject | SortField::Text => tr("Words"),
        SortField::Attachment => "",
        SortField::AttachmentName => "invoice.pdf",
        SortField::Size => tr("Megabytes"),
        SortField::ListId => "news.example.org",
        SortField::Account => "anna@example.org",
    }
}

/// The field of a condition as a choice of a pick list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FieldChoice(SortField);

const FIELD_CHOICES: [FieldChoice; 11] = [
    FieldChoice(SortField::Sender),
    FieldChoice(SortField::Domain),
    FieldChoice(SortField::ReplyTo),
    FieldChoice(SortField::Recipient),
    FieldChoice(SortField::Subject),
    FieldChoice(SortField::Text),
    FieldChoice(SortField::Attachment),
    FieldChoice(SortField::AttachmentName),
    FieldChoice(SortField::Size),
    FieldChoice(SortField::ListId),
    FieldChoice(SortField::Account),
];

impl std::fmt::Display for FieldChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            SortField::Sender => tr("Sender address"),
            SortField::Domain => tr("Sender domain"),
            SortField::ReplyTo => tr("Reply address"),
            SortField::Recipient => tr("Recipient (To/Cc)"),
            SortField::Subject => tr("Subject"),
            SortField::Text => tr("Message text"),
            SortField::Attachment => tr("Attachment"),
            SortField::AttachmentName => tr("Attachment name or type"),
            SortField::Size => tr("Message size"),
            SortField::ListId => tr("Mailing list (List-ID)"),
            SortField::Account => tr("Email account"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComparisonChoice(SortComparison);

const COMPARISON_CHOICES: [ComparisonChoice; 10] = [
    ComparisonChoice(SortComparison::Is),
    ComparisonChoice(SortComparison::IsNot),
    ComparisonChoice(SortComparison::Contains),
    ComparisonChoice(SortComparison::NotContains),
    ComparisonChoice(SortComparison::StartsWith),
    ComparisonChoice(SortComparison::EndsWith),
    ComparisonChoice(SortComparison::Present),
    ComparisonChoice(SortComparison::Absent),
    ComparisonChoice(SortComparison::GreaterThan),
    ComparisonChoice(SortComparison::LessThan),
];

impl std::fmt::Display for ComparisonChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            SortComparison::Auto => tr("matches"),
            SortComparison::Is => tr("is"),
            SortComparison::IsNot => tr("is not"),
            SortComparison::Contains => tr("contains"),
            SortComparison::NotContains => tr("does not contain"),
            SortComparison::StartsWith => tr("starts with"),
            SortComparison::EndsWith => tr("ends with"),
            SortComparison::Present => tr("present"),
            SortComparison::Absent => tr("not present"),
            SortComparison::GreaterThan => tr("larger than"),
            SortComparison::LessThan => tr("smaller than"),
        })
    }
}

#[cfg(test)]
mod tests {
    use torro_ipc::inbox::CategoryInfo;
    use torro_mail::CategoryDef;

    use super::*;

    /// The suite's catalogue and one category of the person's own, "Tax" in "Paper trail".
    /// Issue #481: a category's page opens its rules and goes back there; a new rule it starts is
    /// no change yet; with changes on the page "Discard" opens the rule all the same; "Move up" and
    /// "Move down" move it one place in its group, and stop at its ends; the popovers close on Escape.
    #[test]
    fn a_categorys_page_leads_to_its_rules_and_back() {
        let (data, mut state) = (data(), State::default());
        let invoice = Category::Invoice;
        let _: Action<()> = update(&mut state, Message::EditCategory(invoice), &data);
        let _ = page(&state, &data);
        let _: Action<()> = update(&mut state, Message::NewRuleIn(invoice), &data);
        assert!(matches!(&state.form, Some(Form::Rule(form)) if form.category == Some(invoice)));
        let _: Action<()> = update(&mut state, Message::Back, &data);
        assert!(state.asking.is_none(), "nothing typed, nothing to discard");
        assert_eq!(state.open_category(), Some(invoice), "back on the category's page");
        let _: Action<()> = update(&mut state, Message::EditRule("r1".into()), &data);
        let _: Action<()> = update(&mut state, Message::Back, &data);
        assert_eq!(state.open_category(), Some(invoice));
        // Changes on the page: asked first, then the rule opens and comes back to the category.
        let _: Action<()> = update(&mut state, typed("Changed."), &data);
        let _: Action<()> = update(&mut state, Message::EditRule("r1".into()), &data);
        assert_eq!(state.asking, Some(Ask::Discard));
        let _: Action<()> = update(&mut state, Message::Answer(true), &data);
        assert!(matches!(&state.form, Some(Form::Rule(form)) if form.id.as_deref() == Some("r1")));
        let _: Action<()> = update(&mut state, Message::Back, &data);
        assert_eq!(state.open_category(), Some(invoice));
        // One place down in its group; the first has no place above.
        let first = data.categories.iter().find(|info| info.group == CategoryGroup::PaperTrail).expect("one");
        let down = sent(&mut state, &data, vec![Message::Shift { category: first.category, up: false }]);
        assert_eq!(
            down,
            vec![Request::MoveCategory { category: first.category, group: CategoryGroup::PaperTrail, position: 1 }]
        );
        assert!(sent(&mut state, &data, vec![Message::Shift { category: first.category, up: true }]).is_empty());
        let _: Action<()> = update(&mut state, Message::Popover(Some(Popover::Group)), &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Popover(None))));
    }

    fn data() -> Data {
        let mut defs = CategoryDef::defaults();
        defs.push(CategoryDef {
            category: tax(),
            group: CategoryGroup::PaperTrail,
            name: Some("Tax".into()),
            description: Some("For the tax return.".into()),
            model: true,
            icon: CategoryIcon::Tag,
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
            conditions: vec![SortCondition {
                comparison: SortComparison::Auto,
                field: SortField::Domain,
                value: domain.into(),
            }],
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

    #[test]
    fn main_category_can_be_created_and_used_by_a_child() {
        let mut data = data();
        data.category_groups =
            torro_mail::CategoryGroupDef::defaults().iter().map(torro_ipc::inbox::CategoryGroupInfo::of).collect();
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::NewGroup, &data);
        let _ = update::<()>(&mut state, Message::GroupName("Belege".into()), &data);
        let _ = update::<()>(&mut state, Message::GroupIcon(CategoryIcon::Paperclip), &data);
        let _ = update::<()>(&mut state, Message::GroupColor(2), &data);
        let request = update::<()>(&mut state, Message::SaveGroup, &data);
        assert_eq!(
            request,
            Action::send(Request::CreateCategoryGroup {
                name: "Belege".into(),
                icon: CategoryIcon::Paperclip,
                color: 2,
            })
        );
        assert!(state.group_form.as_ref().is_some_and(|form| form.sending));
        let created = torro_ipc::inbox::CategoryGroupInfo {
            group: CategoryGroup::Custom(5),
            label: "Belege".into(),
            icon: CategoryIcon::Paperclip,
            color: 2,
            built_in: false,
        };
        // Another send came back first: the catalogue does not hold it yet, the form waits.
        super::sent(&mut state, &Ok(data.clone()));
        assert!(state.group_form.as_ref().is_some_and(|form| form.sending));
        data.category_groups.push(created);
        super::sent(&mut state, &Ok(data.clone()));
        assert!(state.group_form.is_none(), "saved: the catalogue holds it");
        let _ = update::<()>(&mut state, Message::NewCategoryInGroup(CategoryGroup::Custom(5)), &data);
        assert!(matches!(&state.form, Some(Form::Category(form)) if form.group == CategoryGroup::Custom(5)));
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
                icon: CategoryIcon::Tag,
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
                icon: CategoryIcon::Tag,
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
            vec![
                Message::EditCategory(Category::Invoice),
                Message::Name("Bills".into()),
                Message::Icon(CategoryIcon::MailOpen),
                Message::SaveCategory,
            ],
        );
        assert_eq!(
            requests,
            vec![Request::UpdateCategory {
                category: Category::Invoice,
                group: None,
                name: Some("Bills".into()),
                description: None,
                model: None,
                icon: Some(CategoryIcon::MailOpen),
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
        assert!(matches!(
            reset.as_slice(),
            [Request::UpdateCategory { name: Some(name), icon: Some(icon), .. }]
                if name.is_empty() && *icon == Category::Invoice.default_icon()
        ));
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
                conditions: vec![SortCondition {
                    comparison: SortComparison::Is,
                    field: SortField::Domain,
                    value: "rail.example".into()
                }],
                exceptions: vec![SortCondition {
                    comparison: SortComparison::Contains,
                    field: SortField::Subject,
                    value: "delay".into()
                }],
                position: Some(0),
                enabled: true,
            }]
        );
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::NewRule, &data);
        assert!(matches!(update::<()>(&mut state, Message::SaveRule, &data), Action::Focus(CONDITION_FIELD)));
    }

    #[test]
    fn the_eleven_mail_fields_offer_fitting_comparisons_and_attachment_needs_no_value() {
        assert_eq!(FIELD_CHOICES.len(), 11);
        assert!(SortField::Attachment.allows(SortComparison::Absent));
        assert!(!SortField::Attachment.allows(SortComparison::Contains));
        let (data, mut state) = (data(), State::default());
        let requests = sent(
            &mut state,
            &data,
            vec![
                Message::NewRule,
                Message::Field { exception: false, at: 0, field: SortField::Attachment },
                Message::Comparison { exception: false, at: 0, comparison: SortComparison::Absent },
                Message::RuleCategory(Category::Invoice),
                Message::SaveRule,
            ],
        );
        let [Request::CreateSortRule { conditions, .. }] = requests.as_slice() else { panic!("one sorting rule") };
        assert_eq!(
            conditions,
            &[SortCondition { field: SortField::Attachment, comparison: SortComparison::Absent, value: String::new() }]
        );
        let _: Action<()> = update(&mut state, Message::NewRule, &data);
        let _: Action<()> =
            update(&mut state, Message::Field { exception: false, at: 0, field: SortField::Size }, &data);
        let _: Action<()> =
            update(&mut state, Message::Value { exception: false, at: 0, value: "a lot".into() }, &data);
        let _: Action<()> = update(&mut state, Message::RuleCategory(Category::Invoice), &data);
        assert!(sent(&mut state, &data, vec![Message::SaveRule]).is_empty());
    }

    /// A stored rule: the switch on its page is saved with it; its page has no place to move it to
    /// any more (issue #481), the order of the categories decides.
    #[test]
    fn a_rule_switches_on_its_page() {
        let (data, mut state) = (data(), State::default());
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
