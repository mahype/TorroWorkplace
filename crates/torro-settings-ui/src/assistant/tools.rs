//! Assistant › Tools: where the assistant's own tools went (task MA.7, issue #302). The switches, the
//! PDF tool's folder and its templates (task WZ.6) moved to the control centre's section "MCP
//! servers" as its built-in tools, next to the MCP servers, because a tool exists for every client there, not only for
//! the assistant (`docs/assistent.md`, "Eigene Werkzeuge").
//!
//! ```text
//! Tools     Tools moved to the control centre ↗
//! ```
//!
//! **Nothing here is sent for tools any more:** the one row, a link to another window like the others
//! of these settings (an arrow, the whole row pressable), only opens the control centre at that
//! section ([`crate::Action::ControlCentre`] with [`CONTROL_CENTRE_SECTION`]). What the assistant may
//! use stays its rights, which Access in this window edits (task S.34).

use iced::widget::svg;
use iced::{Element, Theme};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::palette;
use torro_gui::widgets;

use super::Action;
use crate::parts::{pressable, setting_row};

/// The word of the control centre's section with the tools (`torroworkplace ui --section
/// connections`): what the row hands to [`crate::Action::ControlCentre`].
pub const CONTROL_CENTRE_SECTION: &str = "connections";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The row: the control centre at MCP servers.
    Open,
}

/// The row opens the control centre at [`CONTROL_CENTRE_SECTION`]; nothing else is sent.
pub fn update(message: Message) -> Action {
    match message {
        Message::Open => Action::ControlCentre(CONTROL_CENTRE_SECTION.to_string()),
    }
}

/// One group with one row: where the tools are now, with the arrow of another window, the whole row
/// pressable – as the control centre links to the assistant. The same while the assistant is off: the
/// tools no longer wait for it.
pub fn view<'a>() -> Element<'a, Message> {
    let detail = tr("Under MCP servers › Built-in tools");
    let row = setting_row(None, tr("Tools moved to the control centre"), Some(detail.to_string()), arrow());
    widgets::group(tr("Tools"), vec![pressable(row, Some(Message::Open))], None)
}

/// The arrow at the end of a row that leads to another window: 16, `text_3`.
fn arrow<'a>() -> Element<'a, Message> {
    icon::icon(Icon::ArrowUpRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action as Act;

    /// Task MA.7: the assistant sends nothing for tools any more – the row only leads to the control
    /// centre's section.
    #[test]
    fn the_assistant_sends_nothing_for_tools() {
        assert_eq!(update(Message::Open), Act::ControlCentre("connections".to_string()));
        let _: Element<'_, Message> = view();
    }
}
