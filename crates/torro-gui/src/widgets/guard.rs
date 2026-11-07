//! Two wrappers for a dropdown that opens over a floating dialog (issue #554):
//!
//! - [`Inert`] draws its content but hands it no event. Iced gives an event to every floating layer
//!   in turn, the lower one first, and a button there takes a click that was meant for the menu
//!   above it; the dialog under an open menu therefore stays inert, and a click on it reaches the
//!   menu's click-away layer, which closes the menu.
//! - [`EscapeArea`] keeps Escape from its content and leaves the key uncaptured: a focused search
//!   field would otherwise keep Escape to itself (it only gives up its focus), and the menu would
//!   close only on the second press. Passed on, the key reaches the window's own Escape, which
//!   closes the open menu first as it does for every menu.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, keyboard};

/// `content` drawn and laid out as usual, but deaf to every event.
pub(super) struct Inert<'a, M> {
    pub(super) content: Element<'a, M>,
}

/// `content` that never sees Escape; the key stays free for the window.
pub(super) struct EscapeArea<'a, M> {
    pub(super) content: Element<'a, M>,
}

/// Everything but `update` (and the overlay of [`Inert`]) as the content does it.
macro_rules! forward {
    () => {
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
    };
}

impl<M> Widget<M, Theme, Renderer> for Inert<'_, M> {
    forward!();

    fn update(
        &mut self,
        _tree: &mut Tree,
        _event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
    }

    // No hover look and no tooltips while it is inert: they would answer the pointer.
    fn mouse_interaction(
        &self,
        _tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        None
    }
}

impl<M> Widget<M, Theme, Renderer> for EscapeArea<'_, M> {
    forward!();

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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) = event
        {
            return;
        }
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

impl<'a, M: 'a> From<Inert<'a, M>> for Element<'a, M> {
    fn from(inert: Inert<'a, M>) -> Self {
        Element::new(inert)
    }
}

impl<'a, M: 'a> From<EscapeArea<'a, M>> for Element<'a, M> {
    fn from(area: EscapeArea<'a, M>) -> Self {
        Element::new(area)
    }
}
