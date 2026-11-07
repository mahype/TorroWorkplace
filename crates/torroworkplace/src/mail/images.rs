//! Volatile, caller/account-owned compose images (issue #461). No originals or previews reach
//! disk, logs or Debug. One decoder runs at a time; reservations precede reads and decoding.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{ImageDecoder, ImageReader};
use tokio::sync::Mutex;
use torro_core::i18n::tr;
use torro_ipc::mail::{IMAGE_CHUNK_BYTES, MAX_FILE_BYTES, PreparedImage};
use torro_ipc::{Request, Response};
use torro_mail::{Draft, DraftAttachment, ImageSize, ImageSource};
use torro_mail_imap::mime::OutgoingAttachment;

use super::service::MailService;
use super::store::DraftAuthor;
use super::write::{self, WriteError};
use crate::pim::Reach;
use crate::tools::Register;

const MEMORY: usize = 100 * 1024 * 1024;
const PREVIEW: usize = 256 * 1024;
// Streaming PNG compression, three scanlines, endian conversion and an 8-KiB IDAT chunk.
// Encoded output is charged separately; this never scales with image height.
const ENCODER_SCRATCH: usize = 1024 * 1024;
const ORIGINAL: usize = MAX_FILE_BYTES as usize;
const NO_IMAGE: &str = "The image source is no longer available. Insert it again.";
const BAD_IMAGE: &str = "The image is invalid or exceeds the image limits.";

/// No Debug: entries include the original image bytes.
#[derive(Default)]
pub(crate) struct ImageStore {
    // Keeping this lock across blocking-worker awaits also serializes decoders. Upload calls
    // cannot allocate another buffer while a file import holds its worst-case reservation.
    state: Arc<Mutex<State>>,
    cleanup: std::sync::atomic::AtomicBool,
}

impl ImageStore {
    pub(crate) async fn mime_budget(&self) -> Result<(usize, torro_ipc::mail::ImageReservation), WriteError> {
        let state = self.state.lock().await;
        let limit = state.encoded_budget().map_err(|_| WriteError::Busy)?;
        let bytes = limit.saturating_mul(2).saturating_add(ORIGINAL + 1024 * 1024);
        state.room(bytes).map_err(|_| WriteError::Busy)?;
        let reservation = torro_ipc::mail::ImageReservation::new(bytes).ok_or(WriteError::Busy)?;
        Ok((limit, reservation))
    }

    /// Conservatively retain output and MIME working space until the complete save is done.
    pub(crate) async fn reserve_output(&self, bytes: usize) -> Result<torro_ipc::mail::ImageReservation, WriteError> {
        let state = self.state.lock().await;
        state.room(bytes).map_err(|_| WriteError::Busy)?;
        torro_ipc::mail::ImageReservation::new(bytes).ok_or(WriteError::Busy)
    }

    fn start_cleanup(&self) {
        if self.cleanup.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(&self.state);
        tokio::spawn(async move {
            loop {
                let Some(state) = weak.upgrade() else { break };
                let delay = {
                    let mut state = state.lock().await;
                    state.purge();
                    state
                        .entries
                        .values()
                        .map(|e| {
                            let ttl = Duration::from_secs(if e.ready.is_some() { 3600 } else { 600 });
                            ttl.saturating_sub(e.touched.elapsed())
                        })
                        .min()
                        .unwrap_or(Duration::from_secs(60))
                        .min(Duration::from_secs(60))
                };
                drop(state);
                tokio::time::sleep(delay.max(Duration::from_millis(1))).await;
            }
        });
    }

    async fn reserve_request(&self, request: &Request) -> Result<(), Response> {
        if let Request::UploadMailImage { base64, .. } = request {
            if base64.is_reserved() {
                return Ok(());
            }
            // A fetch/decoder may retain this guard for seconds. Unadmitted payloads must
            // never queue behind it: their bytes would be retained without any budget charge.
            let mut state = self.state.try_lock().map_err(|_| busy())?;
            state.purge();
            // A shared approval clone may have been admitted between the first check and
            // acquiring the guard. Its existing charge must not be tested or added twice.
            if !base64.is_reserved() {
                state.room(base64.len())?;
                if !base64.reserve() {
                    return Err(busy());
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
}

struct Entry {
    author: DraftAuthor,
    account: String,
    touched: Instant,
    bytes: Arc<Vec<u8>>,
    ready: Option<PreparedImage>,
}

impl State {
    /// The source is read while this state stays locked. Budget raw transfer bytes, the MIME
    /// parser's decoded buffer and the final <=25 MiB copy, plus bounded wire/metadata scratch.
    /// This admits a 25 MiB base64 image even inside a much larger message, without fetching it.
    fn encoded_budget(&self) -> Result<usize, Response> {
        self.room(ORIGINAL + 1024 * 1024)?;
        Ok((MEMORY
            .saturating_sub(self.used())
            .saturating_sub(torro_ipc::mail::held_image_bytes())
            .saturating_sub(ORIGINAL + 1024 * 1024)
            / 2)
        .min(36 * 1024 * 1024))
    }

    fn purge(&mut self) {
        self.entries.retain(|_, entry| {
            entry.touched.elapsed() < Duration::from_secs(if entry.ready.is_some() { 3600 } else { 600 })
        });
    }

    fn used(&self) -> usize {
        self.entries.values().map(|e| e.bytes.capacity()).sum()
    }

    fn room(&self, more: usize) -> Result<(), Response> {
        if self.used().saturating_add(torro_ipc::mail::held_image_bytes()).saturating_add(more) > MEMORY {
            Err(busy())
        } else {
            Ok(())
        }
    }

    fn slots(&self, author: &DraftAuthor) -> Result<(), Response> {
        let unfinished: Vec<_> = self.entries.values().filter(|e| e.ready.is_none()).collect();
        if unfinished.len() >= 16 || unfinished.iter().filter(|e| &e.author == author).count() >= 4 {
            Err(busy())
        } else {
            Ok(())
        }
    }

    fn owned(&mut self, id: &str, account: &str, author: &DraftAuthor) -> Result<&mut Entry, Response> {
        let entry =
            self.entries.get_mut(id).filter(|e| e.account == account && &e.author == author).ok_or_else(not_found)?;
        entry.touched = Instant::now();
        Ok(entry)
    }
}

fn error(code: &str, text: &'static str) -> Response {
    Response::Error { code: code.into(), message: tr(text).into() }
}

fn not_found() -> Response {
    error("not-found", NO_IMAGE)
}
fn busy() -> Response {
    error("busy", "Too many images are being prepared. Try again later.")
}
fn invalid() -> Response {
    error("error", BAD_IMAGE)
}
fn conflict() -> Response {
    error("conflict", "The image upload offset or content does not match.")
}

/// Only after caller/reach checks: derive approval descriptions from configured accounts, the
/// mail cache and actual filesystem metadata. No source bytes are decoded or exposed here.
pub(crate) async fn facts(mail: &MailService, request: &Request) -> Option<crate::approvals::MailFacts> {
    use crate::approvals::{ImageFacts, MailFacts};
    let account = mail.config(request.mail_account()?).await.ok().map(|c| c.address);
    let mut image = ImageFacts::default();
    if let Request::PrepareMailImage { source, .. } = request {
        match source {
            ImageSource::File { path } => {
                let path = path.clone();
                image.file = tokio::task::spawn_blocking(move || {
                    let meta = std::fs::symlink_metadata(&path).ok()?;
                    if !meta.is_file() || meta.file_type().is_symlink() {
                        return None;
                    }
                    std::fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
                })
                .await
                .ok()
                .flatten();
            }
            ImageSource::Prepared { id } => {
                let source = {
                    let state = mail.images.state.lock().await;
                    state.entries.get(id).map(|entry| {
                        (entry.account.clone(), entry.ready.as_ref().map(|i| (i.original_width, i.original_height)))
                    })
                };
                if let Some((account, dimensions)) = source {
                    image.source_account = mail.config(&account).await.ok().map(|c| c.address);
                    image.dimensions = dimensions;
                }
            }
            ImageSource::MailPart { message, part } => {
                if let Ok(mut refs) = write::locate(mail, Reach::Suite, &[message]).await
                    && let Some(source) = refs.pop()
                {
                    image.source_account = mail.config(&source.account).await.ok().map(|c| c.address);
                    if let Ok(store) = mail.store(&source.account).await {
                        let id = message.clone();
                        if let Ok(Some(known)) = store.run(move |s| s.message(&id)).await {
                            image.subject = Some(known.summary.subject);
                            if let Some(info) = known.attachments.iter().find(|i| i.part == *part) {
                                image.part = Some(info.part.clone());
                                image.file = info.filename.clone();
                            }
                        }
                    }
                }
            }
        }
    }
    Some(MailFacts { account, image: Some(image), ..MailFacts::default() })
}

pub(crate) fn is_request(request: &Request) -> bool {
    matches!(
        request,
        Request::PrepareMailImage { .. } | Request::UploadMailImage { .. } | Request::ReleaseMailImages { .. }
    )
}

/// Reach and existence are checked for the app as well as clients, including at execution time.
async fn account(mail: &MailService, reach: Reach<'_>, id: &str) -> Result<(), Response> {
    if !mail.accounts_within(reach).await.iter().any(|known| known == id) {
        return Err(error("not-found", "No such mail account."));
    }
    mail.config(id).await.map(drop).map_err(|e| e.response())
}

pub(crate) async fn client_refusal(
    mail: &MailService,
    client: &str,
    reach: Reach<'_>,
    request: &Request,
    tools: Option<&Register>,
) -> Option<Response> {
    let author = DraftAuthor::Client(client.into());
    let account_id = request.mail_account()?;
    if is_request(request) {
        // Account reload may await store opening. Charge incoming payloads before that wait,
        // not just before the later image-store wait; no protected source is read here.
        if let Err(refused) = mail.images.reserve_request(request).await {
            return Some(refused);
        }
        if let Err(refused) = account(mail, reach, account_id).await {
            return Some(refused);
        }
    }
    match request {
        Request::PrepareMailImage { source, .. } => {
            check_source(mail, reach, &author, account_id, source, tools, true).await.err()
        }
        Request::UploadMailImage { upload: Some(id), .. } => {
            let mut state = mail.images.state.lock().await;
            state.purge();
            state.owned(id, account_id, &author).err()
        }
        Request::SaveDraft { draft } => {
            for attachment in &draft.attachments {
                if let DraftAttachment::Image { source, .. } = attachment
                    && let Err(e) = check_source(mail, reach, &author, account_id, source, tools, false).await
                {
                    return Some(e);
                }
            }
            None
        }
        _ => None,
    }
}

async fn check_source(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    account: &str,
    source: &ImageSource,
    tools: Option<&Register>,
    copying: bool,
) -> Result<(), Response> {
    match source {
        ImageSource::Prepared { id } => {
            let mut state = mail.images.state.lock().await;
            state.purge();
            let entry =
                state.entries.get(id).filter(|e| &e.author == author && e.ready.is_some()).ok_or_else(not_found)?;
            if entry.account != account {
                if !copying {
                    return Err(not_found());
                }
                // Preparing may copy this author's RAM original to another reachable account;
                // saving still resolves only an entry already owned by the destination account.
                self::account(mail, reach, &entry.account).await?;
            }
            Ok(())
        }
        ImageSource::File { path } => {
            let draft = Draft {
                id: None,
                account: account.into(),
                to: vec![],
                cc: vec![],
                bcc: vec![],
                subject: String::new(),
                body: String::new(),
                html: None,
                in_reply_to: None,
                attachments: vec![DraftAttachment::File { path: path.clone() }],
            };
            write::check_draft(mail, author, &draft, mail.attachment_cache().as_ref(), tools)
                .await
                .map(drop)
                .map_err(|e| e.response())
        }
        ImageSource::MailPart { .. } => Ok(()), // message_ids/locate checks reach before approval.
    }
}

pub(crate) async fn handle(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    request: Request,
    tools: Option<&Register>,
) -> Response {
    if let Some(refused) = torro_ipc::mail::limit_refusal(&request) {
        return refused;
    }
    let Some(id) = request.mail_account() else { return invalid() };
    if let Err(refused) = mail.images.reserve_request(&request).await {
        return refused;
    }
    if let Err(refused) = account(mail, reach, id).await {
        return refused;
    }
    // Start only after admission, so the first upload cannot contend with the cleanup worker
    // it just spawned while still holding an unbudgeted payload.
    mail.images.start_cleanup();
    match request {
        Request::PrepareMailImage { account, source } => {
            prepare(mail, reach, author, &account, source, tools).await.unwrap_or_else(|e| e)
        }
        Request::UploadMailImage { account, upload, offset, base64, finish } => {
            upload_image(mail, author, &account, upload, offset, &base64, finish).await.unwrap_or_else(|e| e)
        }
        Request::ReleaseMailImages { account, ids } => {
            let mut state = mail.images.state.lock().await;
            state.purge();
            for id in ids {
                if state.entries.get(&id).is_some_and(|e| e.account == account && &e.author == author) {
                    state.entries.remove(&id);
                }
            }
            Response::Ok
        }
        _ => invalid(),
    }
}

async fn prepare(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    account: &str,
    source: ImageSource,
    tools: Option<&Register>,
) -> Result<Response, Response> {
    if let ImageSource::Prepared { id } = &source {
        let mut state = mail.images.state.lock().await;
        state.purge();
        let entry = state.entries.get_mut(id).filter(|e| &e.author == author).ok_or_else(not_found)?;
        let mut image = entry.ready.clone().ok_or_else(not_found)?;
        if entry.account == account {
            entry.touched = Instant::now();
        } else {
            self::account(mail, reach, &entry.account).await?;
            let bytes = Arc::clone(&entry.bytes);
            // Both accounts were checked, and the original never passes through the UI or disk.
            // A distinct lease lets the old account be released only after the copy succeeded.
            state.room(bytes.capacity())?;
            let id = super::random_hex(16).map_err(|_| invalid())?;
            image.id = id.clone();
            state.entries.insert(
                id,
                Entry {
                    author: author.clone(),
                    account: account.into(),
                    touched: Instant::now(),
                    bytes,
                    ready: Some(image.clone()),
                },
            );
        }
        return Ok(Response::MailImage { image });
    }
    let mut state = mail.images.state.lock().await;
    state.purge();
    state.slots(author)?;
    state.room(ORIGINAL + ENCODER_SCRATCH + PREVIEW * 3)?;
    let attachment = match source {
        ImageSource::File { path } => DraftAttachment::File { path },
        ImageSource::MailPart { message, part } => DraftAttachment::MailPart { message, part },
        ImageSource::Prepared { .. } => return Err(not_found()),
    };
    // The lock reserves the worst-case size before a byte is read. The decoder's transient
    // allocation is separately bounded at 128 MiB and there is only one decoder.
    let budget = state.encoded_budget()?;
    let input =
        write::image_attachment_of(mail, reach, &attachment, author, tools, budget).await.map_err(|e| e.response())?;
    state.room(input.bytes.capacity().saturating_add(ENCODER_SCRATCH + PREVIEW * 3))?;
    let bytes = Arc::new(input.bytes);
    let decoding = Arc::clone(&bytes);
    let image = tokio::task::spawn_blocking(move || describe(&decoding)).await.map_err(|_| invalid())??;
    let id = super::random_hex(16).map_err(|_| invalid())?;
    let image = PreparedImage { id: id.clone(), ..image };
    // The new preview is now charged globally; check the actual retained source capacity
    // again before publishing its lease, even if a future reader changes allocation strategy.
    state.room(bytes.capacity())?;
    state.entries.insert(
        id,
        Entry {
            author: author.clone(),
            account: account.into(),
            touched: Instant::now(),
            bytes,
            ready: Some(image.clone()),
        },
    );
    Ok(Response::MailImage { image })
}

async fn upload_image(
    mail: &MailService,
    author: &DraftAuthor,
    account: &str,
    upload: Option<String>,
    offset: u64,
    base64: &str,
    finish: bool,
) -> Result<Response, Response> {
    let mut state = mail.images.state.lock().await;
    state.purge();
    state.room(IMAGE_CHUNK_BYTES + ENCODER_SCRATCH + PREVIEW * 3)?;
    let chunk = STANDARD.decode(base64).map_err(|_| invalid())?;
    if chunk.len() > IMAGE_CHUNK_BYTES {
        return Err(invalid());
    }
    let id = match upload {
        Some(id) => id,
        None => {
            if offset != 0 {
                return Err(conflict());
            }
            state.slots(author)?;
            let id = super::random_hex(16).map_err(|_| invalid())?;
            state.entries.insert(
                id.clone(),
                Entry {
                    author: author.clone(),
                    account: account.into(),
                    touched: Instant::now(),
                    bytes: Arc::new(Vec::new()),
                    ready: None,
                },
            );
            id
        }
    };
    let entry = state.owned(&id, account, author)?;
    let offset = usize::try_from(offset).map_err(|_| conflict())?;
    let end = offset.checked_add(chunk.len()).ok_or_else(invalid)?;
    if end > ORIGINAL {
        return Err(invalid());
    }
    if offset < entry.bytes.len() || entry.ready.is_some() {
        if entry.bytes.get(offset..end) != Some(chunk.as_slice()) {
            return Err(conflict());
        }
    } else if offset == entry.bytes.len() {
        let bytes = Arc::get_mut(&mut entry.bytes).ok_or_else(busy)?;
        bytes.try_reserve_exact(chunk.len()).map_err(|_| busy())?;
        bytes.extend_from_slice(&chunk);
    } else {
        return Err(conflict());
    }
    if finish {
        if end != entry.bytes.len() {
            return Err(conflict());
        }
        if entry.ready.is_none() {
            let bytes = Arc::clone(&entry.bytes);
            let image = tokio::task::spawn_blocking(move || describe(&bytes)).await.map_err(|_| invalid())??;
            entry.ready = Some(PreparedImage { id: id.clone(), ..image });
        }
        Ok(Response::MailImage { image: entry.ready.clone().ok_or_else(invalid)? })
    } else {
        Ok(Response::MailImageUpload { id, offset: entry.bytes.len() as u64 })
    }
}

/// Resolves image bytes while holding the source lease. Saved MailParts are decoded afresh,
/// never silently re-read from a file when a Prepared lease expired.
pub(crate) struct ImageAttachment {
    pub content: OutgoingAttachment,
    pub reservations: Vec<torro_ipc::mail::ImageReservation>,
}

impl std::ops::Deref for ImageAttachment {
    type Target = OutgoingAttachment;
    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

pub(crate) async fn attachment(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    account_id: &str,
    source: &ImageSource,
    size: ImageSize,
    tools: Option<&Register>,
) -> Result<ImageAttachment, WriteError> {
    account(mail, reach, account_id).await.map_err(|_| WriteError::NotFound(NO_IMAGE))?;
    let mut state = mail.images.state.lock().await;
    state.purge();
    let bytes = match source {
        ImageSource::Prepared { id } => {
            let entry = state.owned(id, account_id, author).map_err(|_| WriteError::NotFound(NO_IMAGE))?;
            if entry.ready.is_none() {
                return Err(WriteError::NotFound(NO_IMAGE));
            }
            Arc::clone(&entry.bytes)
        }
        source => {
            state.room(ORIGINAL + PREVIEW * 2).map_err(|_| WriteError::Busy)?;
            let input = match source {
                ImageSource::File { path } => DraftAttachment::File { path: path.clone() },
                ImageSource::MailPart { message, part } => {
                    DraftAttachment::MailPart { message: message.clone(), part: part.clone() }
                }
                ImageSource::Prepared { .. } => return Err(WriteError::NotFound(NO_IMAGE)),
            };
            let budget = state.encoded_budget().map_err(|_| WriteError::Busy)?;
            let input = write::image_attachment_of(mail, reach, &input, author, tools, budget).await?;
            Arc::new(input.bytes)
        }
    };
    // Reserve before the encoder can copy an Original or allocate its normal output. Moving the
    // charge into the worker also retains it if the caller drops its future during encoding.
    state
        .room(
            ORIGINAL
                + ENCODER_SCRATCH
                + if matches!(source, ImageSource::Prepared { .. }) { 0 } else { bytes.capacity() },
        )
        .map_err(|_| WriteError::Busy)?;
    let output = torro_ipc::mail::ImageReservation::new(ORIGINAL + ENCODER_SCRATCH).ok_or(WriteError::Busy)?;
    let (content, mut output) = tokio::task::spawn_blocking(move || {
        encode(&bytes, size == ImageSize::Original).map(|content| (content, output))
    })
    .await
    .map_err(|_| WriteError::Invalid(BAD_IMAGE))?
    .map_err(|_| WriteError::Invalid(BAD_IMAGE))?;
    output.shrink_to(content.bytes.capacity());
    // Existing MIME construction clones attachments and may simultaneously retain transfer-
    // encoded Vec capacity, a Bcc copy and parser buffers. Charge a conservative upper bound,
    // rather than allowing repeated CIDs to amplify one source into hundreds of MiB.
    let scratch = content.bytes.capacity().saturating_mul(8).saturating_add(64 * 1024);
    state.room(scratch).map_err(|_| WriteError::Busy)?;
    let scratch = torro_ipc::mail::ImageReservation::new(scratch).ok_or(WriteError::Busy)?;
    Ok(ImageAttachment { content, reservations: vec![output, scratch] })
}

fn decode(bytes: &[u8]) -> Result<(image::DynamicImage, image::ImageFormat), Response> {
    if bytes.len() > ORIGINAL {
        return Err(invalid());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| invalid())?;
    let format = reader.format().ok_or_else(invalid)?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP
    ) {
        return Err(invalid());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| invalid())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32_000_000 {
        return Err(invalid());
    }
    let orientation = decoder.orientation().map_err(|_| invalid())?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|_| invalid())?;
    image.apply_orientation(orientation);
    Ok((image, format))
}

fn describe(bytes: &[u8]) -> Result<PreparedImage, Response> {
    let _scratch = torro_ipc::mail::ImageReservation::new(ENCODER_SCRATCH + PREVIEW).ok_or_else(busy)?;
    let (image, _) = decode(bytes)?;
    let (original_width, original_height) = (image.width(), image.height());
    let width = original_width.min(1600);
    let height = (u64::from(original_height) * u64::from(width) / u64::from(original_width)).max(1) as u32;
    let mut preview = image.thumbnail(512, 512);
    let preview = loop {
        let mut png = Bounded::<PREVIEW>(Cursor::new(Vec::new()));
        if stream_png(&preview, &mut png).is_ok() {
            break png.0.into_inner();
        }
        if preview.width() == 1 && preview.height() == 1 {
            return Err(invalid());
        }
        preview = preview.thumbnail((preview.width() / 2).max(1), (preview.height() / 2).max(1));
    };
    let preview: torro_ipc::mail::ImageData = STANDARD.encode(preview).into();
    if !preview.reserve() {
        return Err(busy());
    }
    Ok(PreparedImage { id: String::new(), width, height, original_width, original_height, preview })
}

fn encode(bytes: &[u8], original: bool) -> Result<OutgoingAttachment, Response> {
    let (mut image, format) = decode(bytes)?;
    // MIME size markers are sender-controlled UI metadata, never evidence of normalization.
    // Only an explicit Original choice may preserve source bytes without conversion.
    if original {
        let kind = match format {
            image::ImageFormat::Jpeg => "jpeg",
            image::ImageFormat::Gif => "gif",
            image::ImageFormat::WebP => "webp",
            _ => "png",
        };
        return Ok(OutgoingAttachment {
            filename: format!("image.{kind}"),
            content_type: format!("image/{kind}"),
            bytes: bytes.to_vec(),
            content_id: None,
            inline: false,
            image_size: None,
        });
    }
    if image.width() > 1600 {
        image = image.resize(1600, u32::MAX, image::imageops::FilterType::Lanczos3);
    }
    let mut out = Bounded::<ORIGINAL>(Cursor::new(Vec::new()));
    let kind = if format == image::ImageFormat::Jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&image).map_err(|_| invalid())?;
        "jpeg"
    } else {
        stream_png(&image, &mut out)?;
        "png"
    };
    if out.0.get_ref().len() > ORIGINAL {
        return Err(invalid());
    }
    Ok(OutgoingAttachment {
        filename: format!("image.{kind}"),
        content_type: format!("image/{kind}"),
        bytes: out.0.into_inner(),
        content_id: None,
        inline: false,
        image_size: None,
    })
}

/// Encode scanlines, never a full-frame zlib staging buffer. Preserve 8/16-bit colour and alpha;
/// the only conversion buffer is one 16-bit row in PNG's big-endian sample order.
fn stream_png(image: &image::DynamicImage, output: &mut impl std::io::Write) -> Result<(), Response> {
    use image::ColorType;
    use std::io::Write as _;
    let (color, depth, channels) = match image.color() {
        ColorType::L8 => (png::ColorType::Grayscale, png::BitDepth::Eight, 1),
        ColorType::La8 => (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight, 2),
        ColorType::Rgb8 => (png::ColorType::Rgb, png::BitDepth::Eight, 3),
        ColorType::Rgba8 => (png::ColorType::Rgba, png::BitDepth::Eight, 4),
        ColorType::L16 => (png::ColorType::Grayscale, png::BitDepth::Sixteen, 1),
        ColorType::La16 => (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen, 2),
        ColorType::Rgb16 => (png::ColorType::Rgb, png::BitDepth::Sixteen, 3),
        ColorType::Rgba16 => (png::ColorType::Rgba, png::BitDepth::Sixteen, 4),
        _ => return Err(invalid()),
    };
    if image.width() == 0 || image.width() > 1600 {
        return Err(invalid());
    }
    let sixteen = depth == png::BitDepth::Sixteen;
    let row_bytes = image.width() as usize * channels * if sixteen { 2 } else { 1 };
    let mut endian = Vec::new();
    if sixteen {
        endian.try_reserve_exact(row_bytes).map_err(|_| busy())?;
    }
    let mut encoder = png::Encoder::new(output, image.width(), image.height());
    encoder.set_color(color);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header().map_err(|_| invalid())?;
    {
        let mut stream = writer.stream_writer_with_size(8192).map_err(|_| invalid())?;
        for row in image.as_bytes().chunks_exact(row_bytes) {
            let row = if sixteen {
                endian.clear();
                for sample in row.chunks_exact(2) {
                    endian.extend_from_slice(&u16::from_ne_bytes([sample[0], sample[1]]).to_be_bytes());
                }
                endian.as_slice()
            } else {
                row
            };
            stream.write_all(row).map_err(|_| invalid())?;
        }
        stream.finish().map_err(|_| invalid())?;
    }
    writer.finish().map_err(|_| invalid())
}

/// Bound capacity as well as length before forwarding a write. A small PNG CRC/IEND trailer
/// must never double an already large Vec beyond the output reservation.
struct Bounded<const LIMIT: usize>(Cursor<Vec<u8>>);
impl<const LIMIT: usize> std::io::Write for Bounded<LIMIT> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = usize::try_from(self.0.position()).ok().and_then(|position| position.checked_add(bytes.len()));
        let Some(end) = end.filter(|end| *end <= LIMIT) else {
            return Err(std::io::Error::other("image output limit"));
        };
        let buffer = self.0.get_mut();
        if end > buffer.capacity() {
            let capacity = buffer.capacity().saturating_mul(2).max(end).min(LIMIT);
            buffer.try_reserve_exact(capacity - buffer.len()).map_err(std::io::Error::other)?;
        }
        std::io::Write::write(&mut self.0, bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<const LIMIT: usize> std::io::Seek for Bounded<LIMIT> {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_small_trailer_never_doubles_capacity_beyond_its_reservation() {
        use std::io::Write as _;
        let mut out = Bounded::<ORIGINAL>(Cursor::new(Vec::new()));
        out.write_all(&vec![0; 15 * 1024 * 1024]).expect("large encoder chunk");
        out.write_all(&[0; 4]).expect("CRC trailer");
        assert!(
            out.0.get_ref().capacity() <= ORIGINAL,
            "output capacity exceeded its 25-MiB reservation: {}",
            out.0.get_ref().capacity()
        );
        out.write_all(&vec![0; ORIGINAL - 15 * 1024 * 1024 - 4]).expect("exact limit");
        assert_eq!(out.0.get_ref().len(), ORIGINAL);
        assert_eq!(out.0.get_ref().capacity(), ORIGINAL);
        assert!(out.write_all(&[0]).is_err(), "reject excess before allocating");
        assert_eq!(out.0.get_ref().capacity(), ORIGINAL);
    }

    #[test]
    fn streamed_png_preserves_all_supported_sample_depths_and_alpha() {
        use image::{DynamicImage, ImageBuffer};
        let bytes = [0_u8, 1, 0x12, 0x80, 0xfe, 0xff];
        let words = [0_u16, 1, 0x1234, 0x80ff, 0xff00, 0xffff];
        let images = [
            DynamicImage::ImageLuma8(ImageBuffer::from_fn(5, 3, |x, y| image::Luma([bytes[((x + y) % 6) as usize]]))),
            DynamicImage::ImageLumaA8(ImageBuffer::from_fn(5, 3, |x, y| {
                image::LumaA([bytes[x as usize], bytes[y as usize]])
            })),
            DynamicImage::ImageRgb8(ImageBuffer::from_fn(5, 3, |x, y| {
                image::Rgb([bytes[x as usize], bytes[y as usize], 0xff])
            })),
            DynamicImage::ImageRgba8(ImageBuffer::from_fn(5, 3, |x, y| {
                image::Rgba([bytes[x as usize], 0x80, 0xff, bytes[y as usize]])
            })),
            DynamicImage::ImageLuma16(ImageBuffer::from_fn(5, 3, |x, y| image::Luma([words[((x + y) % 6) as usize]]))),
            DynamicImage::ImageLumaA16(ImageBuffer::from_fn(5, 3, |x, y| {
                image::LumaA([words[x as usize], words[y as usize]])
            })),
            DynamicImage::ImageRgb16(ImageBuffer::from_fn(5, 3, |x, y| {
                image::Rgb([words[x as usize], words[y as usize], 0xffff])
            })),
            DynamicImage::ImageRgba16(ImageBuffer::from_fn(5, 3, |x, y| {
                image::Rgba([words[x as usize], 0x80ff, 0xffff, words[y as usize]])
            })),
        ];
        for image in images {
            let mut png = Vec::new();
            stream_png(&image, &mut png).expect("stream");
            let restored = image::load_from_memory(&png).expect("decode streamed PNG");
            assert_eq!(restored.color(), image.color());
            assert_eq!(restored.as_bytes(), image.as_bytes(), "sample bytes and alpha must survive streaming");
            let normalized = encode(&png, false).expect("normalize");
            let restored = image::load_from_memory(&normalized.bytes).expect("decode normalized PNG");
            assert_eq!(restored.color(), image.color());
            assert_eq!(restored.as_bytes(), image.as_bytes());
            let prepared = describe(&png).expect("preview also accepts this colour type");
            assert!(!prepared.preview.is_empty());
        }
    }

    #[test]
    fn png_encoding_streams_bounded_idat_chunks_instead_of_staging_a_whole_frame() {
        let mut pixels = image::RgbaImage::new(192, 192);
        let mut random = 42_u32;
        for pixel in pixels.as_mut() {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *pixel = random as u8;
        }
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels).write_to(&mut source, image::ImageFormat::Png).expect("source PNG");
        let output = encode(source.get_ref(), false).expect("encode");
        let mut chunks = &output.bytes[8..];
        let mut count = 0;
        while chunks.len() >= 12 {
            let length = u32::from_be_bytes(chunks[..4].try_into().expect("length")) as usize;
            if &chunks[4..8] == b"IDAT" {
                assert!(length <= 8192, "whole-frame PNG staging produced an IDAT of {length} bytes");
                count += 1;
            }
            chunks = &chunks[length + 12..];
        }
        assert!(count > 1, "incompressible output must exercise multiple bounded chunks");
        assert!(image::load_from_memory(&output.bytes).is_ok(), "streamed output remains a valid PNG");
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(width, height).write_to(&mut out, image::ImageFormat::Png).expect("png");
        out.into_inner()
    }

    #[test]
    fn normal_reduces_without_upscaling_and_original_is_exact() {
        let bytes = png(1800, 900);
        let normal = encode(&bytes, false).expect("normal");
        let (reduced, _) = decode(&normal.bytes).expect("decode");
        assert_eq!((reduced.width(), reduced.height()), (1600, 800));
        assert_eq!(encode(&bytes, true).expect("original").bytes, bytes);
        let small = png(60, 40);
        assert_eq!(describe(&small).expect("preview").width, 60);
        assert!(describe(b"<svg></svg>").is_err());
        assert!(describe(&png(8001, 4000)).is_err(), "32MP before decoding");
    }

    #[tokio::test]
    async fn uploads_are_owned_idempotent_limited_and_expire() {
        let mail = MailService::without_home();
        let owner = DraftAuthor::Client("one".into());
        let foreign = DraftAuthor::Client("two".into());
        let chunk = STANDARD.encode(png(10, 10));
        let result = upload_image(&mail, &owner, "mail:a", None, 0, &chunk, false).await.expect("upload");
        let Response::MailImageUpload { id, offset } = result else { panic!("upload") };
        let retry = upload_image(&mail, &owner, "mail:a", Some(id.clone()), 0, &chunk, false).await.expect("retry");
        assert_eq!(retry, Response::MailImageUpload { id: id.clone(), offset });
        assert!(
            matches!(upload_image(&mail, &foreign, "mail:a", Some(id.clone()), 0, &chunk, false).await, Err(Response::Error { code, .. }) if code == "not-found")
        );
        assert!(upload_image(&mail, &owner, "mail:b", Some(id.clone()), 0, &chunk, false).await.is_err());
        assert!(upload_image(&mail, &owner, "mail:a", Some(id.clone()), 1, &chunk, false).await.is_err());
        assert!(matches!(
            upload_image(&mail, &owner, "mail:a", Some(id.clone()), 0, &chunk, true).await,
            Ok(Response::MailImage { .. })
        ));
        let mut state = mail.images.state.lock().await;
        state.entries.get_mut(&id).expect("entry").touched -= Duration::from_secs(3601);
        state.purge();
        assert!(state.entries.is_empty());
    }

    #[tokio::test]
    async fn file_imports_budget_retained_capacity_under_source_preload() {
        const TEST: &str = "mail::images::tests::file_imports_budget_retained_capacity_under_source_preload";
        const CHILD: &str = "TORRO_FILE_IMAGE_BUDGET_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([TEST, "--exact", "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .expect("isolated source budget");
            assert!(
                output.status.success() && String::from_utf8_lossy(&output.stdout).contains(&format!("{TEST} ... ok")),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("source.png");
        let mail = MailService::without_home();
        let source = ImageSource::File { path: path.to_string_lossy().into_owned() };
        // These real valid imports used to retain 32 + 32 + 8 + 32 MiB: all were admitted
        // by the 25.5-MiB precheck, but the fourth put the store above its 100-MiB limit.
        for size in [24, 24, 6, 24] {
            let mut bytes = png(10, 10);
            bytes.resize(size * 1024 * 1024, 0);
            std::fs::write(&path, &bytes).expect("valid PNG with trailing source bytes");
            drop(bytes);
            let response = prepare(&mail, Reach::Suite, &DraftAuthor::App, "mail:a", source.clone(), None).await;
            assert!(matches!(response, Ok(Response::MailImage { .. })), "prepare: {response:?}");
            let state = mail.images.state.lock().await;
            assert!(
                state.used() + torro_ipc::mail::held_image_bytes() <= MEMORY,
                "retained source capacities exceeded 100 MiB: {}",
                state.used()
            );
        }
        let response = prepare(&mail, Reach::Suite, &DraftAuthor::App, "mail:a", source, None).await;
        assert!(
            matches!(response, Err(Response::Error { code, .. }) if code == "busy"),
            "the next large source is refused before reading"
        );
        assert_eq!(mail.images.state.lock().await.entries.len(), 4, "rejection stores no additional source");
    }

    #[tokio::test]
    async fn parallel_upload_payloads_do_not_wait_unbudgeted_behind_a_blocked_import() {
        use futures_util::{FutureExt as _, future::join_all};
        let store = ImageStore::default();
        // Imports retain this same guard across the bounded MIME fetch and blocking decoder.
        // Hold that phase open deterministically, with no network timing or timeout involved.
        let import = store.state.lock().await;
        let requests = (0..8)
            .map(|_| Request::UploadMailImage {
                account: "mail:a".into(),
                upload: None,
                offset: 0,
                base64: STANDARD.encode(vec![0; IMAGE_CHUNK_BYTES]).into(),
                finish: false,
            })
            .collect::<Vec<_>>();
        let replies = join_all(requests.iter().map(|request| store.reserve_request(request)))
            .now_or_never()
            .expect("unadmitted payloads must be refused before waiting for an import");
        for (request, reply) in requests.iter().zip(replies) {
            assert!(matches!(reply, Err(Response::Error { code, .. }) if code == "busy"));
            let Request::UploadMailImage { base64, .. } = request else { panic!("upload") };
            assert!(!base64.is_reserved(), "rejection neither queues nor retains payloads");
        }
        drop(import);
        store.reserve_request(&requests[0]).await.expect("admission after the import releases its lock");
        let Request::UploadMailImage { base64, .. } = &requests[0] else { panic!("upload") };
        assert!(base64.is_reserved());
        // A cloned, already admitted approval is counted throughout a later import. It need
        // not wait again merely to confirm the existing charge, and must not double-charge it.
        let _import = store.state.lock().await;
        store
            .reserve_request(&requests[0].clone())
            .now_or_never()
            .expect("already admitted approvals remain budgeted without taking the import lock")
            .expect("admitted");
    }

    #[tokio::test]
    async fn cleanup_runs_without_another_image_request_and_upload_slots_are_bounded() {
        let mail = MailService::without_home();
        let owner = DraftAuthor::Client("one".into());
        let mut ids = Vec::new();
        for _ in 0..4 {
            let response = upload_image(&mail, &owner, "mail:a", None, 0, "AA==", false).await.expect("slot");
            let Response::MailImageUpload { id, .. } = response else { panic!("upload") };
            ids.push(id);
        }
        assert!(
            matches!(upload_image(&mail, &owner, "mail:a", None, 0, "AA==", false).await, Err(Response::Error { code, .. }) if code == "busy")
        );
        {
            let mut state = mail.images.state.lock().await;
            assert!(state.room(MEMORY).is_err());
            for entry in state.entries.values_mut() {
                entry.touched -= Duration::from_secs(601);
            }
        }
        mail.images.start_cleanup();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if mail.images.state.lock().await.entries.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("scheduled cleanup, no subsequent image operation");
        let request = Request::UploadMailImage {
            account: "mail:missing".into(),
            upload: None,
            offset: 0,
            base64: "AA==".into(),
            finish: false,
        };
        assert!(
            matches!(client_refusal(&mail, "one", Reach::Suite, &request, None).await, Some(Response::Error { code, .. }) if code == "not-found")
        );
    }
}
