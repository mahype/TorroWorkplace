//! Insights (task E8.8b in `docs/tasks/eingang.md`; `docs/eingang.md`, "Drei Fenster", "Insights"):
//! the hub's page of what the inbox counted, so cutting back does not rest on a feeling.
//!
//! - **Span:** Week | Month | Quarter ([`RANGES`]), asked as `inbox.get_stats` with the local day
//!   as `until` ([`StatsSource`]). The window keeps the span and the table switch while it runs
//!   ([`Insights`]).
//! - **Four figures** ([`figures`]): what came in, the popups, the share opened and the median time
//!   until done.
//! - **Per day by source** ([`day_columns`]): stacked columns of what came in, one series per
//!   origin ([`series`]: all mail, all agents, the two busiest other sources – a script, or a service
//!   such as GitHub with all its accounts, task E5.15 –, the rest as "Other"), each
//!   in its chart colour (`torro_gui::theme::ChartColor`), with a legend and a tooltip per column.
//!   A quarter has a column per week ([`buckets`]), so its columns stay readable.
//! - **By hour of day** ([`hour_columns`]): the busy hours, all sources together.
//! - **Where you could cut back** ([`cuts`]): the core's suggestions – much came in, little was
//!   opened –, each with its ready suggestion as button ([`suggestion`], task E8.12: "Sender to Info",
//!   "Mute app" …), which mutes it (`inbox.mute_item` with the suggestion's scope and effect, the
//!   "For" of the window as in the bell's menu; the toast offers the undo). At most a reading width.
//! - **Table view** ([`day_table`], [`hour_table`]): the same numbers as the charts, as tables, for
//!   reading them exactly and where colour does not reach.
//! - **Sizes:** wide puts the two charts side by side and the figures in a row; medium stacks the
//!   charts and puts the figures two by two (task E8.12); narrow puts the span and the table switch
//!   at the top of the page, the figures two by two and every suggestion's numbers under its name.
//!   Mini and strip show the glance, as always.
//!
//! The page only lays out: the app asks for the numbers and sends the muting. Values of traits are
//! foreign text (`untrusted`) and are shown as plain text.

use std::future::Future;

use chrono::{Days, NaiveDate};
use iced::widget::{column, container, row, rule, scrollable};
use iced::{Alignment, Element, Fill, Length};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, ChartColor, palette, size, space};
use torro_gui::widgets::{self, ChartColumn, ChartSeries, Mark};
use torro_gui::{datetime, style, typo};
use torro_inbox::demo::DemoSource;
use torro_inbox::item::{FacetKey, MuteEffect, MuteScope};
use torro_inbox::{InboxError, InboxSource, ItemFilter, SourceKind, SourceRef};
use torro_ipc::inbox::InboxClient;
use torro_ipc::{InboxStats, SourceStats, StatsCounts, StatsDay, StatsRange, StatsSuggestion};

use crate::app::Message;
use crate::{hub, list};

/// What the window asks for Insights. One method, one request of the app key; the core counts, keeps
/// the rights and builds the suggestions, a source never does it itself.
pub trait StatsSource: Send + Sync {
    /// `inbox.get_stats`: what the core counted over `range`, the last day `until` (a local day;
    /// `None` today) – the days with their sources, the busy hours, the sums, the median time until
    /// done and the suggestions ([`InboxStats`]). Errors as the inbox maps them ([`InboxError`]).
    fn get_stats(
        &self,
        range: StatsRange,
        until: Option<NaiveDate>,
    ) -> impl Future<Output = Result<InboxStats, InboxError>> + Send;
}

impl StatsSource for InboxClient {
    async fn get_stats(&self, range: StatsRange, until: Option<NaiveDate>) -> Result<InboxStats, InboxError> {
        InboxClient::get_stats(self, range, until).await
    }
}

/// The demo keeps no statistics: it makes up a plausible span around its "now" from its sources,
/// and suggestions for its own entries, so the buttons mute real demo entries.
impl StatsSource for DemoSource {
    async fn get_stats(&self, range: StatsRange, until: Option<NaiveDate>) -> Result<InboxStats, InboxError> {
        let sources: Vec<SourceRef> = self.list_sources().await?.into_iter().map(|info| info.source).collect();
        // The app passes its day (the demo's `--now`); without it, today.
        let until = until.unwrap_or_else(|| chrono::Local::now().date_naive());
        let mut stats = demo_stats(range, until, &sources);
        stats.suggestions = demo_suggestions(self, range).await;
        Ok(stats)
    }
}

#[cfg(test)]
impl StatsSource for crate::empty::EmptySource {
    async fn get_stats(&self, range: StatsRange, until: Option<NaiveDate>) -> Result<InboxStats, InboxError> {
        let until = until.unwrap_or_default();
        Ok(demo_stats(range, until, &[]))
    }
}

/// What a figure shows when there is nothing to measure.
const DASH: &str = "–";

/// The spans to choose, in the order of the switch.
pub const RANGES: [StatsRange; 3] = [StatsRange::Week, StatsRange::Month, StatsRange::Quarter];

/// The word of a span in the switch.
pub fn range_name(range: StatsRange) -> &'static str {
    match range {
        StatsRange::Week => tr("Week"),
        StatsRange::Month => tr("Month"),
        StatsRange::Quarter => tr("Quarter"),
    }
}

/// The page's window state: whether it shows, its span, the table switch and the numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Insights {
    /// The hub shows Insights instead of "Act now" (its sidebar entry).
    pub shown: bool,
    pub range: StatsRange,
    /// "Table view": the numbers as tables instead of charts.
    pub table: bool,
    /// The answer for `range`: `None` while it loads.
    pub stats: Option<Result<InboxStats, String>>,
}

impl Default for Insights {
    fn default() -> Insights {
        Insights { shown: false, range: StatsRange::Week, table: false, stats: None }
    }
}

/// An origin the charts show as one series (task E8.8b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    /// Every mail account together.
    Mail,
    /// Every coding agent together.
    Agents,
    /// One of the two busiest other sources of the span (task E5.15): a script by its client's name
    /// ([`hub::Origin::Script`]) or a service with all its accounts ([`hub::Origin::Service`]).
    Source(hub::Origin),
    /// The suite and every other source.
    Other,
}

/// A series of the charts: what it groups, its name and its colour slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    pub group: Group,
    pub name: String,
    pub color: ChartColor,
}

/// The origin `source` counts under besides mail, agents and the suite: a script by its name, a
/// service with all its accounts ([`hub::Origin`], the hub's tiles count the same way).
fn origin_of(source: &SourceRef) -> Option<hub::Origin> {
    match source.kind {
        SourceKind::Mail | SourceKind::Agent | SourceKind::Suite => None,
        SourceKind::Script => Some(hub::Origin::Script(source.name.clone())),
        kind @ (SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy) => {
            Some(hub::Origin::Service(kind))
        }
    }
}

/// The series of `stats`, by what came in over the span: Mail (slot 1), Agents (slot 2), the two
/// busiest other sources (slots 3 and 4, the busier first, then by name; a script, or a service such
/// as GitHub with all its accounts, task E5.15) and "Other" (the suite and the remaining sources),
/// each only when something of it came in. Mail and agents keep their colour whatever else there
/// is; only the slots of the other sources follow the span.
pub fn series(stats: &InboxStats) -> Vec<Series> {
    let (mut mail, mut agents, mut other) = (0u32, 0u32, 0u32);
    let mut origins: Vec<(hub::Origin, u32)> = Vec::new();
    for counts in stats.days.iter().flat_map(|day| &day.sources) {
        let new = counts.counts.new;
        match (counts.source.kind, origin_of(&counts.source)) {
            (SourceKind::Mail, _) => mail = mail.saturating_add(new),
            (SourceKind::Agent, _) => agents = agents.saturating_add(new),
            (_, None) => other = other.saturating_add(new),
            (_, Some(origin)) => match origins.iter_mut().find(|(known, _)| *known == origin) {
                Some((_, sum)) => *sum = sum.saturating_add(new),
                None => origins.push((origin, new)),
            },
        }
    }
    origins.retain(|(_, sum)| *sum > 0);
    origins.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name().cmp(&b.0.name())));
    let rest: u32 = origins.iter().skip(2).map(|(_, sum)| *sum).sum();
    other = other.saturating_add(rest);
    let mut series = Vec::new();
    if mail > 0 {
        series.push(Series { group: Group::Mail, name: tr("Mail").into(), color: ChartColor::Series(0) });
    }
    if agents > 0 {
        series.push(Series { group: Group::Agents, name: tr("Agents").into(), color: ChartColor::Series(1) });
    }
    for (slot, (origin, _)) in origins.into_iter().take(2).enumerate() {
        let name = origin.name();
        series.push(Series { group: Group::Source(origin), name, color: ChartColor::Series(2 + slot) });
    }
    if other > 0 {
        series.push(Series { group: Group::Other, name: tr("Other").into(), color: ChartColor::Other });
    }
    series
}

/// The series `source` counts in, as an index into `series`; `None` when it has none (nothing of
/// it came in).
fn series_of(series: &[Series], source: &SourceRef) -> Option<usize> {
    let find = |group: &Group| series.iter().position(|series| series.group == *group);
    match (source.kind, origin_of(source)) {
        (SourceKind::Mail, _) => find(&Group::Mail),
        (SourceKind::Agent, _) => find(&Group::Agents),
        (_, None) => find(&Group::Other),
        (_, Some(origin)) => find(&Group::Source(origin)).or_else(|| find(&Group::Other)),
    }
}

/// One column of the day chart and one row of its table: a day, or a week of a quarter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub from: NaiveDate,
    pub until: NaiveDate,
    /// What came in, per series of [`series`].
    pub values: Vec<u32>,
}

/// The days of `stats` in columns: one per day for a week and a month, one per seven days for a
/// quarter, counted back from `until` (the first holds what is left over). What came in per series.
pub fn buckets(stats: &InboxStats, series: &[Series]) -> Vec<Bucket> {
    let size = if stats.range == StatsRange::Quarter { 7 } else { 1 };
    let mut buckets: Vec<Bucket> = stats
        .days
        .rchunks(size)
        .filter_map(|days| {
            let (first, last) = (days.first()?, days.last()?);
            let mut values = vec![0u32; series.len()];
            for counts in days.iter().flat_map(|day| &day.sources) {
                if let Some(slot) = series_of(series, &counts.source).and_then(|i| values.get_mut(i)) {
                    *slot = slot.saturating_add(counts.counts.new);
                }
            }
            Some(Bucket { from: first.day, until: last.day, values })
        })
        .collect();
    buckets.reverse();
    buckets
}

/// The label under a bucket's column: the weekday for a week, the day of the month for a month,
/// the first day for a week of a quarter.
fn bucket_label(bucket: &Bucket, range: StatsRange) -> String {
    match range {
        StatsRange::Week => datetime::weekday_name(bucket.until, true).to_owned(),
        StatsRange::Month => chrono::Datelike::day(&bucket.until).to_string(),
        StatsRange::Quarter => datetime::format_day_month(bucket.from),
    }
}

/// A bucket's name in its tooltip and table row: the day ("Tue, 13 Oct"), or the days of a week
/// ("7 Oct – 13 Oct").
fn bucket_title(bucket: &Bucket, today: NaiveDate) -> String {
    if bucket.from == bucket.until {
        datetime::format_short_date(bucket.until, today)
    } else {
        span_words(bucket.from, bucket.until)
    }
}

/// "7 Oct – 13 Oct".
fn span_words(from: NaiveDate, until: NaiveDate) -> String {
    trf(
        "{from} – {until}",
        &[("from", &datetime::format_day_month(from)), ("until", &datetime::format_day_month(until))],
    )
}

/// The span `stats` counted, for the header: "7 Oct – 13 Oct".
pub fn span_text(stats: &InboxStats) -> String {
    span_words(stats.from, stats.until)
}

/// The columns of the day chart: what came in per bucket and series ([`buckets`]).
pub fn day_columns(stats: &InboxStats, series: &[Series], today: NaiveDate) -> Vec<ChartColumn> {
    buckets(stats, series)
        .into_iter()
        .map(|bucket| ChartColumn {
            label: bucket_label(&bucket, stats.range),
            title: bucket_title(&bucket, today),
            values: bucket.values,
        })
        .collect()
}

/// The columns of the hour chart: what came in in each of the 24 local hours over the span.
pub fn hour_columns(stats: &InboxStats) -> Vec<ChartColumn> {
    (0..24u32)
        .map(|hour| ChartColumn {
            label: hour.to_string(),
            title: hour_span(hour),
            values: vec![stats.hours.get(hour as usize).copied().unwrap_or(0)],
        })
        .collect()
}

/// "09:00–10:00".
fn hour_span(hour: u32) -> String {
    format!("{hour:02}:00–{:02}:00", (hour + 1) % 24)
}

/// The day chart's numbers as a table: a row per bucket with its name, a value per series and the
/// total, then the sums. The same numbers as [`day_columns`].
pub fn day_table(
    stats: &InboxStats,
    series: &[Series],
    today: NaiveDate,
) -> (Vec<String>, Vec<Vec<String>>, Vec<String>) {
    let first = if stats.range == StatsRange::Quarter { tr("Week") } else { tr("Day") };
    let mut head = vec![first.to_owned()];
    head.extend(series.iter().map(|series| series.name.clone()));
    head.push(tr("Total").to_owned());
    let columns = day_columns(stats, series, today);
    let mut sums = vec![0u32; series.len()];
    let rows = columns
        .iter()
        .map(|column| {
            for (sum, value) in sums.iter_mut().zip(&column.values) {
                *sum = sum.saturating_add(*value);
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

/// The hour chart's numbers as a table: a row per hour with what came in, then the sum. The same
/// numbers as [`hour_columns`].
pub fn hour_table(stats: &InboxStats) -> (Vec<String>, Vec<Vec<String>>, Vec<String>) {
    let head = vec![tr("Hour").to_owned(), tr("Came in").to_owned()];
    let rows = hour_columns(stats)
        .into_iter()
        .map(|column| {
            let total = column.total().to_string();
            vec![column.title, total]
        })
        .collect();
    let total = vec![tr("Total").to_owned(), stats.hours.iter().sum::<u32>().to_string()];
    (head, rows, total)
}

/// One of the four figures: its label, value and the note under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    pub label: &'static str,
    pub value: String,
    pub note: String,
}

/// The four figures of `stats`: what came in (with the average per day), the popups (with those
/// quiet held back), the share opened (with what was dismissed and muted) and the median time until
/// done (with what was done and made a task).
pub fn figures(stats: &InboxStats) -> [Figure; 4] {
    let t = &stats.totals;
    let days = u32::try_from(stats.days.len()).unwrap_or(1).max(1);
    let per_day = (f64::from(t.new) / f64::from(days)).round() as u32;
    [
        Figure { label: tr("Came in"), value: t.new.to_string(), note: trf("{n} a day", &[("n", &per_day)]) },
        Figure {
            label: tr("Popups"),
            value: t.popups.to_string(),
            note: trf("{n} held back while busy", &[("n", &t.quiet)]),
        },
        Figure {
            label: tr("Opened"),
            value: percent(t.opened, t.new),
            note: trf("{dismissed} dismissed · {muted} muted", &[("dismissed", &t.dismissed), ("muted", &t.muted)]),
        },
        Figure {
            label: tr("Until done (median)"),
            value: stats.median_done_minutes.map_or_else(|| DASH.to_owned(), waited),
            note: trf("{done} done · {tasks} made a task", &[("done", &t.done), ("tasks", &t.tasks)]),
        },
    ]
}

/// `part` of `whole` as a whole percentage, at most 100; "–" when nothing came. English writes
/// "33%" (task E8.12), German "33 %" – the translation of "{n}%" decides.
pub fn percent(part: u32, whole: u32) -> String {
    if whole == 0 {
        return DASH.to_owned();
    }
    let share = (f64::from(part.min(whole)) * 100.0 / f64::from(whole)).round() as u32;
    trf("{n}%", &[("n", &share)])
}

/// A time until done in minutes as short as it gets: "15 min", "2 h", "7 d".
pub fn waited(minutes: u32) -> String {
    const HOUR: u32 = 60;
    const DAY: u32 = 24 * HOUR;
    if minutes < HOUR {
        trf("{n} min", &[("n", &minutes)])
    } else if minutes < DAY {
        trf("{n} h", &[("n", &minutes.div_ceil(HOUR))])
    } else {
        trf("{n} d", &[("n", &minutes.div_ceil(DAY))])
    }
}

/// The hour most came in, for the sentence under the hour chart; `None` when nothing came.
pub fn busiest_hour(stats: &InboxStats) -> Option<u32> {
    let (hour, most) = stats.hours.iter().enumerate().max_by_key(|(hour, n)| (**n, std::cmp::Reverse(*hour)))?;
    (*most > 0).then(|| u32::try_from(hour).unwrap_or(0))
}

/// A suggestion as the page shows it: what it mutes, how much came and was opened, the word of its
/// button and what the button sends.
#[derive(Debug, Clone)]
pub struct Cut {
    pub label: String,
    pub source: SourceRef,
    pub new: u32,
    pub opened: u32,
    /// The ready suggestion ([`suggestion`]): "Sender to Info", "Mute app" …
    pub word: &'static str,
    pub mute: Message,
}

/// The ready suggestion of a row of "Where you could cut back" for muting with `scope` (task E8.12,
/// finding 14; `docs/eingang.md`, "Insights": "Mute channel", "Sender to Info"): what muting does and
/// the word of the button that does it. The core suggests what to mute (`inbox.get_stats` names the
/// trait and its scope); what fits it is the page's: mail that is seldom read goes to "Info" – its
/// sender, domain, list, newsletters; mail is never hidden (issue #339), a filter moves it instead –,
/// everything else keeps coming without a popup ("Mute …"), so no report is lost.
pub fn suggestion(scope: MuteScope) -> (MuteEffect, &'static str) {
    let MuteScope::Facet { key, .. } = scope else { return (MuteEffect::NoPopup, tr("Mute conversation")) };
    match key {
        FacetKey::Sender => (MuteEffect::Info, tr("Sender to Info")),
        FacetKey::Domain => (MuteEffect::Info, tr("Domain to Info")),
        FacetKey::List => (MuteEffect::Info, tr("List to Info")),
        FacetKey::Bulk => (MuteEffect::Info, tr("Newsletters to Info")),
        FacetKey::Account => (MuteEffect::NoPopup, tr("Mute account")),
        FacetKey::Agent => (MuteEffect::NoPopup, tr("Mute agent")),
        FacetKey::Project => (MuteEffect::NoPopup, tr("Mute project")),
        FacetKey::Kind => (MuteEffect::NoPopup, tr("Mute these")),
        FacetKey::Client => (MuteEffect::NoPopup, tr("Mute app")),
        FacetKey::Conversation => (MuteEffect::NoPopup, tr("Mute conversation")),
        FacetKey::Repo
        | FacetKey::Reason
        | FacetKey::Result
        | FacetKey::App
        | FacetKey::Person
        | FacetKey::Topic
        | FacetKey::Priority
        | FacetKey::Tag => (MuteEffect::NoPopup, tr("Mute these")),
    }
}

/// The core's suggestions as rows, in its order (the most first): each button sends
/// `inbox.mute_item` for the suggestion's entry and scope with the effect of its ready suggestion
/// ([`suggestion`], [`Message::MuteSuggested`]), for as long as the window's "For" says.
pub fn cuts(stats: &InboxStats) -> Vec<Cut> {
    stats
        .suggestions
        .iter()
        .map(|suggestion: &StatsSuggestion| {
            let scope = suggestion.option.scope;
            let (effect, word) = self::suggestion(scope);
            Cut {
                label: list::one_line(&suggestion.option.label),
                source: suggestion.source.clone(),
                new: suggestion.new,
                opened: suggestion.opened,
                word,
                mute: Message::MuteSuggested(suggestion.item.clone(), scope, effect),
            }
        })
        .collect()
}

/// What the page needs from the window.
pub struct InsightsData<'a> {
    pub insights: &'a Insights,
    pub class: SizeClass,
    /// The local day, for the dates.
    pub today: NaiveDate,
}

/// Whether `class` is one of the narrow layouts: the span and the switch go on the page there.
fn is_narrow(class: SizeClass) -> bool {
    !matches!(class, SizeClass::Wide | SizeClass::Medium)
}

/// The span switch and "Table view", for the header bar (wide, medium) or the top of the page.
pub fn controls<'a>(insights: &Insights) -> Vec<Element<'a, Message>> {
    let chosen = RANGES.iter().position(|&range| range == insights.range).unwrap_or_default();
    let ranges = widgets::segmented(RANGES.map(|range| (range_name(range), Message::StatsRange(range))), chosen);
    let table = widgets::choice_chip(
        tr("Table view"),
        Some(Mark::Icon(Icon::Table)),
        insights.table,
        Some(Message::StatsTable(!insights.table)),
    );
    vec![ranges, table]
}

/// The page: loading, the error with "Retry", the empty span, or the figures, the charts (or their
/// tables) and the suggestions, at most two reading widths wide and scrolling.
pub fn view(data: InsightsData<'_>) -> Element<'_, Message> {
    let narrow = is_narrow(data.class);
    let stats = match &data.insights.stats {
        None => return widgets::loading(),
        Some(Err(_)) => return widgets::error_state(tr("Couldn't load the numbers."), Message::Refresh),
        Some(Ok(stats)) => stats,
    };
    let mut blocks: Vec<Element<'_, Message>> = Vec::new();
    if narrow {
        blocks.push(row(controls(data.insights)).spacing(space::s()).align_y(Alignment::Center).into());
    }
    if stats.totals.new == 0 && stats.suggestions.is_empty() {
        blocks.push(widgets::empty_state(
            tr("Nothing counted yet."),
            tr("What comes in, what you open and what you mute shows up here."),
        ));
        return page(blocks, narrow);
    }
    blocks.push(figure_tiles(stats, figures_two_by_two(data.class)));
    let series = series(stats);
    let (days, hours) = if data.insights.table {
        (day_table_card(stats, &series, data.today), hour_table_card(stats))
    } else {
        (day_chart_card(stats, series, data.today), hour_chart_card(stats))
    };
    blocks.push(if data.class == SizeClass::Wide {
        row![container(days).width(Length::FillPortion(2)), container(hours).width(Length::FillPortion(1))]
            .spacing(size::card_gap())
            .into()
    } else {
        column![days, hours].spacing(size::card_gap()).into()
    });
    // At most a reading width, as the hub's To sort (task E8.12, finding 8): its button would stand
    // far from what it mutes.
    blocks.push(container(cut_back(stats, narrow)).width(Fill).max_width(size::content_max()).into());
    page(blocks, narrow)
}

/// `blocks` one under the other with the section gap, inside the hub's inset, scrolling, at most
/// the hub's width.
fn page(blocks: Vec<Element<'_, Message>>, narrow: bool) -> Element<'_, Message> {
    let gap = if narrow { space::l() } else { layout::section_gap() };
    let content = container(column(blocks).spacing(gap).width(Fill)).padding(hub::inset(narrow));
    let scrolled = scrollable(content)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill);
    container(scrolled).width(Fill).max_width(hub::max_width()).height(Fill).into()
}

/// Whether the four figures stand two by two in a window of `class`: below the wide layout (task
/// E8.12, finding 13) – four in a row of a medium window squeeze their notes onto two lines.
fn figures_two_by_two(class: SizeClass) -> bool {
    class != SizeClass::Wide
}

/// The four figures: in one row, or two by two (`two_by_two`, [`figures_two_by_two`]).
fn figure_tiles<'a>(stats: &InboxStats, two_by_two: bool) -> Element<'a, Message> {
    let tiles: Vec<Element<'a, Message>> =
        figures(stats).into_iter().map(|figure| widgets::stat_tile(figure.label, figure.value, figure.note)).collect();
    let gap = size::card_gap();
    if !two_by_two {
        return row(tiles).spacing(gap).into();
    }
    let mut tiles = tiles.into_iter();
    let mut lines = column![].spacing(gap);
    while let Some(first) = tiles.next() {
        let mut line = row![first].spacing(gap);
        if let Some(second) = tiles.next() {
            line = line.push(second);
        }
        lines = lines.push(line);
    }
    lines.into()
}

/// The title of the day chart and its table.
fn day_title(range: StatsRange) -> &'static str {
    if range == StatsRange::Quarter { tr("Per week, by source") } else { tr("Per day, by source") }
}

/// The stacked columns of what came in, with the legend over them for two series or more.
fn day_chart_card<'a>(stats: &InboxStats, series: Vec<Series>, today: NaiveDate) -> Element<'a, Message> {
    let columns = day_columns(stats, &series, today);
    let series: Vec<ChartSeries> =
        series.into_iter().map(|series| ChartSeries { name: series.name, color: series.color }).collect();
    let mut body = column![].spacing(space::m());
    if series.len() > 1 {
        body = body.push(widgets::chart_legend(&series));
    }
    body = body.push(widgets::column_chart(series, columns));
    widgets::chart_card(day_title(stats.range), tr("Came in"), body)
}

/// The busy hours as columns, with a sentence on the busiest.
fn hour_chart_card<'a>(stats: &InboxStats) -> Element<'a, Message> {
    let one = vec![ChartSeries { name: tr("Came in").into(), color: ChartColor::Series(0) }];
    let mut body = column![widgets::column_chart(one, hour_columns(stats))].spacing(space::m());
    if let Some(hour) = busiest_hour(stats) {
        // A label, not a sentence: no full stop (task E8.12).
        let words = trf("Busiest: {time}", &[("time", &hour_span(hour))]);
        body = body.push(typo::text(typo::meta(), words).style(theme::text_2));
    }
    widgets::chart_card(tr("By hour of day"), tr("All sources"), body)
}

fn day_table_card<'a>(stats: &InboxStats, series: &[Series], today: NaiveDate) -> Element<'a, Message> {
    let (head, rows, total) = day_table(stats, series, today);
    widgets::chart_card(day_title(stats.range), tr("Came in"), widgets::data_table(head, rows, Some(total)))
}

fn hour_table_card<'a>(stats: &InboxStats) -> Element<'a, Message> {
    let (head, rows, total) = hour_table(stats);
    widgets::chart_card(tr("By hour of day"), tr("All sources"), widgets::data_table(head, rows, Some(total)))
}

/// "Where you could cut back": the suggestions with their numbers and buttons, or a line that
/// there are none.
fn cut_back<'a>(stats: &InboxStats, narrow: bool) -> Element<'a, Message> {
    let head = row![
        typo::text(typo::body_strong(), tr("Where you could cut back")).width(Fill),
        typo::text(typo::meta(), tr("Much came in, little was opened")).style(theme::text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let mut block = column![head].spacing(space::s());
    let cuts = cuts(stats);
    if cuts.is_empty() {
        let none = tr("Nothing stands out: what comes in, you mostly open.");
        return block.push(typo::text(typo::body(), none).style(theme::text_2)).into();
    }
    if !narrow {
        block = block.push(cut_head());
    }
    for cut in cuts {
        block = block.push(rule::horizontal(size::HAIRLINE).style(style::rule));
        block = block.push(if narrow { cut_card(cut) } else { cut_row(cut) });
    }
    block.into()
}

/// The captions over the suggestions' columns (wide, medium).
fn cut_head<'a>() -> Element<'a, Message> {
    let caption = |words: &'static str| typo::text(typo::caption(), words).style(theme::text_3);
    container(
        row![
            caption(tr("What")).width(Length::FillPortion(5)),
            container(caption(tr("Came in"))).width(Length::FillPortion(1)).align_right(Length::FillPortion(1)),
            caption(tr("Opened")).width(Length::FillPortion(2)),
            container(caption("")).width(Length::FillPortion(2)),
        ]
        .spacing(space::l()),
    )
    .padding(layout::row_padding())
    .into()
}

/// The icon of a suggestion's source, quiet.
fn source_mark<'a>(source: &SourceRef) -> Element<'a, Message> {
    icon::icon(list::source_icon(source), icon::SIZE)
        .style(|t, _| iced::widget::svg::Style { color: Some(palette(t).text_3) })
        .into()
}

/// A suggestion in one line (wide, medium): icon and what it mutes, what came in, the share opened
/// with its bar, and the button.
fn cut_row<'a>(cut: Cut) -> Element<'a, Message> {
    let what = row![
        source_mark(&cut.source),
        container(widgets::ellipsized(typo::body(), cut.label, theme::text)).width(Fill)
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let new = typo::text(typo::body().tabular(), cut.new.to_string());
    let opened = row![
        widgets::share_bar(cut.opened, cut.new),
        typo::text(typo::meta().tabular(), percent(cut.opened, cut.new)).style(theme::text_2)
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let line = row![
        container(what).width(Length::FillPortion(5)),
        container(new).width(Length::FillPortion(1)).align_right(Length::FillPortion(1)),
        container(opened).width(Length::FillPortion(2)),
        container(mute_button(cut.mute, cut.word)).width(Length::FillPortion(2)).align_right(Length::FillPortion(2)),
    ]
    .spacing(space::l())
    .align_y(Alignment::Center);
    container(line).center_y(size::header()).padding(cut_row_padding(layout::row_padding())).into()
}

/// The padding of a suggestion's row ([`cut_row`]) from the row padding `pad`: only its sides. The
/// row is [`size::header()`] high and centres its content, so padding above and below would only
/// squeeze the "Mute" button (18 above and below a row of 64 left the button of 36 only 28).
fn cut_row_padding(pad: iced::Padding) -> iced::Padding {
    iced::Padding::new(0.0).left(pad.left).right(pad.right)
}

/// A suggestion where it is narrow: what it mutes, its numbers below, the button at the right.
fn cut_card<'a>(cut: Cut) -> Element<'a, Message> {
    let numbers =
        trf("{new} came in · {opened} opened", &[("new", &cut.new), ("opened", &percent(cut.opened, cut.new))]);
    let words = column![
        container(widgets::ellipsized(typo::body(), cut.label, theme::text)).width(Fill),
        typo::text(typo::meta(), numbers).style(theme::text_3),
    ];
    let line = row![source_mark(&cut.source), container(words).width(Fill), mute_button(cut.mute, cut.word)]
        .spacing(space::m())
        .align_y(Alignment::Center);
    container(line).padding([space::s(), 0.0]).into()
}

/// The button of a suggestion: the bell and its ready suggestion ("Sender to Info", [`suggestion`]).
fn mute_button<'a>(mute: Message, word: &'static str) -> Element<'a, Message> {
    widgets::text_button(Some(Icon::BellOff), word, style::button::secondary, Some(mute))
}

// ---------------------------------------------------------------------------------------------
// Demo numbers
// ---------------------------------------------------------------------------------------------

/// Weights of the local hours for the demo's reports: quiet nights, peaks at 9–11 and 14–16.
const DEMO_HOURS: [u32; 24] = [0, 0, 0, 0, 0, 0, 1, 3, 6, 10, 11, 8, 4, 6, 9, 10, 7, 5, 3, 2, 2, 1, 1, 0];

/// A made-up span of `range` up to `until` for `sources` (the demo has no statistics): weekdays
/// busier than weekends, mail the most, scripts often dismissed. The same numbers every time.
pub fn demo_stats(range: StatsRange, until: NaiveDate, sources: &[SourceRef]) -> InboxStats {
    let count = range.days();
    let from = until.checked_sub_days(Days::new(u64::from(count.saturating_sub(1)))).unwrap_or(until);
    let mut totals = StatsCounts::default();
    let days: Vec<StatsDay> = (0..count)
        .filter_map(|i| from.checked_add_days(Days::new(u64::from(i))))
        .map(|day| {
            let weekend = chrono::Datelike::weekday(&day).num_days_from_monday() >= 5;
            let sources: Vec<SourceStats> = sources
                .iter()
                .enumerate()
                .map(|(j, source)| {
                    let counts = demo_counts(source, day, j, weekend);
                    totals.add(&counts);
                    SourceStats { source: source.clone(), counts }
                })
                .filter(|stats| stats.counts != StatsCounts::default())
                .collect();
            StatsDay { day, sources }
        })
        .collect();
    InboxStats {
        range,
        from,
        until,
        days,
        hours: spread(totals.new, &DEMO_HOURS),
        totals,
        median_done_minutes: (totals.done > 0).then_some(15),
        suggestions: Vec::new(),
    }
}

/// One demo source's counts on `day`.
fn demo_counts(source: &SourceRef, day: NaiveDate, index: usize, weekend: bool) -> StatsCounts {
    let base: u32 = match source.kind {
        SourceKind::Mail => 14,
        SourceKind::Agent => 6,
        SourceKind::Script | SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy
            if index.is_multiple_of(2) =>
        {
            9
        }
        SourceKind::Script | SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy => 4,
        SourceKind::Suite => 2,
    };
    let wobble = (chrono::Datelike::ordinal(&day) * 7 + index as u32 * 13) % 6;
    let new = if weekend { (base + wobble) / 4 } else { base + wobble };
    let (opened, dismissed) = match source.kind {
        SourceKind::Mail => (new / 2, 0),
        SourceKind::Agent => (new * 4 / 5, 0),
        SourceKind::Script | SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy => {
            (new / 8, new / 2)
        }
        SourceKind::Suite => (new, 0),
    };
    let popups = new / 3;
    StatsCounts {
        new,
        opened,
        dismissed,
        done: new / 3,
        snoozed: new / 10,
        tasks: new / 12,
        muted: new / 9,
        popups,
        quiet: popups / 4,
    }
}

/// `total` spread over the hours by `weights`; what rounding leaves goes to the heaviest hour, so
/// the hours add up to `total`.
fn spread(total: u32, weights: &[u32; 24]) -> Vec<u32> {
    let sum: u32 = weights.iter().sum();
    let mut hours: Vec<u32> = weights.iter().map(|&w| (total * w).checked_div(sum).unwrap_or(0)).collect();
    let left = total - hours.iter().sum::<u32>();
    if let Some(peak) = weights.iter().enumerate().max_by_key(|(_, w)| **w).map(|(i, _)| i) {
        hours[peak] += left;
    }
    hours
}

/// Suggestions for the demo's own entries: a newsletter's sender, a chatty script, finished agents –
/// each with the way to mute it that the demo offers for the entry.
async fn demo_suggestions(demo: &DemoSource, range: StatsRange) -> Vec<StatsSuggestion> {
    let Ok(items) = demo.list_items(ItemFilter::default()).await else { return Vec::new() };
    let weeks = range.days().div_ceil(7);
    let wanted = [(FacetKey::List, 22), (FacetKey::Client, 48), (FacetKey::Kind, 12)];
    let mut suggestions = Vec::new();
    for (key, per_week) in wanted {
        for item in &items {
            let Ok(options) = demo.list_mute_options(item.id.clone()).await else { continue };
            let found = options.into_iter().find(|option| option.scope == MuteScope::Facet { key, nth: 0 });
            if let Some(option) = found {
                let new = per_week * weeks;
                let opened = new / 20;
                suggestions.push(StatsSuggestion {
                    item: item.id.clone(),
                    source: item.source.clone(),
                    option,
                    new,
                    opened,
                });
                break;
            }
        }
    }
    suggestions
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use iced::futures::executor::block_on;
    use torro_core::i18n::tr;

    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().expect("a day")
    }

    fn sources() -> Vec<SourceRef> {
        vec![
            SourceRef::mail("anna@example.org"),
            SourceRef::mail("team@example.org"),
            SourceRef::agent("claude-code"),
            SourceRef::script("ci"),
            SourceRef::script("deploy"),
            SourceRef::script("backup"),
            SourceRef::suite(),
        ]
    }

    /// Issue #291 (review of the three designs): a suggestion's row of a fixed height takes only the
    /// sides of the row padding, so its "Mute" button keeps its height in every design (with 18 above
    /// and below a row of 64 squeezed the button of 36 to 28).
    #[test]
    fn a_suggestions_row_holds_its_mute_button_in_every_design() {
        for id in torro_core::design::BUILT_IN_IDS {
            let d = torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design");
            let pad = d.layout.row_padding;
            let full =
                iced::Padding { top: pad.vertical, right: pad.horizontal, bottom: pad.vertical, left: pad.horizontal };
            let sides = cut_row_padding(full);
            assert_eq!((sides.left, sides.right), (pad.horizontal, pad.horizontal), "{id}: the sides stay");
            let inner = d.size.header - sides.top - sides.bottom;
            assert!(inner >= d.size.control_sm, "{id}: {inner} inside the row holds the button");
        }
    }

    #[test]
    fn series_group_mail_and_agents_and_keep_the_two_busiest_scripts() {
        let stats = demo_stats(StatsRange::Week, day("2026-10-13"), &sources());
        let series = series(&stats);
        let groups: Vec<&Group> = series.iter().map(|series| &series.group).collect();
        assert_eq!(groups.len(), 5, "{groups:?}");
        assert_eq!((groups[0], groups[1], groups[4]), (&Group::Mail, &Group::Agents, &Group::Other));
        assert!(matches!(groups[2], Group::Source(_)) && matches!(groups[3], Group::Source(_)));
        let colours: Vec<ChartColor> = series.iter().map(|series| series.color).collect();
        assert_eq!(
            colours,
            [0, 1, 2, 3].map(ChartColor::Series).into_iter().chain([ChartColor::Other]).collect::<Vec<_>>()
        );
        assert_eq!(series[4].name, tr("Other"));
    }

    /// Task E5.15: Mail, Agents, then the two busiest other sources – a service with all its
    /// accounts counts as one –, then "Other" with the suite and the rest.
    #[test]
    fn a_service_is_one_series_with_all_its_accounts() {
        let sources = [
            SourceRef::mail("anna@example.org"),
            SourceRef::agent("claude-code"),
            SourceRef::github("anna-berg"),
            SourceRef::script("ci"),
            SourceRef::github("anna-work"),
            SourceRef::ntfy("ntfy.sh"),
            SourceRef::talk("anna@cloud.example.org"),
            SourceRef::suite(),
        ];
        let stats = demo_stats(StatsRange::Week, day("2026-10-13"), &sources);
        let series = series(&stats);
        let groups: Vec<&Group> = series.iter().map(|series| &series.group).collect();
        assert_eq!(groups.len(), 5, "{groups:?}");
        assert_eq!((groups[0], groups[1], groups[4]), (&Group::Mail, &Group::Agents, &Group::Other));
        assert_eq!(groups[2], &Group::Source(hub::Origin::Service(SourceKind::Github)), "both accounts the most");
        assert_eq!(series[2].name, "GitHub");
        assert!(matches!(groups[3], Group::Source(_)));
        // Both accounts land in GitHub's series, Talk in "Other" or its own.
        let github = series_of(&series, &SourceRef::github("anna-work"));
        assert_eq!(github, series_of(&series, &SourceRef::github("anna-berg")));
        assert_eq!(github, Some(2));
        assert_eq!(series_of(&series, &SourceRef::suite()), Some(4));
        let (_, rows, _) = day_table(&stats, &series, day("2026-10-13"));
        assert_eq!(rows.len(), 7, "the table holds the same series");
    }

    #[test]
    fn mail_keeps_its_colour_without_agents() {
        let stats =
            demo_stats(StatsRange::Week, day("2026-10-13"), &[SourceRef::mail("a@b.c"), SourceRef::script("ci")]);
        let series = series(&stats);
        assert_eq!(series.iter().map(|s| s.color).collect::<Vec<_>>(), [ChartColor::Series(0), ChartColor::Series(2)]);
    }

    #[test]
    fn a_quarter_has_a_column_per_week_ending_today() {
        let stats = demo_stats(StatsRange::Quarter, day("2026-10-13"), &sources());
        let series = series(&stats);
        let buckets = buckets(&stats, &series);
        assert_eq!(buckets.len(), 13, "90 days in weeks back from today");
        assert_eq!(buckets.last().map(|b| (b.from, b.until)), Some((day("2026-10-07"), day("2026-10-13"))));
        assert_eq!(buckets[0].from, stats.from, "the first holds what is left over");
        assert_eq!(buckets.iter().flat_map(|b| &b.values).sum::<u32>(), stats.totals.new, "nothing lost");
        let week = demo_stats(StatsRange::Week, day("2026-10-13"), &sources());
        assert_eq!(super::buckets(&week, &super::series(&week)).len(), 7);
    }

    /// The acceptance: the table shows the numbers the charts show.
    #[test]
    fn the_tables_hold_the_numbers_of_the_charts() {
        let today = day("2026-10-13");
        for range in RANGES {
            let stats = demo_stats(range, today, &sources());
            let series = series(&stats);
            let columns = day_columns(&stats, &series, today);
            let (head, rows, total) = day_table(&stats, &series, today);
            assert_eq!(head.len(), series.len() + 2);
            assert_eq!(rows.len(), columns.len());
            for (row, column) in rows.iter().zip(&columns) {
                let mut cells = vec![column.title.clone()];
                cells.extend(column.values.iter().map(u32::to_string));
                cells.push(column.total().to_string());
                assert_eq!(row, &cells, "{range:?}");
            }
            assert_eq!(total.last(), Some(&stats.totals.new.to_string()));

            let (_, hour_rows, hour_total) = hour_table(&stats);
            let hours: Vec<String> = hour_columns(&stats).iter().map(|c| c.total().to_string()).collect();
            assert_eq!(hour_rows.iter().map(|row| row[1].clone()).collect::<Vec<_>>(), hours);
            assert_eq!(hour_total[1], stats.totals.new.to_string(), "the hours add up to what came in");
        }
    }

    #[test]
    fn figures_say_shares_and_times_shortly() {
        assert_eq!(percent(3, 64), trf("{n}%", &[("n", &5)]));
        assert_eq!(percent(9, 4), trf("{n}%", &[("n", &100)]), "never more than all");
        if torro_core::i18n::tr("{n}%") == "{n}%" {
            assert_eq!(percent(1, 3), "33%", "English: no space (task E8.12)");
        }
        assert_eq!(percent(0, 0), DASH);
        assert_eq!(waited(15), trf("{n} min", &[("n", &15)]));
        assert_eq!(waited(120), trf("{n} h", &[("n", &2)]));
        assert_eq!(waited(10080), trf("{n} d", &[("n", &7)]));
        let stats = demo_stats(StatsRange::Week, day("2026-10-13"), &sources());
        let figures = figures(&stats);
        assert_eq!(
            figures.clone().map(|f| f.label),
            [tr("Came in"), tr("Popups"), tr("Opened"), tr("Until done (median)")]
        );
        assert_eq!(figures[0].value, stats.totals.new.to_string());
        assert_eq!(busiest_hour(&stats), Some(10));
        let empty = demo_stats(StatsRange::Week, day("2026-10-13"), &[]);
        assert_eq!((figures_of(&empty), busiest_hour(&empty)), (DASH.to_owned(), None));
    }

    fn figures_of(stats: &InboxStats) -> String {
        figures(stats)[3].value.clone()
    }

    #[test]
    fn the_demo_suggests_its_own_entries_with_a_way_to_mute_them() {
        let demo = DemoSource::new(Utc::now());
        let stats = block_on(demo.get_stats(StatsRange::Month, None)).expect("stats");
        assert_eq!(stats.suggestions.len(), 3, "a newsletter, a script, finished agents");
        for suggestion in &stats.suggestions {
            let options = block_on(demo.list_mute_options(suggestion.item.clone())).expect("options");
            assert!(options.iter().any(|option| option.scope == suggestion.option.scope), "{suggestion:?}");
        }
        let cuts = cuts(&stats);
        assert!(
            matches!(&cuts[0].mute, Message::MuteSuggested(id, scope, _) if *id == stats.suggestions[0].item && *scope == stats.suggestions[0].option.scope)
        );
        // Task E8.12 (finding 14): each row its own ready suggestion, not the window's "Only Info".
        let words: Vec<&str> = cuts.iter().map(|cut| cut.word).collect();
        assert_eq!(words, [tr("List to Info"), tr("Mute app"), tr("Mute these")]);
        let effects: Vec<MuteEffect> = cuts
            .iter()
            .filter_map(|cut| match cut.mute {
                Message::MuteSuggested(_, _, effect) => Some(effect),
                _ => None,
            })
            .collect();
        assert_eq!(effects, [MuteEffect::Info, MuteEffect::NoPopup, MuteEffect::NoPopup]);
    }

    /// Task E8.12 (finding 14): mail seldom read goes to Info (issue #339: never hidden), everything
    /// else only loses its popups – the word says which.
    #[test]
    fn every_trait_has_its_ready_suggestion() {
        let facet = |key| MuteScope::Facet { key, nth: 0 };
        // Issue #339: mail is never hidden; what is seldom read goes to "Info".
        assert_eq!(suggestion(facet(FacetKey::Sender)), (MuteEffect::Info, tr("Sender to Info")));
        assert_eq!(suggestion(facet(FacetKey::Domain)), (MuteEffect::Info, tr("Domain to Info")));
        assert_eq!(suggestion(facet(FacetKey::List)), (MuteEffect::Info, tr("List to Info")));
        assert_eq!(suggestion(facet(FacetKey::Bulk)), (MuteEffect::Info, tr("Newsletters to Info")));
        assert_eq!(suggestion(facet(FacetKey::Client)), (MuteEffect::NoPopup, tr("Mute app")));
        assert_eq!(suggestion(facet(FacetKey::Project)), (MuteEffect::NoPopup, tr("Mute project")));
        assert_eq!(suggestion(MuteScope::Conversation), (MuteEffect::NoPopup, tr("Mute conversation")));
    }

    /// Task E8.12 (finding 13): the figures stand two by two below the wide layout.
    #[test]
    fn the_figures_stand_two_by_two_below_the_wide_layout() {
        assert!(!figures_two_by_two(SizeClass::Wide));
        assert!(figures_two_by_two(SizeClass::Medium) && figures_two_by_two(SizeClass::Narrow));
    }
}
