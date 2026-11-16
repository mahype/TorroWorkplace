//! Content cards (issue #784, `docs/inhaltskarten.md` "Gemeinsame Gestalt", prototype
//! `docs/prototypes/inhaltskarten/`): one shape for a web page, a mail, an event, a task, a draft, a contact, a file of
//! the own server, a quoted chat message and a GitHub issue, in the assistant, its bots and the chat with people
//! alike. A window says what a card holds ([`ContentCard`]) and draws it full ([`ContentCard::view`]) or as one line
//! ([`ContentCard::short`]); what a card shows comes from the core, never from a model.
//!
//! The parts, each optional but the head and the title:
//!
//! - **Head** ([`Head`]): the kind's icon, the source (a domain, an account, "From <name>"), and the mark
//!   `untrusted` for text from outside the suite. Always one line, the source never left out.
//! - **Picture** ([`Picture`]): at most one, 3:2 ([`Ratio::Photo`]) or 16:9 ([`Ratio::Video`], with the play mark),
//!   at most `size::card_picture_max()` high (a wider card crops it); without its pixels yet the kind's icon stands
//!   in its place, as high, so the card does not jump.
//! - **Title**: one line, cut with "…".
//! - **Lines** ([`ContentCard::line`]): up to [`MAX_LINES`] with an icon each (place, time, sender …), the most
//!   important first.
//! - **Buttons**: one main button ([`ContentCard::main`], "Open …") and up to [`MAX_MORE`] more
//!   ([`ContentCard::more`]); with their words where the card has room, else icons with the word as tooltip.
//! - **States** ([`State`]): loading (a quiet placeholder the size of the card), not available right now (head and
//!   source stay, "Try again"), no longer available (gone or out of reach, no button).
//!
//! [`link_card`] draws the card of a link ([`LinkCard`]) by its kind, [`ask_card`] the question whether a provider's
//! content may load (Settings › Privacy at "Ask"). In the chat with people only buttons that write nothing: a link
//! card has only "Open" and "Play" ([`LinkPress`]).
//!
//! Every card and state is in the gallery: `cargo run -p torro-gui --example gallery -- --content-cards`.

use chrono::{Datelike, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, Space, column, container, image, responsive, row, rule, stack, svg};
use iced::{Alignment, Background, Color, ContentFit, Element, Fill, FillPortion, Padding, Size, Theme, border};
use torro_chat::{ForeignProvider, GithubState, LinkCard, LinkCardKind};
use torro_core::i18n::{tr, trc, trf};

use crate::icon::{self, Icon, icon};
use crate::theme::{self, Palette, palette, radius, size, space};
use crate::widgets::{TileFace, TileSize};
use crate::{datetime, layout, style, typo, units, widgets};

/// At most this many lines under the title ([`ContentCard::line`]); more are left out.
pub const MAX_LINES: usize = 3;

/// At most this many buttons besides the main one ([`ContentCard::more`]); more are left out.
pub const MAX_MORE: usize = 3;

/// The mark of a card that is gone ([`State::Gone`]): not the `CircleSlash` of an issue closed as not planned, so the
/// two never read alike.
pub const GONE_ICON: Icon = Icon::Ban;

/// A hairline: frames of the picture, the `untrusted` tag and the question (torro-design, section 2).
const HAIRLINE: f32 = 1.0;

/// The head of a card: what kind it is and where it comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    /// The kind's icon (`Icon::Globe` for a page, `Icon::Mail`, `Icon::Calendar` …); it also stands in for a missing
    /// picture.
    pub mark: Icon,
    /// Where it comes from, one line: a domain, an account, a calendar, "From <name>".
    pub source: String,
    /// Text from outside the suite (a mail, a page, a server's tool): the head says so.
    pub untrusted: bool,
    /// The colour of the task list the item is in: a ring before the source, as the tasks app marks its lists
    /// (torro-design: the person's colours only as a mark, never as a surface). Set with [`Head::list_color`].
    pub list: Option<Color>,
}

impl Head {
    /// A head with `mark` and `source`, not marked untrusted.
    pub fn new(mark: Icon, source: impl Into<String>) -> Self {
        Self { mark, source: source.into(), untrusted: false, list: None }
    }

    /// The same head, marked `untrusted`.
    pub fn untrusted(self) -> Self {
        Self { untrusted: true, ..self }
    }

    /// The same head with the ring of a task list in `color` before the source.
    pub fn list_color(self, color: Color) -> Self {
        Self { list: Some(color), ..self }
    }
}

/// The calendar sheet of an event (prototype `docs/prototypes/inhaltskarten/`): the month, the day and the weekday of
/// its start, beside the title. The calendar's colour only as the sheet's top edge and the tint of its month band,
/// never as a surface of its own; today's day in the red circle, as everywhere in the suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The month, short ("Oct").
    pub month: String,
    /// The day of the month.
    pub day: u32,
    /// The weekday, short ("Thu").
    pub weekday: String,
    /// The calendar's colour; `None` draws the band neutral, without the edge.
    pub color: Option<Color>,
    /// The day is today: the number in the red circle.
    pub today: bool,
}

impl Sheet {
    /// The sheet of `date` in the calendar colour `color`, `today` for the red circle; month and weekday in the
    /// language of the suite (`datetime::month_name_short`, `datetime::weekday_name`).
    pub fn of(date: NaiveDate, color: Option<Color>, today: NaiveDate) -> Self {
        Self {
            month: datetime::month_name_short(date.month()).to_owned(),
            day: date.day(),
            weekday: datetime::weekday_name(date, true).to_owned(),
            color,
            today: date == today,
        }
    }
}

/// The shape of a card's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ratio {
    /// 3:2: a page's picture, a file's preview.
    Photo,
    /// 16:9: a video, with the play mark.
    Video,
}

impl Ratio {
    /// The picture's height for `width` in its ratio.
    pub fn height(self, width: f32) -> f32 {
        match self {
            Ratio::Photo => width * 2.0 / 3.0,
            Ratio::Video => width * 9.0 / 16.0,
        }
    }

    /// The height the picture stands at in a card `width` wide: its ratio, at most `size::card_picture_max()`. A
    /// wider card crops it (`ContentFit::Cover`); the stand-in without pixels takes the same height.
    pub fn shown_height(self, width: f32) -> f32 {
        self.height(width).min(size::card_picture_max())
    }
}

/// The picture of a card. `handle` `None`: not there (yet), the head's icon stands in its place.
#[derive(Debug, Clone)]
pub struct Picture {
    pub handle: Option<image::Handle>,
    pub ratio: Ratio,
    /// The play mark over it (a video).
    pub play: bool,
}

/// A button of a card: an icon, its word (also the tooltip when only the icon fits) and what a click sends; `None`
/// shows it switched off.
#[derive(Debug, Clone)]
pub struct Action<M> {
    pub icon: Icon,
    pub label: String,
    pub on_press: Option<M>,
    /// A button that only lets the item go ("Done", "Dismiss", "Not now"): drawn `ghost` instead of `secondary`
    /// (torro-design, "Knopfleisten auf Karten"). Set with [`Action::quiet`].
    pub quiet: bool,
}

impl<M> Action<M> {
    /// A button with `icon` and `label` that sends `on_press`, `secondary`.
    pub fn new(icon: Icon, label: impl Into<String>, on_press: M) -> Self {
        Self { icon, label: label.into(), on_press: Some(on_press), quiet: false }
    }

    /// The same button drawn quiet (`ghost`): it only lets the item go.
    pub fn quiet(self) -> Self {
        Self { quiet: true, ..self }
    }
}

/// How far a card is.
#[derive(Debug, Clone, Default)]
pub enum State<M> {
    /// What it shows is there.
    #[default]
    Ready,
    /// Being fetched: a quiet placeholder the size of the card.
    Loading,
    /// It could not be fetched now: head and source stay, "Not available right now" with "Try again" (`retry`, none
    /// without it).
    Failed { retry: Option<M> },
    /// Gone or out of reach: "No longer available", no button; why is not told.
    Gone,
}

/// The colour of a line's icon, picked from the palette when drawn so it follows light and dark; `None` keeps it
/// in `text_3` like every other mark.
pub type Tone = Option<fn(&Palette) -> Color>;

/// What leads a line under the title.
#[derive(Debug, Clone, Copy)]
enum Lead {
    /// An icon in its tone.
    Icon(Icon, Tone),
    /// A task's priority as its dot (`widgets::priority_dot`, 0 none … 4 urgent), the word beside it.
    Priority(usize),
}

/// A line under the title.
#[derive(Debug, Clone)]
struct Line {
    lead: Lead,
    text: String,
}

/// One content card, built with its parts and drawn with [`ContentCard::view`] or [`ContentCard::short`].
///
/// ```ignore
/// ContentCard::new(Head::new(Icon::Mail, "anna@example.org").untrusted(), "Offer Q4")
///     .line(Icon::Clock, "Mon 09:12")
///     .main(Action::new(Icon::ArrowUpRight, tr("Open in Mail"), Message::OpenMail(id)))
///     .view(room)
/// ```
#[derive(Debug, Clone)]
pub struct ContentCard<M> {
    head: Head,
    picture: Option<Picture>,
    sheet: Option<Sheet>,
    title: String,
    lines: Vec<Line>,
    main: Option<Action<M>>,
    more: Vec<Action<M>>,
    state: State<M>,
    on_press: Option<M>,
}

impl<M: Clone> ContentCard<M> {
    /// A card with `head` and `title`, nothing else yet.
    pub fn new(head: Head, title: impl Into<String>) -> Self {
        Self {
            head,
            picture: None,
            sheet: None,
            title: title.into(),
            lines: Vec::new(),
            main: None,
            more: Vec::new(),
            state: State::Ready,
            on_press: None,
        }
    }

    /// Its picture.
    pub fn picture(self, picture: Picture) -> Self {
        Self { picture: Some(picture), ..self }
    }

    /// The calendar sheet beside its title (an event).
    pub fn sheet(self, sheet: Sheet) -> Self {
        Self { sheet: Some(sheet), ..self }
    }

    /// One more line under the title, with `mark`; beyond [`MAX_LINES`] left out. An empty `text` is left out too.
    pub fn line(self, mark: Icon, text: impl Into<String>) -> Self {
        self.line_toned(mark, text, None)
    }

    /// [`ContentCard::line`] with the icon in the colour `tone` picks: a state, never the line's only sign of it –
    /// the word says it (the GitHub state's colour, an overlap in `attention`).
    pub fn line_toned(self, mark: Icon, text: impl Into<String>, tone: Tone) -> Self {
        self.push_line(Lead::Icon(mark, tone), text.into())
    }

    /// A line with a task's priority: its dot (`widgets::priority_dot`, `prio` 0 none … 4 urgent) and its `word`
    /// ("High"), as the tasks app shows it in a chip or a menu (torro-design, "Priorität und Fokus").
    pub fn line_priority(self, prio: usize, word: impl Into<String>) -> Self {
        self.push_line(Lead::Priority(prio), word.into())
    }

    /// One more line, unless the card has [`MAX_LINES`] already or `text` is empty.
    fn push_line(mut self, lead: Lead, text: String) -> Self {
        if self.lines.len() < MAX_LINES && !text.trim().is_empty() {
            self.lines.push(Line { lead, text });
        }
        self
    }

    /// Its main button ("Open …"), the first and the one that keeps its word longest.
    pub fn main(self, action: Action<M>) -> Self {
        Self { main: Some(action), ..self }
    }

    /// One more button after the main one; beyond [`MAX_MORE`] left out.
    pub fn more(mut self, action: Action<M>) -> Self {
        if self.more.len() < MAX_MORE {
            self.more.push(action);
        }
        self
    }

    /// How far it is ([`State`]); [`State::Ready`] unless set.
    pub fn state(self, state: State<M>) -> Self {
        Self { state, ..self }
    }

    /// The whole card a button that sends `message` (opens the item); its buttons take their own clicks first.
    pub fn on_press(self, message: M) -> Self {
        Self { on_press: Some(message), ..self }
    }

    /// The full card, `room` wide. The caller bounds it: the assistant by its reading column (the card grows with the
    /// answer, its picture stays at most `size::card_picture_max()` high), the chat with people and the gallery by
    /// `size::list()`.
    ///
    /// Ready: head, picture, title with its lines, buttons. Loading keeps the room of all of them (the picture, the
    /// title, each line – one at least – and the buttons as quiet bars), so the card does not jump when the content
    /// comes. Failed and gone keep the head with its source and say so in a line; only failed has a button. A card
    /// that is not ready takes no click as a whole.
    pub fn view<'a>(self, room: f32) -> Element<'a, M>
    where
        M: 'a,
    {
        let width = room.max(0.0);
        let inner = (width - layout::panel_padding().left - layout::panel_padding().right).max(0.0);
        let mut parts: Vec<Element<'a, M>> = vec![head_line(&self.head)];
        let mut on_press = None;
        match self.state {
            State::Ready => {
                if let Some(picture) = self.picture {
                    parts.push(picture_frame(picture, self.head.mark, inner));
                }
                let mut text = vec![widgets::ellipsized(typo::body_strong(), self.title, theme::text)];
                text.extend(self.lines.into_iter().map(detail_line));
                let text = column(text).spacing(space::xs()).width(Fill);
                parts.push(match self.sheet {
                    Some(sheet) => row![sheet_view(sheet), text].spacing(space::m()).into(),
                    None => text.into(),
                });
                let mut actions: Vec<Action<M>> = self.main.into_iter().collect();
                actions.extend(self.more);
                if !actions.is_empty() {
                    parts.push(buttons(actions, inner));
                }
                on_press = self.on_press;
            }
            State::Loading => {
                if let Some(picture) = self.picture {
                    parts.push(picture_frame(Picture { handle: None, ..picture }, self.head.mark, inner));
                }
                let mut text = vec![
                    typo::text(typo::body_strong(), tr("Loading…"))
                        .style(theme::text_3)
                        .wrapping(Wrapping::None)
                        .into(),
                ];
                text.extend((0..self.lines.len().max(1)).map(|_| bar_line()));
                parts.push(column(text).spacing(space::xs()).into());
                parts.push(bar_button());
            }
            State::Failed { retry } => {
                parts.push(detail_line(Line {
                    lead: Lead::Icon(Icon::CircleAlert, None),
                    text: tr("Not available right now").to_owned(),
                }));
                if let Some(retry) = retry {
                    parts.push(widgets::text_button(
                        Some(Icon::RefreshCw),
                        tr("Try again"),
                        style::button::secondary,
                        Some(retry),
                    ));
                }
            }
            State::Gone => parts.push(detail_line(Line {
                lead: Lead::Icon(GONE_ICON, None),
                text: tr("No longer available").to_owned(),
            })),
        }
        let card = widgets::card(parts, false, on_press);
        container(card).width(width).into()
    }

    /// The card in one line ("Kurzform"), `size::control_lg()` high: the kind's icon, the title and its first line,
    /// and the main button as an icon with its word as tooltip. For mini and strip tiles and lists of four to ten.
    ///
    /// The title takes the line; the first line (a date, a sender) stands against the end in `text_3`, in its own
    /// width up to two fifths of the line ([`short_detail_width`]), cut with "…" rather than pushing the button out.
    /// In a line narrower than [`short_detail_from`] (a mini tile) the first line is left out, so the title stays
    /// readable. Loading says so in place of the first line, failed shows the source and "Not available right now"
    /// with "Try again" as an icon, gone says "No longer available" without a button.
    pub fn short<'a>(self) -> Element<'a, M>
    where
        M: 'a,
    {
        let (title, detail, button) = match self.state {
            State::Ready => (self.title, self.lines.into_iter().next().map(|line| line.text), self.main),
            State::Loading => (self.title, Some(tr("Loading…").to_owned()), None),
            State::Failed { retry } => (
                self.head.source.clone(),
                Some(tr("Not available right now").to_owned()),
                retry.map(|retry| Action::new(Icon::RefreshCw, tr("Try again"), retry)),
            ),
            State::Gone => (tr("No longer available").to_owned(), None, None),
        };
        let kind = self.head.mark;
        let line = responsive(move |room: Size| {
            let mut line = row![
                mark(kind, icon::SIZE_SM, None),
                container(widgets::ellipsized(typo::body(), title.clone(), theme::text)).width(Fill)
            ]
            .spacing(space::s())
            .align_y(Alignment::Center);
            if let Some(width) = detail.as_deref().and_then(|detail| short_detail_width(detail, room.width)) {
                let detail = detail.clone().unwrap_or_default();
                line = line.push(
                    container(widgets::ellipsized_end(typo::meta().tabular(), detail, theme::text_3)).width(width),
                );
            }
            // Without a button its room stays empty, so the details of a list of short forms end on one edge.
            let line = line.push(match button.clone() {
                Some(main) => widgets::icon_button(main.icon, main.label, main.on_press),
                None => Space::new().width(size::control_sm()).into(),
            });
            // `responsive` takes the whole height: the line stands in its middle, with a button or without.
            container(line).center_y(Fill).into()
        });
        container(line).center_y(size::control_lg()).width(Fill).into()
    }
}

/// Below this width a short form leaves its first line out ([`ContentCard::short`]): a mini tile's line (240 less
/// its margins), where the title alone is what reads from a distance. 280: a sidebar and one large control.
pub fn short_detail_from() -> f32 {
    size::sidebar() + size::control_lg()
}

/// How wide the first line `detail` of a short form stands in a line `room` wide: its own width, at most two fifths
/// of the line; `None` below [`short_detail_from`], where it is left out.
pub fn short_detail_width(detail: &str, room: f32) -> Option<f32> {
    if room < short_detail_from() || detail.trim().is_empty() {
        return None;
    }
    // A little more than the measured width: the estimate must never cut a short date ("Wed", "17:14").
    let own = typo::text_width(detail, typo::meta()) + space::xs();
    Some(own.min(room * 2.0 / 5.0))
}

/// Four to ten items as one card of short forms (`docs/inhaltskarten.md`, "Grenzen"; the gallery's short list):
/// the `head` (the kind's icon, the source, `untrusted` when it is) with `note` at its end in `text_3` ("5 found"),
/// then the short forms ([`ContentCard::short`]) one under the other with a hairline between them. `room` wide.
pub fn short_group<'a, M: Clone + 'a>(
    head: &Head,
    note: Option<String>,
    shorts: Vec<Element<'a, M>>,
    room: f32,
) -> Element<'a, M> {
    let mut top = row![container(head_line(head)).width(Fill)].spacing(space::s()).align_y(Alignment::Center);
    if let Some(note) = note {
        top = top.push(typo::text(typo::meta().tabular(), note).style(theme::text_3).wrapping(Wrapping::None));
    }
    let mut list = Column::new().width(Fill);
    for (i, short) in shorts.into_iter().enumerate() {
        if i > 0 {
            list = list.push(rule::horizontal(HAIRLINE).style(style::rule));
        }
        list = list.push(short);
    }
    container(widgets::card(vec![top.into(), list.into()], false, None)).width(room.max(0.0)).into()
}

/// The head: the kind's icon, the source in `text_3`, and the tag `untrusted` at the end when it is.
fn head_line<'a, M: 'a>(head: &Head) -> Element<'a, M> {
    let mut line = row![mark(head.mark, icon::SIZE_SM, None)].spacing(space::s()).align_y(Alignment::Center);
    if let Some(color) = head.list {
        line = line.push(list_ring(color));
    }
    line = line.push(container(widgets::ellipsized(typo::meta(), head.source.clone(), theme::text_3)).width(Fill));
    if head.untrusted {
        line = line.push(untrusted_tag());
    }
    line.into()
}

/// The stroke of a list's ring and the edge of a calendar sheet (torro-design: rings 2 px, an event's bar 3 px).
const RING_STROKE: f32 = 2.0;
const SHEET_EDGE: f32 = 3.0;

/// The ring of a task list in `color`, `space::m()` across, as the tasks app marks its lists.
fn list_ring<'a, M: 'a>(color: Color) -> Element<'a, M> {
    let edge = space::m();
    container(Space::new())
        .width(edge)
        .height(edge)
        .style(move |_: &Theme| container::Style {
            border: border::rounded(edge / 2.0).color(color).width(RING_STROKE),
            ..container::Style::default()
        })
        .into()
}

/// The calendar sheet ([`Sheet`]): `size::avatar_lg()` and a step wide (56, the prototype's), on `panel` with a
/// hairline; on top the calendar's colour as a 3-px edge over the month band tinted in it (`Palette::tinted`, neutral
/// `selected` without a colour), the day in `TITLE` with tabular figures (today in the red circle), the weekday in
/// `MICRO` and `text_3`.
fn sheet_view<'a, M: 'a>(sheet: Sheet) -> Element<'a, M> {
    let width = size::avatar_lg() + space::s();
    let inner = radius::md() - HAIRLINE;
    let top = border::Radius { top_left: inner, top_right: inner, bottom_right: 0.0, bottom_left: 0.0 };
    let color = sheet.color;
    let month = typo::text(typo::micro(), sheet.month).style(theme::text).wrapping(Wrapping::None);
    let band = container(month).center_x(Fill).padding([space::xs() / 2.0, 0.0]).style(move |t: &Theme| {
        let p = palette(t);
        container::Style {
            background: Some(Background::Color(color.map_or(p.selected, |color| p.tinted(color)))),
            border: border::rounded(if color.is_some() { border::Radius::default() } else { top }),
            ..container::Style::default()
        }
    });
    let mut parts: Vec<Element<'a, M>> = Vec::new();
    if let Some(color) = color {
        parts.push(
            container(Space::new())
                .width(Fill)
                .height(SHEET_EDGE)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: border::rounded(top),
                    ..container::Style::default()
                })
                .into(),
        );
    }
    parts.push(band.into());
    let figure = typo::text(typo::title().tabular(), sheet.day.to_string()).wrapping(Wrapping::None);
    let day: Element<'a, M> = if sheet.today {
        let edge = typo::line_height(typo::title());
        container(
            typo::text(typo::body_strong().tabular(), sheet.day.to_string())
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).on_accent) }),
        )
        .center(edge)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).accent)),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
    } else {
        figure.style(theme::text).into()
    };
    parts.push(container(day).center_x(Fill).padding(Padding::new(0.0).top(space::xs())).into());
    parts.push(
        container(typo::text(typo::micro(), sheet.weekday).style(theme::text_3).wrapping(Wrapping::None))
            .center_x(Fill)
            .padding(Padding::new(0.0).bottom(space::s()))
            .into(),
    );
    container(column(parts))
        .width(width)
        .padding(HAIRLINE)
        .style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.panel)),
                border: border::rounded(radius::md()).color(p.line_strong).width(HAIRLINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// The tag `untrusted`: a quiet pill (fully round, hairline in `line_strong`) with the word in `CAPTION` and
/// `text_3`, as in the prototype – a note beside the source, not a warning that outshouts the card. No colour:
/// the word carries it (torro-design, section 1 – status never by colour alone, and the attention colour is too
/// faint for small text in light mode).
fn untrusted_tag<'a, M: 'a>() -> Element<'a, M> {
    let high = typo::line_height(typo::caption());
    container(typo::text(typo::caption(), tr("untrusted")).style(theme::text_3).wrapping(Wrapping::None))
        .padding(Padding::new(0.0).left(space::s()).right(space::s()))
        .style(move |t: &Theme| container::Style {
            border: border::rounded(high / 2.0 + HAIRLINE).color(palette(t).line_strong).width(HAIRLINE),
            ..container::Style::default()
        })
        .into()
}

/// A line under the title: its icon (in its tone) or a priority's dot in the icon's cell, and its text in `text_2`,
/// one line.
fn detail_line<'a, M: 'a>(line: Line) -> Element<'a, M> {
    let lead: Element<'a, M> = match line.lead {
        Lead::Icon(which, tone) => mark(which, icon::SIZE_SM, tone),
        Lead::Priority(prio) => container(widgets::priority_dot(prio)).center(icon::SIZE_SM).into(),
    };
    row![lead, container(widgets::ellipsized(typo::body(), line.text, theme::text_2)).width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// An icon in `tone`, `text_3` without one.
fn mark<'a, M: 'a>(which: Icon, edge: f32, tone: Tone) -> Element<'a, M> {
    icon(which, edge)
        .style(move |t: &Theme, _| {
            let p = palette(t);
            svg::Style { color: Some(tone.map_or(p.text_3, |tone| tone(&p))) }
        })
        .into()
}

/// A quiet bar of `width` in a slot `height` high: the stand-in of a text or a button while a card loads.
fn bar<'a, M: 'a>(width: impl Into<iced::Length>, height: f32, edge: f32) -> Element<'a, M> {
    let shape = container(Space::new()).width(width).height(edge).style(|t: &Theme| container::Style {
        background: Some(Background::Color(palette(t).selected)),
        border: border::rounded(radius::xs()),
        ..container::Style::default()
    });
    container(shape).center_y(height).width(Fill).into()
}

/// The stand-in of a line while loading: as high as a line, a bar over three fifths of its width (a line is rarely
/// full, and a bar to the edge would read as a rule).
fn bar_line<'a, M: 'a>() -> Element<'a, M> {
    let bar = container(bar(Fill, typo::line_height(typo::body()), space::s())).width(FillPortion(3));
    row![bar, Space::new().width(FillPortion(2))].into()
}

/// The stand-in of the buttons while loading: one button's room.
fn bar_button<'a, M: 'a>() -> Element<'a, M> {
    let width = widgets::text_button_width(true, tr("Open"));
    container(bar(width, size::control_sm(), size::control_sm())).width(width).into()
}

/// The picture `width` wide in its ratio, at most `size::card_picture_max()` high and cropped to it
/// ([`Ratio::shown_height`]); without its pixels the kind's icon in a neutral tile on the panel surface, as high; a
/// video with the play mark over it (alone, without the tile, so the two do not crowd the middle).
fn picture_frame<'a, M: 'a>(picture: Picture, kind: Icon, width: f32) -> Element<'a, M> {
    let height = picture.ratio.shown_height(width);
    let shown: Element<'a, M> = match picture.handle {
        Some(handle) => {
            image(handle).width(width).height(height).content_fit(ContentFit::Cover).border_radius(radius::sm()).into()
        }
        None => {
            let stand_in: Element<'a, M> = match picture.play {
                true => Space::new().into(),
                false => widgets::source_tile(TileFace::Icon(kind), TileSize::Large, None),
            };
            container(stand_in)
                .center(Fill)
                .width(width)
                .height(height)
                .style(|t: &Theme| {
                    let p = palette(t);
                    container::Style {
                        background: Some(Background::Color(p.panel)),
                        border: border::rounded(radius::sm()).color(p.line).width(HAIRLINE),
                        ..container::Style::default()
                    }
                })
                .into()
        }
    };
    if !picture.play {
        return shown;
    }
    stack![shown, container(play_mark()).center(Fill)].width(width).height(height).into()
}

/// The play mark: a round floating surface (`menu`, `line_strong` edge) with the play icon in `text`, readable over
/// any picture in light and dark.
fn play_mark<'a, M: 'a>() -> Element<'a, M> {
    let edge = size::control_lg();
    let play: Element<'a, M> =
        icon(Icon::Play, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into();
    container(play)
        .center(edge)
        .style(move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.menu)),
                border: border::rounded(edge / 2.0).color(p.line_strong).width(HAIRLINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// The buttons of a card in one row, `secondary`, a quiet one ([`Action::quiet`]) `ghost` (torro-design,
/// "Knopfleisten auf Karten": a card's visible buttons are secondary, what only lets the item go is ghost); with
/// words where they fit `room`, else icons with the word as tooltip (the main one keeps its word first,
/// `widgets::bar_words`).
fn buttons<'a, M: Clone + 'a>(actions: Vec<Action<M>>, room: f32) -> Element<'a, M> {
    let labels: Vec<&str> = actions.iter().map(|action| action.label.as_str()).collect();
    let words = widgets::bar_words(&labels, true, 0, room);
    let buttons = actions.into_iter().enumerate().map(|(i, action)| match words.worded(i) {
        true => {
            let look = if action.quiet { style::button::ghost } else { style::button::secondary };
            widgets::text_button(Some(action.icon), action.label, look, action.on_press)
        }
        false => widgets::icon_button(action.icon, action.label, action.on_press),
    });
    Row::with_children(buttons).spacing(space::s()).align_y(Alignment::Center).into()
}

/// What a click on a [`link_card`] sends: `open` opens the link (in the browser, the own Nextcloud, the chat or on
/// GitHub, by its kind), `play` plays a video in the player window where there is one.
#[derive(Debug, Clone)]
pub struct LinkPress<M> {
    pub open: M,
    pub play: Option<M>,
}

/// The card of the link `card` by its kind ([`LinkCardKind`]): a page with its picture (`picture`, fetched by the
/// window through the core), a video with the play mark, a file of the own server, a quoted chat message, a GitHub
/// issue or pull request. `room` wide ([`ContentCard::view`]). Every text on it is foreign: shown, never followed.
///
/// Its buttons only open or play (nothing that writes, so the same card stands in the chat with people), and a
/// click on the card does what its main button does:
///
/// - **Page**: head 🌐 domain · `untrusted`, the picture 3:2 when it has one, the title (the domain without one),
///   the description; "Open".
/// - **Video**: head with the video mark, the picture 16:9 with the play mark, the title; "Play video" where
///   there is a player (`press.play`), then "Open on YouTube".
/// - **File of the own server**: head ☁ server (not `untrusted`: the own server), its preview 3:2, the name, size
///   and folder; "Open in Nextcloud".
/// - **Chat message**: head 💬 conversation · `untrusted`, the author, the start of the message and when;
///   "Open in chat". Without the quote (the reader may not read it) the card is gone.
/// - **GitHub**: head with the GitHub mark and the repository · `untrusted`, "#142 Title", the state with its icon
///   and colour (torro-design: open `CircleDot`/`GitPullRequest` in `ok`, draft neutral, merged `GitMerge` and a
///   done issue `CircleCheck` in `merged`, a closed pull request `GitPullRequestClosed` in `error`, an issue
///   closed as not planned `CircleSlash` neutral); "Open on GitHub".
pub fn link_card<'a, M: Clone + 'a>(
    card: &LinkCard,
    picture: Option<image::Handle>,
    press: LinkPress<M>,
    room: f32,
) -> Element<'a, M> {
    link_content(card, picture, press).view(room)
}

/// The card of the link `card` in one line ([`ContentCard::short`]): its kind's icon, its title (the domain without
/// one) and its first line, the main button as an icon – for the glance of a mini or strip tile, where the card
/// stands for the bare address.
pub fn link_short<'a, M: Clone + 'a>(card: &LinkCard, press: LinkPress<M>) -> Element<'a, M> {
    link_content(card, None, press).short()
}

/// The [`ContentCard`] [`link_card`] draws.
fn link_content<M: Clone>(card: &LinkCard, picture: Option<image::Handle>, press: LinkPress<M>) -> ContentCard<M> {
    match card.kind {
        LinkCardKind::Web => web_card(card, picture, press),
        LinkCardKind::NextcloudFile => {
            let name = card.file.as_ref().map_or_else(|| card.title.clone(), |file| file.name.clone());
            let (bytes, folder) =
                card.file.as_ref().map(|file| (file.size, file.folder.clone())).unwrap_or((0, String::new()));
            let mut built = ContentCard::new(Head::new(Icon::Cloud, card.domain.clone()), name);
            if card.image {
                built = built.picture(Picture { handle: picture, ratio: Ratio::Photo, play: false });
            }
            if bytes > 0 {
                built = built.line(Icon::FileText, units::format_bytes(bytes));
            }
            built
                .line(Icon::Folder, folder)
                .main(Action::new(Icon::ArrowUpRight, tr("Open in Nextcloud"), press.open.clone()))
                .on_press(press.open)
        }
        LinkCardKind::ChatMessage => match &card.quote {
            Some(quote) => ContentCard::new(
                Head::new(Icon::MessageCircle, quote.conversation.clone()).untrusted(),
                quote.author.clone(),
            )
            .line(Icon::TextQuote, quote.text.clone())
            .line(Icon::Clock, datetime::format_moment(quote.at, Utc::now()))
            .main(Action::new(Icon::ArrowUpRight, tr("Open in chat"), press.open.clone()))
            .on_press(press.open),
            None => ContentCard::new(Head::new(Icon::MessageCircle, card.domain.clone()), "").state(State::Gone),
        },
        LinkCardKind::Github => {
            let (repo, title) = match &card.github {
                Some(github) => (github.repo.clone(), format!("#{} {}", github.number, card.title)),
                None => (card.domain.clone(), card.title.clone()),
            };
            let mut built = ContentCard::new(Head::new(Icon::Github, repo).untrusted(), title);
            if let Some(github) = &card.github {
                let (mark, word, tone) = github_state(github.state, github.is_pull);
                built = built.line_toned(mark, word, tone);
            }
            built.main(Action::new(Icon::ExternalLink, tr("Open on GitHub"), press.open.clone())).on_press(press.open)
        }
    }
}

/// A page, or a video when the core found one in the link.
fn web_card<M: Clone>(card: &LinkCard, picture: Option<image::Handle>, press: LinkPress<M>) -> ContentCard<M> {
    let title = if card.title.trim().is_empty() { card.domain.clone() } else { card.title.clone() };
    let mark = if card.video.is_some() { Icon::Video } else { Icon::Globe };
    let built = ContentCard::new(Head::new(mark, card.domain.clone()).untrusted(), title)
        .line(Icon::AlignLeft, card.description.clone().unwrap_or_default());
    if card.video.is_none() {
        let built = match card.image {
            true => built.picture(Picture { handle: picture, ratio: Ratio::Photo, play: false }),
            false => built,
        };
        return built.main(Action::new(Icon::ArrowUpRight, tr("Open"), press.open.clone())).on_press(press.open);
    }
    let built = built.picture(Picture { handle: picture, ratio: Ratio::Video, play: true });
    let youtube = Action::new(Icon::ExternalLink, tr("Open on YouTube"), press.open.clone());
    match press.play {
        Some(play) => built.main(Action::new(Icon::Play, tr("Play video"), play.clone())).more(youtube).on_press(play),
        None => built.main(youtube).on_press(press.open),
    }
}

/// The icon, word and icon colour of a GitHub state (torro-design, section 5, "Icons … für GitHub").
fn github_state(state: GithubState, is_pull: bool) -> (Icon, String, Tone) {
    let ok: fn(&Palette) -> Color = |p| p.ok;
    let merged: fn(&Palette) -> Color = |p| p.merged;
    let error: fn(&Palette) -> Color = |p| p.error;
    let (mark, word, tone) = match (state, is_pull) {
        // An issue is never a draft; an unknown state reads as open.
        (GithubState::Open | GithubState::Draft, false) => (Icon::CircleDot, trc("state", "Open"), Some(ok)),
        (GithubState::Open, true) => (Icon::GitPullRequest, trc("state", "Open"), Some(ok)),
        (GithubState::Draft, true) => (Icon::GitPullRequestDraft, tr("Draft"), None),
        (GithubState::Merged, true) => (Icon::GitMerge, tr("Merged"), Some(merged)),
        (GithubState::Merged, false) => (Icon::CircleCheck, tr("Closed"), Some(merged)),
        (GithubState::Closed, true) => (Icon::GitPullRequestClosed, tr("Closed"), Some(error)),
        (GithubState::Closed, false) => (Icon::CircleSlash, tr("Not planned"), None),
    };
    (mark, word.to_owned(), tone)
}

/// What the three answers of an [`ask_card`] send: show this card once (`assistant.show_card`, `chat.show_card`),
/// always allow the provider, never for it (`settings.set_setting privacy_provider`).
#[derive(Debug, Clone)]
pub struct AskPress<M> {
    pub once: M,
    pub always: M,
    pub never: M,
}

/// The question in place of the card of a page of `provider` (Settings › Privacy at "Ask", or an address the model
/// wrote): "This message shows content from <provider>." with "Show once", "Always allow <provider>" and "Never for
/// <provider>". A card that waits for the person: `raised` with the `line_selected` edge, the sentence in `text_2`
/// after a shield, "Show once" (with the tooltip that the own Nextcloud fetches the page) and "Always allow …"
/// `secondary`, "Never for …" `ghost`; the buttons wrap when the card is narrow. At most `size::list()` wide.
pub fn ask_card<'a, M: Clone + 'a>(provider: &ForeignProvider, press: AskPress<M>) -> Element<'a, M> {
    ask_card_in(provider, press, size::list())
}

/// [`ask_card`] at most `room` wide: in the assistant as wide as the content cards beside it (its reading column).
pub fn ask_card_in<'a, M: Clone + 'a>(provider: &ForeignProvider, press: AskPress<M>, room: f32) -> Element<'a, M> {
    let name = provider.name.as_str();
    let what = trf("This message shows content from {provider}.", &[("provider", &name)]);
    let note = row![
        mark(Icon::Shield, icon::SIZE_SM, None),
        container(typo::text(typo::body(), what).style(theme::text_2)).width(Fill)
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let once = widgets::with_tooltip(
        widgets::text_button(Some(Icon::Eye), tr("Show once"), style::button::secondary, Some(press.once)),
        tr("Your Nextcloud fetches the page, so the site may see when it is read."),
    );
    let always = trf("Always allow {provider}", &[("provider", &name)]);
    let never = trf("Never for {provider}", &[("provider", &name)]);
    let answers = row![
        once,
        widgets::text_button(Some(Icon::Check), always, style::button::secondary, Some(press.always)),
        widgets::text_button(Some(Icon::Ban), never, style::button::ghost, Some(press.never)),
    ]
    .spacing(space::s())
    .wrap()
    .vertical_spacing(space::s());
    container(column![note, answers].spacing(size::card_gap()))
        .padding(layout::panel_padding())
        .width(Fill)
        .max_width(room.max(0.0))
        .style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.raised)),
                text_color: Some(p.text),
                border: border::rounded(radius::lg()).color(p.line_selected).width(HAIRLINE),
                ..container::Style::default()
            }
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_chat::{LinkFile, LinkGithub, LinkQuote, Video};

    fn press() -> LinkPress<&'static str> {
        LinkPress { open: "open", play: Some("play") }
    }

    fn page() -> LinkCard {
        LinkCard {
            url: "https://example.org/calm".into(),
            title: "Designing calm software".into(),
            description: Some("A long read.".into()),
            domain: "example.org".into(),
            image: true,
            ..LinkCard::default()
        }
    }

    fn labels<M>(card: &ContentCard<M>) -> Vec<String> {
        card.main.iter().chain(card.more.iter()).map(|action| action.label.clone()).collect()
    }

    #[test]
    fn a_card_keeps_at_most_three_lines_and_three_more_buttons() {
        let mut card: ContentCard<u8> = ContentCard::new(Head::new(Icon::Mail, "anna@example.org"), "Offer");
        for n in 0..5 {
            card = card.line(Icon::Clock, format!("line {n}")).more(Action::new(Icon::Check, format!("b{n}"), n));
        }
        let card = card.line(Icon::Clock, "  ");
        assert_eq!((card.lines.len(), card.more.len()), (MAX_LINES, MAX_MORE));
        assert_eq!(card.lines[0].text, "line 0", "the first ones stay");
    }

    #[test]
    fn a_priority_line_leads_with_its_dot_and_a_toned_line_with_its_tone() {
        let card: ContentCard<u8> = ContentCard::new(Head::new(Icon::ListTodo, "Work"), "Sort")
            .line_priority(3, "High")
            .line_toned(Icon::TriangleAlert, "Overlaps", Some(|p| p.attention));
        assert!(matches!(card.lines[0].lead, Lead::Priority(3)));
        let Lead::Icon(_, Some(tone)) = card.lines[1].lead else { panic!("a toned icon") };
        assert_eq!(tone(&Palette::dark()), Palette::dark().attention);
    }

    #[test]
    fn a_short_form_keeps_its_first_line_in_its_own_width_and_drops_it_in_a_mini_tile() {
        let from = short_detail_from();
        assert_eq!(short_detail_width("Wed", from - 1.0), None, "a mini tile shows the title alone");
        let own = short_detail_width("Wed", from).expect("room enough");
        assert!(own < from / 4.0, "a short date takes its own width, not a share: {own}");
        let long = "A long description that would take the whole line if it could";
        assert_eq!(short_detail_width(long, 600.0), Some(240.0), "at most two fifths");
        assert_eq!(short_detail_width("  ", 600.0), None);
    }

    #[test]
    fn a_sheet_names_month_day_and_weekday_and_knows_today() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 15).expect("a date");
        let sheet = Sheet::of(day, None, day);
        assert_eq!(sheet.day, 15);
        assert_eq!(sheet.month, datetime::month_name_short(10));
        assert_eq!(sheet.weekday, datetime::weekday_name(day, true));
        assert!(sheet.today);
        assert!(!Sheet::of(day, None, day.succ_opt().expect("a day")).today);
    }

    #[test]
    fn pictures_keep_their_ratio() {
        assert_eq!(Ratio::Photo.height(300.0), 200.0);
        assert_eq!(Ratio::Video.height(320.0), 180.0);
    }

    #[test]
    fn pictures_stand_at_most_the_token_high() {
        let max = size::card_picture_max();
        assert_eq!(Ratio::Photo.shown_height(720.0), max, "a wide card crops its picture");
        assert_eq!(Ratio::Video.shown_height(720.0), max);
        let small = max * 0.75;
        assert_eq!(Ratio::Photo.shown_height(small), small * 2.0 / 3.0, "a narrow one keeps its ratio");
    }

    #[test]
    fn a_page_is_untrusted_has_its_picture_and_only_opens() {
        let card = link_content(&page(), None, press());
        assert!(card.head.untrusted);
        assert_eq!(card.head.source, "example.org");
        assert!(matches!(card.picture, Some(Picture { ratio: Ratio::Photo, play: false, .. })));
        assert_eq!(labels(&card), vec![tr("Open").to_owned()]);
        assert_eq!(card.on_press, Some("open"));
    }

    #[test]
    fn a_page_without_a_title_shows_its_domain() {
        let card = link_content(&LinkCard { title: " ".into(), image: false, ..page() }, None, press());
        assert_eq!(card.title, "example.org");
        assert!(card.picture.is_none());
    }

    #[test]
    fn a_video_plays_where_there_is_a_player_and_else_opens_on_youtube() {
        let video = LinkCard {
            domain: "youtube.com".into(),
            video: Some(Video { id: "dQw4w9WgXcQ".into(), start: None, short: false }),
            ..page()
        };
        let card = link_content(&video, None, press());
        assert!(matches!(card.picture, Some(Picture { ratio: Ratio::Video, play: true, .. })));
        assert_eq!(labels(&card), vec![tr("Play video").to_owned(), tr("Open on YouTube").to_owned()]);
        assert_eq!(card.on_press, Some("play"));
        let card = link_content(&video, None, LinkPress { open: "open", play: None });
        assert_eq!(labels(&card), vec![tr("Open on YouTube").to_owned()]);
        assert_eq!(card.on_press, Some("open"));
    }

    #[test]
    fn a_file_of_the_own_server_shows_size_and_folder_and_is_trusted() {
        let file = LinkCard {
            kind: LinkCardKind::NextcloudFile,
            domain: "cloud.example.org".into(),
            file: Some(Box::new(LinkFile {
                name: "Offer.pdf".into(),
                size: 2_200_000,
                content_type: "application/pdf".into(),
                folder: "Projects/Customer".into(),
            })),
            ..page()
        };
        let card = link_content(&file, None, press());
        assert!(!card.head.untrusted, "the own server");
        assert_eq!(card.title, "Offer.pdf");
        let lines: Vec<&str> = card.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(lines, vec![units::format_bytes(2_200_000).as_str(), "Projects/Customer"]);
        assert_eq!(labels(&card), vec![tr("Open in Nextcloud").to_owned()]);
    }

    #[test]
    fn a_quote_shows_author_and_text_and_is_gone_without_it() {
        let quote = LinkCard {
            kind: LinkCardKind::ChatMessage,
            quote: Some(Box::new(LinkQuote {
                message: "abc/12".into(),
                author: "Ben".into(),
                conversation: "Team".into(),
                at: Utc::now(),
                text: "See #142".into(),
            })),
            ..page()
        };
        let card = link_content(&quote, None, press());
        assert_eq!((card.head.source.as_str(), card.title.as_str()), ("Team", "Ben"));
        assert!(card.head.untrusted);
        assert_eq!(card.lines[0].text, "See #142");
        assert_eq!(labels(&card), vec![tr("Open in chat").to_owned()]);
        let gone = link_content(&LinkCard { quote: None, ..quote }, None, press());
        assert!(matches!(gone.state, State::Gone));
        assert!(gone.main.is_none());
    }

    #[test]
    fn github_shows_number_repository_and_state() {
        let issue = LinkCard {
            kind: LinkCardKind::Github,
            title: "Calendar opens on the wrong day".into(),
            github: Some(Box::new(LinkGithub {
                repo: "torro/workplace".into(),
                number: 142,
                is_pull: false,
                state: GithubState::Open,
            })),
            ..page()
        };
        let card = link_content(&issue, None, press());
        assert_eq!(card.head.source, "torro/workplace");
        assert_eq!(card.title, "#142 Calendar opens on the wrong day");
        assert!(matches!(card.lines[0].lead, Lead::Icon(Icon::CircleDot, Some(_))));
        assert_eq!(card.lines[0].text, trc("state", "Open"));
        assert_eq!(labels(&card), vec![tr("Open on GitHub").to_owned()]);
    }

    #[test]
    fn github_states_follow_the_design() {
        let dark = Palette::dark();
        let cases = [
            (GithubState::Open, false, Icon::CircleDot, trc("state", "Open"), Some(dark.ok)),
            (GithubState::Open, true, Icon::GitPullRequest, trc("state", "Open"), Some(dark.ok)),
            (GithubState::Draft, true, Icon::GitPullRequestDraft, tr("Draft"), None),
            (GithubState::Merged, true, Icon::GitMerge, tr("Merged"), Some(dark.merged)),
            (GithubState::Merged, false, Icon::CircleCheck, tr("Closed"), Some(dark.merged)),
            (GithubState::Closed, true, Icon::GitPullRequestClosed, tr("Closed"), Some(dark.error)),
            (GithubState::Closed, false, Icon::CircleSlash, tr("Not planned"), None),
        ];
        for (state, is_pull, icon, word, colour) in cases {
            let (mark, said, tone) = github_state(state, is_pull);
            assert_eq!((mark, said.as_str()), (icon, word), "{state:?} pull {is_pull}");
            assert_eq!(tone.map(|tone| tone(&dark)), colour, "{state:?} pull {is_pull}");
        }
    }
}
