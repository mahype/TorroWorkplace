//! The card "Add reminder" of the event editor (task TE.8, `docs/termin-editor.md` layout A, picture
//! `termin-editor-a-remind.png`): a chip per common lead (at the start, 5, 10, 15 and 30 minutes, an
//! hour, a day), then "Custom" with three ways to say when – n minutes, hours, days or weeks before or
//! after the start or the end; the day before at a time of day; a fixed date and time – and how the
//! reminder shows (as its calendar says, a notification, full screen). Add takes it, Cancel leaves.
//!
//! A reminder is stored as the trigger of its `VALARM`: the chips and the relative way as `Before` in
//! minutes (negative after), "the day before at 18:00" also as `Before`, counted from the start on the
//! wall clock of the event's zone, a fixed date as `At`. No reminder lies further than the suite's
//! longest lead from the event (`Reminders::MAX_LEAD`, a week), as for the default reminder and the
//! MCP tool. The functions here are pure; [`super::Editor`] keeps the card and adds what it builds.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use iced::widget::{column, container, radio, row, rule, text_input};
use iced::{Alignment, Element, Length};
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::Reminders;
use torro_gui::datetime::{format_date, format_time, parse_date, parse_time};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::{self, text_width};
use torro_gui::widgets::{self, Mark};
use torro_gui::{layout, style};
use torro_pim::{Alarm, AlarmAnchor, AlarmStyle, AlarmTrigger};

use super::Message;

/// The leads the chips offer, in minutes before the start: at the start, 5, 10, 15 and 30 minutes,
/// an hour, a day.
pub(super) const PRESETS: [i64; 7] = [0, 5, 10, 15, 30, 60, 1440];

/// The lead a new card has chosen: the chip "10 min", as in the picture.
const FIRST_PRESET: i64 = 10;

/// The unit of a custom lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Minutes,
    Hours,
    Days,
    Weeks,
}

impl Unit {
    const ALL: [Unit; 4] = [Unit::Minutes, Unit::Hours, Unit::Days, Unit::Weeks];

    /// How many minutes one of it is.
    fn minutes(self) -> i64 {
        match self {
            Unit::Minutes => 1,
            Unit::Hours => 60,
            Unit::Days => 24 * 60,
            Unit::Weeks => 7 * 24 * 60,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Unit::Minutes => tr("min"),
            Unit::Hours => tr("hours"),
            Unit::Days => tr("days"),
            Unit::Weeks => tr("weeks"),
        }
    }
}

/// Which way the card says when: a chip (its minutes before the start) or one of the three custom
/// ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Preset(i64),
    /// n units before or after the start or the end.
    Relative,
    /// The day before the start at a time of day.
    DayBefore,
    /// A date and a time.
    Fixed,
}

/// What a field of the card sends (`Message::Remind`). Typing into a custom field chooses its way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// A chip, by its minutes before the start.
    Preset(i64),
    /// A radio button of "Custom".
    Way(Way),
    /// How many units, as typed.
    Amount(String),
    Unit(Unit),
    /// After (`true`) or before.
    After(bool),
    /// Counted from the start or the end.
    Of(AlarmAnchor),
    /// The time of "Day before at", as typed.
    DayBefore(String),
    /// The date of a fixed reminder, as typed.
    Date(String),
    /// The time of a fixed reminder, as typed.
    Time(String),
    /// How it shows; `None` as its calendar says.
    Style(Option<AlarmStyle>),
    /// "Add": take the reminder, if it can be read.
    Add,
    /// "Cancel": add nothing.
    Cancel,
}

/// Why the card cannot make a reminder yet; shown after "Add".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Amount,
    Time,
    Date,
    /// Further than [`Reminders::MAX_LEAD`] from the event.
    TooFar,
}

impl Problem {
    fn message(self) -> &'static str {
        match self {
            Problem::Amount => tr("Enter a number, e.g. 2"),
            Problem::Time => tr("Enter a time, e.g. 18:00"),
            Problem::Date => tr("Enter a date, e.g. 26.10.2026"),
            Problem::TooFar => tr("A reminder can be at most a week before or after the event"),
        }
    }
}

/// The card as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    way: Way,
    amount: String,
    unit: Unit,
    after: bool,
    of: AlarmAnchor,
    day_before: String,
    date: String,
    time: String,
    style: Option<AlarmStyle>,
    /// "Add" was pressed with something that cannot be read: from then on the problem shows.
    attempted: bool,
}

impl Card {
    /// A card for an event starting on `start`: the chip "10 min" chosen, "2 hours before start",
    /// "Day before at 18:00" and "On" the day before at 09:00 ready to pick, shown as its calendar
    /// says.
    pub fn new(start: NaiveDate) -> Card {
        Card {
            way: Way::Preset(FIRST_PRESET),
            amount: "2".into(),
            unit: Unit::Hours,
            after: false,
            of: AlarmAnchor::Start,
            day_before: "18:00".into(),
            date: format_date(start.pred_opt().unwrap_or(start)),
            time: "09:00".into(),
            style: None,
            attempted: false,
        }
    }

    /// Takes a field's change; `Add` and `Cancel` are the editor's, `Add` only marks the attempt.
    pub fn update(&mut self, edit: Edit) {
        match edit {
            Edit::Preset(minutes) => self.way = Way::Preset(minutes),
            Edit::Way(way) => self.way = way,
            Edit::Amount(value) => {
                self.amount = value;
                self.way = Way::Relative;
            }
            Edit::Unit(unit) => {
                self.unit = unit;
                self.way = Way::Relative;
            }
            Edit::After(after) => {
                self.after = after;
                self.way = Way::Relative;
            }
            Edit::Of(of) => {
                self.of = of;
                self.way = Way::Relative;
            }
            Edit::DayBefore(value) => {
                self.day_before = value;
                self.way = Way::DayBefore;
            }
            Edit::Date(value) => {
                self.date = value;
                self.way = Way::Fixed;
            }
            Edit::Time(value) => {
                self.time = value;
                self.way = Way::Fixed;
            }
            Edit::Style(style) => self.style = style,
            Edit::Add => self.attempted = true,
            Edit::Cancel => {}
        }
    }

    /// The reminder the card describes for an event starting at `start` on the wall clock of its
    /// zone, or why there is none yet. `today` reads dates typed without a year; `instant` turns a
    /// wall clock of the event's zone into an instant.
    pub fn build(
        &self,
        start: NaiveDateTime,
        today: NaiveDate,
        instant: impl Fn(NaiveDateTime) -> DateTime<Utc>,
    ) -> Result<Alarm, Problem> {
        let trigger = match self.way {
            Way::Preset(minutes) => AlarmTrigger::Before { minutes, of: AlarmAnchor::Start },
            Way::Relative => {
                let n = self.amount.trim().parse::<i64>().ok().filter(|n| *n >= 0).ok_or(Problem::Amount)?;
                let minutes = n.checked_mul(self.unit.minutes()).ok_or(Problem::TooFar)?;
                AlarmTrigger::Before { minutes: if self.after { -minutes } else { minutes }, of: self.of }
            }
            Way::DayBefore => {
                let at = parse_time(&self.day_before).ok_or(Problem::Time)?;
                AlarmTrigger::Before { minutes: day_before(start, at, &instant), of: AlarmAnchor::Start }
            }
            Way::Fixed => {
                let date = parse_date(&self.date, today).ok_or(Problem::Date)?;
                let time = parse_time(&self.time).ok_or(Problem::Time)?;
                AlarmTrigger::At(instant(date.and_time(time)))
            }
        };
        if !within_lead(trigger, instant(start)) {
            return Err(Problem::TooFar);
        }
        Ok(Alarm { trigger, style: self.style })
    }

    /// The card under "Add reminder": the chips, "Custom" with its three ways, "Style", what is wrong
    /// (after "Add"), Cancel and Add.
    pub fn view<'a>(
        &'a self,
        start: NaiveDateTime,
        today: NaiveDate,
        instant: impl Fn(NaiveDateTime) -> DateTime<Utc>,
    ) -> Element<'a, Message> {
        let chips = PRESETS
            .iter()
            .map(|&minutes| {
                let on = self.way == Way::Preset(minutes);
                widgets::choice_chip(chip_label(minutes), None::<Mark>, on, Some(edit(Edit::Preset(minutes))))
            })
            .collect();

        let units = Unit::ALL.map(|unit| (unit.label(), edit(Edit::Unit(unit))));
        let unit = Unit::ALL.iter().position(|u| *u == self.unit).unwrap_or_default();
        let relative = row![
            choice(Way::Relative, self.way),
            field(&self.amount, "2", "0000", |v| edit(Edit::Amount(v))),
            widgets::segmented(units, unit),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        // A longer translation puts the units under the number rather than cutting them off.
        .wrap()
        .vertical_spacing(space::s());
        let sides = [(tr("before"), edit(Edit::After(false))), (tr("after"), edit(Edit::After(true)))];
        let ends = [(tr("start"), edit(Edit::Of(AlarmAnchor::Start))), (tr("end"), edit(Edit::Of(AlarmAnchor::End)))];
        let of = if self.of == AlarmAnchor::End { 1 } else { 0 };
        // Under the number, so the two lines read as one: "2 hours / before start".
        let relation = row![widgets::segmented(sides, usize::from(self.after)), widgets::segmented(ends, of)]
            .spacing(space::s())
            .align_y(Alignment::Center);
        let relation = container(relation).padding(iced::Padding::ZERO.left(style::CHECK_SIZE + space::s()));
        let day_before = row![
            choice(Way::DayBefore, self.way),
            quiet(tr("Day before at")),
            field(&self.day_before, "18:00", "00:00", |v| edit(Edit::DayBefore(v))),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let fixed = row![
            choice(Way::Fixed, self.way),
            quiet(trc("date", "On")),
            field(&self.date, "26.10.2026", "00.00.0000", |v| edit(Edit::Date(v))),
            quiet(tr("at")),
            field(&self.time, "09:00", "00:00", |v| edit(Edit::Time(v))),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);

        // Chips rather than a segmented control: they wrap, so a longer translation still fits the card.
        let styles = [
            (Icon::Calendar, tr("Calendar default"), None),
            (Icon::Bell, tr("Notification"), Some(AlarmStyle::Notification)),
            (Icon::Eye, tr("Full screen"), Some(AlarmStyle::FullScreen)),
        ]
        .into_iter()
        .map(|(mark, label, style)| {
            widgets::choice_chip(label, Some(Mark::Icon(mark)), self.style == style, Some(edit(Edit::Style(style))))
        })
        .collect();

        let mut card = column![
            typo::text(typo::title(), tr("Add reminder")),
            widgets::chip_row(chips),
            caption(tr("Custom")),
            relative,
            relation,
            day_before,
            fixed,
            caption(tr("Style")),
            widgets::chip_row(styles),
            rule::horizontal(size::HAIRLINE).style(style::rule),
        ]
        .spacing(space::s());
        if self.attempted
            && let Err(problem) = self.build(start, today, instant)
        {
            card = card.push(
                typo::text(typo::meta(), problem.message())
                    .style(|t| iced::widget::text::Style { color: Some(palette(t).error_text) }),
            );
        }
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(edit(Edit::Cancel))),
            widgets::text_button(None, tr("Add"), style::button::secondary, Some(edit(Edit::Add))),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        container(card.push(actions))
            .width(widgets::POPOVER_WIDTH)
            .padding(layout::panel_padding())
            .style(style::container::popover)
            .into()
    }
}

/// The minutes before `start` (a wall clock) at which "the day before at `at`" fires, on the wall
/// clock of the event's zone (`instant`): 960 for 18:00 before a 10:00 event, also across a change of
/// the clocks.
pub(super) fn day_before(start: NaiveDateTime, at: NaiveTime, instant: impl Fn(NaiveDateTime) -> DateTime<Utc>) -> i64 {
    let day = start.date().pred_opt().unwrap_or(start.date());
    (instant(start) - instant(day.and_time(at))).num_minutes()
}

/// Whether `trigger` lies within [`Reminders::MAX_LEAD`] of the event starting at `start`, before or
/// after.
pub(super) fn within_lead(trigger: AlarmTrigger, start: DateTime<Utc>) -> bool {
    let most = i64::from(Reminders::MAX_LEAD);
    match trigger {
        AlarmTrigger::Before { minutes, .. } => minutes.abs() <= most,
        AlarmTrigger::At(at) => (start - at).num_minutes().abs() <= most,
    }
}

/// "Day before at 18:00" for a reminder "the day before at" made: before the start, on the calendar
/// day before it (on the wall clock `wall` of the event's zone), more than half a day and less than
/// two days ahead and not whole days. `None` for any other, which reads as "16 hours before" would
/// not tell.
pub(super) fn day_before_words(
    trigger: AlarmTrigger,
    start: DateTime<Utc>,
    wall: impl Fn(DateTime<Utc>) -> NaiveDateTime,
) -> Option<String> {
    let AlarmTrigger::Before { minutes, of: AlarmAnchor::Start } = trigger else { return None };
    if !(12 * 60 < minutes && minutes < 2 * 24 * 60) || minutes % (24 * 60) == 0 {
        return None;
    }
    let fires = wall(start - chrono::Duration::minutes(minutes));
    (Some(fires.date()) == wall(start).date().pred_opt())
        .then(|| trf("Day before at {time}", &[("time", &format_time(fires.time()))]))
}

/// The word of a chip: "At start", "5 min", "1 hour", "1 day".
fn chip_label(minutes: i64) -> String {
    if minutes == 0 { tr("At start").to_string() } else { super::span(minutes) }
}

/// `edit` as the editor's message.
fn edit(edit: Edit) -> Message {
    Message::Remind(edit)
}

/// The radio button of `way`, chosen while the card's way is `current`.
fn choice<'a>(way: Way, current: Way) -> Element<'a, Message> {
    radio("", way, Some(current), |way| edit(Edit::Way(way)))
        .size(style::CHECK_SIZE)
        .spacing(0.0)
        .style(style::radio)
        .into()
}

/// A word between the fields: "Day before at", "On", "at".
fn quiet<'a>(words: &'static str) -> Element<'a, Message> {
    typo::text(typo::body(), words).style(theme::text_2).into()
}

/// A section title inside the card: "Custom", "Style".
fn caption<'a>(title: &'static str) -> Element<'a, Message> {
    typo::text(typo::caption(), title).style(theme::text_3).into()
}

/// A framed input of the card, as wide as `sample` (or the placeholder) in tabular figures; Enter
/// is "Add".
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
        .on_submit(edit(Edit::Add))
        .padding(style::INPUT_PADDING)
        .size(level.size)
        .line_height(level.line_height)
        .font(level.font)
        .width(Length::Fixed(width.ceil()))
        .style(style::text_input)
        .into()
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::editor::zone;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).and_then(|day| day.and_hms_opt(h, min, 0)).expect("a time")
    }

    fn berlin(wall: NaiveDateTime) -> DateTime<Utc> {
        zone::instant(wall, Some("Europe/Berlin"))
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 22).expect("a date")
    }

    /// Task TE.8: the day before at 18:00 of an event at 10:00 is 960 minutes before its start.
    #[test]
    fn the_day_before_at_six_is_960_minutes_before_a_ten_oclock_event() {
        let start = at(2026, 10, 27, 10, 0);
        assert_eq!(day_before(start, NaiveTime::from_hms_opt(18, 0, 0).expect("18:00"), berlin), 960);
        // Across the change of the clocks (early on 25 October 2026 in Berlin) the wall clock still counts: the day
        // before has an hour more.
        let after_change = at(2026, 10, 25, 10, 0);
        assert_eq!(day_before(after_change, NaiveTime::from_hms_opt(18, 0, 0).expect("18:00"), berlin), 960 + 60);

        let mut card = Card::new(start.date());
        card.update(Edit::DayBefore("18:00".into()));
        let alarm = card.build(start, today(), berlin).expect("a reminder");
        assert_eq!(alarm.trigger, AlarmTrigger::Before { minutes: 960, of: AlarmAnchor::Start });
        let words = day_before_words(alarm.trigger, berlin(start), |at| zone::wall(at, Some("Europe/Berlin")));
        assert_eq!(words, Some(trf("Day before at {time}", &[("time", &"18:00")])));
    }

    #[test]
    fn only_a_reminder_on_the_day_before_reads_as_the_day_before() {
        let start = berlin(at(2026, 10, 27, 10, 0));
        let wall = |at| zone::wall(at, Some("Europe/Berlin"));
        let before = |minutes| AlarmTrigger::Before { minutes, of: AlarmAnchor::Start };
        assert_eq!(day_before_words(before(24 * 60), start, wall), None, "a whole day stays \"1 day before\"");
        assert_eq!(day_before_words(before(120), start, wall), None);
        assert_eq!(day_before_words(AlarmTrigger::Before { minutes: 960, of: AlarmAnchor::End }, start, wall), None);
    }

    #[test]
    fn chips_and_custom_ways_build_their_triggers() {
        let start = at(2026, 10, 27, 10, 0);
        let mut card = Card::new(start.date());
        assert_eq!(card.build(start, today(), berlin).map(|a| a.trigger), Ok(before(FIRST_PRESET)));
        card.update(Edit::Preset(0));
        card.update(Edit::Style(Some(AlarmStyle::FullScreen)));
        let alarm = card.build(start, today(), berlin).expect("a reminder");
        assert_eq!(alarm, Alarm { trigger: before(0), style: Some(AlarmStyle::FullScreen) });

        // Typing into a custom field chooses its way.
        card.update(Edit::Amount("3".into()));
        card.update(Edit::Unit(Unit::Days));
        assert_eq!(card.build(start, today(), berlin).map(|a| a.trigger), Ok(before(3 * 24 * 60)));
        card.update(Edit::After(true));
        card.update(Edit::Of(AlarmAnchor::End));
        card.update(Edit::Unit(Unit::Minutes));
        assert_eq!(
            card.build(start, today(), berlin).map(|a| a.trigger),
            Ok(AlarmTrigger::Before { minutes: -3, of: AlarmAnchor::End })
        );

        card.update(Edit::Date("26.10.".into()));
        card.update(Edit::Time("09:30".into()));
        assert_eq!(
            card.build(start, today(), berlin).map(|a| a.trigger),
            Ok(AlarmTrigger::At(berlin(at(2026, 10, 26, 9, 30))))
        );
    }

    /// Task TE.8: no reminder further than `Reminders::MAX_LEAD` (a week) from the event.
    #[test]
    fn the_longest_lead_is_kept() {
        let start = at(2026, 10, 27, 10, 0);
        let mut card = Card::new(start.date());
        card.update(Edit::Unit(Unit::Weeks));
        card.update(Edit::Amount("1".into()));
        assert!(card.build(start, today(), berlin).is_ok(), "a week is the most");
        card.update(Edit::Amount("2".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::TooFar));
        card.update(Edit::Unit(Unit::Minutes));
        card.update(Edit::Amount(Reminders::MAX_LEAD.to_string()));
        assert!(card.build(start, today(), berlin).is_ok());
        card.update(Edit::Amount((Reminders::MAX_LEAD + 1).to_string()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::TooFar));
        card.update(Edit::Amount("99999999999999999".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::TooFar), "no overflow");

        let far = start - Duration::days(8);
        card.update(Edit::Date(format_date(far.date())));
        card.update(Edit::Time("10:00".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::TooFar));
        assert!(within_lead(AlarmTrigger::At(berlin(start) - Duration::days(7)), berlin(start)));
    }

    #[test]
    fn what_cannot_be_read_says_so() {
        let start = at(2026, 10, 27, 10, 0);
        let mut card = Card::new(start.date());
        card.update(Edit::Amount("two".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::Amount));
        card.update(Edit::Amount("-2".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::Amount), "after is a choice, not a sign");
        card.update(Edit::DayBefore("25:00".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::Time));
        card.update(Edit::Date("31.02.2026".into()));
        assert_eq!(card.build(start, today(), berlin), Err(Problem::Date));
        card.update(Edit::Add);
        let _ = card.view(start, today(), berlin);
    }

    /// The custom lines fit the card in this language: the units may go under the number (the row
    /// wraps), the style chips wrap; nothing else may need to.
    #[test]
    fn the_custom_lines_fit_the_card() {
        let padding = layout::panel_padding();
        let inner = widgets::POPOVER_WIDTH - padding.left - padding.right;
        let level = typo::body().tabular();
        let field = |sample: &str| {
            text_width(sample, level) + style::INPUT_PADDING.left + style::INPUT_PADDING.right + space::s()
        };
        let units: Vec<&str> = Unit::ALL.iter().map(|unit| unit.label()).collect();
        let number = style::CHECK_SIZE + field("0000") + space::s();
        assert!(
            widgets::segmented_width(&units).max(number) <= inner,
            "the units fit, if need be on a line of their own"
        );
        let relation = style::CHECK_SIZE
            + space::s()
            + widgets::segmented_width(&[tr("before"), tr("after")])
            + widgets::segmented_width(&[tr("start"), tr("end")])
            + space::s();
        assert!(relation <= inner, "{relation} > {inner}");
        let words = text_width(trc("date", "On"), typo::body()) + text_width(tr("at"), typo::body());
        let fixed = style::CHECK_SIZE + words + field("00.00.0000") + field("00:00") + 4.0 * space::s();
        assert!(fixed <= inner, "{fixed} > {inner}");
    }

    fn before(minutes: i64) -> AlarmTrigger {
        AlarmTrigger::Before { minutes, of: AlarmAnchor::Start }
    }
}
