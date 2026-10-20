//! The Iced widget that shows a [`Content`]: as wide as its parent gives, as tall as the mail. It paints the
//! visible part through [`Content`], passes the mouse on for selecting and clicking, copies the selection with
//! Ctrl+C (⌘C) and polls for pictures while they load.

use std::time::Duration;

use iced::advanced::image::{Image, Renderer as _};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, clipboard};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, keyboard, mouse, window};

use crate::Scheme;
use crate::content::{BROWSER_MARGIN, Content, surface};

/// How often the widget looks for arrived pictures while some are on their way.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// The width a mail is laid out at when the parent sets no limit (a horizontal scroller).
const UNBOUNDED_WIDTH: f32 = 640.0;

/// Shows a [`Content`]: `Length::Fill` wide, as tall as the mail. Put it into a vertical `scrollable`.
///
/// What it does by itself: draws the visible part, shows a hand over links and a text cursor over text,
/// selects text by dragging, copies the selection with Ctrl+C (⌘C on macOS) once the mail was clicked, and
/// draws again when pictures arrive. What it reports: a click on a link through [`HtmlView::on_link`] – the
/// app opens it (only `http`, `https` and `mailto` are reported). Nothing else leaves the widget.
pub struct HtmlView<'a, Message> {
    content: &'a Content,
    scheme: Scheme,
    scale: f32,
    margin: f32,
    on_link: Option<Box<dyn Fn(String) -> Message + 'a>>,
}

impl<'a, Message> HtmlView<'a, Message> {
    /// A view of `content` in [`Scheme::Light`] at scale 1 with the browser's margin, reporting no links.
    pub fn new(content: &'a Content) -> Self {
        Self { content, scheme: Scheme::Light, scale: 1.0, margin: BROWSER_MARGIN, on_link: None }
    }

    /// The colours of the mail (only the mail's, never the rest of the window).
    pub fn scheme(mut self, scheme: Scheme) -> Self {
        self.scheme = scheme;
        self
    }

    /// Device pixels per logical pixel: the window's scale factor times the app's zoom
    /// (`window::scale_factor` × `torro_gui::zoom`). With the wrong value text is blurred (too low) or
    /// painted needlessly large (too high); the layout stays the same.
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The margin at the sides of a mail without a background of its own (none on `html`, on `body`,
    /// nor on a wrapper around the whole mail), in logical pixels, in place of the browser's 8; above
    /// and below it keeps the browser's 8. The app passes the inset
    /// of what stands above the view, so the mail's text starts in line with it. A mail
    /// with a background of its own keeps the browser's margin: its colour fills the view to the
    /// edges and its content places itself. A margin the mail sets on `body` itself always wins.
    pub fn margin(mut self, margin: f32) -> Self {
        self.margin = margin;
        self
    }

    /// The message for a click on a link; it gets the link's absolute URL (`http`, `https` or `mailto`).
    pub fn on_link(mut self, on_link: impl Fn(String) -> Message + 'a) -> Self {
        self.on_link = Some(Box::new(on_link));
        self
    }
}

/// What the widget remembers between frames.
#[derive(Debug, Default)]
struct State {
    /// The mail was clicked last (not something else): Ctrl+C copies its selection.
    focused: bool,
    /// The primary button went down over the mail and is still down.
    dragging: bool,
}

impl<Message> Widget<Message, Theme, Renderer> for HtmlView<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let width = if width.is_finite() { width } else { UNBOUNDED_WIDTH };
        self.content.set_margin(self.margin);
        let height = self.content.height(width, self.scale, self.scheme);
        layout::Node::new(limits.resolve(Length::Fill, Length::Shrink, Size::new(width, height)))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let local = |position: iced::Point| position - bounds.position();
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                if self.content.poll() {
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                if self.content.is_loading() {
                    shell.request_redraw_at(*now + POLL_EVERY);
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => match cursor.position_in(bounds) {
                Some(point) => {
                    state.focused = true;
                    state.dragging = true;
                    self.content.press(point);
                    shell.request_redraw();
                }
                None => state.focused = false,
            },
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(position) = cursor.position() {
                        self.content.drag(iced::Point::ORIGIN + local(position));
                        shell.request_redraw();
                    }
                } else if let Some(point) = cursor.position_in(bounds) {
                    self.content.hover(point);
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                let point =
                    cursor.position().map_or(iced::Point::ORIGIN, |position| iced::Point::ORIGIN + local(position));
                let link = self.content.release(point);
                if let (Some(link), Some(on_link)) = (link, &self.on_link) {
                    shell.publish(on_link(link));
                }
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Character(c), modifiers, .. })
                if state.focused && modifiers.command() && c.as_str() == "c" =>
            {
                if let Some(text) = self.content.selected_text() {
                    clipboard.write(clipboard::Kind::Standard, text);
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else { return };
        renderer.fill_quad(renderer::Quad { bounds: visible, ..renderer::Quad::default() }, surface(self.scheme));
        let top = visible.y - bounds.y;
        self.content.set_margin(self.margin);
        let Some(frame) = self.content.frame(bounds.width, self.scale, self.scheme, top, visible.height) else {
            return;
        };
        for tile in frame.tiles {
            let at = Rectangle { x: bounds.x, y: bounds.y + tile.top, width: frame.width, height: tile.height };
            if at.intersection(&visible).is_some() {
                renderer.draw_image(Image { snap: true, ..Image::new(tile.handle) }, at, visible);
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().dragging {
            return mouse::Interaction::Text;
        }
        if cursor.is_over(layout.bounds()) { self.content.cursor() } else { mouse::Interaction::None }
    }
}

impl<'a, Message: 'a> From<HtmlView<'a, Message>> for Element<'a, Message, Theme, Renderer> {
    fn from(view: HtmlView<'a, Message>) -> Self {
        Element::new(view)
    }
}
