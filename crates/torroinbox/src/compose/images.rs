//! Local image input only (issue #461): dialogs return paths; screenshot pixels are encoded on
//! a worker and uploaded to the service. No UI files, clipboard polling or rich-text paste.

use super::ComposeError;
use std::path::PathBuf;

/// Debug reveals neither pixels nor clipboard text.
#[derive(Clone)]
pub(crate) enum Clipboard {
    Image(Vec<u8>),
    Text(String),
    Empty,
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Clipboard(…)")
    }
}

pub(super) async fn choose() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(super::tr("Insert image"))
        .add_filter(super::tr("Images"), &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file()
        .await
        .map(|f| f.path().to_path_buf())
}

pub(super) async fn clipboard() -> Result<Clipboard, ComposeError> {
    tokio::task::spawn_blocking(|| {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| failed())?;
        match clipboard.get_image() {
            Ok(pixels) => {
                if pixels.width == 0
                    || pixels.height == 0
                    || pixels.width > 32768
                    || pixels.height > 32768
                    || pixels.width.saturating_mul(pixels.height) > 32_000_000
                {
                    return Err(failed());
                }
                let mut png = std::io::Cursor::new(Vec::new());
                let image =
                    image::RgbaImage::from_raw(pixels.width as u32, pixels.height as u32, pixels.bytes.into_owned())
                        .ok_or_else(failed)?;
                image::DynamicImage::ImageRgba8(image)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .map_err(|_| failed())?;
                if png.get_ref().len() as u64 > torro_ipc::mail::MAX_FILE_BYTES {
                    return Err(failed());
                }
                Ok(Clipboard::Image(png.into_inner()))
            }
            Err(arboard::Error::ContentNotAvailable) => match clipboard.get_text() {
                Ok(text) => Ok(Clipboard::Text(text)),
                Err(arboard::Error::ContentNotAvailable) => Ok(Clipboard::Empty),
                Err(_) => Err(failed()),
            },
            Err(_) => Err(failed()),
        }
    })
    .await
    .map_err(|_| failed())?
}

fn failed() -> ComposeError {
    ComposeError::Other(super::tr("Couldn't read an image from the clipboard.").into())
}

pub(super) fn dropped(event: iced::Event, _: iced::event::Status, _: iced::window::Id) -> Option<super::Message> {
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(super::Message::ImagePath(Some(path))),
        _ => None,
    }
}
