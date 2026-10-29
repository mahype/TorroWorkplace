//! Mail filters in the core (issue #339): the filters in `<data>/mail/filters.json` ([`MailFilters`])
//! and the handlers of `mail.list_filters`, `mail.create_filter`, `mail.update_filter`,
//! `mail.remove_filter`, `mail.preview_filter` and `mail.apply_filter` ([`answer`]).
//!
//! **Filters** are a search of the mail search language (`super::query`) with what to do with the
//! mail it finds in the inbox folder ([`torro_mail::MailFilter`]). They live in their own file beside
//! the stores, like the save targets (`super::save`), so `settings.update_settings` never writes
//! them. The file holds the next number to give besides the list, so an id `f<n>` is given once
//! ([`torro_mail::filter::filter_id`]). A missing file holds none; one that cannot be read is
//! `error` without its path, and is left as it is.
//!
//! **Checks** ([`check`]): the search must read ([`super::query::parse`]), not be empty and not use
//! `in:` (a filter looks at the inbox only) or `filename:` (the names of attachments are not known
//! when mail arrives); at least one action; a folder only with an account, and one of that account
//! that is not its inbox ([`check_folder`]); a category of the catalogue; at most
//! [`torro_mail::FILTERS_MAX`] filters. A change checks only what it brings anew: the category and
//! the folder a filter has already pass as they are, even when the person removed the category or
//! the folder is gone from the server, so such a filter can still be switched off or changed.
//!
//! **Reach** ([`visible`], [`client_refusal`]): a client sees a filter of an account it reaches,
//! and one for every account only when it reaches every account; any other filter behaves as if it
//! were not there (`not-found`). Making a filter for every account without reaching every account is
//! `forbidden`, and so is a category from a client without admin access to `inbox` (the category
//! works like a sorting rule). The core checks this before anyone is asked to approve and again when
//! an approved call runs.
//!
//! **Finding mail** ([`preview`], [`apply`], [`matching`]): a [`SearchQuery`] over the inbox folder
//! (`in_inbox`) of every account the caller reaches and the filter names. `mail.apply_filter` takes
//! the 500 newest that still need an action per call ([`APPLY_MAX`], `more` for the rest) and runs
//! them through the changing mail commands (`super::write`) in pieces of [`MAX_IDS`]: read, then the
//! category ([`MailService::set_category`], never over one the person set by hand), then the move.
//! The trash is a move, never deleting for good.
//!
//! **New mail** ([`on_arrival`]): the mail feed hands over the mail that just arrived in an
//! account's inbox folder (not a folder's first sync, not a catch-up); the enabled filters of that
//! account run on it with the same steps, and a category one of them set goes on the inbox entry.
//!
//! **Approvals** ([`to_approve`]): what a person is shown comes from the stored filter and the mail
//! store (the account's address, the folder's name), never from the client's words but the new
//! search; an approval of a change, a removal or a run is pinned to the stored filter
//! ([`fingerprint`]), so a filter a person changed meanwhile ends it `conflict`.
//!
//! **Log.** Nothing here writes a search, a folder or a category anywhere; error messages hold no path.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_core::i18n::tr;
use torro_ipc::api::{Access, Area, Rights, Selection};
use torro_ipc::mail::{MAX_IDS, MoveRole, MoveTarget, limit_refusal};
use torro_ipc::{Request, Response};
use torro_mail::filter::{filter_id, filter_number};
use torro_mail::{
    Category, CategoryDef, CategorySource, FILTERS_MAX, FilterActions, FilterFile, Flag, MailCategory, MailFilter,
    MessageSummary, Role,
};

use super::query::{self, Field, Query, Test};
use super::service::{MailService, SharedStore};
use super::store::SearchQuery;
use crate::approvals::{FilterFacts, MailFacts};
use crate::pim::Reach;

/// The file of the filters in the mail data directory (`<data>/mail/`).
pub const FILE_NAME: &str = "filters.json";

/// The most mails `mail.apply_filter` changes in one call; `more` says there are others.
pub const APPLY_MAX: usize = 500;

/// How many mails `mail.preview_filter` shows, newest first.
pub const PREVIEW_SHOWN: usize = 5;

/// How many mails of a search are read at a time while looking for those that still need an action.
const PAGE: u32 = 500;

// ----- errors -----

/// Why a filter could not be stored, found or used; [`FilterError::response`] is the answer.
#[derive(Debug)]
pub enum FilterError {
    /// A value breaks a rule (`error`), with the message for people.
    Invalid(String),
    /// No filter with this id, or one the caller does not see (`not-found`).
    NotFound,
    /// The caller may not do this with a filter (`forbidden`), with the message for people.
    Forbidden(String),
    /// The file could not be read or written (`error`); the details go to the log.
    Io(io::Error),
    /// The file holds something this version does not read (`error`); it is left as it is.
    Damaged,
    /// There is no data directory (no home directory), so nothing can be stored.
    NoPlace,
}

impl FilterError {
    /// The answer for the caller: `error`, `not-found` or `forbidden`, never with a path.
    pub fn response(&self) -> Response {
        let unusable = || tr("The mail filters cannot be read or written.").to_string();
        let (code, message) = match self {
            FilterError::Invalid(message) => ("error", message.clone()),
            FilterError::NotFound => ("not-found", tr("No such mail filter.").to_string()),
            FilterError::Forbidden(message) => ("forbidden", message.clone()),
            FilterError::Io(err) => {
                eprintln!("torroworkplace: mail: cannot use the mail filters: {}", err.kind());
                ("error", unusable())
            }
            FilterError::Damaged => {
                eprintln!("torroworkplace: mail: the file of the mail filters cannot be read; it is left as it is");
                ("error", unusable())
            }
            FilterError::NoPlace => ("error", unusable()),
        };
        Response::Error { code: code.into(), message }
    }
}

fn invalid(message: &str) -> FilterError {
    FilterError::Invalid(message.to_string())
}

// ----- what a request sets -----

/// What a person sets for a filter, as `mail.create_filter` and `mail.update_filter` carry it.
/// `enabled: None` keeps the stored switch (a new filter is on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    pub query: String,
    pub account: Option<String>,
    pub file: Option<FilterFile>,
    pub mark_read: bool,
    pub category: Option<Category>,
    pub enabled: Option<bool>,
}

impl FilterSpec {
    /// The spec of `mail.create_filter` or `mail.update_filter`; `None` for every other request.
    pub fn of(request: &Request) -> Option<FilterSpec> {
        match request {
            Request::CreateMailFilter { query, account, file, mark_read, category, enabled } => Some(FilterSpec {
                query: query.clone(),
                account: account.clone(),
                file: file.clone(),
                mark_read: *mark_read,
                category: *category,
                enabled: Some(*enabled),
            }),
            Request::UpdateMailFilter { query, account, file, mark_read, category, enabled, .. } => Some(FilterSpec {
                query: query.clone(),
                account: account.clone(),
                file: file.clone(),
                mark_read: *mark_read,
                category: *category,
                enabled: *enabled,
            }),
            _ => None,
        }
    }

    /// The filter `id` made of this spec, created at `created`, its switch `enabled` unless the spec
    /// names one. The search is trimmed.
    fn into_filter(self, id: String, enabled: bool, created: DateTime<Utc>) -> MailFilter {
        MailFilter {
            id,
            query: self.query.trim().to_string(),
            account: self.account,
            file: self.file,
            mark_read: self.mark_read,
            category: self.category,
            enabled: self.enabled.unwrap_or(enabled),
            created,
        }
    }
}

// ----- the filters -----

/// The file as written: the filters and the number the next one gets.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredFilters {
    #[serde(default)]
    filters: Vec<MailFilter>,
    /// The number of the next filter's id (`f<next>`); never given twice.
    #[serde(default)]
    next: u32,
}

/// The mail filters of the suite in their file (`<data>/mail/filters.json`, 0600, written over a
/// temporary file and renamed). Every change reads, checks and writes under one lock.
#[derive(Debug)]
pub struct MailFilters {
    file: Option<PathBuf>,
    lock: Mutex<()>,
}

impl MailFilters {
    /// The filters in `file`; `None` without a data directory: then there are none and none can be
    /// added. Nothing is read or made before the first call.
    pub fn at(file: Option<PathBuf>) -> MailFilters {
        MailFilters { file, lock: Mutex::new(()) }
    }

    /// Every filter, in the order the person keeps them. A missing file holds none.
    pub fn list(&self) -> Result<Vec<MailFilter>, FilterError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(self.read()?.filters)
    }

    /// The filter `id`, `None` when there is none.
    pub fn get(&self, id: &str) -> Result<Option<MailFilter>, FilterError> {
        Ok(self.list()?.into_iter().find(|filter| filter.id == id))
    }

    /// The enabled filters that run on new mail of `account`, in their order
    /// ([`MailFilter::runs_in`]); for the core's handling of arriving mail ([`on_arrival`]).
    pub fn running_in(&self, account: &str) -> Result<Vec<MailFilter>, FilterError> {
        Ok(self.list()?.into_iter().filter(|filter| filter.runs_in(account)).collect())
    }

    /// Adds `spec` as a new filter at the end, with the next id `f<n>` ([`check`], the search
    /// trimmed). `Invalid` for a spec that breaks a rule or a filter past [`FILTERS_MAX`]. Whether its
    /// folder is one of its account is the caller's check ([`check_folder`]): it needs the store.
    pub fn create(
        &self,
        spec: FilterSpec,
        categories: &[Category],
        now: DateTime<Utc>,
    ) -> Result<MailFilter, FilterError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stored = self.read()?;
        if stored.filters.len() >= FILTERS_MAX {
            return Err(invalid(tr("There can be at most 200 mail filters.")));
        }
        let taken = stored.filters.iter().filter_map(|filter| filter_number(&filter.id)).max().unwrap_or(0);
        let number = stored.next.max(taken.saturating_add(1)).max(1);
        let filter = spec.into_filter(filter_id(number), true, now);
        check(&filter, categories)?;
        stored.filters.push(filter.clone());
        stored.next = number.saturating_add(1);
        self.write(&stored)?;
        Ok(filter)
    }

    /// Changes the filter `id` to `spec`; its id, place and age stay, and its switch unless `spec`
    /// names one. `NotFound` for an unknown id, `Invalid` as for [`MailFilters::create`] – but the
    /// category the filter has already passes even when it is no longer in `categories`
    /// ([`with_kept`]), so a filter whose category the person removed can still be switched off.
    pub fn update(&self, id: &str, spec: FilterSpec, categories: &[Category]) -> Result<MailFilter, FilterError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stored = self.read()?;
        let at = stored.filters.iter().position(|filter| filter.id == id).ok_or(FilterError::NotFound)?;
        let old = &stored.filters[at];
        let known = with_kept(categories, Some(old));
        let filter = spec.into_filter(old.id.clone(), old.enabled, old.created);
        check(&filter, &known)?;
        stored.filters[at] = filter.clone();
        self.write(&stored)?;
        Ok(filter)
    }

    /// Removes the filter `id` and returns the rest. `NotFound` for an unknown id.
    pub fn remove(&self, id: &str) -> Result<Vec<MailFilter>, FilterError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stored = self.read()?;
        let before = stored.filters.len();
        stored.filters.retain(|filter| filter.id != id);
        if stored.filters.len() == before {
            return Err(FilterError::NotFound);
        }
        self.write(&stored)?;
        Ok(stored.filters)
    }

    fn read(&self) -> Result<StoredFilters, FilterError> {
        let Some(file) = &self.file else { return Ok(StoredFilters::default()) };
        match fs::read(file) {
            Ok(bytes) => serde_json::from_slice::<StoredFilters>(&bytes).map_err(|_| FilterError::Damaged),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(StoredFilters::default()),
            Err(err) => Err(FilterError::Io(err)),
        }
    }

    fn write(&self, stored: &StoredFilters) -> Result<(), FilterError> {
        let Some(file) = &self.file else { return Err(FilterError::NoPlace) };
        if let Some(dir) = file.parent() {
            crate::clients::create_private_dir(dir).map_err(FilterError::Io)?;
        }
        let bytes = serde_json::to_vec_pretty(stored).map_err(|err| FilterError::Io(io::Error::other(err)))?;
        crate::clients::write_private(file, &bytes).map_err(|err| match err {
            crate::clients::ClientsError::Io { err, .. } => FilterError::Io(err),
            _ => FilterError::NoPlace,
        })
    }
}

/// The search of a filter, read: `Invalid` when it does not read, is empty, or uses `in:` or
/// `filename:` (see the module's documentation).
pub fn filter_query(text: &str) -> Result<Query, FilterError> {
    let parsed = query::parse(text).map_err(|err| FilterError::Invalid(err.message()))?;
    if text.trim().is_empty() || parsed.is_empty() {
        return Err(invalid(tr("A mail filter needs a search.")));
    }
    for term in parsed.terms() {
        match &term.test {
            Test::In(_) => return Err(invalid(tr("A mail filter looks at the inbox only; leave out in:."))),
            Test::Text { field: Field::Filename, .. } => {
                return Err(invalid(tr("A mail filter cannot search the names of attachments; leave out filename:.")));
            }
            _ => {}
        }
    }
    Ok(parsed)
}

/// Whether `filter` may be stored, without the folder's account ([`check_folder`]): its search
/// ([`filter_query`]), at least one action, a folder only with an account, a category of
/// `categories` (the catalogue).
pub fn check(filter: &MailFilter, categories: &[Category]) -> Result<(), FilterError> {
    filter_query(&filter.query)?;
    if !filter.has_action() {
        return Err(invalid(tr("A mail filter needs an action: move the mail, mark it as read or set a category.")));
    }
    if matches!(filter.file, Some(FilterFile::Folder { .. })) && filter.account.is_none() {
        return Err(invalid(tr("A mail filter that moves mail to a folder needs an account.")));
    }
    if filter.category.is_some_and(|category| !categories.contains(&category)) {
        return Err(invalid(tr("The category of the mail filter is not in the catalogue.")));
    }
    Ok(())
}

/// The catalogue `categories` with the category of `stored` (the filter as it is stored) added:
/// a change of a filter checks only a category it brings anew. Removing a category the person made
/// must not lock the filters that set it – the switch in the list sends the category it holds.
/// Filters skip a category that is gone when they run ([`category_due`], [`on_arrival`]).
fn with_kept(categories: &[Category], stored: Option<&MailFilter>) -> Vec<Category> {
    let mut known = categories.to_vec();
    known.extend(stored.and_then(|filter| filter.category).filter(|kept| !categories.contains(kept)));
    known
}

/// Whether `spec` moves its mail to the same folder of the same account as `stored`: then the
/// folder is not looked up again ([`spec_refusal`]). A folder gone from the server (renamed,
/// removed) must not lock the filter; when it runs, the move fails and is logged (`failed` for
/// `mail.apply_filter`), the mail stays.
fn same_folder(spec: &FilterSpec, stored: Option<&MailFilter>) -> bool {
    stored.is_some_and(|stored| {
        matches!(spec.file, Some(FilterFile::Folder { .. }))
            && stored.file == spec.file
            && stored.account == spec.account
    })
}

/// Whether the folder `spec` moves its mail to is a folder of its account other than the inbox
/// (`Invalid` otherwise). Nothing to check without a folder; without an account [`check`] refuses
/// it already.
pub async fn check_folder(mail: &MailService, spec: &FilterSpec) -> Result<(), FilterError> {
    let (Some(FilterFile::Folder { id }), Some(account)) = (&spec.file, &spec.account) else { return Ok(()) };
    let no_folder = || invalid(tr("The mail filter's account has no such folder."));
    let store = mail.store(account).await.map_err(|_| no_folder())?;
    let id = id.clone();
    let folder = store.run(move |store| store.folder(&id)).await.map_err(|_| no_folder())?.ok_or_else(no_folder)?;
    match folder.role {
        Some(Role::Inbox) => Err(invalid(tr("A mail filter cannot move mail into the inbox."))),
        _ => Ok(()),
    }
}

// ----- reach -----

/// Whether `reach` sees a filter for `account` (`None`: every account): the suite always; a client
/// with the account in its selection, and with no account only with every account.
pub fn visible(reach: Reach<'_>, account: Option<&str>) -> bool {
    match (reach, account) {
        (Reach::Suite, _) => true,
        (Reach::Client(rights), Some(account)) => rights.reaches_mail_account(account),
        (Reach::Client(rights), None) => rights.mail_accounts == Selection::All,
    }
}

/// Whether a client with `rights` may set a filter's category: admin access to `inbox`, as for a
/// sorting rule. Always for the suite.
fn sets_category(reach: Reach<'_>) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.area(Area::Inbox).allows(Access::Admin),
    }
}

/// Whether `request` changes or runs a mail filter, the requests [`client_refusal`] looks at.
pub fn changes_filters(request: &Request) -> bool {
    matches!(
        request,
        Request::CreateMailFilter { .. }
            | Request::UpdateMailFilter { .. }
            | Request::RemoveMailFilter { .. }
            | Request::ApplyMailFilter { .. }
    )
}

/// What the core checks of a client's filter request before anyone is asked to approve it, and
/// again when an approved one runs (`Core::handle_client`, `Core::run_approved`); `None` when it may
/// go on, and for every other request. The account a request names was checked before
/// (`not-found` outside the client's selection, `mail_out_of_reach`).
///
/// - the stored filter of `mail.update_filter`, `mail.remove_filter`, `mail.apply_filter` is there
///   and [`visible`]: else `not-found`;
/// - a new or changed filter for every account needs every account, and a category (also one that
///   `mail.apply_filter` would set) admin access to `inbox`: else `forbidden`;
/// - a new or changed filter passes [`check`] and [`check_folder`] (a change only with what it brings
///   anew, [`spec_refusal`]): else `error`, nothing to approve.
pub async fn client_refusal(
    mail: &MailService,
    rights: &Rights,
    request: &Request,
    categories: &[Category],
) -> Option<Response> {
    let reach = Reach::Client(rights);
    let stored = match request {
        Request::UpdateMailFilter { id, .. } | Request::RemoveMailFilter { id } | Request::ApplyMailFilter { id } => {
            match visible_filter(mail, reach, id) {
                Ok(filter) => Some(filter),
                Err(err) => return Some(err.response()),
            }
        }
        _ => None,
    };
    if let Some(stored) = &stored
        && matches!(request, Request::ApplyMailFilter { .. })
        && stored.category.is_some()
        && !sets_category(reach)
    {
        return Some(no_category().response());
    }
    let spec = FilterSpec::of(request)?;
    if let Err(err) = spec_refusal(mail, reach, &spec, stored.as_ref(), categories).await {
        return Some(err.response());
    }
    None
}

/// `forbidden`, `not-found` or `error` for a filter `spec` from `reach` (see [`client_refusal`]).
/// `stored` is the filter a change replaces (`None` for a new one): its category and folder pass
/// unchanged without being looked up again ([`with_kept`], [`same_folder`]); everything else is
/// checked as for a new filter.
async fn spec_refusal(
    mail: &MailService,
    reach: Reach<'_>,
    spec: &FilterSpec,
    stored: Option<&MailFilter>,
    categories: &[Category],
) -> Result<(), FilterError> {
    match &spec.account {
        Some(account) if !visible(reach, Some(account)) => return Err(FilterError::NotFound),
        Some(_) => {}
        None if !visible(reach, None) => {
            return Err(FilterError::Forbidden(
                tr("A mail filter for every account needs access to every mail account.").to_string(),
            ));
        }
        None => {}
    }
    if spec.category.is_some() && !sets_category(reach) {
        return Err(no_category());
    }
    let candidate = spec.clone().into_filter(filter_id(1), true, DateTime::<Utc>::UNIX_EPOCH);
    check(&candidate, &with_kept(categories, stored))?;
    if same_folder(spec, stored) {
        return Ok(());
    }
    check_folder(mail, spec).await
}

fn no_category() -> FilterError {
    FilterError::Forbidden(
        tr("Setting a category with a mail filter needs admin access to the Notification Hub.").into(),
    )
}

/// The stored filter `id` when `reach` sees it, else `NotFound`.
fn visible_filter(mail: &MailService, reach: Reach<'_>, id: &str) -> Result<MailFilter, FilterError> {
    match mail.filters().get(id)? {
        Some(filter) if visible(reach, filter.account.as_deref()) => Ok(filter),
        _ => Err(FilterError::NotFound),
    }
}

// ----- the requests -----

/// Answers a mail filter request for `reach` (the six requests of issue #339); any other request is
/// `unsupported`. The limits are checked first, also for the app key; for a client the reach is
/// checked again here ([`visible`], [`spec_refusal`]), so a filter it does not see is `not-found`
/// also when this runs later. `categories` is the catalogue a filter's category must be in.
pub async fn answer(mail: &MailService, reach: Reach<'_>, categories: &[Category], request: Request) -> Response {
    if let Some(refused) = limit_refusal(&request) {
        return refused;
    }
    answered(mail, reach, categories, request).await.unwrap_or_else(|err| err.response())
}

async fn answered(
    mail: &MailService,
    reach: Reach<'_>,
    categories: &[Category],
    request: Request,
) -> Result<Response, FilterError> {
    let filters = mail.filters();
    match request {
        Request::ListMailFilters => Ok(Response::MailFilters { filters: seen(filters.list()?, reach) }),
        request @ Request::CreateMailFilter { .. } => {
            let spec = FilterSpec::of(&request).ok_or(FilterError::NotFound)?;
            spec_refusal(mail, reach, &spec, None, categories).await?;
            Ok(Response::MailFilter { filter: filters.create(spec, categories, Utc::now())? })
        }
        request @ Request::UpdateMailFilter { .. } => {
            let Request::UpdateMailFilter { id, .. } = &request else { return Err(FilterError::NotFound) };
            let stored = visible_filter(mail, reach, id)?;
            let spec = FilterSpec::of(&request).ok_or(FilterError::NotFound)?;
            spec_refusal(mail, reach, &spec, Some(&stored), categories).await?;
            Ok(Response::MailFilter { filter: filters.update(id, spec, categories)? })
        }
        Request::RemoveMailFilter { id } => {
            visible_filter(mail, reach, &id)?;
            Ok(Response::MailFilters { filters: seen(filters.remove(&id)?, reach) })
        }
        Request::PreviewMailFilter { query, account } => {
            let (count, messages) = preview(mail, reach, &query, account.as_deref()).await?;
            Ok(Response::MailFilterPreview { count, messages })
        }
        Request::ApplyMailFilter { id } => {
            let filter = visible_filter(mail, reach, &id)?;
            if filter.category.is_some() && !sets_category(reach) {
                return Err(no_category());
            }
            let applied = apply(mail, reach, &filter, categories).await?;
            Ok(Response::MailFilterApplied { applied: applied.applied, failed: applied.failed, more: applied.more })
        }
        other => Err(FilterError::Invalid(format!("{} is not a command for mail filters", other.operation().name()))),
    }
}

/// The filters of `all` that `reach` sees, in their order.
fn seen(all: Vec<MailFilter>, reach: Reach<'_>) -> Vec<MailFilter> {
    all.into_iter().filter(|filter| visible(reach, filter.account.as_deref())).collect()
}

/// The stores the search of a filter for `account` (`None`: every account) runs in for `reach`.
async fn stores_for(mail: &MailService, reach: Reach<'_>, account: Option<&str>) -> Vec<(String, SharedStore)> {
    let mut stores = mail.stores(reach).await;
    if let Some(account) = account {
        stores.retain(|(id, _)| id == account);
    }
    stores
}

/// The search of `query` over the inbox folder.
fn in_inbox(query: Query) -> SearchQuery {
    SearchQuery { query, in_inbox: true, ..SearchQuery::default() }
}

/// `mail.preview_filter`: how many mails the search `query` finds in the inbox of the accounts
/// `reach` sees (only `account` when given) and the [`PREVIEW_SHOWN`] newest of them.
pub async fn preview(
    mail: &MailService,
    reach: Reach<'_>,
    query: &str,
    account: Option<&str>,
) -> Result<(u64, Vec<MessageSummary>), FilterError> {
    let search = in_inbox(filter_query(query)?);
    let (mut count, mut newest) = (0, Vec::new());
    for (_, store) in stores_for(mail, reach, account).await {
        let search = search.clone();
        let found =
            store.run(move |store| Ok((store.count(&search)?, store.search(&search, PREVIEW_SHOWN as u32, 0)?))).await;
        match found {
            Ok((found, messages)) => {
                count += found;
                newest.extend(messages);
            }
            Err(err) => eprintln!("torroworkplace: mail: a filter's preview cannot read a store: {err}"),
        }
    }
    newest.sort_by_key(|message| std::cmp::Reverse(message.date));
    newest.truncate(PREVIEW_SHOWN);
    Ok((count, newest))
}

/// What one run of `mail.apply_filter` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub applied: u64,
    pub failed: u64,
    pub more: bool,
}

/// Whether `filter` still has something to do with `message` (in the inbox): a move always; reading
/// an unread mail; its category (when still in `categories`) on a mail without it, unless the person
/// set one by hand (decision 4).
fn needs(filter: &MailFilter, message: &MessageSummary, categories: &[Category]) -> bool {
    filter.file.is_some()
        || (filter.mark_read && !message.flags.contains(&Flag::Seen))
        || filter.category.is_some_and(|category| category_due(message, category, categories))
}

/// The category `category` is still to be set on `message`: it is in the catalogue, and the mail has
/// another one or none, not one the person set by hand.
fn category_due(message: &MessageSummary, category: Category, categories: &[Category]) -> bool {
    categories.contains(&category)
        && match message.category {
            Some(kept) if kept.source == CategorySource::Person => false,
            Some(kept) => kept.category != category,
            None => true,
        }
}

/// Up to `max` mails of the inbox of `store` that `filter` finds and still has something to do with
/// ([`needs`]), newest first; `true` when there are more.
fn due_in(
    store: &super::store::MailStore,
    filter: &MailFilter,
    search: &SearchQuery,
    categories: &[Category],
    max: usize,
) -> Result<(Vec<MessageSummary>, bool), super::MailStoreError> {
    let mut due = Vec::new();
    let mut offset = 0;
    loop {
        let page = store.search(search, PAGE, offset)?;
        let read = page.len();
        for message in page {
            if needs(filter, &message, categories) {
                if due.len() == max {
                    return Ok((due, true));
                }
                due.push(message);
            }
        }
        if read < PAGE as usize {
            return Ok((due, false));
        }
        offset += PAGE;
    }
}

/// The mails `mail.apply_filter` would change now for `reach`: the [`APPLY_MAX`] newest of every
/// account it runs in that still need an action, and whether there are more.
pub async fn due(
    mail: &MailService,
    reach: Reach<'_>,
    filter: &MailFilter,
    categories: &[Category],
) -> Result<(Vec<MessageSummary>, bool), FilterError> {
    let mut search = in_inbox(filter_query(&filter.query)?);
    // Only reading: what is read already needs nothing.
    if filter.file.is_none() && filter.category.is_none() {
        search.unread = Some(true);
    }
    let (mut all, mut more) = (Vec::new(), false);
    for (_, store) in stores_for(mail, reach, filter.account.as_deref()).await {
        let (filter, search, categories) = (filter.clone(), search.clone(), categories.to_vec());
        let found = store.run(move |store| due_in(store, &filter, &search, &categories, APPLY_MAX)).await;
        match found {
            Ok((found, others)) => {
                more |= others;
                all.extend(found);
            }
            Err(err) => eprintln!("torroworkplace: mail: a filter cannot read a store: {err}"),
        }
    }
    all.sort_by_key(|message| std::cmp::Reverse(message.date));
    if all.len() > APPLY_MAX {
        all.truncate(APPLY_MAX);
        more = true;
    }
    Ok((all, more))
}

/// `mail.apply_filter`: carries out `filter`'s actions on the mails [`due`] finds for `reach`, account
/// by account, in pieces of [`MAX_IDS`]: read, then the category, then the move (the trash as a move,
/// never for good). The changes run as the suite's (they were approved); a piece that fails counts
/// its mails as `failed` and the rest goes on.
pub async fn apply(
    mail: &MailService,
    reach: Reach<'_>,
    filter: &MailFilter,
    categories: &[Category],
) -> Result<Applied, FilterError> {
    let (due, more) = due(mail, reach, filter, categories).await?;
    let actions = FilterActions::of([filter]);
    let mut failed: HashSet<String> = HashSet::new();
    let mut accounts: Vec<String> = due.iter().map(|message| message.account.clone()).collect();
    accounts.sort();
    accounts.dedup();
    for account in accounts {
        let mine: Vec<&MessageSummary> = due.iter().filter(|message| message.account == account).collect();
        failed.extend(carry_out(mail, &account, &actions, &mine, categories).await.failed);
    }
    let failed_count = failed.len() as u64;
    Ok(Applied { applied: due.len() as u64 - failed_count, failed: failed_count, more })
}

/// What [`carry_out`] did: the mails an action failed on, and those that got the category.
#[derive(Debug, Default)]
struct CarriedOut {
    failed: HashSet<String>,
    categorised: Vec<String>,
}

/// Carries out `actions` on `messages` (all of `account`, in its inbox), in pieces of [`MAX_IDS`]:
/// read (the unread ones), then the category ([`category_due`]: in `categories`, never over one the
/// person set by hand), then the move (the trash as a move, never for good). The changes run as the
/// suite's: a person approved them, or made the filter. A step that fails notes its mails as failed
/// (logged with a code, never a subject) and the next step goes on.
async fn carry_out(
    mail: &MailService,
    account: &str,
    actions: &FilterActions,
    messages: &[&MessageSummary],
    categories: &[Category],
) -> CarriedOut {
    let mut done = CarriedOut::default();
    if actions.mark_read {
        let unread = messages.iter().filter(|message| !message.flags.contains(&Flag::Seen));
        let ids: Vec<String> = unread.map(|message| message.id.clone()).collect();
        for piece in ids.chunks(MAX_IDS) {
            let answer = super::write::set_flags(mail, Reach::Suite, piece, &[Flag::Seen], &[]).await;
            note_failed(&mut done.failed, piece, &answer);
        }
    }
    if let Some(category) = actions.category {
        let ids: Vec<String> = messages
            .iter()
            .filter(|message| category_due(message, category, categories))
            .map(|message| message.id.clone())
            .collect();
        match mail.set_category(account, &ids, Some(MailCategory::by_rule(category))).await {
            Ok(()) => done.categorised = ids,
            Err(err) => {
                eprintln!("torroworkplace: mail: a filter cannot set a category: {}", err.code());
                done.failed.extend(ids);
            }
        }
    }
    if let Some(file) = &actions.file {
        let ids: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
        for piece in ids.chunks(MAX_IDS) {
            let answer = match file {
                FilterFile::Archive => {
                    super::write::move_messages(mail, Reach::Suite, piece, &MoveTarget::Role(MoveRole::Archive)).await
                }
                FilterFile::Folder { id } => {
                    super::write::move_messages(mail, Reach::Suite, piece, &MoveTarget::Folder(id.clone())).await
                }
                FilterFile::Trash => super::write::delete_messages(mail, Reach::Suite, piece, false).await,
            };
            note_failed(&mut done.failed, piece, &answer);
        }
    }
    done
}

/// Notes `piece` as failed unless `done` is `ok`; the error's code goes to the log, never a subject.
fn note_failed(failed: &mut HashSet<String>, piece: &[String], done: &Response) {
    if let Response::Error { code, .. } = done {
        eprintln!("torroworkplace: mail: a filter's action failed on {} mails: {code}", piece.len());
        failed.extend(piece.iter().cloned());
    }
}

// ----- new mail (issue #339, step 3) -----

/// Which of the messages `new` (ids `m:…` of `account`, in its inbox) the `filters` find, each with
/// what they do together ([`FilterActions::of`], in the order of `filters`); messages no filter
/// finds are left out. For the core's handling of arriving mail: pass
/// [`MailFilters::running_in`]. A filter whose search no longer reads is skipped (and logged by its
/// id). Nothing is changed here.
pub async fn matching(
    store: &SharedStore,
    filters: &[MailFilter],
    new: &[String],
) -> Result<Vec<(String, FilterActions)>, super::MailStoreError> {
    if filters.is_empty() || new.is_empty() {
        return Ok(Vec::new());
    }
    let mut searches = Vec::new();
    for filter in filters {
        match filter_query(&filter.query) {
            Ok(query) => searches.push((filter.clone(), query)),
            Err(_) => eprintln!("torroworkplace: mail: the search of mail filter {} does not read", filter.id),
        }
    }
    let new = new.to_vec();
    store
        .run(move |store| {
            let mut found: Vec<(String, Vec<&MailFilter>)> = Vec::new();
            for (filter, query) in &searches {
                let search = SearchQuery { only: Some(new.clone()), ..in_inbox(query.clone()) };
                for message in store.search(&search, new.len() as u32, 0)? {
                    match found.iter_mut().find(|(id, _)| *id == message.id) {
                        Some((_, filters)) => filters.push(filter),
                        None => found.push((message.id, vec![filter])),
                    }
                }
            }
            Ok(found.into_iter().map(|(id, filters)| (id, FilterActions::of(filters))).collect())
        })
        .await
}

/// Runs the enabled filters of `account` ([`MailFilters::running_in`]) on its new mail `new` (ids
/// `m:…` that just arrived in its inbox folder; the mail feed calls it, never on a folder's first
/// sync nor on a catch-up) and gives back the mails that got a filter's category, with it, so their
/// inbox entries carry it too (decision 4: before the sorting rules, without asking a model).
///
/// - Mail whose `Message-ID` the account holds in another folder already is left alone
///   ([`delivered`]): it was moved or copied into the inbox, not delivered, and a filter must not
///   take it out again.
/// - What several filters do together follows their order ([`matching`], [`FilterActions::of`],
///   decision 3); a category that is no longer in `categories` (the catalogue) is skipped.
/// - Read, then the category, then the move ([`carry_out`]). Mail read or moved by a filter gets no
///   entry: the feed files only what is still unread in the inbox folder, as the store says.
/// - A failure goes to the log (the account and a code, never a search or a subject); the mail
///   stays as it came, and gets an entry as any other.
pub async fn on_arrival(
    mail: &MailService,
    store: &SharedStore,
    account: &str,
    new: &[String],
    categories: &[Category],
) -> Vec<(String, MailCategory)> {
    let filters = match mail.filters().running_in(account) {
        Ok(filters) => filters,
        Err(_) => {
            eprintln!("torroworkplace: mail: the mail filters cannot be read; new mail of {account} is not filtered");
            return Vec::new();
        }
    };
    if filters.is_empty() || new.is_empty() {
        return Vec::new();
    }
    let ids = new.to_vec();
    let arrived = match store.run(move |store| delivered(store, &ids)).await {
        Ok(arrived) => arrived,
        Err(err) => {
            eprintln!("torroworkplace: mail: cannot read the new mail of {account} to filter it: {err}");
            return Vec::new();
        }
    };
    let arrived_ids: Vec<String> = arrived.iter().map(|message| message.id.clone()).collect();
    let found = match matching(store, &filters, &arrived_ids).await {
        Ok(found) => found,
        Err(err) => {
            eprintln!("torroworkplace: mail: cannot run the mail filters on new mail of {account}: {err}");
            return Vec::new();
        }
    };
    // Mails that get the same actions go together, so each action runs once per kind.
    let mut groups: Vec<(FilterActions, Vec<&MessageSummary>)> = Vec::new();
    for (id, mut actions) in found {
        actions.category = actions.category.filter(|category| categories.contains(category));
        let Some(message) = arrived.iter().find(|message| message.id == id) else { continue };
        if actions.is_empty() {
            continue;
        }
        match groups.iter_mut().find(|(same, _)| *same == actions) {
            Some((_, messages)) => messages.push(message),
            None => groups.push((actions, vec![message])),
        }
    }
    let mut categorised = Vec::new();
    for (actions, messages) in &groups {
        let done = carry_out(mail, account, actions, messages, categories).await;
        if let Some(category) = actions.category {
            categorised.extend(done.categorised.into_iter().map(|id| (id, MailCategory::by_rule(category))));
        }
    }
    categorised
}

/// Those of `ids` the store holds that were delivered rather than moved: no other folder of the
/// account holds a mail with the same `Message-ID` (a mail without one counts as delivered).
fn delivered(store: &super::store::MailStore, ids: &[String]) -> Result<Vec<MessageSummary>, super::MailStoreError> {
    let mut arrived = Vec::new();
    for id in ids {
        let Some(summary) = store.summary(id)? else { continue };
        if let Some(rfc) = store.resolve_message(id)?.and_then(|found| found.rfc_message_id)
            && store.messages_by_rfc_id(&rfc)?.iter().any(|copy| copy.folder != summary.folder)
        {
            continue;
        }
        arrived.push(summary);
    }
    Ok(arrived)
}

// ----- approvals -----

/// The fingerprint an approval of `mail.update_filter`, `mail.remove_filter` or `mail.apply_filter`
/// is pinned to: a hash of the filter as stored ([`crate::audit::hashed`] of its JSON). The approved
/// call runs only while the stored filter has the same one, else `conflict` (`Core::run_approved`).
pub fn fingerprint(filter: &MailFilter) -> String {
    crate::audit::hashed(&serde_json::to_string(filter).unwrap_or_default())
}

/// The fingerprint of the stored filter `id` now; `None` when there is none.
pub fn fingerprint_of(mail: &MailService, id: &str) -> Option<String> {
    mail.filters().get(id).ok().flatten().as_ref().map(fingerprint)
}

/// The fingerprint an approval of `mail.apply_filter` is pinned to, and how many mails the run
/// would change: the filter as stored and the mails [`due`] finds for `reach` now. The approval
/// names that number, so the approved run must change exactly those mails; new mail the filter
/// finds meanwhile (a switched-off filter keeps finding it) makes it `conflict` instead.
async fn apply_fingerprint(
    mail: &MailService,
    reach: Reach<'_>,
    filter: &MailFilter,
    categories: &[Category],
) -> (String, usize) {
    let mut ids: Vec<String> = match due(mail, reach, filter, categories).await {
        Ok((due, _)) => due.into_iter().map(|message| message.id).collect(),
        Err(_) => Vec::new(),
    };
    ids.sort();
    let pinned = crate::audit::hashed(&format!("{}\n{}", fingerprint(filter), ids.join("\n")));
    (pinned, ids.len())
}

/// The fingerprint `request` (`mail.update_filter`, `mail.remove_filter`, `mail.apply_filter`) has
/// for `reach` now, to compare with the approval's ([`to_approve`]); `None` for another request or
/// a filter that is not there.
pub async fn fingerprint_now(
    mail: &MailService,
    reach: Reach<'_>,
    request: &Request,
    categories: &[Category],
) -> Option<String> {
    match request {
        Request::UpdateMailFilter { id, .. } | Request::RemoveMailFilter { id } => fingerprint_of(mail, id),
        Request::ApplyMailFilter { id } => {
            let filter = mail.filters().get(id).ok().flatten()?;
            Some(apply_fingerprint(mail, reach, &filter, categories).await.0)
        }
        _ => None,
    }
}

/// What the approval of a filter request of `reach` shows ([`MailFacts`] for the account and folder
/// the request names and, for `mail.apply_filter`, how many mails it would change now;
/// [`FilterFacts`] for the stored filter), and the fingerprint it is pinned to (none for
/// `mail.create_filter`: there is nothing stored yet). Looked up in the core's file and stores, never
/// taken from the client's words but the new search. `categories` names the categories.
pub async fn to_approve(
    mail: &MailService,
    reach: Reach<'_>,
    request: &Request,
    categories: Vec<CategoryDef>,
) -> (MailFacts, FilterFacts, Option<String>) {
    let stored = match request {
        Request::UpdateMailFilter { id, .. } | Request::RemoveMailFilter { id } | Request::ApplyMailFilter { id } => {
            visible_filter(mail, reach, id).ok()
        }
        _ => None,
    };
    let mut pinned = stored.as_ref().map(fingerprint);
    let mut facts = MailFacts::default();
    if let Some(spec) = FilterSpec::of(request) {
        (facts.account, facts.folder) = names(mail, spec.account.as_deref(), spec.file.as_ref()).await;
    }
    if let (Request::ApplyMailFilter { .. }, Some(filter)) = (request, &stored) {
        let known: Vec<Category> = categories.iter().map(|def| def.category).collect();
        let (print, count) = apply_fingerprint(mail, reach, filter, &known).await;
        (pinned, facts.count) = (Some(print), count);
    }
    let (stored_account, stored_folder) = match &stored {
        Some(filter) => names(mail, filter.account.as_deref(), filter.file.as_ref()).await,
        None => (None, None),
    };
    (facts, FilterFacts { stored, stored_account, stored_folder, categories }, pinned)
}

/// The address of `account` and the name of the folder of `file`, as the core knows them.
async fn names(
    mail: &MailService,
    account: Option<&str>,
    file: Option<&FilterFile>,
) -> (Option<String>, Option<String>) {
    let Some(account) = account else { return (None, None) };
    let address = mail.config(account).await.ok().map(|config| config.address);
    let folder = match file {
        Some(FilterFile::Folder { id }) => match mail.store(account).await {
            Ok(store) => {
                let id = id.clone();
                store.run(move |store| store.folder(&id)).await.ok().flatten().map(|folder| folder.name)
            }
            Err(_) => None,
        },
        _ => None,
    };
    (address, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(query: &str) -> FilterSpec {
        FilterSpec {
            query: query.into(),
            account: None,
            file: Some(FilterFile::Archive),
            mark_read: false,
            category: None,
            enabled: None,
        }
    }

    fn at(dir: &tempfile::TempDir) -> MailFilters {
        MailFilters::at(Some(dir.path().join("mail").join(FILE_NAME)))
    }

    fn code(err: FilterError) -> String {
        match err.response() {
            Response::Error { code, .. } => code,
            other => panic!("an error, not {other:?}"),
        }
    }

    /// Filters are kept in their file in order, with ids given once: a removed id never comes back.
    #[test]
    fn filters_are_kept_in_order_with_ids_given_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let filters = at(&dir);
        assert!(filters.list().expect("an empty list").is_empty());
        let now = Utc::now();
        let first = filters.create(spec("  from:shop.example "), &[], now).expect("made");
        let second = filters.create(spec("from:news.example"), &[], now).expect("made");
        assert_eq!((first.id.as_str(), second.id.as_str()), ("f1", "f2"));
        assert_eq!(first.query, "from:shop.example", "trimmed");
        assert!(first.enabled);

        let off = FilterSpec { mark_read: true, enabled: Some(false), ..spec("from:shop.example") };
        let changed = filters.update("f1", off, &[]).expect("changed");
        assert!(!changed.enabled && changed.mark_read && changed.created == first.created);
        let kept = filters.update("f1", spec("from:shop.example"), &[]).expect("changed");
        assert!(!kept.enabled, "left out, the switch stays");
        assert_eq!(code(filters.update("f9", spec("x"), &[]).expect_err("unknown")), "not-found");

        assert_eq!(filters.remove("f2").expect("removed").len(), 1);
        assert_eq!(code(filters.remove("f2").expect_err("gone")), "not-found");
        let third = filters.create(spec("from:other.example"), &[], now).expect("made");
        assert_eq!(third.id, "f3", "f2 is never given again");
        let ids: Vec<String> = at(&dir).list().expect("read back").into_iter().map(|f| f.id).collect();
        assert_eq!(ids, ["f1", "f3"]);
        assert_eq!(filters.running_in("mail:anna@example.org").expect("read").len(), 1, "f1 is off");
    }

    /// A file that does not read is left as it is and answers `error`.
    #[test]
    fn a_damaged_file_is_left_as_it_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("mail").join(FILE_NAME);
        fs::create_dir_all(dir.path().join("mail")).expect("dir");
        fs::write(&file, b"{not json").expect("write");
        let filters = at(&dir);
        assert_eq!(code(filters.list().expect_err("damaged")), "error");
        assert_eq!(code(filters.create(spec("from:x"), &[], Utc::now()).expect_err("damaged")), "error");
        assert_eq!(fs::read(&file).expect("still there"), b"{not json");
        assert_eq!(code(MailFilters::at(None).create(spec("from:x"), &[], Utc::now()).expect_err("no place")), "error");
    }

    /// The checks of a filter: a search that reads and is not empty, no `in:` or `filename:`, an
    /// action, a folder only with an account, a category of the catalogue, at most 200.
    #[test]
    fn filters_are_checked_before_they_are_stored() {
        let dir = tempfile::tempdir().expect("temp dir");
        let filters = at(&dir);
        let now = Utc::now();
        let refused = |spec: FilterSpec, categories: &[Category]| {
            code(filters.create(spec, categories, now).expect_err("refused"))
        };
        for query in ["", "   ", "in:archive", "from:x in:inbox", "filename:pdf", "after:yesterday", "is:maybe"] {
            assert_eq!(refused(spec(query), &[]), "error", "{query:?}");
        }
        assert_eq!(refused(FilterSpec { file: None, ..spec("from:x") }, &[]), "error", "no action");
        let folder = Some(FilterFile::Folder { id: "f:3".into() });
        assert_eq!(refused(FilterSpec { file: folder, ..spec("from:x") }, &[]), "error", "a folder needs an account");
        let invoice = FilterSpec { category: Some(Category::Invoice), ..spec("from:x") };
        assert_eq!(refused(invoice.clone(), &[Category::Newsletter]), "error", "not in the catalogue");
        assert!(filters.create(invoice, &[Category::Invoice], now).is_ok());
        assert!(filters.create(FilterSpec { file: None, mark_read: true, ..spec("-from:x OR y") }, &[], now).is_ok());
        for _ in 2..FILTERS_MAX {
            filters.create(spec("from:x"), &[], now).expect("made");
        }
        assert_eq!(refused(spec("from:x"), &[]), "error", "the 201st");
    }

    /// A client sees a filter of an account it reaches, one for every account only with every account.
    #[test]
    fn a_client_sees_the_filters_of_its_accounts() {
        let mut rights = torro_ipc::api::RightsTemplate::FullAccess.rights();
        rights.mail_accounts = Selection::Only(vec!["mail:anna@example.org".into()]);
        let some = Reach::Client(&rights);
        assert!(visible(some, Some("mail:anna@example.org")));
        assert!(!visible(some, Some("mail:bernd@example.org")));
        assert!(!visible(some, None));
        rights.mail_accounts = Selection::All;
        assert!(visible(Reach::Client(&rights), None));
        assert!(visible(Reach::Suite, None));
    }

    fn message(id: &str, flags: Vec<Flag>, category: Option<MailCategory>) -> MessageSummary {
        let json = serde_json::json!({
            "id": id, "account": "mail:anna@example.org", "folder": "f:1", "thread_id": "t",
            "date": "2026-10-01T09:00:00Z", "untrusted": true,
        });
        let mut summary: MessageSummary = serde_json::from_value(json).expect("a summary");
        summary.flags = flags;
        summary.category = category;
        summary
    }

    /// Decision 4: a category set by hand stays; reading and a category already there need nothing.
    #[test]
    fn only_mail_that_still_needs_an_action_is_due() {
        let filter = |mark_read, category, file| MailFilter {
            id: "f1".into(),
            query: "from:x".into(),
            account: None,
            file,
            mark_read,
            category,
            enabled: true,
            created: Utc::now(),
        };
        let catalogue = [Category::Invoice];
        let read = message("m:1", vec![Flag::Seen], None);
        let unread = message("m:2", Vec::new(), None);
        assert!(!needs(&filter(true, None, None), &read, &catalogue));
        assert!(needs(&filter(true, None, None), &unread, &catalogue));
        assert!(needs(&filter(false, None, Some(FilterFile::Trash)), &read, &catalogue));
        let invoice = filter(false, Some(Category::Invoice), None);
        assert!(needs(&invoice, &read, &catalogue));
        let by_rule = message("m:3", Vec::new(), Some(MailCategory::by_rule(Category::Invoice)));
        let by_person = message("m:4", Vec::new(), Some(MailCategory::by_person(Category::Newsletter)));
        let by_model = message("m:5", Vec::new(), Some(MailCategory::by_model(Category::Newsletter, 0.9)));
        assert!(!needs(&invoice, &by_rule, &catalogue));
        assert!(!needs(&invoice, &by_person, &catalogue));
        assert!(needs(&invoice, &by_model, &catalogue));
        assert!(!needs(&invoice, &read, &[]), "a category removed from the catalogue is skipped");
    }

    /// Step 3 of issue #339: of the new mail, the filters find what their searches match in the
    /// inbox, and what they do together follows their order.
    #[tokio::test]
    async fn matching_finds_new_mail_and_merges_the_filters_in_order() {
        use crate::account::github::tests::MemoryTokens;
        use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
        use crate::mail::sync::fake::{FakeImap, mail};
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &crate::mail::save::tests::raw_on(Utc::now(), "Bill", &[]), &[]);
        server.add("INBOX", &mail("Hello", 1, None), &[]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;
        service.sync_all(Reach::Suite).await.expect("synced");
        let (_, store) = service.stores(Reach::Suite).await.into_iter().next().expect("Anna's store");
        let bill = crate::mail::save::tests::message_id(&service, "Bill").await;
        let hello = crate::mail::save::tests::message_id(&service, "Hello").await;

        let filter = |id: &str, query: &str, category, file| MailFilter {
            id: id.into(),
            query: query.into(),
            account: None,
            file,
            mark_read: false,
            category,
            enabled: true,
            created: Utc::now(),
        };
        let filters = [
            filter("f1", "from:stadtwerke-bonn.de", Some(Category::Invoice), None),
            filter("f2", "from:stadtwerke-bonn.de", Some(Category::Newsletter), Some(FilterFile::Archive)),
            filter("f3", "in:archive", None, Some(FilterFile::Trash)),
        ];
        let new = vec![bill.clone(), hello];
        let found = matching(&store, &filters, &new).await.expect("searched");
        let expected =
            FilterActions { mark_read: false, category: Some(Category::Invoice), file: Some(FilterFile::Archive) };
        assert_eq!(
            found,
            vec![(bill, expected)],
            "Bernd's mail matches none; a search that no filter may have is skipped"
        );
        assert!(matching(&store, &filters, &[]).await.expect("nothing").is_empty());
    }

    /// A change checks only a category it brings anew: the one a filter has passes after the person
    /// removed it from the catalogue, so the filter can still be switched off; another is checked.
    #[test]
    fn a_change_keeps_a_category_gone_from_the_catalogue() {
        let dir = tempfile::tempdir().expect("temp dir");
        let filters = at(&dir);
        let gone = Category::Custom(torro_mail::CustomCategory::new(9).expect("a number"));
        let held = FilterSpec { category: Some(gone), ..spec("from:x") };
        filters.create(held.clone(), &[gone], Utc::now()).expect("made while it was there");
        let off = filters.update("f1", FilterSpec { enabled: Some(false), ..held }, &[]).expect("switched off");
        assert!(!off.enabled && off.category == Some(gone));
        let other = Category::Custom(torro_mail::CustomCategory::new(8).expect("a number"));
        let changed = FilterSpec { category: Some(other), ..spec("from:x") };
        assert_eq!(code(filters.update("f1", changed, &[]).expect_err("not in the catalogue")), "error");
    }

    /// The fingerprint changes with every field of the stored filter.
    #[test]
    fn the_fingerprint_follows_the_stored_filter() {
        let dir = tempfile::tempdir().expect("temp dir");
        let filters = at(&dir);
        let made = filters.create(spec("from:x"), &[], Utc::now()).expect("made");
        let before = fingerprint(&made);
        assert_eq!(before, fingerprint(&filters.get("f1").expect("read").expect("there")));
        let changed = filters.update("f1", FilterSpec { enabled: Some(false), ..spec("from:x") }, &[]).expect("off");
        assert_ne!(before, fingerprint(&changed));
    }
}
