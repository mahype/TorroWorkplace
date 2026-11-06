//! Content that opens a context menu where it is right-clicked ([`super::context_area`], issue #497):
//! Iced's `mouse_area` only says that the right button went down, not where, so a menu could only
//! hang beside the whole row. This wrapper hands the point of the click to the message, relative to
//! the content's top-left corner, and the menu floats there with [`super::Placement::At`].
//!
//! It reports the point only on the right press, never on mouse moves: tracking the pointer would
//! rebuild the view on every move.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// `content` that sends `on_right_press(point)` when it is right-clicked.
pub(super) struct ContextArea<'a, M> {
    pub(super) content: Element<'a, M>,
    pub(super) on_right_press: Box<dyn Fn(Point) -> M + 'a>,
}

impl<M> Widget<M, Theme, Renderer> for ContextArea<'_, M> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        // The content first, as Iced's `mouse_area` does: a button inside the row that takes the
        // right press keeps it.
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(point) = cursor.position_in(layout.bounds())
        {
            shell.publish((self.on_right_press)(point));
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, M: 'a> From<ContextArea<'a, M>> for Element<'a, M> {
    fn from(area: ContextArea<'a, M>) -> Self {
        Element::new(area)
    }
}
