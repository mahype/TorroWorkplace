//! Mini and strip: the inbox as a glance at a small tile (task E3.3a in `docs/tasks/eingang.md`;
//! design skill `.claude/skills/torro-design`, section 6; the pattern is `torrotasks/src/glance.rs`).
//!
//! - **Mini** ([`mini`]): at the top the numbers, each a big figure (`DISPLAY`) with its word beside
//!   it – "6 for you  3 ● waiting" (the second part only while something waits) – then as many
//!   open entries of "For me" as fit the height, at least the top [`SHOWN`], as rows: the source's icon, the title cut to fit, how long
//!   ago at the right. Nothing for the user: a calm "Nothing for you right now." under the inbox
//!   icon.
//! - **Strip** ([`strip`]): the same numbers at the left, the same entries beside them as cells in
//!   one row – icon and title, below it the source and how long ago – sharing the width, each at
//!   most [`CELL_MAX`] wide (a wide strip is not stretched).
//!
//! **Waiting entries** (urgent and open, `list::is_waiting`) lead, as in the list. They are marked
//! as in its group "Waiting for you": the source's icon in the attention colour – never Torro red –
//! and, so colour is not the only sign, the attention dot in front of the time, the same dot that
//! stands before the word "waiting" in the numbers.
//!
//! **Mail** (task E4.4): while a mail account has unread mail in its inbox, a small block shows it –
//! in the mini tile under the numbers, one row per account (address, unread number at the right)
//! and a quiet line "From Anna, Ben, Chris" with the newest senders of the mail entries in "For me"
//! ([`latest_senders`]); in the strip as one more cell before the entries, with the total, the
//! accounts and the senders. When there is no room for every account the block shrinks to the total
//! ([`mail_rows`]); the entries keep at least half of the rows. Senders are foreign text: drawn as
//! plain text on one line, cut. The block is a display, not a button: a click opens entries only.
//!
//! Both are displays, not small apps: no header, sidebar, menu, form or dialog, no keys. The one
//! thing a click does is open the entry: `Message::OpenForMe` puts it under the cursor and into the
//! panel of "For me", which the window shows once the tile is larger again (making the tile larger
//! is the tiling manager's job, as in torrocalendar's glance). The clicked row stays highlighted.
//!
//! The decisions – counts, what is shown, texts, how much fits – are plain functions with tests;
//! the views only lay out their results.

use chrono::{DateTime, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, svg};
use iced::{Alignment, Element, Fill, Padding, Size, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::{style, widgets};
use torro_inbox::{Item, SourceKind};
use torro_mail::{Folder, Role};

use crate::app::Message;
use crate::{list, mail};

/// The strip shows at most this many entries (`docs/eingang.md`: "die drei obersten"); the mini
/// tile shows as many as fit its height, like torrotasks fills a tall tile. The numbers at the
/// top say how many there are in all.
pub const SHOWN: usize = 3;
/// The height of a row of the mini tile: a sidebar row, so a row is a 28-high click target.
const ROW: f32 = size::ROW_SIDEBAR;
/// The levels of the mini's figures, largest first: the first that fits is used.
const FIGURE_LEVELS: [typo::Level; 2] = [typo::DISPLAY.tabular(), typo::TITLE.tabular()];
/// The levels of the strip's first line and of the calm line, largest first.
const HEAD_LEVELS: [typo::Level; 2] = [typo::TITLE.tabular(), typo::BODY_STRONG.tabular()];
/// The gap between the mini's mail block and the entries below it.
const MAIL_GAP: f32 = space::XS;
/// The attention dot (`widgets::status_dot`) is this wide.
const DOT: f32 = space::S;
/// The numbers of the strip take at most this share of its width.
const STRIP_INFO_SHARE: f32 = 4.0;
/// A cell of the strip is never wider than this, so a wide strip keeps its entries together at
/// the left instead of spreading three words over the whole monitor.
pub const CELL_MAX: f32 = size::PROPS;
/// A cell of the strip is never narrower than this; with less room fewer entries are shown.
pub const CELL_MIN: f32 = size::SIDEBAR / 2.0;
/// The gap between the strip's cells.
const CELL_GAP: f32 = space::S;
/// Inside a row or cell: the button's own padding at the sides ([`style::ITEM_PADDING`]). The
/// tile's edge is that much closer, so text still stands 12 in from the edge, as in the sisters.
const SIDE: f32 = space::S;

/// What the glance gets to draw.
pub struct GlanceData<'a> {
    /// The open entries of "For me", waiting ones first (as `App::take` keeps them).
    pub items: &'a [Item],
    /// Id of the entry open in the panel (after a click here): its row is highlighted.
    pub selected: Option<&'a str>,
    /// The folders of the mail accounts (empty without mail), for the unread numbers.
    pub folders: &'a [Folder],
    /// "Now", for the times ("4 min").
    pub now: DateTime<Utc>,
    /// A later call that failed, as one quiet line (the glance has no retry button).
    pub error: Option<&'a str>,
}

/// How many of `items` wait for the user (`list::is_waiting`).
pub fn waiting(items: &[Item]) -> usize {
    items.iter().filter(|item| list::is_waiting(item)).count()
}

/// The parts of the numbers as `(text, waiting)`: "6 for you", then "3 waiting" while something
/// waits. Nothing open: none (the glance says [`calm`] instead).
pub fn count_parts(items: &[Item]) -> Vec<(String, bool)> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![(trf("{n} for you", &[("n", &items.len())]), false)];
    let waiting = waiting(items);
    if waiting > 0 {
        parts.push((trf("{n} waiting", &[("n", &waiting)]), true));
    }
    parts
}

/// The newest senders the mail block names.
pub const SENDERS: usize = 3;

/// The unread mail in the inbox of one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailUnread {
    /// The address of the account (`anna@example.org`).
    pub address: String,
    pub unread: u32,
}

/// The accounts with unread mail in their inbox, most unread first (accounts with as many in the
/// core's order). Accounts without unread mail, and folders other than the inbox, are left out.
pub fn mail_unread(folders: &[Folder]) -> Vec<MailUnread> {
    let mut unread: Vec<MailUnread> = mail::by_account(folders)
        .into_iter()
        .filter_map(|group| {
            let inbox = group.folders.into_iter().find(|folder| folder.role == Some(Role::Inbox))?;
            (inbox.unread > 0)
                .then(|| MailUnread { address: mail::address(group.account).to_owned(), unread: inbox.unread })
        })
        .collect();
    unread.sort_by_key(|account| std::cmp::Reverse(account.unread));
    unread
}

/// The names of the up to [`SENDERS`] newest different senders among the mail entries of `items`
/// (the open entries of "For me"): the name, else the address, on one line.
pub fn latest_senders(items: &[Item]) -> Vec<String> {
    let mut mails: Vec<&Item> = items.iter().filter(|item| item.source.kind == SourceKind::Mail).collect();
    mails.sort_by_key(|item| std::cmp::Reverse(item.updated));
    let mut senders: Vec<String> = Vec::new();
    for from in mails.iter().filter_map(|item| item.from.as_ref()) {
        let name = list::one_line(&from.name);
        let name = if name.is_empty() { list::one_line(from.address.as_deref().unwrap_or_default()) } else { name };
        if !name.is_empty() && !senders.contains(&name) {
            senders.push(name);
        }
        if senders.len() == SENDERS {
            break;
        }
    }
    senders
}

/// "4 unread".
pub fn unread_text(unread: u32) -> String {
    trf("{n} unread", &[("n", &unread)])
}

/// "From Anna, Ben, Chris".
pub fn senders_text(senders: &[String]) -> String {
    trf("From {senders}", &[("senders", &senders.join(", "))])
}

/// What the mini tile's mail block shows, in rows of [`ROW`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MailRows {
    /// One row per account (address and number); they all fit, else [`MailRows::total`] is set.
    pub accounts: usize,
    /// One row with the number of unread mail in all accounts, when the accounts do not all fit.
    pub total: bool,
    /// The line with the newest senders.
    pub senders: bool,
}

impl MailRows {
    /// How many rows of the tile the block takes.
    pub fn used(&self) -> usize {
        (if self.total { 1 } else { self.accounts }) + usize::from(self.senders)
    }
}

/// What the mail block shows when `rows` rows of the tile are free under the numbers: nothing
/// without unread mail; at most half of the rows (rounded up), so the entries keep theirs. The
/// senders line comes when there are senders and at least two rows for the block; every account
/// has a row if they all fit, else the block is the one row with the total.
pub fn mail_rows(unread: &[MailUnread], has_senders: bool, rows: usize) -> MailRows {
    let budget = rows.div_ceil(2);
    if unread.is_empty() || budget == 0 {
        return MailRows::default();
    }
    let senders = has_senders && budget >= 2;
    let room = budget - usize::from(senders);
    if unread.len() <= room {
        MailRows { accounts: unread.len(), total: false, senders }
    } else {
        MailRows { accounts: 0, total: true, senders }
    }
}

/// The calm line when nothing is open for the user: the sentence of the empty "For me" view.
pub fn calm() -> &'static str {
    tr("Nothing for you right now.")
}

/// The height of the head of the mini tile: the figures at the largest level.
fn figure_height() -> f32 {
    line_height(FIGURE_LEVELS[0])
}

/// The height of the quiet error line with its gap above.
fn error_height() -> f32 {
    space::XS + line_height(typo::META)
}

/// How many rows fit in a mini tile `height` high under the numbers (with room for the error line
/// if there is one).
pub fn rows_that_fit(height: f32, error: bool) -> usize {
    let above = 2.0 * space::M + figure_height() + space::S + if error { error_height() } else { 0.0 };
    ((height - above) / ROW).floor().max(0.0) as usize
}

/// How many cells fit beside the numbers in a strip with `avail` width, at most [`SHOWN`].
pub fn cells_that_fit(avail: f32) -> usize {
    slots_that_fit(avail, SHOWN)
}

/// How many cells of at least [`CELL_MIN`] fit in `avail` width, at most `most`.
fn slots_that_fit(avail: f32, most: usize) -> usize {
    (((avail + CELL_GAP) / (CELL_MIN + CELL_GAP)).floor().max(0.0) as usize).min(most)
}

/// The width of each of `count` cells in `avail`: an even share, at most [`CELL_MAX`].
pub fn cell_width(avail: f32, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    ((avail - CELL_GAP * (count - 1) as f32) / count as f32).clamp(0.0, CELL_MAX)
}

/// The first level of `levels` in which `text` fits `width`, else the last one.
fn fit_level(text: &str, levels: &[typo::Level], width: f32) -> typo::Level {
    levels.iter().copied().find(|level| text_width(text, *level) <= width).unwrap_or(levels[levels.len() - 1])
}

/// A part of the numbers split into its figure and its word: "6 for you" → ("6", "for you"). A
/// text without a space (a translation that does not start with the number) has no figure.
fn split_count(text: &str) -> (&str, &str) {
    text.split_once(' ').unwrap_or(("", text))
}

/// The width of a part in the mini's head at `level`: figure, gap, the dot of the waiting part
/// with its gap, the word in `META`.
fn part_width(text: &str, waiting: bool, level: typo::Level) -> f32 {
    let (figure, word) = split_count(text);
    let dot = if waiting { DOT + space::XS } else { 0.0 };
    text_width(figure, level) + space::XS + dot + text_width(word, typo::META)
}

/// The level of the mini's figures: the largest of [`FIGURE_LEVELS`] under which all parts with
/// their gaps fit `width`.
fn figure_level(parts: &[(String, bool)], width: f32) -> typo::Level {
    let total = |level: typo::Level| {
        let widths: f32 = parts.iter().map(|(text, waiting)| part_width(text, *waiting, level)).sum();
        widths + space::L * parts.len().saturating_sub(1) as f32
    };
    FIGURE_LEVELS.iter().copied().find(|level| total(*level) <= width).unwrap_or(FIGURE_LEVELS[FIGURE_LEVELS.len() - 1])
}

/// A line in `META` and `text_3`, never wrapped.
fn quiet(line: String) -> iced::widget::Text<'static> {
    typo::text(typo::META, line).style(theme::text_3).wrapping(Wrapping::None)
}

/// The error line: `META` in the error colour, cut to `width`.
fn error_line(error: &str, width: f32) -> Element<'static, Message> {
    typo::text(typo::META, ellipsize(error, typo::META, width))
        .wrapping(Wrapping::None)
        .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
        .into()
}

/// The source's icon of `item`: in the attention colour while it waits, else `text_3` (as in the
/// list).
fn source_mark(item: &Item) -> Element<'static, Message> {
    let waiting = list::is_waiting(item);
    icon::icon(list::source_icon(&item.source), icon::SIZE)
        .style(move |t: &Theme, _| {
            let p = palette(t);
            svg::Style { color: Some(if waiting { p.attention_text } else { p.text_3 }) }
        })
        .into()
}

/// How long ago the entry last changed ("4 min"), quiet and tabular; a waiting one has the
/// attention dot in front of it.
fn time_view(item: &Item, now: DateTime<Utc>) -> Element<'static, Message> {
    let time = typo::text(typo::META.tabular(), list::ago(item.updated, now)).style(theme::text_3);
    let mut line = row![].spacing(space::XS).align_y(Alignment::Center);
    if list::is_waiting(item) {
        line = line.push(widgets::status_dot(widgets::Status::Attention));
    }
    line.push(time.wrapping(Wrapping::None)).into()
}

/// The width of [`time_view`] for `item`.
fn time_width(item: &Item, now: DateTime<Utc>) -> f32 {
    let dot = if list::is_waiting(item) { DOT + space::XS } else { 0.0 };
    dot + text_width(&list::ago(item.updated, now), typo::META)
}

/// A clickable row or cell: `content` on a sidebar-row surface (hover, and `selected` while it is
/// the cursor's entry), padded `padding`; a click opens the entry (`Message::OpenForMe`).
fn entry_button(
    content: Element<'static, Message>,
    item: &Item,
    selected: bool,
    padding: Padding,
) -> Element<'static, Message> {
    button(content)
        .width(Fill)
        .padding(padding)
        .style(style::button::sidebar_item(selected))
        .on_press(Message::OpenForMe(item.id.clone()))
        .into()
}

/// The tile around a glance: the panel colour, `padding` in from the edge, clipped.
fn tile(content: impl Into<Element<'static, Message>>, padding: Padding) -> Element<'static, Message> {
    container(content).padding(padding).width(Fill).height(Fill).clip(true).style(style::container::panel_flush).into()
}

/// The calm line, in the largest level that fits `width`, cut when even the smallest does not.
fn calm_line(width: f32) -> Element<'static, Message> {
    let level = fit_level(calm(), &HEAD_LEVELS, width);
    typo::text(level, ellipsize(calm(), level, width)).style(theme::text_2).wrapping(Wrapping::None).into()
}

/// The empty tile: the inbox icon, quiet, over the calm line, centred – and the error line below
/// when a call failed. Nothing is open, so there is nothing to click.
fn nothing(width: f32, error: Option<&str>) -> Element<'static, Message> {
    let mark = icon::icon(Icon::Inbox, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let mut content = column![mark, calm_line(width)].spacing(space::S).align_x(Alignment::Center);
    if let Some(error) = error {
        content = content.push(error_line(error, width));
    }
    container(content).center(Fill).into()
}

/// The mini's numbers: per part the figure in the fitting level (`text`), then the word in `META`
/// (`text_2`); the waiting part has the attention dot in front of its word, so the colour never
/// carries the text and the word says it too.
fn figures_line(parts: &[(String, bool)], width: f32) -> Element<'static, Message> {
    let level = figure_level(parts, width);
    let mut line = row![].spacing(space::L).align_y(Alignment::Center);
    for (text, waiting) in parts {
        let (figure, word) = split_count(text);
        let mut part = row![typo::text(level, figure.to_owned()).style(theme::text).wrapping(Wrapping::None)]
            .spacing(space::XS)
            .align_y(Alignment::Center);
        if *waiting {
            part = part.push(widgets::status_dot(widgets::Status::Attention));
        }
        part = part.push(typo::text(typo::META, word.to_owned()).style(theme::text_2).wrapping(Wrapping::None));
        line = line.push(part);
    }
    container(line).height(figure_height()).center_y(figure_height()).into()
}

/// One entry of the mini tile, `width` wide inside its padding: icon, title cut to fit, the time
/// at the right.
fn row_view(item: &Item, data: &GlanceData<'_>, width: f32) -> Element<'static, Message> {
    let room = (width - icon::SIZE - space::S - space::S - time_width(item, data.now)).max(0.0);
    let title = list::one_line(&item.title);
    let title = typo::text(typo::BODY, ellipsize(&title, typo::BODY, room)).style(theme::text).wrapping(Wrapping::None);
    let line = row![source_mark(item), container(title).width(Fill).clip(true), time_view(item, data.now)]
        .spacing(space::S)
        .align_y(Alignment::Center);
    let content = container(line).height(ROW).center_y(ROW);
    let selected = data.selected == Some(item.id.as_str());
    entry_button(content.into(), item, selected, style::ITEM_PADDING)
}

/// The mail icon, quiet like the entries' source icons.
fn mail_mark() -> Element<'static, Message> {
    icon::icon(Icon::Mail, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) }).into()
}

/// A row of the mini's mail block: [`ROW`] high, its text where the entries' text stands.
fn mail_row(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    container(content).width(Fill).height(ROW).center_y(ROW).padding(style::ITEM_PADDING).into()
}

/// The row of one account: its address (cut), the unread number at the right.
fn account_row(account: &MailUnread, width: f32) -> Element<'static, Message> {
    let number = account.unread.to_string();
    let room = (width - icon::SIZE - 2.0 * space::S - text_width(&number, typo::BODY)).max(0.0);
    let address = typo::text(typo::BODY, ellipsize(&account.address, typo::BODY, room))
        .style(theme::text_2)
        .wrapping(Wrapping::None);
    let number = typo::text(typo::BODY.tabular(), number).style(theme::text).wrapping(Wrapping::None);
    mail_row(
        row![mail_mark(), container(address).width(Fill).clip(true), number]
            .spacing(space::S)
            .align_y(Alignment::Center),
    )
}

/// The row with the number of unread mail in all accounts.
fn total_row(unread: &[MailUnread], width: f32) -> Element<'static, Message> {
    let total: u32 = unread.iter().map(|account| account.unread).sum();
    let room = (width - icon::SIZE - space::S).max(0.0);
    let text = typo::text(typo::BODY, ellipsize(&unread_text(total), typo::BODY, room))
        .style(theme::text)
        .wrapping(Wrapping::None);
    mail_row(row![mail_mark(), text].spacing(space::S).align_y(Alignment::Center))
}

/// The line with the newest senders, under the accounts' text.
fn senders_row(senders: &[String], width: f32) -> Element<'static, Message> {
    let indent = icon::SIZE + space::S;
    let text = ellipsize(&senders_text(senders), typo::META, (width - indent).max(0.0));
    let text = typo::text(typo::META, text).style(theme::text_2).wrapping(Wrapping::None);
    mail_row(row![Space::new().width(indent), text].align_y(Alignment::Center))
}

/// The mini's mail block as `plan` decides ([`mail_rows`]), `width` wide inside its padding.
fn mail_block(plan: MailRows, unread: &[MailUnread], senders: &[String], width: f32) -> Element<'static, Message> {
    let mut block = column![];
    if plan.total {
        block = block.push(total_row(unread, width));
    }
    for account in unread.iter().take(if plan.total { 0 } else { plan.accounts }) {
        block = block.push(account_row(account, width));
    }
    if plan.senders {
        block = block.push(senders_row(senders, width));
    }
    // A raised group like the settings pages' groups, so the summary reads apart from the entries.
    container(block).width(Fill).style(style::container::group).into()
}

/// The mini tile for a window of `size`: the numbers at the top, the mail block while there is
/// unread mail, then the top entries that fit the height. Nothing open: the calm tile.
pub fn mini(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    // The rows' buttons reach to `SIDE` from the edge, their text to 12 like the head.
    let padding = Padding { top: space::M, bottom: space::M, left: space::M - SIDE, right: space::M - SIDE };
    let width = (size.width - 2.0 * space::M).max(0.0);
    if data.items.is_empty() {
        return tile(nothing(width, data.error), Padding::new(space::M));
    }
    let head = container(figures_line(&count_parts(data.items), width)).padding(Padding::ZERO.left(SIDE));
    let mut content = column![head];
    if let Some(error) = data.error {
        content = content
            .push(Space::new().height(space::XS))
            .push(container(error_line(error, width)).padding(Padding::ZERO.left(SIDE)));
    }
    content = content.push(Space::new().height(space::S));
    let free = rows_that_fit(size.height, data.error.is_some());
    let (unread, senders) = (mail_unread(data.folders), latest_senders(data.items));
    let plan = mail_rows(&unread, !senders.is_empty(), free);
    let shown = free - plan.used();
    if plan.used() > 0 {
        content = content.push(mail_block(plan, &unread, &senders, width));
        // The gap below the block only where it does not push the last entry out.
        if rows_that_fit(size.height - MAIL_GAP, data.error.is_some()) >= free {
            content = content.push(Space::new().height(MAIL_GAP));
        }
    }
    for item in data.items.iter().take(shown) {
        content = content.push(row_view(item, data, width));
    }
    tile(content, padding)
}

/// The strip's numbers, `width` wide: "6 for you" in the largest head level that fits, below it
/// the attention dot and "3 waiting" while something waits.
fn numbers_column(parts: &[(String, bool)], width: f32) -> Element<'static, Message> {
    let mut lines = column![].spacing(space::XS);
    for (text, waiting) in parts {
        if *waiting {
            // META like the cells' second line, so both lines of the numbers sit level with the
            // two lines of the cells beside them.
            let word = typo::text(typo::META, ellipsize(text, typo::META, width - DOT - space::XS))
                .style(theme::text_2)
                .wrapping(Wrapping::None);
            let line = row![widgets::status_dot(widgets::Status::Attention), word]
                .spacing(space::XS)
                .align_y(Alignment::Center);
            lines = lines.push(line);
        } else {
            let level = fit_level(text, &HEAD_LEVELS, width);
            lines = lines
                .push(typo::text(level, ellipsize(text, level, width)).style(theme::text).wrapping(Wrapping::None));
        }
    }
    lines.into()
}

/// The width the strip's numbers want: the wider of their lines.
fn numbers_width(parts: &[(String, bool)]) -> f32 {
    parts
        .iter()
        .map(
            |(text, waiting)| {
                if *waiting { DOT + space::XS + text_width(text, typo::META) } else { text_width(text, HEAD_LEVELS[0]) }
            },
        )
        .fold(0.0, f32::max)
}

/// One entry of the strip as a cell `width` wide: the icon beside the title (`BODY`), below the
/// title the source and how long ago in `META` (a waiting one with the dot in front).
fn cell(item: &Item, data: &GlanceData<'_>, width: f32) -> Element<'static, Message> {
    let room = (width - 2.0 * SIDE - icon::SIZE - space::S).max(0.0);
    let title = list::one_line(&item.title);
    let title = typo::text(typo::BODY, ellipsize(&title, typo::BODY, room)).style(theme::text).wrapping(Wrapping::None);
    // The separator stays whole; only the source's name is cut.
    let separator = " · ";
    let source_room = (room - time_width(item, data.now) - text_width(separator, typo::META)).max(0.0);
    let source = format!("{}{separator}", ellipsize(&item.source.label(), typo::META, source_room));
    let meta = row![quiet(source), time_view(item, data.now)].align_y(Alignment::Center);
    let words = column![title, meta].width(Fill).clip(true);
    // The icon stands on the title's line.
    let mark = container(source_mark(item)).center_y(line_height(typo::BODY));
    let content = row![mark, words].spacing(space::S);
    let selected = data.selected == Some(item.id.as_str());
    let padding = Padding { top: space::XS, bottom: space::XS, left: SIDE, right: SIDE };
    container(entry_button(content.into(), item, selected, padding)).width(width).into()
}

/// How the strip's `avail` width is shared: `(places, mail, entries)` - the number of cells, whether
/// the first is the mail cell, and how many of the others are entries. Mail takes a place only
/// when an entry still has one beside it.
pub fn cell_plan(avail: f32, has_mail: bool) -> (usize, bool, usize) {
    let places = if has_mail { slots_that_fit(avail, SHOWN + 1) } else { cells_that_fit(avail) };
    let mail = has_mail && places >= 2;
    (places, mail, places - usize::from(mail))
}

/// The accounts of the strip's mail cell, "anna@example.org 3 · work@example.com 1", in the
/// longest form that fits `width` in `META`: whole addresses, else the short ones ("anna@example 3
/// · work@example 1": the first label of the domain), else cut.
pub fn accounts_text(unread: &[MailUnread], width: f32) -> String {
    let join = |name: &dyn Fn(&str) -> String| {
        unread
            .iter()
            .map(|account| format!("{} {}", name(&account.address), account.unread))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let short = |address: &str| match address.split_once('@') {
        Some((local, domain)) => format!("{local}@{}", domain.split('.').next().unwrap_or(domain)),
        None => address.to_owned(),
    };
    let whole = join(&|address| address.to_owned());
    if text_width(&whole, typo::META) <= width {
        return whole;
    }
    ellipsize(&join(&short), typo::META, width)
}

/// The strip's mail cell, `width` wide: the icon beside the total unread, below it the accounts
/// with their numbers and the newest senders, `META` lines cut to fit. Not a button.
fn mail_cell(unread: &[MailUnread], senders: &[String], width: f32) -> Element<'static, Message> {
    let total: u32 = unread.iter().map(|account| account.unread).sum();
    let room = (width - 2.0 * SIDE - icon::SIZE - space::S).max(0.0);
    let head = typo::text(typo::BODY_STRONG, ellipsize(&unread_text(total), typo::BODY_STRONG, room))
        .style(theme::text)
        .wrapping(Wrapping::None);
    let accounts = typo::text(typo::META, accounts_text(unread, room)).style(theme::text_3).wrapping(Wrapping::None);
    let mut words = column![head, accounts].width(Fill).clip(true);
    if !senders.is_empty() {
        words = words.push(
            typo::text(typo::META, ellipsize(&senders_text(senders), typo::META, room))
                .style(theme::text_2)
                .wrapping(Wrapping::None),
        );
    }
    let mark = container(mail_mark()).center_y(line_height(typo::BODY_STRONG));
    let padding = Padding { top: space::XS, bottom: space::XS, left: SIDE, right: SIDE };
    container(row![mark, words].spacing(space::S)).width(width).padding(padding).style(style::container::group).into()
}

/// The strip for a window of `size`: at the left the numbers, beside them the top entries as cells
/// in one row, as many as fit; with unread mail the mail cell comes first and takes one place.
/// Nothing open: the calm tile.
pub fn strip(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    let inner = (size.width - 2.0 * space::M).max(0.0);
    if data.items.is_empty() {
        return tile(nothing(inner, data.error), Padding::new(space::S));
    }
    let parts = count_parts(data.items);
    let left = numbers_width(&parts).min(size::SIDEBAR).min(inner / STRIP_INFO_SHARE).ceil();
    let mut info = column![numbers_column(&parts, left)];
    if let Some(error) = data.error {
        info = info.push(Space::new().height(space::XS)).push(error_line(error, left));
    }
    // The cells' buttons reach `SIDE` towards the numbers, their text keeps the full gap.
    let avail = (inner - left - space::L + SIDE).max(0.0);
    let (unread, senders) = (mail_unread(data.folders), latest_senders(data.items));
    let (places, with_mail, shown) = cell_plan(avail, !unread.is_empty());
    let width = cell_width(avail, places);
    let mut cells = row![].spacing(CELL_GAP).align_y(Alignment::Center);
    if with_mail {
        cells = cells.push(mail_cell(&unread, &senders, width));
    }
    for item in data.items.iter().take(shown) {
        cells = cells.push(cell(item, data, width));
    }
    let content = row![
        container(info).width(left).clip(true),
        container(cells).width(Fill).clip(true).padding(Padding::ZERO.left(space::L - SIDE))
    ]
    .align_y(Alignment::Center)
    .height(Fill);
    let padding = Padding { top: space::S, bottom: space::S, left: space::M, right: space::M - SIDE };
    tile(content, padding)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_inbox::demo::{self, DemoSource};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 8, 30, 0).single().expect("valid time")
    }

    /// The demo's open entries of "For me", waiting first, as the app keeps them.
    fn for_me() -> Vec<Item> {
        let mut items: Vec<Item> = demo::sample(now())
            .into_iter()
            .filter(|item| item.lane == torro_inbox::Lane::ForMe && item.state.is_open())
            .collect();
        items.sort_by_key(|item| !list::is_waiting(item));
        items
    }

    fn calm_item(mut item: Item) -> Item {
        item.urgent = false;
        item
    }

    #[test]
    fn the_numbers_count_all_and_the_waiting_ones() {
        let items = for_me();
        let waiting = waiting(&items);
        assert!(waiting > 0 && waiting < items.len(), "the demo has both");
        assert_eq!(
            count_parts(&items),
            [(trf("{n} for you", &[("n", &items.len())]), false), (trf("{n} waiting", &[("n", &waiting)]), true)]
        );
        let quiet: Vec<Item> = items.into_iter().map(calm_item).collect();
        assert_eq!(count_parts(&quiet), [(trf("{n} for you", &[("n", &quiet.len())]), false)], "no 0 waiting");
        assert!(count_parts(&[]).is_empty());
        assert_eq!(calm(), tr("Nothing for you right now."));
    }

    #[test]
    fn waiting_entries_lead() {
        let items = for_me();
        assert!(items.len() > SHOWN);
        assert!(list::is_waiting(&items[0]), "a waiting entry leads");
    }

    #[test]
    fn the_height_of_the_tile_decides_how_many_rows_fit() {
        // 240×160: 12 above and below, the figures (32), 8 gap, rows of 28.
        assert_eq!(rows_that_fit(160.0, false), 3);
        assert_eq!(rows_that_fit(150.0, true), 2, "the error line takes room");
        assert_eq!(rows_that_fit(120.0, false), 2, "the smallest window");
        assert_eq!(rows_that_fit(40.0, false), 0);
        assert!(rows_that_fit(480.0, false) > SHOWN, "a tall tile fills its height");
    }

    #[test]
    fn the_strip_shares_its_width_but_does_not_stretch() {
        assert_eq!(cells_that_fit(2000.0), SHOWN);
        assert_eq!(cells_that_fit(CELL_MIN * 2.0 + CELL_GAP), 2);
        assert_eq!(cells_that_fit(CELL_MIN - 1.0), 0);
        assert_eq!(cell_width(2000.0, 3), CELL_MAX, "a wide strip keeps its cells together");
        assert_eq!(cell_width(CELL_MIN * 2.0 + CELL_GAP, 2), CELL_MIN);
        assert_eq!(cell_width(500.0, 0), 0.0);
    }

    #[test]
    fn a_count_splits_into_figure_and_word() {
        assert_eq!(split_count("6 for you"), ("6", "for you"));
        assert_eq!(split_count("waiting"), ("", "waiting"));
    }

    #[test]
    fn the_figures_take_the_smaller_level_when_the_big_one_does_not_fit() {
        let parts = count_parts(&for_me());
        assert_eq!(figure_level(&parts, 216.0), FIGURE_LEVELS[0], "a 240-wide tile has room for both");
        assert_eq!(figure_level(&parts, 100.0), FIGURE_LEVELS[1]);
    }

    /// An inbox folder of `account` with `unread` unread messages.
    fn inbox(account: &str, unread: u32) -> Folder {
        Folder {
            id: format!("f:{account}-inbox"),
            account: format!("mail:{account}"),
            name: "INBOX".into(),
            path: "INBOX".into(),
            role: Some(Role::Inbox),
            unread,
            total: unread + 10,
        }
    }

    /// The demo's folders of both mail accounts.
    fn mail_folders() -> Vec<Folder> {
        DemoSource::new(now()).mail_folders().expect("folders")
    }

    /// A mail entry in "For me" from `name`, `minutes` before now.
    fn mail_entry(id: &str, name: &str, minutes: i64) -> Item {
        let at = now() - chrono::TimeDelta::minutes(minutes);
        Item {
            id: id.into(),
            source: torro_inbox::SourceRef::mail("anna@example.org"),
            kind: torro_inbox::Kind::Message,
            from: Some(torro_inbox::Sender { name: name.into(), address: Some(format!("{id}@example.com")) }),
            received: at,
            updated: at,
            ..calm_item(for_me().remove(0))
        }
    }

    #[test]
    fn unread_mail_counts_the_inboxes_most_unread_first() {
        let mut junk = inbox("anna@example.org", 9);
        junk.role = Some(Role::Junk);
        let folders = [junk, inbox("anna@example.org", 2), inbox("work@example.com", 5), inbox("none@example.com", 0)];
        assert_eq!(
            mail_unread(&folders),
            [
                MailUnread { address: "work@example.com".into(), unread: 5 },
                MailUnread { address: "anna@example.org".into(), unread: 2 }
            ],
            "only inboxes, only with unread mail, the most first"
        );
        assert!(mail_unread(&[]).is_empty());
        let demo = mail_unread(&mail_folders());
        assert!(!demo.is_empty() && demo.iter().all(|account| account.unread > 0), "{demo:?}");
    }

    #[test]
    fn the_newest_three_different_senders_are_named() {
        let items = [
            mail_entry("a", "Lena", 50),
            mail_entry("b", "Ben", 5),
            mail_entry("c", "Lena", 10),
            mail_entry("d", "  ", 20),
            mail_entry("e", "Chris\nSmith", 30),
            mail_entry("f", "Dana", 40),
        ];
        assert_eq!(
            latest_senders(&items),
            ["Ben", "Lena", "d@example.com"],
            "newest first, each once, the address for no name"
        );
        assert_eq!(latest_senders(&items[..1]), ["Lena"]);
        // Entries of other sources have no place here.
        let agent = for_me().remove(0);
        assert!(latest_senders(&[agent]).is_empty());
        assert_eq!(latest_senders(&items[4..5]), ["Chris Smith"], "on one line");
    }

    #[test]
    fn the_mail_texts_are_translated() {
        assert_eq!(unread_text(4), trf("{n} unread", &[("n", &4)]));
        assert_eq!(senders_text(&["Lena".into(), "Ben".into()]), trf("From {senders}", &[("senders", &"Lena, Ben")]));
        let accounts = [
            MailUnread { address: "work@example.com".into(), unread: 5 },
            MailUnread { address: "anna@example.org".into(), unread: 2 },
        ];
        assert_eq!(accounts_text(&accounts, 1000.0), "work@example.com 5 · anna@example.org 2");
        let tight = text_width("work@example 5 · anna@example 2", typo::META);
        assert_eq!(accounts_text(&accounts, tight), "work@example 5 · anna@example 2", "the short form");
        assert!(accounts_text(&accounts, tight / 2.0).ends_with('…'), "then cut");
    }

    #[test]
    fn the_mail_block_leaves_the_entries_at_least_half_of_the_rows() {
        let one = [MailUnread { address: "anna@example.org".into(), unread: 3 }];
        let two = [one[0].clone(), MailUnread { address: "work@example.com".into(), unread: 1 }];
        let rows = |unread: &[MailUnread], senders, free| {
            let plan = mail_rows(unread, senders, free);
            (plan.accounts, plan.total, plan.senders, plan.used())
        };
        assert_eq!(rows(&[], true, 8), (0, false, false, 0), "no unread mail, no block");
        assert_eq!(rows(&one, true, 0), (0, false, false, 0), "no room");
        assert_eq!(rows(&one, true, 1), (1, false, false, 1), "the one row is the account");
        assert_eq!(rows(&two, true, 1), (0, true, false, 1), "two accounts do not fit one row: the total");
        assert_eq!(rows(&one, true, 3), (1, false, true, 2), "240x160: the account and the senders, one entry");
        assert_eq!(rows(&two, true, 3), (0, true, true, 2), "...the total and the senders");
        assert_eq!(rows(&two, false, 3), (2, false, false, 2), "no senders: both accounts");
        assert_eq!(rows(&two, true, 14), (2, false, true, 3), "a tall tile: everything");
        for free in 0..16 {
            for senders in [false, true] {
                assert!(mail_rows(&two, senders, free).used() <= free.div_ceil(2), "{free} {senders}");
            }
        }
    }

    #[test]
    fn mail_takes_a_place_in_the_strip_only_with_an_entry_beside_it() {
        let wide = 2000.0;
        assert_eq!(cell_plan(wide, false), (SHOWN, false, SHOWN));
        assert_eq!(cell_plan(wide, true), (SHOWN + 1, true, SHOWN));
        let two = CELL_MIN * 2.0 + CELL_GAP;
        assert_eq!(cell_plan(two, true), (2, true, 1));
        assert_eq!(cell_plan(CELL_MIN, true), (1, false, 1), "the entry comes first");
        assert_eq!(cell_plan(CELL_MIN - 1.0, true), (0, false, 0));
    }

    #[test]
    fn the_views_build_in_every_size_and_state() {
        let items = for_me();
        let quiet: Vec<Item> = items.iter().cloned().map(calm_item).collect();
        let sets: [&[Item]; 4] = [&[], &items[..1], &items, &quiet];
        let sizes = [(200.0, 120.0), (240.0, 160.0), (320.0, 480.0), (600.0, 120.0), (1200.0, 200.0), (3840.0, 270.0)];
        let folders = mail_folders();
        for items in sets {
            for (width, height) in sizes {
                let size = Size::new(width, height);
                let selected = items.first().map(|item| item.id.as_str());
                let data = GlanceData { items, selected, folders: &folders, now: now(), error: None };
                let _ = mini(&data, size);
                let _ = strip(&data, size);
                let failed = GlanceData { error: Some("Couldn't change the entry."), ..data };
                let _ = mini(&failed, size);
                let _ = strip(&failed, size);
                let no_mail = GlanceData { folders: &[], ..failed };
                let _ = mini(&no_mail, size);
                let _ = strip(&no_mail, size);
            }
        }
    }
}
