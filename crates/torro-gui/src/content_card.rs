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
//! - **Picture** ([`Picture`]): at most one, 3:2 ([`Ratio::Photo`]) or 16:9 ([`Ratio::Video`], with the play mark);
//!   without its pixels yet the kind's icon stands in its place, so the card does not jump.
//! - **Title**: one line, cut with "…".
//! - **Lines** ([`ContentCard::line`]): up to [`MAX_LINES`] with an icon each (place, time, sender …), the most
//!   important first.
//! - **Buttons**: one main button ([`ContentCard::main`], "Open …") and up to [`MAX_MORE`] more
//!   ([`ContentCard::more`]); with their words where the card has room, else icons with the word as tooltip.
//! - **States** ([`State`]): loading (a quiet placeholder the size of the card), not available right now (head and
//!   source stay, "Try again"), no longer available (gone or out of reach, no button).
//!
//! [`link_card`] draws the card of a link ([`LinkCard`]) by its kind, [`ask_card`] the question whether a provider's
//! content may load (Settings › Privacy at "Ask"). In the chat with people only buttons that write nothing.
//!
//! Strand 2a of issue #784 gives these their final look (the gallery's `--content-cards`); the API stays as it is.

use iced::widget::text::Wrapping;
use iced::widget::{Row, column, container, image, row, stack, svg};
use iced::{Alignment, Background, ContentFit, Element, Fill, Theme, border};
use torro_chat::{ForeignProvider, GithubState, LinkCard, LinkCardKind};
use torro_core::i18n::{tr, trf};

use crate::icon::{self, Icon, icon};
use crate::theme::{self, palette, radius, size, space};
use crate::{layout, style, typo, widgets};

/// At most this many lines under the title ([`ContentCard::line`]); more are left out.
pub const MAX_LINES: usize = 3;

/// At most this many buttons besides the main one ([`ContentCard::more`]); more are left out.
pub const MAX_MORE: usize = 3;

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
}

impl Head {
    /// A head with `mark` and `source`, not marked untrusted.
    pub fn new(mark: Icon, source: impl Into<String>) -> Self {
        Self { mark, source: source.into(), untrusted: false }
    }

    /// The same head, marked `untrusted`.
    pub fn untrusted(self) -> Self {
        Self { untrusted: true, ..self }
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
    /// The picture's height for `width`.
    pub fn height(self, width: f32) -> f32 {
        match self {
            Ratio::Photo => width * 2.0 / 3.0,
            Ratio::Video => width * 9.0 / 16.0,
        }
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
}

impl<M> Action<M> {
    /// A button with `icon` and `label` that sends `on_press`.
    pub fn new(icon: Icon, label: impl Into<String>, on_press: M) -> Self {
        Self { icon, label: label.into(), on_press: Some(on_press) }
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
    title: String,
    lines: Vec<(Icon, String)>,
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

    /// One more line under the title, with `mark`; beyond [`MAX_LINES`] left out. An empty `text` is left out too.
    pub fn line(mut self, mark: Icon, text: impl Into<String>) -> Self {
        let text = text.into();
        if self.lines.len() < MAX_LINES && !text.trim().is_empty() {
            self.lines.push((mark, text));
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

    /// The full card, at most `room` wide (and never wider than `size::list()`).
    pub fn view<'a>(self, room: f32) -> Element<'a, M>
    where
        M: 'a,
    {
        let width = room.min(size::list());
        let inner = (width - layout::panel_padding().left - layout::panel_padding().right).max(0.0);
        let mut parts: Vec<Element<'a, M>> = vec![head_line(&self.head)];
        match self.state {
            State::Ready => {
                if let Some(picture) = self.picture {
                    parts.push(picture_frame(picture, self.head.mark, inner));
                }
                parts.push(widgets::ellipsized(typo::body_strong(), self.title, theme::text));
                for (mark, text) in self.lines {
                    parts.push(detail_line(mark, text));
                }
                let mut actions: Vec<Action<M>> = self.main.into_iter().collect();
                actions.extend(self.more);
                if !actions.is_empty() {
                    parts.push(buttons(actions, inner));
                }
            }
            State::Loading => parts.push(quiet(tr("Loading…"))),
            State::Failed { retry } => {
                parts.push(quiet(tr("Not available right now")));
                if let Some(retry) = retry {
                    parts.push(widgets::text_button(
                        Some(Icon::RefreshCw),
                        tr("Try again"),
                        style::button::secondary,
                        Some(retry),
                    ));
                }
            }
            State::Gone => parts.push(quiet(tr("No longer available"))),
        }
        let card = widgets::card(parts, false, self.on_press);
        container(card).width(width).into()
    }

    /// The card in one line ("Kurzform"), `size::control_lg()` high: the kind's icon, the title and its first line,
    /// and the main button as an icon with its word as tooltip. For mini and strip tiles and lists of four to ten.
    pub fn short<'a>(self) -> Element<'a, M>
    where
        M: 'a,
    {
        let (title, detail) = match self.state {
            State::Ready => (self.title, self.lines.into_iter().next().map(|(_, text)| text)),
            State::Loading => (self.title, Some(tr("Loading…").to_owned())),
            State::Failed { .. } => (self.head.source.clone(), Some(tr("Not available right now").to_owned())),
            State::Gone => (tr("No longer available").to_owned(), None),
        };
        let mut line = row![
            mark(self.head.mark, icon::SIZE),
            container(widgets::ellipsized(typo::body(), title, theme::text)).width(Fill)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        if let Some(detail) = detail {
            // Title and detail share the line; each is cut with "…" rather than pushing the button out.
            line = line.push(container(widgets::ellipsized(typo::meta(), detail, theme::text_3)).width(Fill));
        }
        if let Some(main) = self.main {
            line = line.push(widgets::icon_button(main.icon, main.label, main.on_press));
        }
        container(line).height(size::control_lg()).center_y(size::control_lg()).width(Fill).into()
    }
}

/// The head: the kind's icon, the source, and `untrusted` when it is.
fn head_line<'a, M: 'a>(head: &Head) -> Element<'a, M> {
    let mut line = row![
        mark(head.mark, icon::SIZE_SM),
        container(widgets::ellipsized(typo::meta(), head.source.clone(), theme::text_3)).width(Fill)
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    if head.untrusted {
        line = line.push(typo::text(typo::meta(), tr("untrusted")).style(theme::text_3).wrapping(Wrapping::None));
    }
    line.into()
}

/// A line under the title: its icon and its text, one line.
fn detail_line<'a, M: 'a>(which: Icon, text: String) -> Element<'a, M> {
    row![mark(which, icon::SIZE_SM), container(widgets::ellipsized(typo::meta(), text, theme::text_2)).width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// A quiet sentence in place of the content (loading, not available, gone).
fn quiet<'a, M: 'a>(text: &str) -> Element<'a, M> {
    typo::text(typo::meta(), text.to_owned()).style(theme::text_3).into()
}

/// An icon in `text_3`.
fn mark<'a, M: 'a>(which: Icon, edge: f32) -> Element<'a, M> {
    icon(which, edge).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) }).into()
}

/// The picture `width` wide in its ratio; without its pixels the kind's icon on a quiet surface; a video with the
/// play mark over it.
fn picture_frame<'a, M: 'a>(picture: Picture, kind: Icon, width: f32) -> Element<'a, M> {
    let height = picture.ratio.height(width);
    let shown: Element<'a, M> = match picture.handle {
        Some(handle) => {
            image(handle).width(width).height(height).content_fit(ContentFit::Cover).border_radius(radius::sm()).into()
        }
        None => container(mark(kind, icon::SIZE))
            .center(Fill)
            .width(width)
            .height(height)
            .style(|t: &Theme| container::Style {
                background: Some(Background::Color(palette(t).selected)),
                border: border::rounded(radius::sm()),
                ..container::Style::default()
            })
            .into(),
    };
    if !picture.play {
        return shown;
    }
    stack![shown, container(mark(Icon::Play, icon::SIZE)).center(Fill)].width(width).height(height).into()
}

/// The buttons of a card in one row: the main one `secondary`, the others `ghost`; with words where they fit `room`,
/// else icons with the word as tooltip (the main one keeps its word first, `widgets::bar_words`).
fn buttons<'a, M: Clone + 'a>(actions: Vec<Action<M>>, room: f32) -> Element<'a, M> {
    let labels: Vec<&str> = actions.iter().map(|action| action.label.as_str()).collect();
    let words = widgets::bar_words(&labels, true, 0, room);
    let buttons = actions.into_iter().enumerate().map(|(i, action)| {
        let style = if i == 0 { style::button::secondary } else { style::button::ghost };
        match words.worded(i) {
            true => widgets::text_button(Some(action.icon), action.label, style, action.on_press),
            false => widgets::icon_button(action.icon, action.label, action.on_press),
        }
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
/// issue or pull request. At most `room` wide. Every text on it is foreign: shown, never followed.
///
/// Strand 2a of issue #784 gives each kind its final parts (the GitHub state's icon and colour after `torro-design`,
/// the file's size and folder); this draws them simply.
pub fn link_card<'a, M: Clone + 'a>(
    card: &LinkCard,
    picture: Option<image::Handle>,
    press: LinkPress<M>,
    room: f32,
) -> Element<'a, M> {
    let built = match card.kind {
        LinkCardKind::Web => {
            let title = if card.title.trim().is_empty() { card.domain.clone() } else { card.title.clone() };
            let mut built = ContentCard::new(Head::new(Icon::Globe, card.domain.clone()).untrusted(), title)
                .line(Icon::AlignLeft, card.description.clone().unwrap_or_default());
            if card.video.is_some() {
                built = built.picture(Picture { handle: picture, ratio: Ratio::Video, play: true });
                if let Some(play) = press.play {
                    built = built.main(Action::new(Icon::Play, tr("Play video"), play));
                }
                built.more(Action::new(Icon::ExternalLink, tr("Open on YouTube"), press.open))
            } else {
                if card.image {
                    built = built.picture(Picture { handle: picture, ratio: Ratio::Photo, play: false });
                }
                built.main(Action::new(Icon::ArrowUpRight, tr("Open"), press.open))
            }
        }
        LinkCardKind::NextcloudFile => {
            let name = card.file.as_ref().map_or_else(|| card.title.clone(), |file| file.name.clone());
            let folder = card.file.as_ref().map(|file| file.folder.clone()).unwrap_or_default();
            let mut built =
                ContentCard::new(Head::new(Icon::Cloud, card.domain.clone()), name).line(Icon::Folder, folder);
            if card.image {
                built = built.picture(Picture { handle: picture, ratio: Ratio::Photo, play: false });
            }
            built.main(Action::new(Icon::ArrowUpRight, tr("Open in Nextcloud"), press.open))
        }
        LinkCardKind::ChatMessage => match &card.quote {
            Some(quote) => ContentCard::new(
                Head::new(Icon::MessageCircle, quote.conversation.clone()).untrusted(),
                quote.author.clone(),
            )
            .line(Icon::TextQuote, quote.text.clone())
            .main(Action::new(Icon::ArrowUpRight, tr("Open in chat"), press.open)),
            None => ContentCard::new(Head::new(Icon::MessageCircle, card.domain.clone()), "").state(State::Gone),
        },
        LinkCardKind::Github => {
            let (repo, title) = match &card.github {
                Some(github) => (github.repo.clone(), format!("#{} {}", github.number, card.title)),
                None => (card.domain.clone(), card.title.clone()),
            };
            let state = card.github.as_ref().map(|github| github_state(github.state, github.is_pull));
            let mut built = ContentCard::new(Head::new(Icon::Github, repo).untrusted(), title);
            if let Some((mark, word)) = state {
                built = built.line(mark, word);
            }
            built.main(Action::new(Icon::ExternalLink, tr("Open on GitHub"), press.open))
        }
    };
    built.view(room)
}

/// The icon and word of a GitHub state ("Open", "Merged" …).
fn github_state(state: GithubState, is_pull: bool) -> (Icon, String) {
    let (mark, word) = match (state, is_pull) {
        (GithubState::Open, false) => (Icon::CircleDot, tr("Open")),
        (GithubState::Open, true) => (Icon::GitPullRequest, tr("Open")),
        (GithubState::Draft, _) => (Icon::GitPullRequestDraft, tr("Draft")),
        (GithubState::Merged, true) => (Icon::GitMerge, tr("Merged")),
        (GithubState::Merged, false) => (Icon::CircleCheck, tr("Closed")),
        (GithubState::Closed, true) => (Icon::GitPullRequestClosed, tr("Closed")),
        (GithubState::Closed, false) => (Icon::CircleX, tr("Closed")),
    };
    (mark, word.to_owned())
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
/// <provider>". Quiet: the sentence in `text_2`, ghost buttons.
pub fn ask_card<'a, M: Clone + 'a>(provider: &ForeignProvider, press: AskPress<M>) -> Element<'a, M> {
    let name = provider.name.as_str();
    let what = trf("This message shows content from {provider}.", &[("provider", &name)]);
    let once = widgets::with_tooltip(
        widgets::text_button(Some(Icon::Eye), tr("Show once"), style::button::ghost, Some(press.once)),
        tr("Your Nextcloud fetches the page, so the site may see when it is read."),
    );
    let always = trf("Always allow {provider}", &[("provider", &name)]);
    let never = trf("Never for {provider}", &[("provider", &name)]);
    let answers = row![
        once,
        widgets::text_button(None, always, style::button::ghost, Some(press.always)),
        widgets::text_button(None, never, style::button::ghost, Some(press.never)),
    ]
    .spacing(space::xs())
    .wrap();
    column![typo::text(typo::meta(), what).style(theme::text_2), answers].spacing(space::xs()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_keeps_at_most_three_lines_and_three_more_buttons() {
        let mut card: ContentCard<u8> = ContentCard::new(Head::new(Icon::Mail, "anna@example.org"), "Offer");
        for n in 0..5 {
            card = card.line(Icon::Clock, format!("line {n}")).more(Action::new(Icon::Check, format!("b{n}"), n));
        }
        let card = card.line(Icon::Clock, "  ");
        assert_eq!((card.lines.len(), card.more.len()), (MAX_LINES, MAX_MORE));
        assert_eq!(card.lines[0].1, "line 0", "the first ones stay");
    }

    #[test]
    fn pictures_keep_their_ratio() {
        assert_eq!(Ratio::Photo.height(300.0), 200.0);
        assert_eq!(Ratio::Video.height(320.0), 180.0);
    }
}
