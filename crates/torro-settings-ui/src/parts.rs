//! The pieces a settings page is built from: rows of a group, rows that open a page, and the
//! dialogs over the window (a confirmation, the question before mail goes to the cloud). Used by the sections of this library and by the control centre, so a
//! row looks and behaves the same in both (task S.16; they grew in the control centre's `ui/mod.rs`).
//!
//! Local stand-ins for what `torro_gui::widgets` does not offer yet (proposals for torro-gui in the
//! report of task W.2): `widgets::group_row` has no mark in front, wraps long labels and cannot be
//! pressed, and there is no dialog surface of its own.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Text, button, column, container, row, svg, text_input};
use iced::{Alignment, Element, Fill, Padding, Theme};

use torro_core::ai::AiModel;
use torro_core::i18n::{tr, trf};
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
    // The row spans the page, its text keeps the reading width (issue #464).
    row_frame(
        row![widgets::reading_cell(text.width(Fill)), control.into()].spacing(space::l()).align_y(Alignment::Center),
        tall,
    )
}

/// `content` as one line of a settings group: as high as the header bar (64 in Torro, 76 with two lines of
/// text), 16 at the sides.
pub fn row_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, tall: bool) -> Element<'a, M> {
    row_frame_with_lines(content, tall, 0)
}

/// Like [`row_frame`], with `extra` more lines of `meta` text under the two a tall row holds (issue
/// #431: the note under the facts of a model), so the height is counted in one place.
pub fn row_frame_with_lines<'a, M: 'a>(content: impl Into<Element<'a, M>>, tall: bool, extra: usize) -> Element<'a, M> {
    let base = if tall { size::header() + space::m() } else { size::header() };
    let height = base + typo::line_height(typo::meta()) * extra as f32;
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

/// The question before mail is sorted by a model in the cloud (issue #424), as a [`dialog`]: "Sort
/// mail in the cloud?", which `model` reads the mail at which `provider`, and at the right "Cancel"
/// and "Use". Choosing the model is the consent, so there is no switch of its own; "Use" is the
/// primary button, not a danger one – nothing is removed. A click next to it cancels.
pub fn cloud_consent<'a, M: Clone + 'a>(model: &str, provider: &str, on_use: M, on_cancel: M) -> Element<'a, M> {
    let question = trf(
        "{model} reads your mail at {provider} outside this computer. Use it?",
        &[("model", &model), ("provider", &provider)],
    );
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(on_cancel.clone())),
        widgets::text_button(None, tr("Use"), style::button::primary, Some(on_use)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let content = column![
        typo::text(typo::title(), tr("Send mail outside this computer?")),
        typo::text(typo::body(), question).style(theme::text_2),
        buttons
    ]
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

/// A row of a settings group whose choice of up to four options is a [`widgets::segmented`] control
/// (issue #554, the rule of choices): beside `label` (and its `detail`) while both fit, under them
/// where they do not (`widgets::segment_column`). `options` are the words and what each sends,
/// `active` the one chosen. More options take a `widgets::Dropdown`.
pub fn segmented_row<'a, M: Clone + 'a>(
    label: &'a str,
    detail: Option<String>,
    options: Vec<(String, M)>,
    active: usize,
) -> Element<'a, M> {
    let mut rows = segmented_rows(vec![SegmentedRow { label, detail, options, active }]);
    rows.pop().unwrap_or_else(|| iced::widget::space().into())
}

/// One row of [`segmented_rows`]: what [`segmented_row`] takes.
pub struct SegmentedRow<'a, M> {
    /// The row's name.
    pub label: &'a str,
    /// The line under the name, if any.
    pub detail: Option<String>,
    /// The words of the segments and what each sends.
    pub options: Vec<(String, M)>,
    /// The segment chosen.
    pub active: usize,
}

/// Several [`segmented_row`]s that decide together (design review of #554): all beside their labels
/// in one column as wide as the widest control, or all under them, so the controls of a page stand
/// on one axis instead of some beside and some under their labels.
pub fn segmented_rows<'a, M: Clone + 'a>(rows: Vec<SegmentedRow<'a, M>>) -> Vec<Element<'a, M>> {
    let slot = rows
        .iter()
        .map(|row| {
            let words: Vec<&str> = row.options.iter().map(|(word, _)| word.as_str()).collect();
            widgets::segmented_width(&words)
        })
        .fold(0.0, f32::max);
    let texts: Vec<(&'a str, Option<String>)> = rows.iter().map(|row| (row.label, row.detail.clone())).collect();
    rows.into_iter()
        .map(|SegmentedRow { label, detail, options, active }| {
            let texts = texts.clone();
            iced::widget::responsive(move |room| {
                let peers = texts.iter().map(|(label, detail)| (*label, detail.as_deref()));
                let column = widgets::segment_column(slot, peers, room.width);
                widgets::segment_row(label, detail.clone(), widgets::segmented(options.clone(), active), column)
            })
            .height(iced::Length::Shrink)
            .into()
        })
        .collect()
}

/// A settings row whose control is `width` wide (issue #554): `beside` – the whole row, the control
/// at its end – while `label` and `detail` fit beside it (`widgets::segment_column`), else the label
/// and detail with `under` on a line of its own below them. For a row whose control holds segments
/// next to something else (an address field, "Remove"), where [`segmented_row`] does not fit.
pub fn stacking_row<'a, M: 'a>(
    label: &'a str,
    detail: Option<String>,
    width: f32,
    beside: impl Fn() -> Element<'a, M> + 'a,
    under: impl Fn() -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    iced::widget::responsive(move |room| {
        match widgets::segment_column(width, [(label, detail.as_deref())], room.width) {
            Some(_) => beside(),
            None => widgets::segment_row(label, detail.clone(), under(), None),
        }
    })
    .height(iced::Length::Shrink)
    .into()
}

/// "131K context", "1M context": how much fits into one conversation, rounded to thousands or, from
/// a million on, to millions of tokens.
pub fn context_text(tokens: u64) -> String {
    match tokens {
        1_000_000.. => trf("{n}M context", &[("n", &((tokens + 500_000) / 1_000_000))]),
        _ => trf("{n}K context", &[("n", &((tokens + 500) / 1000).max(1))]),
    }
}

/// How big a model is, as far as its server tells (issue #431): its parameters (`8.2B`), its bytes
/// on the disk (`4.9 GB`), its quantization (`Q4_K_M`) and its context (`41K context`), in this
/// order. What is unknown is left out, never written as "unknown". Used by the model choice of the
/// mail settings and by the models of an AI account in the control centre, so both say the same.
pub fn size_facts(model: &AiModel) -> Vec<String> {
    let mut facts = Vec::new();
    facts.extend(model.parameters.clone());
    facts.extend(model.size.map(torro_gui::units::format_bytes));
    facts.extend(model.quantization.clone());
    facts.extend(model.context.map(context_text));
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_is_counted_in_thousands_or_millions() {
        assert_eq!(context_text(131_072), trf("{n}K context", &[("n", &131)]));
        assert_eq!(context_text(200_000), trf("{n}K context", &[("n", &200)]));
        assert_eq!(context_text(300), trf("{n}K context", &[("n", &1)]));
        assert_eq!(context_text(1_048_576), trf("{n}M context", &[("n", &1)]));
    }

    /// Issue #431: parameters, disk, quantization and context in this order; unknown ones are left out.
    #[test]
    fn the_size_of_a_model_lists_only_what_is_known() {
        let model = AiModel {
            parameters: Some("8.2B".into()),
            quantization: Some("Q4_K_M".into()),
            size: Some(5_225_388_164),
            context: Some(40_960),
            ..AiModel::named("qwen3:8b")
        };
        let disk = torro_gui::units::format_bytes(5_225_388_164);
        assert_eq!(size_facts(&model), ["8.2B".to_string(), disk, "Q4_K_M".into(), context_text(40_960)]);
        let cloud = AiModel { size: None, quantization: None, ..model };
        assert_eq!(size_facts(&cloud), ["8.2B".to_string(), context_text(40_960)]);
        assert!(size_facts(&AiModel::named("gpt-5-mini")).is_empty());
    }
}
