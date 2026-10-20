//! The editor of the mail categories and the sorting rules (task E15.5, issue #255,
//! `docs/kategorien.md`, "Eigene Kategorien und Sortierregeln"), part of Settings ›
//! Categorization in the mail window.
//!
//! ```text
//! Categories     one row per category, by group; a row opens its page   + New category
//! Sorting rules  one row per rule, in its order: ↑ ↓, on/off, edit     + New sorting rule
//! ```
//!
//! A category's page: name, group, description, "Let the AI choose it" with how many of its group
//! the AI chooses between ("7 of 20"), and for one of the person's own "Remove…" with the
//! category its mail goes to. A rule's page: the category, the conditions and the exceptions
//! (field and words each), on/off, and "Delete rule".
//!
//! Everything is a request (`inbox.create_category`, `inbox.update_category`,
//! `inbox.remove_category`, `inbox.create_sort_rule`, `inbox.update_sort_rule`,
//! `inbox.remove_sort_rule`, `inbox.move_sort_rule`); the page checks only what a person must
//! fix before saving – the core checks all of it again. Mistakes show after the first "Save",
//! never before (`.claude/skills/torro-design`, forms).

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, space as blank, text_editor};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{palette, space};
use torro_gui::{layout, style, typo, widgets};
use torro_inbox::{SortCondition, SortField, SortRule};
use torro_ipc::Request;
use torro_ipc::inbox::CategoryInfo;
use torro_mail::category::{DESCRIPTION_MAX, MODEL_PER_GROUP_MAX, NAME_MAX};
use torro_mail::{Category, CategoryGroup};

use crate::parts::{chevron, choice, field, picker, pressable, row_frame, setting_row, switch};
use crate::{Action, Data};

/// What the editor keeps between two frames: the page that is open, if any.
#[derive(Debug, Default)]
pub struct State {
    form: Option<Form>,
}

impl State {
    /// A page of the editor is open: the section shows it instead of its list.
    pub fn is_open(&self) -> bool {
        self.form.is_some()
    }
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
    SaveCategory,
    /// "Use the suite's words": name and description of a category of the suite back.
    PutBack,
    /// "Remove…": ask where the mail goes.
    Remove,
    Replacement(Category),
    /// "Remove category" after the replacement was chosen.
    ConfirmRemove,
    /// "New sorting rule".
    NewRule,
    /// A rule's "Edit": open its page.
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
    DeleteRule,
    /// ↑ (`up: true`) or ↓ on a rule's row.
    Move {
        id: String,
        up: bool,
    },
    /// The switch on a rule's row.
    Toggle {
        id: String,
        on: bool,
    },
    /// "‹ Back" from a page to the list.
    Back,
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::NewCategory => {
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
                state.form = Some(Form::Category(CategoryForm {
                    category: Some(category),
                    name: info.label.clone(),
                    group: info.group,
                    description: text_editor::Content::with_text(&info.description),
                    model: info.model,
                    removing: false,
                    replacement: None,
                    tried: false,
                }));
            }
            Action::None
        }
        Message::NewRule => {
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
                state.form = Some(Form::Rule(RuleForm::of(rule)));
            }
            Action::None
        }
        Message::Back => {
            state.form = None;
            Action::None
        }
        Message::Move { id, up } => {
            let Some(at) = data.sort_rules.iter().position(|rule| rule.id == id) else { return Action::None };
            let position = match up {
                true if at > 0 => at - 1,
                false if at + 1 < data.sort_rules.len() => at + 1,
                _ => return Action::None,
            };
            Action::send(Request::MoveSortRule { id, position: position as u32 })
        }
        Message::Toggle { id, on } => match data.sort_rules.iter().find(|rule| rule.id == id) {
            Some(rule) => Action::send(Request::UpdateSortRule {
                id,
                category: rule.category,
                conditions: rule.conditions.clone(),
                exceptions: rule.exceptions.clone(),
                enabled: on,
            }),
            None => Action::None,
        },
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

/// A message of a category's page: the action, and whether the page closes.
fn category_update<A>(form: &mut CategoryForm, message: Message, data: &Data) -> (Action<A>, bool) {
    match message {
        Message::Name(name) => form.name = name,
        Message::Group(group) => form.group = group,
        Message::Description(action) => form.description.perform(action),
        Message::Model(on) => form.model = on,
        Message::Remove => form.removing = true,
        Message::Replacement(category) => form.replacement = Some(category),
        Message::PutBack => {
            let Some(category) = form.category else { return (Action::None, false) };
            let put_back = Request::UpdateCategory {
                category,
                group: None,
                name: Some(String::new()),
                description: Some(String::new()),
                model: None,
            };
            return (Action::send(put_back), true);
        }
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
                return (Action::None, false);
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
            if list.len() < torro_inbox::item::SORT_CONDITIONS_MAX {
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
        Message::DeleteRule => {
            let Some(id) = form.id.clone() else { return (Action::None, true) };
            return (Action::send(Request::RemoveSortRule { id }), true);
        }
        Message::SaveRule => {
            form.tried = true;
            if !form.mistakes().is_empty() {
                return (Action::None, false);
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

impl CategoryForm {
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

    /// What must be fixed before saving, in the order of the page.
    fn mistakes(&self, data: &Data) -> Vec<String> {
        let mut found = Vec::new();
        let name = self.name.trim();
        if name.is_empty() {
            found.push(tr("Give the category a name.").to_string());
        } else if name.chars().count() > NAME_MAX {
            let most = NAME_MAX.to_string();
            found.push(trf("A category's name has at most {most} characters.", &[("most", &most)]));
        } else if self.others(data).any(|info| info.label.to_lowercase() == name.to_lowercase()) {
            found.push(trf("There is a category called “{name}” already.", &[("name", &name)]));
        }
        let description = self.description.text();
        if description.trim().is_empty() && self.built_in(data).is_none() {
            found.push(tr("Describe what belongs in the category.").to_string());
        } else if description.trim().chars().count() > DESCRIPTION_MAX {
            let most = DESCRIPTION_MAX.to_string();
            found.push(trf("A description has at most {most} characters.", &[("most", &most)]));
        }
        if self.model && self.for_model(data) > MODEL_PER_GROUP_MAX {
            found.push(trf(
                "{group} would have {n} categories for the AI; at most {most} fit.",
                &[
                    ("group", &torro_core::category::group_label(self.group)),
                    ("n", &self.for_model(data).to_string()),
                    ("most", &MODEL_PER_GROUP_MAX.to_string()),
                ],
            ));
        }
        found
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

    fn list(&mut self, exception: bool) -> &mut Vec<(SortField, String)> {
        if exception { &mut self.exceptions } else { &mut self.conditions }
    }

    /// What must be fixed before saving.
    fn mistakes(&self) -> Vec<String> {
        let mut found = Vec::new();
        if self.category.is_none() {
            found.push(tr("Choose the category the mail is filed as.").to_string());
        }
        if self.conditions.iter().all(|(_, value)| value.trim().is_empty()) {
            found.push(tr("A sorting rule needs at least one condition.").to_string());
        }
        let too_long =
            |(_, value): &(SortField, String)| value.trim().chars().count() > torro_inbox::item::SORT_VALUE_MAX;
        if self.conditions.iter().chain(&self.exceptions).any(too_long) {
            let most = torro_inbox::item::SORT_VALUE_MAX.to_string();
            found.push(trf("A condition's value has at most {most} characters.", &[("most", &most)]));
        }
        found
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
            Some(id) => {
                Request::UpdateSortRule { id: id.clone(), category, conditions, exceptions, enabled: self.enabled }
            }
        })
    }
}

/// The open page's name for the path, and its body; `None` while the list shows.
pub fn page<'a>(state: &'a State, data: &'a Data) -> Option<(String, Element<'a, Message>)> {
    match state.form.as_ref()? {
        Form::Category(form) => {
            let name = match form.category {
                None => tr("New category").to_string(),
                Some(category) => crate::category_name_in(&data.categories, category),
            };
            Some((name, category_page(form, data)))
        }
        Form::Rule(form) => {
            let name = match form.id {
                None => tr("New sorting rule").to_string(),
                Some(_) => tr("Sorting rule").to_string(),
            };
            Some((name, rule_page(form, data)))
        }
    }
}

/// "Categories": one row per category of the catalogue, by group, each opening its page; "New
/// category" last.
pub fn categories_group(data: &Data) -> Element<'_, Message> {
    let mut rows = Vec::new();
    for group in CategoryGroup::ALL {
        let of_group: Vec<&CategoryInfo> = data.categories.iter().filter(|info| info.group == group).collect();
        for (nth, info) in of_group.into_iter().enumerate() {
            let what = match info.model {
                true => tr("The AI may choose it"),
                false => tr("Only by rules and by hand"),
            };
            let detail = match nth {
                0 => format!("{} · {what}", info.group_label),
                _ => what.to_string(),
            };
            let row = setting_row(None, info.label.clone(), Some(detail), chevron());
            rows.push(pressable(row, Some(Message::EditCategory(info.category))));
        }
    }
    rows.push(add_row(tr("New category"), Message::NewCategory));
    widgets::group(
        tr("Categories"),
        rows,
        Some(
            tr("Rename any category, move it to another group, or make your own. The description is what the AI reads.")
                .to_string(),
        ),
    )
}

/// "Sorting rules": the rules in their order, each with ↑ ↓, its switch and "Edit"; "New sorting
/// rule" last.
pub fn rules_group(data: &Data) -> Element<'_, Message> {
    let count = data.sort_rules.len();
    let mut rows: Vec<Element<'_, Message>> = data
        .sort_rules
        .iter()
        .enumerate()
        .map(|(at, rule)| {
            let category = crate::category_name_in(&data.categories, rule.category);
            let label = trf(
                "{conditions} → {category}",
                &[("conditions", &conditions_line(&rule.conditions)), ("category", &category)],
            );
            let detail = match (rule.enabled, rule.exceptions.is_empty()) {
                (false, _) => tr("Off").to_string(),
                (true, true) => trf("Place {n}", &[("n", &(at + 1))]),
                (true, false) => trf("Except {exceptions}", &[("exceptions", &conditions_line(&rule.exceptions))]),
            };
            let up = (at > 0).then(|| Message::Move { id: rule.id.clone(), up: true });
            let down = (at + 1 < count).then(|| Message::Move { id: rule.id.clone(), up: false });
            let id = rule.id.clone();
            let controls = row![
                widgets::icon_button(Icon::ChevronUp, tr("Move up"), up),
                widgets::icon_button(Icon::ChevronDown, tr("Move down"), down),
                switch(rule.enabled, move |on| Message::Toggle { id: id.clone(), on }),
                widgets::icon_button(Icon::SquarePen, tr("Edit rule"), Some(Message::EditRule(rule.id.clone()))),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center);
            setting_row(None, label, Some(detail), controls)
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

/// The last row of a list: "+ New …".
fn add_row<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    let plus = torro_gui::icon::icon(Icon::Plus, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    pressable(setting_row(Some(plus.into()), label, None, blank().width(0)), Some(message))
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

/// A line in the colour of mistakes, for under a group.
fn mistake_lines<'a>(mistakes: Vec<String>) -> Element<'a, Message> {
    let lines = mistakes.into_iter().map(|mistake| {
        typo::text(typo::meta(), mistake)
            .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
            .wrapping(Wrapping::WordOrGlyph)
            .into()
    });
    column(lines).spacing(space::xs()).into()
}

/// "Save" at the end of a page, on only while there is something to save.
fn save_row<'a>(label: &'a str, ready: bool, message: Message) -> Element<'a, Message> {
    let look: widgets::ButtonStyle = if ready { style::button::primary } else { style::button::secondary };
    row![blank::horizontal(), widgets::text_button(None, label, look, ready.then_some(message))].into()
}

/// A category's page.
fn category_page<'a>(form: &'a CategoryForm, data: &'a Data) -> Element<'a, Message> {
    let name = field(tr("Name"), &form.name).on_input(Message::Name).on_submit(Message::SaveCategory);
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
        Some(_) => tr("Leave it as it is to keep the suite's description. A hard exception is a sorting rule."),
        None => tr("The AI reads it to decide. A hard exception is a sorting rule."),
    };
    let mut page = column![
        widgets::group(tr("Name"), vec![row_frame(name, false)], None),
        widgets::group(tr("Group"), groups, Some(tr("The group gives the category its colour and icon.").to_string())),
        widgets::group(
            tr("Description"),
            vec![container(description).width(Fill).padding([space::s(), space::l()]).into()],
            Some(description_note.to_string()),
        ),
        widgets::group(tr("AI"), vec![model], None),
    ]
    .spacing(layout::section_gap());
    let ready = form.category.is_none() || form.request(data).is_some();
    let mut end = column![].spacing(space::s());
    if form.tried {
        end = end.push(mistake_lines(form.mistakes(data)));
    }
    end = end.push(save_row(tr("Save"), ready, Message::SaveCategory));
    page = page.push(end);
    if built_in.is_some_and(|info| info.renamed || info.description != suite_description(info.category)) {
        let put_back =
            widgets::text_button(None, tr("Use the suite's words"), style::button::secondary, Some(Message::PutBack));
        page = page.push(widgets::group(
            tr("Suite"),
            vec![setting_row(None, tr("Name and description of the suite"), None, put_back)],
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

/// "Remove…" for a category of the person's own, and once pressed the categories its mail can go
/// to, with "Remove category".
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
    let mut rows: Vec<Element<'a, Message>> = form
        .others(data)
        .map(|info| {
            choice(info.label.as_str(), form.replacement == Some(info.category), Message::Replacement(info.category))
        })
        .collect();
    let confirm = widgets::text_button(
        Some(Icon::Trash2),
        tr("Remove category"),
        style::button::danger,
        form.replacement.map(|_| Message::ConfirmRemove),
    );
    rows.push(row_frame(row![blank::horizontal(), confirm], false));
    widgets::group(
        tr("File its mail as"),
        rows,
        Some(tr("Who filed the mail stays as it was: what you filed by hand is still yours.").to_string()),
    )
}

/// A rule's page.
fn rule_page<'a>(form: &'a RuleForm, data: &'a Data) -> Element<'a, Message> {
    let mut categories = Vec::new();
    for group in CategoryGroup::ALL {
        for (nth, info) in data.categories.iter().filter(|info| info.group == group).enumerate() {
            let label = match nth {
                0 => format!("{} · {}", info.group_label, info.label),
                _ => info.label.clone(),
            };
            categories.push(choice(label, form.category == Some(info.category), Message::RuleCategory(info.category)));
        }
    }
    let conditions = condition_rows(&form.conditions, false);
    let exceptions = condition_rows(&form.exceptions, true);
    let enabled = setting_row(None, tr("Rule is on"), None, switch(form.enabled, Message::RuleEnabled));
    let mut page = column![
        widgets::group(
            tr("Conditions"),
            conditions,
            Some(tr("Every condition must fit. Upper and lower case do not count.").to_string()),
        ),
        widgets::group(
            tr("Exceptions"),
            exceptions,
            Some(tr("If any of these fits, the rule leaves the mail alone.").to_string())
        ),
        widgets::group(tr("File as"), categories, None),
        widgets::group(tr("Rule"), vec![enabled], None),
    ]
    .spacing(layout::section_gap());
    let ready = match &form.id {
        None => true,
        Some(id) => data
            .sort_rules
            .iter()
            .find(|rule| &rule.id == id)
            .is_none_or(|stored| RuleForm::of(stored) != RuleForm { tried: false, ..form.clone() }),
    };
    let mut end = column![].spacing(space::s());
    if form.tried {
        end = end.push(mistake_lines(form.mistakes()));
    }
    end = end.push(save_row(tr("Save"), ready, Message::SaveRule));
    page = page.push(end);
    if form.id.is_some() {
        let delete = widgets::text_button(
            Some(Icon::Trash2),
            tr("Delete rule"),
            style::button::danger,
            Some(Message::DeleteRule),
        );
        page = page.push(widgets::group(
            tr("Delete"),
            vec![setting_row(None, tr("Delete this rule"), None, delete)],
            Some(tr("Mail it filed keeps its category.").to_string()),
        ));
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
            let words =
                field(placeholder(*field_kind), value).on_input(move |value| Message::Value { exception, at, value });
            let drop = widgets::icon_button(Icon::X, tr("Remove"), Some(Message::Drop { exception, at }));
            row_frame(row![kind, words, drop].spacing(space::s()).align_y(Alignment::Center), false)
        })
        .collect();
    if list.len() < torro_inbox::item::SORT_CONDITIONS_MAX {
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

    /// What `message` sends, after the messages before it on the same state.
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

    /// Mistakes show only after "Save", and nothing is sent while there are any: no name, a name
    /// taken, no description of one's own, more than 20 for the AI in a group.
    #[test]
    fn a_category_with_mistakes_is_not_sent() {
        let (data, mut state) = (data(), State::default());
        assert!(sent(&mut state, &data, vec![Message::NewCategory, Message::SaveCategory]).is_empty());
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
        let off = sent(&mut state, &full, vec![Message::Model(false), Message::SaveCategory]);
        assert_eq!(off.len(), 1, "not for the AI it fits");
    }

    /// Changing a category sends only what changed; the page stays open.
    #[test]
    fn changing_a_category_sends_what_changed() {
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
        let nothing = sent(
            &mut state,
            &data,
            vec![Message::Back, Message::EditCategory(Category::Invoice), Message::SaveCategory],
        );
        assert!(nothing.is_empty(), "nothing changed, nothing sent");
    }

    /// Removing asks where the mail goes and sends nothing before.
    #[test]
    fn removing_a_category_asks_for_its_replacement() {
        let (data, mut state) = (data(), State::default());
        let early =
            sent(&mut state, &data, vec![Message::EditCategory(tax()), Message::Remove, Message::ConfirmRemove]);
        assert!(early.is_empty(), "no replacement chosen yet");
        let requests = sent(&mut state, &data, vec![Message::Replacement(Category::Document), Message::ConfirmRemove]);
        assert_eq!(requests, vec![Request::RemoveCategory { category: tax(), replacement: Some(Category::Document) }]);
        assert!(!state.is_open());
    }

    /// A rule is made with its conditions and exceptions, empty lines left out.
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
        let nothing = sent(&mut state, &data, vec![Message::NewRule, Message::SaveRule]);
        assert!(nothing.is_empty(), "no category and no condition");
    }

    /// ↑ and ↓ move by one place and stop at the ends; the switch keeps the rest of the rule.
    #[test]
    fn the_list_moves_and_switches_rules() {
        let (data, mut state) = (data(), State::default());
        let down = sent(&mut state, &data, vec![Message::Move { id: "r1".into(), up: false }]);
        assert_eq!(down, vec![Request::MoveSortRule { id: "r1".into(), position: 1 }]);
        assert!(sent(&mut state, &data, vec![Message::Move { id: "r1".into(), up: true }]).is_empty(), "first already");
        let off = sent(&mut state, &data, vec![Message::Toggle { id: "r2".into(), on: false }]);
        assert_eq!(
            off,
            vec![Request::UpdateSortRule {
                id: "r2".into(),
                category: Category::Invoice,
                conditions: data.sort_rules[1].conditions.clone(),
                exceptions: Vec::new(),
                enabled: false,
            }]
        );
        let deleted = sent(&mut state, &data, vec![Message::EditRule("r2".into()), Message::DeleteRule]);
        assert_eq!(deleted, vec![Request::RemoveSortRule { id: "r2".into() }]);
    }

    /// Every page and the lists draw, in both languages' words.
    #[test]
    fn the_pages_draw() {
        let (data, mut state) = (data(), State::default());
        let _ = categories_group(&data);
        let _ = rules_group(&data);
        for open in [Message::NewCategory, Message::EditCategory(Category::Invoice), Message::EditCategory(tax())] {
            let _ = update::<()>(&mut state, open, &data);
            let _ = update::<()>(&mut state, Message::Remove, &data);
            assert!(page(&state, &data).is_some());
        }
        let _ = update::<()>(&mut state, Message::EditRule("r1".into()), &data);
        let Some((name, _)) = page(&state, &data) else { panic!("a page") };
        assert_eq!(name, tr("Sorting rule"));
    }
}
