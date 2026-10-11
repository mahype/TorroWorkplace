//! The list of entries (tasks E3.1 and E3.2b in `docs/tasks/eingang.md`).
//!
//! One `widgets::list_row` per entry, in the order the source gives them ([`torro_inbox::rules`]:
//! urgent first, then the newest): the source's icon (in the attention colour while an urgent
//! entry is open – never Torro red), the title on one line (medium weight while new), a preview of
//! the body in `text_3`, how many reports were merged (from two on) and the time. A click on the
//! row → `Select(id)`. Nothing to show → the empty state of the view, with the view's icon.
//!
//! **Urgency never by colour alone** (E3.2b): while urgent entries are open, the list is grouped
//! like torrotasks – a head "Waiting for you" over them and "Other" over the rest ([`groups`]).
//! Without anything waiting there are no heads.
//!
//! **No hard cut** (E3.2b): Iced 0.14 cannot end a line with "…", so the row works out its free
//! width ([`text_room`]) and cuts title and preview with an ellipsis itself ([`fit`], with
//! `typo::ellipsize`'s estimate of Inter). The preview only stands beside the title when about
//! [`PREVIEW_ROOM`] are left; otherwise it is dropped. Narrow windows get two lines per row
//! instead: the title, and the preview in `META` below it. The cells still clip, in case the
//! estimate runs over.
//!
//! The row under the cursor (E3.2) is the highlighted one; it registers [`ListData::anchor`], so
//! the snooze menu and the lane question open beside it. The list reports its scroll position
//! (`Message::Scrolled`), so the app can keep the cursor in view ([`row_span`]).
//!
//! Titles and bodies of other sources are foreign text (`Item::untrusted`). Iced draws text as it
//! is, so nothing in it is ever interpreted; [`one_line`] and [`plain`] also drop control and
//! direction characters, so a title can neither break the row into lines nor turn its text around.

use std::ops::Range;

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{center, column, container, responsive, row, scrollable, svg};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::{self, Level};
use torro_gui::widgets::{self, Anchor, Mark};
use torro_gui::{datetime, style};
use torro_inbox::{Item, SourceKind, SourceRef, State};

use crate::app::{Message, View};

/// What the list gets to draw.
pub struct ListData<'a> {
    /// The entries of the view, in the order to show them.
    pub items: &'a [Item],
    pub view: &'a View,
    /// Id of the entry under the cursor (and in the panel, when it is open).
    pub selected: Option<&'a str>,
    /// "Now", for the times ("4 min").
    pub now: DateTime<Utc>,
    /// Registered by the selected row, for the menus that open beside it.
    pub anchor: Anchor,
    /// Two lines per row (title over preview) instead of one: narrow windows, where a preview
    /// beside the title would hardly ever fit.
    pub two_line: bool,
    /// The attention colour of the current theme, for the icon of the "Waiting for you" head.
    pub attention: Color,
}

/// The id of the list's scrollable, to scroll the cursor into view.
pub const SCROLL: &str = "inbox-list";

/// The free width a preview needs beside the title before it is shown at all: below that only a
/// word or two would fit, which reads as noise rather than as a preview.
pub const PREVIEW_ROOM: f32 = 120.0;

/// The gap the row estimate counts between two cells: a little more than `list_row`'s own (10),
/// so the estimate cuts a letter early rather than running under the next cell.
const CELL_GAP: f32 = space::M;

/// Height of a group head: the 4 above it plus the head itself (`widgets::group_head`).
const GROUP_HEAD: f32 = space::XS + size::ROW_GROUP;
/// What the first head leaves out: the gap above it and its line above, because the header bar
/// right above already ends with a line, and a second one 4 below it would read as a double.
const FIRST_TRIM: f32 = space::XS + size::HAIRLINE;

/// A group of the list while something is waiting ([`groups`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Open urgent entries: waiting agents, approvals.
    Waiting,
    /// Everything else in the view.
    Other,
}

impl Group {
    /// The head's name.
    pub fn label(self) -> &'static str {
        match self {
            Group::Waiting => tr("Waiting for you"),
            Group::Other => tr("Other"),
        }
    }

    /// The head's icon; "Waiting for you" carries the attention colour, like torrotasks' "Overdue".
    fn mark(self, attention: Color) -> Mark {
        match self {
            Group::Waiting => Mark::Tinted(Icon::CircleAlert, attention),
            Group::Other => Mark::Icon(Icon::Layers),
        }
    }
}

/// Whether `item` waits for the user: urgent and still open. These rows lead the list.
pub fn is_waiting(item: &Item) -> bool {
    item.urgent && item.state.is_open()
}

/// The groups of `items` with their ranges, top to bottom: "Waiting for you" over the leading
/// run of waiting entries ([`is_waiting`]), "Other" over the rest (when there is a rest). Empty
/// when nothing waits – the list then has no heads. The app keeps waiting entries first
/// (`App::take`), so every waiting entry is in the first group.
pub fn groups(items: &[Item]) -> Vec<(Group, Range<usize>)> {
    let waiting = items.iter().take_while(|item| is_waiting(item)).count();
    if waiting == 0 {
        return Vec::new();
    }
    let mut groups = vec![(Group::Waiting, 0..waiting)];
    if waiting < items.len() {
        groups.push((Group::Other, waiting..items.len()));
    }
    groups
}

/// Where row `index` of `items` lies in the list's content as `(top, bottom)`; the first row of a
/// group starts at its head, so scrolling up to it shows the head too. `None` past the end. Every
/// row is `size::ROW` high, in one line or two.
pub fn row_span(items: &[Item], index: usize) -> Option<(f32, f32)> {
    let groups = groups(items);
    if groups.is_empty() {
        let top = index as f32 * size::ROW;
        return (index < items.len()).then_some((top, top + size::ROW));
    }
    // The first head is shorter by `FIRST_TRIM`; it still starts at the top of the content.
    let mut y = -FIRST_TRIM;
    for (_, range) in groups {
        let head = y.max(0.0);
        y += GROUP_HEAD;
        for row in range.clone() {
            if row == index {
                return Some((if row == range.start { head } else { y }, y + size::ROW));
            }
            y += size::ROW;
        }
    }
    None
}

/// The list, scrolling; the empty state of the view when there are no entries.
pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    if data.items.is_empty() {
        let (title, hint) = empty_text(data.view);
        return empty_state(data.view.icon(), title, hint);
    }
    // The rows need their width to cut title and preview ([`fit`]).
    responsive(move |bounds| list(&data, bounds.width)).into()
}

/// The rows at `width`, under their group heads when something waits.
fn list<'a>(data: &ListData<'a>, width: f32) -> Element<'a, Message> {
    let mut rows = column![].width(Fill);
    let groups = groups(data.items);
    if groups.is_empty() {
        rows = rows.extend(data.items.iter().map(|item| row_view(data, item, width)));
    }
    for (n, (group, range)) in groups.into_iter().enumerate() {
        let head = widgets::group_head(group.mark(data.attention), group.label(), Some(range.len()), None);
        rows = rows.push(if n == 0 { without_top(head) } else { head });
        rows = rows.extend(data.items[range].iter().map(|item| row_view(data, item, width)));
    }
    scrollable(rows)
        .id(SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The group head without its gap and line above: moved up by [`FIRST_TRIM`] inside a clipping
/// container, so only the part above the head's top line is cut off (as in torrotasks).
fn without_top<'a>(head: Element<'a, Message>) -> Element<'a, Message> {
    container(container(head).padding(Padding { top: -FIRST_TRIM, ..Padding::ZERO })).clip(true).into()
}

/// One entry in a row `width` wide: icon, title and preview (one line, or two in narrow windows),
/// count, time.
fn row_view<'a>(data: &ListData<'a>, item: &'a Item, width: f32) -> Element<'a, Message> {
    let urgent = is_waiting(item);
    let mark = icon::icon(source_icon(&item.source), icon::SIZE).style(move |t: &Theme, _| {
        let p = palette(t);
        svg::Style { color: Some(if urgent { p.attention_text } else { p.text_3 }) }
    });
    let level = if item.state == State::New { typo::BODY_STRONG } else { typo::BODY };
    let count = (item.count > 1).then(|| item.count.to_string());
    let time = time_text(item, data.view, data.now);
    let room = text_room(width, count.as_deref(), &time);
    let (title, preview) = fit(&one_line(&item.title), preview(item).as_deref(), level, room, data.two_line);
    let title = typo::text(level, title).style(theme::text).wrapping(Wrapping::None);
    let words: Element<'a, Message> = match (data.two_line, preview) {
        (true, Some(preview)) => {
            column![title, typo::text(typo::META, preview).style(theme::text_3).wrapping(Wrapping::None)].into()
        }
        (false, Some(preview)) => {
            row![title, typo::text(typo::BODY, preview).style(theme::text_3).wrapping(Wrapping::None)]
                .spacing(space::S)
                .into()
        }
        (_, None) => title.into(),
    };
    // The texts are already cut to fit; the clip only catches an estimate that runs over, so they
    // never run under the count and the time, which keep their own width.
    let mut cells: Vec<Element<'a, Message>> = vec![mark.into(), container(words).width(Fill).clip(true).into()];
    if let Some(count) = count {
        cells.push(widgets::pill(None, count));
    }
    cells.push(typo::text(typo::META.tabular(), time).style(theme::text_3).wrapping(Wrapping::None).into());
    let selected = data.selected == Some(item.id.as_str());
    let row = widgets::list_row(cells, selected, Some(Message::Select(item.id.clone())));
    if selected { widgets::anchored(&data.anchor, row) } else { row }
}

/// The width left for title and preview in a row `width` wide: minus the row's sides, the icon,
/// the count pill (when there is one) and the time, each with the gap before it. Never negative.
pub fn text_room(width: f32, count: Option<&str>, time: &str) -> f32 {
    let pill = count.map_or(0.0, |count| typo::text_width(count, typo::META) + 2.0 * space::S + CELL_GAP);
    let time = typo::text_width(time, typo::META) + CELL_GAP;
    (width - 2.0 * space::L - icon::SIZE - CELL_GAP - pill - time).max(0.0)
}

/// Title and preview cut to fit `room` (see [`text_room`]), with "…" where they were cut:
///
/// - one line (`two_line == false`): the title in `level` first; the preview in `BODY` takes what
///   is left beside it after the gap, but only when that is at least [`PREVIEW_ROOM`] – else
///   there is no preview.
/// - two lines: title and preview (in `META`) each get the whole `room`.
///
/// A preview that would be cut to nothing is dropped.
pub fn fit(title: &str, preview: Option<&str>, level: Level, room: f32, two_line: bool) -> (String, Option<String>) {
    let title = typo::ellipsize(title, level, room);
    let (preview_level, preview_room) =
        if two_line { (typo::META, room) } else { (typo::BODY, room - typo::text_width(&title, level) - space::S) };
    let preview = preview
        .filter(|_| two_line || preview_room >= PREVIEW_ROOM)
        .map(|preview| typo::ellipsize(preview, preview_level, preview_room))
        .filter(|preview| !preview.is_empty());
    (title, preview)
}

/// The empty state of a view: its icon over what shows up here and when – the shared
/// `widgets::empty_state` with an icon on top, as the error state has one.
fn empty_state<'a>(which: Icon, title: &'a str, hint: &'a str) -> Element<'a, Message> {
    let mark = icon::icon(which, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let words = column![
        typo::text(typo::BODY_STRONG, title).align_x(Alignment::Center),
        typo::text(typo::BODY, hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::XS)
    .align_x(Alignment::Center);
    let content = column![mark, words].spacing(space::M).align_x(Alignment::Center).max_width(size::FORM_MAX / 2.0);
    center(content).padding(space::XL).into()
}

/// The icon of a source: the suite's grid, a spark for coding agents, a window for scripts.
pub fn source_icon(source: &SourceRef) -> Icon {
    match source.kind {
        SourceKind::Suite => Icon::LayoutGrid,
        SourceKind::Agent => Icon::Sparkles,
        SourceKind::Script => Icon::AppWindow,
    }
}

/// The first line of the body that has text, on one line ([`one_line`]); `None` without a body.
pub fn preview(item: &Item) -> Option<String> {
    let body = item.body.as_deref()?;
    body.lines().map(one_line).find(|line| !line.is_empty())
}

/// Characters that change the direction of the text around them (Unicode bidi controls). Foreign
/// text could use them to show something else than it says, so they are dropped.
fn is_direction_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` for a single line: line breaks and runs of white space become one space, control and
/// direction characters are dropped, the ends are trimmed.
pub fn one_line(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| !c.is_control() && !is_direction_control(c))
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` for a block that may wrap (the panel): line breaks stay, other control and direction
/// characters are dropped.
pub fn plain(text: &str) -> String {
    text.chars().filter(|&c| c == '\n' || (!c.is_control() && !is_direction_control(c))).collect()
}

/// The time at the end of a row: until when a snoozed entry sleeps ("until Wed, 14 Oct",
/// [`wake_text`]), how long ago it was done in "Done", otherwise how long ago it last changed
/// ([`ago`]).
pub fn time_text(item: &Item, view: &View, now: DateTime<Utc>) -> String {
    match (view, &item.state) {
        (_, State::Snoozed { until }) => trf("until {time}", &[("time", &wake_text(*until, now))]),
        (View::Done, State::Done { at }) => ago(*at, now),
        _ => ago(item.updated, now),
    }
}

/// How long ago `then` was, as short as it gets: "4 min" (at least 1), "2 h", "3 d".
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes().max(1);
    if minutes < 60 {
        trf("{n} min", &[("n", &minutes)])
    } else if minutes < 24 * 60 {
        trf("{n} h", &[("n", &(minutes / 60))])
    } else {
        trf("{n} d", &[("n", &(minutes / (24 * 60)))])
    }
}

/// How long ago `then` was, in words for the panel: "4 min ago" (at least 1), "2 h ago", "3 d ago".
pub fn ago_long(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes().max(1);
    if minutes < 60 {
        trf("{n} min ago", &[("n", &minutes)])
    } else if minutes < 24 * 60 {
        trf("{n} h ago", &[("n", &(minutes / 60))])
    } else {
        trf("{n} d ago", &[("n", &(minutes / (24 * 60)))])
    }
}

/// When a snoozed entry comes back, in local time: "18:30" today, else the short date
/// ("Wed, 14 Oct").
pub fn wake_text(until: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (until, today) = (until.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    if until.date_naive() == today {
        datetime::format_time(until.time())
    } else {
        datetime::format_short_date(until.date_naive(), today)
    }
}

/// What the empty state of `view` says: what shows up here, and when.
pub fn empty_text(view: &View) -> (&'static str, &'static str) {
    match view {
        View::ForMe => {
            (tr("Nothing for you right now."), tr("Waiting agents, approvals and account problems show up here."))
        }
        View::Info => (tr("No info right now."), tr("Notices from scripts and finished jobs show up here.")),
        View::Snoozed => (tr("Nothing snoozed."), tr("Snoozed entries wait here until their time.")),
        View::Done => (tr("Nothing done yet."), tr("Done entries stay here for 30 days.")),
        View::Source(_) => (tr("Nothing open from this source."), tr("New entries from it show up here.")),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone};
    use torro_inbox::demo;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    #[test]
    fn times_are_short_and_relative() {
        assert_eq!(ago(now(), now()), trf("{n} min", &[("n", &1)]), "never 0 min");
        assert_eq!(ago(now() - TimeDelta::minutes(4), now()), trf("{n} min", &[("n", &4)]));
        assert_eq!(ago(now() - TimeDelta::minutes(150), now()), trf("{n} h", &[("n", &2)]));
        assert_eq!(ago(now() - TimeDelta::hours(50), now()), trf("{n} d", &[("n", &2)]));
        assert_eq!(ago(now() + TimeDelta::hours(1), now()), trf("{n} min", &[("n", &1)]), "a clock ahead");
    }

    #[test]
    fn the_panel_says_ago_in_words() {
        assert_eq!(ago_long(now() - TimeDelta::minutes(4), now()), trf("{n} min ago", &[("n", &4)]));
        assert_eq!(ago_long(now() - TimeDelta::minutes(150), now()), trf("{n} h ago", &[("n", &2)]));
        assert_eq!(ago_long(now() - TimeDelta::hours(50), now()), trf("{n} d ago", &[("n", &2)]));
    }

    #[test]
    fn snoozed_rows_say_when_they_come_back() {
        let local = |h| Local.with_ymd_and_hms(2026, 10, 13, h, 30, 0).single().expect("valid").to_utc();
        assert_eq!(wake_text(local(18), local(9)), "18:30");
        let tomorrow = local(9) + TimeDelta::days(1);
        let day = tomorrow.with_timezone(&Local).date_naive();
        assert_eq!(wake_text(tomorrow, local(9)), datetime::format_short_date(day, day - TimeDelta::days(1)));
    }

    #[test]
    fn rows_show_the_time_that_matters_in_their_view() {
        let items = demo::sample(now());
        let by_id = |id: &str| items.iter().find(|i| i.id == id).expect("in the sample");
        let snoozed = by_id("demo-10");
        let State::Snoozed { until } = snoozed.state else { panic!("demo-10 is snoozed") };
        let until = trf("until {time}", &[("time", &wake_text(until, now()))]);
        assert_eq!(time_text(snoozed, &View::Snoozed, now()), until, "until when, not only a date");
        let done = by_id("demo-11");
        assert_eq!(time_text(done, &View::Done, now()), trf("{n} h", &[("n", &22)]), "since it was done");
        assert_eq!(time_text(by_id("demo-1"), &View::ForMe, now()), trf("{n} min", &[("n", &4)]));
    }

    #[test]
    fn foreign_text_stays_on_one_line_and_keeps_its_direction() {
        assert_eq!(one_line("  Deploy\nfailed\t\tnow  "), "Deploy failed now");
        assert_eq!(one_line("abc\u{202E}fed\u{1b}[31m"), "abcfed[31m");
        assert_eq!(plain("line 1\nline\u{202E} 2\u{7}"), "line 1\nline 2");
    }

    #[test]
    fn the_preview_is_the_first_line_with_text() {
        let mut item = demo::sample(now()).remove(0);
        item.body = Some("\n  \nFirst  line\nsecond".into());
        assert_eq!(preview(&item).as_deref(), Some("First line"));
        item.body = Some(" \n ".into());
        assert_eq!(preview(&item), None);
        item.body = None;
        assert_eq!(preview(&item), None);
    }

    const TITLE: &str = "Claude Code is waiting for your permission";
    const PREVIEW: &str = "Claude needs your permission to use Bash: cargo test --workspace";

    #[test]
    fn a_preview_beside_the_title_needs_room_or_goes() {
        let title = typo::text_width(TITLE, typo::BODY_STRONG);
        // Plenty of room: both whole.
        let (t, p) = fit(TITLE, Some(PREVIEW), typo::BODY_STRONG, 2000.0, false);
        assert_eq!((t.as_str(), p.as_deref()), (TITLE, Some(PREVIEW)));
        // Less than the preview's room left beside the title: no preview, never a stub.
        let tight = title + space::S + PREVIEW_ROOM - 1.0;
        assert_eq!(fit(TITLE, Some(PREVIEW), typo::BODY_STRONG, tight, false), (TITLE.to_owned(), None));
        // Enough for a part: the preview ends in "…" within its room.
        let (_, p) = fit(TITLE, Some(PREVIEW), typo::BODY_STRONG, title + space::S + 200.0, false);
        let p = p.expect("a preview");
        assert!(p.ends_with(typo::ELLIPSIS), "{p}");
        assert!(typo::text_width(&p, typo::BODY) <= 200.0, "{p}");
    }

    #[test]
    fn a_long_title_ends_in_an_ellipsis_and_leaves_no_preview() {
        let (t, p) = fit(TITLE, Some(PREVIEW), typo::BODY_STRONG, 150.0, false);
        assert!(t.ends_with(typo::ELLIPSIS), "{t}");
        assert!(typo::text_width(&t, typo::BODY_STRONG) <= 150.0, "{t}");
        assert_eq!(p, None);
    }

    #[test]
    fn two_lines_give_the_preview_the_whole_width() {
        let (t, p) = fit(TITLE, Some(PREVIEW), typo::BODY_STRONG, 300.0, true);
        assert!(typo::text_width(&t, typo::BODY_STRONG) <= 300.0, "{t}");
        let p = p.expect("a preview below the title");
        assert!(typo::text_width(&p, typo::META) <= 300.0, "{p}");
        assert_eq!(fit(TITLE, None, typo::BODY_STRONG, 300.0, true).1, None);
    }

    #[test]
    fn the_room_leaves_out_icon_count_and_time() {
        let plain = text_room(480.0, None, "4 min");
        let counted = text_room(480.0, Some("2"), "4 min");
        assert!(plain < 480.0 - 2.0 * space::L - icon::SIZE, "minus the sides and the icon");
        assert!(counted < plain, "the count pill takes room too");
        assert_eq!(text_room(10.0, Some("12"), "50 min"), 0.0, "never negative");
    }

    #[test]
    fn waiting_entries_get_their_own_group() {
        let items: Vec<Item> = demo::sample(now()).into_iter().filter(|i| i.state.is_open()).collect();
        let mut sorted = items.clone();
        sorted.sort_by_key(|item| !is_waiting(item));
        let groups = groups(&sorted);
        assert_eq!(groups.iter().map(|(g, _)| *g).collect::<Vec<_>>(), [Group::Waiting, Group::Other]);
        let waiting = &groups[0].1;
        assert!(sorted[waiting.clone()].iter().all(is_waiting));
        assert!(sorted[waiting.end..].iter().all(|item| !is_waiting(item)));
        let calm: Vec<Item> = sorted[waiting.end..].to_vec();
        assert!(super::groups(&calm).is_empty(), "nothing waits: no heads");
    }

    #[test]
    fn row_spans_count_the_group_heads() {
        let mut items: Vec<Item> = demo::sample(now()).into_iter().filter(|i| i.state.is_open()).collect();
        items.sort_by_key(|item| !is_waiting(item));
        let waiting = items.iter().filter(|item| is_waiting(item)).count();
        let first_head = GROUP_HEAD - FIRST_TRIM;
        assert_eq!(row_span(&items, 0), Some((0.0, first_head + size::ROW)), "the first row shows its head");
        assert_eq!(row_span(&items, 1), Some((first_head + size::ROW, first_head + 2.0 * size::ROW)));
        let other = first_head + waiting as f32 * size::ROW;
        assert_eq!(row_span(&items, waiting), Some((other, other + GROUP_HEAD + size::ROW)));
        assert_eq!(row_span(&items, items.len()), None);
        let calm = &items[waiting..];
        assert_eq!(row_span(calm, 1), Some((size::ROW, 2.0 * size::ROW)), "no heads: plain rows");
    }

    #[test]
    fn every_source_kind_has_its_icon() {
        assert_eq!(source_icon(&SourceRef::suite()), Icon::LayoutGrid);
        assert_eq!(source_icon(&SourceRef::agent("codex")), Icon::Sparkles);
        assert_eq!(source_icon(&SourceRef::script("backup")), Icon::AppWindow);
    }

    #[test]
    fn the_empty_for_me_view_says_nothing_is_there() {
        assert_eq!(empty_text(&View::ForMe).0, tr("Nothing for you right now."));
        for view in [View::Info, View::Snoozed, View::Done, View::Source(SourceRef::suite())] {
            assert_ne!(empty_text(&view).0, empty_text(&View::ForMe).0, "{view:?} has its own text");
        }
    }
}
