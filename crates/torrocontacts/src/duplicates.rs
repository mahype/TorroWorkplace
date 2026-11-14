//! The view "Duplicates" and the dialog "Merge contacts?" (issue #726).
//!
//! The core finds contacts that look like one person (`contacts.find_duplicates`, loaded with the list,
//! [`crate::backend::load`]); the window shows each group under why they look alike, the contacts as
//! cards with "Keep this one" in their head (the first, the one with most fields, chosen until the
//! person picks another), and "Mark as different" and "Merge…". The window decides nothing about who is
//! whom: merging and dismissing are commands of the core.
//!
//! Keys ([`App::duplicates_key`](crate::app::App)): ↑ ↓ walk the groups (the one under the cursor has
//! the focus ring), ← → and Space choose the card that stays, Enter is "Merge…", `D` "Mark as
//! different".
//!
//! - **Wide:** the cards of a group side by side.
//! - **Medium and narrow:** one under the other.
//!
//! "Merge…" is off, with the reason under the cards, when the core would refuse: a contact to be
//! deleted or the one kept is in a read-only book, one is in conflict, or the group is larger than one
//! merge may be.

use iced::widget::text::Wrapping;
use std::time::Duration;

use iced::widget::{Space, column, container, radio, row, scrollable, tooltip};
use iced::{Alignment, Element, Fill, Length, Theme, border};
use torro_contacts::{Contact, ContactState, DuplicateGroup, DuplicateReason};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, Palette, size, space};
use torro_gui::widgets;
use torro_gui::{dialog, layout, style, typo};
use torro_ipc::contacts::MERGE_MAX;

use crate::app::{App, Message};
use crate::list::{face, role_line, tinted};
use crate::transfer::problem;

/// The dialog "Merge contacts?" while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    /// The group it merges ([`key`]).
    pub group: String,
    /// `contacts.merge_contacts` runs: the buttons wait.
    pub busy: bool,
    /// Why the merge failed, for people.
    pub error: Option<String>,
}

/// A group's name for the window: its ids, sorted. It stays the same when a load brings the group again
/// with its contacts in another order, so the person's "Keep" stays.
pub fn key(group: &DuplicateGroup) -> String {
    let mut ids: Vec<&str> = group.contacts.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    ids.join("\n")
}

/// Why the contacts of a group look alike, every reason named: "Same mail address · Same number".
pub fn reason_words(reasons: &[DuplicateReason]) -> String {
    let words: Vec<&str> = reasons
        .iter()
        .map(|reason| match reason {
            DuplicateReason::Mail => tr("Same mail address"),
            DuplicateReason::Phone => tr("Same number"),
            DuplicateReason::NameAndCompany => tr("Same name and company"),
        })
        .collect();
    if words.is_empty() { tr("Look alike").to_owned() } else { words.join(" · ") }
}

/// The contact of `group` that stays: `chosen` when it is one of them, else the first (the one with
/// most fields, as the core sorts them).
pub fn kept<'a>(group: &'a DuplicateGroup, chosen: Option<&str>) -> Option<&'a Contact> {
    chosen.and_then(|id| group.contacts.iter().find(|c| c.id == id)).or_else(|| group.contacts.first())
}

/// Why "Merge…" is off for `group` keeping `keep`, in words for people; `None` when it may run.
pub fn blocked(group: &DuplicateGroup, keep: &str) -> Option<String> {
    if group.contacts.len() > MERGE_MAX {
        return Some(trf("At most {n} contacts merge at once.", &[("n", &MERGE_MAX.to_string())]));
    }
    for contact in &group.contacts {
        let name = contact.shown_name();
        if contact.readonly && contact.id == keep {
            return Some(trf(
                "{name} is in a read-only address book. Keep a contact you may change.",
                &[("name", &name)],
            ));
        }
        if contact.readonly {
            return Some(trf("{name} is in a read-only address book and cannot be deleted.", &[("name", &name)]));
        }
        if contact.state == ContactState::Conflict {
            return Some(trf("{name} changed elsewhere. Settle that first.", &[("name", &name)]));
        }
    }
    None
}

/// The ids and versions `contacts.merge_contacts` takes, in the order of the group.
pub fn ids_and_etags(group: &DuplicateGroup) -> (Vec<String>, Vec<String>) {
    group.contacts.iter().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default())).unzip()
}

impl App {
    /// The group of duplicates named `key`.
    pub fn duplicate_group(&self, key_of: &str) -> Option<&DuplicateGroup> {
        self.duplicates.iter().find(|group| key(group) == key_of)
    }

    /// The contact of `group` the person keeps ([`kept`] with their choice).
    pub fn kept_of<'a>(&self, group: &'a DuplicateGroup) -> Option<&'a Contact> {
        kept(group, self.keep.get(&key(group)).map(String::as_str))
    }

    /// "Contacts · Acme" under a card's name: the book, and the company when there is one.
    fn book_line(&self, contact: &Contact) -> String {
        let book = self.book(&contact.book).map(|b| b.display_name.clone()).unwrap_or_default();
        role_line(Some(book.as_str()), contact.org.as_deref())
    }

    /// The view "Duplicates": the groups, or a calm word that there are none.
    pub fn duplicates_view(&self) -> Element<'_, Message> {
        if self.duplicates.is_empty() {
            return crate::view::calm(
                Some(Icon::Copy),
                tr("No duplicates found."),
                tr("Contacts that look like one person show up here."),
                None,
            );
        }
        let side_by_side = self.class() == SizeClass::Wide;
        let colours = torro_gui::theme::palette(&self.theme());
        let groups = self.duplicates.iter().map(|group| self.group_view(group, side_by_side, colours));
        let page = column(groups).spacing(layout::section_gap()).max_width(size::content_max());
        scrollable(container(page).width(Fill).padding(layout::page_padding()))
            .height(Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .into()
    }

    fn group_view<'a>(
        &'a self,
        group: &'a DuplicateGroup,
        side_by_side: bool,
        colours: Palette,
    ) -> Element<'a, Message> {
        let name = key(group);
        let keep = self.kept_of(group).map(|c| c.id.clone()).unwrap_or_default();
        let title = typo::text(typo::caption(), reason_words(&group.reasons)).style(theme::text_3);
        let cards: Vec<Element<'a, Message>> = group
            .contacts
            .iter()
            .enumerate()
            .map(|(at, contact)| self.card(contact, at, &name, &keep, colours))
            .collect();
        let cards: Element<'a, Message> = if side_by_side {
            row(cards).spacing(size::card_gap()).into()
        } else {
            column(cards).spacing(size::card_gap()).into()
        };
        let blocked = blocked(group, &keep);
        let busy = self.merge.as_ref().is_some_and(|m| m.busy && m.group == name);
        let reason: Element<'a, Message> = match &blocked {
            Some(words) => row![
                tinted(Icon::Lock, icon::SIZE_SM, |p| p.text_3),
                typo::text(typo::meta(), words.clone()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .width(Fill)
            .into(),
            None => Space::new().width(Fill).into(),
        };
        // One "Mark as different" at a time; its group's "Merge…" waits for it.
        let marking = self.dismissing.as_deref() == Some(name.as_str());
        let dismiss = (group.contacts.len() <= MERGE_MAX && !busy && self.dismissing.is_none())
            .then(|| Message::NotDuplicates(name.clone()));
        let merge = (blocked.is_none() && !busy && !marking).then(|| Message::AskMerge(name.clone()));
        let different: Element<'a, Message> = if marking {
            widgets::busy_button(tr("Marking…"), style::button::ghost)
        } else {
            key_tip(
                widgets::text_button(None, tr("Mark as different"), style::button::ghost, dismiss),
                tr("These are different people"),
                "D",
            )
        };
        let buttons = row![
            different,
            key_tip(
                widgets::text_button(Some(Icon::Users), tr("Merge…"), style::button::secondary, merge),
                tr("Merge into the contact that stays"),
                "Enter",
            ),
        ]
        .spacing(space::s());
        // Narrow, the reason stands over the buttons, so neither is squeezed.
        let foot: Element<'a, Message> = if side_by_side {
            row![reason, buttons].spacing(space::m()).align_y(Alignment::Center).into()
        } else {
            column![reason, row![Space::new().width(Fill), buttons]].spacing(space::s()).into()
        };
        let content = column![title, cards, foot].spacing(space::m());
        // The group under the keyboard cursor has the focus ring; the others keep the same room, so
        // nothing moves when the cursor walks.
        let focused = self.group_cursor.as_deref() == Some(name.as_str());
        container(content)
            .width(Fill)
            .padding(space::s())
            .style(move |t: &Theme| {
                let ring = if focused { theme::palette(t).focus_ring } else { iced::Color::TRANSPARENT };
                container::Style {
                    border: border::rounded(torro_gui::theme::radius::lg()).color(ring).width(style::FOCUS_RING_WIDTH),
                    ..container::Style::default()
                }
            })
            .into()
    }

    /// One contact of a group as a card: "Keep", the face and name, book · company, mail, number. A
    /// click on the card chooses it too.
    fn card<'a>(
        &'a self,
        contact: &'a Contact,
        at: usize,
        group: &str,
        keep: &str,
        colours: Palette,
    ) -> Element<'a, Message> {
        let chosen = contact.id == keep;
        let pick = Message::Keep(group.to_owned(), contact.id.clone());
        let on_pick = pick.clone();
        // "Keep this one" stands quietly in the head's corner: the card starts with who it is.
        let choice = row![
            radio("", at, chosen.then_some(at), move |_| on_pick.clone())
                .size(style::CHECK_SIZE)
                .spacing(0.0)
                .style(style::radio),
            typo::text(typo::meta(), tr("Keep this one")).style(theme::text_2).wrapping(Wrapping::None),
        ]
        .spacing(space::xs())
        .align_y(Alignment::Center);
        let name = contact.shown_name();
        let picture = self.photos.get(&contact.id).and_then(|(_, p)| p.as_ref());
        let mut title =
            row![typo::text(typo::body_strong(), name.clone()).wrapping(Wrapping::WordOrGlyph).width(Length::Shrink)]
                .spacing(space::xs())
                .align_y(Alignment::Center);
        if contact.readonly {
            title = title.push(widgets::with_tooltip(
                tinted(Icon::Lock, icon::SIZE_SM, |p| p.text_3),
                tr("Read-only address book"),
            ));
        }
        let mut words = column![title].spacing(space::xs());
        let place = self.book_line(contact);
        if !place.is_empty() {
            // One line cut with "…" (whole as tooltip), so the card heads of a group stay alike.
            words = words.push(widgets::ellipsized(typo::meta(), place, theme::text_3));
        }
        let head = row![face(&name, picture, size::avatar(), &colours), words.width(Fill), choice]
            .spacing(space::m())
            .align_y(Alignment::Start);
        let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
        let mut reach = column![].spacing(space::xs());
        for (which, value) in [(Icon::Mail, contact.emails.first()), (Icon::Phone, contact.phones.first())] {
            if let Some(value) = value {
                reach = reach.push(
                    row![
                        tinted(which, icon::SIZE_SM, |p| p.text_3),
                        typo::text(typo::body(), value.value.clone())
                            .style(theme::text_2)
                            .wrapping(Wrapping::WordOrGlyph),
                    ]
                    .spacing(space::s())
                    .align_y(Alignment::Center),
                );
            }
        }
        parts.push(reach.into());
        widgets::card(parts, chosen, Some(pick))
    }

    /// The dialog "Merge contacts?": who stays and gets what, who is deleted for good.
    pub fn merge_dialog(&self) -> Element<'_, Message> {
        let merge = self.merge.as_ref();
        let group = merge.and_then(|m| self.duplicate_group(&m.group));
        let keep = group.and_then(|g| self.kept_of(g));
        let named = |contact: &Contact| {
            let book = self.book(&contact.book).map(|b| b.display_name.clone()).unwrap_or_default();
            if book.is_empty() {
                contact.shown_name()
            } else {
                trf("{name} ({book})", &[("name", &contact.shown_name()), ("book", &book)])
            }
        };
        let stays = keep.map(named).unwrap_or_default();
        let gone: Vec<String> = group
            .map(|g| {
                g.contacts.iter().filter(|c| Some(c.id.as_str()) != keep.map(|k| k.id.as_str())).map(named).collect()
            })
            .unwrap_or_default();
        let head = typo::text(typo::title(), tr("Merge contacts?")).wrapping(Wrapping::WordOrGlyph);
        let mut body = column![
            typo::text(
                typo::body(),
                trf("{name} stays and gets the addresses and numbers of the others.", &[("name", &stays)]),
            )
            .wrapping(Wrapping::WordOrGlyph),
            typo::text(typo::body(), trf("Deleted for good: {names}", &[("names", &gone.join(", "))]))
                .style(theme::text_2)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(space::s());
        if let Some(error) = merge.and_then(|m| m.error.as_deref()) {
            body = body.push(problem(error));
        }
        let busy = merge.is_some_and(|m| m.busy);
        let go: Element<'_, Message> = if busy {
            widgets::busy_button(tr("Merging…"), style::button::danger)
        } else {
            widgets::text_button(None, tr("Merge"), style::button::danger, group.map(|_| Message::Merge))
        };
        let foot = row![
            Space::new().width(Fill),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, (!busy).then_some(Message::CloseDialog)),
            go,
        ]
        .spacing(space::s());
        container(dialog::frame(head, body, foot, space::l()))
            .width(Fill)
            .padding(layout::panel_padding())
            .style(style::container::popover)
            .into()
    }
}

/// `content` with `label` and the key that does the same as tooltip: the word, then the key as `kbd`
/// (as the row actions of the suite show theirs).
fn key_tip<'a>(content: Element<'a, Message>, label: &'a str, key: &'a str) -> Element<'a, Message> {
    let words =
        row![typo::text(typo::caption(), label).style(theme::text_2).wrapping(Wrapping::None), widgets::kbd(key)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    let card = container(words)
        .padding([style::KBD_PADDING.top + style::KBD_PADDING.bottom, space::s()])
        .style(style::container::menu);
    // The suite's tooltips wait half a second, so passing over a button shows nothing.
    tooltip(content, card, tooltip::Position::Bottom).gap(space::xs()).delay(Duration::from_millis(500)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(id: &str, readonly: bool) -> Contact {
        Contact {
            id: id.into(),
            display_name: id.into(),
            readonly,
            etag: Some(format!("e-{id}")),
            ..Contact::default()
        }
    }

    fn group(contacts: Vec<Contact>) -> DuplicateGroup {
        DuplicateGroup { reasons: vec![DuplicateReason::Mail, DuplicateReason::Phone], contacts }
    }

    #[test]
    fn the_first_stays_until_another_is_chosen() {
        let g = group(vec![contact("b", false), contact("a", false)]);
        assert_eq!(kept(&g, None).map(|c| c.id.as_str()), Some("b"));
        assert_eq!(kept(&g, Some("a")).map(|c| c.id.as_str()), Some("a"));
        assert_eq!(kept(&g, Some("gone")).map(|c| c.id.as_str()), Some("b"), "a choice no longer there");
        assert_eq!(key(&g), "a\nb", "the order of the load does not matter");
        assert_eq!(ids_and_etags(&g), (vec!["b".into(), "a".into()], vec!["e-b".into(), "e-a".into()]));
    }

    #[test]
    fn a_read_only_contact_blocks_the_merge() {
        let g = group(vec![contact("a", false), contact("b", true)]);
        let why = blocked(&g, "a").expect("b cannot be deleted");
        assert!(why.contains('b'), "{why}");
        assert!(blocked(&g, "b").is_some(), "b cannot take what a brings either");
        assert_eq!(blocked(&group(vec![contact("a", false), contact("b", false)]), "a"), None);
    }

    #[test]
    fn every_reason_is_named() {
        let words = reason_words(&[DuplicateReason::Mail, DuplicateReason::Phone]);
        assert!(words.contains(tr("Same mail address")) && words.contains(tr("Same number")), "{words}");
        assert_eq!(reason_words(&[DuplicateReason::NameAndCompany]), tr("Same name and company"));
    }
}
