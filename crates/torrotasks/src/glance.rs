//! Mini and strip: the tasks as a glance at a small tile (task K.20 in
//! `docs/tasks/kalender-aufgaben.md`; design skill `.claude/skills/torro-design`, section 6; the
//! pattern is `torrocalendar/src/views/glance.rs`).
//!
//! - **Mini** ([`mini`]): at the top "3 overdue · 5 today" (only the parts that are not zero;
//!   overdue in the attention colour, never red), then the next open tasks – overdue first, then
//!   today, then by due date – as many as fit, each with its round check, title and due date. The
//!   rest is counted as "+4 more". Nothing open: a calm "All done for today".
//! - **Strip** ([`strip`]): the same numbers at the left, the same tasks as chips (check and
//!   title) filling the rest in as many lines as the height allows; what does not fit is "+4".
//!
//! Both are displays, not small apps: no header, sidebar, menu, form or dialog. The one thing to
//! do is the purpose of the tile – tick a task off by its check (`Message::ToggleDone`, the same
//! message as in the list). They are rebuilt only with the app's minute tick and on new data.
//!
//! The decisions – counts, order, texts, how many rows or chips fit – are plain functions with
//! tests; the views only lay out their results.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::text::{self, Wrapping};
use iced::widget::{Space, checkbox, column, container, mouse_area, row, svg};
use iced::{Alignment, Color, Element, Fill, Padding, Size, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::shown_rows;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::{style, widgets};
use torro_pim::{EventTime, Todo};

use crate::app::Message;
use crate::list::due_text;

/// Between the parts of the numbers line: "3 overdue · 5 today".
const SEPARATOR: &str = " · ";
/// A chip is never wider than this, so one long title does not push the others out.
const CHIP_MAX: f32 = size::SIDEBAR;
/// The one-pixel frame of a chip (it lies inside the chip's bounds).
const FRAME: f32 = 1.0;
/// The numbers of the strip take at most this share of its width.
const STRIP_INFO_SHARE: f32 = 4.0;
/// The level of the numbers and the calm line, largest first: the first that fits is used.
const HEAD_LEVELS: [typo::Level; 2] = [typo::TITLE.tabular(), typo::BODY_STRONG.tabular()];
/// The height of a row of the mini tile: a control, so the check is a 28-high click target.
const ROW: f32 = size::CONTROL_SM;

/// What the glance gets to draw.
pub struct GlanceData<'a> {
    /// All tasks, in the order the app keeps them: a [`Pick::index`] is an index into this.
    pub todos: &'a [Todo],
    /// Collection id → colour, for the check of each task.
    pub colors: HashMap<String, Color>,
    pub today: NaiveDate,
    /// A failed save or load, as one quiet line (the glance has no retry button).
    pub error: Option<&'a str>,
}

/// How many open tasks are overdue and how many are due today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub overdue: usize,
    pub today: usize,
}

impl Counts {
    /// Nothing is overdue or due today.
    pub fn is_empty(self) -> bool {
        self.overdue == 0 && self.today == 0
    }
}

/// One task of the queue: where it is in [`GlanceData::todos`] and whether it is overdue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub index: usize,
    pub overdue: bool,
}

/// A due as local time; a date-only due is the start of its day.
fn local(due: EventTime) -> NaiveDateTime {
    match due {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => to_local(at),
    }
}

/// Open tasks that are due before today and those due today. Done tasks and tasks without a due
/// date count in neither.
pub fn counts(todos: &[Todo], today: NaiveDate) -> Counts {
    let mut counts = Counts::default();
    for todo in todos.iter().filter(|todo| !todo.completed) {
        match todo.due.map(|due| local(due).date().cmp(&today)) {
            Some(Ordering::Less) => counts.overdue += 1,
            Some(Ordering::Equal) => counts.today += 1,
            _ => {}
        }
    }
    counts
}

/// iCalendar priorities run 1 (highest) to 9 (lowest); 0 and anything else count as none.
fn priority_rank(priority: Option<u8>) -> Option<u8> {
    match priority {
        Some(rank @ 1..=9) => Some(rank),
        _ => None,
    }
}

/// The open tasks with a due date in the order a glance shows them: by due (so overdue first,
/// then today, then the days after), then priority (none last), then title – the list's order, but
/// flat: a subtask stands where its own due puts it, not under its parent. Tasks without a due
/// date are not here; a glance is about what is due.
pub fn queue(todos: &[Todo], today: NaiveDate) -> Vec<Pick> {
    let mut picks: Vec<(usize, NaiveDateTime)> = todos
        .iter()
        .enumerate()
        .filter(|(_, todo)| !todo.completed)
        .filter_map(|(index, todo)| Some((index, local(todo.due?))))
        .collect();
    picks.sort_by(|&(a, a_due), &(b, b_due)| {
        a_due
            .cmp(&b_due)
            .then_with(|| match (priority_rank(todos[a].priority), priority_rank(todos[b].priority)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            })
            .then_with(|| todos[a].summary.to_lowercase().cmp(&todos[b].summary.to_lowercase()))
    });
    picks.into_iter().map(|(index, due)| Pick { index, overdue: due.date() < today }).collect()
}

/// The parts of the numbers line as `(text, overdue)`: "3 overdue", "5 today"; a part that is
/// zero is left out.
pub fn count_parts(counts: Counts) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    if counts.overdue > 0 {
        parts.push((trf("{n} overdue", &[("n", &counts.overdue)]), true));
    }
    if counts.today > 0 {
        parts.push((trf("{n} today", &[("n", &counts.today)]), false));
    }
    parts
}

/// The calm line when nothing is overdue or due today.
pub fn calm() -> &'static str {
    tr("All done for today")
}

/// The due date as a row shows it: "Yesterday", "Tomorrow", "Mon, 12.10." (just "12.10." when
/// `compact`, for a tile too narrow for the weekday); a task due today with a time of day shows
/// just the time ("18:00"), since the numbers above already say it is today.
pub fn due_label(due: EventTime, today: NaiveDate, compact: bool) -> String {
    let at = local(due);
    let day = at.date();
    match due {
        EventTime::DateTime(_) if day == today => format_time(at.time()),
        _ if compact && (day - today).num_days().abs() > 1 => day.format("%d.%m.").to_string(),
        _ => due_text(EventTime::Date(day), today),
    }
}

/// The height of the line with the numbers (or the calm line), at the largest level.
fn head_height() -> f32 {
    line_height(HEAD_LEVELS[0])
}

/// The height of the quiet error line with its gap above.
fn error_height() -> f32 {
    space::XS + line_height(typo::META)
}

/// How many rows fit in a mini tile `height` high under the numbers, with room for the error line
/// if there is one.
pub fn rows_that_fit(height: f32, error: bool) -> usize {
    let above = 2.0 * space::M + head_height() + space::S + if error { error_height() } else { 0.0 };
    ((height - above) / ROW).floor().max(0.0) as usize
}

/// How many chip lines fit in a strip `height` high.
pub fn lines_that_fit(height: f32) -> usize {
    let room = height - 2.0 * space::S;
    ((room + space::XS) / (style::CHIP_HEIGHT + space::XS)).floor().max(0.0) as usize
}

/// The first level of `levels` in which `text` fits `width`, else the last one.
fn fit_level(text: &str, levels: &[typo::Level], width: f32) -> typo::Level {
    levels.iter().copied().find(|level| text_width(text, *level) <= width).unwrap_or(levels[levels.len() - 1])
}

/// Lays chips of the given `widths` out line by line, `gap` apart, in `avail` width and at most
/// `lines` lines. Returns the chips of each line (indices into `widths`) and how many chips are
/// left out. When some are left out, the last line keeps `more` width (plus a gap) free for the
/// "+4" at its end, which may cost one more chip.
pub fn flow(widths: &[f32], avail: f32, lines: usize, gap: f32, more: f32) -> (Vec<Vec<usize>>, usize) {
    // Greedy placement of every chip: its line and where it ends in that line.
    let mut places: Vec<(usize, f32)> = Vec::with_capacity(widths.len());
    let (mut line, mut x) = (0, 0.0);
    for &width in widths {
        if x > 0.0 && x + gap + width > avail {
            line += 1;
            x = 0.0;
        }
        x += if x > 0.0 { gap } else { 0.0 } + width;
        places.push((line, x));
    }
    let placeable = places.iter().take_while(|(line, _)| *line < lines).count();
    let kept = if placeable == widths.len() {
        placeable
    } else {
        (0..=placeable).rev().find(|&n| n == 0 || places[n - 1].1 + gap + more <= avail).unwrap_or(0)
    };
    let mut out: Vec<Vec<usize>> = Vec::new();
    for (index, &(line, _)) in places.iter().take(kept).enumerate() {
        if out.len() <= line {
            out.push(Vec::new());
        }
        out[line].push(index);
    }
    (out, widths.len() - kept)
}

/// The round check of a task: it ticks the task off (`Message::ToggleDone(index)`). Only open
/// tasks are shown, so it is always empty. The click target reaches past the 16-px circle –
/// `above` to the top and bottom, `left` to the left, `right` to the right of it – so it is not
/// smaller than 24 × 24; the circle itself sits `left` in from the edge of the target.
fn check(index: usize, color: Color, above: f32, left: f32, right: f32) -> Element<'static, Message> {
    let circle = checkbox(false)
        .size(style::CHECK_SIZE)
        .style(style::task_check(color))
        .on_toggle(move |_| Message::ToggleDone(index));
    let target = container(circle).padding(Padding { top: above, right, bottom: above, left });
    mouse_area(target).on_press(Message::ToggleDone(index)).interaction(mouse::Interaction::Pointer).into()
}

/// A line in `META` and `text_3`, never wrapped.
fn quiet(line: impl text::IntoFragment<'static>) -> iced::widget::Text<'static> {
    typo::text(typo::META, line).style(theme::text_3).wrapping(Wrapping::None)
}

/// The error line: `META` in the error colour, cut to `width`.
fn error_line(error: &str, width: f32) -> Element<'static, Message> {
    typo::text(typo::META, ellipsize(error, typo::META, width))
        .wrapping(Wrapping::None)
        .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
        .into()
}

/// One part of the numbers line in `level`: overdue in the attention colour, today in `text`.
fn part(content: String, overdue: bool, level: typo::Level) -> iced::widget::Text<'static> {
    typo::text(level, content).wrapping(Wrapping::None).style(move |t: &Theme| {
        let p = palette(t);
        text::Style { color: Some(if overdue { p.attention_text } else { p.text }) }
    })
}

/// "3 overdue · 5 today" in one line, in the largest level of [`HEAD_LEVELS`] that fits `width`.
fn numbers_line(parts: &[(String, bool)], width: f32) -> Element<'static, Message> {
    let joined = parts.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>().join(SEPARATOR);
    let level = fit_level(&joined, &HEAD_LEVELS, width);
    let mut line = row![].align_y(Alignment::Center);
    for (n, (content, overdue)) in parts.iter().enumerate() {
        if n > 0 {
            line = line.push(typo::text(level, SEPARATOR).style(theme::text_3).wrapping(Wrapping::None));
        }
        line = line.push(part(content.clone(), *overdue, level));
    }
    container(line).height(head_height()).center_y(head_height()).into()
}

/// The calm line, in the largest level that fits `width`, cut when even the smallest does not.
fn calm_line(width: f32) -> Element<'static, Message> {
    let level = fit_level(calm(), &HEAD_LEVELS, width);
    let content = ellipsize(calm(), level, width);
    container(typo::text(level, content).style(theme::text_2).wrapping(Wrapping::None))
        .height(head_height())
        .center_y(head_height())
        .into()
}

/// The empty tile: a quiet check mark and "All done for today", centred. Nothing is open, so there
/// is nothing to do here.
fn all_done(width: f32) -> Element<'static, Message> {
    let mark = icon::icon(Icon::CircleCheck, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).text_3) });
    let content = column![mark, calm_line(width)].spacing(space::S).align_x(Alignment::Center);
    container(content).center(Fill).into()
}

/// The tile around a glance: the panel colour, `padding` in from the edge, clipped.
fn tile(content: impl Into<Element<'static, Message>>, padding: Padding) -> Element<'static, Message> {
    container(content).padding(padding).width(Fill).height(Fill).clip(true).style(style::container::panel_flush).into()
}

/// The colour of the check of `todo`: its list's, or the standard one.
fn color_of(data: &GlanceData<'_>, todo: &Todo) -> Color {
    data.colors.get(&todo.collection).copied().unwrap_or_else(|| widgets::collection_color(None))
}

/// The width a row's title has left of `width` when `due` stands at the right.
fn title_room(width: f32, due: &str) -> f32 {
    width - style::CHECK_SIZE - space::S - text_width(due, typo::META) - space::S
}

/// Whether the dates of the `shown` rows lose their weekday: when one title would otherwise be
/// cut. All rows decide together, so the dates do not come in two shapes.
fn compact_dates(shown: &[Pick], data: &GlanceData<'_>, width: f32) -> bool {
    shown.iter().any(|pick| {
        let todo = &data.todos[pick.index];
        todo.due.is_some_and(|due| {
            let full = due_label(due, data.today, false);
            text_width(&todo.summary, typo::BODY) > title_room(width, &full)
        })
    })
}

/// One task of the mini tile: check, title cut to fit, due date at the right (overdue in the
/// attention colour, with the word as well: "Yesterday", never just a colour).
fn row_view(pick: Pick, data: &GlanceData<'_>, width: f32, compact: bool) -> Element<'static, Message> {
    let todo = &data.todos[pick.index];
    let above = (ROW - style::CHECK_SIZE) / 2.0;
    let check = check(pick.index, color_of(data, todo), above, 0.0, space::S);
    let due = todo.due.map(|due| due_label(due, data.today, compact));
    let title_width = due.as_deref().map_or(width - style::CHECK_SIZE - space::S, |due| title_room(width, due));
    let title = typo::text(typo::BODY, ellipsize(&todo.summary, typo::BODY, title_width)).wrapping(Wrapping::None);
    let mut line = row![check, title].align_y(Alignment::Center).height(ROW);
    if let Some(due) = due {
        let overdue = pick.overdue;
        line = line.push(Space::new().width(Fill)).push(
            typo::text(typo::META.tabular(), due).wrapping(Wrapping::None).style(move |t: &Theme| {
                let p = palette(t);
                text::Style { color: Some(if overdue { p.attention_text } else { p.text_3 }) }
            }),
        );
    }
    line.into()
}

/// The mini tile for a window of `size`: the numbers (or the calm line) at the top, then the next
/// open tasks that fit the height, then "+4 more". All done: a calm check mark and the line.
pub fn mini(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    let width = (size.width - 2.0 * space::M).max(0.0);
    let counts = counts(data.todos, data.today);
    let picks = queue(data.todos, data.today);
    if counts.is_empty() && picks.is_empty() && data.error.is_none() {
        return tile(all_done(width), Padding::new(space::M));
    }

    let parts = count_parts(counts);
    let head = if parts.is_empty() { calm_line(width) } else { numbers_line(&parts, width) };
    let mut content = column![head];
    if let Some(error) = data.error {
        content = content.push(Space::new().height(space::XS)).push(error_line(error, width));
    }
    content = content.push(Space::new().height(space::S));

    let (shown, more) = shown_rows(picks.len(), rows_that_fit(size.height, data.error.is_some()));
    let compact = compact_dates(&picks[..shown], data, width);
    for pick in &picks[..shown] {
        content = content.push(row_view(*pick, data, width, compact));
    }
    if let Some(more) = more {
        let label = trf("+{n} more", &[("n", &more)]);
        content = content.push(container(quiet(label)).height(ROW).center_y(ROW));
    }
    tile(content, Padding::new(space::M))
}

/// Width of a chip in front of its title: the left padding, the circle and the gap after it.
fn chip_lead() -> f32 {
    style::CHIP_PADDING.left + style::CHECK_SIZE + space::XS
}

/// The estimated width of the chip of `title`: its text, what stands before and after it, and the
/// frame; never wider than `max`.
pub fn chip_width(title: &str, max: f32) -> f32 {
    (chip_lead() + text_width(title, typo::CAPTION) + style::CHIP_PADDING.right + 2.0 * FRAME).min(max)
}

/// One task of the strip as a chip `width` wide: the check, then the title in `CAPTION` cut with
/// "…" to what is left.
fn chip(pick: Pick, data: &GlanceData<'_>, width: f32) -> Element<'static, Message> {
    let todo = &data.todos[pick.index];
    let above = (style::CHIP_HEIGHT - style::CHECK_SIZE) / 2.0;
    let check = check(pick.index, color_of(data, todo), above, style::CHIP_PADDING.left, space::XS);
    let title_width = width - chip_lead() - style::CHIP_PADDING.right - 2.0 * FRAME;
    let title =
        typo::text(typo::CAPTION, ellipsize(&todo.summary, typo::CAPTION, title_width)).wrapping(Wrapping::None);
    container(row![check, title].align_y(Alignment::Center))
        .width(width)
        .height(style::CHIP_HEIGHT)
        .center_y(style::CHIP_HEIGHT)
        .clip(true)
        .style(move |t: &Theme| {
            let chip = style::container::chip(t);
            // An overdue task has an orange frame; the word "overdue" at the left says it too.
            if pick.overdue {
                container::Style { border: chip.border.color(palette(t).attention), ..chip }
            } else {
                chip
            }
        })
        .into()
}

/// The numbers of the strip: one part per line, so the column is narrow. Calm when both are zero.
fn numbers_column(parts: &[(String, bool)], width: f32) -> Element<'static, Message> {
    if parts.is_empty() {
        return calm_line(width);
    }
    let level = parts
        .iter()
        .map(|(text, _)| fit_level(text, &HEAD_LEVELS, width))
        .min_by(|a, b| a.size.0.total_cmp(&b.size.0))
        .unwrap_or(HEAD_LEVELS[0]);
    let mut lines = column![];
    for (content, overdue) in parts {
        lines = lines.push(part(ellipsize(content, level, width), *overdue, level));
    }
    lines.into()
}

/// The strip for a window of `size`: at the left the numbers (or the calm line), beside them the
/// next open tasks as chips in as many lines as fit, and "+4" for those that do not.
pub fn strip(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    let inner = (size.width - 2.0 * space::M).max(0.0);
    let counts = counts(data.todos, data.today);
    let picks = queue(data.todos, data.today);
    if counts.is_empty() && picks.is_empty() && data.error.is_none() {
        return tile(all_done(inner), Padding::new(space::S));
    }

    let parts = count_parts(counts);
    let wanted = if parts.is_empty() {
        text_width(calm(), HEAD_LEVELS[1])
    } else {
        parts.iter().map(|(text, _)| text_width(text, HEAD_LEVELS[0])).fold(0.0, f32::max)
    };
    let left = wanted.min(size::SIDEBAR).min(inner / STRIP_INFO_SHARE).ceil();
    let mut info = column![numbers_column(&parts, left)];
    if let Some(error) = data.error {
        info = info.push(Space::new().height(space::XS)).push(error_line(error, left));
    }

    let avail = (inner - left - space::L).max(0.0);
    let more_label = |n: usize| format!("+{n}");
    // Room for the widest count that can occur, so the cut does not depend on the count itself.
    let more_room = text_width(&more_label(picks.len()), typo::META) + space::S;
    let widths: Vec<f32> =
        picks.iter().map(|pick| chip_width(&data.todos[pick.index].summary, CHIP_MAX).min(avail)).collect();
    let (lines, left_out) = flow(&widths, avail, lines_that_fit(size.height), space::XS, more_room);

    let mut chips = column![].spacing(space::XS);
    let last = lines.len().saturating_sub(1);
    for (n, line) in lines.iter().enumerate() {
        let mut chips_line = row![].spacing(space::XS).align_y(Alignment::Center);
        for &index in line {
            chips_line = chips_line.push(chip(picks[index], data, widths[index]));
        }
        if n == last && left_out > 0 {
            chips_line = chips_line.push(quiet(more_label(left_out)));
        }
        chips = chips.push(chips_line);
    }
    if lines.is_empty() && left_out > 0 {
        chips = chips.push(quiet(more_label(left_out)));
    }
    // Numbers and chips stand in the middle of the strip's height, not stuck to its top.
    let content = row![container(info).width(left).clip(true), container(chips).width(Fill).clip(true)]
        .spacing(space::L)
        .align_y(Alignment::Center)
        .height(Fill);
    tile(content, Padding { top: space::S, bottom: space::S, left: space::M, right: space::M })
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_gui::datetime::to_utc;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")
    }

    /// Sunday, 11 October 2026.
    fn today() -> NaiveDate {
        day(11)
    }

    fn todo(summary: &str) -> Todo {
        Todo {
            uid: summary.into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: summary.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
        }
    }

    fn due(summary: &str, d: u32) -> Todo {
        Todo { due: Some(EventTime::Date(day(d))), ..todo(summary) }
    }

    fn at(summary: &str, d: u32, hour: u32) -> Todo {
        let local = day(d).and_hms_opt(hour, 0, 0).expect("valid time");
        Todo { due: Some(EventTime::DateTime(to_utc(local))), ..todo(summary) }
    }

    fn done(todo: Todo) -> Todo {
        Todo { completed: true, ..todo }
    }

    fn names(todos: &[Todo], picks: &[Pick]) -> Vec<String> {
        picks.iter().map(|pick| todos[pick.index].summary.clone()).collect()
    }

    #[test]
    fn counts_split_open_tasks_into_overdue_and_today() {
        let todos = vec![
            due("two days ago", 9),
            due("yesterday", 10),
            at("this evening", 11, 18),
            due("today", 11),
            due("tomorrow", 12),
            todo("undated"),
            done(due("done and overdue", 8)),
            done(due("done today", 11)),
        ];
        assert_eq!(counts(&todos, today()), Counts { overdue: 2, today: 2 });
        assert!(counts(&[], today()).is_empty());
        assert!(counts(&[due("tomorrow", 12), todo("undated")], today()).is_empty());
    }

    #[test]
    fn a_time_of_day_earlier_today_is_not_overdue() {
        let todos = vec![at("this morning", 11, 6)];
        assert_eq!(counts(&todos, today()), Counts { overdue: 0, today: 1 });
        assert_eq!(queue(&todos, today()), [Pick { index: 0, overdue: false }]);
    }

    #[test]
    fn the_queue_runs_overdue_then_today_then_by_due_date() {
        let todos = vec![
            due("in a week", 18),
            due("today", 11),
            due("yesterday", 10),
            at("this evening", 11, 18),
            due("tomorrow", 12),
            due("last week", 5),
        ];
        let picks = queue(&todos, today());
        assert_eq!(names(&todos, &picks), ["last week", "yesterday", "today", "this evening", "tomorrow", "in a week"]);
        let overdue: Vec<bool> = picks.iter().map(|pick| pick.overdue).collect();
        assert_eq!(overdue, [true, true, false, false, false, false]);
    }

    #[test]
    fn the_queue_skips_done_and_undated_tasks() {
        let todos = vec![todo("undated"), done(due("done", 10)), due("open", 11)];
        assert_eq!(names(&todos, &queue(&todos, today())), ["open"]);
        assert!(queue(&[], today()).is_empty());
    }

    #[test]
    fn same_day_ties_go_by_priority_then_title() {
        let with = |summary: &str, priority: Option<u8>| Todo { priority, ..due(summary, 11) };
        let todos = vec![with("zeta", None), with("low", Some(9)), with("Beta", None), with("high", Some(1))];
        assert_eq!(names(&todos, &queue(&todos, today())), ["high", "low", "Beta", "zeta"]);
    }

    #[test]
    fn a_subtask_stands_where_its_own_due_puts_it() {
        let child = Todo { parent: Some("parent".into()), ..due("child", 10) };
        let todos = vec![due("parent", 12), child];
        assert_eq!(names(&todos, &queue(&todos, today())), ["child", "parent"]);
    }

    #[test]
    fn the_queue_keeps_the_index_into_the_apps_tasks() {
        let todos = vec![done(due("done", 10)), due("later", 14), due("sooner", 12)];
        let picks = queue(&todos, today());
        assert_eq!(picks.iter().map(|pick| pick.index).collect::<Vec<_>>(), [2, 1]);
    }

    #[test]
    fn the_numbers_leave_out_zero_parts() {
        let overdue = |n: usize| (trf("{n} overdue", &[("n", &n)]), true);
        let today_ = |n: usize| (trf("{n} today", &[("n", &n)]), false);
        assert_eq!(count_parts(Counts { overdue: 3, today: 5 }), [overdue(3), today_(5)]);
        assert_eq!(count_parts(Counts { overdue: 0, today: 5 }), [today_(5)]);
        assert_eq!(count_parts(Counts { overdue: 2, today: 0 }), [overdue(2)]);
        assert!(count_parts(Counts::default()).is_empty());
        assert_eq!(calm(), tr("All done for today"));
    }

    #[test]
    fn due_labels_name_the_day_or_the_time() {
        let date = |d: u32| EventTime::Date(day(d));
        let label = |due, compact| due_label(due, today(), compact);
        assert_eq!(label(date(11), false), tr("Today"));
        assert_eq!(label(date(10), false), tr("Yesterday"));
        assert_eq!(label(date(12), false), tr("Tomorrow"));
        let evening = at("x", 11, 18).due.expect("due");
        assert_eq!(label(evening, false), "18:00", "today with a time: just the time");
        let tomorrow = at("x", 12, 9).due.expect("due");
        assert_eq!(label(tomorrow, false), tr("Tomorrow"), "another day: the day, not the time");
        assert_eq!(label(date(7), false), format!("{}, 07.10.", tr("Wed")));
        assert_eq!(label(date(7), true), "07.10.", "compact: without the weekday");
        assert_eq!(label(date(10), true), tr("Yesterday"), "the words stay");
    }

    #[test]
    fn the_height_of_the_tile_decides_how_many_rows_fit() {
        // 240×160: 12 above and below, the numbers (22), 8 gap, rows of 28.
        assert_eq!(rows_that_fit(160.0, false), 3);
        assert_eq!((rows_that_fit(150.0, false), rows_that_fit(150.0, true)), (3, 2), "the error line takes room");
        assert_eq!(rows_that_fit(100.0, false), 1);
        assert_eq!(rows_that_fit(40.0, false), 0);
        assert!(rows_that_fit(480.0, false) >= 12);
    }

    #[test]
    fn chip_lines_follow_the_height() {
        assert_eq!(lines_that_fit(200.0), 6);
        assert_eq!(lines_that_fit(120.0), 3);
        assert_eq!(lines_that_fit(30.0), 0);
    }

    #[test]
    fn chips_flow_into_lines_and_say_what_is_left_out() {
        // Everything fits.
        assert_eq!(flow(&[40.0, 40.0], 100.0, 2, 4.0, 20.0), (vec![vec![0, 1]], 0));
        // A new line when the next chip does not fit the rest of this one.
        assert_eq!(flow(&[60.0, 60.0, 30.0], 100.0, 3, 4.0, 20.0), (vec![vec![0], vec![1, 2]], 0));
        // One line of 100: 40 + 4 + 40 = 84, the third does not fit, and the "+1" needs 24 more
        // after the second: so the second goes as well and "+2" follows the first.
        assert_eq!(flow(&[40.0, 40.0, 40.0], 100.0, 1, 4.0, 20.0), (vec![vec![0]], 2));
        // With room to spare, the "+1" sits after the second.
        assert_eq!(flow(&[40.0, 40.0, 40.0], 120.0, 1, 4.0, 20.0), (vec![vec![0, 1]], 1));
        // No line at all, or not even one chip with its count: only the count.
        assert_eq!(flow(&[40.0, 40.0], 100.0, 0, 4.0, 20.0), (vec![], 2));
        assert_eq!(flow(&[90.0, 40.0], 100.0, 1, 4.0, 20.0), (vec![], 2));
        assert_eq!(flow(&[], 100.0, 2, 4.0, 20.0), (vec![], 0));
    }

    #[test]
    fn chips_are_as_wide_as_their_title_but_never_too_wide() {
        let short = chip_width("Oats", CHIP_MAX);
        let long = chip_width("Write the quarterly report for the whole department", CHIP_MAX);
        assert!(short < long);
        assert_eq!(long, CHIP_MAX);
        assert!(short >= chip_lead() + style::CHIP_PADDING.right);
    }

    #[test]
    fn the_numbers_take_the_smaller_level_when_the_big_one_does_not_fit() {
        assert_eq!(fit_level("3 overdue", &HEAD_LEVELS, 200.0), HEAD_LEVELS[0]);
        assert_eq!(fit_level("12 overdue · 12 today", &HEAD_LEVELS, 176.0), HEAD_LEVELS[1]);
        assert_eq!(fit_level("a very long line indeed that fits nowhere", &HEAD_LEVELS, 50.0), HEAD_LEVELS[1]);
    }

    fn data(todos: &[Todo]) -> GlanceData<'_> {
        GlanceData { todos, colors: HashMap::new(), today: today(), error: None }
    }

    #[test]
    fn the_views_build_in_every_size_and_state() {
        let many: Vec<Todo> =
            (1..=30u32).map(|n| due(&format!("Task number {n} with a longer title"), 8 + n % 6)).collect();
        let sets = [vec![], vec![due("one", 11)], vec![due("late", 9)], vec![todo("undated only")], many];
        let sizes = [(200.0, 120.0), (240.0, 160.0), (320.0, 480.0), (600.0, 120.0), (1200.0, 200.0), (3000.0, 270.0)];
        for todos in &sets {
            for (width, height) in sizes {
                let size = Size::new(width, height);
                let _ = mini(&data(todos), size);
                let _ = strip(&data(todos), size);
                let with_error = GlanceData { error: Some("Save failed: no network"), ..data(todos) };
                let _ = mini(&with_error, size);
                let _ = strip(&with_error, size);
            }
        }
    }
}
