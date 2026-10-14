//! Mini and strip: the inbox as a glance at a small tile (task E3.3a in `docs/tasks/eingang.md`;
//! design skill `.claude/skills/torro-design`, section 6; the pattern is `torrotasks/src/glance.rs`).
//!
//! - **Mini** ([`mini`]): at the top the numbers, each a big figure (`DISPLAY`) with its word beside
//!   it – "6 for you  3 ● waiting" (the second part only while something waits) – then as many
//!   open entries of "For me" as fit the height, at least the top [`SHOWN`], as rows: the source's icon, the title cut to fit, when
//!   it came at the right ("16:40"). Nothing for the user: a calm "Nothing for you right now." under the inbox
//!   icon.
//! - **Strip** ([`strip`]): the same numbers at the left – the figure in `DISPLAY` as in the mini
//!   tile (task E8.11) –, the same entries beside them as cells in one row – icon and title, below it
//!   the source and when it came – sharing the width, each at most [`cell_max()`] wide (a wide strip
//!   keeps them together at the left), the last one taking what is left ([`cell_widths`], task
//!   E8.12), so the strip's end is not empty.
//!
//! **Waiting entries** (urgent and open, `list::is_waiting`) lead, as in the list. They are marked
//! as in its group "Waiting for you": the source's icon in the attention colour – never Torro red –
//! and, so colour is not the only sign, the attention dot in front of the time, the same dot that
//! stands before the word "waiting" in the numbers.
//!
//! **Mail** (task E4.4, reworked in E8.11): the mail window's numbers count its mail for the user, so
//! the quiet line "From Anna, Ben, Chris" with the newest senders of those entries
//! ([`latest_senders`]) stands right under them ([`GlanceData::mail`]). While a mail account has
//! unread mail in its inbox, a small block shows it – in the mini tile one row per account (address,
//! unread number at the right), only when every account has its row and the entries keep at least
//! half of the rows ([`mail_rows`]); in the strip as one more cell before the entries, an account
//! per line. There is no total of unread mail: a second, other number beside "for you" would only
//! puzzle. Senders are foreign text: drawn as plain text on one line, cut. The block is a display,
//! not a button: a click opens entries only.
//!
//! Both are displays, not small apps: no header, sidebar, menu, form or dialog, no keys. The one
//! thing a click does is open the entry: `Message::OpenForMe` puts it under the cursor and into the
//! panel of "For me", which the window shows once the tile is larger again (making the tile larger
//! is the tiling manager's job, as in torrocalendar's glance). The clicked row stays highlighted.
//!
//! **Sessions with a name** (task E2.6, shown here since E3.9): a coding agent's entry whose session
//! has a name shows the name instead of "Claude Code is waiting for you", as the list and the panel
//! do ([`title`]).
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
fn row_height() -> f32 {
    size::row_sidebar()
}
/// The levels of the mini's figures, largest first: the first that fits is used.
fn figure_levels() -> [typo::Level; 2] {
    [typo::display().tabular(), typo::title().tabular()]
}
/// The levels of the calm line, largest first.
fn head_levels() -> [typo::Level; 2] {
    [typo::title().tabular(), typo::body_strong().tabular()]
}
/// The gap between the mini's mail block and the entries below it.
fn mail_gap() -> f32 {
    space::xs()
}
/// The attention dot (`widgets::status_dot`) is this wide.
fn dot() -> f32 {
    space::s()
}
/// The numbers of the strip take at most this share of its width.
const STRIP_INFO_SHARE: f32 = 4.0;
/// A cell of the strip is never wider than this, so a wide strip keeps its entries together at
/// the left instead of spreading three words over the whole monitor.
pub fn cell_max() -> f32 {
    size::props()
}
/// A cell of the strip is never narrower than this; with less room fewer entries are shown.
pub fn cell_min() -> f32 {
    size::sidebar() / 2.0
}
/// The gap between the strip's cells.
fn cell_gap() -> f32 {
    space::s()
}
/// Inside a row or cell: the button's own padding at the sides ([`style::ITEM_PADDING`]). The
/// tile's edge is that much closer, so text still stands 12 in from the edge, as in the sisters.
fn side() -> f32 {
    space::s()
}

/// What the glance gets to draw.
pub struct GlanceData<'a> {
    /// The open entries of "For me", waiting ones first (as `App::take` keeps them).
    pub items: &'a [Item],
    /// Id of the entry open in the panel (after a click here): its row is highlighted.
    pub selected: Option<&'a str>,
    /// The folders of the mail accounts (empty without mail), for the unread numbers.
    pub folders: &'a [Folder],
    /// "Now", for the times ("16:40", "Yesterday 16:40").
    pub now: DateTime<Utc>,
    /// A later call that failed, as one quiet line (the glance has no retry button).
    pub error: Option<&'a str>,
    /// The mail window (task E8.11): its numbers count mail, so the newest senders of `items`
    /// ("From …", [`latest_senders`]) stand right under them. The other windows name no senders.
    pub mail: bool,
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

/// "From Anna, Ben, Chris".
pub fn senders_text(senders: &[String]) -> String {
    trf("From {senders}", &[("senders", &senders.join(", "))])
}

/// How many rows of [`row_height()`] the mini tile's mail block takes when `rows` rows of the tile
/// are free under the numbers: one per account with unread mail when they all fit in half of the
/// rows (rounded up), so the entries keep theirs; else none. The block never shrinks to a total of
/// unread mail (task E8.11): that would be a second, other number beside "for you".
pub fn mail_rows(unread: &[MailUnread], rows: usize) -> usize {
    if unread.len() <= rows.div_ceil(2) { unread.len() } else { 0 }
}

/// The newest senders the glance names under its numbers: those of `data`'s mail entries in the
/// mail window ([`GlanceData::mail`]), none elsewhere.
fn glance_senders(data: &GlanceData<'_>) -> Vec<String> {
    if data.mail { latest_senders(data.items) } else { Vec::new() }
}

/// The calm line when nothing is open for the user: the sentence of the empty "For me" view.
pub fn calm() -> &'static str {
    tr("Nothing for you right now.")
}

/// The height of the head of the mini tile: the figures at the largest level.
fn figure_height() -> f32 {
    line_height(figure_levels()[0])
}

/// The height of the quiet error line with its gap above.
fn error_height() -> f32 {
    space::xs() + line_height(typo::meta())
}

/// How many rows fit in a mini tile `height` high under the numbers (with room for the line of
/// `senders` under them and for the error line, if there are any).
pub fn rows_that_fit(height: f32, error: bool, senders: bool) -> usize {
    let lines = if error { error_height() } else { 0.0 } + if senders { line_height(typo::meta()) } else { 0.0 };
    let above = 2.0 * space::m() + figure_height() + space::s() + lines;
    ((height - above) / row_height()).floor().max(0.0) as usize
}

/// How many cells fit beside the numbers in a strip with `avail` width, at most [`SHOWN`].
pub fn cells_that_fit(avail: f32) -> usize {
    slots_that_fit(avail, SHOWN)
}

/// How many cells of at least [`cell_min()`] fit in `avail` width, at most `most`.
fn slots_that_fit(avail: f32, most: usize) -> usize {
    (((avail + cell_gap()) / (cell_min() + cell_gap())).floor().max(0.0) as usize).min(most)
}

/// The width of each of `count` cells in `avail`: an even share, at most [`cell_max()`].
pub fn cell_width(avail: f32, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    ((avail - cell_gap() * (count - 1) as f32) / count as f32).clamp(0.0, cell_max())
}

/// The widths of `count` cells in `avail`, from the left (task E8.12, finding 9): each
/// [`cell_width`], the last one also taking what is left at the right, so its title is cut later
/// instead of leaving the strip's end empty. The first cells keep their place, so a wide strip
/// still keeps its entries together at the left.
pub fn cell_widths(avail: f32, count: usize) -> Vec<f32> {
    let width = cell_width(avail, count);
    let mut widths = vec![width; count];
    if let Some(last) = widths.last_mut() {
        let others = (count - 1) as f32 * (width + cell_gap());
        *last = (avail - others).max(width);
    }
    widths
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
    let dot = if waiting { dot() + space::xs() } else { 0.0 };
    text_width(figure, level) + space::xs() + dot + text_width(word, typo::meta())
}

/// The level of the mini's figures: the largest of [`figure_levels()`] under which all parts with
/// their gaps fit `width`.
fn figure_level(parts: &[(String, bool)], width: f32) -> typo::Level {
    let total = |level: typo::Level| {
        let widths: f32 = parts.iter().map(|(text, waiting)| part_width(text, *waiting, level)).sum();
        widths + space::l() * parts.len().saturating_sub(1) as f32
    };
    figure_levels()
        .iter()
        .copied()
        .find(|level| total(*level) <= width)
        .unwrap_or(figure_levels()[figure_levels().len() - 1])
}

/// A line in `META` and `text_3`, never wrapped.
fn quiet(line: String) -> iced::widget::Text<'static> {
    typo::text(typo::meta(), line).style(theme::text_3).wrapping(Wrapping::None)
}

/// The error line: `META` in the error colour, cut to `width`.
fn error_line(error: &str, width: f32) -> Element<'static, Message> {
    typo::text(typo::meta(), ellipsize(error, typo::meta(), width))
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

/// When the entry last changed ("16:40", `list::stamp`), quiet and tabular; a waiting one has the
/// attention dot in front of it.
fn time_view(item: &Item, now: DateTime<Utc>) -> Element<'static, Message> {
    let time = typo::text(typo::meta().tabular(), list::stamp(item.updated, now)).style(theme::text_3);
    let mut line = row![].spacing(space::xs()).align_y(Alignment::Center);
    if list::is_waiting(item) {
        line = line.push(widgets::status_dot(widgets::Status::Attention));
    }
    line.push(time.wrapping(Wrapping::None)).into()
}

/// The width of [`time_view`] for `item`.
fn time_width(item: &Item, now: DateTime<Utc>) -> f32 {
    let dot = if list::is_waiting(item) { dot() + space::xs() } else { 0.0 };
    dot + text_width(&list::stamp(item.updated, now), typo::meta())
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
    let level = fit_level(calm(), &head_levels(), width);
    typo::text(level, ellipsize(calm(), level, width)).style(theme::text_2).wrapping(Wrapping::None).into()
}

/// The empty tile: the inbox icon, quiet, over the calm line, centred – and the error line below
/// when a call failed. Nothing is open, so there is nothing to click.
fn nothing(width: f32, error: Option<&str>) -> Element<'static, Message> {
    let mark = icon::icon(Icon::Inbox, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let mut content = column![mark, calm_line(width)].spacing(space::s()).align_x(Alignment::Center);
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
    let mut line = row![].spacing(space::l()).align_y(Alignment::Center);
    for (text, waiting) in parts {
        let (figure, word) = split_count(text);
        let mut part = row![typo::text(level, figure.to_owned()).style(theme::text).wrapping(Wrapping::None)]
            .spacing(space::xs())
            .align_y(Alignment::Center);
        if *waiting {
            part = part.push(widgets::status_dot(widgets::Status::Attention));
        }
        part = part.push(typo::text(typo::meta(), word.to_owned()).style(theme::text_2).wrapping(Wrapping::None));
        line = line.push(part);
    }
    container(line).height(figure_height()).center_y(figure_height()).into()
}

/// The title of `item` in a glance, on one line: for a coding agent's session with a name the name
/// (`list::session_name`, task E2.6), else the entry's title.
pub fn title(item: &Item) -> String {
    list::session_name(item).unwrap_or_else(|| list::one_line(&item.title))
}

/// One entry of the mini tile, `width` wide inside its padding: icon, title cut to fit, the time
/// at the right.
fn row_view(item: &Item, data: &GlanceData<'_>, width: f32) -> Element<'static, Message> {
    let room = (width - icon::SIZE - space::s() - space::s() - time_width(item, data.now)).max(0.0);
    let title = title(item);
    let title =
        typo::text(typo::body(), ellipsize(&title, typo::body(), room)).style(theme::text).wrapping(Wrapping::None);
    let line = row![source_mark(item), container(title).width(Fill).clip(true), time_view(item, data.now)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let content = container(line).height(row_height()).center_y(row_height());
    let selected = data.selected == Some(item.id.as_str());
    entry_button(content.into(), item, selected, style::ITEM_PADDING)
}

/// The mail icon, quiet like the entries' source icons.
fn mail_mark() -> Element<'static, Message> {
    icon::icon(Icon::Mail, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) }).into()
}

/// A row of the mini's mail block: [`row_height()`] high, its text where the entries' text stands.
fn mail_row(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    container(content).width(Fill).height(row_height()).center_y(row_height()).padding(style::ITEM_PADDING).into()
}

/// The row of one account: its address (cut), the unread number at the right.
fn account_row(account: &MailUnread, width: f32) -> Element<'static, Message> {
    let number = account.unread.to_string();
    let room = (width - icon::SIZE - 2.0 * space::s() - text_width(&number, typo::body())).max(0.0);
    let address = typo::text(typo::body(), ellipsize(&account.address, typo::body(), room))
        .style(theme::text_2)
        .wrapping(Wrapping::None);
    let number = typo::text(typo::body().tabular(), number).style(theme::text).wrapping(Wrapping::None);
    mail_row(
        row![mail_mark(), container(address).width(Fill).clip(true), number]
            .spacing(space::s())
            .align_y(Alignment::Center),
    )
}

/// The line with the newest senders under the numbers (task E8.11), `META` in `text_2`, cut to
/// `width`.
fn senders_line(senders: &[String], width: f32) -> Element<'static, Message> {
    let text = ellipsize(&senders_text(senders), typo::meta(), width.max(0.0));
    typo::text(typo::meta(), text).style(theme::text_2).wrapping(Wrapping::None).into()
}

/// The mini's mail block: the first `accounts` of `unread` ([`mail_rows`]), a row each, `width`
/// wide inside its padding.
fn mail_block(unread: &[MailUnread], accounts: usize, width: f32) -> Element<'static, Message> {
    let block = column(unread.iter().take(accounts).map(|account| account_row(account, width)));
    // A raised group like the settings pages' groups, so the summary reads apart from the entries.
    container(block).width(Fill).style(style::container::group).into()
}

/// The mini tile for a window of `size`: the numbers at the top – in the mail window with the
/// newest senders right under them (task E8.11) –, the mail block while there is unread mail and
/// every account has room, then the top entries that fit the height. Nothing open: the calm tile.
pub fn mini(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    // The rows' buttons reach to `side()` from the edge, their text to 12 like the head.
    let padding =
        Padding { top: space::m(), bottom: space::m(), left: space::m() - side(), right: space::m() - side() };
    let width = (size.width - 2.0 * space::m()).max(0.0);
    if data.items.is_empty() {
        return tile(nothing(width, data.error), Padding::new(space::m()));
    }
    let head = container(figures_line(&count_parts(data.items), width)).padding(Padding::ZERO.left(side()));
    let mut content = column![head];
    let senders = glance_senders(data);
    if !senders.is_empty() {
        content = content.push(container(senders_line(&senders, width)).padding(Padding::ZERO.left(side())));
    }
    if let Some(error) = data.error {
        content = content
            .push(Space::new().height(space::xs()))
            .push(container(error_line(error, width)).padding(Padding::ZERO.left(side())));
    }
    content = content.push(Space::new().height(space::s()));
    let (error, named) = (data.error.is_some(), !senders.is_empty());
    let free = rows_that_fit(size.height, error, named);
    let unread = mail_unread(data.folders);
    let accounts = mail_rows(&unread, free);
    if accounts > 0 {
        content = content.push(mail_block(&unread, accounts, width));
        // The gap below the block only where it does not push the last entry out.
        if rows_that_fit(size.height - mail_gap(), error, named) >= free {
            content = content.push(Space::new().height(mail_gap()));
        }
    }
    for item in data.items.iter().take(free - accounts) {
        content = content.push(row_view(item, data, width));
    }
    tile(content, padding)
}

/// The strip's numbers, `width` wide: "6 for you" with its figure in `DISPLAY`, as in the mini tile
/// (task E8.11, [`figures_line`]; the smaller level when it does not fit), below it the attention
/// dot and "3 waiting" while something waits, and in the mail window the newest `senders`.
fn numbers_column(parts: &[(String, bool)], senders: &[String], width: f32) -> Element<'static, Message> {
    let mut lines = column![].spacing(space::xs());
    for (text, waiting) in parts {
        if *waiting {
            // META like the cells' second line, so both lines of the numbers sit level with the
            // two lines of the cells beside them.
            let word = typo::text(typo::meta(), ellipsize(text, typo::meta(), width - dot() - space::xs()))
                .style(theme::text_2)
                .wrapping(Wrapping::None);
            let line = row![widgets::status_dot(widgets::Status::Attention), word]
                .spacing(space::xs())
                .align_y(Alignment::Center);
            lines = lines.push(line);
        } else {
            lines = lines.push(figures_line(&[(text.clone(), false)], width));
        }
    }
    if !senders.is_empty() {
        lines = lines.push(senders_line(senders, width));
    }
    lines.into()
}

/// The width the strip's numbers want: the wider of their lines, the figure at the largest level.
fn numbers_width(parts: &[(String, bool)], senders: &[String]) -> f32 {
    let named = if senders.is_empty() { 0.0 } else { text_width(&senders_text(senders), typo::meta()) };
    parts
        .iter()
        .map(|(text, waiting)| {
            if *waiting {
                dot() + space::xs() + text_width(text, typo::meta())
            } else {
                part_width(text, false, figure_levels()[0])
            }
        })
        .fold(named, f32::max)
}

/// One entry of the strip as a cell `width` wide: the icon beside the title (`BODY`), below the
/// title the source and when it came in `META` (a waiting one with the dot in front).
fn cell(item: &Item, data: &GlanceData<'_>, width: f32) -> Element<'static, Message> {
    let room = (width - 2.0 * side() - icon::SIZE - space::s()).max(0.0);
    let title = title(item);
    let title =
        typo::text(typo::body(), ellipsize(&title, typo::body(), room)).style(theme::text).wrapping(Wrapping::None);
    // The separator stays whole; only the source's name is cut.
    let separator = " · ";
    let source_room = (room - time_width(item, data.now) - text_width(separator, typo::meta())).max(0.0);
    let source = format!("{}{separator}", ellipsize(&item.source.label(), typo::meta(), source_room));
    let meta = row![quiet(source), time_view(item, data.now)].align_y(Alignment::Center);
    let words = column![title, meta].width(Fill).clip(true);
    // The icon stands on the title's line.
    let mark = container(source_mark(item)).center_y(line_height(typo::body()));
    let content = row![mark, words].spacing(space::s());
    let selected = data.selected == Some(item.id.as_str());
    let padding = Padding { top: space::xs(), bottom: space::xs(), left: side(), right: side() };
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
    if text_width(&whole, typo::meta()) <= width {
        return whole;
    }
    ellipsize(&join(&short), typo::meta(), width)
}

/// How many accounts the strip's mail cell names a line each; with more, one line names them all
/// ([`accounts_text`]).
const CELL_ACCOUNTS: usize = 2;

/// The strip's mail cell, `width` wide: the icon beside the accounts with unread mail, an account
/// per line (address in `text_2`, its number at the right) – no total, which would be a second
/// number beside "for you" (task E8.11) –; with more than [`CELL_ACCOUNTS`] one `META` line with
/// them all. Not a button.
fn mail_cell(unread: &[MailUnread], width: f32) -> Element<'static, Message> {
    let room = (width - 2.0 * side() - icon::SIZE - space::s()).max(0.0);
    let words: Element<'static, Message> = if unread.len() > CELL_ACCOUNTS {
        typo::text(typo::meta(), accounts_text(unread, room)).style(theme::text_2).wrapping(Wrapping::None).into()
    } else {
        let line = |account: &MailUnread| -> Element<'static, Message> {
            let number = account.unread.to_string();
            let space = (room - text_width(&number, typo::body()) - space::s()).max(0.0);
            let address = typo::text(typo::body(), ellipsize(&account.address, typo::body(), space))
                .style(theme::text_2)
                .wrapping(Wrapping::None);
            let number = typo::text(typo::body().tabular(), number).style(theme::text).wrapping(Wrapping::None);
            row![container(address).width(Fill).clip(true), number].spacing(space::s()).into()
        };
        column(unread.iter().map(line)).into()
    };
    let words = container(words).width(Fill).clip(true);
    let mark = container(mail_mark()).center_y(line_height(typo::body()));
    let padding = Padding { top: space::xs(), bottom: space::xs(), left: side(), right: side() };
    container(row![mark, words].spacing(space::s())).width(width).padding(padding).style(style::container::group).into()
}

/// The strip for a window of `size`: at the left the numbers, beside them the top entries as cells
/// in one row, as many as fit; with unread mail the mail cell comes first and takes one place.
/// Nothing open: the calm tile.
pub fn strip(data: &GlanceData<'_>, size: Size) -> Element<'static, Message> {
    let inner = (size.width - 2.0 * space::m()).max(0.0);
    if data.items.is_empty() {
        return tile(nothing(inner, data.error), Padding::new(space::s()));
    }
    let parts = count_parts(data.items);
    let senders = glance_senders(data);
    let left = numbers_width(&parts, &senders).min(size::sidebar()).min(inner / STRIP_INFO_SHARE).ceil();
    let mut info = column![numbers_column(&parts, &senders, left)];
    if let Some(error) = data.error {
        info = info.push(Space::new().height(space::xs())).push(error_line(error, left));
    }
    // The cells' buttons reach `side()` towards the numbers, their text keeps the full gap.
    let avail = (inner - left - space::l() + side()).max(0.0);
    let unread = mail_unread(data.folders);
    let (places, with_mail, shown) = cell_plan(avail, !unread.is_empty());
    // The last cell takes the rest of the row (task E8.12).
    let mut widths = cell_widths(avail, places).into_iter();
    let mut cells = row![].spacing(cell_gap()).align_y(Alignment::Center);
    if with_mail {
        cells = cells.push(mail_cell(&unread, widths.next().unwrap_or_default()));
    }
    for (item, width) in data.items.iter().take(shown).zip(widths) {
        cells = cells.push(cell(item, data, width));
    }
    let content = row![
        container(info).width(left).clip(true),
        container(cells).width(Fill).clip(true).padding(Padding::ZERO.left(space::l() - side()))
    ]
    .align_y(Alignment::Center)
    .height(Fill);
    let padding = Padding { top: space::s(), bottom: space::s(), left: space::m(), right: space::m() - side() };
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
        assert_eq!(rows_that_fit(160.0, false, false), 3);
        assert_eq!(rows_that_fit(150.0, true, false), 2, "the error line takes room");
        assert_eq!(rows_that_fit(120.0, false, false), 2, "the smallest window");
        assert_eq!(rows_that_fit(160.0, false, true), 2, "the senders' line takes room (task E8.11)");
        assert_eq!(rows_that_fit(40.0, false, false), 0);
        assert!(rows_that_fit(480.0, false, false) > SHOWN, "a tall tile fills its height");
    }

    #[test]
    fn the_strip_shares_its_width_but_does_not_stretch() {
        assert_eq!(cells_that_fit(2000.0), SHOWN);
        assert_eq!(cells_that_fit(cell_min() * 2.0 + cell_gap()), 2);
        assert_eq!(cells_that_fit(cell_min() - 1.0), 0);
        assert_eq!(cell_width(2000.0, 3), cell_max(), "a wide strip keeps its cells together");
        // Task E8.12 (finding 9): the last cell takes the rest, the others keep their place.
        let widths = cell_widths(1000.0, 3);
        assert_eq!(widths[..2], [cell_max(), cell_max()]);
        assert_eq!(widths[2], 1000.0 - 2.0 * (cell_max() + cell_gap()));
        assert_eq!(widths.iter().sum::<f32>() + 2.0 * cell_gap(), 1000.0, "the row is filled");
        assert_eq!(cell_widths(300.0, 2), vec![cell_width(300.0, 2); 2], "shared evenly: nothing left");
        assert!(cell_widths(500.0, 0).is_empty());
        assert_eq!(cell_width(cell_min() * 2.0 + cell_gap(), 2), cell_min());
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
        assert_eq!(figure_level(&parts, 216.0), figure_levels()[0], "a 240-wide tile has room for both");
        assert_eq!(figure_level(&parts, 100.0), figure_levels()[1]);
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
        assert_eq!(senders_text(&["Lena".into(), "Ben".into()]), trf("From {senders}", &[("senders", &"Lena, Ben")]));
        let accounts = [
            MailUnread { address: "work@example.com".into(), unread: 5 },
            MailUnread { address: "anna@example.org".into(), unread: 2 },
        ];
        assert_eq!(accounts_text(&accounts, 1000.0), "work@example.com 5 · anna@example.org 2");
        let tight = text_width("work@example 5 · anna@example 2", typo::meta());
        assert_eq!(accounts_text(&accounts, tight), "work@example 5 · anna@example 2", "the short form");
        assert!(accounts_text(&accounts, tight / 2.0).ends_with('…'), "then cut");
    }

    /// Task E8.11 (finding 8): the mail block shows every account or none – never a total of unread
    /// mail beside "for you" – and leaves the entries at least half of the rows.
    #[test]
    fn the_mail_block_leaves_the_entries_at_least_half_of_the_rows() {
        let one = [MailUnread { address: "anna@example.org".into(), unread: 3 }];
        let two = [one[0].clone(), MailUnread { address: "work@example.com".into(), unread: 1 }];
        assert_eq!(mail_rows(&[], 8), 0, "no unread mail, no block");
        assert_eq!(mail_rows(&one, 0), 0, "no room");
        assert_eq!(mail_rows(&one, 1), 1, "the one row is the account");
        assert_eq!(mail_rows(&two, 2), 0, "two accounts do not fit one row: no block, no total");
        assert_eq!(mail_rows(&two, 3), 2, "both accounts");
        for free in 0..16 {
            assert!(mail_rows(&two, free) <= free.div_ceil(2), "{free}");
        }
    }

    /// Task E8.11 (finding 8): only the mail window names the senders under its numbers, and its
    /// numbers are "for you" alone.
    #[test]
    fn the_mail_window_names_its_senders_under_the_numbers() {
        let items = [mail_entry("a", "Lena", 5), mail_entry("b", "Ben", 10)];
        let folders = mail_folders();
        let data = GlanceData { items: &items, selected: None, folders: &folders, now: now(), error: None, mail: true };
        assert_eq!(glance_senders(&data), ["Lena", "Ben"]);
        assert_eq!(count_parts(&items), [(trf("{n} for you", &[("n", &2)]), false)], "one number");
        let other = GlanceData { mail: false, ..data };
        assert!(glance_senders(&other).is_empty(), "the other windows name no senders");
    }

    /// Task E8.11 (finding 9): the strip's figure is as big as the mini's.
    #[test]
    fn the_strip_shows_its_figure_like_the_mini() {
        let parts = count_parts(&for_me());
        let figure = part_width(&parts[0].0, false, figure_levels()[0]);
        assert_eq!(numbers_width(&parts[..1], &[]), figure, "the figure at the mini's largest level");
        assert_eq!(figure_levels()[0], typo::display().tabular());
        let senders = ["Lena".to_owned()];
        assert!(numbers_width(&parts[..1], &senders) >= text_width(&senders_text(&senders), typo::meta()));
    }

    #[test]
    fn mail_takes_a_place_in_the_strip_only_with_an_entry_beside_it() {
        let wide = 2000.0;
        assert_eq!(cell_plan(wide, false), (SHOWN, false, SHOWN));
        assert_eq!(cell_plan(wide, true), (SHOWN + 1, true, SHOWN));
        let two = cell_min() * 2.0 + cell_gap();
        assert_eq!(cell_plan(two, true), (2, true, 1));
        assert_eq!(cell_plan(cell_min(), true), (1, false, 1), "the entry comes first");
        assert_eq!(cell_plan(cell_min() - 1.0, true), (0, false, 0));
    }

    #[test]
    fn a_named_session_shows_its_name_like_the_list() {
        let named = for_me().into_iter().find(|item| item.id == "demo-1").expect("a named session");
        let name = named.agent.as_ref().and_then(|agent| agent.title.clone()).expect("demo-1 has a name");
        assert_eq!(title(&named), name, "the name, not {:?}", named.title);
        let mut unnamed = named.clone();
        if let Some(agent) = unnamed.agent.as_mut() {
            agent.title = None;
        }
        assert_eq!(title(&unnamed), list::one_line(&named.title), "without a name: the title");
        let notice = for_me().into_iter().find(|item| item.agent.is_none()).expect("a notice");
        assert_eq!(title(&notice), list::one_line(&notice.title));
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
                let data = GlanceData { items, selected, folders: &folders, now: now(), error: None, mail: true };
                let _ = mini(&data, size);
                let _ = strip(&data, size);
                let elsewhere = GlanceData { mail: false, ..data };
                let _ = mini(&elsewhere, size);
                let _ = strip(&elsewhere, size);
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
