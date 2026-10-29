//! The catalogue of mail categories as the person keeps it (task E15.2, issue #255,
//! `docs/kategorien.md`, "Eigene Kategorien und Sortierregeln").
//!
//! The suite brings 23 categories ([`CategoryDef::defaults`]); the person changes name, group,
//! description and whether a model may choose any of them (`category_edits`) and makes their own
//! (`custom_categories`, `custom_<n>`). A key never changes, so renaming keeps every mail, entry
//! and rule valid. The numbers of the person's own come from SQLite's `AUTOINCREMENT`, which never
//! hands one out twice – also not after the category was removed.
//!
//! **Removing** a category of the person's own may touch three places: the sorting rules and the
//! entries here, and the mail in the store of every account, which is another database. One
//! transaction cannot hold them all, so the removal is kept as a step of its own: the category
//! notes its replacement (`replaced_by`) and the rules move with it ([`begin_removal`]), the core
//! moves the mail, and [`finish_removal`] moves the entries and deletes the row. While a removal
//! stands, the category is out of the catalogue; one cut short is taken up again when the core
//! starts ([`pending`]).
//!
//! **The sorting rules** (task E15.3) live here too: in the order the person gives them
//! (`position`, 0 first, no gaps), checked before they are kept ([`checked_rule`]). "Always for
//! this sender?" keeps one with a single condition on the sender and the mail account it was made
//! in ([`keep_sender`]); `inbox.list_rules` lists those as it always did ([`kept_rules`]).

use rusqlite::{Connection, OptionalExtension, params};
use torro_inbox::item::{SORT_CONDITIONS_MAX, SORT_RULES_MAX, SORT_VALUE_MAX};
use torro_inbox::{Rule, RuleEffect, SortCondition, SortField, SortRule, SourceRef};
use torro_mail::category::{DESCRIPTION_MAX, MODEL_PER_GROUP_MAX, NAME_MAX};
use torro_mail::{Category, CategoryDef, CategoryGroup, CategoryGroupDef, CategoryIcon, CustomCategory};

use super::{Failure, Scope, StoreError, read_items, stamp, text, write_item};
use chrono::{DateTime, Utc};
use torro_core::i18n::{tr, trf};

/// Main categories, including empty ones; each has a stable key independent of its name.
pub(super) fn groups_in(db: &Connection) -> Result<Vec<CategoryGroupDef>, Failure> {
    let mut statement = db.prepare("SELECT id, name, icon, color FROM category_groups ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
    })?;
    let mut groups = Vec::new();
    for row in rows {
        let (id, name, icon, color) = row?;
        let Some(group) = u32::try_from(id).ok().and_then(CategoryGroup::from_database_id) else { continue };
        let color = u8::try_from(color).ok().filter(|color| *color < 5).unwrap_or(0);
        groups.push(CategoryGroupDef { group, name, icon: CategoryIcon::from_key(&icon), color });
    }
    Ok(groups)
}

fn find_group(db: &Connection, group: CategoryGroup) -> Result<CategoryGroupDef, Failure> {
    groups_in(db)?
        .into_iter()
        .find(|held| held.group == group)
        .ok_or_else(|| Failure::NotFound(trf("There is no main category {group}.", &[("group", &group.key())])))
}

fn checked_group_name(db: &Connection, group: Option<CategoryGroup>, name: &str) -> Result<String, Failure> {
    let name = checked_name(name)?;
    let taken = groups_in(db)?.into_iter().any(|held| {
        Some(held.group) != group
            && held
                .name
                .as_deref()
                .unwrap_or_else(|| torro_core::category::group_label(held.group))
                .eq_ignore_ascii_case(&name)
    });
    if taken {
        return Err(Failure::Invalid(trf("There is a main category called “{name}” already.", &[("name", &name)])));
    }
    Ok(name)
}

pub(super) fn create_group(
    db: &Connection,
    name: &str,
    icon: CategoryIcon,
    color: u8,
) -> Result<CategoryGroupDef, Failure> {
    if color >= 5 {
        return Err(Failure::Invalid(tr("Choose a category colour.").into()));
    }
    if groups_in(db)?.len() >= 20 {
        return Err(Failure::Invalid(tr("There can be at most 20 main categories.").into()));
    }
    let name = checked_group_name(db, None, name)?;
    db.execute(
        "INSERT INTO category_groups (name, icon, color) VALUES (?1, ?2, ?3)",
        params![name, icon.key(), color],
    )?;
    let id = u32::try_from(db.last_insert_rowid()).map_err(|_| Failure::Invalid("no more main category IDs".into()))?;
    let group =
        CategoryGroup::from_database_id(id).ok_or_else(|| Failure::Invalid("invalid main category ID".into()))?;
    Ok(CategoryGroupDef { group, name: Some(name), icon, color })
}

pub(super) fn update_group(
    db: &Connection,
    group: CategoryGroup,
    name: &str,
    icon: CategoryIcon,
    color: u8,
) -> Result<CategoryGroupDef, Failure> {
    find_group(db, group)?;
    if color >= 5 {
        return Err(Failure::Invalid(tr("Choose a category colour.").into()));
    }
    let name = if name.trim().is_empty() && !matches!(group, CategoryGroup::Custom(_)) {
        None
    } else {
        Some(checked_group_name(db, Some(group), name)?)
    };
    db.execute(
        "UPDATE category_groups SET name = ?2, icon = ?3, color = ?4 WHERE id = ?1",
        params![group.database_id(), name, icon.key(), color],
    )?;
    Ok(CategoryGroupDef { group, name, icon, color })
}

/// Every category of the catalogue, by group in the order of [`CategoryGroup::ALL`]; within a
/// group the suite's first in their order, then the person's own in the order they were made. A
/// category being removed is left out.
pub(super) fn catalogue_in(db: &Connection) -> Result<Vec<CategoryDef>, Failure> {
    let mut defs = CategoryDef::defaults();
    let mut edits = db.prepare("SELECT category, grp, name, description, model, icon FROM category_edits")?;
    let rows = edits.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<bool>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for row in rows {
        let (key, group, name, description, model, icon) = row?;
        // A key this version does not know (a newer one wrote it) changes nothing.
        let Some(def) = defs.iter_mut().find(|def| def.category.key() == key) else { continue };
        if let Some(group) = group.as_deref().and_then(CategoryGroup::from_key) {
            def.group = group;
        }
        def.name = name;
        def.description = description;
        if let Some(model) = model {
            def.model = model;
        }
        if let Some(icon) = icon {
            def.icon = torro_mail::CategoryIcon::from_key(&icon);
        }
    }
    let mut own = db.prepare(
        "SELECT id, grp, name, description, model, icon FROM custom_categories WHERE replaced_by IS NULL ORDER BY id",
    )?;
    let rows = own.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (id, group, name, description, model, icon) = row?;
        let category = u32::try_from(id).ok().and_then(CustomCategory::new).map(Category::Custom);
        let (Some(category), Some(group)) = (category, CategoryGroup::from_key(&group)) else { continue };
        defs.push(CategoryDef {
            category,
            group,
            name: Some(name),
            description: Some(description),
            model,
            icon: torro_mail::CategoryIcon::from_key(&icon),
        });
    }
    // By group, as the sidebar lists them; within a group the suite's order, then the person's.
    defs.sort_by_key(|def| def.group.database_id());
    Ok(defs)
}

/// The removals cut short: each category with the one its mail and entries go to.
pub(super) fn pending(db: &Connection) -> Result<Vec<(Category, Category)>, Failure> {
    let mut statement =
        db.prepare("SELECT id, replaced_by FROM custom_categories WHERE replaced_by IS NOT NULL ORDER BY id")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let mut found = Vec::new();
    for row in rows {
        let (id, replacement) = row?;
        let category = u32::try_from(id).ok().and_then(CustomCategory::new).map(Category::Custom);
        if let (Some(category), Some(replacement)) = (category, Category::from_key(&replacement)) {
            found.push((category, replacement));
        }
    }
    Ok(found)
}

/// The entry of `category` in the catalogue; `not-found` when there is none.
pub(super) fn find(catalogue: &[CategoryDef], category: Category) -> Result<&CategoryDef, Failure> {
    catalogue.iter().find(|def| def.category == category).ok_or_else(|| {
        Failure::NotFound(trf("There is no mail category {category}.", &[("category", &category.key())]))
    })
}

/// A new category of the person's own (`inbox.create_category`).
pub(super) fn create(
    db: &Connection,
    group: CategoryGroup,
    name: &str,
    description: &str,
    model: bool,
    icon: torro_mail::CategoryIcon,
    now: DateTime<Utc>,
) -> Result<CategoryDef, Failure> {
    find_group(db, group)?;
    let name = checked_name(name)?;
    let description = checked_description(description)?
        .ok_or_else(|| Failure::Invalid(tr("Describe what belongs in the category.").into()))?;
    let mut catalogue = catalogue_in(db)?;
    // Checked as it would stand, with a key no category has yet.
    let unborn = CustomCategory::new(u32::MAX).map_or(Category::Other, Category::Custom);
    catalogue.push(CategoryDef {
        category: unborn,
        group,
        name: Some(name.clone()),
        description: Some(description.clone()),
        model,
        icon,
    });
    check(&catalogue, unborn, true)?;
    db.execute(
        "INSERT INTO custom_categories (grp, name, description, model, created, icon) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![group.key(), name, description, model, stamp(now), icon.key()],
    )?;
    let id = db.last_insert_rowid();
    let category = u32::try_from(id)
        .ok()
        .and_then(CustomCategory::new)
        .map(Category::Custom)
        .ok_or_else(|| Failure::Invalid("no more numbers for categories".into()))?;
    Ok(CategoryDef { category, group, name: Some(name), description: Some(description), model, icon })
}

/// What `inbox.update_category` changes; `None` leaves a field as it is.
pub(super) struct Changes {
    pub group: Option<CategoryGroup>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<bool>,
    pub icon: Option<torro_mail::CategoryIcon>,
}

/// Changes `category` (`inbox.update_category`): only the fields given. An empty name or
/// description puts back the suite's words for one of its categories; one of the person's own
/// always has both.
pub(super) fn update(db: &Connection, category: Category, changes: Changes) -> Result<CategoryDef, Failure> {
    let mut catalogue = catalogue_in(db)?;
    let mut def = find(&catalogue, category)?.clone();
    if let Some(group) = changes.group {
        find_group(db, group)?;
        def.group = group;
    }
    if let Some(model) = changes.model {
        def.model = model;
    }
    if let Some(icon) = changes.icon {
        def.icon = icon;
    }
    if let Some(name) = changes.name {
        def.name = match (name.trim().is_empty(), category.is_built_in()) {
            (true, true) => None,
            (true, false) => return Err(Failure::Invalid(tr("Give the category a name.").into())),
            (false, _) => Some(checked_name(&name)?),
        };
    }
    if let Some(description) = changes.description {
        def.description = match (checked_description(&description)?, category.is_built_in()) {
            (None, true) => None,
            (None, false) => return Err(Failure::Invalid(tr("Describe what belongs in the category.").into())),
            (Some(description), _) => Some(description),
        };
    }
    let renamed = def.name != find(&catalogue, category)?.name && def.name.is_some();
    if let Some(held) = catalogue.iter_mut().find(|held| held.category == category) {
        *held = def.clone();
    }
    check(&catalogue, category, renamed)?;
    if category.is_built_in() {
        // Only what differs from the suite is kept, so a category put back reads as new.
        let suite = CategoryDef::built_in(category);
        let differs = |field: bool| field.then_some(());
        let group = differs(suite.as_ref().is_none_or(|held| held.group != def.group)).map(|()| def.group.key());
        let model = differs(suite.as_ref().is_none_or(|held| held.model != def.model)).map(|()| def.model);
        let icon = differs(suite.as_ref().is_none_or(|held| held.icon != def.icon)).map(|()| def.icon.key());
        if group.is_none() && model.is_none() && icon.is_none() && def.name.is_none() && def.description.is_none() {
            db.execute("DELETE FROM category_edits WHERE category = ?1", [category.key()])?;
        } else {
            db.execute(
                "INSERT INTO category_edits (category, grp, name, description, model, icon) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (category) DO UPDATE SET grp = ?2, name = ?3, description = ?4, model = ?5, icon = ?6",
                params![category.key(), group, def.name, def.description, model, icon],
            )?;
        }
    } else {
        let Category::Custom(custom) = category else { return Ok(def) };
        db.execute(
            "UPDATE custom_categories SET grp = ?2, name = ?3, description = ?4, model = ?5, icon = ?6 WHERE id = ?1",
            params![i64::from(custom.number()), def.group.key(), def.name, def.description, def.model, def.icon.key()],
        )?;
    }
    Ok(def)
}

/// The catalogue as it would stand with `changed` made or changed is one a person can read and a
/// model can be asked about: `changed` reads like no other category, and its group offers a model
/// no more than it can choose between.
///
/// Only `changed` is weighed (correctness review of #255): the suite's words follow the core's
/// language, so a name of the person's own may read like one of the suite's after the language
/// changed – that must not block every other change of the catalogue.
///
/// The name is weighed only when `named` – the person gave it now. Changing only the group or the
/// model switch of a category whose name came to read like the suite's must still work.
pub(super) fn check(catalogue: &[CategoryDef], changed: Category, named: bool) -> Result<(), Failure> {
    let Some(def) = catalogue.iter().find(|def| def.category == changed) else { return Ok(()) };
    let name = torro_core::category::name_of(def);
    let taken = named
        && catalogue
            .iter()
            .filter(|other| other.category != changed)
            .any(|other| torro_core::category::name_of(other).to_lowercase() == name.to_lowercase());
    if taken {
        return Err(Failure::Invalid(trf("There is a category called “{name}” already.", &[("name", &name)])));
    }
    // "Other" is always offered: it is what "nothing fits" means (task E15.4).
    let offered = |held: &&CategoryDef| held.model || held.category == Category::Other;
    let for_model = catalogue.iter().filter(|held| held.group == def.group).filter(offered).count();
    if for_model > MODEL_PER_GROUP_MAX {
        return Err(Failure::Invalid(trf(
            "{group} would have {n} categories for the AI; at most {most} fit.",
            &[
                ("group", &torro_core::category::group_label(def.group)),
                ("n", &for_model.to_string()),
                ("most", &MODEL_PER_GROUP_MAX.to_string()),
            ],
        )));
    }
    Ok(())
}

/// A name as it is kept: trimmed, one line, at most [`NAME_MAX`] characters.
fn checked_name(name: &str) -> Result<String, Failure> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Failure::Invalid(tr("Give the category a name.").into()));
    }
    if name.chars().any(char::is_control) {
        return Err(Failure::Invalid(tr("A category's name is one line.").into()));
    }
    if name.chars().count() > NAME_MAX {
        let most = NAME_MAX.to_string();
        return Err(Failure::Invalid(trf("A category's name has at most {most} characters.", &[("most", &most)])));
    }
    Ok(name.to_string())
}

/// A description as it is kept: trimmed, lines allowed, no other control characters, at most
/// [`DESCRIPTION_MAX`] characters. `None` for an empty one.
fn checked_description(description: &str) -> Result<Option<String>, Failure> {
    let description = description.trim();
    if description.is_empty() {
        return Ok(None);
    }
    if description.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Failure::Invalid(tr("A description holds text and line breaks only.").into()));
    }
    if description.chars().count() > DESCRIPTION_MAX {
        let most = DESCRIPTION_MAX.to_string();
        return Err(Failure::Invalid(trf("A description has at most {most} characters.", &[("most", &most)])));
    }
    Ok(Some(description.to_string()))
}

/// What names a category here, in the inbox: its entries and its sorting rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Uses {
    pub entries: u64,
    pub rules: u64,
}

/// How many entries and sorting rules name `category`.
pub(super) fn uses(db: &Connection, category: Category) -> Result<Uses, Failure> {
    let entries = mail_entries_of(db, category)?.len() as u64;
    let rules: i64 =
        db.query_row("SELECT count(*) FROM sort_rules WHERE category = ?1", [category.key()], |row| row.get(0))?;
    Ok(Uses { entries, rules: rules.max(0) as u64 })
}

/// The entries of mail whose category is `category`, as stored. Read whole and filtered here:
/// the category lives in the entry's JSON, and this runs only when a category goes.
fn mail_entries_of(db: &Connection, category: Category) -> Result<Vec<torro_inbox::Item>, Failure> {
    let all = read_items(db, &Scope::All, "instr(source, ?) = 1", vec![text(torro_mail::ACCOUNT_PREFIX)], "")?;
    Ok(all
        .into_iter()
        .map(|stored| stored.item)
        .filter(|item| item.category.is_some_and(|held| held.category == category))
        .collect())
}

/// The first step of removing `category` (`inbox.remove_category`): the category notes
/// `replacement` and leaves the catalogue, and the sorting rules that name it name the
/// replacement now – one transaction. The mail and [`finish_removal`] follow.
pub(super) fn begin_removal(db: &mut Connection, category: Category, replacement: Category) -> Result<(), Failure> {
    let Category::Custom(custom) = category else {
        return Err(Failure::Unsupported(tr("The categories of the suite can be changed, not removed.").into()));
    };
    let tx = db.transaction()?;
    // Checked in the same transaction that notes the removal, so two removals at once cannot
    // chain (second correctness review of #255): no category hands its mail to this one, and the
    // replacement is not being removed itself.
    let taking: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM custom_categories WHERE replaced_by = ?1)",
        [category.key()],
        |row| row.get(0),
    )?;
    let going = match replacement {
        Category::Custom(other) => tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM custom_categories WHERE id = ?1 AND replaced_by IS NOT NULL)",
            [i64::from(other.number())],
            |row| row.get::<_, bool>(0),
        )?,
        _ => false,
    };
    if taking || going {
        let message = tr("This category is still taking the mail of a category removed. Try again in a moment.");
        return Err(Failure::Busy(message.into()));
    }
    tx.execute(
        "UPDATE custom_categories SET replaced_by = ?2 WHERE id = ?1",
        params![i64::from(custom.number()), replacement.key()],
    )?;
    let mut statement = tx.prepare("SELECT id, data FROM sort_rules WHERE category = ?1")?;
    let rows: Vec<(String, String)> =
        statement.query_map([category.key()], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    drop(statement);
    for (id, data) in rows {
        let mut rule: torro_inbox::SortRule = serde_json::from_str(&data)?;
        rule.category = replacement;
        tx.execute(
            "UPDATE sort_rules SET category = ?2, data = ?3 WHERE id = ?1",
            params![id, replacement.key(), serde_json::to_string(&rule)?],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The last step of removing `category`: its entries name `replacement`, keeping where their
/// category came from, and the category is gone for good. Nothing to do for one gone already.
pub(super) fn finish_removal(db: &mut Connection, category: Category, replacement: Category) -> Result<(), Failure> {
    let Category::Custom(custom) = category else { return Ok(()) };
    let tx = db.transaction()?;
    for mut item in mail_entries_of(&tx, category)? {
        if let Some(held) = item.category.as_mut() {
            held.category = replacement;
        }
        write_item(&tx, &item)?;
    }
    tx.execute("DELETE FROM custom_categories WHERE id = ?1", [i64::from(custom.number())])?;
    tx.commit()?;
    Ok(())
}

/// Removes `category` at once: nothing names it, so nothing needs to move.
pub(super) fn remove_unused(db: &Connection, category: Category) -> Result<(), Failure> {
    let Category::Custom(custom) = category else {
        return Err(Failure::Unsupported(tr("The categories of the suite can be changed, not removed.").into()));
    };
    db.execute("DELETE FROM custom_categories WHERE id = ?1", [i64::from(custom.number())])?;
    Ok(())
}

/// The replacement a removal cut short noted for `category`, if one stands.
pub(super) fn replacement_of(db: &Connection, category: Category) -> Result<Option<Category>, Failure> {
    let Category::Custom(custom) = category else { return Ok(None) };
    let noted: Option<Option<String>> = db
        .query_row("SELECT replaced_by FROM custom_categories WHERE id = ?1", [i64::from(custom.number())], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(noted.flatten().as_deref().and_then(Category::from_key))
}

// ----- sorting rules (task E15.3) -----

/// Every sorting rule in its order, first first.
pub(super) fn sort_rules_in(db: &Connection) -> Result<Vec<SortRule>, Failure> {
    let mut statement = db.prepare("SELECT data FROM sort_rules ORDER BY position, id")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut rules = Vec::new();
    for data in rows {
        rules.push(serde_json::from_str(&data?)?);
    }
    Ok(rules)
}

/// The sorting rule `id`, as stored; `None` without one.
pub(super) fn sort_rule_in(db: &Connection, id: &str) -> Result<Option<SortRule>, Failure> {
    let data: Option<String> =
        db.query_row("SELECT data FROM sort_rules WHERE id = ?1", [id], |row| row.get(0)).optional()?;
    Ok(data.map(|data| serde_json::from_str(&data)).transpose()?)
}

/// `inbox.list_sort_rules`: the rules in their order.
pub(super) fn list_rules(db: &Connection) -> Result<Vec<SortRule>, Failure> {
    sort_rules_in(db)
}

/// What `inbox.create_sort_rule` and `inbox.update_sort_rule` ask for.
pub(super) struct RuleWords {
    pub category: Category,
    pub conditions: Vec<SortCondition>,
    pub exceptions: Vec<SortCondition>,
    pub enabled: bool,
}

/// Adds a sorting rule at `position` (left out: last) and gives it back with its place.
pub(super) fn create_rule(
    db: &mut Connection,
    words: RuleWords,
    position: Option<u32>,
    now: DateTime<Utc>,
) -> Result<(SortRule, u32), Failure> {
    let (conditions, exceptions) = checked_rule(db, &words)?;
    let tx = db.transaction()?;
    let mut order = order_of(&tx)?;
    if order.len() >= SORT_RULES_MAX {
        let most = SORT_RULES_MAX.to_string();
        return Err(Failure::Invalid(trf(
            "There are {most} sorting rules already; remove one first.",
            &[("most", &most)],
        )));
    }
    let rule = SortRule {
        id: super::new_id()?,
        category: words.category,
        conditions,
        exceptions,
        enabled: words.enabled,
        created: now,
    };
    tx.execute(
        "INSERT INTO sort_rules (id, position, category, source, data) VALUES (?1, ?2, ?3, NULL, ?4)",
        params![rule.id, order.len() as i64, rule.category.key(), serde_json::to_string(&rule)?],
    )?;
    let at = position.map_or(order.len(), |wanted| (wanted as usize).min(order.len()));
    order.insert(at, rule.id.clone());
    renumber(&tx, &order)?;
    tx.commit()?;
    Ok((rule, at as u32))
}

/// Changes the sorting rule `id`: all of it but its id, its place and when it was made.
pub(super) fn update_rule(db: &mut Connection, id: &str, words: RuleWords) -> Result<(SortRule, u32), Failure> {
    let (conditions, exceptions) = checked_rule(db, &words)?;
    let mut rule = sort_rule_in(db, id)?.ok_or_else(|| no_rule(id))?;
    rule.category = words.category;
    rule.conditions = conditions;
    rule.exceptions = exceptions;
    rule.enabled = words.enabled;
    db.execute(
        "UPDATE sort_rules SET category = ?2, data = ?3 WHERE id = ?1",
        params![id, rule.category.key(), serde_json::to_string(&rule)?],
    )?;
    let at = order_of(db)?.iter().position(|held| held == id).unwrap_or_default();
    Ok((rule, at as u32))
}

/// Removes the sorting rule `id`; `not-found` without one. The others close the gap.
pub(super) fn remove_rule(db: &mut Connection, id: &str) -> Result<(), Failure> {
    let tx = db.transaction()?;
    if tx.execute("DELETE FROM sort_rules WHERE id = ?1", [id])? == 0 {
        return Err(no_rule(id));
    }
    let order = order_of(&tx)?;
    renumber(&tx, &order)?;
    tx.commit()?;
    Ok(())
}

/// Moves the sorting rule `id` to `position` (past the end: last); the others keep their order
/// around it.
pub(super) fn move_rule(db: &mut Connection, id: &str, position: u32) -> Result<(), Failure> {
    let tx = db.transaction()?;
    let mut order = order_of(&tx)?;
    let from = order.iter().position(|held| held == id).ok_or_else(|| no_rule(id))?;
    let moved = order.remove(from);
    let at = (position as usize).min(order.len());
    order.insert(at, moved);
    renumber(&tx, &order)?;
    tx.commit()?;
    Ok(())
}

/// "Always for this sender?" (`inbox.set_item_category` with `remember`): the sorting rule that
/// files `sender`'s mail as `category`, with the mail account it was made in. One sender has one:
/// the same again is the rule there (`false`); another category replaces it (`true`). A new one
/// goes first, since the person just decided about this very sender.
///
/// Only rules this very path made count (`source`), and only of mail accounts `sees` lets
/// through (correctness review of #255): a rule of the editor with the same sender stays the
/// editor's, and a client never replaces a rule of an account it may not read.
pub(super) fn keep_sender(
    db: &Connection,
    sender: &str,
    category: Category,
    source: &SourceRef,
    now: DateTime<Utc>,
    sees: impl Fn(&str) -> bool,
) -> Result<(SortRule, bool), Failure> {
    let mut order = order_of(db)?;
    let mut kept: Vec<SortRule> = Vec::new();
    let mut statement = db.prepare("SELECT source, data FROM sort_rules WHERE source IS NOT NULL")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (made_in, data) = row?;
        let rule: SortRule = serde_json::from_str(&data)?;
        if sees(&made_in) && only_sender(&rule).as_deref() == Some(sender) {
            kept.push(rule);
        }
    }
    drop(statement);
    if let Some(same) = kept.iter().find(|rule| rule.category == category && rule.enabled) {
        return Ok((same.clone(), false));
    }
    if order.len() - kept.len() >= SORT_RULES_MAX {
        let most = SORT_RULES_MAX.to_string();
        return Err(Failure::Invalid(trf(
            "There are {most} sorting rules already; remove one first.",
            &[("most", &most)],
        )));
    }
    for old in &kept {
        db.execute("DELETE FROM sort_rules WHERE id = ?1", [&old.id])?;
        order.retain(|held| *held != old.id);
    }
    let rule = SortRule {
        id: super::new_id()?,
        category,
        conditions: vec![SortCondition {
            comparison: torro_inbox::SortComparison::Auto,
            field: SortField::Sender,
            value: sender.to_string(),
        }],
        exceptions: Vec::new(),
        enabled: true,
        created: now,
    };
    db.execute(
        "INSERT INTO sort_rules (id, position, category, source, data) VALUES (?1, 0, ?2, ?3, ?4)",
        params![rule.id, rule.category.key(), source.key(), serde_json::to_string(&rule)?],
    )?;
    order.insert(0, rule.id.clone());
    renumber(db, &order)?;
    Ok((rule, true))
}

/// The sender a rule of "Always for this sender?" is about: exactly one condition on the sender
/// and no exception. `None` for every other rule.
fn only_sender(rule: &SortRule) -> Option<String> {
    match (rule.conditions.as_slice(), rule.exceptions.is_empty()) {
        ([condition], true) if condition.field == SortField::Sender => Some(condition.value.clone()),
        _ => None,
    }
}

/// The sorting rules "Always for this sender?" made, as `inbox.list_rules` has always listed them
/// (a [`Rule`] with [`RuleEffect::Category`]): those that still are one sender or one domain, on,
/// without exception. Changed in the editor into more, a rule is only the editor's.
pub(super) fn kept_rules(db: &Connection) -> Result<Vec<Rule>, Failure> {
    let mut statement = db.prepare("SELECT source, data FROM sort_rules WHERE source IS NOT NULL ORDER BY position")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut kept = Vec::new();
    for row in rows {
        let (source, data) = row?;
        let rule: SortRule = serde_json::from_str(&data)?;
        if let Some(view) = as_rule(&rule, &source) {
            kept.push(view);
        }
    }
    Ok(kept)
}

/// `rule` as a [`Rule`] of the mail account `source`, if it is one sender or one domain, on and
/// without exception.
fn as_rule(rule: &SortRule, source: &str) -> Option<Rule> {
    if !rule.enabled || !rule.exceptions.is_empty() {
        return None;
    }
    let [condition] = rule.conditions.as_slice() else { return None };
    if !matches!(condition.comparison, torro_inbox::SortComparison::Auto | torro_inbox::SortComparison::Is) {
        return None;
    }
    let (sender, domain) = match condition.field {
        SortField::Sender => (Some(condition.value.clone()), None),
        SortField::Domain => (None, Some(condition.value.clone())),
        _ => return None,
    };
    Some(Rule {
        id: rule.id.clone(),
        source: source.parse().ok()?,
        sender,
        domain,
        effect: RuleEffect::Category { category: rule.category },
        created: rule.created,
        facet: None,
        until: None,
    })
}

/// The ids of the rules in their order.
fn order_of(db: &Connection) -> Result<Vec<String>, Failure> {
    let mut statement = db.prepare("SELECT id FROM sort_rules ORDER BY position, id")?;
    let ids = statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Gives the rules of `order` the places 0, 1, 2 …: no gaps, no two in one place.
fn renumber(db: &Connection, order: &[String]) -> Result<(), Failure> {
    for (position, id) in order.iter().enumerate() {
        db.execute("UPDATE sort_rules SET position = ?2 WHERE id = ?1", params![id, position as i64])?;
    }
    Ok(())
}

/// The mail account "Always for this sender?" made the rule `id` in; `None` for a rule of the
/// editor and one that is not there.
pub(super) fn source_of(db: &Connection, id: &str) -> Result<Option<String>, Failure> {
    let source: Option<Option<String>> =
        db.query_row("SELECT source FROM sort_rules WHERE id = ?1", [id], |row| row.get(0)).optional()?;
    Ok(source.flatten())
}

pub(super) fn no_rule(id: &str) -> Failure {
    Failure::NotFound(trf("There is no sorting rule {id}.", &[("id", &id)]))
}

/// The conditions and exceptions of `words` as they are kept, after checking them: at least one
/// condition, at most [`SORT_CONDITIONS_MAX`] of each, every value one line of at most
/// [`SORT_VALUE_MAX`] characters, and a category of the catalogue. A sender and a domain are kept
/// in small letters, without `@` and final dot for a domain; words of the subject and text as
/// typed.
fn checked_rule(db: &Connection, words: &RuleWords) -> Result<(Vec<SortCondition>, Vec<SortCondition>), Failure> {
    find(&catalogue_in(db)?, words.category)?;
    if words.conditions.is_empty() {
        return Err(Failure::Invalid(tr("A sorting rule needs at least one condition.").into()));
    }
    if words.conditions.len() > SORT_CONDITIONS_MAX || words.exceptions.len() > SORT_CONDITIONS_MAX {
        let most = SORT_CONDITIONS_MAX.to_string();
        return Err(Failure::Invalid(trf(
            "A sorting rule has at most {most} conditions and {most} exceptions.",
            &[("most", &most)],
        )));
    }
    let kept = |conditions: &[SortCondition]| -> Result<Vec<SortCondition>, Failure> {
        conditions
            .iter()
            .map(|condition| {
                let comparison = if condition.comparison == torro_inbox::SortComparison::Auto {
                    condition.field.default_comparison()
                } else {
                    condition.comparison
                };
                if !condition.field.allows(comparison) {
                    return Err(Failure::Invalid(tr("This comparison does not fit the selected mail field.").into()));
                }
                let value = match condition.field {
                    SortField::Sender | SortField::Domain => {
                        torro_inbox::rules::sort_value(condition.field, &condition.value)
                    }
                    SortField::Attachment => String::new(),
                    _ => condition.value.trim().to_string(),
                };
                if value.is_empty() && condition.field != SortField::Attachment {
                    return Err(Failure::Invalid(tr("A condition needs a value.").into()));
                }
                if condition.field == SortField::Size
                    && value
                        .parse::<f64>()
                        .ok()
                        .is_none_or(|mb| !mb.is_finite() || mb < 0.0 || mb * 1_000_000.0 > u64::MAX as f64)
                {
                    return Err(Failure::Invalid(tr("Enter the message size in megabytes.").into()));
                }
                if value.chars().any(char::is_control) {
                    return Err(Failure::Invalid(tr("A condition's value is one line.").into()));
                }
                if value.chars().count() > SORT_VALUE_MAX {
                    let most = SORT_VALUE_MAX.to_string();
                    return Err(Failure::Invalid(trf(
                        "A condition's value has at most {most} characters.",
                        &[("most", &most)],
                    )));
                }
                Ok(SortCondition { field: condition.field, comparison: condition.comparison, value })
            })
            .collect()
    };
    Ok((kept(&words.conditions)?, kept(&words.exceptions)?))
}

/// The migration to schema 11 (task E15.3): every rule "Always for this sender?" kept before –
/// `rules` with effect `category`, a sender or a domain – becomes a sorting rule of the account it
/// was made in, in the order they were made, and leaves `rules`.
pub(super) fn move_kept_rules(db: &Connection) -> Result<(), StoreError> {
    let mut statement = db.prepare("SELECT data FROM rules WHERE effect = 'category'")?;
    let mut kept: Vec<Rule> = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|data| Ok(serde_json::from_str::<Rule>(&data?)?))
        .collect::<Result<_, StoreError>>()?;
    drop(statement);
    kept.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
    let mut position = 0_i64;
    for rule in kept {
        let RuleEffect::Category { category } = rule.effect else { continue };
        let condition = match (&rule.sender, &rule.domain) {
            (Some(sender), _) => SortCondition {
                comparison: torro_inbox::SortComparison::Auto,
                field: SortField::Sender,
                value: sender.clone(),
            },
            (None, Some(domain)) => SortCondition {
                comparison: torro_inbox::SortComparison::Auto,
                field: SortField::Domain,
                value: domain.clone(),
            },
            (None, None) => continue,
        };
        let sorting = SortRule {
            id: rule.id.clone(),
            category,
            conditions: vec![condition],
            exceptions: Vec::new(),
            enabled: true,
            created: rule.created,
        };
        db.execute(
            "INSERT INTO sort_rules (id, position, category, source, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sorting.id, position, category.key(), rule.source.key(), serde_json::to_string(&sorting)?],
        )?;
        position += 1;
    }
    db.execute("DELETE FROM rules WHERE effect = 'category'", [])?;
    Ok(())
}
