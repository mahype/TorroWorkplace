//! Draws a mail's cleaned HTML inside an Iced window (task M9.6, decision 9 in `docs/mail.md`).
//!
//! Blitz parses the HTML, Stylo resolves the CSS, Taffy lays it out and Vello paints it on the CPU into a
//! picture that Iced shows (way 2 of the trial M9.4, `docs/research.md` section 8). There is no JavaScript:
//! Blitz has none. The HTML is expected to be cleaned by the core already (`mail.get_message` with
//! `include_html`, M9.1/M9.5); this library adds a second line: it only ever hands pictures to Blitz. Blitz
//! also asks for style sheets (`@import`), fonts (`@font-face`) and pictures in CSS `url()`, and its request
//! does not say which kind it wants, so these go out like a picture's; their answer is refused unless it is a
//! picture.
//!
//! Two pieces, like Iced's `text_editor`:
//!
//! - [`Content`] holds one parsed mail. Keep it in the app state while the mail is open; build a new one for
//!   the next mail. Pictures start loading as soon as it is built.
//! - [`HtmlView`] is the widget that shows a [`Content`]: as wide as its parent, as tall as the mail, so it
//!   sits in the reader's `scrollable` like any other content.
//!
//! ```ignore
//! // state: content: torro_htmlview::Content  (built with Content::new(&html) or Content::with_cid(…))
//! // view:
//! scrollable(
//!     HtmlView::new(&self.content)
//!         .scheme(Scheme::Dark)
//!         .scale(self.window_scale * self.zoom.factor())
//!         .on_link(Message::LinkClicked),
//! )
//! ```
//!
//! Pictures: `http`/`https` through the workspace's `reqwest` (rustls, no cookies, no `Referer`, redirects
//! followed), `data:` decoded here, `cid:` from the app through [`CidSource`]. Only PNG, JPEG, GIF and WebP
//! (recognised by their first bytes, never SVG), at most [`MAX_PICTURE_BYTES`] (10 MiB) per picture and 50 MiB
//! per mail. Only public hosts: no loopback, no local network, no link-local, neither directly nor through a
//! redirect or a host name that resolves there. Nothing is logged about a mail: no URL, no text, no content id.

mod content;
mod fetch;
mod recolor;
mod widget;

pub use content::{Content, Snapshot};
pub use fetch::{CidSource, MAX_PICTURE_BYTES};
pub use widget::HtmlView;

/// The colours a mail is drawn in. Only the mail's content follows it, never the rest of the window.
///
/// The app picks it from the setting `mail_appearance` (M9.5): `light` and `dark` directly, `app` from the
/// suite's current appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scheme {
    /// The original colours, on the light content surface of the design (white in the standard design).
    #[default]
    Light,
    /// Recoloured while painting: light backgrounds turn dark, dark text turns light, the hue stays, text keeps
    /// at least 4.5:1 against what lies under it. Pictures stay as they are. The surface is the dark content
    /// surface of the design. A mail with its own `prefers-color-scheme: dark` rules gets them, too.
    Dark,
}
