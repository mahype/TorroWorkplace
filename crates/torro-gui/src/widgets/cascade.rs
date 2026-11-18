//! Cascading menus (issue #906): an entry marked "▸" opens its submenu right beside it – after a
//! short rest of the pointer ([`CASCADE_DELAY`]) or at once on a click –, the submenu stays open
//! while the pointer travels across other entries towards it, flips to the left at the window's
//! right edge and answers the keys ↑ ↓ → ← Enter and Escape, as the menus of macOS, Windows, GNOME
//! and KDE do.
//!
//! The app keeps a [`Cascade`] beside its open menu, routes every [`CascadeEvent`] to
//! [`Cascade::update`] and acts on the [`CascadeChange`] it returns: it builds the submenu that
//! opened, carries out the entry the keys chose, closes what closed. The cascade itself only knows
//! which entries are open, where they are on screen and what the pointer and the keys did – the
//! entries' content stays with the app, which shows it with [`Cascade::layer`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iced::widget::scrollable;
use iced::widget::{column, container, float, mouse_area, opaque, rule, stack};
use iced::{Element, Fill, Length, Padding, Point, Rectangle, Vector};

use super::{Anchor, MenuSection, Placement, blank, centre, menu_entry_lit, menu_width, place};
use crate::style;
use crate::theme::{self, space};
use crate::typo;

/// How long the pointer rests on an entry before its submenu opens, or before an open submenu of
/// another entry closes (issue #906: "etwa 150–250 ms").
pub const CASCADE_DELAY: Duration = Duration::from_millis(200);

/// How often an app checks a waiting submenu ([`Cascade::waiting`]): often enough that the delay
/// does not feel longer than it is.
pub const CASCADE_TICK: Duration = Duration::from_millis(40);

/// How many submenus can be open below the root menu: "Save mail as ▸ PDF ▸ targets" is the deepest.
pub const CASCADE_DEPTH: usize = 2;

/// A key that moves through a cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeKey {
    /// ↑: the entry above in the deepest open menu, from the first to the last.
    Up,
    /// ↓: the entry below, from the last to the first.
    Down,
    /// →: opens the submenu of the entry under the keyboard cursor.
    Right,
    /// ←: closes the deepest submenu.
    Left,
    /// Enter: opens the submenu of the entry under the cursor, or carries out that entry.
    Enter,
    /// Escape: closes the deepest submenu, or the whole menu when none is open.
    Escape,
}

/// What the pointer and the keys do in a cascade. An entry is named by its path: its place in the
/// root menu, then its place in each submenu down to it (`[3]` is the root's fourth entry, `[3, 1]`
/// the second entry of its submenu).
#[derive(Debug, Clone, PartialEq)]
pub enum CascadeEvent {
    /// The pointer came onto the entry.
    Enter(Vec<usize>),
    /// The pointer left the entry.
    Leave(Vec<usize>),
    /// The pointer came into the card of the submenu at this level (1 for the first submenu).
    Card(usize),
    /// The pointer moved, to this point of the window.
    Move(Point),
    /// A click on an entry that opens a submenu: open it now.
    Press(Vec<usize>),
    /// The app's tick while the cascade waits ([`Cascade::waiting`]).
    Tick(Instant),
    /// A key.
    Key(CascadeKey),
}

/// What changed with an event, for the app to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CascadeChange {
    /// Nothing the app must act on (the keyboard cursor may have moved; the view shows it).
    None,
    /// The submenu of the entry at this path opened, and every other submenu at its level or
    /// deeper closed: build it.
    Opened(Vec<usize>),
    /// The keys chose the entry at `index` of the menu at `level` (0 for the root): carry it out
    /// and close every level, as a click on it does.
    Activate { level: usize, index: usize },
    /// The submenus at this level and deeper closed (1: every submenu); the menus above stay.
    Closed(usize),
    /// Escape in the root menu: close the whole menu.
    CloseAll,
}

/// A submenu that opens – or an open one that closes – once the pointer rested long enough.
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    path: Vec<usize>,
    since: Instant,
}

/// The state of a cascading menu: which submenus are open, which one waits, the keyboard cursor
/// of each level and where the entries and cards are on screen.
///
/// The shape of the open menus – how many entries each level has and which of them open a
/// submenu – comes with every [`Cascade::update`] as `shape`: `shape[0]` for the root, `shape[1]`
/// for the open submenu, and so on; `true` marks an entry with a submenu.
#[derive(Debug, Clone, Default)]
pub struct Cascade {
    /// The entries whose submenus are open, from the root down: `open[0]` in the root menu.
    open: Vec<usize>,
    pending: Option<Pending>,
    /// The keyboard cursor of each level, the root's first.
    cursor: [Option<usize>; CASCADE_DEPTH + 1],
    /// Where the pointer was last, in the window.
    pointer: Option<Point>,
    /// One anchor per entry, written while the window is laid out, read by its submenu's card.
    anchors: RefCell<HashMap<Vec<usize>, Anchor>>,
    /// Where each level's card stood in the last frame (0 for the root), for [`aims_at`].
    cards: RefCell<HashMap<usize, Anchor>>,
    /// How far each level's card was moved from where it was laid out (0 for the root): its entries
    /// record where they stood before the move, so their submenu and the pointer add it.
    shifts: RefCell<HashMap<usize, Rc<Cell<Vector>>>>,
}

impl PartialEq for Cascade {
    /// Equal when the same submenus are open, waiting and under the cursor; where things are on
    /// screen is no state.
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open && self.pending == other.pending && self.cursor == other.cursor
    }
}

impl Cascade {
    /// A cascade with nothing open.
    pub fn new() -> Self {
        Self::default()
    }

    /// The paths' entries whose submenus are open, from the root down (empty: only the root).
    pub fn open(&self) -> &[usize] {
        &self.open
    }

    /// Whether the submenu of the entry at `path` is open.
    pub fn is_open(&self, path: &[usize]) -> bool {
        !path.is_empty() && self.open.starts_with(path)
    }

    /// The keyboard cursor of the menu at `level` (0 for the root).
    pub fn cursor(&self, level: usize) -> Option<usize> {
        self.cursor.get(level).copied().flatten()
    }

    /// Whether a submenu waits to open or close: tick the cascade with [`CascadeEvent::Tick`]
    /// every [`CASCADE_TICK`] only then.
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// Closes every submenu and forgets cursor and pointer: the menu closed, or a new one opened.
    pub fn reset(&mut self) {
        self.open.clear();
        self.pending = None;
        self.cursor = [None; CASCADE_DEPTH + 1];
        self.pointer = None;
    }

    /// Closes the submenus at `level` and deeper (1: every submenu), as [`CascadeChange::Closed`]
    /// says: the app found nothing to show for a submenu that opened.
    pub fn close_from(&mut self, level: usize) {
        let level = level.max(1);
        self.open.truncate(level - 1);
        self.pending = None;
        for deeper in level..=CASCADE_DEPTH {
            self.cursor[deeper] = None;
        }
    }

    /// The anchor of the entry at `path`: its submenu's card hangs beside it.
    pub fn anchor(&self, path: &[usize]) -> Anchor {
        self.anchors.borrow_mut().entry(path.to_vec()).or_default().clone()
    }

    /// Where the entry at `path` stands on screen: where it was laid out, moved with its card.
    pub fn entry_rect(&self, path: &[usize]) -> Option<Rectangle> {
        let level = path.len().checked_sub(1)?;
        self.anchor(path).get().map(|bounds| bounds + self.shift(level).get())
    }

    fn shift(&self, level: usize) -> Rc<Cell<Vector>> {
        Rc::clone(self.shifts.borrow_mut().entry(level).or_insert_with(|| Rc::new(Cell::new(Vector::ZERO))))
    }

    fn card(&self, level: usize) -> Anchor {
        self.cards.borrow_mut().entry(level).or_default().clone()
    }

    /// Applies `event` at `now`; `shape` is the shape of the open menus (see [`Cascade`]).
    pub fn update(&mut self, event: CascadeEvent, now: Instant, shape: &[Vec<bool>]) -> CascadeChange {
        let opens = |path: &[usize]| {
            let (level, index) = (path.len().wrapping_sub(1), path.last().copied());
            index.is_some_and(|index| shape.get(level).and_then(|entries| entries.get(index)).copied() == Some(true))
        };
        match event {
            CascadeEvent::Enter(path) => {
                let Some(&index) = path.last() else { return CascadeChange::None };
                let level = path.len() - 1;
                if level > CASCADE_DEPTH || !self.open.starts_with(&path[..level]) {
                    return CascadeChange::None;
                }
                self.pending = None;
                self.cursor[level] = Some(index);
                // The pointer in a submenu keeps the entries above it open that lead to it.
                let open_here = self.open.get(level).copied();
                if open_here == Some(index) {
                    return CascadeChange::None;
                }
                // Another entry: its submenu opens, or the open one of another entry closes, once the
                // pointer rested on it – crossing it on the way to the open submenu changes nothing.
                if opens(&path) || open_here.is_some() {
                    self.pending = Some(Pending { path, since: now });
                }
                CascadeChange::None
            }
            CascadeEvent::Leave(path) => {
                if self.pending.as_ref().is_some_and(|pending| pending.path == path) {
                    self.pending = None;
                }
                CascadeChange::None
            }
            CascadeEvent::Card(level) => {
                // Arrived in the submenu: whatever waited in the menus above it is dropped.
                if self.pending.as_ref().is_some_and(|pending| pending.path.len() <= level) && self.open.len() >= level
                {
                    self.pending = None;
                }
                CascadeChange::None
            }
            CascadeEvent::Move(point) => {
                let from = self.pointer.replace(point);
                // On the way to the open submenu the wait starts again with every step, so the
                // submenu stays while the pointer heads for it (the "safe triangle").
                let level = self.pending.as_ref().map_or(0, |pending| pending.path.len());
                let target = (level > 0 && self.open.len() >= level).then(|| self.card(level).get()).flatten();
                if let (Some(from), Some(card), Some(pending)) = (from, target, self.pending.as_mut())
                    && aims_at(from, point, card)
                {
                    pending.since = now;
                }
                CascadeChange::None
            }
            CascadeEvent::Press(path) => {
                if path.is_empty() || path.len() > CASCADE_DEPTH || !opens(&path) {
                    return CascadeChange::None;
                }
                self.pending = None;
                self.cursor[path.len() - 1] = path.last().copied();
                if self.open == path {
                    return CascadeChange::None;
                }
                self.open_at(path, None)
            }
            CascadeEvent::Tick(at) => {
                let Some(pending) = self.pending.take_if(|pending| at.duration_since(pending.since) >= CASCADE_DELAY)
                else {
                    return CascadeChange::None;
                };
                if opens(&pending.path) && pending.path.len() <= CASCADE_DEPTH {
                    return self.open_at(pending.path, None);
                }
                let level = pending.path.len() - 1;
                if self.open.len() > level {
                    self.open.truncate(level);
                    return CascadeChange::Closed(level + 1);
                }
                CascadeChange::None
            }
            CascadeEvent::Key(key) => {
                self.pending = None;
                let level = self.open.len();
                let count = shape.get(level).map_or(0, Vec::len);
                let cursor = self.cursor(level);
                match key {
                    CascadeKey::Up | CascadeKey::Down if count > 0 => {
                        let next = match (key, cursor) {
                            (CascadeKey::Down, Some(at)) => (at + 1) % count,
                            (CascadeKey::Down, None) => 0,
                            (_, Some(at)) => (at + count - 1) % count,
                            (_, None) => count - 1,
                        };
                        self.cursor[level] = Some(next);
                        CascadeChange::None
                    }
                    CascadeKey::Right | CascadeKey::Enter => {
                        let Some(at) = cursor.filter(|at| *at < count) else { return CascadeChange::None };
                        let mut path = self.open.clone();
                        path.push(at);
                        if opens(&path) && path.len() <= CASCADE_DEPTH {
                            return self.open_at(path, Some(0));
                        }
                        if key == CascadeKey::Enter {
                            return CascadeChange::Activate { level, index: at };
                        }
                        CascadeChange::None
                    }
                    CascadeKey::Left | CascadeKey::Escape if level > 0 => {
                        self.open.pop();
                        self.cursor[level] = None;
                        CascadeChange::Closed(level)
                    }
                    CascadeKey::Escape => CascadeChange::CloseAll,
                    _ => CascadeChange::None,
                }
            }
        }
    }

    /// Opens the submenu of the entry at `path`, closing the others at its level and deeper, with
    /// the keyboard cursor of the new level at `cursor`.
    fn open_at(&mut self, path: Vec<usize>, cursor: Option<usize>) -> CascadeChange {
        let level = path.len();
        for deeper in level..=CASCADE_DEPTH {
            self.cursor[deeper] = None;
        }
        self.cursor[level] = cursor;
        self.open = path.clone();
        CascadeChange::Opened(path)
    }

    /// `content`, an entry of a cascading menu at `path`: it tells the cascade when the pointer
    /// comes onto it, leaves it and moves over it, and registers where it is ([`Cascade::anchor`]),
    /// so its submenu hangs beside it. Its press stays the content's own – for an entry with a
    /// submenu send [`CascadeEvent::Press`].
    pub fn entry<'a, M: Clone + 'a>(
        &self,
        path: Vec<usize>,
        content: impl Into<Element<'a, M>>,
        on_event: impl Fn(CascadeEvent) -> M + Clone + 'a,
    ) -> Element<'a, M> {
        let anchor = self.anchor(&path);
        let (at, shift) = (anchor.clone(), self.shift(path.len().saturating_sub(1)));
        let (enter, leave, moved) = (on_event.clone(), on_event.clone(), on_event);
        let (into, out) = (path.clone(), path);
        mouse_area(super::anchored(&anchor, content))
            .on_enter(enter(CascadeEvent::Enter(into)))
            .on_exit(leave(CascadeEvent::Leave(out)))
            .on_move(move |point| {
                let origin = at.get().map_or(Point::ORIGIN, |bounds| bounds.position()) + shift.get();
                moved(CascadeEvent::Move(Point::new(origin.x + point.x, origin.y + point.y)))
            })
            .into()
    }

    /// A menu card of `sections` at level `prefix.len()` of the cascade, below the entries `prefix`:
    /// a [`super::menu`] whose entries are [`Cascade::entry`]s, the one under the keyboard cursor lit
    /// and the one whose submenu is open lit too. An entry with a submenu ends in "▸"
    /// ([`super::MenuItem::submenu`]) and sends [`CascadeEvent::Press`] with its path on a click.
    pub fn menu<'a, M: Clone + 'a>(
        &self,
        prefix: &[usize],
        sections: Vec<MenuSection<'a, M>>,
        on_event: impl Fn(CascadeEvent) -> M + Clone + 'a,
    ) -> Element<'a, M> {
        let level = prefix.len();
        let (cursor, open) =
            (self.cursor(level), self.open.get(level).copied().filter(|_| self.open.starts_with(prefix)));
        let mut rows: Vec<Element<'a, M>> = Vec::new();
        let mut at = 0;
        for (i, section) in sections.into_iter().enumerate() {
            if i > 0 {
                rows.push(
                    container(rule::horizontal(super::LINE).style(style::rule)).padding([space::xs(), 0.0]).into(),
                );
            }
            if let Some(title) = section.title {
                let label = typo::text(typo::caption(), title).style(theme::text_3);
                rows.push(container(label).padding(Padding::new(space::s()).bottom(space::xs())).into());
            }
            for item in section.items {
                let mut path = prefix.to_vec();
                path.push(at);
                let lit = cursor == Some(at) || open == Some(at);
                rows.push(self.entry(path, menu_entry_lit(item, None, lit), on_event.clone()));
                at += 1;
            }
        }
        // A long submenu scrolls, its scrollbar beside the entries.
        let body = scrollable(column(rows))
            .direction(scrollable::Direction::Vertical(style::scrollbar().spacing(space::xs())))
            .style(style::scrollable)
            .height(Length::Shrink)
            .width(Fill);
        container(container(body).max_height(crate::theme::size::panel()))
            .width(menu_width())
            .padding(style::MENU_INSET)
            .style(style::container::menu)
            .into()
    }

    /// The layer of an open cascading menu: `root` at `spot` as [`super::floating`] places it, and
    /// each of `children` – the card of the first open submenu, then of the one in it – beside the
    /// entry that opened it ([`Placement::Cascade`]). A click next to the cards sends `on_dismiss`;
    /// `on_event` hears the pointer come into a submenu's card ([`CascadeEvent::Card`]).
    pub fn layer<'a, M: Clone + 'a>(
        &self,
        spot: &Anchor,
        placement: Placement,
        root: impl Into<Element<'a, M>>,
        children: Vec<Element<'a, M>>,
        on_dismiss: M,
        on_event: impl Fn(CascadeEvent) -> M + 'a,
    ) -> Element<'a, M> {
        let catcher = opaque(mouse_area(blank().width(Fill).height(Fill)).on_press(on_dismiss));
        let mut layers: Vec<Element<'a, M>> = vec![catcher, {
            let spot = spot.clone();
            self.placed(move || spot.get(), placement, root.into(), 0)
        }];
        for (at, child) in children.into_iter().enumerate().take(self.open.len()) {
            let level = at + 1;
            let card = mouse_area(child).on_enter(on_event(CascadeEvent::Card(level)));
            let (entry, shift) = (self.anchor(&self.open[..level]), self.shift(level - 1));
            let target = move || entry.get().map(|bounds| bounds + shift.get());
            layers.push(self.placed(target, Placement::Cascade, opaque(card), level));
        }
        stack(layers).into()
    }

    /// `card` floated to its place at `target` (for a submenu: its entry, moved with the card it
    /// stands in) as `placement` says, recording where it stood and how far it moved as the
    /// card of `level`.
    fn placed<'a, M: 'a>(
        &self,
        target: impl Fn() -> Option<Rectangle> + 'a,
        placement: Placement,
        card: Element<'a, M>,
        level: usize,
    ) -> Element<'a, M> {
        let (record, moved) = (self.card(level), self.shift(level));
        float(card)
            .translate(move |bounds, viewport| {
                let corner = match target() {
                    Some(target) => place(target, bounds.size(), viewport, placement),
                    None => centre(bounds.size(), viewport),
                };
                record.set(Rectangle::new(corner, bounds.size()));
                let offset = corner - bounds.position();
                moved.set(offset);
                offset
            })
            .into()
    }
}

/// Whether the pointer, moving from `from` to `to`, heads for `card` (a submenu beside the menu it
/// left): `to` lies in the triangle between `from` and the card's near edge, as the menus of macOS
/// keep a submenu open while the pointer crosses other entries towards it.
pub fn aims_at(from: Point, to: Point, card: Rectangle) -> bool {
    let edge = if card.x >= from.x { card.x } else { card.x + card.width };
    let (top, bottom) = (Point::new(edge, card.y), Point::new(edge, card.y + card.height));
    let side = |a: Point, b: Point, p: Point| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let (d1, d2, d3) = (side(from, top, to), side(top, bottom, to), side(bottom, from, to));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    to != from && !(negative && positive)
}

/// The shape of a menu of `sections` for [`Cascade::update`]: one `false` per entry, `true` for an
/// entry whose label is followed by "▸" ([`super::MenuItem::submenu`]).
pub fn shape_of<M>(sections: &[MenuSection<'_, M>]) -> Vec<bool> {
    sections.iter().flat_map(|section| section.items.iter().map(super::MenuItem::opens_submenu)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// A root of four entries, the second and fourth with a submenu of three, the submenu's first
    /// with one of its own.
    fn shape(open: &[usize]) -> Vec<Vec<bool>> {
        let mut shape = vec![vec![false, true, false, true]];
        if !open.is_empty() {
            shape.push(vec![true, false, false]);
        }
        if open.len() > 1 {
            shape.push(vec![false, false]);
        }
        shape
    }

    fn update(cascade: &mut Cascade, event: CascadeEvent, now: Instant) -> CascadeChange {
        let shape = shape(cascade.open());
        cascade.update(event, now, &shape)
    }

    #[test]
    fn a_submenu_opens_after_the_pointer_rested_on_its_entry() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        assert_eq!(update(&mut cascade, CascadeEvent::Enter(vec![1]), start), CascadeChange::None);
        assert!(cascade.waiting());
        assert_eq!(update(&mut cascade, CascadeEvent::Tick(at(start, 100)), at(start, 100)), CascadeChange::None);
        assert!(cascade.open().is_empty(), "not before the delay");
        let opened = update(&mut cascade, CascadeEvent::Tick(at(start, 200)), at(start, 200));
        assert_eq!(opened, CascadeChange::Opened(vec![1]));
        assert!(cascade.is_open(&[1]) && !cascade.waiting());
    }

    #[test]
    fn a_click_opens_a_submenu_at_once() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        assert_eq!(update(&mut cascade, CascadeEvent::Press(vec![3]), start), CascadeChange::Opened(vec![3]));
        // A plain entry has no submenu to open.
        assert_eq!(update(&mut cascade, CascadeEvent::Press(vec![2]), start), CascadeChange::None);
        assert!(cascade.is_open(&[3]));
    }

    #[test]
    fn leaving_the_entry_before_the_delay_opens_nothing() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        update(&mut cascade, CascadeEvent::Enter(vec![1]), start);
        update(&mut cascade, CascadeEvent::Leave(vec![1]), at(start, 50));
        assert!(!cascade.waiting());
        assert_eq!(update(&mut cascade, CascadeEvent::Tick(at(start, 300)), at(start, 300)), CascadeChange::None);
    }

    #[test]
    fn resting_on_a_plain_entry_closes_the_open_submenu() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        update(&mut cascade, CascadeEvent::Press(vec![1]), start);
        update(&mut cascade, CascadeEvent::Enter(vec![2]), at(start, 10));
        assert!(cascade.is_open(&[1]), "it stays while the delay runs");
        let closed = update(&mut cascade, CascadeEvent::Tick(at(start, 210)), at(start, 210));
        assert_eq!(closed, CascadeChange::Closed(1));
        assert!(cascade.open().is_empty());
    }

    #[test]
    fn the_pointer_in_the_submenu_keeps_it_open() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        update(&mut cascade, CascadeEvent::Press(vec![1]), start);
        // Crossing the fourth entry on the way into the submenu.
        update(&mut cascade, CascadeEvent::Enter(vec![3]), at(start, 10));
        update(&mut cascade, CascadeEvent::Card(1), at(start, 50));
        update(&mut cascade, CascadeEvent::Enter(vec![1, 1]), at(start, 60));
        assert_eq!(update(&mut cascade, CascadeEvent::Tick(at(start, 400)), at(start, 400)), CascadeChange::None);
        assert!(cascade.is_open(&[1]));
    }

    #[test]
    fn heading_for_the_submenu_restarts_the_wait() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        update(&mut cascade, CascadeEvent::Press(vec![1]), start);
        cascade.card(1).set(Rectangle::new(Point::new(200.0, 0.0), iced::Size::new(150.0, 300.0)));
        update(&mut cascade, CascadeEvent::Move(Point::new(100.0, 100.0)), start);
        update(&mut cascade, CascadeEvent::Enter(vec![3]), at(start, 10));
        // Towards the card, to the right and a little down: the wait starts again.
        update(&mut cascade, CascadeEvent::Move(Point::new(150.0, 120.0)), at(start, 150));
        assert_eq!(update(&mut cascade, CascadeEvent::Tick(at(start, 300)), at(start, 300)), CascadeChange::None);
        assert!(cascade.is_open(&[1]));
        // Away from it, to the left: the wait runs out.
        update(&mut cascade, CascadeEvent::Move(Point::new(100.0, 130.0)), at(start, 320));
        let opened = update(&mut cascade, CascadeEvent::Tick(at(start, 360)), at(start, 360));
        assert_eq!(opened, CascadeChange::Opened(vec![3]));
    }

    #[test]
    fn the_keys_walk_open_and_close_the_levels() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        let key = |cascade: &mut Cascade, key| update(cascade, CascadeEvent::Key(key), start);
        key(&mut cascade, CascadeKey::Down);
        assert_eq!(cascade.cursor(0), Some(0));
        key(&mut cascade, CascadeKey::Up);
        assert_eq!(cascade.cursor(0), Some(3), "↑ from the first goes round to the last");
        key(&mut cascade, CascadeKey::Down);
        key(&mut cascade, CascadeKey::Down);
        assert_eq!(key(&mut cascade, CascadeKey::Right), CascadeChange::Opened(vec![1]));
        assert_eq!(cascade.cursor(1), Some(0));
        assert_eq!(key(&mut cascade, CascadeKey::Enter), CascadeChange::Opened(vec![1, 0]));
        assert_eq!(key(&mut cascade, CascadeKey::Left), CascadeChange::Closed(2));
        assert_eq!(key(&mut cascade, CascadeKey::Down), CascadeChange::None);
        assert_eq!(key(&mut cascade, CascadeKey::Enter), CascadeChange::Activate { level: 1, index: 1 });
        assert_eq!(key(&mut cascade, CascadeKey::Escape), CascadeChange::Closed(1));
        assert_eq!(key(&mut cascade, CascadeKey::Escape), CascadeChange::CloseAll);
    }

    #[test]
    fn enter_on_a_plain_entry_activates_it() {
        let (mut cascade, start) = (Cascade::new(), Instant::now());
        update(&mut cascade, CascadeEvent::Enter(vec![2]), start);
        let chosen = update(&mut cascade, CascadeEvent::Key(CascadeKey::Enter), start);
        assert_eq!(chosen, CascadeChange::Activate { level: 0, index: 2 });
        assert_eq!(update(&mut cascade, CascadeEvent::Key(CascadeKey::Right), start), CascadeChange::None);
    }

    #[test]
    fn a_submenu_hangs_beside_its_entry_and_flips_at_the_right_edge() {
        let viewport = Rectangle::new(Point::ORIGIN, iced::Size::new(1000.0, 800.0));
        let card = iced::Size::new(200.0, 150.0);
        let entry = Rectangle::new(Point::new(104.0, 300.0), iced::Size::new(232.0, 30.0));
        let right = place(entry, card, viewport, Placement::Cascade);
        assert_eq!(right, Point::new(340.0, 296.0), "the menu's right edge, first entries level");
        let near = Rectangle::new(Point::new(704.0, 300.0), iced::Size::new(232.0, 30.0));
        let left = place(near, card, viewport, Placement::Cascade);
        assert_eq!(left, Point::new(500.0, 296.0), "on the menu's left at the window's right edge");
        let low = Rectangle::new(Point::new(104.0, 760.0), iced::Size::new(232.0, 30.0));
        let low = place(low, card, viewport, Placement::Cascade);
        assert!(low.y + card.height <= 800.0, "moved up to stay in the window");
    }

    /// The design review of #906: an entry records where it was laid out, before its card moved to
    /// the pointer; its submenu must hang where the entry is shown.
    #[test]
    fn an_entry_moves_with_its_card() {
        let cascade = Cascade::new();
        cascade.anchor(&[2]).set(Rectangle::new(Point::new(4.0, 64.0), iced::Size::new(232.0, 30.0)));
        cascade.shift(0).set(Vector::new(300.0, 120.0));
        let shown = cascade.entry_rect(&[2]).expect("laid out");
        assert_eq!(shown, Rectangle::new(Point::new(304.0, 184.0), iced::Size::new(232.0, 30.0)));
        assert_eq!(cascade.entry_rect(&[]), None);
    }

    #[test]
    fn a_submenu_goes_under_its_entry_without_room_on_either_side() {
        let viewport = Rectangle::new(Point::ORIGIN, iced::Size::new(480.0, 900.0));
        let entry = Rectangle::new(Point::new(12.0, 300.0), iced::Size::new(232.0, 30.0));
        let at = place(entry, iced::Size::new(300.0, 200.0), viewport, Placement::Cascade);
        assert_eq!(at.y, 330.0, "under the entry");
        assert!(at.x > entry.x, "moved in a little");
    }

    #[test]
    fn the_triangle_points_at_the_card() {
        let card = Rectangle::new(Point::new(200.0, 0.0), iced::Size::new(100.0, 200.0));
        let from = Point::new(100.0, 100.0);
        assert!(aims_at(from, Point::new(150.0, 90.0), card));
        assert!(!aims_at(from, Point::new(90.0, 100.0), card), "away from it");
        assert!(!aims_at(from, Point::new(110.0, 180.0), card), "steeply down past it");
        assert!(!aims_at(from, from, card), "no move");
        // A card on the left, after the menu flipped.
        let left = Rectangle::new(Point::new(0.0, 0.0), iced::Size::new(80.0, 200.0));
        assert!(aims_at(from, Point::new(90.0, 110.0), left));
    }
}
