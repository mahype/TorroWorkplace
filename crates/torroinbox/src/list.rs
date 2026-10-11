//! The list of entries (task E3.1 in `docs/tasks/eingang.md`).
//!
//! One `widgets::list_row` per entry, in the order the source gives them ([`torro_inbox::rules`]:
//! urgent first, then the newest): the source's icon (in the attention colour while an urgent
//! entry is open – never Torro red), the title on one line (medium weight while new), a preview of
//! the body in `text_3`, how many reports were merged (from two on) and the time. A click on the
//! row → `Select(id)`. Nothing to show → the empty state of the view.
//!
//! The row under the cursor (E3.2) is the highlighted one; it registers [`ListData::anchor`], so
//! the snooze menu and the lane question open beside it. The list reports its scroll position
//! (`Message::Scrolled`), so the app can keep the cursor in view; every row is `size::ROW` high.
//!
//! Titles and bodies of other sources are foreign text (`Item::untrusted`). Iced draws text as it
//! is, so nothing in it is ever interpreted; [`one_line`] and [`plain`] also drop control and
//! direction characters, so a title can neither break the row into lines nor turn its text around.

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, scrollable, svg};
use iced::{Element, Fill, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, space};
use torro_gui::widgets::Anchor;
use torro_gui::{datetime, style, typo, widgets};
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
}

/// The id of the list's scrollable, to scroll the cursor into view.
pub const SCROLL: &str = "inbox-list";

/// The list, scrolling; the empty state of the view when there are no entries.
pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    if data.items.is_empty() {
        let (title, hint) = empty_text(data.view);
        return widgets::empty_state(title, hint);
    }
    let rows = data.items.iter().map(|item| row_view(&data, item));
    scrollable(column(rows).width(Fill))
        .id(SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// One entry: icon, title and preview, count, time.
fn row_view<'a>(data: &ListData<'a>, item: &'a Item) -> Element<'a, Message> {
    let urgent = item.urgent && item.state.is_open();
    let mark = icon::icon(source_icon(&item.source), icon::SIZE).style(move |t: &Theme, _| {
        let p = palette(t);
        svg::Style { color: Some(if urgent { p.attention_text } else { p.text_3 }) }
    });
    let level = if item.state == State::New { typo::BODY_STRONG } else { typo::BODY };
    let mut words =
        row![typo::text(level, one_line(&item.title)).style(theme::text).wrapping(Wrapping::None)].spacing(space::S);
    if let Some(preview) = preview(item) {
        words = words.push(typo::text(typo::BODY, preview).style(theme::text_3).wrapping(Wrapping::None));
    }
    // Iced 0.14 has no ellipsis: title and preview take the free width and are clipped there, so
    // they never run under the count and the time, which keep their own width.
    let mut cells: Vec<Element<'a, Message>> = vec![mark.into(), container(words).width(Fill).clip(true).into()];
    if item.count > 1 {
        cells.push(widgets::pill(None, item.count.to_string()));
    }
    cells.push(
        typo::text(typo::META.tabular(), time_text(item, data.view, data.now))
            .style(theme::text_3)
            .wrapping(Wrapping::None)
            .into(),
    );
    let selected = data.selected == Some(item.id.as_str());
    let row = widgets::list_row(cells, selected, Some(Message::Select(item.id.clone())));
    if selected { widgets::anchored(&data.anchor, row) } else { row }
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

/// The time at the end of a row: when it will come back in "Snoozed" ([`wake_text`]), how long
/// ago it was done in "Done", otherwise how long ago it last changed ([`ago`]).
pub fn time_text(item: &Item, view: &View, now: DateTime<Utc>) -> String {
    match (view, &item.state) {
        (_, State::Snoozed { until }) => wake_text(*until, now),
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
        assert_eq!(time_text(snoozed, &View::Snoozed, now()), wake_text(until, now()));
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
