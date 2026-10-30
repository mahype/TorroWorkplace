//! The overview of the mail window (issue #409, `docs/mail.md`, draft
//! `docs/prototypes/mail-uebersicht/index.html`): what stands at the right while no mail is open – how
//! much mail comes in, of which kind, from whom and how much of it is read, the latest mail of people
//! and what lies in the inbox –, so the person sees where tidying up pays off.
//!
//! - **Where** (the app places it, `app.rs`): in the wide window in the reader's column while no mail
//!   is open; in the medium window as a panel over the list and in the narrow one in place of the
//!   list, while [`State::open`]. Mini and strip never show it.
//! - **Asked:** `mail.overview` for the chosen span and account ([`MailSource::mail_overview`]) and
//!   the latest mail of people ([`MailSource::latest_from_people`]). The numbers stay while another
//!   span or account loads ("Updating…") and are asked anew once they are older than [`FRESH`].
//! - **Blocks**, top down: the head (title, account, span, "As of", the switches), four figures
//!   ([`figures`]), "Latest from people", "What comes in" – columns per day by main category
//!   ([`series`], [`columns`]) or the same numbers as a table ([`day_table`]), and the categories
//!   with their share ([`category_rows`]) –, "Who sends the most" (with "Opened x of y" and
//!   [`rarely_opened`], "Show mails" and "Mute" on hover) and "What is in your inbox" ([`inbox_parts`],
//!   [`by_age`] and at most one suggestion to tidy up, [`tidy`]). The page only shows: "Show them"
//!   lists the mail, nothing is archived by itself.
//!
//! Names, addresses and subjects come from mail (`untrusted`) and are drawn as plain text.

use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, pick_list, responsive, row, scrollable};
use iced::{Alignment, Background, Element, Fill, Length, Padding, Theme, border};
use torro_core::i18n::{Lang, lang, tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, ChartColor, palette, size, space};
use torro_gui::units::format_count;
use torro_gui::widgets::{self, Anchor, ChartColumn, ChartSeries, PageAction, RowAction, SharePart, StatFigure};
use torro_gui::{datetime, layout, style, typo};
use torro_ipc::StatsRange;
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::mail::{MailOverview, OverviewCategory, OverviewDay, OverviewGroupCount, OverviewInbox, OverviewSender};
use torro_mail::{Category, CategoryGroup, MessageSummary};

use crate::app::Message;
#[cfg(doc)]
use crate::mail::MailSource;
use crate::{insights, list, mail};

/// How many mails "Latest from people" lists.
pub const PEOPLE: u32 = 5;

/// How old the numbers may grow before the overview asks for them anew when it shows.
pub const FRESH: Duration = Duration::from_secs(60);

/// How many categories "What comes in" names; the rest is "Other".
const CATEGORIES: usize = 6;

/// From how many mails a sender can be "Rarely opened".
const RARELY_FROM: u32 = 5;

/// Below this share in percent of a sender's mail opened, it is "Rarely opened".
const RARELY_BELOW: u32 = 15;

/// The spans to choose, in the order of the switch.
pub const RANGES: [StatsRange; 3] = insights::RANGES;

/// The main categories that have a colour of their own in the charts, in the order of their slots
/// (`ChartColor::Series(0..3)`); every other – "Unsorted", one the person made, mail without a
/// category – is "Other" (decision 2 of the plan: the chart colours, not the groups' own).
const GROUPS: [CategoryGroup; 4] =
    [CategoryGroup::WaitingForMe, CategoryGroup::PaperTrail, CategoryGroup::Info, CategoryGroup::Junk];

/// What the numbers were asked for: one account or all, over one span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub account: Option<String>,
    pub range: StatsRange,
}

/// An answer of `mail.overview` and when it came.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub key: Key,
    pub at: Instant,
    pub result: Result<MailOverview, String>,
}

/// The latest mail of people as it came: for which account (or every one), and the mail or the error.
pub type People = (Option<String>, Result<Vec<MessageSummary>, String>);

/// The overview's window state: whether it is open where it has to be opened (medium, narrow), what
/// is chosen, and the answers.
#[derive(Debug, Clone)]
pub struct State {
    /// Shown in the medium and the narrow window ("Overview" in the path or the sidebar); the wide
    /// window shows it whenever no mail is open.
    pub open: bool,
    pub range: StatsRange,
    /// One account (`mail:<address>`), or `None` for every one.
    pub account: Option<String>,
    /// "Table": the day chart's numbers as a table.
    pub table: bool,
    /// The last answer; it stays while another span or account loads.
    pub data: Option<Loaded>,
    /// The latest mail of people, for the account it was asked for.
    pub people: Option<People>,
    /// What is on its way: an answer for anything else is dropped.
    pub asked: Option<Key>,
    /// The sender (its address) whose "Mute" menu is open: its row keeps its bar, so the menu hangs
    /// at its button.
    pub muting: Option<String>,
    /// "Refresh" asked for the numbers anew, however fresh they are.
    pub stale: bool,
}

impl Default for State {
    fn default() -> State {
        State {
            open: false,
            range: StatsRange::Month,
            account: None,
            table: false,
            data: None,
            people: None,
            asked: None,
            muting: None,
            stale: false,
        }
    }
}

impl State {
    /// What is chosen now.
    pub fn key(&self) -> Key {
        Key { account: self.account.clone(), range: self.range }
    }

    /// Whether the numbers must be asked for at `now`: they are not on their way, and there are none
    /// for what is chosen or they are older than [`FRESH`].
    pub fn due(&self, now: Instant) -> bool {
        let key = self.key();
        if self.asked.as_ref() == Some(&key) {
            return false;
        }
        if self.stale {
            return true;
        }
        match &self.data {
            Some(loaded) if loaded.key == key => now.saturating_duration_since(loaded.at) > FRESH,
            _ => true,
        }
    }

    /// Notes that the numbers for what is chosen are asked for, and returns what.
    pub fn ask(&mut self) -> Key {
        let key = self.key();
        self.asked = Some(key.clone());
        self.stale = false;
        key
    }

    /// Takes the answer for `key` that came at `now`: only while it is the one asked for, so a span
    /// chosen meanwhile never shows another's numbers. `false` when it was dropped.
    pub fn answered(&mut self, key: Key, result: Result<MailOverview, String>, now: Instant) -> bool {
        if self.asked.as_ref() != Some(&key) {
            return false;
        }
        self.asked = None;
        self.data = Some(Loaded { key, at: now, result });
        true
    }

    /// The numbers shown belong to another span or account than chosen, whose own are on their way.
    pub fn updating(&self) -> bool {
        self.data.as_ref().is_some_and(|loaded| loaded.key != self.key())
    }

    /// The overview of the answer shown, if it came without an error.
    pub fn overview(&self) -> Option<&MailOverview> {
        self.data.as_ref().and_then(|loaded| loaded.result.as_ref().ok())
    }

    /// The sender with `address` in the answer shown.
    pub fn sender(&self, address: &str) -> Option<&OverviewSender> {
        self.overview()?.senders.iter().find(|sender| sender.address == address)
    }

    /// The latest mail of people for the account chosen, if it came.
    pub fn people(&self) -> Option<&[MessageSummary]> {
        match &self.people {
            Some((account, Ok(people))) if *account == self.account => Some(people),
            _ => None,
        }
    }
}

/// One choice of the account menu: every account, or one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChoice {
    pub account: Option<String>,
    pub label: String,
}

impl std::fmt::Display for AccountChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The choices of the account menu: "All accounts", then each of `accounts` (id and name).
pub fn account_choices(accounts: &[(String, String)]) -> Vec<AccountChoice> {
    let all = AccountChoice { account: None, label: tr("All accounts").to_owned() };
    std::iter::once(all)
        .chain(accounts.iter().map(|(id, label)| AccountChoice { account: Some(id.clone()), label: label.clone() }))
        .collect()
}

/// The word of a span in the switch: "7 days".
pub fn range_name(range: StatsRange) -> &'static str {
    match range {
        StatsRange::Week => tr("7 days"),
        StatsRange::Month => tr("30 days"),
        StatsRange::Quarter => tr("90 days"),
    }
}

/// The span in the head's line: "Last 30 days".
fn range_words(range: StatsRange) -> &'static str {
    match range {
        StatsRange::Week => tr("Last 7 days"),
        StatsRange::Month => tr("Last 30 days"),
        StatsRange::Quarter => tr("Last 90 days"),
    }
}

// ---------------------------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------------------------

/// What came in over the span: every category counted.
pub fn came_in(overview: &MailOverview) -> u32 {
    overview.categories.iter().map(|category| category.count).sum()
}

/// How much of what came in is of `categories`.
fn of_categories(overview: &MailOverview, categories: &[Category]) -> u32 {
    overview
        .categories
        .iter()
        .filter(|held| held.category.is_some_and(|category| categories.contains(&category)))
        .map(|held| held.count)
        .sum()
}

/// What newsletters and ads are: the categories the suggestion to tidy up and the fourth figure
/// count.
pub const NEWSLETTERS_AND_ADS: [Category; 2] = [Category::Newsletter, Category::Promotion];

/// The mail in the inbox by age: today, the six days before, older.
pub fn by_age(overview: &MailOverview) -> (u32, u32, u32) {
    overview
        .inbox
        .iter()
        .fold((0, 0, 0), |(today, week, older), held| (today + held.today, week + held.week, older + held.older))
}

/// The four figures: what lies in the inbox (with what is older than a week), what came in (with
/// about how much a day), how much of it came from people ("Personal") and the share of newsletters
/// and ads.
pub fn figures(overview: &MailOverview) -> [StatFigure; 4] {
    let (today, week, older) = by_age(overview);
    let inbox = today + week + older;
    let came = came_in(overview);
    let days = u32::try_from(overview.days.len()).unwrap_or(1).max(1);
    let per_day = (f64::from(came) / f64::from(days)).round() as u32;
    let people = of_categories(overview, &[Category::Personal]);
    let ads = of_categories(overview, &NEWSLETTERS_AND_ADS);
    let count = |n: u32| format_count(u64::from(n));
    [
        StatFigure::new(tr("In your inbox"), count(inbox), trf("{n} older than a week", &[("n", &count(older))])),
        StatFigure::new(tr("Came in"), count(came), trf("about {n} a day", &[("n", &count(per_day))])),
        StatFigure::new(
            tr("From people"),
            count(people),
            trf("{share} of what came in", &[("share", &insights::percent(people, came))]),
        ),
        StatFigure::new(
            tr("Newsletters and ads"),
            insights::percent(ads, came),
            trf("{n} of {total} mails", &[("n", &count(ads)), ("total", &count(came))]),
        ),
    ]
}

/// The colour slot of the main category `group` in the charts ([`GROUPS`]).
pub fn slot(group: Option<CategoryGroup>) -> ChartColor {
    match group.and_then(|group| GROUPS.iter().position(|held| *held == group)) {
        Some(at) => ChartColor::Series(at),
        None => ChartColor::Other,
    }
}

/// What a person reads for the main category `group`: the catalogue's name, else the suite's.
fn group_name(catalogue: &[CategoryInfo], group: CategoryGroup) -> String {
    catalogue
        .iter()
        .find(|held| held.group == group)
        .map_or_else(|| torro_core::category::group_label(group).to_owned(), |held| held.group_label.clone())
}

/// The series of the charts: the four main categories with a colour of their own and "Other", each
/// only when something of it came in, in the order of their slots.
pub fn series(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Vec<ChartSeries> {
    let mut sums = [0u32; 5];
    for counted in overview.days.iter().flat_map(|day| &day.groups) {
        sums[slot_index(slot(counted.group))] += counted.count;
    }
    let mut series: Vec<ChartSeries> = GROUPS
        .iter()
        .enumerate()
        .filter(|(at, _)| sums[*at] > 0)
        .map(|(at, group)| ChartSeries { name: group_name(catalogue, *group), color: ChartColor::Series(at) })
        .collect();
    if sums[4] > 0 {
        series.push(ChartSeries { name: tr("Other").to_owned(), color: ChartColor::Other });
    }
    series
}

/// Where a colour slot sums up in [`series`]: the four series, then "Other".
fn slot_index(color: ChartColor) -> usize {
    match color {
        ChartColor::Series(at) => at.min(3),
        ChartColor::Other => 4,
    }
}

/// One column of the day chart: a day, or a week of the quarter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub from: NaiveDate,
    pub until: NaiveDate,
    /// What came in, per series of [`series`].
    pub values: Vec<u32>,
}

/// The days in columns: one per day for 7 and 30 days, one per seven days for 90 (counted back from
/// today, the first holds what is left over), as Insights does.
pub fn buckets(overview: &MailOverview, range: StatsRange, series: &[ChartSeries]) -> Vec<Bucket> {
    let size = if range == StatsRange::Quarter { 7 } else { 1 };
    let mut buckets: Vec<Bucket> = overview
        .days
        .rchunks(size)
        .filter_map(|days: &[OverviewDay]| {
            let (first, last) = (days.first()?, days.last()?);
            let mut values = vec![0u32; series.len()];
            for counted in days.iter().flat_map(|day| &day.groups) {
                let color = slot(counted.group);
                if let Some(at) = series.iter().position(|series| series.color == color)
                    && let Some(value) = values.get_mut(at)
                {
                    *value += counted.count;
                }
            }
            Some(Bucket { from: first.day, until: last.day, values })
        })
        .collect();
    buckets.reverse();
    buckets
}

/// A bucket's name in its tooltip and table row: the day ("Tue, 13 Oct"), or the days of a week.
fn bucket_title(bucket: &Bucket, today: NaiveDate) -> String {
    if bucket.from == bucket.until {
        datetime::format_short_date(bucket.until, today)
    } else {
        trf(
            "{from} – {until}",
            &[("from", &datetime::format_day_month(bucket.from)), ("until", &datetime::format_day_month(bucket.until))],
        )
    }
}

/// The label under a bucket's column: the weekday for 7 days, the day of the month for 30, the first
/// day for a week of 90.
fn bucket_label(bucket: &Bucket, range: StatsRange) -> String {
    match range {
        StatsRange::Week => datetime::weekday_name(bucket.until, true).to_owned(),
        StatsRange::Month => bucket.until.day().to_string(),
        StatsRange::Quarter => datetime::format_day_month(bucket.from),
    }
}

/// The columns of the day chart.
pub fn columns(overview: &MailOverview, range: StatsRange, series: &[ChartSeries]) -> Vec<ChartColumn> {
    buckets(overview, range, series)
        .into_iter()
        .map(|bucket| ChartColumn {
            label: bucket_label(&bucket, range),
            title: bucket_title(&bucket, overview.until),
            values: bucket.values,
        })
        .collect()
}

/// The day chart's numbers as a table: a row per bucket with its name, a value per series and the
/// total, then the sums.
pub fn day_table(
    overview: &MailOverview,
    range: StatsRange,
    series: &[ChartSeries],
) -> (Vec<String>, Vec<Vec<String>>, Vec<String>) {
    let first = if range == StatsRange::Quarter { tr("Week") } else { tr("Day") };
    let mut head = vec![first.to_owned()];
    head.extend(series.iter().map(|series| series.name.clone()));
    head.push(tr("Total").to_owned());
    let mut sums = vec![0u32; series.len()];
    let rows = columns(overview, range, series)
        .into_iter()
        .map(|column| {
            for (sum, value) in sums.iter_mut().zip(&column.values) {
                *sum += *value;
            }
            let mut cells = vec![column.title.clone()];
            cells.extend(column.values.iter().map(u32::to_string));
            cells.push(column.total().to_string());
            cells
        })
        .collect();
    let mut total = vec![tr("Total").to_owned()];
    total.extend(sums.iter().map(u32::to_string));
    total.push(sums.iter().sum::<u32>().to_string());
    (head, rows, total)
}

/// One line of the categories under the day chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryRow {
    pub label: String,
    /// The category's icon and the place of its group's colour; `None` for "Other".
    pub mark: Option<(Icon, usize)>,
    pub count: u32,
    /// Its share of what came in: "34%".
    pub share: String,
}

/// The [`CATEGORIES`] categories most mail came of, the most first, then "Other" with the rest: the
/// category "Other", mail without a category yet and the smaller ones. Nothing for a span without
/// mail.
pub fn category_rows(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Vec<CategoryRow> {
    let whole = came_in(overview);
    let mut named: Vec<&OverviewCategory> = overview
        .categories
        .iter()
        .filter(|held| held.count > 0 && held.category.is_some_and(|category| category != Category::Other))
        .collect();
    named.sort_by_key(|held| std::cmp::Reverse(held.count));
    let shown: u32 = named.iter().take(CATEGORIES).map(|held| held.count).sum();
    let mut rows: Vec<CategoryRow> = named
        .into_iter()
        .take(CATEGORIES)
        .filter_map(|held| {
            let category = held.category?;
            let (label, group) = mail::category_look(catalogue, category);
            let (_, colour) = mail::group_look(catalogue, held.group.unwrap_or(group));
            let symbol = catalogue
                .iter()
                .find(|info| info.category == category)
                .map_or_else(|| category.default_icon(), |info| info.icon);
            Some(CategoryRow {
                label,
                mark: Some((mail::category_symbol(symbol), colour)),
                count: held.count,
                share: insights::percent(held.count, whole),
            })
        })
        .collect();
    let rest = whole.saturating_sub(shown);
    if rest > 0 {
        rows.push(CategoryRow {
            label: tr("Other").to_owned(),
            mark: None,
            count: rest,
            share: insights::percent(rest, whole),
        });
    }
    rows
}

/// A sender is "Rarely opened" with at least [`RARELY_FROM`] mails of which less than
/// [`RARELY_BELOW`] percent were opened.
pub fn rarely_opened(count: u32, opened: u32) -> bool {
    count >= RARELY_FROM && u64::from(opened) * 100 < u64::from(count) * u64::from(RARELY_BELOW)
}

/// How many mails `count` over `days` days are a week, with one decimal in the language's way:
/// "7.5 a week", German "7,5 pro Woche".
pub fn per_week(count: u32, days: usize) -> String {
    let days = days.max(1) as f64;
    let digits = format!("{:.1}", f64::from(count) * 7.0 / days);
    let digits = if lang() == Lang::De { digits.replace('.', ",") } else { digits };
    trf("{n} a week", &[("n", &digits)])
}

/// What lies in the inbox, as parts of a strip: the four main categories with a colour of their own
/// and "Other", each only with mail.
pub fn inbox_parts(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Vec<SharePart> {
    let mut sums = [0u64; 5];
    for held in &overview.inbox {
        sums[slot_index(slot(held.group))] += u64::from(held.today + held.week + held.older);
    }
    let mut parts: Vec<SharePart> = GROUPS
        .iter()
        .enumerate()
        .filter(|(at, _)| sums[*at] > 0)
        .map(|(at, group)| {
            SharePart::new(group_name(catalogue, *group), sums[at], format_count(sums[at]), ChartColor::Series(at))
        })
        .collect();
    if sums[4] > 0 {
        parts.push(SharePart::new(tr("Other"), sums[4], format_count(sums[4]), ChartColor::Other));
    }
    parts
}

/// The one suggestion to tidy up: how many newsletters and ads in the inbox are older than a week;
/// `None` when none are.
pub fn tidy(overview: &MailOverview) -> Option<u32> {
    let older: u32 = overview
        .inbox
        .iter()
        .filter(|held| held.category.is_some_and(|category| NEWSLETTERS_AND_ADS.contains(&category)))
        .map(|held| held.older)
        .sum();
    (older > 0).then_some(older)
}

/// Nothing came over the span and the inbox is empty: the empty state stands under the head.
pub fn is_empty(overview: &MailOverview) -> bool {
    let (today, week, older) = by_age(overview);
    came_in(overview) == 0 && today + week + older == 0
}

// ---------------------------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------------------------

/// Where the overview stands, which decides its head and its inset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The reader's column of the wide window: its own title, the page inset.
    Page,
    /// The panel of the medium window: the panel's head and title stand over it.
    Panel,
    /// In place of the list in the narrow window: the header says "Overview".
    Narrow,
}

/// What the page needs from the window.
pub struct OverviewData<'a> {
    pub state: &'a State,
    /// The catalogue of categories, for their names, icons and groups.
    pub catalogue: &'a [CategoryInfo],
    /// The mail accounts: id and name, for the account menu.
    pub accounts: Vec<(String, String)>,
    pub now: DateTime<Utc>,
    pub place: Place,
    /// Where the menu of "Mute" hangs: at the button of the sender in [`State::muting`].
    pub mute_at: Anchor,
}

/// The overview: its head, then loading, the error with "Retry", the empty span, or the blocks, at
/// most a reading width wide and scrolling. While another span or account loads the numbers stay
/// with "Updating…" over them.
pub fn view(data: OverviewData<'_>) -> Element<'_, Message> {
    let state = data.state;
    let mut blocks: Vec<Element<'_, Message>> = vec![head(&data)];
    match &state.data {
        None => blocks.push(widgets::loading()),
        Some(Loaded { result: Err(_), .. }) if state.updating() => blocks.push(widgets::loading()),
        Some(Loaded { result: Err(_), .. }) => {
            blocks.push(widgets::error_state(tr("Couldn't load the overview."), Message::MailOverviewRetry));
        }
        Some(Loaded { result: Ok(overview), .. }) => {
            if state.updating() {
                blocks.push(widgets::refresh_note(tr("Updating…")));
            }
            if is_empty(overview) {
                blocks.push(widgets::empty_state(
                    tr("No mail in this span."),
                    tr("What comes in, who sends it and what you open shows up here."),
                ));
            } else {
                blocks.push(widgets::stat_grid(figures(overview).into()));
                if let Some(people) = people_card(&data) {
                    blocks.push(people);
                }
                blocks.push(comes_in_card(overview, state, data.catalogue));
                blocks.push(senders_card(overview, state, data.catalogue, &data.mute_at));
                blocks.push(inbox_card(overview, data.catalogue));
            }
        }
    }
    let content = column(blocks).spacing(layout::section_gap()).width(Fill).max_width(size::content_max());
    match data.place {
        // The panel scrolls and insets its body itself.
        Place::Panel => content.into(),
        Place::Page | Place::Narrow => {
            let page = container(content).width(Fill).center_x(Fill).padding(layout::page_padding());
            scrollable(page)
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(style::scrollable)
                .width(Fill)
                .height(Fill)
                .into()
        }
    }
}

/// The head: "Overview" (not in the panel, whose title says it), the line of account, span and "As
/// of", and the switches – beside the title where there is room, under the line in the panel and
/// the narrow window.
fn head<'a>(data: &OverviewData<'a>) -> Element<'a, Message> {
    let state = data.state;
    let choices = account_choices(&data.accounts);
    let account_label = choices
        .iter()
        .find(|choice| choice.account == state.account)
        .map_or_else(|| tr("All accounts").to_owned(), |choice| choice.label.clone());
    let mut line = vec![account_label, range_words(state.range).to_owned()];
    if let Some(overview) = state.overview() {
        line.push(trf("As of {time}", &[("time", &datetime::format_moment(overview.as_of, data.now))]));
    }
    let meta = typo::text(typo::meta(), line.join(" · ")).style(theme::text_3);
    let chosen = RANGES.iter().position(|range| *range == state.range).unwrap_or_default();
    let ranges = widgets::segmented(RANGES.map(|range| (range_name(range), Message::MailOverviewRange(range))), chosen);
    let selected = choices.iter().find(|choice| choice.account == state.account).cloned();
    let accounts: Element<'a, Message> = pick_list(choices, selected, Message::MailOverviewAccount)
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into();
    let controls = row![ranges, accounts].spacing(space::s()).align_y(Alignment::Center);
    match data.place {
        Place::Page => {
            let words = column![typo::text(typo::title(), tr("Overview")), meta].spacing(space::xs());
            row![container(words).width(Fill), controls].spacing(space::m()).align_y(Alignment::Center).into()
        }
        Place::Panel => column![meta, controls.wrap()].spacing(space::s()).into(),
        Place::Narrow => {
            let words = column![typo::text(typo::title(), tr("Overview")), meta].spacing(space::xs());
            column![words, controls.wrap()].spacing(space::m()).into()
        }
    }
}

/// "Latest from people": the newest mails of "Personal", unread ones with a dot, a click opens one,
/// "Show all" the category. Left out while none came or the search failed.
fn people_card<'a>(data: &OverviewData<'a>) -> Option<Element<'a, Message>> {
    let people = data.state.people().filter(|people| !people.is_empty())?;
    let unread = people.iter().filter(|mail| !mail.flags.contains(&torro_mail::Flag::Seen)).count();
    let note = if unread > 0 { trf("{n} unread", &[("n", &unread)]) } else { String::new() };
    let more = widgets::text_button(
        Some(Icon::ChevronRight),
        tr("Show all"),
        style::button::ghost,
        Some(Message::MailOverviewAllPeople),
    );
    let lines = column(people.iter().map(|mail| person_line(mail, data.now))).spacing(space::xs());
    let body = column![lines, container(more).width(Fill).align_right(Fill)].spacing(space::s());
    Some(widgets::chart_card(tr("Latest from people"), note, body))
}

/// One mail of a person: avatar, name with the dot of unread mail, the subject, the time; a click
/// opens it.
fn person_line<'a>(mail: &MessageSummary, now: DateTime<Utc>) -> Element<'a, Message> {
    let name = mail::sender_name(mail.from.as_ref());
    let unread = !mail.flags.contains(&torro_mail::Flag::Seen);
    let mut who = row![].spacing(space::s()).align_y(Alignment::Center);
    if unread {
        who = who.push(unread_mark());
    }
    let who = who.push(widgets::ellipsized(typo::body_strong(), name.clone(), theme::text));
    let words = column![who, widgets::ellipsized(typo::meta(), list::one_line(&mail.subject), theme::text_3)];
    let time =
        typo::text(typo::meta().tabular(), list::stamp(mail.date, now)).style(theme::text_3).wrapping(Wrapping::None);
    let line = row![list::avatar(&name, None, size::tile()), container(words).width(Fill), time]
        .spacing(space::m())
        .align_y(Alignment::Center);
    button(line)
        .width(Fill)
        .padding([space::xs(), space::s()])
        .style(style::button::ghost)
        .on_press(Message::MailOverviewPerson(mail.id.clone()))
        .into()
}

/// The dot of unread mail, as the rows of the list have it.
fn unread_mark<'a>() -> Element<'a, Message> {
    container(Space::new().width(space::s()).height(space::s()))
        .style(|t: &Theme| iced::widget::container::Style {
            background: Some(Background::Color(palette(t).unread)),
            border: border::rounded(space::s() / 2.0),
            ..iced::widget::container::Style::default()
        })
        .into()
}

/// "What comes in": the columns per day by main category (or their table) and the categories with
/// their share.
fn comes_in_card<'a>(overview: &MailOverview, state: &State, catalogue: &[CategoryInfo]) -> Element<'a, Message> {
    let series = series(overview, catalogue);
    let note = if state.range == StatsRange::Quarter {
        tr("per week, by main category")
    } else {
        tr("per day, by main category")
    };
    let table = widgets::choice_chip(
        tr("Table"),
        Some(widgets::Mark::Icon(Icon::Table)),
        state.table,
        Some(Message::MailOverviewTable(!state.table)),
    );
    let mut body = column![].spacing(space::l());
    if state.table {
        let (head, rows, total) = day_table(overview, state.range, &series);
        body = body.push(container(table).width(Fill).align_right(Fill));
        body = body.push(widgets::data_table(head, rows, Some(total)));
    } else {
        let columns = columns(overview, state.range, &series);
        // The legend from two series on, the switch to the table at its right.
        let legend: Element<'a, Message> =
            if series.len() > 1 { widgets::chart_legend(&series) } else { Space::new().into() };
        let top = row![container(legend).width(Fill), table].spacing(space::m()).align_y(Alignment::Center);
        body = body.push(column![top, widgets::column_chart(series, columns)].spacing(space::m()));
    }
    let rows = category_rows(overview, catalogue);
    let largest = rows.iter().map(|row| row.count).max().unwrap_or(0);
    body = body.push(column(rows.into_iter().map(|row| category_line(row, largest))).spacing(space::xs()));
    widgets::chart_card(tr("What comes in"), note, body)
}

/// A category's line: its mark and name, a bar against the largest, the count and the share.
fn category_line<'a>(row_: CategoryRow, largest: u32) -> Element<'a, Message> {
    let mark: Element<'a, Message> = match row_.mark {
        Some((symbol, colour)) => icon::icon(symbol, icon::SIZE_SM)
            .style(move |t: &Theme, _| {
                let p = palette(t);
                iced::widget::svg::Style { color: Some(p.category[colour.min(p.category.len() - 1)]) }
            })
            .into(),
        None => Space::new().width(icon::SIZE_SM).into(),
    };
    let name = row![mark, widgets::ellipsized(typo::body(), row_.label, theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    row![
        container(name).width(Length::FillPortion(2)),
        container(widgets::share_bar(row_.count, largest)).width(Length::FillPortion(3)),
        container(typo::text(typo::body().tabular(), format_count(u64::from(row_.count))))
            .width(Length::FillPortion(1))
            .align_right(Length::FillPortion(1)),
        container(typo::text(typo::meta().tabular(), row_.share).style(theme::text_3))
            .width(Length::FillPortion(1))
            .align_right(Length::FillPortion(1)),
    ]
    .spacing(space::m())
    .align_y(Alignment::Center)
    .into()
}

/// "Who sends the most": a line per sender with how much it sends, how much of it is opened and –
/// on hover – "Show mails" and "Mute". Below a form's width the column of what is opened goes, and
/// the category stands without the address.
fn senders_card<'a>(
    overview: &MailOverview,
    state: &State,
    catalogue: &[CategoryInfo],
    mute_at: &Anchor,
) -> Element<'a, Message> {
    let senders = overview.senders.clone();
    let catalogue = catalogue.to_vec();
    let days = overview.days.len();
    let muting = state.muting.clone();
    let mute_at = mute_at.clone();
    let body: Element<'a, Message> = if senders.is_empty() {
        typo::text(typo::body(), tr("No sender stands out yet.")).style(theme::text_2).into()
    } else {
        responsive(move |room| {
            let wide = room.width >= size::form_max();
            let lines = senders.iter().map(|sender| {
                let muting = muting.as_deref() == Some(sender.address.as_str());
                sender_line(sender, &catalogue, days, wide, muting, &mute_at)
            });
            column(lines).spacing(space::xs()).into()
        })
        .height(Length::Shrink)
        .into()
    };
    widgets::chart_card(tr("Who sends the most"), tr("and how much of it you open"), body)
}

/// One sender: avatar, name, address and usual category; "Opened x of y" with its bar and "Rarely
/// opened" (wide only); how many and how many a week; on hover the bar with "Show mails" and "Mute".
fn sender_line<'a>(
    sender: &OverviewSender,
    catalogue: &[CategoryInfo],
    days: usize,
    wide: bool,
    muting: bool,
    mute_at: &Anchor,
) -> Element<'a, Message> {
    let name = sender
        .name
        .as_deref()
        .map(list::one_line)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| list::one_line(&sender.address));
    let category = sender.category.map(|category| mail::category_look(catalogue, category).0);
    let detail = match (wide, category) {
        (true, Some(category)) => format!("{} · {category}", list::one_line(&sender.address)),
        (true, None) => list::one_line(&sender.address),
        (false, Some(category)) => category,
        (false, None) => list::one_line(&sender.address),
    };
    let who = column![
        widgets::ellipsized(typo::body_strong(), name.clone(), theme::text),
        widgets::ellipsized(typo::meta(), detail, theme::text_3),
    ];
    let mut line = row![list::avatar(&name, None, size::tile()), container(who).width(Length::FillPortion(3))]
        .spacing(space::m())
        .align_y(Alignment::Center);
    if wide {
        let opened = trf(
            "Opened {n} of {total}",
            &[("n", &format_count(u64::from(sender.opened))), ("total", &format_count(u64::from(sender.count)))],
        );
        let mut read = column![
            typo::text(typo::meta().tabular(), opened).style(theme::text_2).wrapping(Wrapping::None),
            widgets::share_bar(sender.opened, sender.count),
        ]
        .spacing(space::xs());
        if rarely_opened(sender.count, sender.opened) {
            let rare = row![
                icon::icon(Icon::EyeOff, icon::SIZE_SM)
                    .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) }),
                typo::text(typo::meta(), tr("Rarely opened")).style(theme::text_3).wrapping(Wrapping::None),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center);
            read = read.push(rare);
        }
        line = line.push(container(read).width(Length::FillPortion(2)));
    }
    let number = column![
        typo::text(typo::body_strong().tabular(), format_count(u64::from(sender.count))),
        typo::text(typo::meta(), per_week(sender.count, days)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .align_x(Alignment::End);
    line = line.push(number);
    let row_ = container(line).width(Fill).padding(Padding::new(space::xs()).left(space::s()).right(space::s()));
    let mute = RowAction::new(Icon::BellOff, tr("Mute"), "", Message::MailOverviewMute(sender.address.clone()));
    let actions = vec![
        RowAction::new(Icon::Search, tr("Show mails"), "", Message::MailOverviewSender(sender.address.clone())),
        if muting { mute.at(mute_at) } else { mute },
    ];
    let words = widgets::row_actions_words(&actions, size::form_max());
    widgets::row_actions(row_, actions, muting, words)
}

/// "What is in your inbox": the inbox by main category as a strip, by age, and at most one
/// suggestion to tidy up, which only lists the mail.
fn inbox_card<'a>(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Element<'a, Message> {
    let (today, week, older) = by_age(overview);
    let total = today + week + older;
    let mut body = column![].spacing(space::m());
    if total == 0 {
        body = body.push(typo::text(typo::body(), tr("Your inbox is empty.")).style(theme::text_2));
    } else {
        body = body.push(widgets::share_strip(inbox_parts(overview, catalogue), None));
        let age = |label: &'static str, n: u32| {
            row![
                typo::text(typo::meta(), label).style(theme::text_3),
                typo::text(typo::meta().tabular(), format_count(u64::from(n))).style(theme::text),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center)
        };
        let ages = row![
            typo::text(typo::meta(), tr("By age")).style(theme::text_3),
            age(tr("Today"), today),
            age(tr("This week"), week),
            age(tr("Older"), older),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center)
        .wrap();
        body = body.push(ages);
    }
    if let Some(old) = tidy(overview) {
        let title = trf("{n} newsletters and ads are older than a week", &[("n", &format_count(u64::from(old)))]);
        let show = PageAction::new(tr("Show them"), Some(Message::MailOverviewTidy));
        body =
            body.push(widgets::question_card(Icon::Archive, title, tr("Nothing is archived by itself."), vec![show]));
    }
    let note = trf("{n} mails", &[("n", &format_count(u64::from(total)))]);
    widgets::chart_card(tr("What is in your inbox"), note, body)
}

// ---------------------------------------------------------------------------------------------
// The demo
// ---------------------------------------------------------------------------------------------

/// A made-up overview for the demo up to `today` over `range`, around its `inbox` (the demo counts
/// nothing): the days and categories follow a fixed pattern – weekdays busier than weekends,
/// newsletters the most –, the senders are those of its inbox with made-up numbers (so "Show mails"
/// and "Mute" act on real demo mail), the inbox is its own. `one` (a single account) counts about
/// half. The same numbers every time.
pub fn demo(today: NaiveDate, range: StatsRange, one: bool, inbox: &[MessageSummary]) -> MailOverview {
    let count = range.days();
    let from = today.checked_sub_days(Days::new(u64::from(count.saturating_sub(1)))).unwrap_or(today);
    let scale = |n: u32| if one { n.div_ceil(2) } else { n };
    let days: Vec<OverviewDay> = (0..count)
        .filter_map(|i| from.checked_add_days(Days::new(u64::from(i))))
        .map(|day| {
            let weekend = day.weekday().num_days_from_monday() >= 5;
            let wobble = (day.ordinal() * 7) % 5;
            let per = |base: u32, quiet: u32| scale(if weekend { quiet + wobble / 2 } else { base + wobble });
            let groups = [
                (Some(CategoryGroup::WaitingForMe), per(2, 1)),
                (Some(CategoryGroup::PaperTrail), per(1, 0)),
                (Some(CategoryGroup::Info), per(8, 5)),
                (Some(CategoryGroup::Junk), per(0, 0)),
                (None, per(2, 1)),
            ]
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(group, count)| OverviewGroupCount { group, count })
            .collect();
            OverviewDay { day, groups }
        })
        .collect();
    let came: u32 = days.iter().flat_map(|day| &day.groups).map(|counted| counted.count).sum();
    // Shares of what came in, in per mille, as the draft has them.
    let shares = [
        (Some(Category::Newsletter), 345, 80),
        (Some(Category::Personal), 131, 700),
        (Some(Category::Promotion), 114, 20),
        (Some(Category::Shipping), 100, 600),
        (Some(Category::ServiceNotice), 92, 300),
        (Some(Category::Invoice), 53, 800),
        (None, 165, 400),
    ];
    let categories = shares
        .iter()
        .map(|(category, share, opened)| {
            let count = came * share / 1000;
            OverviewCategory {
                category: *category,
                group: category.and_then(Category::default_group),
                count,
                opened: count * opened / 1000,
            }
        })
        .collect();
    let weeks = count.div_ceil(7);
    let made_up = [(4, 0), (3, 0), (3, 2), (3, 1), (2, 2), (2, 1)];
    let mut senders: Vec<OverviewSender> = Vec::new();
    for mail in inbox {
        let Some(from) = mail.from.as_ref() else { continue };
        let address = from.address.to_lowercase();
        if senders.iter().any(|sender| sender.address == address) {
            continue;
        }
        let Some((per_week, opened)) = made_up.get(senders.len()) else { break };
        let count = scale(per_week * weeks);
        senders.push(OverviewSender {
            name: from.name.clone(),
            address,
            count,
            opened: (count * opened / per_week.max(&1)).min(count),
            category: mail.category.map(|held| held.category),
            latest: mail.clone(),
        });
    }
    let mut held: Vec<OverviewInbox> = Vec::new();
    for mail in inbox {
        let category = mail.category.map(|held| held.category);
        let day = mail.date.with_timezone(&Local).date_naive();
        let age = (today - day).num_days();
        let entry = match held.iter_mut().find(|entry| entry.category == category) {
            Some(entry) => entry,
            None => {
                held.push(OverviewInbox {
                    category,
                    group: category.and_then(Category::default_group),
                    today: 0,
                    week: 0,
                    older: 0,
                });
                let Some(last) = held.last_mut() else { continue };
                last
            }
        };
        match age {
            ..=0 => entry.today += 1,
            1..=6 => entry.week += 1,
            _ => entry.older += 1,
        }
    }
    MailOverview {
        from,
        until: today,
        as_of: today.and_hms_opt(9, 14, 0).map_or_else(Utc::now, datetime::to_utc),
        days,
        categories,
        senders,
        inbox: held,
    }
}

/// An overview of nothing over `range` up to `today`: every day there, nothing counted.
pub fn nothing(today: NaiveDate, range: StatsRange) -> MailOverview {
    let mut overview = demo(today, range, false, &[]);
    for day in &mut overview.days {
        day.groups.clear();
    }
    overview.categories.clear();
    overview
}

/// The address of a sender as the search's `from:` takes it.
pub fn from_query(address: &str) -> String {
    format!("from:{address}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_mail::{MailAddress, MailCategory};

    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, n).expect("a day")
    }

    fn overview() -> MailOverview {
        let counted = |group, count| OverviewGroupCount { group, count };
        MailOverview {
            from: day(24),
            until: day(30),
            as_of: Utc::now(),
            days: (24..=30)
                .map(|n| OverviewDay {
                    day: day(n),
                    groups: vec![
                        counted(Some(CategoryGroup::Info), 10),
                        counted(Some(CategoryGroup::WaitingForMe), 2),
                        counted(None, 1),
                    ],
                })
                .collect(),
            categories: vec![
                OverviewCategory {
                    category: Some(Category::Newsletter),
                    group: Some(CategoryGroup::Info),
                    count: 50,
                    opened: 5,
                },
                OverviewCategory {
                    category: Some(Category::Promotion),
                    group: Some(CategoryGroup::Info),
                    count: 20,
                    opened: 0,
                },
                OverviewCategory {
                    category: Some(Category::Personal),
                    group: Some(CategoryGroup::WaitingForMe),
                    count: 14,
                    opened: 14,
                },
                OverviewCategory { category: None, group: None, count: 7, opened: 1 },
            ],
            senders: Vec::new(),
            inbox: vec![
                OverviewInbox {
                    category: Some(Category::Newsletter),
                    group: Some(CategoryGroup::Info),
                    today: 2,
                    week: 3,
                    older: 10,
                },
                OverviewInbox {
                    category: Some(Category::Personal),
                    group: Some(CategoryGroup::WaitingForMe),
                    today: 1,
                    week: 1,
                    older: 0,
                },
            ],
        }
    }

    #[test]
    fn the_figures_count_the_inbox_what_came_people_and_ads() {
        let [inbox, came, people, ads] = figures(&overview());
        assert_eq!((inbox.label.as_str(), inbox.value.as_str()), (tr("In your inbox"), "17"));
        assert_eq!(inbox.note, trf("{n} older than a week", &[("n", &"10")]));
        assert_eq!(came.value, "91");
        assert_eq!(came.note, trf("about {n} a day", &[("n", &"13")]));
        assert_eq!(people.value, "14");
        assert_eq!(people.note, trf("{share} of what came in", &[("share", &insights::percent(14, 91))]));
        assert_eq!(ads.value, insights::percent(70, 91));
        assert_eq!(ads.note, trf("{n} of {total} mails", &[("n", &"70"), ("total", &"91")]));
    }

    #[test]
    fn the_main_categories_keep_their_slots_and_the_rest_is_other() {
        assert_eq!(slot(Some(CategoryGroup::WaitingForMe)), ChartColor::Series(0));
        assert_eq!(slot(Some(CategoryGroup::Junk)), ChartColor::Series(3));
        assert_eq!(slot(Some(CategoryGroup::Unsorted)), ChartColor::Other);
        assert_eq!(slot(Some(CategoryGroup::Custom(7))), ChartColor::Other);
        assert_eq!(slot(None), ChartColor::Other);
        let series = series(&overview(), &[]);
        // Paper trail and junk had nothing: no series, the others keep their colours.
        let colours: Vec<ChartColor> = series.iter().map(|series| series.color).collect();
        assert_eq!(colours, [ChartColor::Series(0), ChartColor::Series(2), ChartColor::Other]);
        assert_eq!(series[2].name, tr("Other"));
        let columns = columns(&overview(), StatsRange::Week, &series);
        assert_eq!(columns.len(), 7);
        assert_eq!(columns[6].values, [2, 10, 1]);
        let (head, rows, total) = day_table(&overview(), StatsRange::Week, &series);
        assert_eq!(head.first().map(String::as_str), Some(tr("Day")));
        assert_eq!(rows.len(), 7);
        assert_eq!(total.last().map(String::as_str), Some("91"));
    }

    #[test]
    fn a_quarter_has_a_column_per_week() {
        let mut long = overview();
        long.days = (0..90)
            .map(|i| OverviewDay {
                day: day(30) - chrono::TimeDelta::days(89 - i),
                groups: vec![OverviewGroupCount { group: Some(CategoryGroup::Info), count: 1 }],
            })
            .collect();
        let series = series(&long, &[]);
        let buckets = buckets(&long, StatsRange::Quarter, &series);
        assert_eq!(buckets.len(), 13, "90 days: 12 weeks and what is left over first");
        assert_eq!(buckets.last().map(|bucket| bucket.values[0]), Some(7));
        assert_eq!(buckets.first().map(|bucket| bucket.values[0]), Some(6));
    }

    #[test]
    fn the_categories_name_the_largest_and_put_the_rest_under_other() {
        let mut many = overview();
        for (i, category) in
            [Category::Invoice, Category::Shipping, Category::Order, Category::Social].into_iter().enumerate()
        {
            many.categories.push(OverviewCategory {
                category: Some(category),
                group: category.default_group(),
                count: 4 - i as u32,
                opened: 0,
            });
        }
        let rows = category_rows(&many, &[]);
        assert_eq!(rows.len(), CATEGORIES + 1);
        assert_eq!(rows[0].count, 50);
        let other = rows.last().expect("other");
        // Social (1) and the mail without a category (7).
        assert_eq!((other.label.as_str(), other.count, other.mark), (tr("Other"), 8, None));
        assert!(category_rows(&nothing(day(30), StatsRange::Week), &[]).is_empty());
    }

    #[test]
    fn a_sender_is_rarely_opened_from_five_mails_under_fifteen_percent() {
        assert!(rarely_opened(30, 2));
        assert!(!rarely_opened(4, 0), "too few to say");
        assert!(!rarely_opened(20, 3), "15% is not rarely");
        assert!(rarely_opened(20, 2));
        assert_eq!(per_week(30, 30), trf("{n} a week", &[("n", &if lang() == Lang::De { "7,0" } else { "7.0" })]));
    }

    #[test]
    fn the_inbox_shows_its_groups_ages_and_one_suggestion() {
        let o = overview();
        assert_eq!(by_age(&o), (3, 4, 10));
        let parts = inbox_parts(&o, &[]);
        assert_eq!(parts.iter().map(|part| part.value).collect::<Vec<_>>(), [2, 15]);
        assert_eq!(parts[1].color, ChartColor::Series(2));
        assert_eq!(tidy(&o), Some(10));
        let mut tidy_already = o.clone();
        tidy_already.inbox.retain(|held| held.category == Some(Category::Personal));
        assert_eq!(tidy(&tidy_already), None, "no suggestion without old newsletters and ads");
        assert!(!is_empty(&o));
        assert!(is_empty(&nothing(day(30), StatsRange::Month)));
        assert_eq!(nothing(day(30), StatsRange::Month).days.len(), 30, "every day stays");
    }

    #[test]
    fn an_answer_counts_only_for_what_was_asked() {
        let mut state = State::default();
        let now = Instant::now();
        assert!(state.due(now));
        let month = state.ask();
        assert!(!state.due(now), "on its way");
        state.range = StatsRange::Week;
        assert!(state.due(now), "another span is asked anew");
        let week = state.ask();
        assert!(!state.answered(month, Ok(overview()), now), "the month came late");
        assert!(state.data.is_none());
        assert!(state.answered(week, Ok(overview()), now));
        assert!(!state.due(now));
        assert!(state.due(now + FRESH + Duration::from_secs(1)), "older than a minute");
        state.account = Some("mail:anna@example.org".into());
        assert!(state.updating(), "the numbers of all accounts stay while one account loads");
    }

    #[test]
    fn the_demo_takes_its_senders_and_inbox_from_its_mail() {
        let mail = |id: &str, address: &str, days_ago: u64, category| MessageSummary {
            id: id.into(),
            account: "mail:anna@example.org".into(),
            folder: "f:inbox".into(),
            thread_id: id.into(),
            from: Some(MailAddress { name: Some("Golem".into()), address: address.into() }),
            to: Vec::new(),
            subject: "News".into(),
            date: datetime::to_utc((day(30) - Days::new(days_ago)).and_hms_opt(8, 0, 0).expect("a time")),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            invite: None,
            back: None,
            pile: None,
            category: Some(MailCategory::by_rule(category)),
            untrusted: true,
            pile_since: None,
        };
        let inbox = [
            mail("m:1", "News@Golem.de", 0, Category::Newsletter),
            mail("m:2", "news@golem.de", 3, Category::Newsletter),
            mail("m:3", "anna@posteo.de", 10, Category::Personal),
        ];
        let demo = demo(day(30), StatsRange::Month, false, &inbox);
        assert_eq!(demo.days.len(), 30);
        assert_eq!(demo.senders.len(), 2, "one sender per address");
        assert_eq!(demo.senders[0].latest.id, "m:1");
        assert_eq!(by_age(&demo), (1, 1, 1));
        assert!(figures(&demo)[1].value != "0");
    }
}
