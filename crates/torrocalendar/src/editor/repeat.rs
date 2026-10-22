//! The repeat of an event in the editor (task TE.7, `docs/termin-editor.md` layout A): the short list
//! the "Repeat" menu offers for a start date, the sentence that names a rule, and the card "Custom
//! repeat" with interval, weekdays, day of the month and end.
//!
//! The short list follows the big calendars (Apple, Google): never, every day, every weekday, every
//! week and every two weeks on the start's weekday, every month on the start's n-th weekday (on the
//! last day when the start is the month's last day; the fifth weekday counts as the last), every year
//! on the start's date, then "Custom…". A rule read from the server with parts the editor does not show
//! (`Repeat::custom`) is named "Custom rule" and can only be replaced whole.
//!
//! Sentences are put together from translated parts, so German reads as German: the every-part
//! ("Every 2 weeks", "Alle 2 Wochen"), then the on-part ("On Tue", "Am Di"), which stands with a
//! capital at the start of a choice in the card and goes lower case behind the every-part, then the end
//! ("until 22 Dec", "10 times"). The functions here are pure; [`super::Editor`] keeps the state.

use chrono::{Datelike, Duration, NaiveDate};
use iced::widget::{column, container, row, rule, text_input};
use iced::{Alignment, Element, Length};
use torro_core::i18n::{tr, trc, trf};
use torro_gui::datetime::{format_date, month_name_short, parse_date, to_local};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::{self, text_width};
use torro_gui::widgets::{self, Mark};
use torro_gui::{layout, style};
use torro_pim::EventTime;
use torro_pim::model::{Repeat, RepeatDay, RepeatEnd, RepeatFreq, Weekday};

use super::Message;

/// The days of the week from Monday, as a rule names them.
const DAYS: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];
/// Their short names in English; `tr` gives the German ones.
const SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Which of [`DAYS`] `day` is.
fn index(day: Weekday) -> usize {
    chrono::Weekday::from(day).num_days_from_monday() as usize
}

/// "Tue" (German "Di").
fn short(day: Weekday) -> &'static str {
    tr(SHORT[index(day)])
}

/// The weekday of `date`, as a rule names it.
fn weekday_of(date: NaiveDate) -> Weekday {
    Weekday::from(date.weekday())
}

/// A rule the editor makes: no stored text, nothing custom, no end.
fn rule_of(freq: RepeatFreq, interval: u16, weekdays: Vec<RepeatDay>, month_days: Vec<i8>) -> Repeat {
    Repeat {
        freq,
        interval,
        weekdays,
        month_days,
        months: Vec::new(),
        end: RepeatEnd::Never,
        rule: String::new(),
        custom: false,
    }
}

/// Every such weekday (`BYDAY=TU`).
fn every(day: Weekday) -> RepeatDay {
    RepeatDay { day, nth: None }
}

/// Whether `date` is the last day of its month.
fn last_of_month(date: NaiveDate) -> bool {
    date.succ_opt().is_none_or(|next| next.month() != date.month())
}

/// Which `date`'s weekday it is in its month: 1 to 4, or -1 for the fifth (which is always the last).
fn nth_in_month(date: NaiveDate) -> i8 {
    match (date.day() - 1) / 7 + 1 {
        5 => -1,
        // At most 4 here, so the cast keeps the value.
        n => n as i8,
    }
}

/// The monthly rule the short list offers for `start`: on the last day when `start` is the month's
/// last day (`BYMONTHDAY=-1`), else on its n-th weekday (`BYDAY=4TU`, the fifth as `-1TU`).
fn monthly(start: NaiveDate) -> Repeat {
    if last_of_month(start) {
        rule_of(RepeatFreq::Monthly, 1, Vec::new(), vec![-1])
    } else {
        let day = RepeatDay { day: weekday_of(start), nth: Some(nth_in_month(start)) };
        rule_of(RepeatFreq::Monthly, 1, vec![day], Vec::new())
    }
}

/// The rules the "Repeat" menu offers for an event starting on `start`, after "Never": every day,
/// every weekday, every week and every two weeks on the start's weekday, every month ([`monthly`]),
/// every year on the start's date (`FREQ=YEARLY`, the date comes from the start).
pub(super) fn shortlist(start: NaiveDate) -> Vec<Repeat> {
    let day = weekday_of(start);
    vec![
        rule_of(RepeatFreq::Daily, 1, Vec::new(), Vec::new()),
        rule_of(RepeatFreq::Weekly, 1, DAYS[..5].iter().copied().map(every).collect(), Vec::new()),
        rule_of(RepeatFreq::Weekly, 1, vec![every(day)], Vec::new()),
        rule_of(RepeatFreq::Weekly, 2, vec![every(day)], Vec::new()),
        monthly(start),
        rule_of(RepeatFreq::Yearly, 1, Vec::new(), Vec::new()),
    ]
}

/// `repeat` with what a rule leaves to the start filled in, for comparing: the start's weekday for a
/// weekly rule without days, the start's day for a monthly one without days, month and day for a
/// yearly one; weekdays in week order, interval at least 1. End, text and `custom` stay.
fn normal(repeat: &Repeat, start: NaiveDate) -> Repeat {
    let mut r = repeat.clone();
    r.interval = r.interval.max(1);
    let no_day = r.weekdays.is_empty() && r.month_days.is_empty();
    match r.freq {
        RepeatFreq::Weekly if r.weekdays.is_empty() => r.weekdays = vec![every(weekday_of(start))],
        RepeatFreq::Monthly if no_day => r.month_days = vec![start.day() as i8],
        RepeatFreq::Yearly => {
            if r.months.is_empty() {
                r.months = vec![start.month() as u8];
            }
            if no_day {
                r.month_days = vec![start.day() as i8];
            }
        }
        _ => {}
    }
    r.weekdays.sort_by_key(|d| (index(d.day), d.nth));
    r.month_days.sort_unstable();
    r.months.sort_unstable();
    r
}

/// Whether `a` and `b` repeat on the same days for a series starting on `start`, whatever their end.
/// A custom rule is the same only as itself.
pub(super) fn same_pattern(a: &Repeat, b: &Repeat, start: NaiveDate) -> bool {
    if a.custom || b.custom {
        return a.custom == b.custom && a.rule == b.rule;
    }
    let (a, b) = (normal(a, start), normal(b, start));
    (a.freq, a.interval, &a.weekdays, &a.month_days, &a.months)
        == (b.freq, b.interval, &b.weekdays, &b.month_days, &b.months)
}

/// Whether `a` and `b` are the same rule for a series starting on `start`: the same days
/// ([`same_pattern`]) and the same end. Two missing rules are the same.
pub(super) fn same(a: Option<&Repeat>, b: Option<&Repeat>, start: NaiveDate) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same_pattern(a, b, start) && (a.custom || a.end == b.end),
        _ => false,
    }
}

/// `pattern` with the end of `current` (when there is one that the editor shows): picking another
/// rule from the menu keeps when the series ends.
pub(super) fn keeping_end(pattern: &Repeat, current: Option<&Repeat>) -> Repeat {
    let end = current.filter(|current| !current.custom).map_or(RepeatEnd::Never, |current| current.end);
    Repeat { end, ..pattern.clone() }
}

/// The sentence for `repeat` of a series starting on `start`: "Every week on Tue, until 22 Dec",
/// "Every month on the fourth Tue, 10 times", "Every weekday"; "Custom rule" for a rule the editor
/// cannot show.
pub(super) fn sentence(repeat: &Repeat, start: NaiveDate) -> String {
    if repeat.custom {
        return tr("Custom rule").to_string();
    }
    let pattern = pattern(repeat, start);
    match end_words(repeat.end, start) {
        Some(end) => format!("{pattern}, {end}"),
        None => pattern,
    }
}

/// The sentence for the days of `repeat` without its end: "Every 2 weeks on Tue".
pub(super) fn pattern(repeat: &Repeat, start: NaiveDate) -> String {
    let r = normal(repeat, start);
    let workdays: Vec<RepeatDay> = DAYS[..5].iter().copied().map(every).collect();
    if r.freq == RepeatFreq::Weekly && r.interval == 1 && r.weekdays == workdays {
        return tr("Every weekday").to_string();
    }
    let n = r.interval;
    let every = match (r.freq, n) {
        (RepeatFreq::Daily, 1) => tr("Every day").to_string(),
        (RepeatFreq::Daily, n) => trf("Every {n} days", &[("n", &n)]),
        (RepeatFreq::Weekly, 1) => tr("Every week").to_string(),
        (RepeatFreq::Weekly, n) => trf("Every {n} weeks", &[("n", &n)]),
        (RepeatFreq::Monthly, 1) => tr("Every month").to_string(),
        (RepeatFreq::Monthly, n) => trf("Every {n} months", &[("n", &n)]),
        (RepeatFreq::Yearly, 1) => tr("Every year").to_string(),
        (RepeatFreq::Yearly, n) => trf("Every {n} years", &[("n", &n)]),
    };
    let on = match r.freq {
        RepeatFreq::Daily if r.weekdays.is_empty() => None,
        RepeatFreq::Daily | RepeatFreq::Weekly => Some(on_days(&r.weekdays)),
        RepeatFreq::Monthly => Some(on_month(&r)),
        RepeatFreq::Yearly => Some(on_year(&r, start)),
    };
    match on {
        Some(on) => format!("{every} {}", lower_first(&on)),
        None => every,
    }
}

/// `text` with its first letter in lower case: the on-part behind the every-part.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "On Mon, Wed, Fri".
fn on_days(days: &[RepeatDay]) -> String {
    let names: Vec<&str> = days.iter().map(|d| short(d.day)).collect();
    trf("On {days}", &[("days", &names.join(", "))])
}

/// The on-part of a monthly rule: "On the fourth Tue", "On the last day", "On day 1, 15".
fn on_month(r: &Repeat) -> String {
    match r.weekdays.as_slice() {
        [RepeatDay { day, nth: Some(nth) }] => on_nth(*nth, *day).unwrap_or_else(|| on_days(&r.weekdays)),
        [] => on_month_days(&r.month_days),
        days => on_days(days),
    }
}

/// "On day 27", "On the last day", "On day 1, 15".
fn on_month_days(days: &[i8]) -> String {
    match days {
        [-1] => tr("On the last day").to_string(),
        days => {
            let list: Vec<String> = days.iter().map(i8::to_string).collect();
            trf("On day {n}", &[("n", &list.join(", "))])
        }
    }
}

/// "On the fourth Tue", "On the last Fri"; `None` for an n-th the words do not cover (a yearly rule's
/// 20th Monday).
fn on_nth(nth: i8, day: Weekday) -> Option<String> {
    let day = short(day);
    let words = match nth {
        1 => trf("On the first {day}", &[("day", &day)]),
        2 => trf("On the second {day}", &[("day", &day)]),
        3 => trf("On the third {day}", &[("day", &day)]),
        4 => trf("On the fourth {day}", &[("day", &day)]),
        5 => trf("On the fifth {day}", &[("day", &day)]),
        -1 => trf("On the last {day}", &[("day", &day)]),
        _ => return None,
    };
    Some(words)
}

/// The on-part of a yearly rule: "On 27 Oct"; the start's date where the rule names no single day.
fn on_year(r: &Repeat, start: NaiveDate) -> String {
    let (month, day) = match (r.months.as_slice(), r.month_days.as_slice()) {
        ([month], [day]) if *day > 0 => (u32::from(*month), i64::from(*day)),
        _ => (start.month(), i64::from(start.day())),
    };
    let date = trf("{day} {month}", &[("day", &day), ("month", &month_name_short(month))]);
    trf("On {date}", &[("date", &date)])
}

/// The end of a series in words, behind the days: "until 22 Dec" (with the year when it is not the
/// start's), "10 times", "once"; `None` for a series that goes on.
fn end_words(end: RepeatEnd, start: NaiveDate) -> Option<String> {
    match end {
        RepeatEnd::Never => None,
        RepeatEnd::Count(1) => Some(tr("once").to_string()),
        RepeatEnd::Count(n) => Some(trf("{n} times", &[("n", &n)])),
        RepeatEnd::Until(until) => {
            let day = local_day(until);
            let date = if day.year() == start.year() {
                trf("{day} {month}", &[("day", &day.day()), ("month", &month_name_short(day.month()))])
            } else {
                trf(
                    "{day} {month} {year}",
                    &[("day", &day.day()), ("month", &month_name_short(day.month())), ("year", &day.year())],
                )
            };
            Some(trf("until {date}", &[("date", &date)]))
        }
    }
}

/// The day of `time` on this computer's calendar.
fn local_day(time: EventTime) -> NaiveDate {
    match time {
        EventTime::Date(day) => day,
        EventTime::DateTime(at) => to_local(at).date(),
    }
}

// ---------------------------------------------------------------------------------------------
// The card "Custom repeat"
// ---------------------------------------------------------------------------------------------

/// How a custom rule ends, as the card offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    Never,
    /// On a day ([`Custom::until`]).
    On,
    /// After a number of events ([`Custom::count`]).
    After,
}

/// What a field of the card "Custom repeat" sends (`Message::Custom`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// "Every n": the number as typed.
    Every(String),
    /// Day, week, month or year.
    Freq(RepeatFreq),
    /// A weekday chip, by its place from Monday.
    Day(usize),
    /// Monthly: on the start's n-th weekday (`true`) or on its day of the month.
    ByWeekday(bool),
    Ends(Ends),
    /// The last day, as typed.
    Until(String),
    /// The number of events, as typed.
    Count(String),
    /// "Done": take the rule, if it can be read.
    Done,
    /// "Cancel": leave the rule as it is.
    Cancel,
}

/// Why the card cannot make a rule yet; shown after "Done".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Every,
    Count,
    Until,
    UntilBeforeStart,
}

impl Problem {
    fn message(self) -> &'static str {
        match self {
            Problem::Every => tr("Enter how often, e.g. 2"),
            Problem::Count => tr("Enter how many times, e.g. 10"),
            Problem::Until => tr("Enter the last day, e.g. 22.12.2026"),
            Problem::UntilBeforeStart => tr("The last day must not be before the start"),
        }
    }
}

/// The card "Custom repeat" as typed: every how many of which unit, on which weekdays (weekly) or
/// which day (monthly), and how it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Custom {
    every: String,
    freq: RepeatFreq,
    days: [bool; 7],
    by_weekday: bool,
    ends: Ends,
    until: String,
    count: String,
    /// "Done" was pressed with something that cannot be read: from then on the problem shows.
    attempted: bool,
}

impl Custom {
    /// The card for the rule `current` (none: every week) of a series starting on `start`.
    pub fn new(current: Option<&Repeat>, start: NaiveDate) -> Custom {
        let current = current.filter(|r| !r.custom).map(|r| normal(r, start));
        let freq = current.as_ref().map_or(RepeatFreq::Weekly, |r| r.freq);
        let mut days = [false; 7];
        match current.as_ref().filter(|r| matches!(r.freq, RepeatFreq::Weekly | RepeatFreq::Daily)) {
            Some(r) if !r.weekdays.is_empty() => r.weekdays.iter().for_each(|d| days[index(d.day)] = true),
            _ => days[index(weekday_of(start))] = true,
        }
        let by_weekday = match current.as_ref().filter(|r| r.freq == RepeatFreq::Monthly) {
            Some(r) => !r.weekdays.is_empty(),
            None => !last_of_month(start),
        };
        let (ends, until, count) = match current.as_ref().map(|r| r.end) {
            Some(RepeatEnd::Until(until)) => (Ends::On, format_date(local_day(until)), String::new()),
            Some(RepeatEnd::Count(n)) => (Ends::After, String::new(), n.to_string()),
            _ => (Ends::Never, String::new(), String::new()),
        };
        let every = current.as_ref().map_or(1, |r| r.interval).to_string();
        Custom { every, freq, days, by_weekday, ends, until, count, attempted: false }
    }

    /// Takes a field's change; `Done` and `Cancel` are the editor's.
    pub fn update(&mut self, edit: Edit) {
        match edit {
            Edit::Every(value) => self.every = value,
            Edit::Freq(freq) => self.freq = freq,
            Edit::Day(i) => {
                if let Some(day) = self.days.get_mut(i) {
                    *day = !*day;
                }
            }
            Edit::ByWeekday(on) => self.by_weekday = on,
            Edit::Ends(ends) => self.ends = ends,
            Edit::Until(value) => self.until = value,
            Edit::Count(value) => self.count = value,
            Edit::Done => self.attempted = true,
            Edit::Cancel => {}
        }
    }

    /// The rule the card describes for a series starting on `start`, or why there is none yet.
    /// `today` reads dates typed without a year; `until` turns the last day into the rule's end (a
    /// day for an all-day series, the end of that day as an instant for a timed one).
    pub fn build(
        &self,
        start: NaiveDate,
        today: NaiveDate,
        until: impl Fn(NaiveDate) -> EventTime,
    ) -> Result<Repeat, Problem> {
        let interval = self.every.trim().parse::<u16>().ok().filter(|n| (1..=999).contains(n)).ok_or(Problem::Every)?;
        let (weekdays, month_days) = match self.freq {
            RepeatFreq::Weekly => {
                let mut chosen: Vec<RepeatDay> =
                    DAYS.iter().zip(self.days).filter(|(_, on)| *on).map(|(day, _)| every(*day)).collect();
                if chosen.is_empty() {
                    chosen.push(every(weekday_of(start)));
                }
                (chosen, Vec::new())
            }
            RepeatFreq::Monthly if self.by_weekday => {
                (vec![RepeatDay { day: weekday_of(start), nth: Some(nth_in_month(start)) }], Vec::new())
            }
            RepeatFreq::Monthly if last_of_month(start) => (Vec::new(), vec![-1]),
            RepeatFreq::Monthly => (Vec::new(), vec![start.day() as i8]),
            RepeatFreq::Daily | RepeatFreq::Yearly => (Vec::new(), Vec::new()),
        };
        let end = match self.ends {
            Ends::Never => RepeatEnd::Never,
            Ends::After => RepeatEnd::Count(
                self.count.trim().parse::<u32>().ok().filter(|n| (1..=999).contains(n)).ok_or(Problem::Count)?,
            ),
            Ends::On => {
                let day = parse_date(&self.until, today).ok_or(Problem::Until)?;
                if day < start {
                    return Err(Problem::UntilBeforeStart);
                }
                RepeatEnd::Until(until(day))
            }
        };
        Ok(Repeat { end, ..rule_of(self.freq, interval, weekdays, month_days) })
    }

    /// The card under the "Repeat" row: "Every n" with the unit, the weekdays (weekly) or the day
    /// (monthly), the end, the sentence of the rule (or what is wrong, after "Done"), Cancel and
    /// Done.
    pub fn view<'a>(
        &'a self,
        start: NaiveDate,
        today: NaiveDate,
        until: impl Fn(NaiveDate) -> EventTime,
    ) -> Element<'a, Message> {
        let built = self.build(start, today, until);
        let labels = [tr("Every"), tr("On"), tr("Ends")];
        let width = labels.iter().map(|l| text_width(l, typo::meta())).fold(0.0, f32::max).ceil();
        let label = move |text: &'static str| -> Element<'a, Message> {
            container(typo::text(typo::meta(), text).style(theme::text_3)).width(width).into()
        };
        let line = |name: &'static str, value: Element<'a, Message>| -> Element<'a, Message> {
            row![label(name), value].spacing(space::s()).align_y(Alignment::Center).into()
        };

        let units = [
            (tr("Day"), edit(Edit::Freq(RepeatFreq::Daily))),
            (tr("Week"), edit(Edit::Freq(RepeatFreq::Weekly))),
            (tr("Month"), edit(Edit::Freq(RepeatFreq::Monthly))),
            (tr("Year"), edit(Edit::Freq(RepeatFreq::Yearly))),
        ];
        let unit = match self.freq {
            RepeatFreq::Daily => 0,
            RepeatFreq::Weekly => 1,
            RepeatFreq::Monthly => 2,
            RepeatFreq::Yearly => 3,
        };
        let every_row = row![field(&self.every, "1", "000", |v| edit(Edit::Every(v))), widgets::segmented(units, unit)]
            .spacing(space::s())
            .align_y(Alignment::Center);
        let mut card = column![typo::text(typo::title(), tr("Custom repeat")), line(tr("Every"), every_row.into()),]
            .spacing(space::s());

        match self.freq {
            RepeatFreq::Weekly => {
                let chips = DAYS
                    .iter()
                    .enumerate()
                    .map(|(i, day)| {
                        widgets::choice_chip(short(*day), None::<Mark>, self.days[i], Some(edit(Edit::Day(i))))
                    })
                    .collect();
                card = card.push(line(tr("On"), widgets::chip_row(chips)));
            }
            RepeatFreq::Monthly => {
                let by_day = on_month_days(&[if last_of_month(start) { -1 } else { start.day() as i8 }]);
                let by_weekday = on_nth(nth_in_month(start), weekday_of(start)).unwrap_or_default();
                let chips = vec![
                    widgets::choice_chip(by_day, None::<Mark>, !self.by_weekday, Some(edit(Edit::ByWeekday(false)))),
                    widgets::choice_chip(by_weekday, None::<Mark>, self.by_weekday, Some(edit(Edit::ByWeekday(true)))),
                ];
                card = card.push(line(tr("On"), widgets::chip_row(chips)));
            }
            RepeatFreq::Daily | RepeatFreq::Yearly => {}
        }

        let ends = [
            (tr("Never"), edit(Edit::Ends(Ends::Never))),
            (trc("date", "On"), edit(Edit::Ends(Ends::On))),
            (tr("After"), edit(Edit::Ends(Ends::After))),
        ];
        let chosen = match self.ends {
            Ends::Never => 0,
            Ends::On => 1,
            Ends::After => 2,
        };
        let mut end_row = row![widgets::segmented(ends, chosen)].spacing(space::s()).align_y(Alignment::Center);
        match self.ends {
            Ends::Never => {}
            Ends::On => {
                end_row = end_row.push(field(&self.until, "22.12.2026", "00.00.0000", |v| edit(Edit::Until(v))))
            }
            Ends::After => {
                end_row = end_row
                    .push(field(&self.count, "10", "000", |v| edit(Edit::Count(v))))
                    .push(typo::text(typo::body(), tr("times")).style(theme::text_2));
            }
        }
        card = card.push(line(tr("Ends"), end_row.wrap().vertical_spacing(space::s()).into()));

        let note: Element<'a, Message> = match &built {
            Err(problem) if self.attempted => typo::text(typo::meta(), problem.message())
                .style(|t| iced::widget::text::Style { color: Some(palette(t).error_text) })
                .into(),
            Ok(rule) => typo::text(typo::meta(), sentence(rule, start)).style(theme::text_2).into(),
            Err(_) => typo::text(typo::meta(), "").into(),
        };
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(edit(Edit::Cancel))),
            widgets::text_button(None, trc("dialog", "Done"), style::button::secondary, Some(edit(Edit::Done))),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        card = card.push(rule::horizontal(size::HAIRLINE).style(style::rule)).push(note).push(actions);
        container(card)
            .width(widgets::POPOVER_WIDTH)
            .padding(layout::panel_padding())
            .style(style::container::popover)
            .into()
    }
}

/// `edit` as the editor's message.
fn edit(edit: Edit) -> Message {
    Message::Custom(edit)
}

/// A framed input of the card, as wide as `sample` (or the placeholder) in tabular figures; Enter
/// is "Done".
fn field<'a>(
    value: &'a str,
    placeholder: &'static str,
    sample: &'static str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let level = typo::body().tabular();
    let text = text_width(sample, level).max(text_width(placeholder, level));
    let width = text + style::INPUT_PADDING.left + style::INPUT_PADDING.right + space::s();
    text_input(placeholder, value)
        .on_input(on_input)
        .on_submit(edit(Edit::Done))
        .padding(style::INPUT_PADDING)
        .size(level.size)
        .line_height(level.line_height)
        .font(level.font)
        .width(Length::Fixed(width.ceil()))
        .style(style::text_input)
        .into()
}

/// The last day of a series as its end: the day itself for an all-day series, else the end of that
/// day in `wall_to_utc`'s zone (so the day's event is still in).
pub(super) fn until_of(
    day: NaiveDate,
    all_day: bool,
    wall_to_utc: impl Fn(chrono::NaiveDateTime) -> chrono::DateTime<chrono::Utc>,
) -> EventTime {
    if all_day {
        return EventTime::Date(day);
    }
    let end = day.and_time(chrono::NaiveTime::MIN) + Duration::days(1) - Duration::seconds(1);
    EventTime::DateTime(wall_to_utc(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("a date")
    }

    fn nth(day: Weekday, nth: i8) -> RepeatDay {
        RepeatDay { day, nth: Some(nth) }
    }

    /// Task TE.7: the short list follows the start date.
    #[test]
    fn the_short_list_follows_the_start_date() {
        // Tuesday, 27 October 2026: the fourth Tuesday of the month.
        let list = shortlist(day(2026, 10, 27));
        assert_eq!(list.len(), 6);
        assert_eq!((list[0].freq, list[0].interval), (RepeatFreq::Daily, 1));
        assert_eq!(list[1].weekdays.len(), 5, "every weekday");
        assert_eq!(list[2].weekdays, [every(Weekday::Tuesday)]);
        assert_eq!((list[3].interval, list[3].weekdays.as_slice()), (2, [every(Weekday::Tuesday)].as_slice()));
        assert_eq!(list[4].freq, RepeatFreq::Monthly);
        assert_eq!(list[4].weekdays, [nth(Weekday::Tuesday, 4)], "the fourth Tuesday");
        assert!(list[4].month_days.is_empty());
        assert_eq!(list[5].freq, RepeatFreq::Yearly);
        assert!(list.iter().all(|r| r.end == RepeatEnd::Never && !r.custom && r.rule.is_empty()));

        // The last day of the month repeats on the last day, not on a weekday.
        let last = monthly(day(2026, 10, 31));
        assert_eq!((last.weekdays.len(), last.month_days.as_slice()), (0, [-1].as_slice()));
        assert_eq!(monthly(day(2026, 2, 28)).month_days, [-1], "February's last day too");
        // A fifth weekday is the last one: Tuesday, 29 September 2026.
        assert_eq!(monthly(day(2026, 9, 29)).weekdays, [nth(Weekday::Tuesday, -1)]);
        assert_eq!(monthly(day(2026, 10, 1)).weekdays, [nth(Weekday::Thursday, 1)]);
    }

    /// Task TE.7: the sentence of a rule, put together from translated parts (so this test holds in
    /// English and German, `TORRO_LANG=de`).
    #[test]
    fn the_sentence_names_the_rule_in_the_language_of_the_suite() {
        let start = day(2026, 10, 27);
        let list = shortlist(start);
        let tue = tr("Tue");
        let on = |words: String| lower_first(&words);
        assert_eq!(sentence(&list[0], start), tr("Every day"));
        assert_eq!(sentence(&list[1], start), tr("Every weekday"));
        let weekly = format!("{} {}", tr("Every week"), on(trf("On {days}", &[("days", &tue)])));
        assert_eq!(sentence(&list[2], start), weekly);
        let biweekly = format!("{} {}", trf("Every {n} weeks", &[("n", &2)]), on(trf("On {days}", &[("days", &tue)])));
        assert_eq!(sentence(&list[3], start), biweekly);
        let fourth = format!("{} {}", tr("Every month"), on(trf("On the fourth {day}", &[("day", &tue)])));
        assert_eq!(sentence(&list[4], start), fourth);
        let date = trf("{day} {month}", &[("day", &27), ("month", &month_name_short(10))]);
        assert_eq!(
            sentence(&list[5], start),
            format!("{} {}", tr("Every year"), on(trf("On {date}", &[("date", &date)])))
        );
        let last = format!("{} {}", tr("Every month"), on(tr("On the last day").to_string()));
        assert_eq!(sentence(&monthly(day(2026, 10, 31)), day(2026, 10, 31)), last);

        // With an end: "…, until 22 Dec", the year only when it is not the start's.
        let until = Repeat { end: RepeatEnd::Until(EventTime::Date(day(2026, 12, 22))), ..list[2].clone() };
        let date = trf("{day} {month}", &[("day", &22), ("month", &month_name_short(12))]);
        assert_eq!(sentence(&until, start), format!("{weekly}, {}", trf("until {date}", &[("date", &date)])));
        let later = Repeat { end: RepeatEnd::Until(EventTime::Date(day(2027, 1, 5))), ..list[2].clone() };
        let date = trf("{day} {month} {year}", &[("day", &5), ("month", &month_name_short(1)), ("year", &2027)]);
        assert_eq!(sentence(&later, start), format!("{weekly}, {}", trf("until {date}", &[("date", &date)])));
        let ten = Repeat { end: RepeatEnd::Count(10), ..list[2].clone() };
        assert_eq!(sentence(&ten, start), format!("{weekly}, {}", trf("{n} times", &[("n", &10)])));

        // A rule the editor cannot show.
        let custom = Repeat { custom: true, rule: "FREQ=MONTHLY;BYSETPOS=-1;BYDAY=MO,TU".into(), ..list[4].clone() };
        assert_eq!(sentence(&custom, start), tr("Custom rule"));
    }

    #[test]
    fn the_on_part_goes_lower_case_behind_the_every_part() {
        assert_eq!(lower_first("On Tue"), "on Tue");
        assert_eq!(lower_first("Am Di"), "am Di");
        assert_eq!(lower_first(""), "");
    }

    #[test]
    fn a_rule_from_the_server_matches_its_choice_in_the_menu() {
        let start = day(2026, 10, 27);
        // `FREQ=WEEKLY` without days and `FREQ=WEEKLY;BYDAY=TU` are the same for a Tuesday start.
        let read = Repeat { rule: "FREQ=WEEKLY".into(), ..rule_of(RepeatFreq::Weekly, 1, Vec::new(), Vec::new()) };
        assert!(same_pattern(&read, &shortlist(start)[2], start));
        assert!(!same_pattern(&read, &shortlist(start)[3], start), "every two weeks is another rule");
        let yearly =
            Repeat { months: vec![10], month_days: vec![27], ..rule_of(RepeatFreq::Yearly, 1, vec![], vec![]) };
        assert!(same_pattern(&yearly, &shortlist(start)[5], start));
        // The end counts for `same`, not for the pattern.
        let ending = Repeat { end: RepeatEnd::Count(3), ..read.clone() };
        assert!(same_pattern(&ending, &read, start) && !same(Some(&ending), Some(&read), start));
        assert_eq!(
            keeping_end(&shortlist(start)[0], Some(&ending)).end,
            RepeatEnd::Count(3),
            "a new choice keeps the end"
        );
        assert!(same(None, None, start) && !same(Some(&read), None, start));
    }

    #[test]
    fn the_custom_card_makes_a_rule_or_says_what_is_missing() {
        let start = day(2026, 10, 27);
        let today = day(2026, 10, 22);
        let until = |d: NaiveDate| EventTime::Date(d);
        let mut card = Custom::new(None, start);
        assert_eq!(card.build(start, today, until), Ok(shortlist(start)[2].clone()), "every week on the start's day");

        card.update(Edit::Day(0));
        card.update(Edit::Every("2".into()));
        card.update(Edit::Ends(Ends::On));
        assert_eq!(card.build(start, today, until), Err(Problem::Until));
        card.update(Edit::Until("22.12.2026".into()));
        let rule = card.build(start, today, until).expect("a rule");
        assert_eq!(rule.weekdays, [every(Weekday::Monday), every(Weekday::Tuesday)]);
        assert_eq!((rule.interval, rule.end), (2, RepeatEnd::Until(EventTime::Date(day(2026, 12, 22)))));
        card.update(Edit::Until("01.10.2026".into()));
        assert_eq!(card.build(start, today, until), Err(Problem::UntilBeforeStart));

        card.update(Edit::Ends(Ends::After));
        card.update(Edit::Count("0".into()));
        assert_eq!(card.build(start, today, until), Err(Problem::Count));
        card.update(Edit::Count("10".into()));
        card.update(Edit::Every("x".into()));
        assert_eq!(card.build(start, today, until), Err(Problem::Every));
        card.update(Edit::Every("1".into()));
        card.update(Edit::Freq(RepeatFreq::Monthly));
        let monthly_rule = card.build(start, today, until).expect("a rule");
        assert_eq!(
            (monthly_rule.weekdays.as_slice(), monthly_rule.end),
            ([nth(Weekday::Tuesday, 4)].as_slice(), RepeatEnd::Count(10))
        );
        card.update(Edit::ByWeekday(false));
        assert_eq!(card.build(start, today, until).map(|r| r.month_days), Ok(vec![27]));

        // The card opens on the rule it replaces.
        let again = Custom::new(Some(&rule), start);
        assert_eq!(again.build(start, today, until), Ok(rule));
        let _ = again.view(start, today, until);
    }

    #[test]
    fn a_timed_series_ends_with_the_end_of_its_last_day() {
        let utc = |naive: chrono::NaiveDateTime| naive.and_utc();
        let end = until_of(day(2026, 12, 22), false, utc);
        assert_eq!(end, EventTime::DateTime(day(2026, 12, 22).and_hms_opt(23, 59, 59).expect("a time").and_utc()));
        assert_eq!(until_of(day(2026, 12, 22), true, utc), EventTime::Date(day(2026, 12, 22)));
    }
}
