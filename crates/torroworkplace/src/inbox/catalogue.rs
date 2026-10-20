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

use rusqlite::{Connection, OptionalExtension, params};
use torro_mail::category::{DESCRIPTION_MAX, MODEL_PER_GROUP_MAX, NAME_MAX};
use torro_mail::{Category, CategoryDef, CategoryGroup, CustomCategory};

use super::{Failure, Scope, read_items, stamp, text, write_item};
use chrono::{DateTime, Utc};
use torro_core::i18n::{tr, trf};

/// Every category of the catalogue, the suite's first in their order, then the person's own in
/// the order they were made. A category being removed is left out.
pub(super) fn catalogue_in(db: &Connection) -> Result<Vec<CategoryDef>, Failure> {
    let mut defs = CategoryDef::defaults();
    let mut edits = db.prepare("SELECT category, grp, name, description, model FROM category_edits")?;
    let rows = edits.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<bool>>(4)?,
        ))
    })?;
    for row in rows {
        let (key, group, name, description, model) = row?;
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
    }
    let mut own = db.prepare(
        "SELECT id, grp, name, description, model FROM custom_categories WHERE replaced_by IS NULL ORDER BY id",
    )?;
    let rows = own.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
        ))
    })?;
    for row in rows {
        let (id, group, name, description, model) = row?;
        let category = u32::try_from(id).ok().and_then(CustomCategory::new).map(Category::Custom);
        let (Some(category), Some(group)) = (category, CategoryGroup::from_key(&group)) else { continue };
        defs.push(CategoryDef { category, group, name: Some(name), description: Some(description), model });
    }
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
    now: DateTime<Utc>,
) -> Result<CategoryDef, Failure> {
    let name = checked_name(name)?;
    let description = checked_description(description)?
        .ok_or_else(|| Failure::Invalid(tr("Describe what belongs in the category.").into()))?;
    let mut catalogue = catalogue_in(db)?;
    // Checked as it would stand, with a key no category has yet.
    catalogue.push(CategoryDef {
        category: CustomCategory::new(u32::MAX).map_or(Category::Other, Category::Custom),
        group,
        name: Some(name.clone()),
        description: Some(description.clone()),
        model,
    });
    check(&catalogue)?;
    db.execute(
        "INSERT INTO custom_categories (grp, name, description, model, created) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![group.key(), name, description, model, stamp(now)],
    )?;
    let id = db.last_insert_rowid();
    let category = u32::try_from(id)
        .ok()
        .and_then(CustomCategory::new)
        .map(Category::Custom)
        .ok_or_else(|| Failure::Invalid("no more numbers for categories".into()))?;
    Ok(CategoryDef { category, group, name: Some(name), description: Some(description), model })
}

/// What `inbox.update_category` changes; `None` leaves a field as it is.
pub(super) struct Changes {
    pub group: Option<CategoryGroup>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<bool>,
}

/// Changes `category` (`inbox.update_category`): only the fields given. An empty name or
/// description puts back the suite's words for one of its categories; one of the person's own
/// always has both.
pub(super) fn update(db: &Connection, category: Category, changes: Changes) -> Result<CategoryDef, Failure> {
    let mut catalogue = catalogue_in(db)?;
    let mut def = find(&catalogue, category)?.clone();
    if let Some(group) = changes.group {
        def.group = group;
    }
    if let Some(model) = changes.model {
        def.model = model;
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
    if let Some(held) = catalogue.iter_mut().find(|held| held.category == category) {
        *held = def.clone();
    }
    check(&catalogue)?;
    if category.is_built_in() {
        // Only what differs from the suite is kept, so a category put back reads as new.
        let suite = CategoryDef::built_in(category);
        let differs = |field: bool| field.then_some(());
        let group = differs(suite.as_ref().is_none_or(|held| held.group != def.group)).map(|()| def.group.key());
        let model = differs(suite.as_ref().is_none_or(|held| held.model != def.model)).map(|()| def.model);
        if group.is_none() && model.is_none() && def.name.is_none() && def.description.is_none() {
            db.execute("DELETE FROM category_edits WHERE category = ?1", [category.key()])?;
        } else {
            db.execute(
                "INSERT INTO category_edits (category, grp, name, description, model) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (category) DO UPDATE SET grp = ?2, name = ?3, description = ?4, model = ?5",
                params![category.key(), group, def.name, def.description, model],
            )?;
        }
    } else {
        let Category::Custom(custom) = category else { return Ok(def) };
        db.execute(
            "UPDATE custom_categories SET grp = ?2, name = ?3, description = ?4, model = ?5 WHERE id = ?1",
            params![i64::from(custom.number()), def.group.key(), def.name, def.description, def.model],
        )?;
    }
    Ok(def)
}

/// The catalogue as it would stand is one a person can read and a model can be asked about:
/// no two categories read alike, and no group offers a model more than it can choose between.
pub(super) fn check(catalogue: &[CategoryDef]) -> Result<(), Failure> {
    let mut seen: Vec<String> = Vec::new();
    for def in catalogue {
        let name = torro_core::category::name_of(def);
        let folded = name.to_lowercase();
        if seen.contains(&folded) {
            return Err(Failure::Invalid(trf("There is a category called “{name}” already.", &[("name", &name)])));
        }
        seen.push(folded);
    }
    for group in CategoryGroup::ALL {
        let for_model = catalogue.iter().filter(|def| def.group == group && def.model).count();
        if for_model > MODEL_PER_GROUP_MAX {
            return Err(Failure::Invalid(trf(
                "{group} would have {n} categories for the AI; at most {most} fit.",
                &[
                    ("group", &torro_core::category::group_label(group)),
                    ("n", &for_model.to_string()),
                    ("most", &MODEL_PER_GROUP_MAX.to_string()),
                ],
            )));
        }
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
