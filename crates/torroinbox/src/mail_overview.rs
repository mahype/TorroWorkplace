//! The overview of the mail window (issue #409, slimmed in #485, `docs/mail.md`, draft
//! `docs/prototypes/mail-uebersicht/index.html`): what stands at the right while no mail is open – how
//! much mail comes in and of which kind, the way to the details, and what is new in the suite –, so
//! the person sees what comes in without being buried in numbers.
//!
//! - **Where** (the app places it, `app.rs`): in the wide window in the reader's column while no mail
//!   is open; in the medium window as a panel over the list and in the narrow one in place of the
//!   list, while [`State::open`]. Mini and strip never show it.
//! - **Asked:** `mail.overview` for the chosen span and account ([`MailSource::mail_overview`]). The
//!   numbers stay while another span or account loads ("Updating…") and are asked anew once they are
//!   older than [`FRESH`]. Span and account hold for the statistics page too (`mail_statistics.rs`),
//!   which keeps its tab and chosen category here ([`State::tab`], [`State::category`]).
//! - **Blocks**, top down: the head (title, account, span, "As of", the switches, [`controls`]), "What
//!   comes in" – columns per day by main category ([`series`], [`columns`]) and the categories with
//!   their share ([`category_rows`]) – with "Details ›" to the statistics page in the settings, and
//!   "What's new" (`news.rs`) once the numbers are there or failed. The figures, the senders, the inbox and the day chart's
//!   table ([`day_table`]) are the statistics page's.

use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, pick_list, responsive, row, scrollable};
use iced::{Alignment, Element, Fill, Length, Theme};
use torro_core::i18n::{tr, trc, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, ChartColor, palette, size, space};
use torro_gui::units::format_count;
use torro_gui::widgets::{self, Anchor, ChartColumn, ChartSeries};
use torro_gui::{datetime, layout, style, typo};
use torro_ipc::StatsRange;
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::mail::{
    MailOverview, OverviewCategory, OverviewDay, OverviewGroupCount, OverviewInbox, OverviewSender, UnsubscribeTarget,
};
use torro_mail::{Category, CategoryGroup, MessageSummary};

use crate::app::Message;
#[cfg(doc)]
use crate::mail::MailSource;
use crate::{insights, mail, news};

/// How old the numbers may grow before the overview asks for them anew when it shows.
pub const FRESH: Duration = Duration::from_secs(60);

/// How many categories "What comes in" names; the rest is "Other".
const CATEGORIES: usize = 6;

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

/// The tabs of the statistics page (issue #485), in their order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    /// The four figures and the day chart with its table.
    #[default]
    Overview,
    /// Who sends the most.
    Senders,
    /// What lies in the inbox.
    Inbox,
    /// Each category over the span.
    Categories,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Senders, Tab::Inbox, Tab::Categories];

    /// The tab's word.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => tr("Overview"),
            Tab::Senders => tr("Senders"),
            // "Posteingang" in German: the window's "Eingang" would read like the hub.
            Tab::Inbox => trc("statistics", "Inbox"),
            Tab::Categories => tr("Categories"),
        }
    }
}

/// The overview's window state: whether it is open where it has to be opened (medium, narrow), what
/// is chosen – for the statistics page too –, and the answers.
#[derive(Debug, Clone)]
pub struct State {
    /// Shown in the medium and the narrow window ("Overview" in the path or the sidebar); the wide
    /// window shows it whenever no mail is open.
    pub open: bool,
    pub range: StatsRange,
    /// One account (`mail:<address>`), or `None` for every one.
    pub account: Option<String>,
    /// "Table" of the statistics page: the day chart's numbers as a table.
    pub table: bool,
    /// The tab of the statistics page.
    pub tab: Tab,
    /// The category the tab "Categories" charts: `None` for the one with the most mail,
    /// `Some(None)` for the mail not sorted yet.
    pub category: Option<Option<Category>>,
    /// The last answer; it stays while another span or account loads.
    pub data: Option<Loaded>,
    /// What is on its way: an answer for anything else is dropped.
    pub asked: Option<Key>,
    /// The sender (its address) whose "Mute" menu is open: its row keeps its bar, so the menu hangs
    /// at its button.
    pub muting: Option<String>,
    /// "Refresh" asked for the numbers anew, however fresh they are.
    pub stale: bool,
    /// The overview or the statistics page showed at the last look: coming into view again checks the
    /// age of the numbers.
    pub showing: bool,
}

impl Default for State {
    fn default() -> State {
        State {
            open: false,
            range: StatsRange::Month,
            account: None,
            table: false,
            tab: Tab::Overview,
            category: None,
            data: None,
            asked: None,
            muting: None,
            stale: false,
            showing: false,
        }
    }
}

impl State {
    /// What is chosen now.
    pub fn key(&self) -> Key {
        Key { account: self.account.clone(), range: self.range }
    }

    /// Whether the numbers must be asked for at `now`: they are not on their way, and there are none
    /// for what is chosen, "Refresh" asked, or – when the overview comes into view (`anew`) – they are
    /// older than [`FRESH`].
    pub fn due(&self, now: Instant, anew: bool) -> bool {
        let key = self.key();
        if self.asked.as_ref() == Some(&key) {
            return false;
        }
        if self.stale {
            return true;
        }
        match &self.data {
            Some(loaded) if loaded.key == key => anew && now.saturating_duration_since(loaded.at) > FRESH,
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
pub(crate) fn range_words(range: StatsRange) -> &'static str {
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

/// The mail in the inbox by age: today, the six days before, older.
pub fn by_age(overview: &MailOverview) -> (u32, u32, u32) {
    overview
        .inbox
        .iter()
        .fold((0, 0, 0), |(today, week, older), held| (today + held.today, week + held.week, older + held.older))
}

/// The colour slot of the main category `group` in the charts ([`GROUPS`]).
pub fn slot(group: Option<CategoryGroup>) -> ChartColor {
    match group.and_then(|group| GROUPS.iter().position(|held| *held == group)) {
        Some(at) => ChartColor::Series(at),
        None => ChartColor::Other,
    }
}

/// What a person reads for the main category `group`: the catalogue's name, else the suite's.
pub(crate) fn group_name(catalogue: &[CategoryInfo], group: CategoryGroup) -> String {
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

/// The main category with the colour slot `at` ([`GROUPS`]); `None` past the four.
pub(crate) fn group_at(at: usize) -> Option<CategoryGroup> {
    GROUPS.get(at).copied()
}

/// Where a colour slot sums up in [`series`]: the four series, then "Other".
pub(crate) fn slot_index(color: ChartColor) -> usize {
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
pub(crate) fn bucket_title(bucket: &Bucket, today: NaiveDate) -> String {
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
pub(crate) fn bucket_label(bucket: &Bucket, range: StatsRange) -> String {
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
    /// The reader's column of the wide window: the page inset.
    Page,
    /// The panel of the medium window: the panel's head and title stand over it; lined up with the
    /// title.
    Panel,
    /// In place of the list in the narrow window: the header says "Overview", the page inset.
    Narrow,
}

/// What the page – and the statistics page – needs from the window.
pub struct OverviewData<'a> {
    pub state: &'a State,
    /// The catalogue of categories, for their names, icons and groups.
    pub catalogue: &'a [CategoryInfo],
    /// The mail accounts: id and name, for the account menu.
    pub accounts: Vec<(String, String)>,
    pub now: DateTime<Utc>,
    /// Where it stands; the statistics page takes [`Place::Page`] in the wide window and
    /// [`Place::Narrow`] in the medium and narrow ones.
    pub place: Place,
    /// Where the menu of "Mute" hangs: at the button of the sender in [`State::muting`].
    pub mute_at: Anchor,
    /// The suite's announcements, for "What's new".
    pub news: &'a news::State,
}

/// How the numbers of a [`State`] stand.
pub(crate) enum Numbers<'a> {
    /// None yet, or an error while another span or account loads.
    Loading,
    /// They could not be counted: the error with "Retry".
    Failed,
    /// Counted; `updating` while another span's or account's are on their way.
    Ready { overview: &'a MailOverview, updating: bool },
}

/// How the numbers of `state` stand now.
pub(crate) fn numbers(state: &State) -> Numbers<'_> {
    match &state.data {
        None => Numbers::Loading,
        Some(Loaded { result: Err(_), .. }) if state.updating() => Numbers::Loading,
        Some(Loaded { result: Err(_), .. }) => Numbers::Failed,
        Some(Loaded { result: Ok(overview), .. }) => Numbers::Ready { overview, updating: state.updating() },
    }
}

/// The overview: its head, then loading, the error with "Retry", the empty span, or "What comes in",
/// and "What's new" under it once the numbers are there or failed (`news::block`, left out without
/// announcements), at most
/// a reading width wide and scrolling. While another span or account loads the numbers stay with
/// "Updating…" over them.
pub fn view(data: OverviewData<'_>) -> Element<'_, Message> {
    let state = data.state;
    let mut blocks: Vec<Element<'_, Message>> = vec![head(&data)];
    match numbers(state) {
        Numbers::Loading => blocks.push(widgets::loading()),
        Numbers::Failed => {
            blocks.push(widgets::error_state(tr("Couldn't load the overview."), Message::MailOverviewRetry));
        }
        Numbers::Ready { overview, updating } => {
            if updating {
                blocks.push(widgets::refresh_note(tr("Updating…")));
            }
            if is_empty(overview) {
                blocks.push(widgets::empty_state(
                    tr("No mail in this span."),
                    tr("What comes in and of which kind shows up here."),
                ));
            } else {
                blocks.push(comes_in_card(overview, state.range, data.catalogue));
            }
        }
    }
    // While the numbers load, "What's new" waits too: the card would push it down once they came.
    if !matches!(numbers(state), Numbers::Loading) {
        blocks.extend(news::block(data.news));
    }
    let content = column(blocks).spacing(layout::section_gap()).width(Fill).max_width(size::content_max());
    match data.place {
        // The panel scrolls its body itself; inset, the blocks line up with its title.
        Place::Panel => torro_gui::panel::inset(content),
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

/// The head: the line of account, span and "As of" ([`head_line`]), the switches ([`controls`]) under
/// it, wrapping where the room is narrow. No title: the window's top row, the panel's title and the
/// narrow header say "Overview" already (issue #485); the reader's column is too narrow for the line
/// beside the switches.
fn head<'a>(data: &OverviewData<'a>) -> Element<'a, Message> {
    let meta = typo::text(typo::meta(), head_line(data)).style(theme::text_3);
    column![meta, controls(data.state, &data.accounts, true)].spacing(space::s()).into()
}

/// The line under the title: the account, the span and when the numbers were counted.
pub(crate) fn head_line(data: &OverviewData<'_>) -> String {
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
    line.join(" · ")
}

/// The switches of span ("7 days | 30 days | 90 days") and account, one choice for the overview and
/// the statistics page (decision 2 of #485); with `wrap` they wrap when the room is narrow, else they
/// stay one row (beside a line or in a header bar, which never wraps).
pub fn controls<'a>(state: &State, accounts: &[(String, String)], wrap: bool) -> Element<'a, Message> {
    let choices = account_choices(accounts);
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
    if wrap { controls.wrap().into() } else { controls.into() }
}

/// The note of the day chart: what a column is.
pub(crate) fn chart_note(range: StatsRange) -> &'static str {
    if range == StatsRange::Quarter { tr("per week, by main category") } else { tr("per day, by main category") }
}

/// The day chart: the legend from two series on, the columns per day (per week of 90 days) by main
/// category. `switch` stands at the legend's right (the statistics page's "Table").
pub(crate) fn day_chart<'a>(
    overview: &MailOverview,
    range: StatsRange,
    catalogue: &[CategoryInfo],
    switch: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let series = series(overview, catalogue);
    let columns = columns(overview, range, &series);
    let legend: Element<'a, Message> =
        if series.len() > 1 { widgets::chart_legend(&series) } else { Space::new().into() };
    let mut top = row![container(legend).width(Fill)].spacing(space::m()).align_y(Alignment::Center);
    if let Some(switch) = switch {
        top = top.push(switch);
    }
    column![top, widgets::column_chart(series, columns)].spacing(space::m()).into()
}

/// "What comes in": the columns per day by main category, the categories with their share and
/// "Details ›" at the right under them, to the statistics page (issue #485).
fn comes_in_card<'a>(overview: &MailOverview, range: StatsRange, catalogue: &[CategoryInfo]) -> Element<'a, Message> {
    let rows = category_rows(overview, catalogue);
    let largest = rows.iter().map(|row| row.count).max().unwrap_or(0);
    let widths = NumberWidths::of(rows.iter().map(|row| (row.count, row.share.as_str())));
    let details = widgets::text_button_trailing(
        tr("Details"),
        Icon::ChevronRight,
        style::button::ghost,
        Some(Message::ShowMailStatistics),
    );
    // In a narrow column (the reader's beside a wide list) the bars give their room to the names.
    let lines = responsive(move |room| {
        let bars = room.width >= size::panel();
        column(rows.iter().cloned().map(|row| category_line(row, largest, widths, bars))).spacing(space::xs()).into()
    })
    .height(Length::Shrink);
    let body =
        column![day_chart(overview, range, catalogue, None), lines, container(details).width(Fill).align_right(Fill),]
            .spacing(space::l());
    widgets::chart_card(tr("What comes in"), chart_note(range), body)
}

/// The mark of a category: its icon in its group's colour; room of the same width for "Other".
pub(crate) fn category_mark<'a>(mark: Option<(Icon, usize)>) -> Element<'a, Message> {
    match mark {
        Some((symbol, colour)) => icon::icon(symbol, icon::SIZE_SM)
            .style(move |t: &Theme, _| {
                let p = palette(t);
                iced::widget::svg::Style { color: Some(p.category[colour.min(p.category.len() - 1)]) }
            })
            .into(),
        None => Space::new().width(icon::SIZE_SM).into(),
    }
}

/// How wide the columns of counts and shares of a list of categories are: as their widest entry, so
/// the numbers line up and the name takes the rest of the line (issue #485: in a narrow window the
/// names keep their room, in a wide one the numbers stay together at the right).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NumberWidths {
    pub count: f32,
    pub share: f32,
}

impl NumberWidths {
    /// The widths for these counts and shares.
    pub(crate) fn of<'s>(entries: impl Iterator<Item = (u32, &'s str)>) -> NumberWidths {
        entries.fold(NumberWidths { count: 0.0, share: 0.0 }, |widths, (count, share)| NumberWidths {
            count: widths.count.max(typo::text_width(&format_count(u64::from(count)), typo::body().tabular())),
            share: widths.share.max(typo::text_width(share, typo::meta().tabular())),
        })
    }
}

/// The numbers at the end of a category's line: its bar against the largest, the count and the share,
/// in the columns of `widths`.
pub(crate) fn category_numbers<'a>(
    count: u32,
    largest: u32,
    share: String,
    widths: NumberWidths,
) -> [Element<'a, Message>; 3] {
    [
        widgets::share_bar(count, largest),
        container(typo::text(typo::body().tabular(), format_count(u64::from(count))).wrapping(Wrapping::None))
            .width(widths.count)
            .align_right(widths.count)
            .into(),
        container(typo::text(typo::meta().tabular(), share).style(theme::text_3).wrapping(Wrapping::None))
            .width(widths.share)
            .align_right(widths.share)
            .into(),
    ]
}

/// A category's line: its mark and name, then the bar (with `bars`), the count and the share
/// ([`category_numbers`]).
fn category_line<'a>(row_: CategoryRow, largest: u32, widths: NumberWidths, bars: bool) -> Element<'a, Message> {
    let name = row![category_mark(row_.mark), widgets::ellipsized(typo::body(), row_.label, theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let [bar, count, share] = category_numbers(row_.count, largest, row_.share, widths);
    let mut line = row![container(name).width(Fill)].spacing(space::m()).align_y(Alignment::Center);
    if bars {
        line = line.push(bar);
    }
    line.push(count).push(share).into()
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
            // Per day the category's share of that day (issue #485), so its chart follows the week.
            let per_day =
                days.iter().map(|day| day.groups.iter().map(|counted| counted.count).sum::<u32>() * share / 1000);
            OverviewCategory {
                category: *category,
                group: category.and_then(Category::default_group),
                count,
                opened: count * opened / 1000,
                days: per_day.collect(),
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
            // Issue #416: newsletters and promotions offer unsubscribing, by their newest mail.
            unsubscribe: mail
                .category
                .is_some_and(|held| matches!(held.category, Category::Newsletter | Category::Promotion))
                .then(|| UnsubscribeTarget { account: mail.account.clone(), id: mail.id.clone() }),
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
pub(crate) mod tests {
    use super::*;
    use torro_mail::{MailAddress, MailCategory};

    pub(crate) fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, n).expect("a day")
    }

    /// A week of made-up numbers, also for the statistics page's tests.
    pub(crate) fn overview() -> MailOverview {
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
                    days: vec![5, 5, 10, 5, 5, 10, 10],
                },
                OverviewCategory {
                    category: Some(Category::Promotion),
                    group: Some(CategoryGroup::Info),
                    count: 20,
                    opened: 0,
                    days: Vec::new(),
                },
                OverviewCategory {
                    category: Some(Category::Personal),
                    group: Some(CategoryGroup::WaitingForMe),
                    count: 14,
                    opened: 14,
                    days: vec![2; 7],
                },
                OverviewCategory { category: None, group: None, count: 7, opened: 1, days: vec![1; 7] },
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
                days: Vec::new(),
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
    fn an_answer_counts_only_for_what_was_asked() {
        let mut state = State::default();
        let now = Instant::now();
        assert!(state.due(now, false));
        let month = state.ask();
        assert!(!state.due(now, true), "on its way");
        state.range = StatsRange::Week;
        assert!(state.due(now, false), "another span is asked anew");
        let week = state.ask();
        assert!(!state.answered(month, Ok(overview()), now), "the month came late");
        assert!(state.data.is_none());
        assert!(state.answered(week, Ok(overview()), now));
        assert!(!state.due(now, true));
        let later = now + FRESH + Duration::from_secs(1);
        assert!(!state.due(later, false), "not on every tick while it stands");
        assert!(state.due(later, true), "older than a minute when it comes into view");
        state.stale = true;
        assert!(state.due(now, false), "Refresh asks anew");
        state.stale = false;
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
        let newsletter = demo.categories.first().expect("a category");
        assert_eq!(newsletter.days.len(), demo.days.len(), "issue #485: per day too");
    }
}
