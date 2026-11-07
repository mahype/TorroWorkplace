//! Stable compose blocks (issue #461). Imports reserve a block before any asynchronous work;
//! their replies can replace only that block, never a newer text snapshot.

use iced::widget::text_editor::{Content, Position};
use std::ops::{Deref, DerefMut};
use torro_ipc::mail::PreparedImage;
use torro_mail::{Draft, DraftAttachment, ImageSize, ImageSource, html, markup};

/// The growing editors share one paper scroll area. Use the editor's shaped IME caret rectangle,
/// not a line-count estimate (which is wrong for wrapping and blocks below tall images).
pub(super) fn reveal_caret<'a>(
    content: iced::Element<'a, super::Message>,
    text: &'a Content,
    epoch: Option<u64>,
) -> iced::Element<'a, super::Message> {
    iced::Element::new(RevealCaret { content, text: Some(text), epoch })
}

/// Observe actual offset changes *during input handling*. Iced's `on_scroll` also reports
/// layout changes and programmatic scrolls, neither of which cancels a pending paste.
pub(super) fn paper_scroll(content: iced::Element<'_, super::Message>) -> iced::Element<'_, super::Message> {
    iced::Element::new(RevealCaret { content, text: None, epoch: None })
}

struct RevealCaret<'a> {
    content: iced::Element<'a, super::Message>,
    text: Option<&'a Content>,
    epoch: Option<u64>,
}

#[derive(Default)]
struct RevealState {
    epoch: Option<u64>,
    geometry: Option<(iced::Rectangle, iced::Rectangle, iced::Size)>,
}

impl RevealState {
    fn changed(
        &mut self,
        epoch: u64,
        bounds: iced::Rectangle,
        caret: iced::Rectangle,
        viewport: iced::Rectangle,
    ) -> bool {
        let geometry = (bounds, caret, viewport.size());
        let changed = self.epoch != Some(epoch) || self.geometry != Some(geometry);
        self.epoch = Some(epoch);
        self.geometry = Some(geometry);
        changed
    }

    fn delta(caret: iced::Rectangle, viewport: iced::Rectangle) -> Option<f32> {
        let gap = torro_gui::theme::space::xs();
        let top = viewport.y + gap;
        let bottom = (viewport.y + viewport.height - gap).max(top + caret.height);
        if caret.y < top {
            Some(caret.y - top)
        } else if caret.y + caret.height > bottom {
            Some(caret.y + caret.height - bottom)
        } else {
            None
        }
    }
}

impl iced::advanced::Widget<super::Message, iced::Theme, iced::Renderer> for RevealCaret<'_> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<RevealState>()
    }
    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(RevealState::default())
    }
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> iced::Size<iced::Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> iced::Size<iced::Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, super::Message>,
        viewport: &iced::Rectangle,
    ) {
        let mut messages = Vec::new();
        let mut inner = iced::advanced::Shell::new(&mut messages);
        let before =
            self.text.is_none().then(|| scroll_offset(&mut self.content, &mut tree.children[0], layout, renderer));
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut inner,
            viewport,
        );
        if let Some(epoch) = self.epoch
            && let Some(text) = self.text
            && let iced::advanced::input_method::InputMethod::Enabled { cursor: caret, .. } = inner.input_method()
            && tree.state.downcast_mut::<RevealState>().changed(epoch, layout.bounds(), *caret, *viewport)
        {
            let caret = if text.cursor().selection.is_some() { selection_caret(text, layout.bounds()) } else { *caret };
            if let Some(delta) = RevealState::delta(caret, *viewport) {
                shell.publish(super::Message::RevealCaret(epoch, delta));
            }
        }
        if let Some(before) = before
            && scroll_offset(&mut self.content, &mut tree.children[0], layout, renderer) != before
        {
            shell.publish(super::Message::PaperScrolled);
        }
        shell.merge(inner, |message| message);
    }
    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }
    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &iced::Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, super::Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

fn scroll_offset(
    content: &mut iced::Element<'_, super::Message>,
    tree: &mut iced::advanced::widget::Tree,
    layout: iced::advanced::Layout<'_>,
    renderer: &iced::Renderer,
) -> f32 {
    use iced::advanced::widget::{Operation, operation::Scrollable};
    #[derive(Default)]
    struct Offset(f32);
    impl Operation for Offset {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn scrollable(
            &mut self,
            id: Option<&iced::advanced::widget::Id>,
            _: iced::Rectangle,
            _: iced::Rectangle,
            translation: iced::Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&iced::advanced::widget::Id::from("compose-paper")) {
                self.0 = translation.y;
            }
        }
    }
    let mut offset = Offset::default();
    content.as_widget_mut().operate(tree, layout, renderer, &mut offset);
    offset.0
}

/// Iced's IME rectangle describes the first selected range, not its moving endpoint. Shape a
/// selection-free mirror only on explicit reveal requests; never disturb the real selection.
fn selection_caret(content: &Content, bounds: iced::Rectangle) -> iced::Rectangle {
    use iced::advanced::text::{Editor as _, Highlighter as _};
    let body = torro_gui::typo::body();
    let padding = torro_gui::theme::space::m();
    let mut editor = iced::advanced::graphics::text::Editor::with_text(&content.text());
    let mut highlighter = super::MarkupHighlighter::new(&());
    editor.move_to(iced::widget::text_editor::Cursor { selection: None, ..content.cursor() });
    editor.update(
        iced::Size::new((bounds.width - padding * 2.0).max(0.0), f32::INFINITY),
        body.font,
        body.size,
        body.line_height,
        iced::advanced::text::Wrapping::WordOrGlyph,
        &mut highlighter,
    );
    editor.highlight(body.font, &mut highlighter, |look| super::look_format(look, &iced::Theme::Light));
    let iced::advanced::text::editor::Selection::Caret(position) = editor.selection() else {
        return bounds;
    };
    // The public Iced cursor omits wrap-boundary affinity. Conservatively reveal both possible
    // visual rows at a shared glyph boundary, so either moving endpoint remains visible.
    let column = content.cursor().position.column;
    let ambiguous =
        editor.buffer().lines.get(content.cursor().position.line).and_then(|line| line.layout_opt()).is_some_and(
            |lines| {
                lines.windows(2).any(|pair| {
                    pair[0].glyphs.last().is_some_and(|glyph| glyph.end == column)
                        && pair[1].glyphs.first().is_some_and(|glyph| glyph.start == column)
                })
            },
        );
    iced::Rectangle::new(
        bounds.position() + iced::Vector::new(padding + position.x, padding + position.y),
        iced::Size::new(1.0, body.line_height.to_absolute(body.size).0 * if ambiguous { 2.0 } else { 1.0 }),
    )
}

pub(super) enum Block {
    Text { id: u64, content: Content, history: Option<String> },
    Image(Box<Picture>),
}

pub(super) struct Picture {
    pub block: u64,
    pub cid: String,
    pub source: Option<ImageSource>,
    /// A saved fallback is updated by CID even if other images were removed during the save.
    pub saved: Option<ImageSource>,
    pub prepared: Option<PreparedImage>,
    pub preview: Option<iced::widget::image::Handle>,
    pub size: ImageSize,
    pub inline: bool,
    pub loading: bool,
    pub failed: bool,
    pub quoted: bool,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    replaced: String,
}

impl Picture {
    pub fn draft(&self) -> Option<DraftAttachment> {
        Some(DraftAttachment::Image {
            id: self.cid.clone(),
            source: self.source.clone()?,
            inline: self.inline,
            size: self.size,
        })
    }

    pub fn loaded(&mut self, prepared: PreparedImage) {
        use base64::Engine as _;
        self.preview = base64::engine::general_purpose::STANDARD
            .decode(&prepared.preview)
            .ok()
            .map(iced::widget::image::Handle::from_bytes);
        self.width = prepared.width;
        self.height = prepared.height;
        self.source = Some(ImageSource::Prepared { id: prepared.id.clone() });
        self.prepared = Some(prepared);
        self.loading = false;
        self.failed = false;
    }
}

pub(super) struct Document {
    pub blocks: Vec<Block>,
    pub active: u64,
    pub selected: Option<u64>,
    next: u64,
    fallback: Content,
}

#[derive(Debug, Clone)]
pub(super) struct Inserted {
    pub block: u64,
    pub range: std::ops::Range<usize>,
    pub focused: bool,
}

impl From<Content> for Document {
    fn from(content: Content) -> Self {
        Self {
            blocks: vec![Block::Text { id: 0, content, history: None }],
            active: 0,
            selected: None,
            next: 1,
            fallback: Content::new(),
        }
    }
}

impl Deref for Document {
    type Target = Content;
    fn deref(&self) -> &Content {
        self.blocks
            .iter()
            .find_map(|block| match block {
                Block::Text { id, content, .. } if *id == self.active => Some(content),
                _ => None,
            })
            .unwrap_or(&self.fallback)
    }
}

impl DerefMut for Document {
    fn deref_mut(&mut self) -> &mut Content {
        // Every operation that changes block structure keeps the active id on a text block.
        self.blocks
            .iter_mut()
            .find_map(|b| match b {
                Block::Text { id, content, .. } if *id == self.active => Some(content),
                _ => None,
            })
            .unwrap_or(&mut self.fallback)
    }
}

impl Document {
    /// Query widget focus, not the last editor action: Tab can change focus without changing text.
    pub fn focused_text(&self) -> iced::Task<Option<u64>> {
        iced::advanced::widget::operate(self.focused_operation())
    }

    fn focused_operation(&self) -> FocusedText {
        FocusedText {
            ids: self
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { id, .. } => Some((iced::advanced::widget::Id::from(Self::text_id(*id)), *id)),
                    _ => None,
                })
                .collect(),
            focused: None,
        }
    }

    pub fn text_id(id: u64) -> String {
        if id == 0 { super::BODY.into() } else { format!("compose-block-{id}") }
    }

    pub fn activate(&mut self, id: u64) -> bool {
        if self.focus_text(id) {
            self.selected = None;
            true
        } else {
            false
        }
    }

    /// Widget focus can move by Tab without producing a text action. Editor-local commands
    /// carry their block id so paste uses that editor's caret, while image selection survives.
    pub fn focus_text(&mut self, id: u64) -> bool {
        if self.blocks.iter().any(|b| matches!(b, Block::Text { id: known, .. } if *known == id)) {
            self.active = id;
            true
        } else {
            false
        }
    }

    pub fn pictures(&self) -> impl Iterator<Item = &Picture> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Image(p) => Some(p.as_ref()),
            _ => None,
        })
    }

    pub fn picture(&mut self, id: u64) -> Option<&mut Picture> {
        self.blocks.iter_mut().find_map(|b| match b {
            Block::Image(p) if p.block == id => Some(p.as_mut()),
            _ => None,
        })
    }

    pub fn pending(&self) -> bool {
        self.pictures().any(|p| p.loading)
    }
    pub fn failed(&self) -> bool {
        self.pictures().any(|p| !p.loading && (p.failed || p.source.is_none()))
    }

    /// Replace the current selection by a loading image, moving the suffix into a new text block.
    /// Positions are byte columns, not Unicode scalar counts (the Iced Content contract).
    pub fn reserve(&mut self) -> u64 {
        let text = self.text().replace("\r\n", "\n");
        let cursor = self.cursor();
        let offset =
            |p: Position| -> usize { text.split('\n').take(p.line).map(|s| s.len() + 1).sum::<usize>() + p.column };
        let a = offset(cursor.position).min(text.len());
        let b = cursor.selection.map(offset).unwrap_or(a).min(text.len());
        let (start, end) = (a.min(b), a.max(b));
        // Be defensive if a synthetic key reports a column in the middle of a UTF-8 code point.
        let start = (0..=start).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
        let end = (end..=text.len()).find(|i| text.is_char_boundary(*i)).unwrap_or(text.len());
        let suffix = Content::with_text(&text[end..]);
        **self = Content::with_text(&text[..start]);
        let at =
            self.blocks.iter().position(|b| matches!(b, Block::Text { id, .. } if *id == self.active)).unwrap_or(0);
        let id = self.next;
        self.next += 2;
        self.blocks.insert(
            at + 1,
            Block::Image(Box::new(Picture {
                block: id,
                cid: format!("image-{id}-{}@torro", uuid_token()),
                source: None,
                saved: None,
                prepared: None,
                preview: None,
                size: ImageSize::Medium,
                inline: true,
                loading: true,
                failed: false,
                quoted: false,
                width: 0,
                height: 0,
                alt: super::tr("Image").into(),
                replaced: text[start..end].into(),
            })),
        );
        self.blocks.insert(at + 2, Block::Text { id: id + 1, content: suffix, history: None });
        self.active = id + 1;
        self.selected = Some(id);
        id
    }

    pub fn remove(&mut self, id: u64) -> Option<Picture> {
        let at = self.blocks.iter().position(|b| matches!(b, Block::Image(p) if p.block == id))?;
        let Block::Image(picture) = self.blocks.remove(at) else { return None };
        self.selected = None;
        Some(*picture)
    }

    pub fn replace_reservation(&mut self, id: u64, text: &str) -> bool {
        self.replace_reserved_inner(id, text, false).is_some_and(|inserted| inserted.focused)
    }

    /// Success is independent of focus: a reply may target a block other than the active one.
    pub fn replace_reserved_range(&mut self, id: u64, text: &str) -> Option<Inserted> {
        self.replace_reserved_inner(id, text, true)
    }

    fn replace_reserved_inner(&mut self, id: u64, text: &str, enforce_limits: bool) -> Option<Inserted> {
        let at = self.blocks.iter().position(|b| matches!(b, Block::Image(p) if p.block == id))?;
        let before = at.checked_sub(1).and_then(|i| self.blocks.get(i)).and_then(|b| match b {
            Block::Text { id, content, history } => Some((*id, content.text(), history.clone())),
            _ => None,
        });
        let after = self.blocks.get(at + 1).and_then(|b| match b {
            Block::Text { content, .. } => Some(content.text()),
            _ => None,
        });
        if let (Some((text_id, prefix, history)), Some(suffix)) = (before, after) {
            // Keep typing that arrived in the suffix while the clipboard worker was running,
            // including its cursor. Otherwise put the caret immediately after the pasted text.
            let suffix_active = matches!(self.blocks.get(at + 1), Some(Block::Text { id, .. }) if *id == self.active);
            let merged_active = suffix_active || self.active == text_id;
            let merged = format!("{prefix}{text}{suffix}");
            let cursor = self.cursor();
            let cursor = if suffix_active {
                let shift = |p| position(&merged, prefix.len() + text.len() + offset(&suffix, p));
                iced::widget::text_editor::Cursor {
                    position: shift(cursor.position),
                    selection: cursor.selection.map(shift),
                }
            } else if self.active == text_id {
                cursor
            } else {
                text_editor_cursor(position(&merged, prefix.len() + text.len()))
            };
            let mut content = Content::with_text(&merged);
            content.move_to(cursor);
            let previous: Vec<_> =
                self.blocks.splice(at - 1..=at + 1, [Block::Text { id: text_id, content, history }]).collect();
            if enforce_limits && !self.within_limits() {
                self.blocks.splice(at - 1..at, previous);
                return None;
            }
            if merged_active {
                self.active = text_id;
            }
            self.selected = self.selected.filter(|selected| *selected != id);
            return Some(Inserted {
                block: text_id,
                range: prefix.len()..prefix.len() + text.len(),
                focused: merged_active,
            });
        }
        None
    }

    pub fn text_bytes(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| match block {
                Block::Text { content, .. } => content.text().len(),
                _ => 0,
            })
            .sum()
    }

    pub fn within_limits(&self) -> bool {
        self.text_bytes() <= torro_ipc::mail::MAX_BODY_BYTES && self.html(None).len() <= torro_ipc::mail::MAX_BODY_BYTES
    }

    /// Compare the recorded bytes before replacing just the last insertion. Preserve a later
    /// caret/selection, including focus in a different editor; never restore a document snapshot.
    pub fn replace_inserted(&mut self, inserted: &Inserted, expected: &str, replacement: &str) -> bool {
        let Some(Block::Text { content, .. }) =
            self.blocks.iter_mut().find(|block| matches!(block, Block::Text { id, .. } if *id == inserted.block))
        else {
            return false;
        };
        let old = content.text();
        if old.get(inserted.range.clone()) != Some(expected) {
            return false;
        }
        let mut next = old.clone();
        next.replace_range(inserted.range.clone(), replacement);
        let cursor = content.cursor();
        let shift = |p| {
            let at = offset(&old, p);
            let at = if at < inserted.range.start {
                at
            } else if at <= inserted.range.end {
                inserted.range.start + replacement.len()
            } else {
                at - inserted.range.len() + replacement.len()
            };
            position(&next, at)
        };
        let mut updated = Content::with_text(&next);
        updated.move_to(iced::widget::text_editor::Cursor {
            position: shift(cursor.position),
            selection: cursor.selection.map(shift),
        });
        let previous = std::mem::replace(content, updated);
        if self.text_bytes() > torro_ipc::mail::MAX_BODY_BYTES
            || self.html(None).len() > torro_ipc::mail::MAX_BODY_BYTES
        {
            if let Some(Block::Text { content, .. }) =
                self.blocks.iter_mut().find(|block| matches!(block, Block::Text { id, .. } if *id == inserted.block))
            {
                *content = previous;
            }
            return false;
        }
        true
    }

    pub fn restore_reservation(&mut self, id: u64) -> bool {
        let text = self.picture(id).map(|p| p.replaced.clone()).unwrap_or_default();
        // Restoration is not a new import: selected text and later typing must both survive.
        // If together they exceed the save budget, the window asks to shorten the result.
        self.replace_reservation(id, &text)
    }

    pub fn html(&self, history: Option<&str>) -> String {
        let mut out = String::new();
        let mut quoted = false;
        for block in &self.blocks {
            match block {
                Block::Text { content, history: local, .. } => {
                    let text = content.text();
                    // Once the history boundary was crossed, the suffix remains someone else's
                    // literal text even when an image split away its original attribution line.
                    let boundary = if quoted {
                        text.lines().find(|line| !line.trim().is_empty())
                    } else {
                        local.as_deref().or(history)
                    };
                    let html = markup::to_html(&text, boundary);
                    out.push_str(&html);
                    quoted |= html.contains(&format!("class=\"{}\"", html::QUOTE_CLASS));
                }
                Block::Image(p) if p.inline && !p.loading && !p.failed => {
                    let (natural, height) = if p.size == ImageSize::Original {
                        p.prepared.as_ref().map_or((p.width, p.height), |i| (i.original_width, i.original_height))
                    } else {
                        (p.width, p.height)
                    };
                    let width = p.size.width(natural);
                    let height = (u64::from(height) * u64::from(width) / u64::from(natural.max(1))).max(1);
                    let tag = format!(
                        "<img src=\"cid:{}\" alt=\"{}\" width=\"{width}\" height=\"{height}\">",
                        html::escape(&p.cid),
                        html::escape(&p.alt)
                    );
                    if quoted || p.quoted {
                        out.push_str(&format!("<div class=\"{}\">{tag}</div>", html::QUOTE_CLASS));
                    } else {
                        out.push_str(&tag);
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn attachments(&self) -> Vec<DraftAttachment> {
        self.pictures().filter_map(Picture::draft).collect()
    }

    pub fn from_draft(draft: &Draft) -> Self {
        let mut document = Self::from(Content::new());
        if draft.html.is_none() {
            *document = Content::with_text(&html::plain_to_markup(&draft.body));
        }
        for block in html::to_blocks(draft.html.as_deref().unwrap_or_default()) {
            match block {
                html::HtmlBlock::Text(markup) => {
                    let (text, history) = html::to_markup(&markup);
                    if document.blocks.len() == 1 {
                        if let Some(first) = document.blocks.first_mut() {
                            *first = Block::Text { id: 0, content: Content::with_text(&text), history };
                        }
                    } else {
                        let id = document.next;
                        document.next += 1;
                        document.blocks.push(Block::Text { id, content: Content::with_text(&text), history });
                    }
                }
                html::HtmlBlock::Image { cid, alt, width, height, quoted } => {
                    if let Some(DraftAttachment::Image { source, size, inline, .. }) =
                        draft.attachments.iter().find(|a| matches!(a, DraftAttachment::Image { id, .. } if *id == cid))
                    {
                        let block = document.next;
                        document.next += 1;
                        document.blocks.push(Block::Image(Box::new(Picture {
                            block,
                            cid,
                            source: Some(source.clone()),
                            saved: Some(source.clone()),
                            prepared: None,
                            preview: None,
                            size: *size,
                            inline: *inline,
                            loading: true,
                            failed: false,
                            quoted,
                            width: width.unwrap_or(640),
                            height: height.unwrap_or(480),
                            alt,
                            replaced: String::new(),
                        })));
                    }
                }
            }
        }
        // Attached images have no HTML reference but retain their CID and image actions.
        for a in &draft.attachments {
            if let DraftAttachment::Image { id, source, inline: false, size } = a {
                let block = document.next;
                document.next += 1;
                document.blocks.push(Block::Image(Box::new(Picture {
                    block,
                    cid: id.clone(),
                    source: Some(source.clone()),
                    saved: Some(source.clone()),
                    prepared: None,
                    preview: None,
                    size: *size,
                    inline: false,
                    loading: true,
                    failed: false,
                    quoted: false,
                    width: 640,
                    height: 480,
                    alt: super::tr("Image").into(),
                    replaced: String::new(),
                })));
            }
        }
        if !matches!(document.blocks.last(), Some(Block::Text { .. })) {
            let id = document.next;
            document.next += 1;
            document.blocks.push(Block::Text { id, content: Content::new(), history: None });
        }
        document
    }
}

struct FocusedText {
    ids: Vec<(iced::advanced::widget::Id, u64)>,
    focused: Option<u64>,
}

impl iced::advanced::widget::Operation<Option<u64>> for FocusedText {
    fn focusable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if state.is_focused() {
            self.focused = self.ids.iter().find_map(|(known, block)| (Some(known) == id).then_some(*block));
        }
    }

    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Option<u64>>)) {
        operate(self);
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<Option<u64>> {
        // A missing focused editor still delivers a reply, retaining the last body caret.
        iced::advanced::widget::operation::Outcome::Some(self.focused)
    }
}

fn text_editor_cursor(position: Position) -> iced::widget::text_editor::Cursor {
    iced::widget::text_editor::Cursor { position, selection: None }
}

fn offset(text: &str, p: Position) -> usize {
    let at = (text.split('\n').take(p.line).map(|s| s.len() + 1).sum::<usize>() + p.column).min(text.len());
    (0..=at).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0)
}

fn position(text: &str, at: usize) -> Position {
    let at = at.min(text.len());
    Position {
        line: text[..at].bytes().filter(|b| *b == b'\n').count(),
        column: text[..at].rsplit('\n').next().map_or(0, str::len),
    }
}

fn uuid_token() -> String {
    // CIDs need uniqueness, not secrecy; the service validates them before building MIME.
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor::Cursor;

    #[test]
    fn inserted_range_is_utf8_exact_even_without_focus_and_replacement_checks_original_bytes() {
        let mut doc = Document::from(Content::with_text("Grüße 世界!"));
        doc.move_to(Cursor {
            position: Position { line: 0, column: 8 },
            selection: Some(Position { line: 0, column: 14 }),
        });
        let id = doc.reserve();
        let later = doc.reserve();
        let active = doc.active;
        let inserted = doc.replace_reserved_range(id, "**Bold**").expect("success without focus");
        assert!(!inserted.focused);
        assert_eq!(inserted.range, 8..16);
        assert_eq!(doc.active, active);
        assert!(doc.replace_inserted(&inserted, "**Bold**", "Plain"));
        assert!(!doc.replace_inserted(&inserted, "**Bold**", "Not again"));
        doc.restore_reservation(later);
        assert_eq!(doc.text(), "Grüße Plain!");
    }

    #[test]
    fn reservation_reply_preserves_suffix_selection_and_rejects_expansion_transactionally() {
        let mut doc = Document::from(Content::with_text("Before After"));
        doc.move_to(text_editor_cursor(Position { line: 0, column: 7 }));
        let id = doc.reserve();
        doc.move_to(Cursor {
            position: Position { line: 0, column: 2 },
            selection: Some(Position { line: 0, column: 5 }),
        });
        let inserted = doc.replace_reserved_range(id, "世界").expect("reply");
        assert_eq!(doc.cursor().position.column, 15);
        assert_eq!(doc.cursor().selection.expect("selection").column, 18);
        assert!(doc.replace_inserted(&inserted, "世界", "X"));
        assert_eq!(doc.cursor().position.column, 10);
        assert_eq!(doc.cursor().selection.expect("selection").column, 13);
        let id = doc.reserve();
        assert!(doc.replace_reserved_range(id, &"&".repeat(300_000)).is_none(), "HTML expansion also limited");
        assert!(doc.picture(id).is_some());
        doc.restore_reservation(id);
        assert_eq!(doc.text(), "Before XAfter");
    }

    #[test]
    fn growing_editors_reveal_paste_and_selection_below_images_without_stealing_scroll() {
        use iced::advanced::renderer::Headless as _;
        use iced::advanced::widget::{Operation, operation};
        use iced_test::runtime::{UserInterface, user_interface};

        #[derive(Default)]
        struct ScrollPosition(f32);
        impl Operation for ScrollPosition {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn scrollable(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                _: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn operation::Scrollable,
            ) {
                self.0 = translation.y;
            }
        }

        let mut renderer = iced_test::futures::futures::executor::block_on(iced::Renderer::new(
            torro_gui::typo::body().font,
            torro_gui::typo::body().size,
            None,
        ))
        .expect("headless renderer");
        let mut cache = user_interface::Cache::default();
        let mut content = Content::new();
        let prefix = Content::with_text("Before image");
        let mut epoch = 0;
        // Cache survives every rebuild, just as in the application. A nonzero container origin
        // catches confusing content coordinates with window coordinates.
        let mut frame = |content: &Content, epoch: Option<u64>, focus: bool, scroll: Option<f32>, image_height: f32| {
            let body = torro_gui::typo::body();
            let editor = iced::widget::text_editor(content)
                .id("caret-test")
                .font(body.font)
                .size(body.size)
                .line_height(body.line_height)
                .padding(torro_gui::theme::space::m())
                .height(iced::Shrink)
                .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
                .on_action(super::super::Message::Body)
                .highlight_with::<super::super::MarkupHighlighter>((), super::super::look_format);
            let root = iced::widget::container(paper_scroll(
                iced::widget::scrollable(iced::widget::column![
                    iced::widget::text_editor(&prefix)
                        .id("prefix")
                        .height(iced::Shrink)
                        .on_action(super::super::Message::Body),
                    iced::widget::space().height(image_height),
                    reveal_caret(editor.into(), content, epoch),
                ])
                .id("compose-paper")
                .height(iced::Fill)
                .into(),
            ))
            .padding(23);
            let mut ui =
                UserInterface::build(root, iced::Size::new(340.0, 220.0), std::mem::take(&mut cache), &mut renderer);
            if focus {
                ui.operate(&renderer, &mut operation::focusable::focus("prefix".into()));
                let mut next: Box<dyn Operation> = Box::new(operation::focusable::focus_next());
                loop {
                    ui.operate(&renderer, next.as_mut());
                    if let operation::Outcome::Chain(chained) = next.finish() {
                        next = chained;
                    } else {
                        break;
                    }
                }
            }
            if let Some(y) = scroll.filter(|y| *y >= 0.0) {
                ui.operate(
                    &renderer,
                    &mut operation::scrollable::scroll_to(
                        "compose-paper".into(),
                        operation::scrollable::AbsoluteOffset { x: None, y: Some(y) },
                    ),
                );
            }
            let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(iced::time::Instant::now()));
            let wheel = scroll.is_some_and(|y| y < 0.0);
            let mut events = Vec::new();
            if wheel {
                events.push(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                    delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: 100.0 },
                }));
            }
            events.push(redraw);
            let mut messages = Vec::new();
            let (state, _) = ui.update(
                &events,
                if wheel {
                    iced::mouse::Cursor::Available(iced::Point::new(160.0, 110.0))
                } else {
                    iced::mouse::Cursor::Unavailable
                },
                &mut renderer,
                &mut iced::advanced::clipboard::Null,
                &mut messages,
            );
            let caret = match state {
                user_interface::State::Updated {
                    input_method: iced::advanced::InputMethod::Enabled { cursor, .. },
                    ..
                } => Some(cursor),
                _ => None,
            };
            assert_eq!(
                messages.iter().any(|m| matches!(m, super::super::Message::PaperScrolled)),
                wheel,
                "only an actual user offset change cancels pending follow, not layout/programmatic scrolling"
            );
            let deltas = messages
                .into_iter()
                .filter_map(|message| match message {
                    super::super::Message::RevealCaret(_, delta) => Some(delta),
                    _ => None,
                })
                .collect::<Vec<_>>();
            for delta in &deltas {
                ui.operate(
                    &renderer,
                    &mut operation::scrollable::scroll_by(
                        "compose-paper".into(),
                        operation::scrollable::AbsoluteOffset { x: 0.0, y: *delta },
                    ),
                );
            }
            let mut offset = ScrollPosition::default();
            ui.operate(&renderer, &mut offset);
            cache = ui.into_cache();
            (deltas, offset.0, caret)
        };

        let (delta, _, _) = frame(&content, Some(epoch), true, None, 0.0);
        assert!(delta.is_empty(), "initial caret is visible");
        for text in ["line\n".repeat(90), "wrapped words 世界 ".repeat(500)] {
            content.perform(iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Paste(
                std::sync::Arc::new(text),
            )));
            epoch += 1;
            let (deltas, offset, _) = frame(&content, Some(epoch), false, None, 0.0);
            assert!(deltas.iter().any(|delta| *delta > 500.0), "long paste scrolls the paper: {deltas:?}");
            assert!(offset > 500.0);
            let (deltas, _, caret) = frame(&content, Some(epoch), false, None, 0.0);
            assert!(deltas.is_empty(), "follow converges after one scroll");
            let caret = caret.expect("focused editor");
            assert!(caret.y >= 23.0 && caret.y + caret.height <= 197.1, "visible caret: {caret:?}");
        }

        content = Content::with_text("after image");
        epoch += 1;
        let (deltas, _, _) = frame(&content, Some(epoch), true, Some(0.0), 1300.0);
        assert!(deltas.iter().any(|delta| *delta > 1000.0), "focus below a tall image follows exact layout");
        frame(&content, Some(epoch), false, Some(-1.0), 1300.0);
        let (_, before, _) = frame(&content, Some(epoch), false, Some(0.0), 1300.0);
        let (deltas, after, _) = frame(&content, Some(epoch), false, None, 1300.0);
        assert!(deltas.is_empty() && before == after, "manual scrolling survives repeated redraws");
        let (deltas, _, _) = frame(&content, None, false, None, 1500.0);
        assert!(deltas.is_empty(), "a late image changing layout cannot re-arm cancelled follow");

        content = Content::with_text(&"selected line\n".repeat(100));
        content.move_to(Cursor {
            position: Position { line: 90, column: 4 },
            selection: Some(Position { line: 0, column: 0 }),
        });
        epoch += 1;
        let (deltas, _, _) = frame(&content, Some(epoch), true, Some(0.0), 0.0);
        assert!(
            deltas.iter().any(|delta| *delta > 1500.0),
            "follow the moving endpoint, not selection anchor: {deltas:?}"
        );
        content.move_to(Cursor {
            position: Position { line: 0, column: 0 },
            selection: Some(Position { line: 90, column: 4 }),
        });
        epoch += 1;
        let (deltas, _, _) = frame(&content, Some(epoch), false, None, 0.0);
        assert!(deltas.iter().any(|delta| *delta < -1500.0), "reverse selection follows upward: {deltas:?}");

        content = Content::with_text(&"x".repeat(1000));
        epoch += 1;
        frame(&content, Some(epoch), true, Some(0.0), 0.0);
        let line = torro_gui::typo::line_height(torro_gui::typo::body());
        let mut ambiguous = false;
        for _ in 0..12 {
            // Keyboard selection moves on visual rows and can retain After affinity at a wrap.
            content.perform(iced::widget::text_editor::Action::Select(iced::widget::text_editor::Motion::Down));
            epoch += 1;
            frame(&content, Some(epoch), false, None, 0.0);
            let rect = selection_caret(&content, iced::Rectangle::with_size(iced::Size::new(294.0, f32::INFINITY)));
            ambiguous |= rect.height > line;
        }
        assert!(ambiguous, "wrap-boundary affinity is covered by both possible caret rows");
    }

    #[test]
    fn a_plain_draft_reopens_its_non_inline_images_even_without_body_text() {
        let attachment = DraftAttachment::Image {
            id: "attached@torro".into(),
            source: ImageSource::MailPart { message: "m:draft".into(), part: "1".into() },
            inline: false,
            size: ImageSize::Original,
        };
        let draft = Draft {
            id: Some("m:draft".into()),
            account: "mail:a".into(),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "Attached image".into(),
            body: String::new(),
            html: None,
            in_reply_to: None,
            attachments: vec![attachment.clone()],
        };
        for body in ["", "Plain **literal** text"] {
            let draft = Draft { body: body.into(), ..draft.clone() };
            let doc = Document::from_draft(&draft);
            assert_eq!(doc.pictures().count(), 1);
            assert_eq!(doc.attachments(), vec![attachment.clone()]);
            assert_eq!(html::to_text(&doc.html(None)), body);
        }
    }

    #[test]
    fn split_history_remains_literal_after_its_attribution_was_split_away() {
        let history = "On Thursday, Bob wrote:";
        let mut doc = Document::from(Content::with_text(&format!(
            "**Own text**\n\n{history}\n> Literal **text** and __marks__\n> - not a list"
        )));
        doc.move_to(text_editor_cursor(Position { line: 3, column: 0 }));
        let id = doc.reserve();
        doc.picture(id).expect("image").loaded(super::super::demo_image());
        let markup = doc.html(Some(history));
        assert!(markup.contains("<strong>Own text</strong>"));
        assert!(markup.contains("Literal **text** and __marks__"));
        assert!(!markup.contains("<strong>text</strong>") && !markup.contains("<u>marks</u>"));
        assert!(markup.contains("- not a list") && !markup.contains("<li>"));
        assert_eq!(html::sanitize(&markup), markup);
    }

    #[test]
    fn focused_editor_query_follows_tab_without_needing_a_text_action() {
        use iced::advanced::widget::{
            Operation as _,
            operation::{Focusable, Outcome},
        };
        struct Focus(bool);
        impl Focusable for Focus {
            fn is_focused(&self) -> bool {
                self.0
            }
            fn focus(&mut self) {
                self.0 = true;
            }
            fn unfocus(&mut self) {
                self.0 = false;
            }
        }
        let mut doc = Document::from(Content::with_text("Before After"));
        doc.move_to(text_editor_cursor(Position { line: 0, column: 7 }));
        doc.reserve();
        let suffix = doc.active;
        doc.activate(0);
        let mut query = doc.focused_operation();
        query.focusable(
            Some(&iced::advanced::widget::Id::from(Document::text_id(0))),
            iced::Rectangle::default(),
            &mut Focus(false),
        );
        query.focusable(
            Some(&iced::advanced::widget::Id::from(Document::text_id(suffix))),
            iced::Rectangle::default(),
            &mut Focus(true),
        );
        assert!(matches!(query.finish(), Outcome::Some(Some(id)) if id == suffix));
        let empty = doc.focused_operation();
        assert!(matches!(empty.finish(), Outcome::Some(None)), "no focused editor still returns a reply");
    }

    #[test]
    fn unicode_selection_and_late_import_never_overwrite_new_text() {
        let mut doc = Document::from(Content::with_text("Grüße 世界!"));
        doc.move_to(Cursor {
            position: Position { line: 0, column: 7 },
            selection: Some(Position { line: 0, column: 13 }),
        });
        let first = doc.reserve();
        super::super::paste(&mut doc, "new ");
        let second = doc.reserve();
        doc.picture(first).expect("first").failed = true;
        assert!(doc.text().contains('!'));
        assert_ne!(first, second);
        assert!(doc.html(None).contains("new"));
        assert!(doc.remove(first).is_some());
        assert!(doc.picture(first).is_none(), "late replies cannot recreate a removed block");
    }

    #[test]
    fn earlier_clipboard_reply_does_not_steal_the_later_reservations_caret() {
        let mut doc = Document::from(Content::new());
        let first = doc.reserve();
        let second = doc.reserve();
        let active = doc.active;
        assert!(!doc.replace_reservation(first, "P"), "focus stays outside the replaced blocks");
        assert_eq!(doc.active, active);
        assert_eq!(doc.selected, Some(second));
        super::super::paste(&mut doc, "x");
        assert!(doc.replace_reservation(second, "P"));
        assert_eq!(doc.text(), "PPx");
        assert_eq!(doc.cursor().position, Position { line: 0, column: 3 });
    }

    #[test]
    fn clipboard_fallback_keeps_later_suffix_typing_and_cursor() {
        let mut doc = Document::from(Content::with_text("Before After"));
        doc.move_to(text_editor_cursor(Position { line: 0, column: 7 }));
        let image = doc.reserve();
        super::super::paste(&mut doc, "latest ");
        doc.replace_reservation(image, "世界\n");
        assert_eq!(doc.text(), "Before 世界\nlatest After");
        assert_eq!(doc.cursor().position, Position { line: 1, column: 7 });
    }
}
