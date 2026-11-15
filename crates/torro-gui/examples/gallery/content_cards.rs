//! The gallery's page `--content-cards` (issue #785): every kind and state of `torro_gui::content_card` – the link
//! cards of a page, a video, a file of the own server, a quoted chat message and GitHub in each state, the states of
//! a card (loading, not available, no longer available), the question of a provider at "Ask" and the short forms.
//!
//! Mini and strip show the short forms, as those tiles would; every other size the cards, as many side by side as
//! fit. The pictures are drawn here, so the page needs no network. Clicks do nothing.

use std::sync::LazyLock;

use chrono::{Duration, Utc};
use iced::widget::{Column, column, container, image, responsive, row, rule, scrollable};
use iced::{Element, Fill, Size};
use torro_chat::{ForeignProvider, GithubState, LinkCard, LinkCardKind, LinkFile, LinkGithub, LinkQuote, Video};
use torro_gui::content_card::{self, Action, AskPress, ContentCard, Head, LinkPress, Picture, Ratio, State};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, size, space};
use torro_gui::{style, typo};

use super::Msg;

/// A page's picture: a calm gradient with a sun, 3:2.
static PHOTO: LazyLock<image::Handle> = LazyLock::new(|| picture(480, 320, [0x3B, 0x6E, 0xF0], [0xC2, 0x50, 0x8F]));

/// A video's still: a dark gradient, 16:9.
static STILL: LazyLock<image::Handle> = LazyLock::new(|| picture(480, 270, [0x1F, 0x2A, 0x44], [0x3F, 0xB9, 0x84]));

/// A file's preview: a light page, 3:2.
static PREVIEW: LazyLock<image::Handle> = LazyLock::new(|| picture(480, 320, [0xE9, 0xEA, 0xED], [0xB4, 0xB8, 0xBF]));

/// A `width` × `height` picture running from `from` at the top left to `to` at the bottom right, with a lighter
/// disc in its upper third.
fn picture(width: u32, height: u32, from: [u8; 3], to: [u8; 3]) -> image::Handle {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let (cx, cy, r) = (width as f32 * 0.7, height as f32 * 0.35, height as f32 * 0.16);
    for y in 0..height {
        for x in 0..width {
            let t = (x as f32 / width as f32 + y as f32 / height as f32) / 2.0;
            let sun = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < r) as u8 as f32 * 0.35;
            for (a, b) in from.iter().zip(to) {
                let base = *a as f32 + (b as f32 - *a as f32) * t;
                pixels.push((base + (255.0 - base) * sun) as u8);
            }
            pixels.push(255);
        }
    }
    image::Handle::from_rgba(width, height, pixels)
}

fn press() -> LinkPress<Msg> {
    LinkPress { open: Msg::Noop, play: Some(Msg::Noop) }
}

fn page_card(title: &str, description: Option<&str>, image: bool) -> LinkCard {
    LinkCard {
        url: "https://example.org/calm".into(),
        title: title.into(),
        description: description.map(str::to_owned),
        domain: "example.org".into(),
        image,
        ..LinkCard::default()
    }
}

fn video_card() -> LinkCard {
    LinkCard {
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        title: "Quiet interfaces in practice".into(),
        description: Some("12:41 · Example Channel".into()),
        domain: "youtube.com".into(),
        image: true,
        video: Some(Video { id: "dQw4w9WgXcQ".into(), start: None, short: false }),
        ..LinkCard::default()
    }
}

fn file_card(name: &str, bytes: u64, image: bool) -> LinkCard {
    LinkCard {
        url: "https://cloud.example.org/f/4711".into(),
        title: name.into(),
        domain: "cloud.example.org".into(),
        image,
        kind: LinkCardKind::NextcloudFile,
        file: Some(Box::new(LinkFile {
            name: name.into(),
            size: bytes,
            content_type: String::new(),
            folder: "Projects/Customer".into(),
        })),
        ..LinkCard::default()
    }
}

fn quote_card(quote: bool) -> LinkCard {
    LinkCard {
        url: "https://cloud.example.org/call/abc#message_12".into(),
        domain: "cloud.example.org".into(),
        kind: LinkCardKind::ChatMessage,
        quote: quote.then(|| {
            Box::new(LinkQuote {
                message: "abc/12".into(),
                author: "Ben Weber".into(),
                conversation: "Team Workplace".into(),
                at: Utc::now() - Duration::minutes(42),
                text: "See #142 and the pull request, the offer is attached.".into(),
            })
        }),
        ..LinkCard::default()
    }
}

fn github_card(number: u64, title: &str, is_pull: bool, state: GithubState) -> LinkCard {
    let kind = if is_pull { "pull" } else { "issues" };
    LinkCard {
        url: format!("https://github.com/torro/workplace/{kind}/{number}"),
        title: title.into(),
        domain: "github.com".into(),
        kind: LinkCardKind::Github,
        github: Some(Box::new(LinkGithub { repo: "torro/workplace".into(), number, is_pull, state })),
        ..LinkCard::default()
    }
}

/// A mail as the assistant shows it, with every part: three lines, the main button and three more.
fn mail(state: State<Msg>) -> ContentCard<Msg> {
    ContentCard::new(Head::new(Icon::Mail, "Mail · anna@example.org").untrusted(), "API review notes")
        .line(Icon::User, "Anna Berg · Thu, 16 Oct")
        .line(Icon::AlignLeft, "Hi Sven, here are my notes for Thursday. The open points are …")
        .line(Icon::Paperclip, "notes.pdf · 240 KB")
        .main(Action::new(Icon::ArrowUpRight, "Open in mail", Msg::Noop))
        .more(Action::new(Icon::Reply, "Reply", Msg::Noop))
        .more(Action::new(Icon::Archive, "Archive", Msg::Noop))
        .more(Action::new(Icon::ListTodo, "Task", Msg::Noop))
        .state(state)
        .on_press(Msg::Noop)
}

/// A section of the page, to show alone with `--open <name>` (so a picture of a narrow window holds all of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Pages,
    Server,
    Github,
    States,
    Question,
    Short,
}

impl Part {
    /// The names `--open` takes.
    pub const NAMES: &str = "pages, server, github, states, question, short";

    /// The part `name` names.
    pub fn parse(name: &str) -> Option<Part> {
        match name {
            "pages" => Some(Part::Pages),
            "server" => Some(Part::Server),
            "github" => Some(Part::Github),
            "states" => Some(Part::States),
            "question" => Some(Part::Question),
            "short" => Some(Part::Short),
            _ => None,
        }
    }
}

/// The page: the short forms in mini and strip, else the cards by section; `only` one section of them.
pub fn page<'a>(only: Option<Part>) -> Element<'a, Msg> {
    responsive(move |room: Size| match layout::of(room) {
        SizeClass::Mini | SizeClass::Strip => shorts(),
        _ => cards(room.width, only),
    })
    .into()
}

/// The cards by section (`only` one of them), `width` the room of the page.
fn cards<'a>(width: f32, only: Option<Part>) -> Element<'a, Msg> {
    let pad = layout::page_padding();
    let room = (width - pad.left - pad.right).max(0.0);
    let link = |card: LinkCard, picture: Option<&image::Handle>| {
        content_card::link_card(&card, picture.cloned(), press(), room)
    };
    let no_player =
        |card: LinkCard| content_card::link_card(&card, None, LinkPress { open: Msg::Noop, play: None }, room);
    let pages = vec![
        link(
            page_card("Designing calm software", Some("A long read on interfaces that stay out of the way."), true),
            Some(&PHOTO),
        ),
        link(page_card("Designing calm software", Some("The picture is on its way."), true), None),
        link(
            page_card(
                "A very long title of a page that does not fit on one line of the card at all, so it is cut",
                Some("No picture, a long title cut with an ellipsis."),
                false,
            ),
            None,
        ),
        link(page_card("", None, false), None),
        link(video_card(), Some(&STILL)),
        no_player(LinkCard { image: false, ..video_card() }),
    ];
    let own = vec![
        link(file_card("Offer 2026-114.pdf", 1_468_006, true), Some(&PREVIEW)),
        link(file_card("Budget 2027.ods", 88_000, false), None),
        link(quote_card(true), None),
        link(quote_card(false), None),
    ];
    let github = vec![
        link(github_card(142, "Calendar opens on the wrong day", false, GithubState::Open), None),
        link(github_card(138, "Sync stops after sleep", false, GithubState::Merged), None),
        link(github_card(131, "Dark mode for the tray", false, GithubState::Closed), None),
        link(github_card(143, "Fix the week view", true, GithubState::Open), None),
        link(github_card(144, "Content cards", true, GithubState::Draft), None),
        link(github_card(140, "Move the settings", true, GithubState::Merged), None),
        link(github_card(139, "Try another renderer", true, GithubState::Closed), None),
    ];
    let page_loading = ContentCard::new(Head::new(Icon::Globe, "example.org").untrusted(), "")
        .picture(Picture { handle: None, ratio: Ratio::Photo, play: false })
        .line(Icon::AlignLeft, "…")
        .state(State::Loading);
    let states = vec![
        mail(State::Ready).view(room),
        mail(State::Loading).view(room),
        page_loading.view(room),
        mail(State::Failed { retry: Some(Msg::Noop) }).view(room),
        mail(State::Failed { retry: None }).view(room),
        mail(State::Gone).view(room),
    ];
    let provider = ForeignProvider { key: "example.org".into(), name: "example.org".into() };
    let ask =
        vec![content_card::ask_card(&provider, AskPress { once: Msg::Noop, always: Msg::Noop, never: Msg::Noop })];
    let all = [
        (Part::Pages, section("Pages and videos", pages)),
        (Part::Server, section("Own server", own)),
        (Part::Github, section("GitHub", github)),
        (Part::States, section("States", states)),
        (Part::Question, section("Question (Settings › Privacy at Ask)", ask)),
        (Part::Short, column![heading("Short forms"), short_list(size::content_max())].spacing(space::s()).into()),
    ];
    let shown = all.into_iter().filter(|(part, _)| only.is_none_or(|only| only == *part)).map(|(_, section)| section);
    let sections = Column::with_children(shown).spacing(layout::section_gap()).padding(pad);
    scrollable(sections).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable).into()
}

/// A section: its title in `CAPTION`, `text_3`, then its cards side by side, wrapping.
fn section<'a>(title: &'a str, cards: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let cards = row(cards).spacing(size::card_gap()).wrap().vertical_spacing(size::card_gap());
    column![heading(title), cards].spacing(space::s()).into()
}

fn heading<'a>(title: &'a str) -> Element<'a, Msg> {
    typo::text(typo::caption(), title).style(theme::text_3).into()
}

/// The short forms alone, as a mini or strip tile shows them.
fn shorts<'a>() -> Element<'a, Msg> {
    let list = container(short_list(size::content_max())).padding(space::m());
    scrollable(list).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable).into()
}

/// Short forms of every kind, one line each with a hairline between, at most `max` wide.
fn short_list<'a>(max: f32) -> Element<'a, Msg> {
    let shorts = [
        mail(State::Ready),
        ContentCard::new(Head::new(Icon::Mail, "Mail"), "Jonas Klein · Re: Offer")
            .line(Icon::Clock, "Wed")
            .main(Action::new(Icon::ArrowUpRight, "Open in mail", Msg::Noop)),
        ContentCard::new(Head::new(Icon::Github, "torro/workplace"), "#142 Calendar opens on the wrong day")
            .line(Icon::CircleDot, "Open")
            .main(Action::new(Icon::ExternalLink, "Open on GitHub", Msg::Noop)),
        mail(State::Loading),
        mail(State::Failed { retry: Some(Msg::Noop) }),
        mail(State::Gone),
    ];
    let mut list = Column::new().width(Fill).max_width(max);
    for (i, card) in shorts.into_iter().enumerate() {
        if i > 0 {
            list = list.push(rule::horizontal(1).style(style::rule));
        }
        list = list.push(card.short());
    }
    list.into()
}
