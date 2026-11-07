//! Stable compose blocks (issue #461). Imports reserve a block before any asynchronous work;
//! their replies can replace only that block, never a newer text snapshot.

use iced::widget::text_editor::{Content, Position};
use std::ops::{Deref, DerefMut};
use torro_ipc::mail::PreparedImage;
use torro_mail::{Draft, DraftAttachment, ImageSize, ImageSource, html, markup};

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
        self.pictures().any(|p| p.failed || p.source.is_none())
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
        let Some(at) = self.blocks.iter().position(|b| matches!(b, Block::Image(p) if p.block == id)) else {
            return false;
        };
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
            let cursor = self.cursor();
            let suffix_active = matches!(self.blocks.get(at + 1), Some(Block::Text { id, .. }) if *id == self.active);
            let merged_active = suffix_active || self.active == text_id;
            let added = if suffix_active {
                suffix.split('\n').take(cursor.position.line).map(|s| s.len() + 1).sum::<usize>()
                    + cursor.position.column
            } else {
                0
            };
            let merged = format!("{prefix}{text}{suffix}");
            let end = (prefix.len() + text.len() + added).min(merged.len());
            let line = merged[..end].bytes().filter(|b| *b == b'\n').count();
            let column = merged[..end].rsplit('\n').next().map_or(0, str::len);
            let mut content = Content::with_text(&merged);
            content.move_to(text_editor_cursor(Position { line, column }));
            self.blocks.splice(at - 1..=at + 1, [Block::Text { id: text_id, content, history }]);
            if merged_active {
                self.active = text_id;
            }
            self.selected = self.selected.filter(|selected| *selected != id);
            return merged_active;
        }
        false
    }

    pub fn restore_reservation(&mut self, id: u64) -> bool {
        let text = self.picture(id).map(|p| p.replaced.clone()).unwrap_or_default();
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
