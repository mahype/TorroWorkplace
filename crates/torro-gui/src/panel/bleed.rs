//! Content that reaches past the side inset of the panel's body to the panel's edges ([`super::bleed`]):
//! a mail's HTML, whose own background should fill the panel from edge to edge as in a mail app,
//! while the rest of the body keeps its inset.
//!
//! Iced has no negative padding, so this wrapper lays its content out wider than itself and moves it
//! left by the inset. Nothing clips it there: the panel's scrollable spans the inset, too.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// `content` widened by `by` on both sides.
pub(super) struct Bleed<'a, M> {
    pub(super) content: Element<'a, M>,
    pub(super) by: f32,
}

/// The width the content is laid out at and where it starts, for a wrapper `width` wide that bleeds
/// `by` into each side: `by` wider on both sides, starting `by` to the left. An unbounded width stays
/// unbounded.
pub(super) fn spread(width: f32, by: f32) -> (f32, f32) {
    if width.is_finite() { (width + 2.0 * by, -by) } else { (width, 0.0) }
}

impl<M> Widget<M, Theme, Renderer> for Bleed<'_, M> {
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
        Size::new(Length::Fill, self.content.as_widget().size().height)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let (wide, x) = spread(width, self.by);
        let inner = layout::Limits::new(Size::new(wide, limits.min().height), Size::new(wide, limits.max().height));
        let child = self.content.as_widget_mut().layout(tree, renderer, &inner).move_to(Point::new(x, 0.0));
        let size = limits.resolve(Length::Fill, Length::Shrink, Size::new(width, child.size().height));
        layout::Node::with_children(size, vec![child])
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        if let Some(child) = layout.children().next() {
            self.content.as_widget_mut().operate(tree, child, renderer, operation);
        }
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
        if let Some(child) = layout.children().next() {
            self.content.as_widget_mut().update(tree, event, child, cursor, renderer, clipboard, shell, viewport);
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
        layout.children().next().map_or(mouse::Interaction::None, |child| {
            self.content.as_widget().mouse_interaction(tree, child, cursor, viewport, renderer)
        })
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
        if let Some(child) = layout.children().next() {
            self.content.as_widget().draw(tree, renderer, theme, style, child, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let child = layout.children().next()?;
        self.content.as_widget_mut().overlay(tree, child, renderer, viewport, translation)
    }
}

impl<'a, M: 'a> From<Bleed<'a, M>> for Element<'a, M> {
    fn from(bleed: Bleed<'a, M>) -> Self {
        Element::new(bleed)
    }
}
