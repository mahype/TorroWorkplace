//! The notifications window (task E8.4 in `docs/tasks/eingang.md`; `docs/eingang.md`, "Drei
//! Fenster"): what the apps report – everything but mail –, each entry with its source's icon.
//!
//! - **Which sources** ([`Shown`]): every window keeps its own choice ("This window shows" in the
//!   sidebar, chips over the entries where the sidebar is folded away). Without `--source` it shows
//!   what the apps report – every source but the coding agents and the suite, which belong to the
//!   hub and can be switched on here. `--source` (repeated) starts it on exactly those, so a
//!   workspace template brings back a window of only Slack. The choice is window state, no request:
//!   the app filters `inbox.list_items` (with the API's `source` while the window shows one source)
//!   and keeps what the window shows ([`Shown::shows`]).
//! - **Board** ([`Board`], [`board`]): in the wide layout a column per app ([`app_columns`]: head
//!   with icon, name and number, the sources with the most entries first, the rest together under
//!   "Other apps"), or "By time" a grid of cards, newest or oldest first ([`by_time`], task E8.15); "By app | By time" in
//!   the header bar ([`Arrange`]). Narrow windows, and any with no room for two columns, show one
//!   column: "For me", then "Info" ([`groups`]).
//! - **Card** (comfortable density, `widgets::card`): the source's tile, the title, a line with
//!   source, "Info", reports and time, the ✕ at the top right (dismiss: done, counted apart), two
//!   lines of text, and its buttons, always shown ([`card_buttons`]): Open (or Go to window), Task,
//!   Later and the bell ([`bell`], "Mute…"), which opens the bell's menu (task E8.6b, `App::mute_menu`:
//!   the ways to mute from `inbox.list_mute_options`, "For", "What happens"). An approval has Allow ·
//!   Deny and neither ✕ nor Later nor bell; a coding agent Go to window ·
//!   Later · Done. Buttons carry all their words where they fit, else icons with the word as
//!   tooltip – only the lead (Open, Go to window) may keep its word alone ([`worded`], task E8.12);
//!   Done is quiet, the rest framed ([`button_look`]). The menus of a card's buttons open at its row
//!   of buttons, above it where the window has no room below – never over the bell.
//! - **Heads and axis** (task E8.12): over cards the groups have the same head as over rows (icon,
//!   name, number, `overview::icon_caption`); chips, heads and cards share one left edge
//!   ([`source_chips`]).
//! - **Row** (compact density, `list::notice_content`): the row of every view with its action bar
//!   ([`row_actions`]): Open, Task, Later, Mute, ✕.
//! - **Services** (task E5.15): GitHub, Nextcloud, Talk and ntfy have their own icons
//!   (`list::source_icon`) and columns; with two accounts of one service its columns, chips and
//!   checks name the account (`list::source_name`). The meta line says what the traits say
//!   ("torro/workplace · Review requested", "Files · Jonas Weber", "Server · Priority 5",
//!   `list::facet_line`). Their buttons are those of every app – ✕ · Open · Task · Later · bell, Talk
//!   without Reply until a source can answer. An ntfy alert is urgent: its tile is tinted in the
//!   attention colour and its meta line says "Urgent" – never Torro red, never colour alone.
//! - **The question after a finished task** (task E8.1b): the suite's "You finished a task from a
//!   mail" (`hub::is_follow_up`), shown when the window shows the suite, has the hub's buttons –
//!   Reply · Mark mail done · Not now – on its card and in its row, and no ✕ beside "Not now".
//!
//! Everything a button does goes through the app (`Message::Act`, `Message::OpenRow`,
//! `Message::Decide`), so it does what its key does at the entry: `x` dismisses, `m` opens the
//! bell's menu here. The board only lays out; heights and the order the keys walk ([`order`]) are plain
//! functions with tests, so the app can scroll the cursor into view ([`span`]).
//!
//! Not yet: Reply (no source can answer before Slack, E7) and the reminders' "In 5 min" (no
//! reminder reaches the inbox yet).

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, responsive, row, scrollable};
use iced::{Alignment, Color, Element, Fill, Padding};
use torro_core::design::Density;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::SidebarMode;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, BarWords, ButtonStyle, Mark, RowAction, TileFace, TileSize, Trail};
use torro_gui::{style, typo};
use torro_inbox::{Action, Item, Kind, Lane, Link, Order, SourceInfo, SourceKind, SourceRef, State};

use crate::app::{self, Message, View};
use crate::detail;
use crate::hub::{self, Act};
use crate::keys::Command;
use crate::list::{self, Rows};
use crate::overview;

/// How many lines of its text a card shows (the design's cards clamp to two or three; two keep a
/// column of cards calm).
const TEXT_LINES: usize = 2;

/// How the board arranges a view when there is room for columns (task E8.4, "By app | By time" in
/// the header bar). Window state, kept while the window runs; a smaller window shows one column
/// whatever is chosen, and gets the choice back when it grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrange {
    /// A column per app ([`app_columns`]).
    #[default]
    ByApp,
    /// By time, newest first unless the window turned it: a grid of cards, rows in the compact
    /// density ([`by_time`]).
    ByTime,
}

impl Arrange {
    /// In the order of the switch.
    pub const ALL: [Arrange; 2] = [Arrange::ByApp, Arrange::ByTime];

    /// The word in the switch.
    pub fn name(self) -> &'static str {
        match self {
            Arrange::ByApp => tr("By app"),
            Arrange::ByTime => tr("By time"),
        }
    }
}

/// Which sources a window shows (task E8.4, "This window shows"). The hub and the mail window show
/// everything of their kind; the notifications window chooses. Mail never shows here: the window's
/// kind keeps it out before this is asked (`Kind::keeps`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Shown {
    /// Every source: the hub and the mail window, which have no choice of their own.
    #[default]
    Everything,
    /// The notifications window as it starts without `--source`: what the apps report, every source
    /// [`by_default`] – not the coding agents and not the suite, which belong to the hub.
    Apps,
    /// Exactly these sources, in the order they were chosen (`--source`, repeated; the sidebar's
    /// checks, the chips). Never empty: the last shown source cannot be switched off.
    Only(Vec<SourceRef>),
}

impl Shown {
    /// The choice of `--source` given `sources` times (doubles dropped): [`Shown::Apps`] for none,
    /// else [`Shown::Only`] those.
    pub fn from_args(sources: Vec<SourceRef>) -> Shown {
        let mut unique: Vec<SourceRef> = Vec::new();
        for source in sources {
            if !unique.contains(&source) {
                unique.push(source);
            }
        }
        if unique.is_empty() { Shown::Apps } else { Shown::Only(unique) }
    }

    /// Whether the window shows entries of `source`.
    pub fn shows(&self, source: &SourceRef) -> bool {
        match self {
            Shown::Everything => true,
            Shown::Apps => by_default(source),
            Shown::Only(sources) => sources.contains(source),
        }
    }

    /// The one source the window shows, if it shows exactly one: then the app asks
    /// `inbox.list_items` for it alone (the API's `source`).
    pub fn single(&self) -> Option<&SourceRef> {
        match self {
            Shown::Only(sources) if sources.len() == 1 => sources.first(),
            Shown::Everything | Shown::Apps | Shown::Only(_) => None,
        }
    }

    /// The sources of `listed` (what the sidebar lists, [`listed`]) the window shows now.
    fn shown_of(&self, listed: &[SourceRef]) -> Vec<SourceRef> {
        match self {
            Shown::Only(sources) => sources.clone(),
            Shown::Everything | Shown::Apps => listed.iter().filter(|source| self.shows(source)).cloned().collect(),
        }
    }

    /// Whether `source` may be switched off: it is shown and another source of `listed` is too –
    /// a window always shows at least one source.
    pub fn can_hide(&self, source: &SourceRef, listed: &[SourceRef]) -> bool {
        self.shows(source) && self.shown_of(listed).iter().any(|other| other != source)
    }

    /// The choice after a click on `source` in the sidebar or its chip, given the sources `listed`:
    /// shown sources go, others come – as [`Shown::Only`] from then on, so new sources do not join a
    /// window someone chose. The last shown source stays ([`Shown::can_hide`]).
    pub fn toggled(&self, source: &SourceRef, listed: &[SourceRef]) -> Shown {
        let mut shown = self.shown_of(listed);
        match shown.iter().position(|other| other == source) {
            Some(_) if shown.len() == 1 => return self.clone(),
            Some(place) => {
                shown.remove(place);
            }
            None => shown.push(source.clone()),
        }
        Shown::Only(shown)
    }

    /// The part of the window's name in `ui.json` (task E13.2, `Launch::window_key`) for a chosen
    /// set: `source:` and the keys with `+` between them (`source:script:ci+script:deploy`); `None`
    /// for the windows that choose nothing.
    pub fn slice(&self) -> Option<String> {
        match self {
            Shown::Only(sources) => {
                Some(format!("source:{}", sources.iter().map(SourceRef::key).collect::<Vec<_>>().join("+")))
            }
            Shown::Everything | Shown::Apps => None,
        }
    }
}

/// Whether a notifications window shows `source` without being told ([`Shown::Apps`]): every app,
/// script and service – not the coding agents and the suite (approvals, account problems), whose
/// place is the hub, and never mail.
pub fn by_default(source: &SourceRef) -> bool {
    !matches!(source.kind, SourceKind::Agent | SourceKind::Suite | SourceKind::Mail)
}

/// The sources the sidebar's "This window shows" and the chips list, from `sources`
/// (`inbox.list_sources`): every source but mail, those shown by default first, then the agents and
/// the suite; and a source of `shown` that has no entries yet at the end, so it can be switched off.
pub fn listed(sources: &[SourceInfo], shown: &Shown) -> Vec<SourceRef> {
    let mut listed: Vec<SourceRef> =
        sources.iter().map(|info| info.source.clone()).filter(|source| source.kind != SourceKind::Mail).collect();
    listed.sort_by_key(|source| !by_default(source));
    if let Shown::Only(chosen) = shown {
        for source in chosen {
            if !listed.contains(source) {
                listed.push(source.clone());
            }
        }
    }
    listed
}

/// How the board lays out a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// One column: "For me", then "Info" ([`groups`]). Narrow windows and any without room for two
    /// columns.
    Single,
    /// So many columns side by side, one per app ([`app_columns`]).
    Columns(usize),
    /// By time ([`by_time`]) in rows of so many cards; 1 in the compact density – a list of rows.
    Grid(usize),
}

impl Board {
    /// Whether the board is a list of rows flush with the window, as the list of a view: one column
    /// in the compact density. Cards and columns stand in [`list::cards_inset`].
    pub fn flush(self, density: Density) -> bool {
        density == Density::Compact && matches!(self, Board::Single | Board::Grid(_))
    }
}

/// How many columns at least [`size::props()`] wide fit in `width` with [`size::card_gap()`] between
/// them; at least one. The narrowest column the design has is the properties column, and a card
/// with its buttons needs about as much.
pub fn fit(width: f32) -> usize {
    let gap = size::card_gap();
    let count = ((width + gap) / (size::props() + gap)).floor();
    if count >= 1.0 { count as usize } else { 1 }
}

/// Whether the header bar offers "By app | By time": a window of `class` with a board `width`
/// wide that has room for two columns ([`fit`]) and is not narrow.
pub fn has_arrange(width: f32, class: SizeClass) -> bool {
    class != SizeClass::Narrow && fit(width) >= 2
}

/// The board of a window of `class` whose entries get `width` (the content beside sidebar, rail and
/// panel, without the board's inset), arranged as `arrange` in `density`: one column where two do
/// not fit ([`has_arrange`]), else a column per app or the grid by time.
pub fn board(width: f32, class: SizeClass, arrange: Arrange, density: Density) -> Board {
    if !has_arrange(width, class) {
        return Board::Single;
    }
    match (arrange, density) {
        (Arrange::ByApp, _) => Board::Columns(fit(width)),
        (Arrange::ByTime, Density::Compact) => Board::Grid(1),
        (Arrange::ByTime, Density::Comfortable) => Board::Grid(fit(width)),
    }
}

/// The head of a column of the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// The column of one source.
    Source(SourceRef),
    /// The last column, which holds the sources that got none of their own.
    Others,
}

/// One column of the board "By app".
#[derive(Debug, Clone, PartialEq)]
pub struct AppColumn<'a> {
    pub head: Head,
    /// Its entries: per source "For me" before "Info", otherwise in the order of `items`.
    pub items: Vec<&'a Item>,
}

/// The entries of `items` in at most `count` columns, one per source: the sources with the most
/// entries first (ties in the order of `items`, the app's: waiting first, then the newest). With
/// more sources than columns the last column is "Other apps" with the rest, source after source.
/// So the order of all entries, column after column, is the same however many columns fit.
pub fn app_columns<'a>(items: &'a [Item], count: usize) -> Vec<AppColumn<'a>> {
    let mut by_source: Vec<(SourceRef, Vec<&'a Item>)> = Vec::new();
    for item in items {
        match by_source.iter_mut().find(|(source, _)| *source == item.source) {
            Some((_, entries)) => entries.push(item),
            None => by_source.push((item.source.clone(), vec![item])),
        }
    }
    for (_, entries) in &mut by_source {
        entries.sort_by_key(|item| item.lane == Lane::Info);
    }
    by_source.sort_by_key(|(_, entries)| std::cmp::Reverse(entries.len()));
    let count = count.max(1);
    let own = |(source, items): (SourceRef, Vec<&'a Item>)| AppColumn { head: Head::Source(source), items };
    if by_source.len() <= count {
        return by_source.into_iter().map(own).collect();
    }
    let rest = by_source.split_off(count - 1);
    let mut columns: Vec<AppColumn<'a>> = by_source.into_iter().map(own).collect();
    columns.push(AppColumn { head: Head::Others, items: rest.into_iter().flat_map(|(_, items)| items).collect() });
    columns
}

/// The entries of `items` by when they last changed, newest or oldest first as `sort` says (task
/// E8.15); entries of the same moment keep their order.
pub fn by_time(items: &[Item], sort: Order) -> Vec<&Item> {
    let mut all: Vec<&Item> = items.iter().collect();
    all.sort_by(|a, b| sort.apply(b.updated.cmp(&a.updated)));
    all
}

/// The groups of one column: in "New" (the overview) "For me", then "Info", each when it has
/// entries; in "Later" and "Done" all entries in one group without a head (`None`).
pub fn groups<'a>(items: &'a [Item], view: &View) -> Vec<(Option<Lane>, Vec<&'a Item>)> {
    if *view != View::Overview {
        return if items.is_empty() { Vec::new() } else { vec![(None, items.iter().collect())] };
    }
    [Lane::ForMe, Lane::Info]
        .into_iter()
        .map(|lane| (Some(lane), items.iter().filter(|item| item.lane == lane).collect::<Vec<_>>()))
        .filter(|(_, entries)| !entries.is_empty())
        .collect()
}

/// The entries of `items` in the order the keys walk them on `board` in `view`: group after group,
/// column after column, or by time in `sort`.
pub fn order<'a>(items: &'a [Item], board: Board, view: &View, sort: Order) -> Vec<&'a Item> {
    match board {
        Board::Single => groups(items, view).into_iter().flat_map(|(_, entries)| entries).collect(),
        Board::Columns(count) => app_columns(items, count).into_iter().flat_map(|column| column.items).collect(),
        Board::Grid(_) => by_time(items, sort),
    }
}

/// `h`/`l` on the board "By app": the entry at the same place in the column left (`right: false`) or
/// right of the one of `id` – the column's last when it is shorter. `None` on other boards, at the
/// outer columns and for an entry not shown.
pub fn beside(items: &[Item], board: Board, id: &str, right: bool) -> Option<String> {
    let Board::Columns(count) = board else { return None };
    let columns = app_columns(items, count);
    let (at, place) = columns
        .iter()
        .enumerate()
        .find_map(|(at, column)| column.items.iter().position(|item| item.id == id).map(|place| (at, place)))?;
    let to = if right { at.checked_add(1)? } else { at.checked_sub(1)? };
    let target = columns.get(to)?;
    let last = target.items.len().checked_sub(1)?;
    target.items.get(place.min(last)).map(|item| item.id.clone())
}

/// The height of a card's head: its tile, or the title over the meta line.
fn head_height() -> f32 {
    size::tile().max(typo::line_height(typo::body_strong()) + typo::line_height(typo::meta()))
}

/// The height the text of a card always takes: [`TEXT_LINES`] lines of `BODY`, also when its text
/// is shorter, so a card's height never depends on its width.
fn text_height() -> f32 {
    TEXT_LINES as f32 * typo::line_height(typo::body())
}

/// The height of `item`'s card ([`card`]): the card's padding, the head, the text when there is one
/// ([`card_text`]) and the row of buttons, [`size::card_gap()`] between them.
pub fn card_height(item: &Item) -> f32 {
    let pad = layout::panel_padding();
    let gap = size::card_gap();
    let text = if card_text(item).is_some() { text_height() + gap } else { 0.0 };
    pad.top + pad.bottom + head_height() + gap + text + size::control_sm()
}

/// The height of `item` on the board in `density`: its card, or its row.
pub fn height(item: &Item, density: Density) -> f32 {
    match density {
        Density::Comfortable => card_height(item),
        Density::Compact => list::row_height_of(item),
    }
}

/// The height of a group's head in one column: the caption over cards, the group head over rows.
fn group_height(density: Density) -> f32 {
    match density {
        Density::Comfortable => overview::caption_height(),
        Density::Compact => list::group_head() - list::head_trim(),
    }
}

/// Where the card or row of `id` lies in the board's scrolling content as `(top, bottom)` – in its
/// column on the board "By app", whose heads stand above the scrolling part; the first of a group
/// starts at its head. `None` for an entry not shown. `sort` is the order by time ([`by_time`]).
pub fn span(items: &[Item], board: Board, view: &View, density: Density, id: &str, sort: Order) -> Option<(f32, f32)> {
    let cards = density == Density::Comfortable;
    let gap = if cards { size::card_gap() } else { 0.0 };
    let top = if board.flush(density) { 0.0 } else { list::cards_inset().top };
    let high = |item: &Item| height(item, density);
    match board {
        Board::Single => {
            let mut y = top;
            for (lane, entries) in groups(items, view) {
                let start = y;
                if lane.is_some() {
                    y += group_height(density);
                }
                for (i, item) in entries.iter().enumerate() {
                    if i > 0 {
                        y += gap;
                    }
                    if item.id == id {
                        return Some((if i == 0 { start } else { y }, y + high(item)));
                    }
                    y += high(item);
                }
            }
            None
        }
        Board::Columns(count) => app_columns(items, count).iter().find_map(|column| {
            let mut y = top;
            for item in &column.items {
                if item.id == id {
                    return Some((y, y + high(item)));
                }
                y += high(item) + gap;
            }
            None
        }),
        Board::Grid(count) => {
            let mut y = top;
            for line in by_time(items, sort).chunks(count.max(1)) {
                let tallest = line.iter().map(|item| high(item)).fold(0.0, f32::max);
                if line.iter().any(|item| item.id == id) {
                    return Some((y, y + tallest));
                }
                y += tallest + gap;
            }
            None
        }
    }
}

/// Whether `item` has the ✕ (dismiss): every notification that is not done – not an approval, which
/// waits for yes or no, never mail, which is done or hidden in the mail window, and not the suite's
/// question after a finished task, whose "Not now" closes it ([`asks_to_follow_up`]).
pub fn dismissable(item: &Item) -> bool {
    !matches!(item.state, State::Done { .. })
        && !detail::is_approval(item)
        && item.source.kind != SourceKind::Mail
        && !asks_to_follow_up(item)
}

/// Whether `item` is the suite's open "You finished a task from a mail" (task E8.1b,
/// [`hub::is_follow_up`]): its card and row offer what the hub offers, Reply · Mark mail done · Not
/// now ([`hub::acts`]).
fn asks_to_follow_up(item: &Item) -> bool {
    hub::is_follow_up(item) && item.state.is_open()
}

/// What "Dismiss all" dismisses of `items` (the entries shown): every one with the ✕
/// ([`dismissable`]) but those of the suite – an account problem is dismissed one by one.
pub fn dismiss_all(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter(|item| dismissable(item) && item.source.kind != SourceKind::Suite)
        .map(|item| item.id.clone())
        .collect()
}

/// The text of `item`'s card on one line, which the card cuts after [`TEXT_LINES`] lines: what a
/// named coding agent does and where (`list::agent_line`), else the whole body. `None` without one.
pub fn card_text(item: &Item) -> Option<String> {
    match list::session_name(item) {
        Some(_) => Some(list::agent_line(item)),
        None => item.body.as_deref().map(list::one_line).filter(|text| !text.is_empty()),
    }
}

/// What a card's meta line names besides the time ([`meta_line`]): what the board around it does
/// not say already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Names {
    /// The source: outside its own column ("Other apps", by time, one column), unless the window
    /// shows one source only.
    pub source: bool,
    /// "Info" for an entry of that lane in "New": where no caption "Info" stands above it.
    pub lane: bool,
}

/// The line under a card's title: the source as far as `names` asks (with its account where
/// `known`, every source the inbox lists, has two of its service, `list::source_name`), "Waiting for
/// you" – "Urgent" for an alert – while it waits, else the lane as far as `names` asks (in "New"
/// only), what its traits say (`list::facet_line`, task E5.15), the number of reports from two on,
/// and the time (`list::time_text`), with " · " between them. The urgency comes before the traits, so
/// a narrow card cuts the traits, never the word.
pub fn meta_line(item: &Item, rows: &Rows<'_>, names: Names, known: &[SourceInfo]) -> String {
    let mut parts = Vec::new();
    if names.source {
        parts.push(list::source_name(&item.source, known.iter().map(|info| &info.source)));
    }
    if list::is_waiting(item) {
        parts.push(list::waiting_word(item).to_owned());
    } else if names.lane && item.lane == Lane::Info && *rows.view == View::Overview {
        parts.push(tr("Info").to_owned());
    }
    parts.extend(list::facet_line(item));
    if item.count > 1 {
        parts.push(trf("{n} reports", &[("n", &item.count)]));
    }
    parts.push(list::time_text(item, rows.view, rows.now));
    parts.join(" · ")
}

/// A button of a card: icon, word (the tooltip when it shows as icon) and what it sends; `None`
/// draws it disabled (Allow and Deny while a decision is on its way).
#[derive(Debug, Clone)]
pub struct CardButton {
    pub icon: Icon,
    pub label: String,
    pub on_press: Option<Message>,
    /// The action the entry is about ("Open", "Go to window", "Reply"): as the first button of its
    /// bar it may keep its word while the others are icons (`widgets::BarWords::Lead`, task E8.12).
    pub lead: bool,
    /// Its look with its word ([`button_look`]): `ghost` for what lets the entry go, `secondary`
    /// for the rest.
    pub look: ButtonStyle,
}

impl CardButton {
    fn new(icon: Icon, label: impl Into<String>, on_press: Option<Message>) -> CardButton {
        CardButton { icon, label: label.into(), on_press, lead: false, look: button_look(false) }
    }

    /// The same button as the lead of its bar ([`CardButton::lead`]).
    fn leading(self) -> CardButton {
        CardButton { lead: true, ..self }
    }

    /// The same button drawn quietly ([`button_look`]): it lets the entry go.
    fn quiet(self) -> CardButton {
        CardButton { look: button_look(true), ..self }
    }
}

/// The look of a card's button with its word (task E8.12, finding 12, one rule for the hub and the
/// notifications window): `ghost` for what only lets the entry go – Done, Dismiss, Not now –,
/// `secondary` for every other visible button (Open, Go to window, Task, Event, Later, Reply,
/// Allow, Deny …). The ✕ of a card's head and the bell stay icon buttons.
pub fn button_look(lets_go: bool) -> ButtonStyle {
    if lets_go { style::button::ghost } else { style::button::secondary }
}

/// Whether `item` is a coding agent's: waiting, or done with its turn.
fn is_agent(item: &Item) -> bool {
    matches!(item.kind, Kind::AgentWaiting | Kind::AgentDone)
}

/// The buttons of `item`'s card from the left, as the concept's table has them ("Knöpfe je
/// Eintrag"), each doing what its key does at the entry:
///
/// - an approval: Allow · Deny (disabled while `deciding` and once it is closed), Go to window when
///   its agent's window can be opened – no Task, no Later;
/// - the suite's "You finished a task from a mail": Reply · Mark mail done · Not now, as in the hub
///   ([`asks_to_follow_up`], task E8.1b);
/// - a done entry: Reopen;
/// - a coding agent: Go to window (when it can be opened) · Later · Done;
/// - everything else: Open (a page or an app, when [`detail::openable`] offers it) · Task · Later.
///
/// The ✕ stands in the card's head ([`dismissable`]), the bell at the right ([`bell`]).
pub fn card_buttons(item: &Item, deciding: bool) -> Vec<CardButton> {
    let act = |command| Some(Message::Act(item.id.clone(), command));
    let open = || Some(Message::OpenRow(item.id.clone()));
    let window = || CardButton::new(Icon::AppWindow, tr("Go to window"), open());
    if asks_to_follow_up(item) {
        return hub::acts(item).iter().map(|act| hub::card_button(act, item, deciding)).collect();
    }
    if detail::is_approval(item) {
        let decidable = item.state.is_open() && !deciding;
        let mut buttons: Vec<CardButton> = item
            .actions
            .iter()
            .map(|action| {
                let (mark, approval, allow) = match action {
                    Action::Allow { approval } => (Icon::Check, approval, true),
                    Action::Deny { approval } => (Icon::X, approval, false),
                };
                let decide = decidable.then(|| Message::Decide { approval: approval.clone(), allow });
                CardButton::new(mark, action.label(), decide)
            })
            .collect();
        if detail::openable(item) == Some(Link::Window) {
            buttons.push(window());
        }
        return buttons;
    }
    if matches!(item.state, State::Done { .. }) {
        return vec![CardButton::new(Icon::Inbox, tr("Reopen"), act(Command::Done))];
    }
    let mut buttons = Vec::new();
    match detail::openable(item) {
        Some(Link::Window) => buttons.push(window().leading()),
        Some(Link::Url { .. } | Link::App { .. }) => {
            buttons.push(CardButton::new(Icon::ArrowUpRight, tr("Open"), open()).leading())
        }
        Some(Link::Mail { .. }) | None => {}
    }
    let later = CardButton::new(Icon::Clock, tr("Later"), act(Command::Snooze));
    if is_agent(item) {
        buttons.extend([later, CardButton::new(Icon::Check, tr("Done"), act(Command::Done)).quiet()]);
    } else {
        buttons.extend([CardButton::new(Icon::ListTodo, tr("Task"), act(Command::MakeTask)), later]);
    }
    buttons
}

/// The action bar of the suite's question after a finished task ([`asks_to_follow_up`]): the hub's
/// buttons ([`hub::acts`]) with the keys that do the same at the entry – `r` replies, `e` is "Not
/// now"; "Mark mail done" has no key of its own.
fn follow_up_actions<'a>(item: &Item) -> Vec<RowAction<'a, Message>> {
    hub::acts(item)
        .into_iter()
        .filter_map(|act| {
            let key = match act {
                Act::Reply => "R",
                Act::NotNow => "E",
                _ => "",
            };
            act.message(item, false).map(|message| RowAction::new(act.icon(), act.label(), key, message))
        })
        .collect()
}

/// The bell of `item`'s card, row and panel: "Mute…", which opens the bell's menu (task E8.6b) as
/// `m` does – the ways to mute the entry, how long and what happens; a source muted as a whole is
/// unmuted there. None for the suite (never muted, security rule 1), an approval and a done entry.
pub fn bell(item: &Item) -> Option<CardButton> {
    if !app::rules_allowed(&item.source) || detail::is_approval(item) || matches!(item.state, State::Done { .. }) {
        return None;
    }
    Some(CardButton::new(Icon::BellOff, tr("Mute…"), Some(Message::Act(item.id.clone(), Command::Hide))))
}

/// The action bar of `item`'s row in the compact density (`widgets::row_actions`): Open or Go to
/// window (`Enter`), Task (`t`), Later (`s`) or Reopen (`e`) for a done entry, the bell (`m`,
/// [`bell`]) and the ✕ (`x`, [`dismissable`]) – only what works for the entry, as its card. The
/// suite's question after a finished task has Reply (`r`), Mark mail done and Not now (`e`) instead.
pub fn row_actions<'a>(item: &Item) -> Vec<RowAction<'a, Message>> {
    if asks_to_follow_up(item) {
        return follow_up_actions(item);
    }
    let act = |command| Message::Act(item.id.clone(), command);
    let approval = detail::is_approval(item);
    let done = matches!(item.state, State::Done { .. });
    let mut actions = Vec::new();
    match detail::openable(item) {
        Some(Link::Window) => {
            actions.push(RowAction::new(
                Icon::AppWindow,
                tr("Go to window"),
                "Enter",
                Message::OpenRow(item.id.clone()),
            ));
        }
        Some(Link::Url { .. } | Link::App { .. }) => {
            actions.push(RowAction::new(Icon::ArrowUpRight, tr("Open"), "Enter", Message::OpenRow(item.id.clone())));
        }
        Some(Link::Mail { .. }) | None => {}
    }
    if !approval {
        actions.push(RowAction::new(Icon::ListTodo, tr("Task"), "T", act(Command::MakeTask)));
    }
    if done {
        actions.push(RowAction::new(Icon::Inbox, tr("Reopen"), "E", act(Command::Done)));
    } else if !approval {
        actions.push(RowAction::new(Icon::Clock, tr("Later"), "S", act(Command::Snooze)));
    }
    if let Some(bell) = bell(item) {
        actions.push(RowAction::new(bell.icon, bell.label, "M", act(Command::Hide)));
    }
    if dismissable(item) {
        actions.push(RowAction::new(Icon::X, tr("Dismiss notification"), "X", act(Command::Dismiss)));
    }
    actions
}

/// What the board gets to draw.
pub struct BoardData<'a> {
    /// The entries of the view, those the window shows, in the app's order.
    pub items: &'a [Item],
    pub board: Board,
    pub density: Density,
    /// The cursor, the time, the anchor for the menus, the view (`list::Rows`).
    pub rows: Rows<'a>,
    /// A decision of an approval is on its way: its Allow and Deny are disabled.
    pub deciding: bool,
    /// The window shows one source only: no card needs to name it.
    pub single_source: bool,
    /// The icon of the view, for its empty state.
    pub icon: Icon,
    /// Every source the inbox lists (`inbox.list_sources`), so two accounts of one service are told
    /// apart in the heads of the columns and on the cards (`list::source_name`, task E5.15).
    pub sources: &'a [SourceInfo],
    /// The theme's attention colour: the tile of a card that waits is tinted in it (an alert).
    pub attention: Color,
    /// Newest or oldest first on the board "By time" (task E8.15).
    pub sort: Order,
}

/// What the empty state of `view` says in the notifications window: "Nothing new." for "New", as
/// the list's for the others.
pub fn empty_text(view: &View) -> (&'static str, &'static str) {
    match view {
        View::Overview => (tr("Nothing new."), tr("What your apps report shows up here.")),
        other => list::empty_text(other),
    }
}

/// The widest the board gets: [`size::list()`] per column (a wide window is not stretched), the gaps
/// and its inset; one column of rows or cards [`size::content_max()`].
pub fn max_width(board: Board) -> f32 {
    let columns = match board {
        Board::Columns(count) | Board::Grid(count) if count > 1 => count,
        Board::Single | Board::Columns(_) | Board::Grid(_) => return size::content_max(),
    };
    let count = columns as f32;
    count * size::list() + (count - 1.0) * size::card_gap() + list::cards_side()
}

/// The board: the empty state, one column, the columns by app or the grid by time – scrolling as one
/// ([`list::SCROLL`], so the app keeps the cursor in view), capped at [`max_width`].
pub fn view<'a>(data: BoardData<'a>) -> Element<'a, Message> {
    if data.items.is_empty() {
        let (title, hint) = empty_text(data.rows.view);
        return list::empty_state(data.icon, title, hint);
    }
    let cap = max_width(data.board);
    let board = responsive(move |bounds| match data.board {
        Board::Single => scroll(single(&data, bounds.width)),
        Board::Columns(count) => columns(&data, count, bounds.width),
        Board::Grid(count) => scroll(grid(&data, count, bounds.width)),
    });
    container(board).width(Fill).max_width(cap).height(Fill).into()
}

/// `content` scrolling as the list of a view does.
fn scroll<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    scrollable(content)
        .id(list::SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// An entry `width` wide: its card (naming `names`) or its row.
fn entry<'a>(item: &'a Item, data: &BoardData<'a>, width: f32, names: Names) -> Element<'a, Message> {
    match data.density {
        Density::Comfortable => card(item, data, names),
        Density::Compact => entry_row(item, data, width),
    }
}

/// One column `width` wide: per group its caption (cards) or head (rows), then its entries.
fn single<'a>(data: &BoardData<'a>, width: f32) -> Element<'a, Message> {
    let cards = data.density == Density::Comfortable;
    let inner = if cards { width - list::cards_side() } else { width };
    // The captions say the lane.
    let names = Names { source: !data.single_source, lane: false };
    let mut content = column![].width(Fill);
    for (lane, entries) in groups(data.items, data.rows.view) {
        if let Some(lane) = lane {
            let name = lane_name(lane);
            // The same head in both densities (task E8.12, finding 11): icon, name and number.
            content = content.push(if cards {
                overview::icon_caption(app::lane_icon(lane), None, name, entries.len())
            } else {
                let head = widgets::group_head(Mark::Icon(app::lane_icon(lane)), name, Some(entries.len()), None);
                list::without_top(head)
            });
        }
        let shown = entries.into_iter().map(|item| entry(item, data, inner, names));
        content = content.push(column(shown).spacing(if cards { size::card_gap() } else { 0.0 }));
    }
    if cards { container(content).padding(list::cards_inset()).into() } else { content.into() }
}

/// The grid by time `width` wide: rows of `count` cards (in `data.sort`), the last row filled up with
/// room so its cards keep the width of the others; with rows (compact) one list.
fn grid<'a>(data: &BoardData<'a>, count: usize, width: f32) -> Element<'a, Message> {
    let all = by_time(data.items, data.sort);
    if data.board.flush(data.density) {
        return column(all.into_iter().map(|item| entry_row(item, data, width))).width(Fill).into();
    }
    let count = count.max(1);
    let gap = size::card_gap();
    let names = Names { source: !data.single_source, lane: true };
    let lines = all.chunks(count).map(|line| {
        let mut cells: Vec<Element<'a, Message>> =
            line.iter().map(|item| container(card(item, data, names)).width(Fill).into()).collect();
        cells.extend((line.len()..count).map(|_| Space::new().width(Fill).into()));
        row(cells).spacing(gap).into()
    });
    container(column(lines).spacing(gap)).padding(list::cards_inset()).into()
}

/// The board "By app" `width` wide: the heads in a row that stays, below them the columns scrolling
/// together.
fn columns<'a>(data: &BoardData<'a>, count: usize, width: f32) -> Element<'a, Message> {
    let columns = app_columns(data.items, count);
    let gap = size::card_gap();
    let shown = columns.len().max(1) as f32;
    let inner = ((width - list::cards_side() - (shown - 1.0) * gap) / shown).max(0.0);
    let inset = list::cards_inset();
    let heads = columns.iter().map(|column| container(head(column, inner, data.sources)).width(Fill).into());
    let heads = container(row(heads).spacing(gap))
        .padding(Padding::new(0.0).top(inset.top).left(inset.left).right(inset.right));
    let item_gap = if data.density == Density::Comfortable { gap } else { 0.0 };
    let bodies = columns.iter().map(|shown| {
        let names = Names { source: shown.head == Head::Others, lane: true };
        let entries = shown.items.iter().map(|item| entry(item, data, inner, names));
        column(entries).spacing(item_gap).width(Fill).into()
    });
    let bodies = container(row(bodies).spacing(gap)).padding(inset);
    column![heads, scroll(bodies.into())].width(Fill).height(Fill).into()
}

/// The head of a column `width` wide: the source's tile, its name – with the account where `known`
/// has two of its service (`list::source_name`) – and the number of its entries, as high as a
/// property row ([`size::control()`]).
fn head<'a>(column: &AppColumn<'_>, width: f32, known: &[SourceInfo]) -> Element<'a, Message> {
    let (face, name) = match &column.head {
        Head::Source(source) => (
            TileFace::Icon(list::source_icon(source)),
            list::source_name(source, known.iter().map(|info| &info.source)),
        ),
        // Not the suite's grid: the layers of "the rest".
        Head::Others => (TileFace::Icon(Icon::Layers), tr("Other apps").to_owned()),
    };
    let count = column.items.len().to_string();
    let room = width - size::tile() - typo::text_width(&count, typo::meta()) - 2.0 * space::s();
    let name = typo::text(typo::body_strong(), typo::ellipsize(&name, typo::body_strong(), room))
        .style(theme::text)
        .wrapping(Wrapping::None);
    let count = typo::text(typo::meta().tabular(), count).style(theme::text_3).wrapping(Wrapping::None);
    let line = row![widgets::source_tile(face, TileSize::Medium, None), container(name).width(Fill).clip(true), count]
        .spacing(space::s())
        .align_y(Alignment::Center);
    container(line).center_y(size::control()).into()
}

/// "For me" or "Info".
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// The card of `item` (`widgets::card`, as high as [`card_height`]): the head with the source's tile,
/// the title (a named session's name), the meta line ([`meta_line`]) and the ✕; the text on
/// [`TEXT_LINES`] lines; the buttons. Under the cursor it is `selected` and registers the anchor, so
/// the menus of its buttons open at it. A click opens it in the panel.
fn card<'a>(item: &'a Item, data: &BoardData<'a>, names: Names) -> Element<'a, Message> {
    let selected = data.rows.selected == Some(item.id.as_str());
    // Tinted in the attention colour while it waits (an alert), as rows tint their icon.
    let tint = list::is_waiting(item).then_some(data.attention);
    let tile = widgets::source_tile(TileFace::Icon(list::source_icon(&item.source)), TileSize::Medium, tint);
    let (title, _) = list::words(item);
    let meta = meta_line(item, &data.rows, names, data.sources);
    let mut head =
        row![widgets::card_head(Some(tile), title, meta, None)].spacing(space::m()).align_y(Alignment::Center);
    if dismissable(item) {
        let dismiss = Message::Act(item.id.clone(), Command::Dismiss);
        head = head.push(widgets::icon_button(Icon::X, tr("Dismiss notification"), Some(dismiss)));
    }
    let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
    if let Some(text) = card_text(item) {
        let lines = responsive(move |bounds| {
            let lines = list::clamp_lines(&text, typo::body(), bounds.width, TEXT_LINES)
                .into_iter()
                .map(|line| typo::text(typo::body(), line).style(theme::text_2).wrapping(Wrapping::None).into());
            column(lines).into()
        });
        parts.push(container(lines).width(Fill).height(text_height()).clip(true).into());
    }
    let bar = buttons(card_buttons(item, data.deciding), bell(item), (data.rows.anchors.clone(), selected));
    // The menus of a card's buttons (the bell's, "Later until", task E8.12, finding 6) open at the
    // button that opened them (issue #122): below it, or above it where the window has no room below
    // – never over it. Its row of buttons is the fallback, never the whole card.
    parts.push(if selected { widgets::anchored(&data.rows.anchors.row, bar) } else { bar });
    widgets::card(parts, selected, Some(Message::Select(item.id.clone())))
}

/// The row of buttons of a card: `main` from the left, the `bell` at the right; all with their
/// words where they fit the card's width, else icons with the word as tooltip – only the lead, the
/// one the entry is about ("Go to window", "Open"), may keep its word alone ([`worded`], task E8.12:
/// a bar never mixes words and icons otherwise). Under the cursor (the `bool` of `at`) each button
/// that opens a menu registers its anchor (issue #122, `detail::Anchors::wrap`).
fn buttons<'a>(main: Vec<CardButton>, bell: Option<CardButton>, at: (detail::Anchors, bool)) -> Element<'a, Message> {
    let (anchors, shown) = at;
    let line = responsive(move |bounds| {
        let words = worded(&main, bell.is_some(), bounds.width);
        let mut line = row![].spacing(space::s()).align_y(Alignment::Center);
        for (i, button) in main.iter().enumerate() {
            let element = card_button(button.clone(), words.worded(i));
            line = line.push(anchors.wrap(shown, button.on_press.as_ref(), element));
        }
        if let Some(bell) = &bell {
            let element = widgets::icon_button(bell.icon, bell.label.clone(), bell.on_press.clone());
            line =
                line.push(iced::widget::space::horizontal()).push(anchors.wrap(shown, bell.on_press.as_ref(), element));
        }
        line.into()
    });
    container(line).width(Fill).height(size::control_sm()).into()
}

/// A button of a card: icon and word in its look ([`CardButton::look`]), or the icon alone with the
/// word as tooltip.
pub fn card_button<'a>(button: CardButton, words: bool) -> Element<'a, Message> {
    if words {
        widgets::text_button(Some(button.icon), button.label, button.look, button.on_press)
    } else {
        widgets::icon_button(button.icon, button.label, button.on_press)
    }
}

/// How the buttons `main` show their words in a row `width` wide with the bell after them (`bell`)
/// (`widgets::bar_words`, task E8.12): all of them where they fit, else only the first when it is
/// the entry's lead ([`CardButton::lead`]), else none. The estimate takes the buttons' real gaps
/// (the icon's 6, [`space::s()`] between buttons), so a design with roomier buttons (Plex) keeps its
/// words where they fit.
pub fn worded(main: &[CardButton], bell: bool, width: f32) -> BarWords {
    let labels: Vec<&str> = main.iter().map(|button| button.label.as_str()).collect();
    let lead = main.first().is_some_and(|button| button.lead);
    widgets::bar_words(&labels, lead, usize::from(bell), width)
}

/// The row of `item` `width` wide in the compact density: the notice row of every view
/// (`list::notice_content`) with the action bar of [`row_actions`] on its first line.
fn entry_row<'a>(item: &'a Item, data: &BoardData<'a>, width: f32) -> Element<'a, Message> {
    let selected = data.rows.selected == Some(item.id.as_str());
    let height = list::row_height_of(item);
    let content = list::notice_content(item, &data.rows, width);
    let line = list::sized_row(content, selected, Message::Select(item.id.clone()), height);
    let actions = data.rows.anchors.mark(selected, row_actions(item));
    let words = widgets::row_actions_words(&actions, width);
    let middle = list::first_line_middle(height, list::line_count(item));
    let line = widgets::row_actions_at(line, actions, selected, words, middle);
    if selected { widgets::anchored(&data.rows.anchors.row, line) } else { line }
}

/// The rows of "This window shows" in the sidebar in `mode`: per source of `listed` a check box in
/// `colour` (checked while shown), its name and its open entries (`sources`); a click switches it
/// on or off ([`Shown::toggled`] through `Message::ToggleSource`).
pub fn source_rows<'a>(
    mode: SidebarMode,
    listed: &[SourceRef],
    shown: &Shown,
    sources: &[SourceInfo],
    colour: Color,
) -> Vec<Element<'a, Message>> {
    listed
        .iter()
        .map(|source| {
            let open = sources.iter().find(|info| info.source == *source).map_or(0, |info| info.open);
            let trail = usize::try_from(open).ok().filter(|&open| open > 0).map_or(Trail::None, Trail::Count);
            let mark = Mark::Check(colour, shown.shows(source));
            let name = list::source_name(source, listed);
            widgets::sidebar_item_in(mode, mark, name, trail, false, Message::ToggleSource(source.clone()))
        })
        .collect()
}

/// The sources of `listed` as chips over the board (narrow and medium windows, where the sidebar is
/// folded away): chosen while shown; the last shown one cannot be switched off and is disabled.
///
/// They start on the board's left axis (task E8.12, finding 10): over a `flush` list of rows where
/// the rows' text starts ([`layout::row_padding`]), over cards and columns at the cards' edge
/// ([`list::cards_inset`]), where the groups' heads start too.
pub fn source_chips<'a>(listed: &[SourceRef], shown: &Shown, flush: bool) -> Element<'a, Message> {
    let chips = listed
        .iter()
        .map(|source| {
            let on = shown.shows(source);
            let press = (!on || shown.can_hide(source, listed)).then(|| Message::ToggleSource(source.clone()));
            let name = list::source_name(source, listed);
            widgets::choice_chip(name, Some(Mark::Icon(list::source_icon(source))), on, press)
        })
        .collect();
    let (left, right) = if flush {
        let pad = layout::row_padding();
        (pad.left, pad.right)
    } else {
        let inset = list::cards_inset();
        (inset.left, inset.right)
    };
    container(widgets::chip_row(chips))
        .padding(Padding::new(0.0).top(space::m()).bottom(space::xs()).left(left).right(right))
        .into()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Local, TimeZone, Utc};
    use torro_inbox::demo::DemoSource;
    use torro_inbox::{InboxSource, ItemFilter, StateFilter};

    use super::*;

    fn now() -> DateTime<Utc> {
        Local.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time").to_utc()
    }

    /// Every open entry of the demo that is no mail, waiting first – what the window holds when it
    /// shows everything.
    fn open() -> Vec<Item> {
        let demo = DemoSource::new(now());
        let filter = ItemFilter { state: Some(StateFilter::Open), ..ItemFilter::default() };
        let mut items = iced::futures::executor::block_on(demo.list_items(filter)).expect("the demo answers");
        items.retain(|item| item.source.kind != SourceKind::Mail);
        items.sort_by_key(|item| !list::is_waiting(item));
        items
    }

    fn ids<'a>(items: &[&'a Item]) -> Vec<&'a str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    fn script(name: &str) -> SourceRef {
        SourceRef::script(name)
    }

    #[test]
    fn a_window_shows_the_apps_unless_told_otherwise() {
        let apps = Shown::from_args(Vec::new());
        assert_eq!(apps, Shown::Apps);
        assert!(apps.shows(&script("deploy")));
        assert!(!apps.shows(&SourceRef::agent("claude-code")), "agents belong to the hub");
        assert!(!apps.shows(&SourceRef::suite()), "and so do approvals");
        assert!(!apps.shows(&SourceRef::mail("anna@example.org")));
        assert!(Shown::Everything.shows(&SourceRef::suite()));
        let only = Shown::from_args(vec![script("ci"), script("ci"), SourceRef::agent("codex")]);
        assert_eq!(only, Shown::Only(vec![script("ci"), SourceRef::agent("codex")]), "doubles dropped");
        assert!(only.shows(&SourceRef::agent("codex")) && !only.shows(&script("deploy")));
        assert_eq!(only.single(), None);
        assert_eq!(Shown::from_args(vec![script("ci")]).single(), Some(&script("ci")));
        assert_eq!(only.slice().as_deref(), Some("source:script:ci+agent:codex"));
        assert_eq!(Shown::Apps.slice(), None);
    }

    #[test]
    fn a_click_switches_a_source_and_the_last_one_stays() {
        let listed = vec![script("ci"), script("deploy"), SourceRef::agent("codex")];
        let fewer = Shown::Apps.toggled(&script("ci"), &listed);
        assert_eq!(fewer, Shown::Only(vec![script("deploy")]), "off: the others stay");
        assert!(!fewer.can_hide(&script("deploy"), &listed), "the last shown source stays");
        assert_eq!(fewer.toggled(&script("deploy"), &listed), fewer);
        let more = fewer.toggled(&SourceRef::agent("codex"), &listed);
        assert_eq!(more, Shown::Only(vec![script("deploy"), SourceRef::agent("codex")]), "on: an agent comes");
        assert!(more.can_hide(&script("deploy"), &listed));
        assert!(!more.can_hide(&script("ci"), &listed), "a source not shown is switched on, not off");
    }

    #[test]
    fn the_sidebar_lists_apps_first_and_chosen_sources_without_entries() {
        let info = |source: SourceRef| SourceInfo { source, open: 1, muted: false, health: None };
        let sources = vec![
            info(SourceRef::suite()),
            info(SourceRef::agent("codex")),
            info(script("ci")),
            info(SourceRef::mail("a@b")),
        ];
        assert_eq!(listed(&sources, &Shown::Apps), [script("ci"), SourceRef::suite(), SourceRef::agent("codex")]);
        let chosen = Shown::Only(vec![script("backup")]);
        assert_eq!(listed(&sources, &chosen).last(), Some(&script("backup")), "so it can be switched off");
    }

    #[test]
    fn the_board_has_columns_only_where_two_fit() {
        let two = 2.0 * size::props() + size::card_gap();
        assert_eq!(fit(two), 2);
        assert_eq!(fit(two - 1.0), 1);
        assert_eq!(fit(0.0), 1, "never none");
        let comfortable = Density::Comfortable;
        assert_eq!(board(two, SizeClass::Wide, Arrange::ByApp, comfortable), Board::Columns(2));
        assert_eq!(board(two, SizeClass::Wide, Arrange::ByTime, comfortable), Board::Grid(2));
        assert_eq!(board(two, SizeClass::Wide, Arrange::ByTime, Density::Compact), Board::Grid(1), "a list of rows");
        assert_eq!(board(two - 1.0, SizeClass::Wide, Arrange::ByApp, comfortable), Board::Single);
        assert_eq!(board(two, SizeClass::Narrow, Arrange::ByApp, comfortable), Board::Single, "narrow: one column");
        assert!(has_arrange(two, SizeClass::Medium) && !has_arrange(two, SizeClass::Narrow));
        assert!(Board::Single.flush(Density::Compact) && !Board::Single.flush(comfortable));
        assert!(!Board::Columns(2).flush(Density::Compact), "columns keep their inset");
    }

    #[test]
    fn columns_go_by_app_and_the_rest_to_other_apps() {
        let mut items = open();
        // Two more reports of the deploy script, so it has the most entries; the first in Info.
        let deploy = items.iter().find(|item| item.id == "demo-6").expect("the deploy notice").clone();
        items.insert(0, Item { id: "demo-6b".into(), lane: Lane::Info, ..deploy.clone() });
        items.push(Item { id: "demo-6c".into(), ..deploy });
        let columns = app_columns(&items, 3);
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].head, Head::Source(script("deploy")), "the most entries first");
        assert_eq!(ids(&columns[0].items), ["demo-6", "demo-6c", "demo-6b"], "For me before Info");
        assert_eq!(columns[1].head, Head::Source(SourceRef::github("anna-berg")), "three as well: ties as they come");
        assert_eq!(columns[2].head, Head::Others);
        let all: usize = columns.iter().map(|column| column.items.len()).sum();
        assert_eq!(all, items.len(), "nothing lost");
        // The order of the keys is the same however many columns fit.
        let walk = |count| {
            order(&items, Board::Columns(count), &View::Overview, Order::NewestFirst).into_iter().map(|i| i.id.clone())
        };
        assert_eq!(walk(2).collect::<Vec<_>>(), walk(6).collect::<Vec<_>>());
        assert_eq!(app_columns(&items, 100).len(), 12, "one column per source when there is room");
    }

    #[test]
    fn h_and_l_go_to_the_same_place_in_the_next_column() {
        let items = open();
        let board = Board::Columns(3);
        let columns = app_columns(&items, 3);
        let first = &columns[0].items[0].id;
        let right = beside(&items, board, first, true).expect("a column to the right");
        assert_eq!(right, columns[1].items[0].id);
        assert_eq!(beside(&items, board, &right, false).as_deref(), Some(first.as_str()));
        assert_eq!(beside(&items, board, first, false), None, "nothing left of the first");
        assert_eq!(beside(&items, Board::Single, first, true), None);
    }

    #[test]
    fn one_column_has_for_me_then_info_and_by_time_is_newest_first() {
        let items = open();
        let walked = order(&items, Board::Single, &View::Overview, Order::NewestFirst);
        let first_info = walked.iter().position(|item| item.lane == Lane::Info).expect("Info entries");
        assert!(walked[..first_info].iter().all(|item| item.lane == Lane::ForMe));
        assert!(walked[first_info..].iter().all(|item| item.lane == Lane::Info));
        assert_eq!(groups(&items, &View::Done).len(), 1, "Later and Done have no heads");
        let newest = by_time(&items, Order::NewestFirst);
        assert!(newest.windows(2).all(|pair| pair[0].updated >= pair[1].updated));
        let oldest = by_time(&items, Order::OldestFirst);
        assert!(oldest.windows(2).all(|pair| pair[0].updated <= pair[1].updated), "task E8.15");
    }

    #[test]
    fn spans_count_captions_cards_and_gaps() {
        let items = open();
        let walked = order(&items, Board::Single, &View::Overview, Order::NewestFirst);
        let top = list::cards_inset().top;
        let first = card_height(walked[0]);
        let comfortable = Density::Comfortable;
        assert_eq!(
            span(&items, Board::Single, &View::Overview, comfortable, &walked[0].id, Order::NewestFirst),
            Some((top, top + overview::caption_height() + first)),
            "the first card starts at its caption"
        );
        let second = top + overview::caption_height() + first + size::card_gap();
        assert_eq!(
            span(&items, Board::Single, &View::Overview, comfortable, &walked[1].id, Order::NewestFirst),
            Some((second, second + card_height(walked[1])))
        );
        // Rows of one column are flush: no inset, no gaps.
        let head = list::group_head() - list::head_trim();
        assert_eq!(
            span(&items, Board::Single, &View::Overview, Density::Compact, &walked[1].id, Order::NewestFirst),
            Some((head + list::row_height_of(walked[0]), head + 2.0 * size::row_notice()))
        );
        // In a column of the board "By app" the heads stand above the scrolling part.
        let columns = app_columns(&items, 2);
        let second_in_first = &columns[0].items[1];
        let below = top + card_height(columns[0].items[0]) + size::card_gap();
        assert_eq!(
            span(&items, Board::Columns(2), &View::Overview, comfortable, &second_in_first.id, Order::NewestFirst),
            Some((below, below + card_height(second_in_first)))
        );
        assert_eq!(span(&items, Board::Grid(2), &View::Overview, comfortable, "gone", Order::NewestFirst), None);
    }

    #[test]
    fn a_card_is_as_high_as_what_it_shows() {
        let items = open();
        let with_text = items.iter().find(|item| card_text(item).is_some()).expect("a notice with a text");
        let mut without = with_text.clone();
        without.body = None;
        without.agent = None;
        assert_eq!(card_height(with_text) - card_height(&without), text_height() + size::card_gap());
    }

    #[test]
    fn buttons_follow_the_kind_of_entry() {
        let items = open();
        let by_id = |id: &str| items.iter().find(|item| item.id == id).expect("in the demo");
        let words = |item: &Item| card_buttons(item, false).into_iter().map(|b| b.label).collect::<Vec<_>>();
        assert_eq!(words(by_id("demo-3")), [tr("Allow"), tr("Deny")], "an approval: no Task, no Later");
        assert!(!dismissable(by_id("demo-3")) && bell(by_id("demo-3")).is_none(), "and no ✕, no bell");
        assert!(card_buttons(by_id("demo-3"), true).iter().all(|b| b.on_press.is_none()), "off while deciding");
        assert_eq!(words(by_id("demo-1")), [tr("Go to window"), tr("Later"), tr("Done")], "a waiting agent");
        assert_eq!(words(by_id("demo-6")), [tr("Open"), tr("Task"), tr("Later")], "a notice with a page");
        assert_eq!(words(by_id("demo-12")), [tr("Task"), tr("Later")], "a notice without a link");
        let bell_of = |id: &str| bell(by_id(id));
        assert_eq!(bell_of("demo-6").map(|b| b.label), Some(tr("Mute…").to_owned()));
        assert!(
            matches!(bell_of("demo-6").and_then(|b| b.on_press), Some(Message::Act(id, Command::Hide)) if id == "demo-6"),
            "the bell does what m does: its menu"
        );
        assert!(bell_of("demo-5").is_none(), "the suite is never muted");
        let mut done = by_id("demo-6").clone();
        done.state = State::Done { at: now() };
        assert_eq!(words(&done), [tr("Reopen")]);
        assert!(!dismissable(&done));
    }

    /// Task E8.1b: the suite's "You finished a task from a mail" asks here what it asks in the hub.
    #[test]
    fn the_question_after_a_finished_task_has_the_hubs_buttons() {
        let mut question = open().into_iter().find(|item| item.id == "demo-9").expect("in the demo");
        app::demo_task(&mut question, now());
        assert!(hub::is_follow_up(&question));
        let words: Vec<String> = card_buttons(&question, false).into_iter().map(|b| b.label).collect();
        assert_eq!(words, [tr("Reply"), tr("Mark mail done"), tr("Not now")]);
        let sent: Vec<Option<Message>> = card_buttons(&question, false).into_iter().map(|b| b.on_press).collect();
        assert!(matches!(&sent[0], Some(Message::Act(id, Command::Reply)) if id == "demo-9"));
        assert!(matches!(&sent[1], Some(Message::MarkMailDone(id)) if id == "demo-9"));
        assert!(matches!(&sent[2], Some(Message::Act(_, Command::Done))), "Not now closes the question");
        assert!(!dismissable(&question) && bell(&question).is_none(), "no ✕ beside Not now, no bell");
        let bar = row_actions(&question);
        let labels: Vec<String> = bar.iter().map(|action| action.label.to_string()).collect();
        assert_eq!(labels, words, "the row offers the same");
        assert_eq!(bar.iter().map(|action| action.key).collect::<Vec<_>>(), ["R", "", "E"]);
        // Answered, it is an entry like any other.
        question.state = State::Done { at: now() };
        assert_eq!(card_buttons(&question, false).into_iter().map(|b| b.label).collect::<Vec<_>>(), [tr("Reopen")]);
    }

    #[test]
    fn the_row_bar_has_open_task_later_mute_and_dismiss() {
        let items = open();
        let deploy = items.iter().find(|item| item.id == "demo-6").expect("the deploy notice");
        let labels: Vec<String> = row_actions(deploy).iter().map(|action| action.label.to_string()).collect();
        assert_eq!(labels, [tr("Open"), tr("Task"), tr("Later"), tr("Mute…"), tr("Dismiss notification")]);
        let approval = items.iter().find(|item| detail::is_approval(item)).expect("an approval");
        assert!(row_actions(approval).is_empty(), "decided in the panel");
    }

    #[test]
    fn dismiss_all_leaves_approvals_and_the_suite() {
        let items = open();
        let gone = dismiss_all(&items);
        assert!(gone.contains(&"demo-6".to_owned()) && gone.contains(&"demo-1".to_owned()));
        assert!(!gone.contains(&"demo-3".to_owned()), "an approval waits for yes or no");
        assert!(!gone.contains(&"demo-5".to_owned()), "an account problem is dismissed one by one");
        assert!(dismissable(items.iter().find(|item| item.id == "demo-5").expect("the account problem")));
    }

    #[test]
    fn the_card_line_names_source_info_reports_and_time() {
        let items = open();
        let backup = items.iter().find(|item| item.id == "demo-7").expect("the backup notice");
        let view = View::Overview;
        let rows = Rows {
            view: &view,
            selected: None,
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: Vec::new(),
            hidable: Vec::new(),
            look: list::Look::Rows,
            folders: &[],
            edge_strip: false,
        };
        let time = list::time_text(backup, &view, now());
        let reports = trf("{n} reports", &[("n", &3)]);
        let all = Names { source: true, lane: true };
        assert_eq!(meta_line(backup, &rows, all, &[]), format!("backup · {} · {reports} · {time}", tr("Info")));
        let in_column = Names { source: false, lane: true };
        assert_eq!(meta_line(backup, &rows, in_column, &[]), format!("{} · {reports} · {time}", tr("Info")));
        let under_caption = Names { source: true, lane: false };
        assert_eq!(meta_line(backup, &rows, under_caption, &[]), format!("backup · {reports} · {time}"));
        let waiting = items.iter().find(|item| item.id == "demo-1").expect("a waiting agent");
        assert!(meta_line(waiting, &rows, in_column, &[]).starts_with(tr("Waiting for you")));

        // Task E5.15: a service's card says what its traits say, an alert that it is urgent, and the
        // account where two of one service are listed.
        let review = items.iter().find(|item| item.id == "demo-14").expect("the review");
        let time = list::time_text(review, &view, now());
        let said = format!("torro/workplace · {} · {time}", tr("Review requested"));
        assert_eq!(meta_line(review, &rows, in_column, &[]), said);
        let info = |source: SourceRef| SourceInfo { source, open: 1, muted: false, health: None };
        let two = [info(SourceRef::github("anna-berg")), info(SourceRef::github("anna-work"))];
        assert_eq!(meta_line(review, &rows, all, &two), format!("GitHub · anna-berg · {said}"));
        assert_eq!(meta_line(review, &rows, all, &two[..1]), format!("GitHub · {said}"));
        let alert = items.iter().find(|item| item.id == "demo-18").expect("the alert");
        let alert_time = list::time_text(alert, &view, now());
        let priority = trf("Priority {n}", &[("n", &5)]);
        assert_eq!(
            meta_line(alert, &rows, in_column, &[]),
            format!("{} · Server · {priority} · {alert_time}", tr("Urgent"))
        );
    }

    /// Task E5.15: the demo's services each get a column of their own, with their own icon – GitHub
    /// with its three entries, none of them from a script.
    #[test]
    fn every_service_has_its_column_and_icon() {
        let items = open();
        let columns = app_columns(&items, 100);
        let of = |source: SourceRef| columns.iter().find(|column| column.head == Head::Source(source.clone()));
        let github = of(SourceRef::github("anna-berg")).expect("GitHub's column");
        assert_eq!(ids(&github.items), ["demo-14", "demo-15", "demo-13"]);
        for source in [
            SourceRef::nextcloud("anna@cloud.example.org"),
            SourceRef::talk("anna@cloud.example.org"),
            SourceRef::ntfy("ntfy.sh"),
        ] {
            assert_eq!(of(source.clone()).map(|column| column.items.len()), Some(1), "{source}");
        }
        assert!(of(script("github")).is_none(), "no script dressed as GitHub");
        let icon = |source: SourceRef| list::source_icon(&source);
        assert_eq!(
            [
                icon(SourceRef::github("anna-berg")),
                icon(SourceRef::nextcloud("anna@cloud.example.org")),
                icon(SourceRef::talk("anna@cloud.example.org")),
                icon(SourceRef::ntfy("ntfy.sh")),
            ],
            [Icon::Github, Icon::Cloud, Icon::MessageCircle, Icon::RadioTower]
        );
    }

    /// Task E5.15, the concept's table "Knöpfe je Eintrag": GitHub, Nextcloud and ntfy have ✕ · Open
    /// · Task · Later · bell; Talk the same, without Reply until a source can answer.
    #[test]
    fn services_have_open_task_later_bell_and_dismiss() {
        let items = open();
        for id in ["demo-14", "demo-15", "demo-16", "demo-17", "demo-18"] {
            let item = items.iter().find(|item| item.id == id).expect("in the demo");
            let words: Vec<String> = card_buttons(item, false).into_iter().map(|b| b.label).collect();
            assert_eq!(words, [tr("Open"), tr("Task"), tr("Later")], "{id}");
            assert!(card_buttons(item, false)[0].lead, "{id}: Open leads");
            assert!(dismissable(item) && bell(item).is_some(), "{id}: ✕ and the bell");
            let bar: Vec<String> = row_actions(item).iter().map(|action| action.label.to_string()).collect();
            assert_eq!(bar, [tr("Open"), tr("Task"), tr("Later"), tr("Mute…"), tr("Dismiss notification")], "{id}");
        }
    }

    /// Task E5.15: with two accounts of one service, its columns, checks and chips name the account.
    #[test]
    fn two_accounts_of_a_service_are_told_apart() {
        let listed = vec![SourceRef::github("anna-berg"), SourceRef::github("anna-work"), script("ci")];
        let names: Vec<String> = listed.iter().map(|source| list::source_name(source, &listed)).collect();
        assert_eq!(names, ["GitHub · anna-berg", "GitHub · anna-work", "ci"]);
        let column = AppColumn { head: Head::Source(listed[0].clone()), items: Vec::new() };
        let _ = head(&column, size::props(), &[]);
    }

    /// Task E8.12 (finding 4): a card's bar shows all its words or none – only its lead ("Go to
    /// window", "Open") may keep its word alone; a bar without a lead never mixes.
    #[test]
    fn a_bar_shows_all_words_or_only_its_lead() {
        let items = open();
        let agent = items.iter().find(|item| item.id == "demo-1").expect("a waiting agent");
        let buttons = card_buttons(agent, false);
        assert_eq!(worded(&buttons, true, size::content_max()), BarWords::All, "room for all words");
        assert_eq!(worded(&buttons, true, 0.0), BarWords::None, "no room: icons only");
        let steps: Vec<BarWords> = (0..=72).map(|step| worded(&buttons, true, step as f32 * space::s())).collect();
        assert!(steps.contains(&BarWords::Lead), "a width where only the lead has its word: {steps:?}");
        let rank = |words: &BarWords| match words {
            BarWords::None => 0,
            BarWords::Lead => 1,
            BarWords::All => 2,
        };
        assert!(steps.windows(2).all(|pair| rank(&pair[0]) <= rank(&pair[1])), "more room, more words");
        // Task and Later have no lead: all or none.
        let notice = items.iter().find(|item| item.id == "demo-12").expect("a notice without a link");
        let plain = card_buttons(notice, false);
        assert!(!plain[0].lead);
        let mixed = (0..=72).map(|step| worded(&plain, true, step as f32 * space::s()));
        assert!(mixed.into_iter().all(|words| words != BarWords::Lead), "never one word among icons");
    }

    /// Task E8.12 (finding 4): the estimate takes the buttons' real gaps, so a card as wide as the
    /// buttons with their words keeps all of them (Plex was cut too early).
    #[test]
    fn a_card_exactly_as_wide_as_its_words_keeps_them() {
        let items = open();
        let notice = items.iter().find(|item| item.id == "demo-6").expect("a notice with a link");
        let buttons = card_buttons(notice, false);
        let labels: Vec<&str> = buttons.iter().map(|button| button.label.as_str()).collect();
        let room = widgets::bar_width(&labels, BarWords::All, 1);
        assert_eq!(worded(&buttons, true, room), BarWords::All);
    }

    #[test]
    fn wide_boards_are_capped() {
        assert_eq!(max_width(Board::Single), size::content_max());
        assert_eq!(max_width(Board::Grid(1)), size::content_max());
        let two = 2.0 * size::list() + size::card_gap() + list::cards_side();
        assert_eq!(max_width(Board::Columns(2)), two);
    }
}
