//! The overview (task E3.9 in `docs/tasks/eingang.md`): the inbox at a glance in two columns, the
//! window's first view.
//!
//! - **Columns** ([`Column`]): "Mail" holds the entries of mail accounts, "Notifications" every
//!   other entry. Each starts with "For me", grouped ([`Group`]): "Waiting for you" (only
//!   notifications – urgent and open), "Today", "Yesterday", "Earlier" by the local day the entry
//!   last changed. At the end "Info · n" lies folded; a click unfolds it ([`Message::ToggleInfo`]),
//!   and the window remembers it in `ui.json` (task E3.11, [`Column::info_key`]).
//! - **Rows** are those of every view (`list::entry_row`): mail on three lines, the rest on two, each
//!   with its action bar. Waiting notifications are **cards** instead ([`card`]): `raised`, framed,
//!   with the panel's buttons – "Allow" and "Deny" for an approval (task E3.8), "Go to window" for a
//!   coding agent's session (task E2.4) – and the session's name as title (task E2.6).
//! - **Sizes**: the wide layout shows both columns, each at most [`size::content_max()`], a line
//!   between them, every column under its head ([`size::header()`]: icon, name, the number of "For
//!   me"). An open panel takes the place of the column its entry is not in. Medium and narrow show
//!   one column, chosen with "Mail | Notifications" in the header bar. Mini and strip show the
//!   glance (`glance.rs`), as in every view.
//! - **Keys** (`keys.rs`): `j`/`k` walk the column ([`Sections::order`]), `h`/`l` and ←/→ change the
//!   column; the app keeps which column has the cursor.
//!
//! - **Cards** (task E8.3, `list::Look::Cards`, the comfortable density of the mail window): the
//!   groups are quiet heads over a column of mail cards (`list::entry_card`) – the same icon, name
//!   and number as the rows' heads ("Today 3", task E8.11), only without their band and with the
//!   cards' spacing ([`group_caption`]) –, "Info" folded at the end as before.
//! - **Lead**: in narrow mail windows the column starts with how many mails are new for the user
//!   and from whom ([`ColumnData::lead`], [`lead`]), in both densities (task E8.11).
//!
//! A column with nothing for the user says so quietly ([`calm`]); a column with nothing at all shows
//! the empty state of its own.
//!
//! Only the window: the overview sends the requests of the other views (`inbox.list_items` for all
//! open entries, then `set_item_done`, `snooze_item`, … through the app). No request of its own.

use chrono::{DateTime, Days, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Theme, border};
use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell;
use torro_gui::theme::{self, palette, radius, size, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{datetime, style, typo};
use torro_inbox::{Item, Lane, Link, Order, SourceKind};

use crate::app::Message;
use crate::detail;
use crate::list::{self, Look, Rows};

/// A column of the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    /// The entries of the mail accounts.
    #[default]
    Mail,
    /// Every other entry: agents, approvals, scripts, the suite.
    Notices,
}

impl Column {
    /// Left to right.
    pub const ALL: [Column; 2] = [Column::Mail, Column::Notices];

    /// The column `item` stands in.
    pub fn of(item: &Item) -> Column {
        if item.source.kind == SourceKind::Mail { Column::Mail } else { Column::Notices }
    }

    /// The name in the column's head and in "Mail | Notifications".
    pub fn name(self) -> &'static str {
        match self {
            Column::Mail => tr("Mail"),
            Column::Notices => tr("App Feed"),
        }
    }

    /// The icon in the column's head and in its empty state.
    pub fn icon(self) -> Icon {
        match self {
            Column::Mail => Icon::Mail,
            Column::Notices => Icon::Bell,
        }
    }

    /// Its place from the left, for state kept per column.
    pub fn index(self) -> usize {
        match self {
            Column::Mail => 0,
            Column::Notices => 1,
        }
    }

    /// The name under which `ui.json` remembers that the column's "Info" is unfolded (task E3.11,
    /// `torro_gui::ui_state::unfolded_from`). Never changed: a stored name must keep meaning the
    /// same column.
    pub fn info_key(self) -> &'static str {
        match self {
            Column::Mail => "overview.mail.info",
            Column::Notices => "overview.notices.info",
        }
    }

    /// The id of its scrollable, to scroll the cursor into view.
    pub fn scroll_id(self) -> &'static str {
        match self {
            Column::Mail => "inbox-overview-mail",
            Column::Notices => "inbox-overview-notices",
        }
    }

    /// What its empty state says: what shows up here.
    fn empty_text(self) -> (&'static str, &'static str) {
        match self {
            Column::Mail => (tr("No mail for you."), tr("New mail that needs you shows up here.")),
            Column::Notices => (tr("Nothing for you right now."), tr("Agents, approvals and notices show up here.")),
        }
    }
}

/// A group of a column's "For me", top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Urgent and open (`list::is_waiting`): waiting agents, approvals. Only notifications.
    Waiting,
    /// Last changed today (local time).
    Today,
    /// Last changed yesterday.
    Yesterday,
    /// Before that.
    Earlier,
}

impl Group {
    /// The head's name.
    pub fn label(self) -> &'static str {
        match self {
            Group::Waiting => tr("Waiting for you"),
            Group::Today => tr("Today"),
            Group::Yesterday => tr("Yesterday"),
            Group::Earlier => tr("Earlier"),
        }
    }

    /// The head's icon, the same over rows and over cards (task E8.11).
    pub fn icon(self) -> Icon {
        match self {
            Group::Waiting => Icon::CircleAlert,
            Group::Today => Icon::Sun,
            Group::Yesterday => Icon::Clock,
            Group::Earlier => Icon::CalendarDays,
        }
    }

    /// The head's mark; "Waiting for you" carries the attention colour, as in the list of a view.
    fn mark(self, attention: Color) -> Mark {
        match self {
            Group::Waiting => Mark::Tinted(self.icon(), attention),
            Group::Today | Group::Yesterday | Group::Earlier => Mark::Icon(self.icon()),
        }
    }
}

/// The day group of an entry that last changed at `updated`, seen from `now` (local days).
pub fn day_group(updated: DateTime<Utc>, now: DateTime<Utc>) -> Group {
    let (day, today) = (datetime::to_local(updated).date(), datetime::to_local(now).date());
    if day >= today {
        Group::Today
    } else if today.checked_sub_days(Days::new(1)) == Some(day) {
        Group::Yesterday
    } else {
        Group::Earlier
    }
}

/// The entries of one column as it shows them: the groups of "For me" that have entries, in order,
/// and the entries of "Info" (folded or not).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sections<'a> {
    pub groups: Vec<(Group, Vec<&'a Item>)>,
    pub info: Vec<&'a Item>,
}

impl<'a> Sections<'a> {
    /// The open entries of `items` that stand in `column` ([`Column::of`]), sorted into the groups
    /// at `now`; every group keeps the order of `items` (the app's: waiting first, then the newest).
    /// Done and snoozed entries are left out.
    pub fn of(items: &'a [Item], column: Column, now: DateTime<Utc>) -> Sections<'a> {
        let mut sections = Sections::default();
        let mut by_group: [Vec<&'a Item>; 4] = Default::default();
        for item in items.iter().filter(|item| Column::of(item) == column && item.state.is_open()) {
            if item.lane == Lane::Info {
                sections.info.push(item);
                continue;
            }
            let group = if list::is_waiting(item) && column == Column::Notices {
                Group::Waiting
            } else {
                day_group(item.updated, now)
            };
            let slot = match group {
                Group::Waiting => 0,
                Group::Today => 1,
                Group::Yesterday => 2,
                Group::Earlier => 3,
            };
            by_group[slot].push(item);
        }
        let order = [Group::Waiting, Group::Today, Group::Yesterday, Group::Earlier];
        sections.groups = order.into_iter().zip(by_group).filter(|(_, items)| !items.is_empty()).collect();
        sections
    }

    /// The sections with the day groups in `sort` (task E8.15): oldest first "Earlier",
    /// "Yesterday", "Today" – the entries in them come in that order from the source already –,
    /// "Waiting for you" on top either way.
    pub fn ordered(mut self, sort: Order) -> Sections<'a> {
        if sort == Order::OldestFirst {
            let days = usize::from(self.groups.first().is_some_and(|(group, _)| *group == Group::Waiting));
            self.groups[days..].reverse();
        }
        self
    }

    /// How many entries of "For me" the column has: the number in its head.
    pub fn for_me(&self) -> usize {
        self.groups.iter().map(|(_, items)| items.len()).sum()
    }

    /// The entries in the order the keys walk them: the groups of "For me", then – while it is
    /// unfolded – "Info".
    pub fn order(&self, info_open: bool) -> Vec<&'a Item> {
        let info = if info_open { &self.info[..] } else { &[] };
        self.groups.iter().flat_map(|(_, items)| items.iter().copied()).chain(info.iter().copied()).collect()
    }

    /// Nothing at all in the column, not even in "Info".
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.info.is_empty()
    }
}

/// The height of a waiting entry's card ([`card`]): the frame, the inset, the two lines of text
/// and – when it has buttons ([`has_buttons`]) – the gap and the row of buttons.
pub fn card_height(item: &Item) -> f32 {
    let text = 2.0 * typo::line_height(typo::body());
    let buttons = if has_buttons(item) { space::m() + size::control_sm() } else { 0.0 };
    2.0 * (space::m() + size::HAIRLINE) + text + buttons
}

/// The height of the calm line of a column whose "For me" is empty while "Info" has entries.
fn calm_height() -> f32 {
    size::row_message()
}

/// The height of the caption over a group of cards ([`caption`]): its line with [`space::m()`]
/// above and [`space::s()`] below.
pub(crate) fn caption_height() -> f32 {
    space::m() + typo::line_height(typo::caption()) + space::s()
}

/// The height of the lead of a narrow column ([`lead`]): the big number or its two lines beside it,
/// whichever is higher, under [`space::xs()`].
fn lead_height() -> f32 {
    let words = typo::line_height(typo::body_strong()) + typo::line_height(typo::meta());
    space::xs() + typo::line_height(typo::display()).max(words)
}

/// The padding around the lead over rows (task E8.11): its figure where the rows' avatars start,
/// [`space::s()`] above and [`space::m()`] below, before the first group's head.
fn lead_rows_padding() -> Padding {
    let left = layout::row_padding().left - space::xs();
    Padding::new(0.0).left(left).top(space::s()).bottom(space::m())
}

/// The height of the lead over rows, with its padding ([`lead_rows_padding`]).
fn lead_rows_height() -> f32 {
    let pad = lead_rows_padding();
    pad.top + lead_height() + pad.bottom
}

/// Where the row (or card) of `id` lies in the scrolling content of a column with `sections`, as
/// `(top, bottom)`; the first of a group starts at its head, so scrolling up to it shows the head.
/// Rows of "Info" only while it is unfolded (`info_open`). `None` for an entry not shown. `lead`
/// says whether the column starts with its [`lead`] (narrow mail windows, both densities).
///
/// With cards (`look`, task E8.3) the heads are captions and the entries cards `width` wide (task
/// E8.11: as high as their previews' lines at that width, [`cards_span`]); rows ignore `width`.
pub fn row_span(
    sections: &Sections<'_>,
    info_open: bool,
    id: &str,
    look: Look,
    lead: bool,
    width: f32,
) -> Option<(f32, f32)> {
    if look.cards() {
        return cards_span(sections, info_open, id, lead, width);
    }
    let head = list::group_head() - list::head_trim();
    let mut y = if lead { lead_rows_height() } else { 0.0 };
    for (group, items) in &sections.groups {
        let start = y;
        y += head;
        if *group == Group::Waiting {
            y += space::s();
            for (i, item) in items.iter().enumerate() {
                let height = card_height(item);
                if item.id == id {
                    return Some((if i == 0 { start } else { y - space::s() }, y + height + space::s()));
                }
                y += height + space::s();
            }
            y += size::HAIRLINE;
            continue;
        }
        for (i, item) in items.iter().enumerate() {
            let height = list::row_height_of(item);
            if item.id == id {
                return Some((if i == 0 { start } else { y }, y + height));
            }
            y += height;
        }
    }
    if sections.groups.is_empty() {
        y += calm_height();
    }
    y += size::row();
    for item in sections.info.iter().filter(|_| info_open) {
        let height = list::row_height_of(item);
        if item.id == id {
            return Some((y, y + height));
        }
        y += height;
    }
    None
}

/// [`row_span`] for a column of cards `width` wide: the lead, then per group its caption and its
/// cards with [`size::card_gap()`] between them, the calm line when "For me" is empty, the "Info"
/// toggle [`space::m()`] below, and – unfolded – the cards of "Info" [`space::s()`] below it.
fn cards_span(sections: &Sections<'_>, info_open: bool, id: &str, lead: bool, width: f32) -> Option<(f32, f32)> {
    let look = Look::Cards;
    let mut y = list::cards_inset().top + if lead { lead_height() } else { 0.0 };
    for (_, items) in &sections.groups {
        let start = y;
        y += caption_height();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                y += size::card_gap();
            }
            let height = list::height_of(item, look, width);
            if item.id == id {
                return Some((if i == 0 { start } else { y }, y + height));
            }
            y += height;
        }
    }
    if sections.groups.is_empty() {
        y += calm_height();
    }
    y += space::m() + size::row() + space::s();
    for (i, item) in sections.info.iter().filter(|_| info_open).enumerate() {
        if i > 0 {
            y += size::card_gap();
        }
        let height = list::height_of(item, look, width);
        if item.id == id {
            return Some((y, y + height));
        }
        y += height;
    }
    None
}

/// Whether the card of `item` has buttons: an approval its "Allow" and "Deny", a coding agent's
/// session "Go to window" when its window can be opened ([`detail::openable`]).
pub fn has_buttons(item: &Item) -> bool {
    detail::is_approval(item) || detail::openable(item) == Some(Link::Window)
}

/// The columns a window of `class` shows, left to right, of the `columns` its kind has (both in the
/// hub, one in the mail and the notifications window, task E8.1): all of them in the wide layout –
/// only the column of the entry in the panel while one is open (`detail`), the panel takes the
/// other's place –, else the one with the cursor (`column`).
pub fn shown(class: SizeClass, detail: Option<&Item>, column: Column, columns: &[Column]) -> Vec<Column> {
    match (class, detail) {
        (SizeClass::Wide, Some(item)) if columns.contains(&Column::of(item)) => vec![Column::of(item)],
        (SizeClass::Wide, _) => columns.to_vec(),
        _ => vec![column],
    }
}

/// What one column gets to draw.
pub struct ColumnData<'a> {
    pub column: Column,
    pub sections: Sections<'a>,
    /// "Info" is unfolded.
    pub info_open: bool,
    pub rows: Rows<'a>,
    /// A decision of an approval is on its way: its card's "Allow" and "Deny" are disabled.
    pub deciding: bool,
    /// The attention colour of the theme, for the "Waiting for you" head.
    pub attention: Color,
    /// The column's head (icon, name, number): in the wide layout, where both columns stand side by
    /// side; medium and narrow name the column in the header bar instead.
    pub head: bool,
    /// With cards in a narrow window (task E8.3): how many mails are new for the user and the line
    /// of their newest senders ("From Anna, Ben, Chris", `glance::senders_text`), over the cards.
    pub lead: Option<(usize, String)>,
}

/// One column: its head (when [`ColumnData::head`]), then its groups, cards and rows, the calm line
/// or the empty state, and "Info" at the end – scrolling. It takes the width it is given; the app
/// caps the columns together ([`width`]): in a row, Iced cannot cap a filling child on its own.
pub fn column_view<'a>(data: ColumnData<'a>) -> Element<'a, Message> {
    let column = data.column;
    let mut content = column![].width(Fill);
    if data.head {
        content = content.push(head(column, data.sections.for_me()));
    }
    let body: Element<'a, Message> = if data.sections.is_empty() {
        let (title, hint) = column.empty_text();
        list::empty_state(column.icon(), title, hint)
    } else {
        // The rows need their width to cut their texts (`list::entry_row`).
        iced::widget::responsive(move |bounds| {
            let rows = content_rows(&data, bounds.width);
            scrollable(rows)
                .id(column.scroll_id())
                .on_scroll(move |viewport| {
                    Message::ColumnScrolled(column, viewport.absolute_offset().y, viewport.bounds().height)
                })
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(style::scrollable)
                .width(Fill)
                .height(Fill)
                .into()
        })
        .into()
    };
    container(content.push(body).height(Fill)).width(Fill).into()
}

/// The widest the `count` columns shown side by side get together: [`size::content_max()`] each and
/// the line between them; more room stays free at the right (a wide window is not stretched).
pub fn width(count: usize) -> f32 {
    count as f32 * size::content_max() + count.saturating_sub(1) as f32 * size::HAIRLINE
}

/// The head of a column: its icon, name and number of "For me", [`size::header()`] high, a line
/// below – the look of the header bar's title.
fn head<'a>(column: Column, for_me: usize) -> Element<'a, Message> {
    let note = (for_me > 0).then(|| for_me.to_string());
    let title = shell::header_title(Some(column.icon()), column.name(), note);
    let pad = layout::row_padding();
    column![
        container(title)
            .center_y(size::header() - size::HAIRLINE)
            .padding(Padding::new(0.0).left(pad.left).right(pad.right)),
        rule::horizontal(size::HAIRLINE).style(style::rule),
    ]
    .into()
}

/// The scrolling content of a column `width` wide: rows, or cards ([`content_cards`]).
fn content_rows<'a>(data: &ColumnData<'a>, width: f32) -> Element<'a, Message> {
    if data.rows.look.cards() {
        return content_cards(data, width);
    }
    let mut rows = column![].width(Fill);
    // The narrow mail window leads with its number in both densities (task E8.11).
    if let Some((count, senders)) = &data.lead {
        rows = rows.push(container(lead(*count, senders.clone())).padding(lead_rows_padding()));
    }
    for (group, items) in &data.sections.groups {
        let head = widgets::group_head(group.mark(data.attention), group.label(), Some(items.len()), None);
        rows = rows.push(list::without_top(head));
        if *group == Group::Waiting {
            let cards = items.iter().map(|item| card(item, &data.rows, data.deciding, width));
            let block =
                column(cards).spacing(space::s()).padding(Padding::new(space::s()).left(space::l()).right(space::l()));
            rows = rows.push(block).push(rule::horizontal(size::HAIRLINE).style(style::rule));
        } else {
            rows = rows.extend(items.iter().map(|item| list::entry_row(item, &data.rows, width)));
        }
    }
    if data.sections.groups.is_empty() {
        rows = rows.push(calm(data.column));
    }
    rows = rows.push(info_toggle(data.column, data.sections.info.len(), data.info_open));
    if data.info_open {
        rows = rows.extend(data.sections.info.iter().map(|item| list::entry_row(item, &data.rows, width)));
    }
    rows.into()
}

/// The scrolling content of a column of cards `width` wide (task E8.3): the [`lead`], every group
/// under its [`group_caption`] as a column of cards, the calm line, "Info" with its cards – laid out as
/// [`cards_span`] counts it.
fn content_cards<'a>(data: &ColumnData<'a>, width: f32) -> Element<'a, Message> {
    let inner = width - list::cards_side();
    let cards = |items: &[&'a Item]| {
        column(items.iter().map(|item| list::entry_card(item, &data.rows, inner))).spacing(size::card_gap())
    };
    let mut content = column![].width(Fill);
    if let Some((count, senders)) = &data.lead {
        content = content.push(lead(*count, senders.clone()));
    }
    for (group, items) in &data.sections.groups {
        content = content.push(group_caption(*group, items.len(), data.attention)).push(cards(items));
    }
    if data.sections.groups.is_empty() {
        content = content.push(calm(data.column));
    }
    let toggle = info_toggle(data.column, data.sections.info.len(), data.info_open);
    content = content.push(container(toggle).padding(Padding::new(0.0).top(space::m()).bottom(space::s())));
    if data.info_open {
        content = content.push(cards(&data.sections.info));
    }
    container(content).padding(list::cards_inset()).into()
}

/// The head over a group of cards (task E8.11): what the rows' head (`widgets::group_head`) shows –
/// the group's icon (14, in `text_2`, "Waiting for you" in the attention colour), its name in
/// `CAPTION` and `text_2`, the number tabular in `META` and `text_3`, "Today 3" – without the band
/// and its lines, with the cards' spacing ([`caption_height`]: [`space::m()`] above, [`space::s()`]
/// below).
pub(crate) fn group_caption<'a>(group: Group, count: usize, attention: Color) -> Element<'a, Message> {
    let tint = (group == Group::Waiting).then_some(attention);
    icon_caption(group.icon(), tint, group.label(), count)
}

/// The head over a group of cards with `icon` (in `tint`, else `text_2`), `label` and `count` – the
/// one building block of [`group_caption`] and the notifications window's "For me 3" over cards
/// (task E8.12, finding 11: the same head as over rows in the other density). It starts at the
/// cards' edge, the one left axis of chips, heads and cards (finding 10).
pub(crate) fn icon_caption<'a>(icon: Icon, tint: Option<Color>, label: &str, count: usize) -> Element<'a, Message> {
    let mark = torro_gui::icon::icon(icon, torro_gui::icon::SIZE_SM)
        .style(move |t: &Theme, _| iced::widget::svg::Style { color: Some(tint.unwrap_or(palette(t).text_2)) });
    let label = typo::text(typo::caption(), label.to_owned()).style(theme::text_2).wrapping(Wrapping::None);
    let number = typo::text(typo::meta().tabular(), count.to_string()).style(theme::text_3).wrapping(Wrapping::None);
    let line = row![mark, label, number].spacing(space::s()).align_y(Alignment::Center);
    container(line).padding(Padding::new(0.0).top(space::m()).bottom(space::s())).into()
}

/// The lead of a narrow mail column (task E8.3, "N new for you"; task E8.11 in both densities): the
/// number in `DISPLAY`, beside it "new for you" and the newest senders in `META`.
fn lead<'a>(count: usize, senders: String) -> Element<'a, Message> {
    let figure = typo::text(typo::display().tabular(), count.to_string()).style(theme::text).wrapping(Wrapping::None);
    let words = column![
        typo::text(typo::body_strong(), tr("new for you")).style(theme::text).wrapping(Wrapping::None),
        container(typo::text(typo::meta(), senders).style(theme::text_3).wrapping(Wrapping::None)).clip(true),
    ];
    let line = row![figure, words].spacing(space::m()).align_y(Alignment::Center);
    // At the cards' edge, the column's one left axis (task E8.12).
    container(line).padding(Padding::new(0.0).top(space::xs())).into()
}

/// The quiet line of a column whose "For me" is empty while "Info" has entries: what would show up
/// here, centred, as high as a mail row.
pub fn calm<'a>(column: Column) -> Element<'a, Message> {
    let (title, hint) = column.empty_text();
    let words = column![
        typo::text(typo::body_strong(), title).align_x(Alignment::Center),
        typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center),
    ]
    .spacing(space::xs())
    .align_x(Alignment::Center);
    container(words).width(Fill).center(calm_height()).into()
}

/// The folded "Info" at the end of a column: chevron, "Info" and its number, as high as a row; a
/// click folds or unfolds it.
fn info_toggle<'a>(column: Column, count: usize, open: bool) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let mark = torro_gui::icon::icon(chevron, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) });
    let label = typo::text(typo::body(), tr("Info")).style(theme::text_2).wrapping(Wrapping::None);
    let number = typo::text(typo::meta().tabular(), count.to_string()).style(theme::text_3);
    let cells = vec![
        container(mark).center_x(list::mark_width()).into(),
        row![label, number].spacing(space::s()).align_y(Alignment::Center).width(Fill).into(),
    ];
    widgets::list_row(cells, false, Some(Message::ToggleInfo(column)))
}

/// The card of a waiting notification `width` wide (the column's): the source's tile, the title (the
/// session's name for a named session) with the time, the text below, and the panel's buttons for
/// it ([`has_buttons`]); highlighted under the cursor, with its action bar like a row. A click opens
/// it in the panel.
fn card<'a>(item: &'a Item, rows: &Rows<'a>, deciding: bool, width: f32) -> Element<'a, Message> {
    let selected = rows.selected == Some(item.id.as_str());
    // Inside the card the text has the room of a row minus the card's inset and frame.
    let inner = width - 2.0 * space::l() - 2.0 * (space::m() + size::HAIRLINE);
    let room = (inner - list::tile() - space::m()).max(0.0);
    let (title, text) = list::words(item);
    let time = list::time_text(item, rows.view, rows.now);
    let title_room = room - typo::text_width(&time, typo::meta()) - list::cell_gap();
    let title = typo::text(typo::body_strong(), typo::ellipsize(&title, typo::body_strong(), title_room))
        .style(theme::text)
        .wrapping(Wrapping::None);
    let time = typo::text(typo::meta().tabular(), time).style(theme::text_3).wrapping(Wrapping::None);
    let first = row![container(title).width(Fill).clip(true), time].spacing(space::s()).align_y(Alignment::Center);
    let text = text.map(|text| typo::ellipsize(&text, typo::body(), room)).unwrap_or_default();
    let second = container(typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::None)).clip(true);
    let words = column![first, second].width(Fill);
    let mut card = column![row![list::source_tile(item), words].spacing(space::m()).align_y(Alignment::Center)]
        .spacing(space::m());
    if has_buttons(item) {
        let mut buttons =
            detail::decide(item, deciding).into_iter().filter(|_| detail::is_approval(item)).collect::<Vec<_>>();
        if detail::openable(item) == Some(Link::Window) {
            buttons.push(widgets::text_button(
                Some(Icon::AppWindow),
                tr("Go to window"),
                style::button::secondary,
                Some(Message::OpenRow(item.id.clone())),
            ));
        }
        let indent = list::tile() + space::m();
        card = card.push(row(buttons).spacing(space::s()).padding(Padding::new(0.0).left(indent)));
    }
    let card = button(card)
        .width(Fill)
        .padding(space::m())
        .style(card_style(selected))
        .on_press(Message::Select(item.id.clone()));
    let actions = rows.anchors.mark(selected, list::actions(item, rows.hidable.contains(&item.id.as_str()), rows.ai));
    let words = widgets::row_actions_words(&actions, inner);
    // Beside the card's own buttons, not over its text.
    let card = if has_buttons(item) {
        widgets::card_actions(card, actions, selected, words, space::m() + size::HAIRLINE)
    } else {
        widgets::row_actions(card, actions, selected, words)
    };
    if selected { widgets::anchored(&rows.anchors.row, card) } else { card }
}

/// The card's look: `raised` with a `line_strong` frame, radius [`radius::md()`]; hovered and
/// pressed the frame turns `line_selected` (as on a framed button, where `hover` on `raised` would
/// hardly show); under the cursor `selected` with that frame.
fn card_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Copy {
    move |t, status| {
        let p = palette(t);
        let lit = selected || matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if selected { p.selected } else { p.raised })),
            text_color: p.text,
            border: border::rounded(radius::md())
                .color(if lit { p.line_selected } else { p.line_strong })
                .width(size::HAIRLINE),
            ..button::Style::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeDelta, TimeZone};
    use torro_inbox::demo::DemoSource;
    use torro_inbox::{InboxSource, ItemFilter, StateFilter};

    use super::*;

    fn now() -> DateTime<Utc> {
        Local.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time").to_utc()
    }

    /// Every open entry of the demo, waiting first – what the app holds in the overview.
    fn open() -> Vec<Item> {
        let demo = DemoSource::new(now());
        let filter = ItemFilter { state: Some(StateFilter::Open), ..ItemFilter::default() };
        let mut items = iced::futures::executor::block_on(demo.list_items(filter)).expect("the demo answers");
        items.sort_by_key(|item| !list::is_waiting(item));
        items
    }

    /// The height of the block of cards around `cards`: the gap above, below and between the
    /// cards, and the line under the block.
    fn cards_height(cards: &[&Item]) -> f32 {
        let gaps = space::s() * (cards.len() + 1) as f32;
        cards.iter().map(|item| card_height(item)).sum::<f32>() + gaps + size::HAIRLINE
    }

    fn ids<'a>(items: &[&'a Item]) -> Vec<&'a str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn mail_and_everything_else_go_to_their_columns() {
        let items = open();
        for column in Column::ALL {
            let sections = Sections::of(&items, column, now());
            let shown = sections.order(true);
            assert!(shown.iter().all(|item| Column::of(item) == column), "{column:?}");
            assert_eq!(shown.len(), items.iter().filter(|item| Column::of(item) == column).count(), "nothing lost");
        }
        let mail = Sections::of(&items, Column::Mail, now());
        assert!(mail.order(true).iter().all(|item| item.source.kind == SourceKind::Mail));
    }

    #[test]
    fn for_me_is_grouped_waiting_then_by_day() {
        let items = open();
        let notices = Sections::of(&items, Column::Notices, now());
        let groups: Vec<Group> = notices.groups.iter().map(|(group, _)| *group).collect();
        assert_eq!(groups, [Group::Waiting, Group::Today, Group::Yesterday, Group::Earlier], "the demo has every day");
        assert_eq!(
            ids(&notices.groups[0].1),
            ["demo-18", "demo-1", "demo-3", "demo-2"],
            "the alert and the waiting ones first, as the app sorts"
        );
        assert_eq!((ids(&notices.groups[2].1), ids(&notices.groups[3].1)), (vec!["demo-12"], vec!["demo-13"]));
        let mail = Sections::of(&items, Column::Mail, now());
        let groups: Vec<Group> = mail.groups.iter().map(|(group, _)| *group).collect();
        assert_eq!(groups, [Group::Today, Group::Yesterday, Group::Earlier], "no waiting group for mail");
        assert!(mail.groups.iter().flat_map(|(_, items)| items).all(|item| item.lane == Lane::ForMe));
        assert_eq!(mail.for_me() + notices.for_me(), items.iter().filter(|item| item.lane == Lane::ForMe).count());

        // Task E8.15: oldest first the days turn around, "Waiting for you" stays on top.
        let turned = |sections: Sections<'_>| {
            sections.ordered(Order::OldestFirst).groups.iter().map(|(group, _)| *group).collect::<Vec<_>>()
        };
        let notices = turned(Sections::of(&items, Column::Notices, now()));
        assert_eq!(notices, [Group::Waiting, Group::Earlier, Group::Yesterday, Group::Today]);
        assert_eq!(turned(Sections::of(&items, Column::Mail, now())), [Group::Earlier, Group::Yesterday, Group::Today]);
    }

    #[test]
    fn days_count_in_local_time() {
        assert_eq!(day_group(now() - TimeDelta::hours(10), now()), Group::Today, "00:30 today");
        assert_eq!(day_group(now() - TimeDelta::hours(11), now()), Group::Yesterday, "23:30 yesterday");
        assert_eq!(day_group(now() - TimeDelta::hours(34), now()), Group::Yesterday, "00:30 yesterday");
        assert_eq!(day_group(now() - TimeDelta::hours(35), now()), Group::Earlier, "23:30 the day before");
        assert_eq!(day_group(now() + TimeDelta::hours(1), now()), Group::Today, "a clock ahead");
    }

    #[test]
    fn info_lies_folded_at_the_end() {
        let items = open();
        let notices = Sections::of(&items, Column::Notices, now());
        assert_eq!(ids(&notices.info), ["demo-8", "demo-9", "demo-7"]);
        let folded = notices.order(false);
        assert!(folded.iter().all(|item| item.lane == Lane::ForMe), "folded: the keys skip Info");
        let open = notices.order(true);
        assert_eq!(open.len(), folded.len() + 3);
        assert_eq!(ids(&open[folded.len()..]), ["demo-8", "demo-9", "demo-7"], "unfolded: at the end");
        assert_eq!(row_span(&notices, false, "demo-8", Look::Rows, false, 480.0), None, "a folded row is not shown");
        assert!(row_span(&notices, true, "demo-8", Look::Rows, false, 480.0).is_some());
    }

    #[test]
    fn spans_count_heads_cards_rows_and_the_info_toggle() {
        let items = open();
        let notices = Sections::of(&items, Column::Notices, now());
        let head = list::group_head() - list::head_trim();
        let card = card_height(notices.groups[0].1[0]);
        assert_eq!(
            row_span(&notices, false, "demo-18", Look::Rows, false, 480.0),
            Some((0.0, head + space::s() + card + space::s()))
        );
        let block = cards_height(&notices.groups[0].1);
        let today = &notices.groups[1].1;
        let first = row_span(&notices, false, &today[0].id, Look::Rows, false, 480.0).expect("the first row of Today");
        assert_eq!(first, (block + head, block + 2.0 * head + size::row_notice()), "its head comes along");
        let rows: f32 = today.iter().map(|item| list::row_height_of(item)).sum();
        // Yesterday and Earlier, each with its head, lie between Today and Info.
        let later: f32 = notices.groups[2..]
            .iter()
            .map(|(_, items)| head + items.iter().map(|item| list::row_height_of(item)).sum::<f32>())
            .sum();
        let info = block + 2.0 * head + rows + later + size::row();
        assert_eq!(
            row_span(&notices, true, "demo-8", Look::Rows, false, 480.0),
            Some((info, info + size::row_notice()))
        );
        // An approval's card has buttons, so it is taller than a card without.
        let approval = notices.groups[0].1.iter().find(|item| detail::is_approval(item)).expect("an approval");
        let mut plain = (*approval).clone();
        plain.kind = torro_inbox::Kind::Notice;
        assert!(card_height(approval) > card_height(&plain));
    }

    #[test]
    fn a_column_with_only_info_says_so_calmly() {
        let items: Vec<Item> = open().into_iter().filter(|item| item.lane == Lane::Info).collect();
        let mail = Sections::of(&items, Column::Mail, now());
        assert!(mail.groups.is_empty() && !mail.info.is_empty() && !mail.is_empty());
        let first = &mail.info[0].id;
        assert_eq!(
            row_span(&mail, true, first, Look::Rows, false, 480.0),
            Some((calm_height() + size::row(), calm_height() + size::row() + size::row_message()))
        );
        assert!(Sections::of(&[], Column::Mail, now()).is_empty());
    }

    #[test]
    fn wide_shows_both_columns_and_the_panel_replaces_one() {
        let items = open();
        let mail = items.iter().find(|item| Column::of(item) == Column::Mail).expect("a mail entry");
        let all = &Column::ALL;
        assert_eq!(shown(SizeClass::Wide, None, Column::Notices, all), Column::ALL);
        assert_eq!(shown(SizeClass::Wide, Some(mail), Column::Notices, all), [Column::Mail], "the panel beside it");
        for class in [SizeClass::Medium, SizeClass::Narrow] {
            assert_eq!(shown(class, None, Column::Notices, all), [Column::Notices], "{class:?}: one column");
            assert_eq!(
                shown(class, Some(mail), Column::Mail, all),
                [Column::Mail],
                "{class:?}: the panel lies over it"
            );
        }
    }

    /// The mail and the notifications window (task E8.1) have one column each; the wide layout shows
    /// it alone, also beside a panel whose entry stands in the other one.
    #[test]
    fn a_window_of_one_kind_shows_its_own_column() {
        let items = open();
        let mail = items.iter().find(|item| Column::of(item) == Column::Mail).expect("a mail entry");
        let notices = &[Column::Notices];
        assert_eq!(shown(SizeClass::Wide, None, Column::Notices, notices), [Column::Notices]);
        assert_eq!(shown(SizeClass::Wide, Some(mail), Column::Notices, notices), [Column::Notices]);
        assert_eq!(shown(SizeClass::Wide, Some(mail), Column::Mail, &[Column::Mail]), [Column::Mail]);
    }

    #[test]
    fn the_columns_are_named_and_change() {
        assert_eq!(Column::Mail.name(), tr("Mail"));
        assert_eq!(Column::Notices.name(), tr("App Feed"));
        assert_eq!((Column::Mail.index(), Column::Notices.index()), (0, 1));
        assert_eq!(Column::default(), Column::Mail, "mail first");
        assert_ne!(Column::Mail.scroll_id(), Column::Notices.scroll_id());
        assert_eq!(
            (Column::Mail.info_key(), Column::Notices.info_key()),
            ("overview.mail.info", "overview.notices.info")
        );
    }

    /// Task E8.3: a column of cards counts the lead, the captions, the cards and their gaps, the
    /// "Info" toggle and the cards of "Info".
    #[test]
    fn spans_of_cards_count_captions_gaps_and_the_lead() {
        let items = open();
        let mail = Sections::of(&items, Column::Mail, now());
        let look = Look::Cards;
        let today = &mail.groups[0].1;
        let height = |item: &Item| list::height_of(item, look, 480.0);
        let top = list::cards_inset().top;
        assert_eq!(
            row_span(&mail, false, &today[0].id, look, false, 480.0),
            Some((top, top + caption_height() + height(today[0])))
        );
        let second = top + caption_height() + height(today[0]) + size::card_gap();
        assert_eq!(row_span(&mail, false, &today[1].id, look, false, 480.0), Some((second, second + height(today[1]))));
        let led = row_span(&mail, false, &today[1].id, look, true, 480.0).expect("with the lead");
        assert_eq!(led.0, second + lead_height(), "the lead comes first");
        assert_eq!(row_span(&mail, false, &mail.info[0].id, look, false, 480.0), None, "Info folded");
        let (info, _) = row_span(&mail, true, &mail.info[0].id, look, false, 480.0).expect("Info unfolded");
        let last = mail.groups.last().and_then(|(_, items)| items.last()).expect("a last card");
        let (_, end) = row_span(&mail, false, &last.id, look, false, 480.0).expect("the last card of For me");
        assert_eq!(info, end + space::m() + size::row() + space::s());
    }

    /// Task E8.11 (finding 7): the lead stands over rows too, and the rows' spans make room for it.
    #[test]
    fn the_lead_comes_before_rows_as_before_cards() {
        let items = open();
        let mail = Sections::of(&items, Column::Mail, now());
        let first = &mail.groups[0].1[0].id;
        let plain = row_span(&mail, false, first, Look::Rows, false, 480.0).expect("the first row");
        let led = row_span(&mail, false, first, Look::Rows, true, 480.0).expect("the first row with the lead");
        assert_eq!(led.0, plain.0 + lead_rows_height());
        assert_eq!(led.1 - led.0, plain.1 - plain.0, "the row keeps its height");
        let rows = Rows {
            view: &crate::app::View::Overview,
            selected: None,
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: vec!["anna@work.example.org"],
            hidable: Vec::new(),
            look: Look::Rows,
            folders: &[],
            ai: false,
        };
        for look in [Look::Rows, Look::Cards] {
            let data = ColumnData {
                column: Column::Mail,
                sections: mail.clone(),
                info_open: true,
                rows: Rows { look, ..rows.clone() },
                deciding: false,
                attention: Color::BLACK,
                head: false,
                lead: Some((3, "From Jonas".into())),
            };
            let _ = content_rows(&data, 480.0);
        }
    }

    /// Task E8.11 (finding 6): the heads over cards show the icon of the rows' heads, and keep the
    /// height the spans count.
    #[test]
    fn heads_over_cards_and_rows_are_the_same() {
        for group in [Group::Waiting, Group::Today, Group::Yesterday, Group::Earlier] {
            let icon = match group.mark(Color::BLACK) {
                Mark::Icon(icon) | Mark::Tinted(icon, _) => Some(icon),
                _ => None,
            };
            assert_eq!(icon, Some(group.icon()), "{group:?}");
            let _ = group_caption(group, 3, Color::BLACK);
        }
        assert_eq!(caption_height(), space::m() + typo::line_height(typo::caption()) + space::s());
    }
}
