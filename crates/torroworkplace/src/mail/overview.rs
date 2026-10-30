//! The numbers of `mail.overview` (issue #409): what came in over a span of local days, of which
//! kind, from whom and how much of it was opened, and what lies in the inbox folder now.
//!
//! The store reads the rows ([`MailStore::overview_rows`], [`MailStore::inbox_rows`]); [`count`]
//! turns the rows of every reachable account into the answer without any IO, so the rules are
//! tested here with fixed days and a fixed time zone:
//!
//! - **Came in** is every folder but Sent and Drafts (the store leaves those out): archived mail
//!   counts as well, or a person who files away everything would see nothing come in.
//! - **One message once.** The same `Message-ID` in two folders or two accounts (Gmail's "All Mail"
//!   beside the inbox) counts once: opened when any copy is, with the first category any copy has.
//!   A message without a `Message-ID` counts per row, as nothing says two of them are the same.
//! - **Opened** is what the server marks as seen.
//! - **Days** are local days: a mail at 23:30 belongs to the evening it arrived, not to UTC's day.
//! - **Groups** come from the catalogue as it is now, so a category the person moved counts in its
//!   new group; a category the catalogue lacks falls back to the suite's default group.
//! - **Senders** are told apart by their address in lower case. The newest mail outside the trash
//!   stands for a sender ([`Latest`]), the handler reads its summary from the account's store.
//!
//! Nothing here keeps or logs anything; the answer is only ever handed to the caller.

#[cfg(doc)]
use super::store::MailStore;

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use torro_ipc::StatsRange;
use torro_ipc::mail::{
    MailOverview, OVERVIEW_SENDERS, OverviewCategory, OverviewDay, OverviewGroupCount, OverviewInbox, OverviewSender,
};
use torro_mail::{Category, CategoryDef, CategoryGroup, MailAddress, MessageSummary};

/// One message of what came in, as [`MailStore::overview_rows`] reads it: any folder but Sent and
/// Drafts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameIn {
    /// The message's id in its account's store (`m:…`).
    pub id: String,
    /// The `Date:` the store keeps.
    pub date: DateTime<Utc>,
    /// The sender; `None` when the header had none or the store cannot read it.
    pub from: Option<MailAddress>,
    /// The server marks it as seen (`\Seen`).
    pub seen: bool,
    /// Its category; `None` when it has none or one this version does not know.
    pub category: Option<Category>,
    /// The `Message-ID:` header; `None` when it has none.
    pub message_id: Option<String>,
    /// It lies in the trash folder.
    pub trash: bool,
}

/// One message of the inbox folder, as [`MailStore::inbox_rows`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InInbox {
    pub date: DateTime<Utc>,
    /// `None` when it has none or one this version does not know.
    pub category: Option<Category>,
    /// The `Message-ID:` header, so a message in the inbox of two accounts counts once.
    pub message_id: Option<String>,
}

/// The rows of one account, in the order the store gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRows {
    /// `mail:<address>`.
    pub account: String,
    /// What came in from [`since`] on.
    pub came_in: Vec<CameIn>,
    /// The inbox folder now.
    pub inbox: Vec<InInbox>,
}

/// Which message stands for a sender: the store of `account` resolves `id` into its summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub account: String,
    pub id: String,
}

/// A sender as [`count`] found it, before its newest mail is read ([`SenderCount::with`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderCount {
    pub name: Option<String>,
    /// In lower case.
    pub address: String,
    pub count: u32,
    pub opened: u32,
    pub category: Option<Category>,
    /// The newest mail outside the trash; the newest of all when every one lies there.
    pub latest: Latest,
}

impl SenderCount {
    /// The sender as `mail.overview` answers it, with `latest` the summary of [`SenderCount::latest`].
    pub fn with(self, latest: MessageSummary) -> OverviewSender {
        OverviewSender {
            name: self.name,
            address: self.address,
            count: self.count,
            opened: self.opened,
            category: self.category,
            latest,
        }
    }
}

/// What [`count`] counted: the overview with `senders` still empty, and the senders it is to hold,
/// the most first, at most [`OVERVIEW_SENDERS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub overview: MailOverview,
    pub senders: Vec<SenderCount>,
}

/// The first local day of `range` ending with `today`.
pub fn first_day(range: StatsRange, today: NaiveDate) -> NaiveDate {
    today.checked_sub_days(Days::new(u64::from(range.days().saturating_sub(1)))).unwrap_or(today)
}

/// From which instant the store is asked for `range` ending with `today`: midnight UTC a day before
/// the first local day. No time zone is more than a day off UTC, so every mail of the first local
/// day is read; [`count`] drops what lies before it.
pub fn since(range: StatsRange, today: NaiveDate) -> DateTime<Utc> {
    let first = first_day(range, today);
    first.checked_sub_days(Days::new(1)).unwrap_or(first).and_time(NaiveTime::MIN).and_utc()
}

/// Counts the overview of `range` ending with the local day `today` from the rows of every account
/// in reach. `local_day` puts an instant on its local day (`chrono::Local` in the service, a fixed
/// offset in tests); `as_of` is when it was counted.
pub fn count(
    accounts: &[AccountRows],
    catalogue: &[CategoryDef],
    range: StatsRange,
    today: NaiveDate,
    as_of: DateTime<Utc>,
    local_day: impl Fn(DateTime<Utc>) -> NaiveDate,
) -> Counted {
    let from = first_day(range, today);
    let group = |category: Option<Category>| group_of(catalogue, category);
    let messages = came_in(accounts, from, today, &local_day);

    let mut per_day: BTreeMap<NaiveDate, BTreeMap<(bool, Option<CategoryGroup>), u32>> = BTreeMap::new();
    let mut per_category: HashMap<Option<Category>, (u32, u32)> = HashMap::new();
    for message in &messages {
        let group = group(message.category);
        // Mail without a group goes last on its day.
        let on_day = per_day.entry(message.day).or_default().entry((group.is_none(), group)).or_default();
        *on_day = on_day.saturating_add(1);
        let (count, opened) = per_category.entry(message.category).or_default();
        *count = count.saturating_add(1);
        *opened = opened.saturating_add(u32::from(message.seen));
    }

    let days = from
        .iter_days()
        .take_while(|day| *day <= today)
        .map(|day| OverviewDay {
            day,
            groups: per_day
                .get(&day)
                .map(|groups| {
                    groups
                        .iter()
                        .map(|((_, group), count)| OverviewGroupCount { group: *group, count: *count })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();

    let mut categories: Vec<OverviewCategory> = per_category
        .into_iter()
        .map(|(category, (count, opened))| OverviewCategory { category, group: group(category), count, opened })
        .collect();
    categories.sort_by_key(|entry| (Reverse(entry.count), order(entry.category)));

    let overview = MailOverview {
        from,
        until: today,
        as_of,
        days,
        categories,
        senders: Vec::new(),
        inbox: inbox(accounts, today, &local_day, &group),
    };
    Counted { overview, senders: senders(&messages) }
}

/// The group of `category` in the catalogue now, else the suite's default; `None` without one.
fn group_of(catalogue: &[CategoryDef], category: Option<Category>) -> Option<CategoryGroup> {
    let category = category?;
    catalogue.iter().find(|def| def.category == category).map(|def| def.group).or_else(|| category.default_group())
}

/// How two categories with the same count are put in order: by key, mail without one last.
fn order(category: Option<Category>) -> (bool, String) {
    (category.is_none(), category.map(|category| category.key().into_owned()).unwrap_or_default())
}

/// A `Message-ID` worth comparing: a blank one says nothing.
fn message_id(id: Option<&String>) -> Option<&str> {
    id.map(|id| id.trim()).filter(|id| !id.is_empty())
}

/// One message of what came in, its copies merged.
struct Message<'a> {
    date: DateTime<Utc>,
    day: NaiveDate,
    from: Option<&'a MailAddress>,
    seen: bool,
    category: Option<Category>,
    /// The copy that stands for it: one outside the trash when there is one.
    copy: (&'a str, &'a str),
    /// Every copy lies in the trash.
    trash: bool,
}

/// What came in on the days `from` to `until`, every message once.
fn came_in<'a>(
    accounts: &'a [AccountRows],
    from: NaiveDate,
    until: NaiveDate,
    local_day: &impl Fn(DateTime<Utc>) -> NaiveDate,
) -> Vec<Message<'a>> {
    let mut messages: Vec<Message<'a>> = Vec::new();
    let mut seen_ids: HashMap<&'a str, usize> = HashMap::new();
    for account in accounts {
        for row in &account.came_in {
            let day = local_day(row.date);
            if day < from || day > until {
                continue;
            }
            let held = message_id(row.message_id.as_ref()).and_then(|id| seen_ids.get(id).copied());
            if let Some(index) = held
                && let Some(message) = messages.get_mut(index)
            {
                message.seen |= row.seen;
                message.category = message.category.or(row.category);
                message.from = message.from.or(row.from.as_ref());
                if message.trash && !row.trash {
                    message.copy = (account.account.as_str(), row.id.as_str());
                    message.trash = false;
                }
                continue;
            }
            if let Some(id) = message_id(row.message_id.as_ref()) {
                seen_ids.insert(id, messages.len());
            }
            messages.push(Message {
                date: row.date,
                day,
                from: row.from.as_ref(),
                seen: row.seen,
                category: row.category,
                copy: (account.account.as_str(), row.id.as_str()),
                trash: row.trash,
            });
        }
    }
    messages
}

/// A sender while counting.
#[derive(Default)]
struct Tally<'a> {
    count: u32,
    opened: u32,
    categories: HashMap<Option<Category>, u32>,
    /// The newest mail and the name it carries.
    newest: Option<&'a Message<'a>>,
    /// The newest mail outside the trash.
    kept: Option<&'a Message<'a>>,
}

/// The newer of `held` and `message`; on the same second the larger id, so the answer does not
/// depend on the order of the rows.
fn newer<'a>(held: Option<&'a Message<'a>>, message: &'a Message<'a>) -> &'a Message<'a> {
    match held {
        Some(held) if (held.date, held.copy) >= (message.date, message.copy) => held,
        _ => message,
    }
}

/// The senders with the most mail, at most [`OVERVIEW_SENDERS`]; on the same count by address.
fn senders(messages: &[Message<'_>]) -> Vec<SenderCount> {
    let mut tallies: HashMap<String, Tally<'_>> = HashMap::new();
    for message in messages {
        let Some(from) = message.from else { continue };
        let address = from.address.trim().to_lowercase();
        if address.is_empty() {
            continue;
        }
        let tally = tallies.entry(address).or_default();
        tally.count = tally.count.saturating_add(1);
        tally.opened = tally.opened.saturating_add(u32::from(message.seen));
        let of_category = tally.categories.entry(message.category).or_default();
        *of_category = of_category.saturating_add(1);
        tally.newest = Some(newer(tally.newest, message));
        if !message.trash {
            tally.kept = Some(newer(tally.kept, message));
        }
    }
    let mut senders: Vec<SenderCount> = tallies
        .into_iter()
        .filter_map(|(address, tally)| {
            let newest = tally.newest?;
            let latest = tally.kept.unwrap_or(newest);
            let name = newest.from.and_then(|from| from.name.clone()).filter(|name| !name.trim().is_empty());
            // The most frequent category; on a tie one that is a category, then by key.
            let category = tally
                .categories
                .iter()
                .max_by_key(|(category, count)| (**count, Reverse(order(**category))))
                .and_then(|(category, _)| *category);
            Some(SenderCount {
                name,
                address,
                count: tally.count,
                opened: tally.opened,
                category,
                latest: Latest { account: latest.copy.0.to_string(), id: latest.copy.1.to_string() },
            })
        })
        .collect();
    senders.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.address.cmp(&b.address)));
    senders.truncate(OVERVIEW_SENDERS);
    senders
}

/// The inbox folders now per category: today, the six days before, older. A message in the inbox of
/// two accounts counts once, with the first category any copy has.
fn inbox(
    accounts: &[AccountRows],
    today: NaiveDate,
    local_day: &impl Fn(DateTime<Utc>) -> NaiveDate,
    group: &impl Fn(Option<Category>) -> Option<CategoryGroup>,
) -> Vec<OverviewInbox> {
    let week_start = today.checked_sub_days(Days::new(6)).unwrap_or(today);
    let mut messages: Vec<(NaiveDate, Option<Category>)> = Vec::new();
    let mut seen_ids: HashMap<&str, usize> = HashMap::new();
    for row in accounts.iter().flat_map(|account| &account.inbox) {
        let held = message_id(row.message_id.as_ref()).and_then(|id| seen_ids.get(id).copied());
        if let Some(index) = held
            && let Some((_, category)) = messages.get_mut(index)
        {
            *category = category.or(row.category);
            continue;
        }
        if let Some(id) = message_id(row.message_id.as_ref()) {
            seen_ids.insert(id, messages.len());
        }
        messages.push((local_day(row.date), row.category));
    }
    let mut per_category: HashMap<Option<Category>, [u32; 3]> = HashMap::new();
    for (day, category) in messages {
        // A date in the future (a wrong clock at the sender) counts as today.
        let age = if day >= today {
            0
        } else if day >= week_start {
            1
        } else {
            2
        };
        let counts = per_category.entry(category).or_default();
        counts[age] = counts[age].saturating_add(1);
    }
    let mut inbox: Vec<OverviewInbox> = per_category
        .into_iter()
        .map(|(category, [today, week, older])| OverviewInbox { category, group: group(category), today, week, older })
        .collect();
    inbox.sort_by_key(|entry| {
        (Reverse(entry.today.saturating_add(entry.week).saturating_add(entry.older)), order(entry.category))
    });
    inbox
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};
    use torro_mail::CategoryIcon;

    use super::*;

    const ANNA: &str = "mail:anna@example.org";
    const BERND: &str = "mail:bernd@example.org";

    /// Berlin in summer: two hours ahead of UTC.
    fn zone() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).expect("offset")
    }

    fn local_day(at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&zone()).date_naive()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 13).expect("day")
    }

    /// `days_ago` local days before today at `hour:minute` local time.
    fn at(days_ago: u64, hour: u32, minute: u32) -> DateTime<Utc> {
        let day = today().checked_sub_days(Days::new(days_ago)).expect("day");
        let local = day.and_hms_opt(hour, minute, 0).expect("time");
        zone().from_local_datetime(&local).single().expect("instant").with_timezone(&Utc)
    }

    fn row(id: &str, date: DateTime<Utc>, from: &str, category: Option<Category>) -> CameIn {
        CameIn {
            id: id.into(),
            date,
            from: Some(MailAddress { name: None, address: from.into() }),
            seen: false,
            category,
            message_id: Some(format!("<{id}@example.org>")),
            trash: false,
        }
    }

    fn rows(account: &str, came_in: Vec<CameIn>) -> AccountRows {
        AccountRows { account: account.into(), came_in, inbox: Vec::new() }
    }

    fn counted(accounts: &[AccountRows], range: StatsRange) -> Counted {
        count(accounts, &CategoryDef::defaults(), range, today(), at(0, 12, 0), local_day)
    }

    /// All mail that came in over the span.
    fn total(overview: &MailOverview) -> u32 {
        overview.days.iter().flat_map(|day| &day.groups).map(|group| group.count).sum()
    }

    /// The same mail in two accounts (Gmail's "All Mail" and a forward) counts once, opened when one
    /// copy is, with the category one of them has; without a Message-ID each row counts.
    #[test]
    fn a_message_in_two_accounts_counts_once() {
        let mut theirs = row("x", at(1, 9, 0), "news@shop.example", None);
        theirs.id = "m:b".into();
        let mut ours = row("x", at(1, 9, 0), "news@shop.example", Some(Category::Newsletter));
        ours.seen = true;
        let mut loose = row("y", at(1, 10, 0), "news@shop.example", None);
        loose.message_id = None;
        let mut loose_again = loose.clone();
        loose_again.id = "m:y2".into();
        let accounts = [rows(ANNA, vec![theirs, loose]), rows(BERND, vec![ours, loose_again])];
        let counted = counted(&accounts, StatsRange::Week);
        assert_eq!(total(&counted.overview), 3, "the shared one once, the two without an id each");
        let newsletter = counted
            .overview
            .categories
            .iter()
            .find(|entry| entry.category == Some(Category::Newsletter))
            .expect("the category of the other copy");
        assert_eq!((newsletter.count, newsletter.opened), (1, 1), "opened as one copy is");
        assert_eq!(newsletter.group, Some(CategoryGroup::Info));
        let sender = &counted.senders[0];
        assert_eq!((sender.address.as_str(), sender.count, sender.opened), ("news@shop.example", 3, 1));
    }

    /// A span without mail still has every day, all of them empty, and nothing else.
    #[test]
    fn an_empty_span_has_every_day_and_no_numbers() {
        let counted = counted(&[rows(ANNA, Vec::new())], StatsRange::Month);
        let overview = counted.overview;
        assert_eq!(overview.days.len(), 30);
        assert_eq!((overview.from, overview.until), (today() - Days::new(29), today()));
        assert_eq!(overview.days.first().map(|day| day.day), Some(overview.from));
        assert_eq!(overview.days.last().map(|day| day.day), Some(today()));
        assert!(overview.days.iter().all(|day| day.groups.is_empty()));
        assert!(overview.categories.is_empty() && overview.inbox.is_empty() && counted.senders.is_empty());
        assert_eq!(overview.as_of, at(0, 12, 0));
    }

    /// Local days: a mail at 23:50 local time belongs to that evening, one at 00:10 to the next
    /// morning, though UTC still puts it on the evening before. Mail before the first day or after
    /// today does not count.
    #[test]
    fn mail_lands_on_its_local_day() {
        let late = row("late", at(2, 23, 50), "a@example.org", None);
        let early = row("early", at(1, 0, 10), "a@example.org", None);
        assert_eq!(early.date.date_naive(), today() - Days::new(2), "UTC puts it on the day before");
        let before = row("before", at(7, 23, 0), "a@example.org", None);
        let future = row("future", at(0, 23, 59) + chrono::Duration::hours(1), "a@example.org", None);
        let counted = counted(&[rows(ANNA, vec![late, early, before, future])], StatsRange::Week);
        let on = |days_ago: u64| {
            let day = today() - Days::new(days_ago);
            counted.overview.days.iter().find(|entry| entry.day == day).map(|entry| entry.groups.clone())
        };
        let one = vec![OverviewGroupCount { group: None, count: 1 }];
        assert_eq!(on(2), Some(one.clone()));
        assert_eq!(on(1), Some(one));
        assert_eq!(total(&counted.overview), 2, "neither the day before the span nor tomorrow");
        assert!(since(StatsRange::Week, today()) <= at(6, 0, 0), "the store is asked early enough");
    }

    /// Six senders at most, the most first, a tie by address; a mail without a sender counts for
    /// none. The name is the newest mail's, the category the most frequent.
    #[test]
    fn the_top_six_senders_and_a_tie() {
        let mut came_in = Vec::new();
        for (n, address) in
            ["g@x.org", "f@x.org", "e@x.org", "d@x.org", "c@x.org", "b@x.org", "a@x.org"].into_iter().enumerate()
        {
            // a and b one each, the rest more the earlier they stand.
            let times = if n >= 5 { 1 } else { 7 - n };
            for i in 0..times {
                came_in.push(row(&format!("{address}{i}"), at(1, 9, i as u32), address, None));
            }
        }
        let mut named = row("named-old", at(3, 9, 0), "G@X.org", Some(Category::Newsletter));
        named.from = Some(MailAddress { name: Some("Old name".into()), address: "G@X.org".into() });
        let mut newest = row("named-new", at(0, 9, 0), "g@x.org", Some(Category::Newsletter));
        newest.from = Some(MailAddress { name: Some("Golem".into()), address: "g@x.org".into() });
        let mut nobody = row("nobody", at(0, 8, 0), "x@x.org", None);
        nobody.from = None;
        came_in.extend([named, newest, nobody]);
        let counted = counted(&[rows(ANNA, came_in)], StatsRange::Week);
        let order: Vec<(&str, u32)> =
            counted.senders.iter().map(|sender| (sender.address.as_str(), sender.count)).collect();
        assert_eq!(
            order,
            [("g@x.org", 9), ("f@x.org", 6), ("e@x.org", 5), ("d@x.org", 4), ("c@x.org", 3), ("a@x.org", 1)],
            "six, b loses the tie with a by address"
        );
        let golem = &counted.senders[0];
        assert_eq!(golem.name.as_deref(), Some("Golem"), "the newest mail's name");
        assert_eq!(golem.category, None, "seven of nine have none");
        assert_eq!(golem.latest, Latest { account: ANNA.into(), id: "named-new".into() });
    }

    /// The newest mail outside the trash stands for a sender; only when every one is there, the newest.
    #[test]
    fn the_latest_mail_lies_outside_the_trash() {
        let older = row("kept", at(2, 9, 0), "a@x.org", Some(Category::Personal));
        let mut thrown = row("thrown", at(0, 9, 0), "a@x.org", Some(Category::Personal));
        thrown.trash = true;
        let mut only_trash = row("only", at(1, 9, 0), "b@x.org", None);
        only_trash.trash = true;
        // A copy in the trash of one account and outside it in the other: the kept copy stands.
        let mut twice = row("twice", at(0, 10, 0), "c@x.org", None);
        twice.trash = true;
        let mut kept_copy = twice.clone();
        kept_copy.id = "twice-kept".into();
        kept_copy.trash = false;
        let accounts = [rows(ANNA, vec![older, thrown, only_trash, twice]), rows(BERND, vec![kept_copy])];
        let counted = counted(&accounts, StatsRange::Week);
        let latest = |address: &str| {
            counted.senders.iter().find(|sender| sender.address == address).map(|sender| sender.latest.clone())
        };
        assert_eq!(latest("a@x.org"), Some(Latest { account: ANNA.into(), id: "kept".into() }));
        assert_eq!(latest("b@x.org"), Some(Latest { account: ANNA.into(), id: "only".into() }));
        assert_eq!(latest("c@x.org"), Some(Latest { account: BERND.into(), id: "twice-kept".into() }));
    }

    /// Groups come from the catalogue as it is now; categories go the most first with what was opened.
    #[test]
    fn groups_follow_the_catalogue_and_categories_go_the_most_first() {
        let mut catalogue = CategoryDef::defaults();
        if let Some(def) = catalogue.iter_mut().find(|def| def.category == Category::Newsletter) {
            def.group = CategoryGroup::WaitingForMe;
        }
        let custom = Category::from_key("custom_7").expect("custom");
        catalogue.push(CategoryDef {
            category: custom,
            group: CategoryGroup::Custom(3),
            name: Some("Club".into()),
            description: None,
            model: false,
            icon: CategoryIcon::default(),
        });
        let mut opened = row("n1", at(1, 9, 0), "a@x.org", Some(Category::Newsletter));
        opened.seen = true;
        let came_in = vec![
            opened,
            row("n2", at(1, 9, 1), "a@x.org", Some(Category::Newsletter)),
            row("c1", at(1, 9, 2), "a@x.org", Some(custom)),
            row("u1", at(1, 9, 3), "a@x.org", None),
        ];
        let overview =
            count(&[rows(ANNA, came_in)], &catalogue, StatsRange::Week, today(), at(0, 12, 0), local_day).overview;
        let categories: Vec<(Option<Category>, Option<CategoryGroup>, u32, u32)> =
            overview.categories.iter().map(|entry| (entry.category, entry.group, entry.count, entry.opened)).collect();
        assert_eq!(
            categories,
            [
                (Some(Category::Newsletter), Some(CategoryGroup::WaitingForMe), 2, 1),
                (Some(custom), Some(CategoryGroup::Custom(3)), 1, 0),
                (None, None, 1, 0),
            ]
        );
        let day = overview.days.iter().find(|day| day.day == today() - Days::new(1)).expect("yesterday");
        let groups: Vec<(Option<CategoryGroup>, u32)> =
            day.groups.iter().map(|group| (group.group, group.count)).collect();
        assert_eq!(
            groups,
            [(Some(CategoryGroup::WaitingForMe), 2), (Some(CategoryGroup::Custom(3)), 1), (None, 1)],
            "mail without a group last"
        );
    }

    /// The inbox by age: today, the six days before, older; the same message in two inboxes once.
    #[test]
    fn the_inbox_goes_by_age() {
        let mail = |days_ago: u64, category: Option<Category>, id: &str| InInbox {
            date: at(days_ago, 0, 5),
            category,
            message_id: Some(id.into()),
        };
        let mut anna = rows(ANNA, Vec::new());
        anna.inbox = vec![
            mail(0, Some(Category::Newsletter), "<1>"),
            mail(6, Some(Category::Newsletter), "<2>"),
            mail(7, Some(Category::Newsletter), "<3>"),
            mail(40, None, "<4>"),
        ];
        let mut bernd = rows(BERND, Vec::new());
        bernd.inbox = vec![mail(7, None, "<3>"), mail(1, None, "<5>")];
        let overview = counted(&[anna, bernd], StatsRange::Quarter).overview;
        let inbox: Vec<(Option<Category>, u32, u32, u32)> =
            overview.inbox.iter().map(|entry| (entry.category, entry.today, entry.week, entry.older)).collect();
        assert_eq!(inbox, [(Some(Category::Newsletter), 1, 1, 1), (None, 0, 1, 1)]);
        assert_eq!(overview.inbox[0].group, Some(CategoryGroup::Info));
    }
}
