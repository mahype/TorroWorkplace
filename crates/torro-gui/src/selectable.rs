//! Text to read that can be selected with the mouse and copied, but not changed (issue #119),
//! and the "Links" of such a text ([`links_section`]), so its addresses stay usable.
//!
//! Iced 0.14 has no selectable text widget; a [`text_editor`] that drops every edit is the way to
//! select text that must not change (the calendar's read-only event does the same). Its links
//! cannot be clicked in it, so the addresses in the text ([`crate::links`]) are listed below it as
//! rows that open them.
//!
//! The editor takes only the keys of reading ([`read_only_key`]): ⌘C / Ctrl + C copies the
//! selection, ⌘A / Ctrl + A selects all, Shift + arrows widen the selection. Every other key passes
//! on to the window, so the app's shortcuts (`e`, `j`, Escape …) keep working while the text has
//! the focus.
//!
//! ```ignore
//! // State: one per text shown, made new when the text changes.
//! self.text = Selectable::new(&mail_text);
//! // Update: Message::Text(action) => self.text.perform(action),
//! // View:
//! column![self.text.view(typo::read(), theme::text_2, Message::Text)]
//!     .push_maybe(selectable::links_section(selectable::web_links(self.text.text()), Message::OpenLink))
//! ```

use iced::widget::text::Wrapping;
use iced::widget::text_editor::{self, Action, Binding, KeyPress, Status};
use iced::widget::{container, row};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use torro_core::i18n::tr;

use crate::icon::Icon;
use crate::theme::{palette, space};
use crate::widgets::{self, Mark};
use crate::{links, style, typo};

/// A text that can be selected and copied but never changed: the text as given ([`Selectable::text`],
/// for "Copy text") and the editor's state (selection, focus) over it.
#[derive(Debug, Default)]
pub struct Selectable {
    text: String,
    content: text_editor::Content,
}

impl Selectable {
    /// `text` to show, nothing selected.
    pub fn new(text: &str) -> Self {
        Selectable { text: text.to_owned(), content: text_editor::Content::with_text(text) }
    }

    /// The whole text as given to [`Selectable::new`], unchanged: what "Copy text" puts on the
    /// clipboard.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Applies a click, a drag, a selection or a move of the editor; edits are dropped, so the text
    /// never changes, whatever the action.
    pub fn perform(&mut self, action: Action) {
        if !action.is_edit() {
            self.content.perform(action);
        }
    }

    /// The text in `level`, coloured by `colour` (a `theme::text_*` function), wrapping at words,
    /// on no background and without a border: it reads like any other text. It can be selected with
    /// the mouse (a double click selects a word, a triple click a line) and copied with ⌘C / Ctrl + C
    /// ([`read_only_key`]). Every action of the editor goes out through `on_action`; hand it to
    /// [`Selectable::perform`].
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        level: typo::Level,
        colour: fn(&Theme) -> iced::widget::text::Style,
        on_action: impl Fn(Action) -> M + 'a,
    ) -> Element<'a, M> {
        iced::widget::text_editor(&self.content)
            .on_action(on_action)
            .key_binding(read_only_key)
            .font(level.font)
            .size(level.size)
            .line_height(level.line_height)
            .padding(Padding::ZERO)
            .wrapping(Wrapping::WordOrGlyph)
            .style(move |theme, status| bare(theme, status, colour(theme).color.unwrap_or(palette(theme).text)))
            .into()
    }
}

/// The keys a read-only text takes while it has the focus: copy, select all, and Shift + arrows to
/// widen the selection; Escape gives the focus back without taking the key, so the window still
/// sees it. Everything else (letters, Enter, plain arrows, paste, cut) is left to the window.
pub fn read_only_key<M>(press: KeyPress) -> Option<Binding<M>> {
    match Binding::<M>::from_key_press(press)? {
        Binding::Copy => Some(Binding::Copy),
        Binding::SelectAll => Some(Binding::SelectAll),
        Binding::Select(motion) => Some(Binding::Select(motion)),
        Binding::Unfocus => Some(Binding::Unfocus),
        _ => None,
    }
}

/// The style of the read-only text: no surface, no border, the text in `value`; the selection as
/// in the suite's text fields.
fn bare(theme: &Theme, status: Status, value: Color) -> text_editor::Style {
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: palette(theme).text_3,
        value,
        ..style::text_editor(theme, status)
    }
}

/// The web addresses in `text` that may be opened ([`links::links`], [`links::openable`]), each
/// once, in the order they appear. `mailto:` addresses are left out: a click on a link opens the
/// browser, never another program.
pub fn web_links(text: &str) -> Vec<String> {
    links::links(text)
        .into_iter()
        .filter(|url| links::openable(url) && !url.to_ascii_lowercase().starts_with("mailto:"))
        .map(str::to_owned)
        .collect()
}

/// "Links" with a row for each address in `urls` ([`link_row`]); `None` without any, so nothing
/// shows. `open` makes the message of a row; the app checks the address again before it opens it
/// (`links::openable`).
pub fn links_section<'a, M: Clone + 'a>(urls: Vec<String>, open: impl Fn(String) -> M) -> Option<Element<'a, M>> {
    if urls.is_empty() {
        return None;
    }
    let rows = urls.into_iter().map(|url| link_row(url, &open)).collect();
    Some(widgets::prop_section(tr("Links"), rows))
}

/// One row of "Links", as the calendar shows them: a video call with the service's name and "Join"
/// (the address as its tooltip); any other address whole, cut with "…", the row opens it.
pub fn link_row<'a, M: Clone + 'a>(url: String, open: impl Fn(String) -> M) -> Element<'a, M> {
    let message = open(url.clone());
    match links::meeting(&url) {
        Some(service) => {
            let join = widgets::text_button(None, tr("Join"), style::button::secondary, Some(message));
            let value =
                row![container(widgets::prop_value(service)).width(Length::Fill), widgets::with_tooltip(join, url)]
                    .spacing(space::s())
                    .align_y(Alignment::Center);
            widgets::prop_row(Some(Mark::Icon(Icon::Video)), value, None)
        }
        None => widgets::prop_row(Some(Mark::Icon(Icon::ArrowUpRight)), widgets::prop_value(url), Some(message)),
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::{self, Key, key};
    use iced::widget::text_editor::{Edit, Motion};

    use super::*;

    fn press(key: Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> KeyPress {
        let physical_key = key::Physical::Unidentified(key::NativeCode::Unidentified);
        KeyPress {
            key: key.clone(),
            modified_key: key,
            physical_key,
            modifiers,
            text: text.map(Into::into),
            status: Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn edits_never_change_the_text() {
        let mut text = Selectable::new("cargo test --workspace");
        text.perform(Action::SelectAll);
        text.perform(Action::Edit(Edit::Delete));
        text.perform(Action::Edit(Edit::Insert('x')));
        text.perform(Action::Edit(Edit::Paste(std::sync::Arc::new("pasted".into()))));
        assert_eq!(text.content.text(), "cargo test --workspace");
        assert_eq!(text.text(), "cargo test --workspace", "copied exactly as given, hyphens included");
        assert_eq!(text.content.selection().as_deref(), Some("cargo test --workspace"), "the selection stays");
    }

    #[test]
    fn only_the_keys_of_reading_are_taken() {
        let command = if cfg!(target_os = "macos") { keyboard::Modifiers::LOGO } else { keyboard::Modifiers::CTRL };
        let taken = |key: Key, modifiers, text| read_only_key::<()>(press(key, modifiers, text));
        assert!(matches!(taken(Key::Character("c".into()), command, None), Some(Binding::Copy)));
        assert!(matches!(taken(Key::Character("a".into()), command, None), Some(Binding::SelectAll)));
        let shift_right = taken(Key::Named(key::Named::ArrowRight), keyboard::Modifiers::SHIFT, None);
        assert!(matches!(shift_right, Some(Binding::Select(Motion::Right))));
        assert!(matches!(
            taken(Key::Named(key::Named::Escape), keyboard::Modifiers::empty(), None),
            Some(Binding::Unfocus)
        ));
        // Left to the window: its shortcuts, Enter, plain arrows, paste and cut.
        assert!(taken(Key::Character("e".into()), keyboard::Modifiers::empty(), Some("e")).is_none());
        assert!(taken(Key::Named(key::Named::Enter), keyboard::Modifiers::empty(), None).is_none());
        assert!(taken(Key::Named(key::Named::ArrowDown), keyboard::Modifiers::empty(), None).is_none());
        assert!(taken(Key::Character("v".into()), command, None).is_none());
        assert!(taken(Key::Character("x".into()), command, None).is_none());
    }

    #[test]
    fn web_links_leave_out_what_a_click_must_not_open() {
        let text = "See https://example.org/a, mailto:anna@example.org, file:///etc/passwd and https://example.org/a.";
        assert_eq!(web_links(text), ["https://example.org/a"]);
        assert!(web_links("no links").is_empty());
        assert!(links_section(web_links("no links"), |_| ()).is_none(), "no section without links");
        assert!(links_section(web_links(text), |_| ()).is_some());
    }
}
