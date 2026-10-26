//! The pieces a settings page is built from: rows of a group, rows that open a page, and the
//! dialog over the window. Used by the sections of this library and by the control centre, so a
//! row looks and behaves the same in both (task S.16; they grew in the control centre's `ui/mod.rs`).
//!
//! Local stand-ins for what `torro_gui::widgets` does not offer yet (proposals for torro-gui in the
//! report of task W.2): `widgets::group_row` has no mark in front, wraps long labels and cannot be
//! pressed, and there is no dialog surface of its own.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Text, button, column, container, row, svg, text_input};
use iced::{Alignment, Element, Fill, Padding, Theme};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::{style, typo, widgets};

/// A row of a settings group (`widgets::group`) with the metrics of `widgets::group_row` – 44
/// high, 56 with a `detail`, 16 at the sides, 16 before the `control` – plus an optional `mark`
/// in front (a status or calendar dot) and label and detail on one line each, so a long account
/// name never breaks the row. A line too long for its room is cut with "…" and shows the whole
/// text as a tooltip (`widgets::ellipsized`, task S.22): `BODY` in `text` for the label, `META` in
/// `text_2` for the detail. Both colours are set explicitly, because a surrounding button
/// ([`pressable`]) would otherwise hand the text its own – so a row of a *disabled* button does
/// not fade; no page builds one, and `ui/general.rs` draws its unusable design packs itself.
pub fn setting_row<'a, M: 'a>(
    mark: Option<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let tall = detail.is_some();
    let mut lines = column![widgets::ellipsized(typo::body(), label, theme::text)];
    if let Some(detail) = detail {
        lines = lines.push(widgets::ellipsized(typo::meta(), detail, theme::text_2));
    }
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::m()).align_y(Alignment::Center);
    row_frame(row![text.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center), tall)
}

/// `content` as one line of a settings group: as high as the header bar (64 in Torro, 76 with two lines of
/// text), 16 at the sides.
pub fn row_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, tall: bool) -> Element<'a, M> {
    let height = if tall { size::header() + space::m() } else { size::header() };
    container(content).width(Fill).center_y(height).padding([0.0, space::l()]).into()
}

/// `text` on one line, as wide as there is room, cut off at the end instead of wrapping (Iced 0.14
/// has no ellipsis).
pub fn one_line<'a, M: 'a>(text: Text<'a>) -> Element<'a, M> {
    container(text.wrapping(Wrapping::None)).width(Fill).clip(true).into()
}

/// A text field in the look of the suite, as wide as its row: `BODY` in the field style, 32 high.
/// The caller adds what it does (`.on_input(…)`, `.on_submit(…)`, `.id(…)`); put into a group with
/// [`row_frame`].
pub fn field<'a, M: Clone + 'a>(placeholder: &str, value: &str) -> text_input::TextInput<'a, M> {
    text_input(placeholder, value)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
}

/// `row` (a [`setting_row`]) as a button over its whole width: hover lays `hover` under it, the
/// label stays in `text`. `None` draws it disabled (40 %).
pub fn pressable<'a, M: Clone + 'a>(row: Element<'a, M>, on_press: Option<M>) -> Element<'a, M> {
    button(row).padding(Padding::ZERO).width(Fill).style(style::button::list_row(false)).on_press_maybe(on_press).into()
}

/// One row of a list to pick from (a retention time, a microphone, an engine): its `label`, and a
/// check at the end when it is the chosen one. The whole row is the button.
pub fn choice<'a, M: Clone + 'a>(label: impl IntoFragment<'a>, chosen: bool, on_press: M) -> Element<'a, M> {
    let check: Element<'a, M> = match chosen {
        true => {
            icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
        }
        false => blank().width(icon::SIZE).into(),
    };
    pressable(setting_row(None, label, None, check), Some(on_press))
}

/// The chevron at the end of a row that opens a page or a next step: 16, `text_3`.
pub fn chevron<'a, M: 'a>() -> Element<'a, M> {
    icon::icon(Icon::ChevronRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

/// A dialog over the whole window, for a page's `overlay`: `content` on the dialog surface (`menu`,
/// radius 10, shadow, 24 inside), centred and at most `max_width` wide; the window behind it is
/// dimmed, and a click next to it sends `on_dismiss`. Escape is the page's `escape`.
pub fn dialog<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, max_width: f32, on_dismiss: M) -> Element<'a, M> {
    let card = container(content).padding(space::xl()).width(Fill).style(style::container::popover);
    // `modal` takes its size from its base; an empty base that fills the window.
    widgets::modal(blank().width(Fill).height(Fill), card, max_width, on_dismiss)
}

/// The confirmation before something is removed, as a [`dialog`]: `title` in `TITLE`, the
/// `consequence` in `text_2`, and at the right "Cancel" and the `confirm` button (danger). A
/// click next to it cancels.
pub fn confirm<'a, M: Clone + 'a>(
    title: &'a str,
    consequence: String,
    confirm: &'a str,
    on_confirm: M,
    on_cancel: M,
) -> Element<'a, M> {
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(on_cancel.clone())),
        widgets::text_button(None, confirm, style::button::danger, Some(on_confirm)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let content =
        column![typo::text(typo::title(), title), typo::text(typo::body(), consequence).style(theme::text_2), buttons]
            .spacing(space::l());
    dialog(content, widgets::POPOVER_WIDTH, on_cancel)
}

/// A failed request or load above the page: an alert icon in the error colour, the message in
/// `text` – the colour is only in the icon.
pub fn notice<'a, M: 'a>(problem: String) -> Element<'a, M> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::body(), problem)].spacing(space::s()).align_y(Alignment::Center).into()
}

/// A switch of a settings row (`toggler` in the suite's look), with what a flip reports.
pub fn switch<'a, M: Clone + 'a>(on: bool, change: impl Fn(bool) -> M + 'a) -> Element<'a, M> {
    iced::widget::toggler(on).size(style::CHECK_SIZE).style(style::toggler).on_toggle(change).into()
}

/// A row's choice as a pick list in the look of a secondary button ("Show new mail", "Mail
/// colors"): `options` as their `Display` says, `chosen` in the closed list.
pub fn picker<'a, T, L, M>(options: L, chosen: T, on_pick: impl Fn(T) -> M + 'a) -> Element<'a, M>
where
    T: std::fmt::Display + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    M: Clone + 'a,
{
    iced::widget::pick_list(options, Some(chosen), on_pick)
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into()
}
