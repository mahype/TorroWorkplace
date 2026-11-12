//! How the ringing window looks (draft "Als kleines Fenster in der Mitte" in
//! `docs/prototypes/chat/`): the top row "Incoming call"; in the middle the caller's face, name and
//! "is calling…"; under them "Decline" and "Accept" side by side, "Audio only" below. "Accept" is the
//! window's one primary button. Mini and strip show the name and "Accept".

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row};
use iced::{Alignment, Background, Element, Fill, Theme, border};
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, TopRow};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, ButtonStyle};
use torro_gui::{style, typo};

use super::{hue, tinted};
use crate::app::ring::{RingBusy, RingMsg, RingState};
use crate::app::{App, Message};
use crate::people;

/// A button of the decision, [`size::control_lg()`] high and as wide as its share of the row: the
/// large buttons of a decision in a small window (torro-design, "Knopfleisten auf Karten").
fn decision<'a>(which: Icon, label: &'a str, style: ButtonStyle, on_press: Option<Message>) -> Element<'a, Message> {
    let tone = move |t: &Theme| iced::widget::svg::Style { color: Some(style(t, button::Status::Active).text_color) };
    let content = row![
        icon::icon(which, icon::SIZE).style(move |t, _| tone(t)),
        typo::text(typo::body_strong(), label).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    button(container(content).center(Fill))
        .width(Fill)
        .height(size::control_lg())
        .padding([0.0, space::m()])
        .style(style)
        .on_press_maybe(on_press)
        .into()
}

impl App {
    /// The ringing window.
    pub(crate) fn ring_window<'a>(&'a self, ring: &'a RingState) -> Element<'a, Message> {
        let mut header = row![].spacing(space::s()).width(Fill).align_y(Alignment::Center);
        if !cfg!(target_os = "macos") {
            header = header.push(shell::app_mark(torro_core::AppId::CHAT));
        }
        let header = header.push(widgets::ellipsized(typo::body_strong(), tr("Incoming call"), theme::text));
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow);
        let name = self.caller(ring);
        let declining = ring.busy == Some(RingBusy::Declining);
        let ready = ring.link().is_some() && !declining;
        let decline_label = if declining { tr("Declining…") } else { tr("Decline") };
        let decline = decision(
            Icon::X,
            decline_label,
            style::button::secondary,
            (!declining).then_some(Message::Ring(RingMsg::Decline)),
        );
        let accept = decision(
            Icon::Video,
            tr("Accept"),
            style::button::primary,
            ready.then_some(Message::Ring(RingMsg::Accept)),
        );
        let audio = widgets::with_tooltip(
            widgets::text_button(
                None,
                tr("Audio only"),
                style::button::ghost,
                ready.then_some(Message::Ring(RingMsg::Accept)),
            ),
            tr("Opens the call in your browser; turn the camera off there"),
        );
        let mut content = column![
            self.ring_face(ring, &name),
            // A long name breaks onto a second line, in the middle like the rest.
            typo::text(typo::title(), name).wrapping(Wrapping::WordOrGlyph).align_x(Alignment::Center),
            typo::text(typo::meta(), tr("is calling…")).style(theme::text_3),
            Space::new().height(space::l()),
            row![decline, accept].spacing(space::s()).width(Fill),
            audio,
        ]
        .spacing(space::s())
        .align_x(Alignment::Center)
        .width(Fill);
        if let Some(error) = &ring.error {
            let words = typo::text(typo::meta(), error.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
            content = content.push(
                row![tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text), words]
                    .spacing(space::s())
                    .align_y(Alignment::Center),
            );
        }
        let body = container(content).width(Fill).height(Fill).padding(space::xl()).center(Fill);
        shell::shell(SizeClass::Narrow, Space::new(), header, body, None).top_row(top).into()
    }

    /// The caller's face, large: their initials on their colour, tinted as every face of the chat.
    fn ring_face<'a>(&self, ring: &RingState, name: &str) -> Element<'a, Message> {
        let id = self
            .conversations
            .iter()
            .find(|conversation| conversation.id == ring.conversation)
            .and_then(|conversation| conversation.person.clone())
            .unwrap_or_else(|| ring.conversation.clone());
        let colour = hue(&self.colours(), &id);
        let edge = 2.0 * size::avatar();
        let letters = typo::text(typo::title(), people::initials(name)).wrapping(Wrapping::None);
        container(letters)
            .center(edge)
            .style(move |t: &Theme| {
                let p = palette(t);
                container::Style {
                    background: Some(Background::Color(p.tinted(colour))),
                    text_color: Some(p.text),
                    border: border::rounded(edge / 2.0),
                    ..container::Style::default()
                }
            })
            .into()
    }

    /// The ringing window as a mini or strip tile: who calls, and "Accept".
    pub(crate) fn ring_glance<'a>(&'a self, ring: &'a RingState, class: SizeClass) -> Element<'a, Message> {
        let name = widgets::ellipsized(typo::body_strong(), self.caller(ring), theme::text);
        let line = widgets::ellipsized(typo::meta(), tr("is calling…"), theme::text_3);
        let accept = widgets::text_button(
            Some(Icon::Video),
            tr("Accept"),
            style::button::primary,
            ring.link().is_some().then_some(Message::Ring(RingMsg::Accept)),
        );
        let pad = if class == SizeClass::Strip { [space::s(), space::m()] } else { [space::m(), space::m()] };
        let content = column![name, line, accept].spacing(space::xs());
        container(content).width(Fill).height(Fill).padding(pad).center_y(Fill).into()
    }
}
