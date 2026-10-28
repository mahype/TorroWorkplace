//! Pictures of contacts: small previews for the lists, and pictures people add.
//!
//! - **Previews** of 64 and 256 pixels are made from the vCard's `PHOTO` and kept as PNG files
//!   (0600) under `<data>/contacts/photos/<account>/`, so a list does not push megabytes over the
//!   socket. Decoding and encoding again leaves every piece of metadata behind (EXIF with a place,
//!   camera, comments). The file name holds the picture's version, so a changed picture gets a new
//!   file; an account's folder goes with the account.
//! - **A new picture** (`contacts.set_photo`) is a PNG or JPEG of at most
//!   [`torro_ipc::contacts::MAX_PHOTO_BYTES`]; it is scaled to at most [`UPLOAD_EDGE`] pixels and
//!   written as JPEG, without metadata.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use torro_contacts::vcard::{self, Photo};
use torro_contacts::{PhotoRef, PhotoSize};

use crate::audit::hashed;

/// The longest edge of a picture a contact gets.
pub const UPLOAD_EDGE: u32 = 512;

/// Why a picture was refused, in English for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoError {
    TooLarge,
    NotAPicture,
}

impl std::fmt::Display for PhotoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhotoError::TooLarge => {
                write!(f, "The picture is larger than {} KB.", torro_ipc::contacts::MAX_PHOTO_BYTES / 1024)
            }
            PhotoError::NotAPicture => f.write_str("The picture is not a PNG or JPEG file."),
        }
    }
}

/// `data` as a contact's picture: checked, scaled down to [`UPLOAD_EDGE`] and written as JPEG.
pub fn prepare(data: &[u8]) -> Result<Photo, PhotoError> {
    if data.len() > torro_ipc::contacts::MAX_PHOTO_BYTES {
        return Err(PhotoError::TooLarge);
    }
    let format = image::guess_format(data).map_err(|_| PhotoError::NotAPicture)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(PhotoError::NotAPicture);
    }
    let picture = image::load_from_memory_with_format(data, format).map_err(|_| PhotoError::NotAPicture)?;
    let picture = if picture.width().max(picture.height()) > UPLOAD_EDGE {
        picture.resize(UPLOAD_EDGE, UPLOAD_EDGE, FilterType::Lanczos3)
    } else {
        picture
    };
    // JPEG has no alpha: flatten first.
    let mut jpeg = Vec::new();
    DynamicImage::ImageRgb8(picture.to_rgb8())
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|_| PhotoError::NotAPicture)?;
    Ok(Photo { data: jpeg, mime: "image/jpeg".into() })
}

/// The square preview of `raw`'s picture in `size`, as PNG; `None` without a picture one can read.
pub fn preview(raw: &str, size: PhotoSize) -> Option<Vec<u8>> {
    let photo = vcard::photo(raw)?;
    let picture = image::load_from_memory(&photo.data).ok()?;
    let edge = size.pixels();
    let square = picture.resize_to_fill(edge, edge, FilterType::Lanczos3);
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(square.to_rgba8()).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).ok()?;
    Some(png)
}

/// The preview files on disk; see the module docs.
#[derive(Debug, Clone)]
pub struct Previews {
    dir: Option<PathBuf>,
}

impl Previews {
    /// Previews under `dir`; `None` keeps none and makes each one anew.
    pub fn new(dir: Option<PathBuf>) -> Previews {
        Previews { dir }
    }

    fn file(&self, account: &str, contact: &str, photo: &PhotoRef, size: PhotoSize) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?.join(hashed(account));
        Some(dir.join(format!("{}-{}-{}.png", hashed(contact), hashed(&photo.version), size.pixels())))
    }

    /// The preview of `contact` (of `account`, its vCard `raw`, its picture `photo`) in `size`: from
    /// the file when there is one, else made and written.
    pub fn get(&self, account: &str, contact: &str, raw: &str, photo: &PhotoRef, size: PhotoSize) -> Option<Vec<u8>> {
        let file = self.file(account, contact, photo, size);
        if let Some(file) = &file
            && let Ok(png) = std::fs::read(file)
        {
            return Some(png);
        }
        let png = preview(raw, size)?;
        if let Some(file) = file
            && let Err(err) = write_private(&file, &png)
        {
            eprintln!("torroworkplace: contacts: a preview could not be kept: {}", err.kind());
        }
        Some(png)
    }

    /// Removes every preview of `account`.
    pub fn forget_account(&self, account: &str) {
        let Some(dir) = self.dir.as_ref().map(|dir| dir.join(hashed(account))) else { return };
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("torroworkplace: contacts: previews of a removed account stay: {}", err.kind()),
        }
    }
}

fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir)?;
    }
    crate::mail::create_private(file)?;
    std::fs::write(file, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(width, height, image::Rgb([200, 80, 40])))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("png");
        bytes
    }

    #[test]
    fn pictures_are_scaled_and_written_as_jpeg() {
        let photo = prepare(&png(1024, 600)).expect("a picture");
        assert_eq!(photo.mime, "image/jpeg");
        let back = image::load_from_memory(&photo.data).expect("jpeg");
        assert_eq!((back.width(), back.height()), (512, 300));
        assert_eq!(prepare(b"GIF89a....."), Err(PhotoError::NotAPicture));
        assert_eq!(prepare(&vec![0u8; torro_ipc::contacts::MAX_PHOTO_BYTES + 1]), Err(PhotoError::TooLarge));
    }

    #[test]
    fn previews_are_square_pngs_kept_per_account() {
        let dir = tempfile::tempdir().expect("dir");
        let raw = vcard::set_photo(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a\r\nFN:A\r\nEND:VCARD\r\n",
            Some(&Photo { data: png(300, 200), mime: "image/png".into() }),
        );
        let photo = vcard::read(&raw).expect("card").photo.expect("photo");
        let previews = Previews::new(Some(dir.path().to_path_buf()));
        let small = previews.get("nextcloud:a@x", "nextcloud:a@x/c/a", &raw, &photo, PhotoSize::Small).expect("png");
        let picture = image::load_from_memory_with_format(&small, ImageFormat::Png).expect("png");
        assert_eq!((picture.width(), picture.height()), (64, 64));
        let kept = std::fs::read_dir(dir.path().join(hashed("nextcloud:a@x"))).expect("folder").count();
        assert_eq!(kept, 1);
        previews.forget_account("nextcloud:a@x");
        assert!(!dir.path().join(hashed("nextcloud:a@x")).exists());
    }
}
