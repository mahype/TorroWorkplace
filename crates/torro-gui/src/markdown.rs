//! Markdown to read, in the Torro look (task G.12 in `docs/tasks/github.md`): the text of GitHub
//! issues and pull requests in the tasks app's panel.
//!
//! Built on Iced's `markdown` widget (pulldown-cmark), with every block drawn by [`Look`] instead
//! of Iced's default look: Inter in the type scale, colours from the [`Palette`], no stock styles.
//!
//! - Heading 1 in `TITLE`, the other levels in `BODY_STRONG`, both in `text`.
//! - Paragraphs in `BODY`, `text_2`, like the notes; links underlined in `text`. Bold becomes
//!   SemiBold: the bundled Inter stops at 600 (`torro_gui::fonts`).
//! - Lists with a small dot, a number or – for task lists – a square that is ticked or not.
//! - Code in the system's monospace face on `raised` (Inter has no monospace cut), code blocks
//!   scroll sideways instead of wrapping.
//! - Quotes behind a line in `line_strong`, rules in `line`; tables in Iced's layout.
//! - Images are not fetched: their alternative text shows as a link to the image.
//!
//! HTML inside the text (comments of issue templates, `<details>`) is dropped by the parser.
//! [`view`] produces the link a click asks for, unchecked: it is foreign text, so the caller opens
//! only addresses it trusts (the tasks app only `http`/`https`).
//!
//! ```ignore
//! use torro_gui::markdown;
//!
//! let items = markdown::parse(&issue.body); // once, when the text changes
//! markdown::view(&items, palette(theme)).map(Message::OpenLink)
//! ```

use iced::widget::markdown::{self, Bullet, HeadingLevel, Settings, Text, Uri, Viewer};
use iced::widget::text::Span;
use iced::widget::{button, column, container, rich_text, row, rule, scrollable};
use iced::{Alignment, Element, Fill, Font, Padding, Theme, border, font, padding};

pub use iced::widget::markdown::Item;

use crate::icon::{self, Icon};
use crate::style;
use crate::theme::{Palette, radius, space};
use crate::typo::{self, Level};
use crate::widgets;

/// The face for code. Inter has no monospace cut, so code takes the system's.
pub const MONO: Font = Font::MONOSPACE;

/// The width of the column for a list's dot, square or number.
const MARK: f32 = space::XL;

/// The blocks of `source`, to keep while the text stays the same: parsing on every frame would
/// be wasted work.
pub fn parse(source: &str) -> Vec<Item> {
    markdown::parse(source).collect()
}

/// `items` (from [`parse`]) as a column of blocks in the colours of `palette`. A click on a link
/// produces its address.
pub fn view<'a>(items: &'a [Item], palette: &'static Palette) -> Element<'a, Uri> {
    markdown::view_with(items, settings(palette), &Look { palette })
}

/// Sizes and the inline style: the base text `BODY`, headings by [`heading_level`], code `META`,
/// `M` between blocks.
fn settings(palette: &Palette) -> Settings {
    let mut settings = Settings::with_text_size(typo::BODY.size, inline_style(palette));
    settings.h1_size = heading_level(HeadingLevel::H1).size;
    for size in [&mut settings.h2_size, &mut settings.h3_size, &mut settings.h4_size] {
        *size = heading_level(HeadingLevel::H2).size;
    }
    settings.h5_size = settings.h4_size;
    settings.h6_size = settings.h4_size;
    settings.code_size = typo::META.size;
    settings.spacing = space::M.into();
    settings
}

/// How spans inside a block look: Inter, links in `text`, inline code on `raised` with a hairline.
fn inline_style(palette: &Palette) -> markdown::Style {
    markdown::Style {
        font: typo::BODY.font,
        inline_code_highlight: markdown::Highlight {
            background: palette.raised.into(),
            border: border::rounded(radius::XS).color(palette.line_strong).width(1),
        },
        inline_code_padding: Padding::ZERO.left(space::XS / 2.0).right(space::XS / 2.0),
        inline_code_color: palette.text,
        inline_code_font: MONO,
        code_block_font: MONO,
        link_color: palette.text,
    }
}

/// The type level of a heading: `TITLE` for the first level, `BODY_STRONG` for the rest. The type
/// scale has no steps in between, and a description is not a document with many levels.
fn heading_level(level: HeadingLevel) -> Level {
    match level {
        HeadingLevel::H1 => typo::TITLE,
        _ => typo::BODY_STRONG,
    }
}

/// The spans of `text` in `style`, with the two changes Iced's own spans need here: bold becomes
/// SemiBold (there is no bolder Inter), and links are underlined, so they stand out without a
/// colour of their own.
fn spans(text: &Text, style: markdown::Style) -> Vec<Span<'static, Uri>> {
    text.spans(style)
        .iter()
        .map(|span| {
            let mut span = span.clone();
            if let Some(font) = span.font.as_mut()
                && font.weight == font::Weight::Bold
            {
                font.weight = font::Weight::Semibold;
            }
            span.underline |= span.link.is_some();
            span
        })
        .collect()
}

/// The height of one line of body text, to centre a list's mark on the first line.
fn body_line() -> f32 {
    typo::BODY.line_height.to_absolute(typo::BODY.size).0
}

/// Draws each kind of block in the Torro look; see the module documentation.
struct Look {
    palette: &'static Palette,
}

impl<'a> Viewer<'a, Uri> for Look {
    fn on_link_click(url: Uri) -> Uri {
        url
    }

    fn image(&self, settings: Settings, url: &'a Uri, _title: &'a str, alt: &Text) -> Element<'a, Uri> {
        let palette = self.palette;
        let mut label = spans(alt, settings.style);
        if label.iter().all(|span| span.text.trim().is_empty()) {
            label = vec![Span::new(url.clone())];
        }
        let link = icon::icon(Icon::Link, icon::SIZE)
            .style(move |_: &Theme, _| iced::widget::svg::Style { color: Some(palette.text_3) });
        let label = rich_text(label)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .color(palette.text)
            .on_link_click(Self::on_link_click);
        button(row![link, label].spacing(space::S).align_y(Alignment::Center))
            .padding(Padding::ZERO)
            .style(style::button::ghost)
            .on_press(url.clone())
            .into()
    }

    fn heading(&self, settings: Settings, level: &'a HeadingLevel, text: &'a Text, index: usize) -> Element<'a, Uri> {
        let level = heading_level(*level);
        let style = markdown::Style { font: level.font, ..settings.style };
        let heading = rich_text(spans(text, style))
            .size(level.size)
            .line_height(level.line_height)
            .color(self.palette.text)
            .on_link_click(Self::on_link_click);
        // A heading belongs to what follows: a little more room above it than between paragraphs.
        let above = if index > 0 { space::XS } else { 0.0 };
        container(heading).padding(padding::top(above)).into()
    }

    fn paragraph(&self, settings: Settings, text: &Text) -> Element<'a, Uri> {
        rich_text(spans(text, settings.style))
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .color(self.palette.text_2)
            .on_link_click(Self::on_link_click)
            .into()
    }

    fn code_block(
        &self,
        settings: Settings,
        _language: Option<&'a str>,
        _code: &'a str,
        lines: &'a [Text],
    ) -> Element<'a, Uri> {
        let palette = self.palette;
        // Without syntax highlighting every span takes the block's font, so the style carries it.
        let style = markdown::Style { font: MONO, ..settings.style };
        let lines = column(lines.iter().map(|line| {
            rich_text(spans(line, style))
                .size(typo::META.size)
                .line_height(typo::META.line_height)
                .color(palette.text)
                .on_link_click(Self::on_link_click)
                .into()
        }));
        let scroll = scrollable(container(lines).padding(space::M))
            .direction(scrollable::Direction::Horizontal(style::scrollbar()))
            .style(style::scrollable);
        container(scroll)
            .width(Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(palette.raised.into()),
                border: border::rounded(radius::SM).color(palette.line).width(1),
                ..container::Style::default()
            })
            .into()
    }

    fn unordered_list(&self, settings: Settings, bullets: &'a [Bullet]) -> Element<'a, Uri> {
        let palette = self.palette;
        let rows = bullets.iter().map(|bullet| {
            let (mark, items) = match bullet {
                Bullet::Point { items } => (widgets::color_dot(palette.text_3, space::XS + space::XS / 2.0), items),
                Bullet::Task { items, done } => {
                    let square = if *done { Icon::SquareCheck } else { Icon::Square };
                    let colour = if *done { palette.text_2 } else { palette.text_3 };
                    let mark = icon::icon(square, icon::SIZE)
                        .style(move |_: &Theme, _| iced::widget::svg::Style { color: Some(colour) });
                    (mark.into(), items)
                }
            };
            self.list_row(settings, mark, items)
        });
        column(rows).spacing(space::XS).into()
    }

    fn ordered_list(&self, settings: Settings, start: u64, bullets: &'a [Bullet]) -> Element<'a, Uri> {
        let rows = bullets.iter().zip(start..).map(|(bullet, number)| {
            let (Bullet::Point { items } | Bullet::Task { items, .. }) = bullet;
            let number = typo::text(typo::BODY.tabular(), format!("{number}.")).style(crate::theme::text_3);
            self.list_row(settings, number.into(), items)
        });
        column(rows).spacing(space::XS).into()
    }

    fn quote(&self, settings: Settings, contents: &'a [Item]) -> Element<'a, Uri> {
        let palette = self.palette;
        let line = rule::vertical(2).style(move |_: &Theme| rule::Style {
            color: palette.line_strong,
            radius: border::Radius::default(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        });
        let blocks =
            column(contents.iter().enumerate().map(|(index, item)| markdown::item(self, settings, item, index)))
                .spacing(settings.spacing.0);
        row![line, blocks].spacing(space::M).height(iced::Shrink).into()
    }

    fn rule(&self, _settings: Settings) -> Element<'a, Uri> {
        rule::horizontal(1).style(style::rule).into()
    }
}

impl Look {
    /// One entry of a list: `mark` in its column, centred on the first line, then the entry's
    /// blocks with less room between them than between the blocks of the text.
    fn list_row<'a>(&self, settings: Settings, mark: Element<'a, Uri>, items: &'a [Item]) -> Element<'a, Uri> {
        let mark = container(mark).width(MARK).height(body_line()).align_y(Alignment::Center);
        let inner = Settings { spacing: space::XS.into(), ..settings };
        row![mark, markdown::view_with(items, inner, self)].into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds of the top-level blocks, as words, to compare without Iced's types.
    fn kinds(items: &[Item]) -> Vec<&'static str> {
        items
            .iter()
            .map(|item| match item {
                Item::Heading(..) => "heading",
                Item::Paragraph(_) => "paragraph",
                Item::CodeBlock { .. } => "code",
                Item::List { start: None, .. } => "list",
                Item::List { start: Some(_), .. } => "numbered",
                Item::Image { .. } => "image",
                Item::Quote(_) => "quote",
                Item::Rule => "rule",
                Item::Table { .. } => "table",
            })
            .collect()
    }

    #[test]
    fn an_issue_text_parses_into_its_blocks() {
        let text = include_str!("../tests/fixtures/issue-body.md");
        assert_eq!(
            kinds(&parse(text)),
            ["heading", "paragraph", "list", "heading", "numbered", "code", "quote", "rule", "table", "image"]
        );
    }

    #[test]
    fn html_comments_of_issue_templates_leave_nothing() {
        assert!(parse("<!-- Describe the bug -->").is_empty());
    }

    #[test]
    fn task_lists_keep_whether_a_task_is_done() {
        let items = parse("- [x] done\n- [ ] open\n- plain");
        let [Item::List { bullets, .. }] = items.as_slice() else { panic!("one list: {items:?}") };
        let done: Vec<Option<bool>> = bullets
            .iter()
            .map(|bullet| match bullet {
                Bullet::Task { done, .. } => Some(*done),
                Bullet::Point { .. } => None,
            })
            .collect();
        assert_eq!(done, [Some(true), Some(false), None]);
    }

    #[test]
    fn bold_becomes_semibold_and_links_are_underlined() {
        let items = parse("**strong** and [a link](https://example.org)");
        let [Item::Paragraph(text)] = items.as_slice() else { panic!("one paragraph: {items:?}") };
        let spans = spans(text, inline_style(Palette::dark()));
        let strong = spans.iter().find(|span| span.text == "strong").expect("strong span");
        assert_eq!(strong.font.map(|font| font.weight), Some(font::Weight::Semibold));
        let link = spans.iter().find(|span| span.link.is_some()).expect("link span");
        assert_eq!(link.link.as_deref(), Some("https://example.org"));
        assert!(link.underline);
        assert!(spans.iter().filter(|span| span.link.is_none()).all(|span| !span.underline));
    }
}
