//! A divider between two panes side by side that the pointer drags (issue #130): first the mail
//! window's list and reader, later any list beside its detail (inbox, tasks).
//!
//! The divider is the 1-px line between the panes in a handle [`handle_width()`] wide with the
//! resize cursor. Pressing it starts a drag; while it runs the whole split reports where the pointer
//! is, so a fast pointer that leaves the thin handle still drags; releasing the button anywhere in
//! the window ends it ([`release`]). A double click goes back to the default width. Both panes keep
//! at least [`min_pane()`].
//!
//! The app keeps a [`Split`] and the width the person chose in `ui.json`
//! (`ui_state::save_split_task`); the logic is here, so every app drags the same way:
//!
//! ```ignore
//! // view
//! let limits = split::Limits::new(size::list(), content_width);
//! split::split(list, reader, self.split.width(limits), self.split.dragging, Message::Split)
//! // update
//! Message::Split(event) => {
//!     if self.split.update(event, limits) {
//!         return ui_state::save_split_task(APP, key, self.split.chosen);
//!     }
//! }
//! // subscription: only while dragging
//! if self.split.dragging { subs.push(split::release().map(|()| Message::Split(SplitEvent::Release))) }
//! ```

use iced::widget::{container, mouse_area, row, rule};
use iced::{Element, Fill, Subscription, mouse};

use crate::style;
use crate::theme::{palette, size, space};

/// What the divider reports to the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitEvent {
    /// The handle was pressed: a drag starts.
    Grab,
    /// The pointer moved while dragging: the first pane would be this wide (not yet clamped).
    Drag(f32),
    /// The left button was released anywhere in the window ([`release`]): the drag ends.
    Release,
    /// The handle was double-clicked: back to the default width.
    Reset,
}

/// The width of the handle the pointer grabs, with the line in its middle: [`space::s()`].
pub fn handle_width() -> f32 {
    space::s()
}

/// The least either pane keeps: [`size::props()`], a column of its own.
pub fn min_pane() -> f32 {
    size::props()
}

/// What the first pane's width is measured against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The width without a choice of the person (`size::list()` for a list beside its detail).
    pub default: f32,
    /// The width of the whole split: both panes and the handle.
    pub total: f32,
    /// The least the first pane keeps: [`min_pane()`] unless [`Limits::bounded`] says otherwise.
    pub least: f32,
    /// The most the first pane may take, whatever room the second leaves: no bound unless
    /// [`Limits::bounded`] sets one (the sidebar of the mail window, issue #376).
    pub most: f32,
    /// The least the second pane keeps: [`min_pane()`] unless [`Limits::bounded`] says otherwise.
    pub rest: f32,
}

impl Limits {
    /// The limits of a split `total` wide whose first pane is `default` wide by default; both panes
    /// keep at least [`min_pane()`].
    pub fn new(default: f32, total: f32) -> Limits {
        Limits { default, total, least: min_pane(), most: f32::INFINITY, rest: min_pane() }
    }

    /// These limits with bounds of their own: the first pane between `least` and `most`, the second
    /// keeping at least `rest`. For a pane that is no list beside its detail, as the sidebar
    /// (issue #376), whose room is not [`min_pane()`].
    pub fn bounded(self, least: f32, most: f32, rest: f32) -> Limits {
        Limits { least, most, rest, ..self }
    }

    /// `width` kept between [`Limits::least`] and what leaves the second pane [`Limits::rest`], never
    /// over [`Limits::most`]; in a split too narrow for both, [`Limits::least`].
    pub fn clamp(&self, width: f32) -> f32 {
        let max = (self.total - handle_width() - self.rest).min(self.most).max(self.least);
        width.clamp(self.least, max)
    }
}

/// The state of one split, kept by the app.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Split {
    /// The width the person dragged the first pane to; `None` for the default. The app stores it.
    pub chosen: Option<f32>,
    /// A drag runs: the app subscribes to [`release`] and the split follows the pointer.
    pub dragging: bool,
    /// `chosen` when the drag began, to tell whether it changed anything.
    grabbed: Option<f32>,
}

impl Split {
    /// A split with the width `chosen` (as stored), not dragging.
    pub fn with(chosen: Option<f32>) -> Split {
        Split { chosen, ..Split::default() }
    }

    /// The first pane's width now: the one chosen, else the default, kept inside `limits`.
    pub fn width(&self, limits: Limits) -> f32 {
        limits.clamp(self.chosen.unwrap_or(limits.default))
    }

    /// Applies `event`. `true` when the width changed for good – a drag ended somewhere else than
    /// it began, or a double click went back to the default –, which is when the app saves
    /// [`Split::chosen`]. A drag stores the width clamped to `limits`.
    pub fn update(&mut self, event: SplitEvent, limits: Limits) -> bool {
        match event {
            SplitEvent::Grab => {
                self.dragging = true;
                self.grabbed = self.chosen;
                false
            }
            SplitEvent::Drag(width) if self.dragging => {
                self.chosen = Some(limits.clamp(width));
                false
            }
            SplitEvent::Drag(_) => false,
            SplitEvent::Release if self.dragging => {
                self.dragging = false;
                self.chosen != self.grabbed
            }
            SplitEvent::Release => false,
            SplitEvent::Reset => {
                self.dragging = false;
                let changed = self.chosen.is_some();
                self.chosen = None;
                changed
            }
        }
    }
}

/// `first` `width` wide, the handle, then `second` in the rest. The handle sends
/// [`SplitEvent::Grab`] and, double-clicked, [`SplitEvent::Reset`]; while `dragging`, the line
/// shows in `line_selected` and every move over the split sends [`SplitEvent::Drag`] with the width
/// that puts the handle's middle under the pointer. The same widgets with and without a drag, so
/// starting one keeps the panes' state (scroll positions, focus).
pub fn split<'a, M: Clone + 'a>(
    first: impl Into<Element<'a, M>>,
    second: impl Into<Element<'a, M>>,
    width: f32,
    dragging: bool,
    on: fn(SplitEvent) -> M,
) -> Element<'a, M> {
    let line = rule::vertical(size::HAIRLINE).style(move |t| {
        let mut look = style::rule(t);
        if dragging {
            look.color = palette(t).line_selected;
        }
        look
    });
    let handle = mouse_area(container(line).center_x(handle_width()).height(Fill))
        .on_press(on(SplitEvent::Grab))
        .on_double_click(on(SplitEvent::Reset))
        .interaction(mouse::Interaction::ResizingHorizontally);
    let panes = row![container(first).width(width).height(Fill), handle, container(second).width(Fill).height(Fill)];
    let area = mouse_area(panes);
    if dragging {
        area.on_move(move |point| on(SplitEvent::Drag(point.x - handle_width() / 2.0)))
            .interaction(mouse::Interaction::ResizingHorizontally)
            .into()
    } else {
        area.into()
    }
}

/// The left button released anywhere in the window: subscribe while [`Split::dragging`] and send
/// [`SplitEvent::Release`]. The split only hears moves over itself, the window hears the release.
pub fn release() -> Subscription<()> {
    iced::event::listen_with(|event, _status, _window| match event {
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    enum Msg {
        Split(SplitEvent),
    }

    fn limits() -> Limits {
        Limits::new(460.0, 1200.0)
    }

    #[test]
    fn the_width_stays_between_both_panes_least() {
        let limits = limits();
        assert_eq!(limits.clamp(100.0), min_pane(), "the list keeps its least");
        assert_eq!(limits.clamp(5000.0), 1200.0 - handle_width() - min_pane(), "the reader keeps its least");
        assert_eq!(limits.clamp(600.0), 600.0);
        let tight = Limits::new(460.0, 500.0);
        assert_eq!(tight.clamp(460.0), min_pane(), "too narrow for both: the first keeps its least");
        assert_eq!(Split::default().width(limits), 460.0, "the default without a choice");
        assert_eq!(Split::with(Some(2000.0)).width(limits), limits.clamp(2000.0), "a stored width is clamped");
    }

    /// Issue #376: bounds of their own keep the first pane between them and leave the second its rest.
    #[test]
    fn bounded_limits_keep_their_own_least_most_and_rest() {
        let bounded = Limits::new(240.0, 1200.0).bounded(180.0, 480.0, 600.0);
        assert_eq!(bounded.clamp(100.0), 180.0, "the first keeps its own least");
        assert_eq!(bounded.clamp(5000.0), 480.0, "never over the most");
        assert_eq!(bounded.clamp(300.0), 300.0);
        let tight = Limits::new(240.0, 900.0).bounded(180.0, 480.0, 600.0);
        assert_eq!(tight.clamp(480.0), 900.0 - handle_width() - 600.0, "the second keeps its rest");
        let tiny = Limits::new(240.0, 500.0).bounded(180.0, 480.0, 600.0);
        assert_eq!(tiny.clamp(300.0), 180.0, "too narrow for both: the first keeps its least");
        assert_eq!(limits().bounded(min_pane(), f32::INFINITY, min_pane()), limits(), "new: the panes' least");
    }

    #[test]
    fn a_drag_moves_the_width_and_asks_to_save_only_a_change() {
        let limits = limits();
        let mut split = Split::default();
        assert!(!split.update(SplitEvent::Drag(700.0), limits), "no drag without a grab");
        assert_eq!(split.chosen, None);
        assert!(!split.update(SplitEvent::Grab, limits));
        assert!(split.dragging);
        assert!(!split.update(SplitEvent::Drag(700.0), limits));
        assert!(!split.update(SplitEvent::Drag(50.0), limits));
        assert_eq!(split.chosen, Some(min_pane()), "clamped while dragging");
        assert!(split.update(SplitEvent::Release, limits), "a new width: save it");
        assert!(!split.dragging);
        assert!(!split.update(SplitEvent::Release, limits), "a second release changes nothing");
        // A click on the handle without moving saves nothing.
        assert!(!split.update(SplitEvent::Grab, limits));
        assert!(!split.update(SplitEvent::Release, limits));
        assert!(split.update(SplitEvent::Reset, limits), "the double click goes back: save");
        assert_eq!((split.chosen, split.width(limits)), (None, 460.0));
        assert!(!split.update(SplitEvent::Reset, limits), "already the default");
    }

    #[test]
    fn the_split_builds_with_and_without_a_drag() {
        for dragging in [false, true] {
            let _: Element<'_, Msg> = split("list", "reader", 460.0, dragging, Msg::Split);
        }
        let _ = release().map(|()| Msg::Split(SplitEvent::Release));
        let Msg::Split(event) = Msg::Split(SplitEvent::Reset);
        assert_eq!(event, SplitEvent::Reset);
    }
}
