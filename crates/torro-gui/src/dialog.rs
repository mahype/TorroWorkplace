//! The frame of a dialog's content: a fixed head, a body that scrolls when the window is too low,
//! and a fixed foot with the actions (task D.12 in `docs/tasks/design.md`).
//!
//! ```text
//! enough room                 low window
//! ┌ head ───────────┐         ┌ head ───────────┐
//! │ body            │         │ body          ▐ │  ← scrolls
//! │                 │         │ …             ▐ │
//! ├ foot ───────────┤         ├ foot ───────────┤  ← always in the window
//! └─────────────────┘         └─────────────────┘
//! ```
//!
//! Iced's `column![head, scrollable(body), foot]` cannot do this inside a dialog that is as high
//! as its content: the column lays its children out in order, so a shrinking `scrollable` takes
//! the whole height that is left before the foot is measured, and the foot falls out of the
//! window. [`Frame`] measures head and foot first and gives the body what remains.
//!
//! ```ignore
//! use torro_gui::dialog;
//! use torro_gui::theme::space;
//!
//! let content = dialog::frame(header, body, footer, space::XL);
//! widgets::modal(base, container(content).padding(space::XL), size::FORM_MAX, Message::Close)
//! ```

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay, renderer};
use iced::widget::scrollable;
use iced::{Element, Event, Fill, Length, Rectangle, Shrink, Size, Vector};

use crate::style;
use crate::theme::space;

/// Head, body and foot of a dialog, `spacing` apart. The body is wrapped in a vertical scrollable
/// in the suite's style; it scrolls only when head, body and foot together are higher than the
/// space the dialog has. Fills the width it gets and is as high as its content allows.
pub fn frame<'a, M: 'a>(
    head: impl Into<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
    foot: impl Into<Element<'a, M>>,
    spacing: f32,
) -> Element<'a, M> {
    // The gap keeps the scroller off the body's rows; Iced reserves it only while it scrolls.
    let scroll = scrollable(body)
        .direction(scrollable::Direction::Vertical(style::scrollbar().spacing(space::XS)))
        .style(style::scrollable)
        .width(Fill)
        .height(Shrink);
    Frame { children: [head.into(), scroll.into(), foot.into()], spacing }.into()
}

/// The widget behind [`frame`]: `children` are head, scrollable body and foot, in this order.
pub struct Frame<'a, M, Theme = iced::Theme, Renderer = iced::Renderer> {
    children: [Element<'a, M, Theme, Renderer>; 3],
    spacing: f32,
}

/// Where the three parts go for the `max` size the frame may take, given the heights head and foot
/// need: the body gets the rest (never less than nothing). Returns the body's maximum height.
fn body_room(max: Size, head: f32, foot: f32, spacing: f32) -> f32 {
    (max.height - head - foot - 2.0 * spacing).max(0.0)
}

impl<M, Theme, Renderer: iced::advanced::Renderer> Widget<M, Theme, Renderer> for Frame<'_, M, Theme, Renderer> {
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        Size { width: Fill, height: Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let limits = limits.width(Fill).height(Shrink);
        let max = limits.max();
        // Fill heights inside the parts shrink to their content, as in a column that shrinks.
        let part = |height: f32| {
            layout::Limits::with_compression(Size::ZERO, Size::new(max.width, height), Size::new(false, true))
        };

        let [head, body, foot] = &mut self.children;
        let [head_tree, body_tree, foot_tree] = &mut tree.children[..] else {
            return layout::Node::new(Size::ZERO);
        };
        let head = head.as_widget_mut().layout(head_tree, renderer, &part(max.height));
        let foot = foot.as_widget_mut().layout(foot_tree, renderer, &part(max.height));
        let room = body_room(max, head.size().height, foot.size().height, self.spacing);
        let body = body.as_widget_mut().layout(body_tree, renderer, &part(room));

        let body_y = head.size().height + self.spacing;
        let foot_y = body_y + body.size().height + self.spacing;
        let height = foot_y + foot.size().height;
        let size = limits.resolve(Fill, Shrink, Size::new(max.width, height));
        layout::Node::with_children(size, vec![head, body.move_to((0.0, body_y)), foot.move_to((0.0, foot_y))])
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((child, state), layout) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()) {
                child.as_widget_mut().operate(state, layout, renderer, operation);
            }
        });
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
        for ((child, state), layout) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()) {
            child.as_widget_mut().update(state, event, layout, cursor, renderer, clipboard, shell, viewport);
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child.as_widget().mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
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
        for ((child, state), layout) in self.children.iter().zip(&tree.children).zip(layout.children()) {
            child.as_widget().draw(state, renderer, theme, style, layout, cursor, viewport);
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
        overlay::from_children(&mut self.children, tree, layout, renderer, viewport, translation)
    }
}

impl<'a, M: 'a, Theme: 'a, Renderer: iced::advanced::Renderer + 'a> From<Frame<'a, M, Theme, Renderer>>
    for Element<'a, M, Theme, Renderer>
{
    fn from(frame: Frame<'a, M, Theme, Renderer>) -> Self {
        Element::new(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::{column, space as blank};

    const SPACING: f32 = 24.0;

    /// An element for Iced's null renderer `()`, which lays out without a window.
    type Part = Element<'static, (), iced::Theme, ()>;

    /// The frame as [`frame`] builds it, for the null renderer.
    fn frame(head: Part, body: Part, foot: Part, spacing: f32) -> Part {
        let scroll = scrollable(body).width(Fill).height(Shrink);
        Frame { children: [head, scroll.into(), foot], spacing }.into()
    }

    /// Lays `frame` out in `max`: the frame's own size and its three parts.
    fn lay_out(mut frame: Part, max: Size) -> (Size, Vec<Rectangle>) {
        let renderer = ();
        let mut tree = Tree::new(&frame);
        let node = frame.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, max));
        let parts = node.children().iter().map(layout::Node::bounds).collect();
        (node.size(), parts)
    }

    /// A part of a fixed height, like a title or a row of buttons.
    fn block(height: f32) -> Part {
        blank().width(Fill).height(height).into()
    }

    /// A body of `rows` rows, 40 high each.
    fn rows(rows: usize) -> Part {
        column((0..rows).map(|_| block(40.0))).into()
    }

    #[test]
    fn with_room_the_frame_is_as_high_as_its_content() {
        let (size, parts) = lay_out(frame(block(50.0), rows(3), block(36.0), SPACING), Size::new(600.0, 900.0));
        assert_eq!(size, Size::new(600.0, 50.0 + SPACING + 120.0 + SPACING + 36.0));
        assert_eq!(parts[1], Rectangle { x: 0.0, y: 50.0 + SPACING, width: 600.0, height: 120.0 });
    }

    #[test]
    fn in_a_low_window_the_body_gives_way_and_the_foot_stays_inside() {
        let max = Size::new(600.0, 300.0);
        let (size, parts) = lay_out(frame(block(50.0), rows(20), block(36.0), SPACING), max);
        assert_eq!(size.height, 300.0);
        assert_eq!(parts[1].height, 300.0 - 50.0 - 36.0 - 2.0 * SPACING);
        let foot = parts[2];
        assert_eq!(foot.y + foot.height, 300.0, "the foot ends at the bottom edge");
    }

    #[test]
    fn the_body_never_gets_a_negative_height() {
        assert_eq!(body_room(Size::new(600.0, 80.0), 50.0, 36.0, SPACING), 0.0);
    }
}
