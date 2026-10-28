//! The rows of entries and the list of a view (tasks E3.1, E3.2b, E3.2c and E3.9 in
//! `docs/tasks/eingang.md`).
//!
//! **Rows** (task E3.9, [`entry_row`]) are the same in every view and in both columns of the
//! overview (`overview.rs`), so an entry looks the same wherever it shows:
//!
//! - a **mail entry** ([`size::row_message()`]): a round avatar with the sender's initials on the
//!   account's colour, tinted ([`account_index`]; neutral for an account not listed); the sender
//!   (`BODY_STRONG` while new, else `BODY`) with the domain (`META`, `text_3`) and the time at the
//!   right; the subject (`BODY_STRONG` while new, else `BODY`) with the number of messages from two on; the preview in
//!   `text_2` on one line, and at its right – with two accounts or more – the account as dot and
//!   name. New has a blue dot in the left row inset; read but open entries keep their
//!   subject in `text` and nothing fainter than `text_2`.
//! - **any other entry** ([`size::row_notice()`]): the source's icon on a small tile (in the
//!   attention colour while it waits – never Torro red, E3.2b), the title (medium weight while new)
//!   with the time, below it the text in `text_2` or – for a coding agent's session with a name
//!   (task E2.6) – what the agent does and where ([`words`]).
//!
//! Both start their text at the same x: the notice's tile stands centred in the avatar's width.
//!
//! **The design's list look** (issue #291, task L.35, `layout::list()`): the same cells in the same
//! order in every design, only drawn another way ([`RowLook`]):
//!
//! - `style = "rows"` (Standard, Compact): the rows above, the avatar `[list] avatar` wide – 0 draws
//!   none and its place goes to the text ([`lead_in`]) –, the preview on `preview_lines` lines.
//! - `style = "line"` (Console): one line per entry, [`size::row_message()`] or [`size::row_notice()`]
//!   high, in the order of the rows and in columns without a column head ([`LineColumns`]): marks or
//!   tile · sender with domain or source · subject or title with the preview or text in `text_3` · time
//!   at the right ([`mail_line`], [`notice_line`]). Mail and notices share the columns, so a mixed
//!   list (the hub) has one grid.
//! - `style = "cards"`: the mail window's cards (below); every other list keeps its rows.
//! - `separators`: a hairline below each row or none; `selection`: the chosen row with a bar of the
//!   accent colour at its left, or the whole row in the accent with its text, icons and coloured marks
//!   in `on_accent` ([`mark_colour`]).
//!
//! The row under the cursor carries its **action bar** in a strip below its lines
//! (`widgets::row_actions_strip`, [`with_strip`]): Task (`t`), Done (`e`), Later (`s`) and Hide
//! (`m`) – only what works for the entry, as in the panel ([`actions`]); a mail entry has Archive (`e`)
//! and Delete first and no Done (task E18.5, [`entry_actions`]). Nothing floats over a row;
//! the selected row grows by the strip ([`strip_height`]). A button sends
//! [`Message::Act`]: the app puts its cursor on the row and does what the key does, so the menus
//! open at the row.
//!
//! **The mail window's rows are bare** (task E14.2, [`Rows::bare`]): no buttons at all – the mail's
//! actions stand in the rail at the right edge of the reader (task E14.3), and the keys act on the
//! row. The selected row is highlighted and shows its preview like any other, as high as unchosen;
//! the menus and dialogs a key opens hang at the row ([`Rows::anchors`]`.row`). The hub and the app
//! feed keep the strip above.
//!
//! **Urgency never by colour alone** (E3.2b): while urgent entries are open, the list of a view is
//! grouped like torrotasks – a head "Waiting for you" over them and "Other" over the rest
//! ([`groups`]). Without anything waiting there are no heads.
//!
//! **No hard cut** (E3.2b): Iced 0.14 cannot end a line with "…", so a row works out the room of
//! each text from its width and cuts it with an ellipsis itself (`typo::ellipsize`); the cells still
//! clip, in case the estimate runs over.
//!
//! The row under the cursor (E3.2) is the highlighted one; it registers [`Rows::anchors`], so the
//! snooze menu and the lane question open beside it. The list reports its scroll position
//! (`Message::Scrolled`), so the app can keep the cursor in view ([`row_span`]).
//!
//! **Sessions with a name** (task E2.6): a coding agent's entry whose session has a name shows the
//! name as title and, where the text stands, what the agent does and where: "Claude Code is
//! waiting for you · TorroWorkplace · feat/inbox" ([`words`]). Without a name the row is as before.
//!
//! **Cards** (task E8.3, [`Look::Cards`]): in the comfortable density of the mail window a mail
//! is a card instead of a row ([`mail_card`], the E8.2 block `widgets::card`): the avatar
//! ([`size::avatar()`]) with sender and "domain · account · time" ([`widgets::card_head`]), the
//! subject, up to two lines of preview ([`clamp_lines`]) and chips (several messages, an attachment, a
//! flag). Cards show only in the mail window, so they carry no buttons either (task E14.2): the keys
//! act on the card under the cursor, the reader's rail holds the actions.
//! Cards have no close button: mail is not dismissed in the mail window. A card is as high as what
//! it shows at its width – a preview of one line takes one line ([`mail_card_height`],
//! [`preview_lines`]) –, so the app can scroll the cursor into view.
//!
//! **Invitations** (task E8.3c): a mail entry whose mail invites to an event ([`Item::invite`], as
//! the core fills it in) shows when it is – as a chip with the answer given on its card, as the
//! calendar's mark in its row – and, in the hub's strip, offers Accept · Maybe · Decline
//! ([`invite_actions`], `Message::AnswerInvitation` for the message it links to) in place of "Done",
//! as long as the event is not over: answering is what is done with it, and the window marks the
//! entry done then.
//!
//! Titles and bodies of other sources are foreign text (`Item::untrusted`). Iced draws text as it
//! is, so nothing in it is ever interpreted; [`one_line`] and [`plain`] also drop control and
//! direction characters, so a title can neither break the row into lines nor turn its text around.

use std::ops::Range;

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{
    button, center, column, container, mouse_area, responsive, row, rule, scrollable, space as blank, svg,
};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Theme, border};
use torro_core::design::{ListLook, ListSelection, ListStyle};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, radius, size, space};
use torro_gui::typo::{self, Level};
use torro_gui::widgets::{self, Mark, RowAction};
use torro_gui::{datetime, layout, style};
use torro_inbox::item::FacetKey;
use torro_inbox::{Item, Kind, Link, SourceKind, SourceRef, State, rules};
use torro_ipc::mail::MailHighlight;
use torro_mail::Folder;

use crate::app::{Message, View};
use crate::keys::Command;
use crate::{detail, mail, notifications};

/// What the rows of entries need besides the entries (task E3.9): the same for the list of a view
/// and for the columns of the overview.
#[derive(Debug, Clone)]
pub struct Rows<'a> {
    /// The view shown, for the times ("until Wed, 14 Oct" in "Snoozed").
    pub view: &'a View,
    /// The catalogue of mail categories (task E15.2): what a mail's category is called and where
    /// it stands now. Empty where no mail is listed.
    pub categories: &'a [torro_ipc::inbox::CategoryInfo],
    /// Id of the entry under the cursor (and in the panel, when it is open): highlighted, with its
    /// action bar shown.
    pub selected: Option<&'a str>,
    /// Message ids selected in the mail window, including mail represented by inbox entries.
    pub selected_mail: &'a [String],
    /// "Now", for the times ("16:40", "Yesterday 16:40").
    pub now: DateTime<Utc>,
    /// Registered by the selected row (`row`) and its buttons that open a menu (issue #122); a bare
    /// row ([`Rows::bare`]) registers only `row`.
    pub anchors: detail::Anchors,
    /// The addresses of the mail accounts in the order the sidebar lists them: an account's place
    /// gives its colour ([`account_index`]), and from two accounts on the mail rows name theirs.
    pub accounts: Vec<&'a str>,
    /// Ids of the entries `m` opens a menu for (`App::hidable`): their
    /// action bar offers "Hide" – the bell "Mute…" for mail (issue #123).
    pub hidable: Vec<&'a str>,
    /// Rows or cards (task E8.3): cards in the comfortable density of the mail window.
    pub look: Look,
    /// Every folder of every mail account: a mail entry offers Archive and Delete only where its
    /// account has such a folder ([`filing`], issue #118).
    pub folders: &'a [Folder],
    /// Rows without buttons (task E14.2): `true` in the mail window – the selected row shows its
    /// preview like any other and is as high as unchosen ([`strip_height_in`] is 0), its menus and
    /// dialogs hang at the row –, `false` in the hub and the app feed, which keep their strip of
    /// buttons where the text starts ([`with_strip`], [`strip_height`]). Cards ([`entry_card`]) never
    /// carry buttons: they show only in the mail window.
    pub bare: bool,
}

/// How a list draws mail (task E8.3): one row per mail, or a card per mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    /// Rows, the action bar in a strip below the row under the cursor unless they are bare ([`Rows::bare`]) (the compact density, and the hub until
    /// E8.5 brings its own cards; the notifications window draws its board, `notifications.rs`).
    #[default]
    Rows,
    /// Cards (the comfortable density of the mail window), without buttons since task E14.2: the
    /// mail's actions stand in the reader's rail.
    Cards,
}

impl Look {
    /// Cards.
    pub fn cards(self) -> bool {
        self == Look::Cards
    }
}

/// What the list of a view gets to draw.
pub struct ListData<'a> {
    /// The entries of the view, in the order to show them.
    pub items: &'a [Item],
    pub rows: Rows<'a>,
    /// The attention colour of the current theme, for the icon of the "Waiting for you" head.
    pub attention: Color,
}

/// The id of the list's scrollable, to scroll the cursor into view.
pub const SCROLL: &str = "inbox-list";

/// The gap the row estimate counts between two cells: a little more than the rows' own (8),
/// so the estimate cuts a letter early rather than running under the next cell.
pub(crate) fn cell_gap() -> f32 {
    space::m()
}

/// Width of the bar at the left of the selected row, as `widgets::selection_bar` draws it.
const SELECTED_BAR: f32 = 2.0 * size::HAIRLINE;

/// How a row of a list draws its selection and its line in the list look `look` (issue #291,
/// `[list]`, `layout::list()`): worked out apart from the widgets, so the tests can check every look
/// without switching the process's design. [`sized_row`] draws what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowLook {
    /// The bar of the accent colour at the row's left (`selection = "bar"`, the chosen row): it takes
    /// its width from the row's left padding, so the cells do not move.
    pub bar: bool,
    /// The chosen row in `accent`, its text, icons and coloured marks in `on_accent`
    /// (`selection = "fill"`, [`mark_colour`]).
    pub fill: bool,
    /// A hairline below the row, inside its height (`separators`).
    pub line: bool,
}

impl RowLook {
    /// The look of a row that is `selected` (under the cursor, in the reader, or chosen with others)
    /// or not, in `look`.
    pub(crate) fn of(look: ListLook, selected: bool) -> RowLook {
        let (bar, fill) = match (selected, look.selection) {
            (false, _) => (false, false),
            (true, ListSelection::Bar) => (true, false),
            (true, ListSelection::Fill) => (false, true),
        };
        RowLook { bar, fill, line: look.separators }
    }
}

/// `colour` for a coloured mark in a row – the unread dot, a flag, a mail's category, an account's
/// dot, a waiting notice's icon – drawn in the theme `t`: `on_accent` inside the chosen row of a design
/// with `[list] selection = "fill"` (issue #291; confirmed with the draft of task L.32: a coloured mark
/// on the accent would not read), else `colour` as it is. The cells of such a row are drawn in
/// `theme::on_accent_theme`, which stays itself – that is how a mark knows where it stands.
pub(crate) fn mark_colour(t: &Theme, colour: Color) -> Color {
    if theme::on_accent_theme(t) == *t { palette(t).on_accent } else { colour }
}

/// Width of the lead of a row in the list look `look` – a mail's avatar, a notice's tile – so the text
/// of mail and of notices starts at the same x: the avatar's edge (`[list] avatar`, 40 in the
/// standard), at least the tile's. A design without an avatar (`avatar = 0`) gives a mail row no lead:
/// its text starts at the row's edge; a notice keeps its tile, the source is no decoration.
pub(crate) fn lead_in(look: ListLook, mail: bool) -> f32 {
    match (look.avatar > 0.0, mail) {
        (true, _) => look.avatar.max(tile()),
        (false, true) => 0.0,
        (false, false) => tile(),
    }
}

/// [`lead_in`] with the gap after it, as the text of a row of `style = "rows"` starts; nothing
/// without a lead.
fn lead_room_in(look: ListLook, mail: bool) -> f32 {
    let lead = lead_in(look, mail);
    if lead > 0.0 { lead + space::m() } else { 0.0 }
}

/// The width the marks of a notice row take in the active design (its tile, [`lead_in`]), in which
/// the folded "Info" of the overview centres its chevron.
pub(crate) fn mark_width() -> f32 {
    lead_in(layout::list(), false)
}

/// Edge of a notice's tile with the source's icon ([`size::tile()`], the "notice row" of the tiles:
/// 28 in the standard, 20 in Console, so it fits a line).
pub(crate) fn tile() -> f32 {
    size::tile()
}

/// How many marks the marks column of a line holds (`style = "line"`): what the mail is, an invitation,
/// an attachment and a flag – every mark a mail can carry, so the column is as wide in every row.
const LINE_MARKS: usize = 4;

/// The share of a line's free room the sender's column takes; the subject gets the rest. Fixed for
/// the whole list, so the columns stand one under the other.
const LINE_SENDER_SHARE: f32 = 0.3;

/// The most of a line's title cell the subject or title takes while a preview follows it, so the
/// preview is cut rather than left out ([`line_title_room`]).
const LINE_TITLE_SHARE: f32 = 0.6;

/// Width of the marks column of a line ([`mail_line`], [`notice_line`]): [`LINE_MARKS`] small icons
/// with [`space::xs()`] between them; a notice's tile stands in it.
pub(crate) fn line_marks_width() -> f32 {
    (LINE_MARKS as f32 * icon::SIZE_SM + (LINE_MARKS - 1) as f32 * space::xs()).max(tile())
}

/// Width of the time column of a wide line: the widest time a row shows on most days – "Yesterday 23:59"
/// or a short date – in `META`, tabular, in the language shown. A wider one ("until Wed, 14 Oct" in
/// "Snoozed") ends in "…".
pub(crate) fn line_date_width() -> f32 {
    let level = typo::meta().tabular();
    let yesterday = trf("Yesterday {time}", &[("time", &"23:59")]);
    let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 30).map(|day| datetime::format_short_date(day, day));
    let widest = day.iter().map(|day| typo::text_width(day, level)).fold(typo::text_width(&yesterday, level), f32::max);
    widest.ceil()
}

/// The width a clock time ("23:59") takes in a line, in `META`, tabular: what a narrow line keeps for
/// its time ([`LineColumns::of`]), the time of most rows.
fn line_clock_width() -> f32 {
    typo::text_width("23:59", typo::meta().tabular()).ceil()
}

/// Whether a line `width` wide is narrow: narrower than a narrow window ([`layout::NARROW_MAX_WIDTH`]).
/// A narrow line gives each row's time its own width instead of the column of the widest
/// ([`line_date_width`]), which would leave a wide empty column beside a clock time.
fn line_narrow(width: f32) -> bool {
    width < layout::NARROW_MAX_WIDTH
}

/// The widths of the cells of a line (`style = "line"`, issue #291), in their order: the marks (a
/// mail's marks, a notice's tile), who (sender with domain, or source), the title (subject or title
/// with the preview or text) and the time at the right, [`cell_gap`] between them. Mail and notices
/// take the same columns, so the title column starts at the same x in every row of a list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LineColumns {
    pub marks: f32,
    pub who: f32,
    pub title: f32,
    pub time: f32,
}

impl LineColumns {
    /// The columns of a line `width` wide whose time takes `time` in full: what is left of the width
    /// beside the marker slot and the right padding ([`line_room`]), the marks [`line_marks_width`], who
    /// [`LINE_SENDER_SHARE`] of the free room – worked out with a fixed time column, so it is the same
    /// in every row –, the time [`line_date_width`] in a wide line and its own width in a narrow one
    /// ([`line_narrow`], at most the wide column), the title the rest.
    pub(crate) fn of(width: f32, time: f32) -> LineColumns {
        let (room, gap, marks) = (line_room(width), cell_gap(), line_marks_width());
        let narrow = line_narrow(width);
        let column = if narrow { line_clock_width() } else { line_date_width() };
        let free = (room - marks - column - 3.0 * gap).max(0.0);
        let who = (free * LINE_SENDER_SHARE).floor();
        let time = if narrow { time.ceil().min(line_date_width()) } else { column };
        let title = (room - marks - who - time - 3.0 * gap).max(0.0);
        LineColumns { marks, who, title, time }
    }

    /// Where each cell starts, from the start of the cells: marks, who, title, time.
    #[cfg(test)]
    pub(crate) fn starts(&self) -> [f32; 4] {
        let gap = cell_gap();
        let who = self.marks + gap;
        let title = who + self.who + gap;
        [0.0, who, title, title + self.title + gap]
    }
}

/// The room of a line's cells `width` wide: the width less the marker slot of a mail ([`mail_row_left`])
/// and the right padding – for a notice too, whose cells start at the same x ([`notice_line`]).
fn line_room(width: f32) -> f32 {
    (width - mail_row_left(false) - layout::row_padding().right).max(0.0)
}

/// The room of the title in the title cell of a line `room` wide, the title `title` wide in full: as
/// wide as it needs, but at most [`LINE_TITLE_SHARE`] of the room while a `preview` follows, which gets
/// the rest – both cut with "…" rather than one left out ([`LineWords::of`]).
fn line_title_room(title: f32, preview: bool, room: f32) -> f32 {
    if preview { title.ceil().min((room * LINE_TITLE_SHARE).floor()) } else { room }
}

/// What the cells of a line show, worked out from the room of its [`LineColumns`] (issue #291): who
/// (the sender's name or the source) and beside it the domain, the title and the preview (each cut with
/// "…", `None` where nothing is left of it), the time and the account (mail from two accounts on).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineWords {
    pub who: String,
    pub domain: Option<String>,
    pub title: String,
    pub preview: Option<String>,
    pub account: Option<String>,
    pub time: String,
}

impl LineWords {
    /// The words of a line in `columns`: `who` in `who_level` with `domain` after it in what is left,
    /// `title` in `level` (· `count`) and `preview` in `BODY`, the `account` before the time – as the
    /// rows show them ([`mail_rows`], [`notice_rows`]).
    fn of(
        columns: LineColumns,
        (who, who_level, domain): (&str, Level, Option<&str>),
        (title, level, count): (&str, Level, Option<&str>),
        preview: Option<&str>,
        account: Option<&str>,
        time: &str,
    ) -> LineWords {
        let who_shown = typo::ellipsize(who, who_level, columns.who);
        let left = columns.who - typo::text_width(&who_shown, who_level) - space::s();
        let domain = domain.map(|domain| typo::ellipsize(domain, typo::meta(), left)).filter(|d| !d.is_empty());
        let account_room = account.map_or(0.0, |name| {
            (typo::text_width(name, typo::meta()) + new_dot() + space::xs() + cell_gap()).min(columns.title / 3.0)
        });
        let account = account.map(|name| typo::ellipsize(name, typo::meta(), account_room - cell_gap()));
        let room = (columns.title - account_room - count.map_or(0.0, pill_width)).max(0.0);
        let preview = preview.filter(|text| !text.is_empty());
        let title_room = line_title_room(typo::text_width(title, level), preview.is_some(), room);
        let title = typo::ellipsize(title, level, title_room);
        let preview_room = room - typo::text_width(&title, level).ceil().min(title_room) - space::s();
        let preview = preview.map(|text| typo::ellipsize(text, typo::body(), preview_room)).filter(|t| !t.is_empty());
        let time = typo::ellipsize(time, typo::meta().tabular(), columns.time);
        LineWords { who: who_shown, domain, title, preview, account: account.filter(|a| !a.is_empty()), time }
    }
}

/// The cells of a line in [`LineColumns`] order: `marks` in its column, who – `who` in `who_level` and
/// `who_colour` with the domain in `META`/`text_3` –, the title in `level` and `text` with the `count`
/// and the preview in `BODY`/`text_3` (the matches of a search stressed, [`marked`]) and the account at
/// its end, and the time right-aligned in its column.
#[allow(clippy::too_many_arguments)]
fn line_cells<'a>(
    columns: LineColumns,
    words: LineWords,
    marks: Element<'a, Message>,
    (who_level, who_colour): (Level, fn(&Theme) -> iced::widget::text::Style),
    level: Level,
    count: Option<String>,
    account: Option<usize>,
    (hits, hit_colour): (&[String], Option<Color>),
) -> Element<'a, Message> {
    let LineWords { who, domain, title, preview, account: account_name, time } = words;
    let mut who = row![typo::text(who_level, who).style(who_colour).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if let Some(domain) = domain {
        who = who.push(typo::text(typo::meta(), domain).style(theme::text_3).wrapping(Wrapping::None));
    }
    let mut title =
        row![marked(title, level, theme::text, hits, hit_colour)].spacing(space::s()).align_y(Alignment::Center);
    if let Some(count) = count {
        title = title.push(widgets::pill(None, count));
    }
    if let Some(preview) = preview {
        title = title.push(marked(preview, typo::body(), theme::text_3, hits, hit_colour));
    }
    let ends: Vec<Element<'a, Message>> = account_name.map(|name| account_cell(account, name)).into_iter().collect();
    let time = container(time_cell(time)).width(columns.time).align_right(columns.time).clip(true);
    row![
        container(marks).width(columns.marks).clip(true),
        container(who).width(columns.who).clip(true),
        container(line(title, ends)).width(Fill).clip(true),
        time,
    ]
    .spacing(cell_gap())
    .align_y(Alignment::Center)
    .width(Fill)
    .into()
}

/// Edge of the unread dot in the left inset.
fn new_dot() -> f32 {
    space::s()
}

/// Height of a group head: the 4 above it plus the head itself (`widgets::group_head`).
pub(crate) fn group_head() -> f32 {
    space::xs() + size::row_group()
}

/// What every head leaves out: the gap above it and its line above. Above the first head the
/// header bar already ends with a line, above the others the last row of the group before; a
/// second line 4 below either would read as a double.
pub(crate) fn head_trim() -> f32 {
    space::xs() + size::HAIRLINE
}

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

/// The height of `item`'s row ([`entry_row`]): [`message_height`] for mail,
/// [`size::row_notice()`] for every other entry – in every list style: a design that draws lists as one
/// line (`style = "line"`) makes both one line high (Console: 24).
pub fn row_height_of(item: &Item) -> f32 {
    if item.source.kind == SourceKind::Mail { message_height() } else { size::row_notice() }
}

/// The height of a mail row, of an entry and of a folder's message alike (task E3.11):
/// [`size::row_message()`].
pub fn message_height() -> f32 {
    size::row_message()
}

/// The height of `item` as a list in `look` draws it: its card `width` wide ([`entry_card_height`])
/// when it is mail and the list shows cards, else its row ([`row_height_of`], whatever the width).
pub fn height_of(item: &Item, look: Look, width: f32) -> f32 {
    match look {
        Look::Cards if item.source.kind == SourceKind::Mail => entry_card_height(item, width),
        Look::Cards | Look::Rows => row_height_of(item),
    }
}

/// Where row `index` of `items` lies in the list's content as `(top, bottom)` while the cursor is
/// on it – the rows above as high as [`row_height_of`] says, row `index` with the strip of its
/// actions below ([`strip_height`]); the first row of a group starts at its head, so scrolling up to it
/// shows the head too. `None` past the end.
///
/// With cards (`look`, task E8.3) the list has no heads: the cards, `width` wide (task E8.11: their
/// previews take the lines they need at it), stand in a column with [`cards_inset`] around it and
/// [`size::card_gap()`] between them ([`card_span`]). Rows ignore `width`.
pub fn row_span(items: &[Item], index: usize, look: Look, width: f32) -> Option<(f32, f32)> {
    row_span_in(items, index, look, width, false)
}

/// [`row_span`] for a list whose rows are `bare` or not ([`Rows::bare`], task E14.2): the row under
/// the cursor is [`strip_height_in`] higher – a bare row (the mail window's) not at all, else by the
/// hub's strip.
pub fn row_span_in(items: &[Item], index: usize, look: Look, width: f32, bare: bool) -> Option<(f32, f32)> {
    let item = items.get(index)?;
    if look.cards() {
        let above = items[..index].iter().map(|item| height_of(item, look, width));
        return Some(card_span(above, height_of(item, look, width)));
    }
    // Every head is shorter by `head_trim()` ([`without_top`]).
    rows_span(items, index, row_height_of, group_head() - head_trim(), strip_height_in(bare))
}

/// The span of row `index` in a list of rows (not cards) whose rows are as high as `height` says, under
/// group heads `head` high, the cursor's row with a strip `strip` high below it ([`row_span_in`]):
/// apart from the active design, so the tests can check the heights of every design (issue #291 –
/// Console's rows are one line of 24). `None` past the end.
fn rows_span(items: &[Item], index: usize, height: impl Fn(&Item) -> f32, head: f32, strip: f32) -> Option<(f32, f32)> {
    let item = items.get(index)?;
    let above: f32 = items[..index].iter().map(&height).sum();
    let groups = groups(items);
    let heads = groups.iter().filter(|(_, range)| range.start <= index).count() as f32;
    let top = above + heads * head;
    let first = groups.iter().any(|(_, range)| range.start == index);
    // Row `index` is the cursor's: it carries the strip with its actions unless it is bare ([`entry_row`]).
    Some((if first { top - head } else { top }, top + height(item) + strip))
}

/// The list, scrolling; the empty state of the view when there are no entries – "Follow up" of the
/// mail window (its rows are bare, [`Rows::bare`]) has its own ([`follow_up_empty`], task E19.4).
pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    if data.items.is_empty() {
        if data.rows.bare && *data.rows.view == View::Snoozed {
            let (title, hint) = follow_up_empty();
            return empty_state(Icon::CalendarClock, title, hint);
        }
        let (title, hint) = empty_text(data.rows.view);
        return empty_state(data.rows.view.icon(), title, hint);
    }
    // The rows need their width to cut their texts ([`entry_row`]).
    responsive(move |bounds| list(&data, bounds.width)).into()
}

/// The rows at `width`, under their group heads when something waits; with cards (task E8.3) a
/// card per entry in a column, no heads (mail never waits).
fn list<'a>(data: &ListData<'a>, width: f32) -> Element<'a, Message> {
    let mut rows = column![].width(Fill);
    let groups = if data.rows.look.cards() { Vec::new() } else { groups(data.items) };
    if data.rows.look.cards() {
        let inner = width - cards_side();
        let cards = data.items.iter().map(|item| entry_card(item, &data.rows, inner));
        rows = rows.push(container(column(cards).spacing(size::card_gap())).padding(cards_inset()));
    } else if groups.is_empty() {
        rows = rows.extend(data.items.iter().map(|item| entry_row(item, &data.rows, width)));
    }
    for (group, range) in groups {
        let head = widgets::group_head(group.mark(data.attention), group.label(), Some(range.len()), None);
        rows = rows.push(without_top(head));
        rows = rows.extend(data.items[range].iter().map(|item| entry_row(item, &data.rows, width)));
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

/// The group head without its gap and line above: moved up by [`head_trim()`] inside a clipping
/// container, so only the part above the head's top line is cut off (as in torrotasks).
pub(crate) fn without_top<'a>(head: Element<'a, Message>) -> Element<'a, Message> {
    container(container(head).padding(Padding { top: -head_trim(), ..Padding::ZERO })).clip(true).into()
}

/// The row of `item`, `width` wide: a mail row or a notice row ([`row_height_of`]), highlighted
/// while it is under the cursor, with its action bar ([`entry_actions`]) in a strip below it then –
/// unless the rows are bare ([`Rows::bare`], the mail window: no buttons, as high as unchosen). A
/// click opens it in the panel.
pub fn entry_row<'a>(item: &'a Item, rows: &Rows<'a>, width: f32) -> Element<'a, Message> {
    let cursor = rows.selected == Some(item.id.as_str());
    let selected = cursor || mail_selected(item, rows);
    let actions = rows.anchors.mark(true, row_actions(item, rows));
    let height = entry_row_height(item, rows);
    let content = if item.source.kind == SourceKind::Mail {
        // An invitation (task E8.3c): the calendar's mark before the time, as in a folder's row.
        let mut marks: Vec<(Element<'a, Message>, f32)> =
            item.category.map(|held| mail::category_mark(held, rows.categories)).into_iter().collect();
        marks.extend(item.invite.as_ref().map(|invite| mail::invite_mark(invite, rows.now)));
        mail_content(MailLines { marks, ..MailLines::of(item, rows) }, width, selected)
    } else {
        notice_content(item, rows, width)
    };
    let unread = (item.source.kind == SourceKind::Mail).then_some(item.state == State::New);
    let row = sized_row(
        with_strip(content, actions, width, unread.map(|_| selected)),
        selected,
        unread,
        Message::Select(item.id.clone()),
        height,
    );
    let row = if rows.bare && item.source.kind == SourceKind::Mail {
        mouse_area(row).on_right_press(Message::ContextEntry(item.id.clone())).into()
    } else {
        row
    };
    // The menus and dialogs of the keys hang at the selected row – in a bare list it has nothing else.
    if cursor { widgets::anchored(&rows.anchors.row, row) } else { row }
}

/// A bulk selection stores message ids, while these rows carry inbox entry ids.
fn mail_selected(item: &Item, rows: &Rows<'_>) -> bool {
    matches!(item.link.as_ref(), Some(Link::Mail { message }) if rows.selected_mail.contains(message))
}

/// What the strip of `item`'s row in `rows` holds ([`entry_row`]): its action bar
/// ([`entry_actions`]) while it is under the cursor, nothing for the other rows and for every bare
/// row ([`Rows::bare`], the mail window, task E14.2).
pub(crate) fn row_actions<'a>(item: &Item, rows: &Rows<'_>) -> Vec<RowAction<'a, Message>> {
    if rows.bare || rows.selected != Some(item.id.as_str()) {
        return Vec::new();
    }
    entry_actions(item, rows.hidable.contains(&item.id.as_str()), rows.folders, rows.now)
}

/// The height of `item`'s row in `rows` ([`entry_row`]): [`row_height_of`], and [`strip_height`]
/// more while its strip holds actions ([`row_actions`]) – a bare row is never higher.
pub(crate) fn entry_row_height(item: &Item, rows: &Rows<'_>) -> f32 {
    let strip = if row_actions(item, rows).is_empty() { 0.0 } else { strip_height() };
    row_height_of(item) + strip
}

/// The height the strip with the action bar adds to the selected row ([`entry_row`], a folder's
/// message row).
pub fn strip_height() -> f32 {
    widgets::row_actions_strip_height()
}

/// The height the strip adds to the selected row of a list whose rows are `bare` or not
/// ([`Rows::bare`], task E14.2): nothing for a bare row (the mail window's), else [`strip_height`].
pub fn strip_height_in(bare: bool) -> f32 {
    if bare { 0.0 } else { strip_height() }
}

/// The `content` of a row `width` wide with the strip of `actions` below it
/// (`widgets::row_actions_strip`): the action bar hangs below the lines of the selected row, in a
/// strip of its own, so it covers nothing of the row; the strip starts where the text does. The
/// row is [`strip_height`] higher then. For mail, `mail_selected` shifts the strip past the
/// reserved marker slot. Without actions the content as it is.
pub(crate) fn with_strip<'a>(
    content: Element<'a, Message>,
    actions: Vec<RowAction<'a, Message>>,
    width: f32,
    mail_selected: Option<bool>,
) -> Element<'a, Message> {
    if actions.is_empty() {
        return content;
    }
    let words = widgets::row_actions_strip_words(
        &actions,
        if mail_selected.is_some() { mail_words_room(width) } else { words_room(width) },
    );
    let look = layout::list();
    let left = mail_selected.map_or(0.0, |selected| mail_row_left(RowLook::of(look, selected).bar));
    let strip = container(widgets::row_actions_strip(actions, words))
        .padding(Padding::new(0.0).left(left + strip_indent_in(look, mail_selected.is_some())));
    column![content, strip].spacing(space::s()).into()
}

/// How far in from the start of a row's cells its strip of actions starts ([`with_strip`]): where the
/// text starts – after the lead in `style = "rows"` ([`lead_room_in`]), at the first column of a line.
fn strip_indent_in(look: ListLook, mail: bool) -> f32 {
    match look.style {
        ListStyle::Line => 0.0,
        ListStyle::Rows | ListStyle::Cards => lead_room_in(look, mail),
    }
}

/// The mail the open mail entry `item` links to (task E18.5): what `e`, its Archive and Delete act
/// on, in every window. `None` for other entries and done ones – their mail may have left the inbox
/// already, and a done entry opens again with `e` ("Reopen", decision 9 of E18).
pub fn entry_mail(item: &Item) -> Option<&str> {
    match &item.link {
        Some(Link::Mail { message })
            if item.source.kind == SourceKind::Mail && !matches!(item.state, State::Done { .. }) =>
        {
            Some(message)
        }
        _ => None,
    }
}

/// What the action bar of `item` offers, in its order (task E3.9), as the panel does:
///
/// - "Task" (`t`) and "Done" (`e`, "Reopen" for a done entry) – not for an approval, which closes
///   once it is decided and becomes no task, and no "Done" for an open mail entry ([`entry_mail`],
///   task E18.5): mail is archived, its Archive (`e`) stands first in [`entry_actions`];
/// - "Later" (`s`) for every entry that is not done – not for an approval either (task E8.1b, as
///   the hub's cards): it waits for yes or no and must not vanish by accident;
/// - "Hide" (`m`) when `m` opens a menu for it (`hidable`, from `App::hidable`: never for an
///   approval, and the suite is never muted) – for mail the bell "Mute…" ([`hide_action`]);
/// - the ✕ "Dismiss" (`x`, issue #117) last, for every entry that may simply go
///   ([`notifications::dismissable`]: no mail, no approval, nothing done) – as in the hub's "Act now"
///   and the notifications window; the toast takes it back.
pub fn actions<'a>(item: &Item, hidable: bool) -> Vec<RowAction<'a, Message>> {
    let act = |command| Message::Act(item.id.clone(), command);
    let approval = detail::is_approval(item);
    let done = matches!(item.state, State::Done { .. });
    let mut actions = Vec::new();
    if !approval {
        actions.push(RowAction::new(Icon::ListTodo, tr("Task"), "T", act(Command::MakeTask)));
        let (mark, word) = if done { (Icon::Inbox, tr("Reopen")) } else { (Icon::Check, tr("Done")) };
        if entry_mail(item).is_none() {
            actions.push(RowAction::new(mark, word, "E", act(Command::Done)));
        }
    }
    if !done && !approval {
        actions.push(RowAction::new(Icon::Clock, tr("Later"), "S", act(Command::Snooze)));
    }
    if hidable {
        actions.push(hide_action(item, tr("Hide")));
    }
    if notifications::dismissable(item) {
        actions.push(RowAction::new(Icon::X, tr("Dismiss"), "X", act(Command::Dismiss)));
    }
    actions
}

/// The action bar of `item`'s row at `now`: a mail entry's Archive · Delete first ([`filing`] in
/// `folders`, issue #118), then [`actions`]; for a mail entry with an invitation to answer (task
/// E8.3c, [`invite_actions`]) Accept · Maybe · Decline · Task · Later (· Hide) – the answer is what
/// closes the entry, so Archive and Delete give way; `e` still archives its mail (task E18.5).
pub fn entry_actions<'a>(
    item: &Item,
    hidable: bool,
    folders: &[Folder],
    now: DateTime<Utc>,
) -> Vec<RowAction<'a, Message>> {
    let mut actions = actions(item, hidable);
    let answers = invite_actions(item, now);
    let lead = if answers.is_empty() { filing(item, folders) } else { answers };
    actions.splice(0..0, lead);
    actions
}

/// Archive and Delete for the mail of the open mail entry `item` (issue #118,
/// [`mail::filing_actions`]): its mail lies in its account's inbox; Archive in the place of Done,
/// with its key `e` (task E18.5, decision 5 of E18), Delete is `#`. None for other entries and done
/// ones ([`entry_mail`]).
pub fn filing<'a>(item: &Item, folders: &[Folder]) -> Vec<RowAction<'a, Message>> {
    match entry_mail(item) {
        Some(message) => mail::filing_actions(folders, &item.source.key(), None, message, "E"),
        None => Vec::new(),
    }
}

/// The answers to the invitation of the mail entry `item` (task E8.3c): Accept · Maybe · Decline,
/// each sending `Message::AnswerInvitation` with the entry's account (its source, `mail:<address>`)
/// and the message it links to ([`Link::Mail`]) – the one whose invitation [`Item::invite`] is.
/// Empty for an entry without an invitation, one that is done or no mail entry, and once the event
/// is over at `now` (the core answers no more).
pub fn invite_actions<'a>(item: &Item, now: DateTime<Utc>) -> Vec<RowAction<'a, Message>> {
    let open = item.invite.as_ref().is_some_and(|invite| !mail::invite_over(invite, now));
    let (true, Some(Link::Mail { message }), false) =
        (open, &item.link, matches!(item.state, State::Done { .. }) || item.source.kind != SourceKind::Mail)
    else {
        return Vec::new();
    };
    mail::answers()
        .into_iter()
        .map(|(icon, word, answer)| {
            RowAction::new(icon, word, "", Message::AnswerInvitation(item.source.key(), message.clone(), answer))
        })
        .collect()
}

/// How many lines of its text a mail card shows at most (task E8.3, "zwei Zeilen Vorschau").
const PREVIEW_LINES: usize = 2;

/// How many lines the preview `text` of a card `width` wide takes: [`clamp_lines`] at the card's
/// inner width ([`card_inner`]) – one for a short text, [`PREVIEW_LINES`] at most, none without
/// words (task E8.11: a one-line preview leaves no hole of a second line). The card draws these
/// lines ([`mail_card`]), so its height ([`mail_card_height`]) is what it shows.
pub(crate) fn preview_lines(text: Option<&str>, width: f32) -> usize {
    text.map_or(0, |text| clamp_lines(text, typo::body(), card_inner(width), PREVIEW_LINES).len())
}

/// The width of the text inside a card `width` wide: less the card's padding and frame.
pub(crate) fn card_inner(width: f32) -> f32 {
    let pad = layout::panel_padding();
    (width - widgets::card_mark_inset() - pad.right - 2.0 * size::HAIRLINE).max(0.0)
}

/// The padding around a column of cards (task E8.3): [`space::m()`] on every side, as far from the
/// list's edges as the cards are apart.
pub(crate) fn cards_inset() -> Padding {
    Padding::new(space::m())
}

/// What [`cards_inset`] takes from a list's width.
pub(crate) fn cards_side() -> f32 {
    let inset = cards_inset();
    inset.left + inset.right
}

/// Where a card `height` high lies in a column of cards after cards of the heights `above`, as
/// `(top, bottom)`: the inset, then every card with the gap after it.
pub(crate) fn card_span(above: impl Iterator<Item = f32>, height: f32) -> (f32, f32) {
    let top = above.fold(cards_inset().top, |y, card| y + card + size::card_gap());
    (top, top + height)
}

/// `text` in `level` broken into at most `lines` lines no wider than `width`, word by word; when
/// text is left over, the last line ends in "…" ([`typo::ellipsize`]), and so does a word wider than
/// a line. Iced 0.14 cannot cut a paragraph after a number of lines, so a card lays out its preview
/// here. Empty for a text without words.
pub(crate) fn clamp_lines(text: &str, level: Level, width: f32, lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut words = text.split_whitespace().peekable();
    while out.len() < lines {
        let Some(first) = words.next() else { break };
        let mut line = first.to_owned();
        while let Some(next) = words.peek() {
            let longer = format!("{line} {next}");
            if typo::text_width(&longer, level) > width {
                break;
            }
            line = longer;
            words.next();
        }
        if out.len() + 1 == lines && words.peek().is_some() {
            let rest: Vec<&str> = words.by_ref().collect();
            line = format!("{line} {}", rest.join(" "));
        }
        out.push(typo::ellipsize(&line, level, width));
    }
    out
}

/// What a mail card shows (task E8.3): the lines of the mail row ([`MailLines`]; its marks are left
/// out, the card says them as chips), the chips and what a click on the card sends. No buttons: cards
/// show only in the mail window, whose actions stand in the reader's rail (task E14.2).
pub(crate) struct MailCard<'a> {
    pub lines: MailLines<'a>,
    /// [`widgets::state_pill`]s: several messages, an attachment, a flag.
    pub chips: Vec<Element<'a, Message>>,
    /// Under the cursor (and in the reader): the `selected` card.
    pub selected: bool,
    pub on_press: Message,
}

/// A mail card `width` wide (task E8.3, `widgets::card`): the head with the avatar
/// ([`size::avatar()`]), the sender and "domain · account · time"
/// ([`widgets::card_head`]); the subject (medium weight while new); the preview on up to
/// [`PREVIEW_LINES`] lines in `text_2`, as many as it takes ([`preview_lines`]); then the chips in a
/// row. No buttons (task E14.2). As high as [`mail_card_height`] says for these parts.
pub(crate) fn mail_card<'a>(card: MailCard<'a>, width: f32) -> Element<'a, Message> {
    let inner = card_inner(width);
    let MailCard { lines, chips, selected, on_press } = card;
    let MailLines { name, domain, time, subject, new, preview, account, account_name, hits, .. } = lines;
    let words = hits.map(|hits| hits.words()).unwrap_or_default();
    let hit_colour = hits.map(|hits| hits.text);
    let lead = avatar(&name, account, size::avatar());
    let account_name = account_shown(account_name, domain.as_deref());
    let meta: Vec<String> =
        [domain, account_name, Some(time)].into_iter().flatten().filter(|p| !p.is_empty()).collect();
    let head = widgets::card_head_with_level(Some(lead), name, meta.join(" · "), mail_text_level(new), None);

    let level = mail_text_level(new);
    let subject = marked(typo::ellipsize(&subject, level, inner), level, theme::text, &words, hit_colour);
    let mut text = column![subject].spacing(space::xs());
    if let Some(preview) = preview {
        let lines: Vec<Element<'a, Message>> = clamp_lines(&preview, typo::body(), inner, PREVIEW_LINES)
            .into_iter()
            .map(|line| marked(line, typo::body(), theme::text_2, &words, hit_colour))
            .collect();
        // Only the lines the preview takes (task E8.11), as [`mail_card_height`] counts them.
        let height = lines.len() as f32 * typo::line_height(typo::body());
        if !lines.is_empty() {
            text = text.push(container(column(lines)).height(height).clip(true));
        }
    }
    let mut parts: Vec<Element<'a, Message>> = vec![head, text.into()];
    if !chips.is_empty() {
        parts.push(container(widgets::chip_row(chips)).center_y(size::control_sm()).clip(true).into());
    }
    mail_card_mark(parts, selected, new, on_press)
}

/// Build either read state with the same left slot inside the card's button.
pub(crate) fn mail_card_mark<'a>(
    parts: Vec<Element<'a, Message>>,
    selected: bool,
    unread: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let marker = if unread { unread_dot() } else { blank().into() };
    widgets::card_with_inset_mark(parts, selected, on_press, marker)
}

/// The height of a mail card ([`mail_card`]) with `preview` lines of preview ([`preview_lines`]),
/// with or without a row of `chips`: the padding and frame, the head, the subject and the preview,
/// the chips' row, [`size::card_gap()`] between the parts – no buttons' row (task E14.2).
pub(crate) fn mail_card_height(preview: usize, chips: bool) -> f32 {
    let pad = layout::panel_padding();
    let gap = size::card_gap();
    let head = size::avatar().max(typo::line_height(typo::body_strong()) + typo::line_height(typo::meta()));
    let mut text = typo::line_height(mail_text_level(false));
    if preview > 0 {
        text += space::xs() + preview as f32 * typo::line_height(typo::body());
    }
    let bottom = if chips { gap + size::control_sm() } else { 0.0 };
    pad.top + pad.bottom + 2.0 * size::HAIRLINE + head + gap + text + bottom
}

/// The button of `m` for `item` ([`Command::Hide`]): for mail the bell "Mute…" – mail's "Hide from
/// inbox" went into the bell's menu (issue #123) –, else "…" with `word` ("Hide" in the action
/// bar).
pub fn hide_action<'a>(item: &Item, word: &'static str) -> RowAction<'a, Message> {
    let act = Message::Act(item.id.clone(), Command::Hide);
    if item.source.kind == SourceKind::Mail {
        RowAction::new(Icon::BellOff, tr("Mute…"), "M", act)
    } else {
        RowAction::new(Icon::Ellipsis, word, "M", act)
    }
}

/// The chips of a mail entry's card: when its invitation is, with the answer given (task E8.3c,
/// `mail::invite_chips`), and the number of messages from two on.
fn entry_chips<'a>(
    item: &Item,
    now: DateTime<Utc>,
    categories: &[torro_ipc::inbox::CategoryInfo],
) -> Vec<Element<'a, Message>> {
    let mut chips: Vec<Element<'a, Message>> =
        item.category.map(|held| mail::category_pill(held, categories)).into_iter().collect();
    chips.extend(item.invite.as_ref().map(|invite| mail::invite_chips(invite, now)).unwrap_or_default());
    if item.count > 1 {
        chips.push(widgets::state_pill(Icon::Mail, trf("{n} messages", &[("n", &item.count)])));
    }
    chips
}

/// The height of the card of the mail entry `item`, `width` wide ([`entry_card`]): its preview takes
/// the lines it needs at that width ([`preview_lines`]).
pub fn entry_card_height(item: &Item, width: f32) -> f32 {
    let preview = preview_lines(card_preview(item).as_deref(), width);
    mail_card_height(preview, item.count > 1 || item.invite.is_some())
}

/// The text a mail entry's card previews: its whole body on one line, which the card cuts after
/// [`PREVIEW_LINES`] lines.
fn card_preview(item: &Item) -> Option<String> {
    item.body.as_deref().map(one_line).filter(|text| !text.is_empty())
}

/// The card of `item` `width` wide (task E8.3): a mail entry's card, without buttons (task E14.2);
/// highlighted under the cursor, where it registers [`Rows::anchors`]`.row` so the menus of the keys
/// open at it. A click opens it, as a row's does. Rows that are no cards – the look is
/// [`Look::Rows`], or the entry is no mail – stay rows ([`entry_row`]).
pub fn entry_card<'a>(item: &'a Item, rows: &Rows<'a>, width: f32) -> Element<'a, Message> {
    if !rows.look.cards() || item.source.kind != SourceKind::Mail {
        return entry_row(item, rows, width);
    }
    let cursor = rows.selected == Some(item.id.as_str());
    let selected = cursor || mail_selected(item, rows);
    let lines = MailLines { preview: card_preview(item), ..MailLines::of(item, rows) };
    let card = MailCard {
        lines,
        chips: entry_chips(item, rows.now, rows.categories),
        selected,
        on_press: Message::Select(item.id.clone()),
    };
    let card: Element<'a, Message> =
        mouse_area(mail_card(card, width)).on_right_press(Message::ContextEntry(item.id.clone())).into();
    if cursor { widgets::anchored(&rows.anchors.row, card) } else { card }
}

/// A row of the list `height` high (including its line, if the design draws one): `content` with the
/// design's inset, centred in the height. How it shows follows the design's list look ([`RowLook`],
/// issue #291): a line below or none; the `selected` row with the bar of the accent colour at its left
/// (`widgets::selection_bar`), or in the accent – its content draws its text in `on_accent` itself
/// ([`mail_content`], [`notice_content`]: `widgets::row_content` around the lines, not around a strip
/// of buttons). Mail content includes its own marker slot, so `Some(unread)` suppresses the button's
/// ordinary left inset.
pub(crate) fn sized_row<'a>(
    content: Element<'a, Message>,
    selected: bool,
    unread: Option<bool>,
    on_press: Message,
    height: f32,
) -> Element<'a, Message> {
    let parts = RowLook::of(layout::list(), selected);
    let pad = layout::row_padding();
    // The bar takes its width from the padding, so the cells do not move when the row is chosen.
    let left = if parts.bar { pad.left - SELECTED_BAR } else { pad.left };
    let height = height - if parts.line { size::HAIRLINE } else { 0.0 };
    let content: Element<'a, Message> = container(content).center_y(Fill).into();
    let body: Element<'a, Message> = button(content)
        .width(Fill)
        .height(height)
        .padding(Padding::new(0.0).left(if unread.is_some() { 0.0 } else { left }).right(pad.right))
        .style(style::button::list_row(selected))
        .on_press(on_press)
        .into();
    let body: Element<'a, Message> = if parts.bar { row![widgets::selection_bar(height), body].into() } else { body };
    if parts.line { column![body, rule::horizontal(size::HAIRLINE).style(style::rule)].into() } else { body }
}

/// The marker slot at the left of a mail row – the row's padding, at least the unread dot with
/// [`space::xs()`] at its sides –, less the selection's bar while one is drawn (`bar`), so the dot and
/// the text stay where they are.
fn mail_row_left(bar: bool) -> f32 {
    let left = layout::row_padding().left.max(new_dot() + 2.0 * space::xs());
    if bar { left - SELECTED_BAR } else { left }
}

fn unread_dot<'a>() -> Element<'a, Message> {
    container(blank())
        .width(new_dot())
        .height(new_dot())
        .style(|t: &Theme| container::Style {
            background: Some(Background::Color(mark_colour(t, palette(t).unread))),
            border: border::rounded(new_dot() / 2.0),
            ..container::Style::default()
        })
        .into()
}

pub(crate) fn mail_text_level(unread: bool) -> Level {
    let strong = typo::body_strong();
    let regular = typo::body();
    mail_text_level_in(unread, strong, regular)
}

/// Only the font weight changes. A custom design may give `body` different metrics or a
/// different family; the strong metrics and family must stay fixed when the mail becomes read.
fn mail_text_level_in(unread: bool, strong: Level, regular: Level) -> Level {
    if unread { strong } else { Level { font: iced::Font { weight: regular.font.weight, ..strong.font }, ..strong } }
}

fn sender_cell(name: &str, room: f32) -> (String, f32) {
    let shown = typo::ellipsize(name, typo::body_strong(), room);
    let width = typo::text_width(&shown, typo::body_strong());
    (shown, width)
}

/// The width the lines of a notice row `width` wide have beside its tile: minus the row's sides, the
/// tile and the gap after it ([`lead_room_in`]). Never negative.
pub fn words_room(width: f32) -> f32 {
    let pad = layout::row_padding();
    (width - pad.left - pad.right - lead_room_in(layout::list(), false)).max(0.0)
}

/// The width the lines of a mail row `width` wide have in `look`: minus the marker slot, the right
/// padding and the avatar with its gap – all of it without an avatar (`avatar = 0`). Never negative.
fn mail_words_room_in(look: ListLook, width: f32) -> f32 {
    (width - mail_row_left(false) - layout::row_padding().right - lead_room_in(look, true)).max(0.0)
}

fn mail_words_room(width: f32) -> f32 {
    mail_words_room_in(layout::list(), width)
}

/// One line of a row: `text` taking the free width (clipped, in case the estimate runs over),
/// then `end` at the right.
fn line<'a>(text: impl Into<Element<'a, Message>>, end: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    row![container(text).width(Fill).clip(true)].extend(end).spacing(space::s()).align_y(Alignment::Center).into()
}

/// The time at the end of the first line (`META`, tabular, `text_3`).
fn time_cell<'a>(time: String) -> Element<'a, Message> {
    typo::text(typo::meta().tabular(), time).style(theme::text_3).wrapping(Wrapping::None).into()
}

/// The width a cell at the end of a line takes from the text, with its gap.
fn end_width(text: &str, level: Level) -> f32 {
    typo::text_width(text, level) + cell_gap()
}

/// The width of the count pill ("3") with its gap.
fn pill_width(count: &str) -> f32 {
    typo::text_width(count, typo::meta()) + 2.0 * space::s() + cell_gap()
}

/// What a mail row shows, the same for a mail entry ([`MailLines::of`]) and for a message in a
/// folder (`mail.rs`, task E3.11): three lines beside the avatar ([`mail_content`]).
pub(crate) struct MailLines<'a> {
    /// The sender's name, else the address ([`sender`]); also the avatar's initials.
    pub name: String,
    /// The domain of the address, when the name is not the address itself.
    pub domain: Option<String>,
    /// The time at the end of the first line.
    pub time: String,
    /// Small marks before the time (paperclip, flag), each with its width.
    pub marks: Vec<(Element<'a, Message>, f32)>,
    /// The subject on one line.
    pub subject: String,
    /// New or unread: the subject in medium weight, the dot in the left inset.
    pub new: bool,
    /// The number of messages from two on.
    pub count: Option<String>,
    /// The start of the text on one line.
    pub preview: Option<String>,
    /// The account's place among the mail accounts, for its colour ([`account_index`]).
    pub account: Option<usize>,
    /// The account's name at the end of the preview's line; `None` with one account (or in a
    /// folder, which is one account's).
    pub account_name: Option<String>,
    /// Where the words of a search matched (issue #136): stressed in the subject and the preview.
    pub hits: Option<Hits<'a>>,
}

/// The matches of a search in a mail row or card (issue #136): the words that matched are stressed
/// wherever they show in the subject and the preview – in medium weight (semibold in a subject
/// already medium) and in `text` –, never by colour alone. The preview is the passage around the
/// first match (the app puts it in place of the start of the text).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hits<'a> {
    pub highlight: &'a MailHighlight,
    /// The text colour of the theme, for the matches in the preview (`text_2` around them).
    pub text: Color,
}

impl Hits<'_> {
    /// The words that matched, in lower case, each once.
    fn words(&self) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        let spans = self.highlight.subject.iter().chain(&self.highlight.snippet).filter(|span| span.hit);
        for word in spans.map(|span| span.text.trim().to_lowercase()).filter(|word| !word.is_empty()) {
            if !words.contains(&word) {
                words.push(word);
            }
        }
        words
    }
}

/// `line` in `level`, coloured by `colour`, with `words` stressed ([`Hits`]): their weight one step
/// up and in `hit_colour` – or, with `None` (the chosen row in the accent, issue #291), in the line's
/// own colour, which is `on_accent` there. A line without a match is plain text.
fn marked<'a>(
    line: String,
    level: Level,
    colour: fn(&Theme) -> iced::widget::text::Style,
    words: &[String],
    hit_colour: Option<Color>,
) -> Element<'a, Message> {
    let spans = crate::search::spans_of(&line, words);
    if !spans.iter().any(|span| span.hit) {
        return typo::text(level, line).style(colour).wrapping(Wrapping::None).into();
    }
    let stressed = if level.font == typo::body_strong().font { theme::font_semibold() } else { theme::font_medium() };
    let spans: Vec<iced::widget::text::Span<'a, Message>> = spans
        .into_iter()
        .map(|span| {
            let piece = iced::widget::span(span.text);
            match (span.hit, hit_colour) {
                (true, Some(colour)) => piece.font(stressed).color(colour),
                (true, None) => piece.font(stressed),
                (false, _) => piece,
            }
        })
        .collect();
    iced::widget::rich_text(spans)
        .size(level.size)
        .line_height(level.line_height)
        .font(level.font)
        .style(colour)
        .wrapping(Wrapping::None)
        .into()
}

impl<'a> MailLines<'a> {
    /// The lines of a mail entry's row.
    pub fn of(item: &Item, rows: &Rows<'a>) -> MailLines<'a> {
        let (name, domain) = sender(item);
        MailLines {
            name,
            domain,
            time: time_text_in(item, rows.view, rows.now, rows.bare),
            marks: Vec::new(),
            subject: one_line(&item.title),
            new: item.state == State::New,
            count: (item.count > 1).then(|| item.count.to_string()),
            preview: preview(item),
            account: account_index(item, &rows.accounts),
            account_name: (rows.accounts.len() > 1).then(|| account_label(&item.source)),
            hits: None,
        }
    }
}

/// The account a mail row or card names (`account_name`, the domain of its address), unless it is
/// the sender's `domain` (task E8.11): mail within one's own domain would name it twice, and the
/// avatar keeps the account's colour.
pub(crate) fn account_shown(account_name: Option<String>, domain: Option<&str>) -> Option<String> {
    account_name.filter(|account| domain != Some(account.as_str()))
}

/// The content of a mail row `width` wide, as the design's list look draws it (issue #291): its rows
/// ([`mail_rows`]) or one line ([`mail_line`]); on the `selected` row of a design with
/// `selection = "fill"` in `on_accent` (`widgets::row_content`).
pub(crate) fn mail_content<'a>(lines: MailLines<'a>, width: f32, selected: bool) -> Element<'a, Message> {
    let look = layout::list();
    let content = match look.style {
        ListStyle::Line => mail_line(look, lines, width, selected),
        ListStyle::Rows | ListStyle::Cards => mail_rows(look, lines, width, selected),
    };
    widgets::row_content(content, selected)
}

/// A mail row `width` wide in `look` (task E3.9, `style = "rows"`): unread marker slot, the avatar
/// (`[list] avatar`; none at 0, its place goes to the text), then sender · domain · (marks) · time,
/// subject (· count), the preview on `preview_lines` lines (· account on the last, unless it is the
/// sender's domain: [`account_shown`]).
fn mail_rows<'a>(look: ListLook, lines: MailLines<'a>, width: f32, selected: bool) -> Element<'a, Message> {
    let parts = RowLook::of(look, selected);
    let room = mail_words_room_in(look, width);
    let MailLines { name, domain, time, marks, subject, new, count, preview, account, account_name, hits } = lines;
    let account_name = account_shown(account_name, domain.as_deref());
    let words = hits.map(|hits| hits.words()).unwrap_or_default();
    // On the accent the matches keep the row's own colour (`on_accent`) and stand out by weight.
    let hit_colour = hits.map(|hits| hits.text).filter(|_| !parts.fill);

    // Sender, then the domain in what is left, then the marks and the time.
    let marks_width: f32 = marks.iter().map(|(_, width)| width + cell_gap()).sum();
    let first_room = room - end_width(&time, typo::meta()) - marks_width;
    let (name_shown, name_width) = sender_cell(&name, first_room);
    let left = first_room - name_width - space::s();
    let domain = domain.map(|domain| typo::ellipsize(&domain, typo::meta(), left)).filter(|d| !d.is_empty());
    let sender = container(typo::text(mail_text_level(new), name_shown).style(theme::text).wrapping(Wrapping::None))
        .width(name_width)
        .clip(true);
    let mut who = row![sender].spacing(space::s()).align_y(Alignment::Center);
    if let Some(domain) = domain {
        who = who.push(typo::text(typo::meta(), domain).style(theme::text_3).wrapping(Wrapping::None));
    }
    let mut ends: Vec<Element<'a, Message>> = marks.into_iter().map(|(mark, _)| mark).collect();
    ends.push(time_cell(time));
    let first = line(who, ends);

    // The subject, with the number of messages from two on.
    let level = mail_text_level(new);
    let subject_room = room - count.as_deref().map_or(0.0, pill_width);
    let subject = marked(typo::ellipsize(&subject, level, subject_room), level, theme::text, &words, hit_colour);
    let second = line(subject, count.map(|count| widgets::pill(None, count)).into_iter().collect());

    // The preview on the design's lines, with the account from two accounts on at the end of the last.
    let account_room = account_name.as_deref().map_or(0.0, |name| {
        (typo::text_width(name, typo::meta()) + new_dot() + space::xs() + cell_gap()).min(room / 3.0)
    });
    let mut previews: Vec<String> = preview
        .map(|text| clamp_lines(&text, typo::body(), room - account_room, preview_count(look)))
        .unwrap_or_default();
    if previews.is_empty() {
        // A mail without text keeps the row's lines: the subject stays where it is.
        previews.push(String::new());
    }
    let last = previews.len() - 1;
    let mut ends: Vec<Element<'a, Message>> = account_name
        .map(|name| account_cell(account, typo::ellipsize(&name, typo::meta(), account_room - cell_gap())))
        .into_iter()
        .collect();
    let mut text_lines = column![first, second].width(Fill);
    for (at, text) in previews.into_iter().enumerate() {
        let text = marked(text, typo::body(), theme::text_2, &words, hit_colour);
        text_lines = text_lines.push(line(text, if at == last { std::mem::take(&mut ends) } else { Vec::new() }));
    }
    let lead = lead_in(look, true);
    let marker = marker_slot(new, parts.bar, lead.max(typo::line_height(level)));
    let content: Element<'a, Message> = if look.avatar > 0.0 {
        let face = container(avatar(&name, account, look.avatar)).center_x(lead);
        row![face, text_lines].spacing(space::m()).align_y(Alignment::Start).into()
    } else {
        // No avatar: its place goes to the text, which starts at the row's edge.
        text_lines.into()
    };
    row![marker, content].align_y(Alignment::Start).into()
}

/// How many lines the preview of a mail row takes in `look` (`[list] preview_lines`, 1 or 2).
fn preview_count(look: ListLook) -> usize {
    usize::from(look.preview_lines.clamp(1, 2))
}

/// The marker slot of a mail row ([`mail_row_left`] wide, `height` high): the unread dot centred in it
/// while the mail is `new`. The selection's `bar` takes its width from this slot; the dot stays at the
/// same x.
fn marker_slot<'a>(new: bool, bar: bool, height: f32) -> Element<'a, Message> {
    let marker: Element<'a, Message> = if new { unread_dot() } else { blank().into() };
    container(marker)
        .width(mail_row_left(bar))
        .height(height)
        .padding(Padding::ZERO.right(if bar { SELECTED_BAR } else { 0.0 }))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .into()
}

/// A mail as one line `width` wide in `look` (`style = "line"`, Console, issue #291), in the order of
/// its rows ([`mail_rows`]): the marker slot with the unread dot, then in the columns of a line
/// ([`LineColumns`], without a column head) the marks (what the mail is, an invitation, an attachment, a
/// flag), the sender with the domain, the subject (· the number of messages from two on) with the
/// preview in `text_3`, the account from two accounts on (unless it is the sender's domain,
/// [`account_shown`]) and the time at the right. What has no room is cut with "…" ([`LineWords`]).
pub(crate) fn mail_line<'a>(look: ListLook, lines: MailLines<'a>, width: f32, selected: bool) -> Element<'a, Message> {
    let parts = RowLook::of(look, selected);
    let MailLines { name, domain, time, marks, subject, new, count, preview, account, account_name, hits } = lines;
    let account_name = account_shown(account_name, domain.as_deref());
    let words = hits.map(|hits| hits.words()).unwrap_or_default();
    let hit_colour = hits.map(|hits| hits.text).filter(|_| !parts.fill);
    let level = mail_text_level(new);
    let columns = LineColumns::of(width, typo::text_width(&time, typo::meta().tabular()));
    let shown = mail_line_words(
        columns,
        (&name, domain.as_deref()),
        (&subject, new, count.as_deref()),
        &preview,
        &account_name,
        &time,
    );
    let marks = row(marks.into_iter().map(|(mark, _)| mark)).spacing(space::xs()).align_y(Alignment::Center);
    let cells =
        line_cells(columns, shown, marks.into(), (level, theme::text), level, count, account, (&words, hit_colour));
    let marker = marker_slot(new, parts.bar, typo::line_height(level));
    row![marker, cells].align_y(Alignment::Center).into()
}

/// What the cells of a mail's line show in `columns` ([`LineWords`]): the sender `name` with the
/// `domain`, the `subject` (medium while `new`, · `count`) with the `preview`, the account and the time.
fn mail_line_words(
    columns: LineColumns,
    (name, domain): (&str, Option<&str>),
    (subject, new, count): (&str, bool, Option<&str>),
    preview: &Option<String>,
    account: &Option<String>,
    time: &str,
) -> LineWords {
    LineWords::of(
        columns,
        (name, typo::body_strong(), domain),
        (subject, mail_text_level(new), count),
        preview.as_deref(),
        account.as_deref(),
        time,
    )
}

/// How far below a row's top the middle of its first line lies – where the action bar sits (task
/// E3.11, `widgets::row_actions_at`): the row is `height` high with its line, its `lines` lines of
/// text are centred in it.
pub(crate) fn first_line_middle(height: f32, lines: usize) -> f32 {
    lines_top(height, lines) + typo::line_height(typo::body()) / 2.0
}

/// How far below a row's top its `lines` lines of text start when they are centred in the row
/// `height` high with its line: the row's padding above them.
pub(crate) fn lines_top(height: f32, lines: usize) -> f32 {
    lines_top_for(height, lines, typo::line_height(typo::body()))
}

/// [`lines_top`] for lines `line` high: the row's line below is no part of it, where the design draws
/// one (`[list] separators`).
fn lines_top_for(height: f32, lines: usize, line: f32) -> f32 {
    let rule = if layout::list().separators { size::HAIRLINE } else { 0.0 };
    (height - rule - lines as f32 * line) / 2.0
}

/// How many lines of text `item`'s row has in the active design ([`line_count_in`]).
pub(crate) fn line_count(item: &Item) -> usize {
    line_count_in(layout::list(), item)
}

/// How many lines of text `item`'s row has in `look`: one in a line (`style = "line"`); else for mail
/// the sender's and the subject's and the preview's (`preview_lines`), two for a notice with a text,
/// one without.
pub(crate) fn line_count_in(look: ListLook, item: &Item) -> usize {
    if look.style == ListStyle::Line {
        1
    } else if item.source.kind == SourceKind::Mail {
        2 + preview_count(look)
    } else if words(item).1.is_some_and(|text| !text.is_empty()) {
        2
    } else {
        1
    }
}

/// The content of any other entry's row `width` wide, as the design's list look draws it (issue
/// #291): its rows ([`notice_rows`]) or one line ([`notice_line`]); while it is under the cursor in a
/// design with `selection = "fill"` in `on_accent` (`widgets::row_content`).
pub(crate) fn notice_content<'a>(item: &'a Item, rows: &Rows<'a>, width: f32) -> Element<'a, Message> {
    let look = layout::list();
    let content = match look.style {
        ListStyle::Line => notice_line(item, rows, width),
        ListStyle::Rows | ListStyle::Cards => notice_rows(look, item, rows, width),
    };
    widgets::row_content(content, rows.selected == Some(item.id.as_str()))
}

/// A notice row in `look` (`style = "rows"`): the source's tile in the lead ([`lead_in`]), then title
/// (· count) · time, and the text below.
fn notice_rows<'a>(look: ListLook, item: &'a Item, rows: &Rows<'a>, width: f32) -> Element<'a, Message> {
    let room = words_room(width);
    let level = if item.state == State::New { typo::body_strong() } else { typo::body() };
    let time = time_text_in(item, rows.view, rows.now, rows.bare);
    let count = (item.count > 1).then(|| item.count.to_string());
    let title_room = room - end_width(&time, typo::meta()) - count.as_deref().map_or(0.0, pill_width);
    let (title, text) = words(item);
    let title =
        typo::text(level, typo::ellipsize(&title, level, title_room)).style(theme::text).wrapping(Wrapping::None);
    let mut ends: Vec<Element<'a, Message>> = count.map(|count| widgets::pill(None, count)).into_iter().collect();
    ends.push(time_cell(time));
    let mut words = column![line(title, ends)].width(Fill);
    if let Some(text) = text.map(|text| typo::ellipsize(&text, typo::body(), room)).filter(|text| !text.is_empty()) {
        words = words
            .push(container(typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::None)).clip(true));
    }
    let mark = container(source_tile(item)).center_x(lead_in(look, false));
    row![mark, words].spacing(space::m()).align_y(Alignment::Center).into()
}

/// A notice as one line `width` wide (`style = "line"`, Console, issue #291), in the order of its rows
/// ([`notice_rows`]) and in the columns of a mail line ([`LineColumns`], one grid in a mixed list): the
/// source's tile in the marks column, the source's name where a mail has its sender, the title (· count)
/// with the text in `text_3` where a mail has its subject and preview, the time at the right. Its cells
/// start where a mail's do, after the marker slot.
pub(crate) fn notice_line<'a>(item: &'a Item, rows: &Rows<'a>, width: f32) -> Element<'a, Message> {
    let level = if item.state == State::New { typo::body_strong() } else { typo::body() };
    let count = (item.count > 1).then(|| item.count.to_string());
    let columns = LineColumns::of(
        width,
        typo::text_width(&time_text_in(item, rows.view, rows.now, rows.bare), typo::meta().tabular()),
    );
    let shown = notice_line_words(item, rows, columns);
    let tile = container(source_tile(item)).align_y(iced::Center);
    let cells = line_cells(columns, shown, tile.into(), (typo::body(), theme::text_2), level, count, None, (&[], None));
    // The marker slot of a mail is wider than the row's padding where the padding is narrower than the
    // unread dot: the notice's cells start at the same x.
    let lead = (mail_row_left(false) - layout::row_padding().left).max(0.0);
    row![blank().width(lead), cells].align_y(Alignment::Center).into()
}

/// What the cells of a notice's line show in `columns` ([`LineWords`]): the source's name
/// ([`SourceRef::label`]) where a mail has its sender, the title (medium while new, · count) with the
/// text ([`words`]), the time.
fn notice_line_words(item: &Item, rows: &Rows<'_>, columns: LineColumns) -> LineWords {
    let level = if item.state == State::New { typo::body_strong() } else { typo::body() };
    let count = (item.count > 1).then(|| item.count.to_string());
    let (title, text) = words(item);
    let time = time_text_in(item, rows.view, rows.now, rows.bare);
    LineWords::of(
        columns,
        (&item.source.label(), typo::body(), None),
        (&title, level, count.as_deref()),
        text.as_deref(),
        None,
        &time,
    )
}

/// The source's icon of `item` on a small tile ([`tile_style`]): in the attention colour while it
/// waits (`on_accent` in a chosen row in the accent, [`mark_colour`]), else `text_2`.
pub(crate) fn source_tile<'a>(item: &Item) -> Element<'a, Message> {
    let waiting = is_waiting(item);
    let mark = icon::icon(source_icon(&item.source), icon::SIZE).style(move |t: &Theme, _| {
        let p = palette(t);
        svg::Style { color: Some(if waiting { mark_colour(t, p.attention_text) } else { p.text_2 }) }
    });
    container(mark).center(tile()).style(tile_style).into()
}

/// A source tile: `raised` in a `line` frame. In a chosen row in the accent the icon is `on_accent`,
/// which would barely read on `raised`: there the tile has no surface and an `on_accent` frame.
fn tile_style(t: &Theme) -> container::Style {
    let p = palette(t);
    let on_accent = theme::on_accent_theme(t) == *t;
    let (background, frame) = if on_accent { (None, p.on_accent) } else { (Some(Background::Color(p.raised)), p.line) };
    container::Style {
        background,
        border: border::rounded(radius::sm()).color(frame).width(size::HAIRLINE),
        ..container::Style::default()
    }
}

/// The round avatar of a mail row or card, `edge` wide (`[list] avatar` in a row, [`size::avatar()`]
/// on a card, [`size::pill()`] in the reader): the initials of `name` ([`initials`], `CAPTION`,
/// `text`) on the colour of the account at `account` ([`account_index`]) tinted over the panel, or
/// on `raised` for an account not listed.
pub(crate) fn avatar<'a>(name: &str, account: Option<usize>, edge: f32) -> Element<'a, Message> {
    let letters = typo::text(typo::caption(), initials(name)).style(theme::text).wrapping(Wrapping::None);
    let face = container(letters).center(edge).style(move |t: &Theme| {
        let p = palette(t);
        let surface = match account {
            Some(i) => theme::over(p.tinted(p.calendar[i % p.calendar.len()]), p.panel),
            None => p.raised,
        };
        container::Style {
            background: Some(Background::Color(surface)),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        }
    });
    face.into()
}

/// The account at the end of the preview's line: its dot in the account's colour and its name
/// (`META`, `text_3`).
fn account_cell<'a>(account: Option<usize>, name: String) -> Element<'a, Message> {
    let dot = container(blank()).width(space::s()).height(space::s()).style(move |t: &Theme| {
        let p = palette(t);
        let colour = account.map_or(p.text_3, |i| mark_colour(t, p.calendar[i % p.calendar.len()]));
        container::Style {
            background: Some(Background::Color(colour)),
            border: border::rounded(space::s() / 2.0),
            ..container::Style::default()
        }
    });
    let name = typo::text(typo::meta(), name).style(theme::text_3).wrapping(Wrapping::None);
    row![dot, name].spacing(space::xs()).align_y(Alignment::Center).into()
}

/// Up to two initials of `name` for an avatar: the first letters of its first and its last word
/// ("Jonas Weber" → "JW", "IT Service Desk" → "ID", "GitHub" → "G"), upper case; of an address
/// the local part, split at dots ("lena.hoffmann@…" → "LH"); words that start with no letter or
/// digit (a "(") are skipped. "?" when nothing is left.
pub fn initials(name: &str) -> String {
    // An address as name: its local part ("lena.hoffmann@…" → "LH").
    let name = name.split('@').next().unwrap_or_default();
    let firsts: Vec<char> = one_line(name)
        .split(|c: char| c.is_whitespace() || c == '.' || c == '@' || c == '_' || c == '-')
        .filter_map(|word| word.chars().next())
        .filter(|c| c.is_alphanumeric())
        .collect();
    let picked: String = match firsts.as_slice() {
        [] => return "?".to_owned(),
        [only] => only.to_string(),
        [first, .., last] => [*first, *last].iter().collect(),
    };
    picked.to_uppercase()
}

/// Who sent a mail entry, for its row: the sender's name (else the address, else the source), and
/// the domain of the address when the name is not the address itself.
pub fn sender(item: &Item) -> (String, Option<String>) {
    let from = item.from.as_ref();
    let address = from.and_then(|from| from.address.as_deref()).map(one_line).filter(|a| !a.is_empty());
    let name = from.map(|from| one_line(&from.name)).filter(|name| !name.is_empty());
    match (name, address) {
        (Some(name), Some(address)) if name != address => (name, rules::domain_of(&address)),
        (Some(name), _) => (name, None),
        (None, Some(address)) => (address, None),
        (None, None) => (item.source.label(), None),
    }
}

/// The place of `item`'s mail account among `accounts` (addresses in the sidebar's order), which
/// picks the account's colour from the default colours (`Palette::calendar`: the first blue, the
/// second green …). `None` for entries that are no mail and accounts not listed: neutral.
pub fn account_index(item: &Item, accounts: &[&str]) -> Option<usize> {
    (item.source.kind == SourceKind::Mail).then(|| accounts.iter().position(|a| *a == item.source.name)).flatten()
}

/// The name of a mail account in a row: the domain of its address ("work.example.org"), which
/// tells two accounts of one person apart; the address when it has none.
pub fn account_label(source: &SourceRef) -> String {
    rules::domain_of(&source.name).unwrap_or_else(|| one_line(&source.name))
}

/// The empty state of a view: its icon over what shows up here and when – the shared
/// `widgets::empty_state` with an icon on top, as the error state has one.
pub(crate) fn empty_state<'a>(which: Icon, title: &'a str, hint: &'a str) -> Element<'a, Message> {
    let mark = icon::icon(which, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let words = column![
        typo::text(typo::body_strong(), title).align_x(Alignment::Center),
        typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::xs())
    .align_x(Alignment::Center);
    let content = column![mark, words].spacing(space::m()).align_x(Alignment::Center).max_width(size::form_max() / 2.0);
    center(content).padding(space::xl()).into()
}

/// The icon of a source: the suite's grid, a spark for coding agents, a window for scripts, the
/// envelope for a mail account, and the services' own (task E5.15): GitHub's mark, a cloud for
/// Nextcloud, a speech bubble for Talk and a radio tower for ntfy. Only the kind of the source
/// decides – a script that calls itself `github` is a script and shows the window, so no client can
/// borrow a service's face.
pub fn source_icon(source: &SourceRef) -> Icon {
    match source.kind {
        SourceKind::Suite => Icon::LayoutGrid,
        SourceKind::Agent => Icon::Sparkles,
        SourceKind::Script => Icon::AppWindow,
        SourceKind::Mail => Icon::Mail,
        SourceKind::Github => Icon::Github,
        SourceKind::Nextcloud => Icon::Cloud,
        SourceKind::Talk => Icon::MessageCircle,
        SourceKind::Ntfy => Icon::RadioTower,
    }
}

/// The name of `source` where it stands among `known` (every source the inbox lists, task E5.15):
/// its label ([`SourceRef::label`]: "GitHub", "Nextcloud", "Talk", "ntfy"), and – only while
/// `known` holds another account of the same service – the account after " · " ("GitHub ·
/// anna-berg", "Nextcloud · anna@cloud.example.org"), so two columns or chips of one service can be
/// told apart. Mail names its address, scripts and agents their own names anyway, so they keep their
/// label. The account is the source's name as the core keeps it, shown as plain text.
pub fn source_name<'s>(source: &SourceRef, known: impl IntoIterator<Item = &'s SourceRef>) -> String {
    let label = source.label();
    let service =
        matches!(source.kind, SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy);
    let twin = service && known.into_iter().any(|other| other.kind == source.kind && other.name != source.name);
    if twin { format!("{label} · {}", one_line(&source.name)) } else { label }
}

/// The value of the first trait `key` of `item` that has text.
fn facet_value(item: &Item, key: FacetKey) -> Option<&str> {
    item.facets.iter().find(|facet| facet.key == key).map(|facet| facet.value.as_str()).filter(|v| !v.is_empty())
}

/// What a service's traits say about `item` (task E5.15), in the order to read them, each with the
/// icon of its row in the panel – the words of the meta line ([`facet_line`]):
///
/// - GitHub: the repository (`owner/repo`) and why it came, in the suite's words ("Review
///   requested", "Check failed" …, [`reason_word`]; a reason without a word is left out);
/// - Nextcloud: the app ("Files", "Talk", else its id, `rules::app_label`) and the person;
/// - Talk: the kind of conversation ("Direct message", "Mention", "Group conversation") – the person
///   is the entry's title already;
/// - ntfy: the topic's label and the priority, unless it is ntfy's default 3.
///
/// Nothing for every other source. Repository, app id and person are foreign text, shown as plain
/// text (Iced interprets nothing, [`one_line`] drops control characters).
pub fn facet_words(item: &Item) -> Vec<(Icon, String)> {
    let value = |key| facet_value(item, key);
    let mut words = Vec::new();
    match item.source.kind {
        SourceKind::Github => {
            if let Some(repo) = value(FacetKey::Repo) {
                words.push((Icon::FolderGit2, one_line(repo)));
            }
            words.extend(value(FacetKey::Reason).and_then(|reason| reason_word(reason, value(FacetKey::Result))));
        }
        SourceKind::Nextcloud => {
            if let Some(app) = value(FacetKey::App) {
                let mark = if app == "spreed" { Icon::MessageCircle } else { Icon::Cloud };
                words.push((mark, one_line(&rules::app_label(app))));
            }
            if let Some(person) = value(FacetKey::Person) {
                words.push((Icon::User, one_line(person)));
            }
        }
        SourceKind::Talk => {
            let room = match value(FacetKey::Kind) {
                Some("direct") => Some((Icon::User, tr("Direct message"))),
                Some("mention") => Some((Icon::Users, tr("Mention"))),
                Some("group") => Some((Icon::Users, tr("Group conversation"))),
                _ => None,
            };
            words.extend(room.map(|(mark, word)| (mark, word.to_owned())));
        }
        SourceKind::Ntfy => {
            if let Some(topic) = value(FacetKey::Topic) {
                words.push((Icon::RadioTower, one_line(topic)));
            }
            if let Some(priority) = value(FacetKey::Priority).filter(|priority| *priority != "3") {
                words.push((Icon::SignalHigh, trf("Priority {n}", &[("n", &priority)])));
            }
        }
        SourceKind::Suite | SourceKind::Agent | SourceKind::Script | SourceKind::Mail => {}
    }
    words
}

/// Why GitHub notified, in the suite's words with an icon ("Review requested" with the eye, as the
/// tasks app has it); a workflow run by how it ended. `None` for a reason the suite has no word for
/// (`other`, anything new GitHub brings).
fn reason_word(reason: &str, result: Option<&str>) -> Option<(Icon, String)> {
    let (mark, word) = match (reason, result) {
        ("ci_activity", Some("failed")) => (Icon::CircleX, tr("Check failed")),
        ("ci_activity", Some("succeeded")) => (Icon::CircleCheck, tr("Check passed")),
        ("ci_activity", Some("cancelled")) => (Icon::CircleDashed, tr("Check cancelled")),
        ("ci_activity", _) => (Icon::CircleDotDashed, tr("Checks")),
        ("review_requested", _) => (Icon::Eye, tr("Review requested")),
        ("mention" | "team_mention", _) => (Icon::User, tr("Mentioned")),
        ("assign", _) => (Icon::UserCheck, tr("Assigned")),
        ("author", _) => (Icon::SquarePen, tr("Author")),
        ("approval_requested", _) => (Icon::Eye, tr("Approval requested")),
        ("security_alert", _) => (Icon::ShieldAlert, tr("Security alert")),
        ("invitation", _) => (Icon::Users, tr("Invitation")),
        ("comment", _) => (Icon::Reply, tr("Comment")),
        ("state_change", _) => (Icon::CircleDot, tr("State changed")),
        ("subscribed" | "manual", _) => (Icon::Bell, tr("Subscribed")),
        _ => return None,
    };
    Some((mark, word.to_owned()))
}

/// The words of [`facet_words`] on one line, " · " between them ("torro/workplace · Review
/// requested", "Files · Jonas Weber", "Server · Priority 5"); `None` without any.
pub fn facet_line(item: &Item) -> Option<String> {
    let words: Vec<String> = facet_words(item).into_iter().map(|(_, word)| word).collect();
    (!words.is_empty()).then(|| words.join(" · "))
}

/// The word for an entry that waits ([`is_waiting`]): "Urgent" for an alert (an ntfy message of
/// priority 5, task E5.15) – nobody waits there, something is wrong –, else "Waiting for you".
pub fn waiting_word(item: &Item) -> &'static str {
    if item.kind == Kind::Alert { tr("Urgent") } else { tr("Waiting for you") }
}

/// The title and the preview of `item`'s row, on one line each ([`one_line`]): for a coding agent's
/// session with a name ([`session_name`]) the name, and below or beside it [`agent_line`]; for
/// every other entry its title and the first line of its body ([`preview`]) – or, for a service's
/// entry without one (GitHub sends no text), what its traits say ([`facet_line`], task E5.15).
pub fn words(item: &Item) -> (String, Option<String>) {
    match session_name(item) {
        Some(name) => (name, Some(agent_line(item))),
        None => (one_line(&item.title), preview(item).or_else(|| facet_line(item))),
    }
}

/// The name of the coding agent's session behind `item` (task E2.6), on one line; `None` for
/// entries without an agent, sessions without a name and names that are nothing but blanks.
pub fn session_name(item: &Item) -> Option<String> {
    let name = one_line(item.agent.as_ref()?.title.as_deref()?);
    (!name.is_empty()).then_some(name)
}

/// What a coding agent does and where, for the line under its session's name: the entry's title
/// (the core's words, "Claude Code is waiting for you"), then project and branch when known,
/// apart by " · ".
pub fn agent_line(item: &Item) -> String {
    let agent = item.agent.as_ref();
    let place = agent.into_iter().flat_map(|agent| [agent.project.as_deref(), agent.branch.as_deref()]).flatten();
    std::iter::once(item.title.as_str())
        .chain(place)
        .map(one_line)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
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

/// The time at the end of a row in the mail window (`mail_window`, `Rows::bare`) or another: until
/// when a snoozed entry sleeps ("until Wed, 14 Oct", [`wake_text`]), when it was done in "Done",
/// otherwise when it last changed ([`stamp`]). Only the mail window tells a follow-up's condition,
/// "until Wed, 14 Oct, only if no reply" (decision 12 of E19, review of PR #327); the hub, the App Feed
/// and the notifications tell the time alone, as before #315.
pub fn time_text_in(item: &Item, view: &View, now: DateTime<Utc>, mail_window: bool) -> String {
    match (view, &item.state) {
        (_, State::Snoozed { until }) if mail_window && item.follow_up.is_some_and(|follow| follow.unless_reply) => {
            trf("until {time}, only if no reply", &[("time", &wake_text(*until, now))])
        }
        (_, State::Snoozed { until }) => trf("until {time}", &[("time", &wake_text(*until, now))]),
        (View::Done, State::Done { at }) => stamp(*at, now),
        _ => stamp(item.updated, now),
    }
}

/// When `then` was, in local time, for the end of a row (task E8.14, issue #115): the clock time
/// today ("16:40"), "Yesterday 16:40" the day before, else the short date ("Sat, 3 Oct"). A clock
/// time reads at a glance; "17 h" has to be counted back. A moment a little ahead of `now` (a
/// clock ahead) on the same day is shown as today.
pub fn stamp(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (then, today) = (then.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    let (day, time) = (then.date_naive(), datetime::format_time(then.time()));
    if day == today {
        time
    } else if today.pred_opt() == Some(day) {
        trf("Yesterday {time}", &[("time", &time)])
    } else {
        datetime::format_short_date(day, today)
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

/// When the mail window follows a conversation up (task E19.4), for its hint and its panel: "Follow
/// up 18:30" today, else with the short date ([`wake_text`]); with `unless_reply` the whole sentence
/// "Follow up 18:30, only if no reply" (one text to translate, never pieces). The hub and the App
/// Feed say "Later until …".
pub fn follow_up_text(until: DateTime<Utc>, unless_reply: bool, now: DateTime<Utc>) -> String {
    let time = wake_text(until, now);
    if unless_reply {
        trf("Follow up {time}, only if no reply", &[("time", &time)])
    } else {
        trf("Follow up {time}", &[("time", &time)])
    }
}

/// What the empty state of the mail window's "Follow up" says (task E19.4).
pub fn follow_up_empty() -> (&'static str, &'static str) {
    (tr("No follow-ups."), tr("Conversations you follow up on wait here until their day."))
}

/// What the empty state of `view` says: what shows up here, and when.
pub fn empty_text(view: &View) -> (&'static str, &'static str) {
    match view {
        // The overview has an empty state per column (`overview.rs`); this one stands for both.
        View::Overview => (tr("Nothing for you right now."), tr("Mail, agents, approvals and notices show up here.")),
        View::ForMe => {
            (tr("Nothing for you right now."), tr("Waiting agents, approvals and account problems show up here."))
        }
        View::Info => (tr("No info right now."), tr("Notices from scripts and finished jobs show up here.")),
        View::Snoozed => (tr("Nothing for later."), tr("What you put off waits here until its time.")),
        View::Done => (tr("Nothing done yet."), tr("Done entries stay here for 30 days.")),
        View::Source(_) => (tr("Nothing open from this source."), tr("New entries from it show up here.")),
        // The views of mail have their own list and empty states (`mail::list`).
        View::Folder(_)
        | View::Mail
        | View::Search(_)
        | View::All
        | View::Hidden
        | View::Archive
        | View::ReplyLater
        | View::SetAside => (tr("No messages here."), tr("Mail in this folder shows up here.")),
        // Task E12.8: a category the person filtered by holds nothing at the moment.
        View::Category(_) => (tr("No mail of this kind."), tr("Mail sorted into it shows up here.")),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone};
    use torro_inbox::demo;
    use torro_mail::{InvitationAnswer, Role};

    use torro_core::i18n::trc;

    use super::*;
    use crate::mail::MailAction;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    /// Task E8.14 (issue #115): a clock time instead of "17 h" for what came today, "Yesterday"
    /// with the time the day before, else the short date.
    #[test]
    fn rows_stamp_the_clock_time_not_how_long_ago() {
        let local =
            |d, h, m| Local.with_ymd_and_hms(2026, 10, d, h, m, 0).single().expect("a valid local time").to_utc();
        let morning = local(13, 10, 30);
        assert_eq!(stamp(morning - TimeDelta::minutes(5), morning), "10:25", "5 min ago");
        let late = local(13, 23, 50);
        assert_eq!(stamp(late - TimeDelta::hours(23), late), "00:50", "23 h ago, the same day");
        let yesterday = trf("Yesterday {time}", &[("time", &"11:30")]);
        assert_eq!(stamp(morning - TimeDelta::hours(23), morning), yesterday, "23 h ago, the day before");
        let today = morning.with_timezone(&Local).date_naive();
        let last_week = today - TimeDelta::days(7);
        assert_eq!(stamp(local(6, 9, 0), morning), datetime::format_short_date(last_week, today));
        assert_eq!(stamp(morning + TimeDelta::minutes(2), morning), "10:32", "a clock ahead");
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
        assert_eq!(time_text_in(snoozed, &View::Snoozed, now(), false), until, "until when, not only a date");
        let done = by_id("demo-11");
        let State::Done { at } = done.state else { panic!("demo-11 is done") };
        assert_eq!(time_text_in(done, &View::Done, now(), false), stamp(at, now()), "when it was done");
        let waiting = by_id("demo-1");
        assert_eq!(time_text_in(waiting, &View::ForMe, now(), false), stamp(waiting.updated, now()));
    }

    /// Review of #315: a follow-up only if no reply says so in its row, and the hint says it as one
    /// sentence – never pieces glued together.
    #[test]
    fn a_follow_up_only_if_no_reply_says_so_in_whole_sentences() {
        let items = demo::sample(now());
        let snoozed = items.iter().find(|i| i.id == "demo-10").expect("in the sample");
        let State::Snoozed { until } = snoozed.state else { panic!("demo-10 is snoozed") };
        let time = wake_text(until, now());
        let follow = torro_inbox::item::FollowUp { at: until, unless_reply: true, since: now() };
        let only = Item { follow_up: Some(follow), ..snoozed.clone() };
        let plain = trf("until {time}", &[("time", &time)]);
        // Only the mail window tells the condition (decision 12 of E19, review of PR #327).
        assert_eq!(
            time_text_in(&only, &View::Snoozed, now(), true),
            trf("until {time}, only if no reply", &[("time", &time)])
        );
        assert_eq!(time_text_in(&only, &View::Snoozed, now(), false), plain, "not the hub, the App Feed");
        let any = Item { follow_up: Some(torro_inbox::item::FollowUp { unless_reply: false, ..follow }), ..only };
        assert_eq!(time_text_in(&any, &View::Snoozed, now(), true), plain);
        assert_eq!(follow_up_text(until, true, now()), trf("Follow up {time}, only if no reply", &[("time", &time)]));
        assert_eq!(follow_up_text(until, false, now()), trf("Follow up {time}", &[("time", &time)]));
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
    fn a_named_session_shows_its_name_over_what_the_agent_does() {
        let named = demo::sample(now()).remove(0);
        let name = named.agent.as_ref().and_then(|agent| agent.title.clone()).expect("demo-1 has a name");
        let (title, below) = words(&named);
        assert_eq!(title, name);
        let waiting = trf("{agent} is waiting for you", &[("agent", &"Claude Code")]);
        assert_eq!(below, Some(format!("{waiting} · TorroWorkplace · feat/inbox")));

        // Without a name, or with a blank one: the title and the message, as before.
        for title in [None, Some(" \u{202E} ".to_string())] {
            let mut unnamed = named.clone();
            if let Some(agent) = unnamed.agent.as_mut() {
                agent.title = title.clone();
            }
            assert_eq!(words(&unnamed), (waiting.clone(), preview(&named)), "{title:?}");
        }
        // No project or branch: only the agent's state.
        let mut bare = named.clone();
        if let Some(agent) = bare.agent.as_mut() {
            (agent.project, agent.branch) = (None, None);
        }
        assert_eq!(agent_line(&bare), waiting);
        // Entries of other sources are untouched.
        let notice = demo::sample(now()).into_iter().find(|i| i.id == "demo-6").expect("a notice");
        assert_eq!(words(&notice), (one_line(&notice.title), preview(&notice)));
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

    /// A mail entry from `name` <`address`> in the account `account` (an address), linking its mail.
    fn mail(name: &str, address: &str, account: &str) -> Item {
        let mut item = demo::sample(now()).remove(5);
        item.source = SourceRef::mail(account);
        item.from = Some(torro_inbox::item::Sender { name: name.into(), address: Some(address.into()) });
        item.link = Some(Link::Mail { message: "m:w1".into() });
        item
    }

    /// The folders of the demo's two accounts, every role among them.
    fn demo_folders() -> Vec<Folder> {
        demo::DemoSource::new(now()).mail_folders().expect("folders")
    }

    #[test]
    fn row_spans_count_the_group_heads_and_each_rows_height() {
        let mut items: Vec<Item> = demo::sample(now()).into_iter().filter(|i| i.state.is_open()).collect();
        items.sort_by_key(|item| !is_waiting(item));
        items.push(mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org"));
        let waiting = items.iter().filter(|item| is_waiting(item)).count();
        let head = group_head() - head_trim();
        let (notice, message) = (size::row_notice(), size::row_message());
        // The row asked for is the cursor's: its strip with the actions hangs below it.
        let strip = strip_height();
        assert_eq!(row_height_of(&items[0]), notice);
        assert_eq!(row_height_of(&items[items.len() - 1]), message, "mail has three lines");
        assert_eq!(
            row_span(&items, 0, Look::Rows, 480.0),
            Some((0.0, head + notice + strip)),
            "the first row shows its head"
        );
        assert_eq!(row_span(&items, 1, Look::Rows, 480.0), Some((head + notice, head + 2.0 * notice + strip)));
        let other = head + waiting as f32 * notice;
        assert_eq!(
            row_span(&items, waiting, Look::Rows, 480.0),
            Some((other, other + head + notice + strip)),
            "no double line"
        );
        let last = items.len() - 1;
        let top = 2.0 * head + last as f32 * notice;
        assert_eq!(row_span(&items, last, Look::Rows, 480.0), Some((top, top + message + strip)));
        assert_eq!(row_span(&items, items.len(), Look::Rows, 480.0), None);
        let calm = &items[waiting..];
        assert_eq!(row_span(calm, 1, Look::Rows, 480.0), Some((notice, 2.0 * notice + strip)), "no heads: plain rows");
        assert_eq!(message_height(), message, "a folder's rows are the mail rows (task E3.11)");
    }

    #[test]
    fn the_action_bar_sits_on_the_first_line() {
        let line = typo::line_height(typo::body());
        let mail = first_line_middle(size::row_message(), 3);
        assert_eq!(mail, (size::row_message() - size::HAIRLINE - 3.0 * line) / 2.0 + line / 2.0);
        let bar = widgets::row_actions_height();
        assert!(mail + bar / 2.0 < size::row_message() - line, "the preview's line stays free");
        assert!(mail - bar / 2.0 >= 0.0, "inside the row");
        let one = first_line_middle(size::row_notice(), 1);
        assert_eq!(one, (size::row_notice() - size::HAIRLINE) / 2.0, "a single line: the middle");
    }

    #[test]
    fn avatars_show_two_initials_at_most() {
        assert_eq!(initials("Jonas Weber"), "JW");
        assert_eq!(initials("IT Service Desk"), "ID");
        assert_eq!(initials("GitHub"), "G");
        assert_eq!(initials("lena.hoffmann@work.example.org"), "LH", "an address: its local part");
        assert_eq!(initials("  ( \u{202E} "), "?");
        assert_eq!(initials("éva  ömer"), "ÉÖ");
    }

    #[test]
    fn a_mail_row_names_the_sender_and_the_domain() {
        let item = mail("Jonas Weber", "jonas.weber@Work.Example.org", "anna@work.example.org");
        assert_eq!(sender(&item), ("Jonas Weber".to_owned(), Some("work.example.org".to_owned())));
        let mut bare = item.clone();
        bare.from = Some(torro_inbox::item::Sender {
            name: "news@shop.example".into(),
            address: Some("news@shop.example".into()),
        });
        assert_eq!(sender(&bare), ("news@shop.example".to_owned(), None), "no domain beside the address");
        bare.from = None;
        assert_eq!(sender(&bare).0, bare.source.label(), "without a sender: the account");
        let accounts = ["anna@work.example.org", "anna@example.org"];
        assert_eq!(account_index(&item, &accounts), Some(0));
        assert_eq!(account_index(&mail("A", "a@b.c", "anna@example.org"), &accounts), Some(1));
        assert_eq!(account_index(&mail("A", "a@b.c", "other@x.org"), &accounts), None, "not listed: neutral");
        assert_eq!(account_index(&demo::sample(now()).remove(0), &accounts), None, "no mail");
        assert_eq!(account_label(&item.source), "work.example.org");
    }

    #[test]
    fn reading_mail_keeps_the_row_geometry_and_changes_both_text_levels() {
        let view = View::ForMe;
        let rows = rows_on(&view, "demo-1", true);
        let mut item = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let unread = MailLines::of(&item, &rows).new;
        let height = row_height_of(&item);
        let left = mail_row_left(false);
        item.state = State::Seen;
        let read = MailLines::of(&item, &rows).new;
        assert!(unread && !read);
        assert_eq!(row_height_of(&item), height);
        assert_eq!(mail_row_left(false), left);
        assert_eq!(mail_text_level(unread), typo::body_strong());
        assert_eq!(mail_text_level(read), typo::body());
        assert_eq!(mail_row_left(true) + SELECTED_BAR, left, "selection does not move the content");
    }

    #[test]
    fn sender_cell_reserves_the_strong_width_for_read_mail() {
        let name = "Jonas Weber";
        let (shown, width) = sender_cell(name, 180.0);
        assert_eq!(shown, name);
        assert_eq!(width, typo::text_width(name, typo::body_strong()));
        let (short, clipped_width) = sender_cell(name, width / 2.0);
        assert!(short.ends_with('…'));
        assert!(clipped_width <= width / 2.0);
    }

    #[test]
    fn read_mail_uses_regular_weight_with_the_unread_metrics() {
        let strong = typo::body_strong();
        let mut regular = typo::body();
        regular.size = iced::Pixels(strong.size.0 + 5.0);
        regular.line_height = iced::widget::text::LineHeight::Absolute(iced::Pixels(36.0));
        regular.font.family = iced::font::Family::Serif;
        assert_ne!(regular.font.family, strong.font.family);
        let read = mail_text_level_in(false, strong, regular);
        assert_eq!(read.size, strong.size);
        assert_eq!(read.line_height, strong.line_height);
        assert_eq!(read.font.weight, regular.font.weight);
        assert_eq!(read.font.family, strong.font.family);
        assert_eq!(mail_text_level_in(true, strong, regular), strong);
    }

    fn labels(actions: &[RowAction<'_, Message>]) -> Vec<String> {
        actions.iter().map(|action| action.label.to_string()).collect()
    }

    #[test]
    fn the_action_bar_offers_what_works_for_the_entry() {
        let by_id = |id: &str| demo::sample(now()).into_iter().find(|i| i.id == id).expect("in the sample");
        let all = [tr("Task"), tr("Done"), tr("Later"), tr("Hide"), tr("Dismiss")];
        assert_eq!(labels(&actions(&by_id("demo-1"), true)), all, "a waiting agent");
        assert!(actions(&by_id("demo-3"), false).is_empty(), "an approval: no task, no done, no later, no hide");
        assert_eq!(labels(&actions(&by_id("demo-5"), true)), all, "the suite's account problem: its own done");
        assert_eq!(labels(&actions(&by_id("demo-11"), true)), [tr("Task"), tr("Reopen"), tr("Hide")], "done: no later");
        let keys: Vec<&str> = actions(&by_id("demo-1"), true).iter().map(|action| action.key).collect();
        assert_eq!(keys, ["T", "E", "S", "M", "X"], "the keys of the list");
        let sent = actions(&by_id("demo-1"), true).pop().map(|action| action.on_press);
        assert!(matches!(&sent, Some(Message::Act(id, Command::Dismiss)) if id == "demo-1"), "{sent:?}");
        let mail = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let sent: Vec<Message> = actions(&mail, true).into_iter().map(|action| action.on_press).collect();
        assert!(matches!(&sent[0], Message::Act(id, Command::MakeTask) if *id == mail.id));
        assert!(matches!(&sent[2], Message::Act(_, Command::Hide)));
        // Task E18.5: mail is archived or hidden, never done nor dismissed – Archive stands first in
        // `entry_actions`; a done mail entry still opens again ("Done today", decision 9 of E18).
        assert_eq!(labels(&actions(&mail, true)), [tr("Task"), tr("Later"), tr("Mute…")]);
        let done = Item { state: State::Done { at: now() }, ..mail.clone() };
        assert_eq!(labels(&actions(&done, true)), [tr("Task"), tr("Reopen"), tr("Mute…")]);
        assert_eq!(entry_mail(&mail), Some("m:w1"));
        assert_eq!(entry_mail(&done), None, "done: its mail may be gone");
        assert_eq!(entry_mail(&by_id("demo-1")), None, "no mail entry");
    }

    #[test]
    fn every_row_builds_in_any_width() {
        let view = View::ForMe;
        let mut items = demo::sample(now());
        items.push(mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org"));
        items.push(Item { count: 3, state: State::Seen, ..mail("", "x@y.z", "anna@example.org") });
        let accounts = [vec!["anna@work.example.org"], vec!["anna@work.example.org", "anna@example.org"]];
        for (accounts, bare) in accounts.into_iter().flat_map(|accounts| [(accounts.clone(), false), (accounts, true)])
        {
            let rows = Rows {
                selected_mail: &[],
                categories: &[],
                view: &view,
                selected: Some("demo-1"),
                now: now(),
                anchors: detail::Anchors::default(),
                accounts,
                hidable: vec!["demo-1"],
                look: Look::Rows,
                folders: &[],
                bare,
            };
            for width in [0.0, 240.0, 480.0, 720.0] {
                for item in &items {
                    let _ = entry_row(item, &rows, width);
                }
            }
        }
    }

    /// `rows` of the list `view` with the cursor on `selected`, bare or not ([`Rows::bare`]).
    fn rows_on<'a>(view: &'a View, selected: &'a str, bare: bool) -> Rows<'a> {
        Rows {
            selected_mail: &[],
            categories: &[],
            view,
            selected: Some(selected),
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: vec!["anna@work.example.org"],
            hidable: vec![selected],
            look: Look::Rows,
            folders: &[],
            bare,
        }
    }

    /// Task E14.2: in the mail window (bare rows) the selected row has no strip – it shows its
    /// preview like any other row and is as high as unchosen –, for a mail entry, an invitation and a
    /// notice alike; the hub's selected row keeps its strip below. Scrolling to the cursor counts
    /// the same heights.
    #[test]
    fn the_mail_windows_selected_row_has_no_buttons_and_its_height() {
        let view = View::ForMe;
        let entry = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let notice = demo::sample(now()).remove(0);
        for item in [entry.clone(), invitation(), notice] {
            let bare = rows_on(&view, &item.id, true);
            assert!(row_actions(&item, &bare).is_empty(), "{}: no buttons in the mail window", item.id);
            assert_eq!(entry_row_height(&item, &bare), row_height_of(&item), "{}: as high as unchosen", item.id);
            let hub = rows_on(&view, &item.id, false);
            assert!(!row_actions(&item, &hub).is_empty(), "{}: the hub keeps its strip", item.id);
            assert_eq!(entry_row_height(&item, &hub), row_height_of(&item) + strip_height());
            let unchosen = rows_on(&view, "someone-else", false);
            assert_eq!(entry_row_height(&item, &unchosen), row_height_of(&item));
            for width in [0.0, 240.0, 480.0, 720.0] {
                let _ = entry_row(&item, &bare, width);
            }
        }
        assert_eq!(strip_height_in(true), 0.0);
        assert_eq!(strip_height_in(false), strip_height(), "the hub's strip");
        let items = vec![entry.clone(), mail("Lena Hoffmann", "lena@work.example.org", "anna@work.example.org")];
        let message = size::row_message();
        assert_eq!(row_span_in(&items, 1, Look::Rows, 480.0, true), Some((message, 2.0 * message)));
        assert_eq!(row_span_in(&items, 1, Look::Rows, 480.0, false), row_span(&items, 1, Look::Rows, 480.0));
        assert_eq!(row_span(&items, 1, Look::Rows, 480.0), Some((message, 2.0 * message + strip_height())));
    }

    #[test]
    fn every_source_kind_has_its_icon() {
        assert_eq!(source_icon(&SourceRef::suite()), Icon::LayoutGrid);
        assert_eq!(source_icon(&SourceRef::agent("codex")), Icon::Sparkles);
        assert_eq!(source_icon(&SourceRef::script("backup")), Icon::AppWindow);
        // Task E5.15: only the kind decides – a script may not borrow GitHub's face.
        assert_eq!(source_icon(&SourceRef::script("github")), Icon::AppWindow, "a script named github");
        assert_eq!(source_icon(&SourceRef::mail("anna@example.org")), Icon::Mail);
        assert_eq!(source_icon(&SourceRef::github("anna-berg")), Icon::Github);
        assert_eq!(source_icon(&SourceRef::nextcloud("anna@cloud.example.org")), Icon::Cloud);
        assert_eq!(source_icon(&SourceRef::talk("anna@cloud.example.org")), Icon::MessageCircle);
        assert_eq!(source_icon(&SourceRef::ntfy("ntfy.sh")), Icon::RadioTower);
        let sample = demo::sample(now());
        let of = |id: &str| sample.iter().find(|item| item.id == id).map(|item| source_icon(&item.source));
        assert_eq!(of("demo-13"), Some(Icon::Github), "from a real GitHub source now");
        assert_eq!(
            [of("demo-16"), of("demo-17"), of("demo-18")],
            [Icon::Cloud, Icon::MessageCircle, Icon::RadioTower].map(Some)
        );
    }

    /// Task E5.15: the account joins the name only where two accounts of one service are listed.
    #[test]
    fn two_accounts_of_a_service_are_named_by_their_account() {
        let (anna, work) = (SourceRef::github("anna-berg"), SourceRef::github("anna-work"));
        let cloud = SourceRef::nextcloud("anna@cloud.example.org");
        let talk = SourceRef::talk("anna@cloud.example.org");
        let one = [anna.clone(), cloud.clone(), talk.clone()];
        assert_eq!(source_name(&anna, &one), "GitHub");
        assert_eq!(source_name(&talk, &one), "Talk", "Talk and Nextcloud are two services");
        let two = [anna.clone(), work.clone(), cloud.clone()];
        assert_eq!(source_name(&anna, &two), "GitHub · anna-berg");
        assert_eq!(source_name(&work, &two), "GitHub · anna-work");
        assert_eq!(source_name(&cloud, &two), "Nextcloud");
        let scripts = [SourceRef::script("ci"), SourceRef::script("deploy")];
        assert_eq!(source_name(&scripts[0], &scripts), "ci", "scripts say their names anyway");
    }

    /// Task E5.15: the meta words come from the traits, in the suite's words; foreign values as they are.
    #[test]
    fn the_meta_line_says_what_the_traits_say() {
        let sample = demo::sample(now());
        let of = |id: &str| sample.iter().find(|item| item.id == id).expect("in the demo");
        let review = format!("torro/workplace · {}", tr("Review requested"));
        assert_eq!(facet_line(of("demo-14")).as_deref(), Some(review.as_str()));
        let run = format!("torro/workplace · {}", tr("Check failed"));
        assert_eq!(facet_line(of("demo-15")).as_deref(), Some(run.as_str()));
        let shared = format!("{} · Jonas Weber", tr("Files"));
        assert_eq!(facet_line(of("demo-16")).as_deref(), Some(shared.as_str()));
        assert_eq!(facet_line(of("demo-17")).as_deref(), Some(tr("Direct message")));
        let alert = format!("Server · {}", trf("Priority {n}", &[("n", &5)]));
        assert_eq!(facet_line(of("demo-18")).as_deref(), Some(alert.as_str()));
        assert_eq!(facet_line(of("demo-6")), None, "a script has no such traits");
        // GitHub sends no text: its row says what the traits say instead.
        assert_eq!(words(of("demo-14")).1, Some(review));
        assert_eq!(words(of("demo-17")).1.as_deref(), Some("Lena: Can we go through the sidebar drafts before 3?"));
        let mut quiet = of("demo-18").clone();
        quiet.facets.retain(|facet| facet.key != FacetKey::Topic);
        quiet.facets.iter_mut().filter(|f| f.key == FacetKey::Priority).for_each(|f| f.value = "3".into());
        assert_eq!(facet_line(&quiet), None, "ntfy's default priority says nothing");
        assert_eq!(waiting_word(of("demo-18")), tr("Urgent"));
        assert_eq!(waiting_word(of("demo-1")), tr("Waiting for you"));
    }

    #[test]
    fn the_empty_for_me_view_says_nothing_is_there() {
        assert_eq!(empty_text(&View::ForMe).0, tr("Nothing for you right now."));
        for view in [View::Info, View::Snoozed, View::Done, View::Source(SourceRef::suite())] {
            assert_ne!(empty_text(&view).0, empty_text(&View::ForMe).0, "{view:?} has its own text");
        }
    }

    /// Task E8.3: a card's preview takes two lines at most and says with "…" that there is more.
    #[test]
    fn a_preview_is_cut_after_two_lines() {
        let level = typo::body();
        let long = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
        let width = typo::text_width("one two three four", level) + 1.0;
        let lines = clamp_lines(long, level, width, 2);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "one two three four");
        assert!(lines[1].ends_with('…'), "{lines:?}");
        assert!(lines.iter().all(|line| typo::text_width(line, level) <= width + 0.5), "every line fits");
        assert_eq!(clamp_lines("short", level, width, 2), ["short"]);
        assert!(clamp_lines("  ", level, width, 2).is_empty());
        let word = clamp_lines("Donaudampfschifffahrtsgesellschaft", level, 40.0, 2);
        assert!(word.len() == 1 && word[0].ends_with('…'), "a long word is cut: {word:?}");
    }

    /// Task E14.2: a mail card carries no buttons – it is as high as its padding and frame, its head,
    /// its subject and preview and, with chips, their row; no row of buttons under them.
    #[test]
    fn a_mail_card_has_no_buttons() {
        let pad = layout::panel_padding();
        let gap = size::card_gap();
        let head = size::avatar().max(typo::line_height(typo::body_strong()) + typo::line_height(typo::meta()));
        let line = typo::line_height(typo::body());
        let frame = pad.top + pad.bottom + 2.0 * size::HAIRLINE;
        assert_eq!(mail_card_height(0, false), frame + head + gap + line, "subject alone");
        assert_eq!(mail_card_height(1, false), frame + head + gap + line + space::xs() + line);
        assert_eq!(mail_card_height(0, true), frame + head + gap + line + gap + size::control_sm(), "chips");
    }

    /// Issue #118: Archive and Delete of a mail entry act on its mail, as `#` does at the entry – the
    /// same `Message::Mail` that the reader's buttons send –, in the hub's row; an account without an
    /// archive folder has no Archive; other entries have neither.
    #[test]
    fn a_mail_entry_files_its_mail_from_row_and_card() {
        let item = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let Some(Link::Mail { message }) = item.link.clone() else { panic!("a mail entry links its mail") };
        let folders = demo_folders();
        let row = entry_actions(&item, true, &folders, now());
        assert_eq!(labels(&row), [trc("action", "Archive"), tr("Delete"), tr("Task"), tr("Later"), tr("Mute…")]);
        assert!(row.iter().all(|action| action.label != tr("Done")), "task E18.5: mail is archived");
        assert_eq!((row[0].key, row[1].key), ("E", "#"), "Archive in the place of Done, with its key");
        assert!(matches!(&row[0].on_press, Message::Mail(MailAction::Archive, id) if *id == message));
        assert!(matches!(&row[1].on_press, Message::Mail(MailAction::Delete, id) if *id == message));
        let no_archive: Vec<Folder> = folders.iter().filter(|f| f.role != Some(Role::Archive)).cloned().collect();
        assert_eq!(labels(&filing(&item, &no_archive)), [tr("Delete")]);
        let by_id = |id: &str| demo::sample(now()).into_iter().find(|i| i.id == id).expect("in the sample");
        assert!(filing(&by_id("demo-1"), &folders).is_empty(), "no mail behind a waiting agent");
    }

    /// Task E8.3, E8.11 (finding 5): a card is as high as what it shows at its width – a preview of one
    /// line takes one line, a long one two.
    #[test]
    fn a_card_is_as_high_as_its_parts() {
        let width = 480.0;
        let line = typo::line_height(typo::body());
        let item = Item { body: Some("Short.".into()), ..mail("Jonas Weber", "jonas@work.example.org", "a@b.c") };
        let long = Item { body: Some("word ".repeat(80)), ..item.clone() };
        let bare = Item { body: None, ..item.clone() };
        assert_eq!(preview_lines(card_preview(&item).as_deref(), width), 1);
        assert_eq!(preview_lines(card_preview(&long).as_deref(), width), PREVIEW_LINES);
        assert_eq!(preview_lines(card_preview(&bare).as_deref(), width), 0);
        assert_eq!(entry_card_height(&long, width) - entry_card_height(&item, width), line, "no hole of a line");
        assert_eq!(entry_card_height(&item, width) - entry_card_height(&bare, width), space::xs() + line);
        let several = Item { count: 3, ..item.clone() };
        let row = size::card_gap() + size::control_sm();
        assert_eq!(entry_card_height(&several, width), entry_card_height(&item, width) + row, "a row of chips");
        assert_eq!(height_of(&item, Look::Rows, width), size::row_message());
        assert_eq!(height_of(&item, Look::Cards, width), entry_card_height(&item, width));
        let notice = demo::sample(now()).remove(0);
        assert_eq!(height_of(&notice, Look::Cards, width), size::row_notice(), "no mail: a row");
        let items = vec![item.clone(), several.clone()];
        let (top, bottom) = row_span(&items, 1, Look::Cards, width).expect("the second card");
        assert_eq!(top, cards_inset().top + entry_card_height(&item, width) + size::card_gap());
        assert_eq!(bottom - top, entry_card_height(&several, width));
    }

    /// Task E8.11 (finding 10): a row names the account only when it is not the sender's domain.
    #[test]
    fn the_account_is_named_only_when_it_says_more_than_the_domain() {
        let work = Some("work.example.org".to_owned());
        assert_eq!(account_shown(work.clone(), Some("work.example.org")), None);
        assert_eq!(account_shown(work.clone(), Some("example.org")), work);
        assert_eq!(account_shown(work.clone(), None), work);
        assert_eq!(account_shown(None, Some("example.org")), None);
    }

    #[test]
    fn every_card_builds_in_any_width() {
        let view = View::Overview;
        let mut items = demo::sample(now());
        items.push(mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org"));
        items.push(Item { count: 3, state: State::Seen, ..mail("", "x@y.z", "anna@example.org") });
        let rows = Rows {
            selected_mail: &[],
            categories: &[],
            view: &view,
            selected: Some("demo-1"),
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: vec!["anna@work.example.org", "anna@example.org"],
            hidable: vec!["demo-1"],
            look: Look::Cards,
            folders: &[],
            bare: true,
        };
        for width in [0.0, 240.0, 480.0, 720.0] {
            for item in &items {
                let _ = entry_card(item, &rows, width);
            }
        }
        let _ = view_of(&items, rows);
    }

    /// A mail entry whose mail `m:invite` invites to an event in a day, an hour long.
    fn invitation() -> Item {
        let invite = torro_mail::Invite {
            title: "Planning".into(),
            start: now() + TimeDelta::days(1),
            end: Some(now() + TimeDelta::days(1) + TimeDelta::hours(1)),
            all_day: false,
            answered: None,
        };
        let item = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        Item { link: Some(Link::Mail { message: "m:invite".into() }), invite: Some(invite), ..item }
    }

    /// Task E8.3c: an invitation's row in the hub offers the answers – Accept · Maybe · Decline, each
    /// for the mail it links to – in place of "Done", its card and row show when it is; once the event
    /// is over, or the entry is done, it is a mail as any.
    #[test]
    fn an_invitation_in_for_me_offers_its_answers() {
        let item = invitation();
        let answers = [tr("Accept"), tr("Maybe"), tr("Decline")];
        let buttons = entry_actions(&item, false, &demo_folders(), now());
        let sent: Vec<Message> = buttons.iter().take(3).map(|button| button.on_press.clone()).collect();
        let account = SourceRef::mail("anna@work.example.org").key();
        let Message::AnswerInvitation(to, about, InvitationAnswer::Accept) = &sent[0] else { panic!("{:?}", sent[0]) };
        assert_eq!((to, about.as_str()), (&account, "m:invite"));
        assert!(matches!(&sent[2], Message::AnswerInvitation(_, _, InvitationAnswer::Decline)));
        let row = labels(&entry_actions(&item, false, &demo_folders(), now()));
        assert_eq!(row, [&answers[..], &[tr("Task"), tr("Later")]].concat());
        let bare = Item { invite: None, ..item.clone() };
        assert!(entry_card_height(&item, 480.0) > entry_card_height(&bare, 480.0), "the chips get a row of their own");

        let over = now() + TimeDelta::days(2);
        assert!(invite_actions(&item, over).is_empty(), "over: no answers");
        let filed = [labels(&filing(&item, &demo_folders())), labels(&actions(&item, false))].concat();
        assert_eq!(labels(&entry_actions(&item, false, &demo_folders(), over)), filed);
        let done = Item { state: State::Done { at: now() }, ..item.clone() };
        assert!(invite_actions(&done, now()).is_empty(), "done: answered or put aside");
        let unlinked = Item { link: None, ..item.clone() };
        assert!(invite_actions(&unlinked, now()).is_empty(), "no mail to answer");

        let view = View::Overview;
        for look in [Look::Rows, Look::Cards] {
            let rows = Rows {
                selected_mail: &[],
                categories: &[],
                view: &view,
                selected: Some(item.id.as_str()),
                now: now(),
                anchors: detail::Anchors::default(),
                accounts: vec!["anna@work.example.org"],
                hidable: Vec::new(),
                look,
                folders: &[],
                bare: look.cards(),
            };
            for width in [0.0, 240.0, 360.0, 480.0, 720.0] {
                let _ = entry_card(&item, &rows, width);
                let _ = entry_row(&item, &rows, width);
            }
        }
    }

    /// A built-in design (issue #291) from its embedded file: the process draws with the standard, and
    /// the tests of this binary share it, so the others are read, never switched to.
    fn built_in(id: &str) -> torro_core::design::Design {
        torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design")
    }

    /// Issue #291 (task L.35): how a row shows its selection and its line follows the design – a bar
    /// and hairlines in Standard and Compact, the whole row in the accent without lines in Console –,
    /// and only the chosen row is marked.
    #[test]
    fn a_row_shows_its_selection_as_the_design_says() {
        assert_eq!(layout::list(), built_in("torro").list, "the process draws with the standard");
        for id in ["torro", "compact"] {
            let look = built_in(id).list;
            assert_eq!(RowLook::of(look, true), RowLook { bar: true, fill: false, line: true }, "{id}");
            assert_eq!(RowLook::of(look, false), RowLook { bar: false, fill: false, line: true }, "{id}");
        }
        let console = built_in("console").list;
        assert_eq!(RowLook::of(console, true), RowLook { bar: false, fill: true, line: false });
        assert_eq!(RowLook::of(console, false), RowLook { bar: false, fill: false, line: false });
        // The bar takes its width from the marker slot: the dot and the text stay where they are.
        assert_eq!(mail_row_left(true) + SELECTED_BAR, mail_row_left(false));
    }

    /// Issue #291 (task L.35): in the chosen row drawn in the accent every text and every coloured mark
    /// is `on_accent` – the cells are drawn in `theme::on_accent_theme` –, elsewhere a mark keeps its
    /// colour; in light and dark mode.
    #[test]
    fn a_chosen_row_in_the_accent_draws_text_and_marks_in_on_accent() {
        for window in [theme::theme(), theme::light()] {
            let p = palette(&window);
            let on = theme::on_accent_theme(&window);
            assert_eq!(mark_colour(&on, p.unread), p.on_accent, "the unread dot");
            assert_eq!(mark_colour(&on, p.attention), p.on_accent, "a flag, a waiting notice");
            assert_eq!(mark_colour(&on, p.category[1]), p.on_accent, "a mail's category");
            let text = palette(&on);
            assert_eq!([text.text, text.text_2, text.text_3], [p.on_accent; 3], "sender, subject, time");
            assert_eq!(mark_colour(&window, p.unread), p.unread, "elsewhere the mark keeps its colour");
            assert!(p.on_accent.relative_contrast(p.accent) >= 4.5, "readable on the accent");
        }
    }

    /// Issue #291 (review of PR #297): a notice's tile in the chosen row in the accent drops its `raised`
    /// surface, so its icon in `on_accent` stands on the accent; its frame turns `on_accent` too.
    /// Elsewhere the tile keeps `raised` and its `line` frame.
    #[test]
    fn a_tile_in_a_chosen_row_in_the_accent_lets_its_icon_stand_on_the_accent() {
        for window in [theme::theme(), theme::light()] {
            let p = palette(&window);
            let chosen = tile_style(&theme::on_accent_theme(&window));
            assert_eq!(chosen.background, None, "no raised surface under an on_accent icon");
            assert_eq!(chosen.border.color, p.on_accent);
            let elsewhere = tile_style(&window);
            assert_eq!(elsewhere.background, Some(Background::Color(p.raised)));
            assert_eq!(elsewhere.border.color, p.line);
        }
    }

    /// Issue #291 (task L.35): a mail row's avatar is the design's (`[list] avatar`); without one its
    /// place goes to the text – the row's words get the avatar's width and its gap –, a notice keeps
    /// its tile, and the reader's head follows the rows (`mail::head_avatar_in`).
    #[test]
    fn without_an_avatar_the_text_starts_at_the_rows_edge() {
        let standard = layout::list();
        assert_eq!((lead_in(standard, true), lead_in(standard, false)), (40.0, 40.0), "the standard's avatar");
        assert_eq!(lead_in(built_in("compact").list, true), 28.0, "Compact's");
        let none = ListLook { avatar: 0.0, ..standard };
        assert_eq!(lead_in(none, true), 0.0, "no avatar, no lead");
        assert_eq!(lead_room_in(none, true), 0.0, "and no gap after it");
        assert_eq!(lead_in(none, false), tile(), "a notice keeps its tile");
        for width in [480.0, 720.0] {
            let gained = mail_words_room_in(none, width) - mail_words_room_in(standard, width);
            assert_eq!(gained, 40.0 + space::m(), "at {width}: the avatar's place goes to the text");
        }
        assert_eq!(strip_indent_in(none, true), 0.0, "the strip starts where the text does");
        assert_eq!(built_in("console").list.avatar, 0.0, "Console draws none");
    }

    /// Issue #291 (task L.35): row heights and the cursor's spans per list style – rows of three lines
    /// (the preview on `preview_lines`) in Standard and Compact, one line of `row_message` (24) for mail
    /// and notices in Console, under the same heads, bare or with the hub's strip.
    #[test]
    fn rows_and_lines_have_their_heights_and_spans() {
        let notice = demo::sample(now()).remove(0);
        let entry = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let (standard, console) = (built_in("torro"), built_in("console"));
        assert_eq!(line_count_in(standard.list, &entry), 3, "sender, subject, one line of preview");
        assert_eq!(line_count_in(ListLook { preview_lines: 2, ..standard.list }, &entry), 4, "two of preview");
        assert_eq!(line_count_in(standard.list, &notice), 2, "a notice with a text");
        assert_eq!(line_count_in(console.list, &entry), 1, "a mail on one line");
        assert_eq!(line_count_in(console.list, &notice), 1, "a notice on one line");
        assert_eq!(line_count(&entry), 3, "the process draws the standard");

        // Console: every row one line of 24, which holds the line of text (18).
        let (message, notice_height) = (console.size.row_message, console.size.row_notice);
        assert_eq!((message, notice_height), (24.0, 24.0));
        assert!(console.size.row_message >= console.typo.body.line, "the line fits");
        let mut items: Vec<Item> = demo::sample(now()).into_iter().filter(|i| i.state.is_open()).collect();
        items.sort_by_key(|item| !is_waiting(item));
        items.push(entry.clone());
        let waiting = items.iter().filter(|item| is_waiting(item)).count();
        let height = |item: &Item| if item.source.kind == SourceKind::Mail { message } else { notice_height };
        let head = console.size.row_group - size::HAIRLINE;
        let last = items.len() - 1;
        let top = 2.0 * head + last as f32 * 24.0;
        assert_eq!(rows_span(&items, last, height, head, 0.0), Some((top, top + 24.0)), "a bare line");
        assert_eq!(rows_span(&items, 0, height, head, 0.0), Some((0.0, head + 24.0)), "the first shows its head");
        let other = head + waiting as f32 * 24.0;
        assert_eq!(rows_span(&items, waiting, height, head, 7.0), Some((other, other + head + 24.0 + 7.0)), "strip");
        assert_eq!(rows_span(&items, items.len(), height, head, 0.0), None);
        // The standard's rows through the same arithmetic as the list's own spans.
        let standard_height = |item: &Item| row_height_of(item);
        assert_eq!(
            rows_span(&items, last, standard_height, group_head() - head_trim(), 0.0),
            row_span_in(&items, last, Look::Rows, 480.0, true)
        );
    }

    /// Issue #291 (decided after the review of the three designs, replaces the columns of L.35): a line
    /// has the order of the rows – marks · who · title with the preview · time –, its cells side by
    /// side with the title column the widest and the time ending at the right edge of the cells.
    #[test]
    fn a_line_has_the_order_of_the_rows_and_the_time_at_the_right() {
        for width in [480.0, 720.0, 1200.0] {
            let columns = LineColumns::of(width, line_clock_width());
            let [marks, who, title, time] = columns.starts();
            assert!(marks < who && who < title && title < time, "{width}: marks, who, title, time");
            assert_eq!(who, columns.marks + cell_gap(), "{width}: who after the marks");
            assert_eq!(time + columns.time, line_room(width), "{width}: the time ends at the right");
            assert!(columns.title > columns.who, "{width}: the title column takes the most");
        }
        assert!(line_marks_width() >= LINE_MARKS as f32 * icon::SIZE_SM, "every mark of a mail");
        assert!(line_marks_width() >= tile(), "a notice's tile");
        let yesterday = trf("Yesterday {time}", &[("time", &"23:59")]);
        assert!(line_date_width() >= typo::text_width(&yesterday, typo::meta().tabular()), "a date in full");
    }

    /// Issue #291 (review): mail and notices share the title column, so a mixed list (the hub) has one
    /// grid – the columns do not follow the row's time, and a notice's cells start where a mail's do.
    #[test]
    fn mail_and_notices_share_the_title_column() {
        for width in [480.0, 720.0, 1200.0] {
            let clock = LineColumns::of(width, line_clock_width());
            let yesterday = LineColumns::of(width, line_date_width());
            assert_eq!(clock.starts()[..3], yesterday.starts()[..3], "{width}: the same columns, whatever the time");
        }
        assert!(mail_row_left(false) >= layout::row_padding().left, "the notice's lead fills the gap, never less");
        let view = View::ForMe;
        let rows = rows_on(&view, "someone", true);
        let notice = demo::sample(now()).remove(0);
        let entry = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let columns = LineColumns::of(720.0, line_clock_width());
        // Both take their words from the same columns: who and title stay in theirs.
        let lines = MailLines::of(&entry, &rows);
        let mail_words = mail_line_words(
            columns,
            (&lines.name, lines.domain.as_deref()),
            (&lines.subject, lines.new, None),
            &lines.preview,
            &None,
            &lines.time,
        );
        let notice_words = notice_line_words(&notice, &rows, columns);
        for shown in [&mail_words, &notice_words] {
            assert!(typo::text_width(&shown.who, typo::body_strong()) <= columns.who, "{shown:?}");
        }
    }

    /// Issue #291 (review): a narrow line gives each row's time its own width – no wide empty column
    /// beside a clock time –, at most the wide column; a wide line keeps the column of the widest time.
    #[test]
    fn a_narrow_line_gives_its_time_its_own_width() {
        assert!(line_narrow(480.0) && !line_narrow(720.0));
        let clock = line_clock_width();
        assert!(clock < line_date_width());
        assert_eq!(LineColumns::of(480.0, clock).time, clock, "narrow: as wide as \"23:59\"");
        assert_eq!(LineColumns::of(480.0, 1000.0).time, line_date_width(), "narrow: at most the wide column");
        assert_eq!(LineColumns::of(720.0, clock).time, line_date_width(), "wide: the column of the widest");
        assert!(LineColumns::of(480.0, clock).title > LineColumns::of(480.0, line_date_width()).title);
    }

    /// Issue #291 (review): a line shows what the rows show – sender with domain, subject, preview,
    /// account, time; a notice its source, title and text –, cut with "…" rather than left out.
    #[test]
    fn a_line_shows_what_the_row_shows_and_cuts_rather_than_drops() {
        let view = View::ForMe;
        let rows = rows_on(&view, "someone", true);
        let entry = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let lines = MailLines::of(&entry, &rows);
        let preview = Some("Can we go through the sidebar drafts before three? I have a few notes.".to_string());
        let account = Some("anna@home.example.org".to_string());
        let shown = |width: f32| {
            let columns = LineColumns::of(width, typo::text_width(&lines.time, typo::meta().tabular()));
            mail_line_words(
                columns,
                (&lines.name, lines.domain.as_deref()),
                (&lines.subject, lines.new, None),
                &preview,
                &account,
                &lines.time,
            )
        };
        let wide = shown(1600.0);
        assert_eq!((wide.who.as_str(), wide.domain.as_deref()), ("Jonas Weber", lines.domain.as_deref()));
        assert!(wide.domain.is_some(), "the domain, as in the rows");
        assert_eq!((wide.title.as_str(), wide.preview.as_ref()), (lines.subject.as_str(), preview.as_ref()));
        assert_eq!((wide.account.as_ref(), wide.time.as_str()), (account.as_ref(), lines.time.as_str()));
        let narrow = shown(480.0);
        let cut = narrow.preview.expect("the preview is cut, not left out");
        assert!(cut.ends_with('…') && cut.len() < preview.as_ref().map_or(0, String::len), "{cut}");

        let notice = demo::sample(now()).remove(0);
        let (title, text) = words(&notice);
        let columns = LineColumns::of(1600.0, line_date_width());
        let shown = notice_line_words(&notice, &rows, columns);
        assert_eq!(shown.who, notice.source.label(), "the source where a mail has its sender");
        assert_eq!((shown.title, shown.preview), (title, text), "title and text");
        assert_eq!(shown.time, time_text_in(&notice, &view, now(), false));
    }

    /// Issue #291: lines and rows build in every look and width, chosen or not.
    #[test]
    fn every_look_builds() {
        let view = View::ForMe;
        let notice = demo::sample(now()).remove(0);
        let entry = mail("Jonas Weber", "jonas@work.example.org", "anna@work.example.org");
        let looks = [
            layout::list(),
            built_in("compact").list,
            built_in("console").list,
            ListLook { avatar: 0.0, preview_lines: 2, ..layout::list() },
        ];
        for look in looks {
            for selected in [true, false] {
                let rows = rows_on(&view, if selected { "demo-1" } else { "someone" }, true);
                for width in [0.0, 240.0, 600.0] {
                    let lines =
                        || MailLines { marks: vec![(blank().into(), icon::SIZE_SM)], ..MailLines::of(&entry, &rows) };
                    let _ = mail_rows(look, lines(), width, selected);
                    let _ = mail_line(look, lines(), width, selected);
                    let _ = notice_rows(look, &notice, &rows, width);
                    let _ = notice_line(&notice, &rows, width);
                }
            }
        }
    }

    fn view_of<'a>(items: &'a [Item], rows: Rows<'a>) -> Element<'a, Message> {
        super::view(ListData { items, rows, attention: Color::BLACK })
    }
}
