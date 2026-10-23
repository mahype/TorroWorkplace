//! A wrapper that draws its content in another theme, picked from the window's while drawing:
//!
//! - [`in_sidebar`]: the sidebar's theme ([`theme::sidebar_theme`]): rows, group titles, the small
//!   month and every other building block inside take the sidebar's colours from [`theme::palette`]
//!   without knowing where they are (task L.6, concept `docs/design-pakete.md`, "Seitenleiste mit
//!   eigenen Farben").
//! - [`on_accent`]: the theme of a chosen row drawn in the accent colour ([`theme::on_accent_theme`],
//!   issue #291, `[list] selection = "fill"`): its text and icons take `on_accent` the same way.
//!
//! Iced's own `Themer` needs the theme when the view is built, but views don't know the window's
//! mode then; this one picks it while drawing, from the theme the window draws in. Overlays
//! (menus, tooltips) opened from inside keep the window's theme: they lie on `menu`.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::theme;

/// `content` drawn in the sidebar's theme.
pub(crate) fn in_sidebar<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Themed { content: content.into(), pick: theme::sidebar_theme })
}

/// `content` drawn in the theme of a row in the accent colour: its text and icons in `on_accent`.
pub(crate) fn on_accent<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Themed { content: content.into(), pick: theme::on_accent_theme })
}

struct Themed<'a, M> {
    content: Element<'a, M>,
    /// The theme to draw in, from the theme the window draws in.
    pick: fn(&Theme) -> Theme,
}

impl<M> Widget<M, Theme, Renderer> for Themed<'_, M> {
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
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let picked = (self.pick)(theme);
        // Plain text without a style of its own takes this colour.
        let style = renderer::Style { text_color: theme::palette(&picked).text };
        self.content.as_widget().draw(tree, renderer, &picked, &style, layout, cursor, viewport);
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
