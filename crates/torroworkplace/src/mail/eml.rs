//! Verified RFC-822 export: bounded local/IMAP capture into a private temporary spool, never a
//! reconstruction from reading bodies. No cache writes, seen flags or external image requests.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, MutexGuard};
use torro_core::i18n::tr;
use torro_ipc::Response;
use torro_mail_imap::imap::{BodyChunks, BodyError, ImapError};

use super::read::{self, Located};
use super::service::{MailError, MailService};
use super::store::{LocalOriginal, MailStore, MessageRef, ORIGINAL_MAX_BYTES};

const CHUNK: usize = 64 * 1024;
static EXPORT: Mutex<()> = Mutex::const_new(());

/// Tests using the process-wide single-export gate must not contend by accident.
#[cfg(test)]
pub(crate) static TESTS: Mutex<()> = Mutex::const_new(());

/// The single-export permit lives through publication, including off-runtime destination I/O.
pub struct Prepared {
    pub file: File,
    pub bytes: u64,
    pub sha256: String,
    _held: Arc<MutexGuard<'static, ()>>,
}

/// Export failures have stable codes and never expose source paths, headers or server text.
#[derive(Debug)]
pub enum Error {
    Busy,
    Gone,
    Changed,
    TooLarge,
    Incomplete,
    Io,
    Write(std::io::ErrorKind),
    Mail(MailError),
}

impl Error {
    pub fn response(&self) -> Box<Response> {
        let (code, message) = match self {
            Self::Busy => ("busy", tr("Another EML export is running.").to_string()),
            Self::Gone => ("not-found", tr("The original message is no longer available.").to_string()),
            Self::Changed => ("conflict", tr("The message changed during export; nothing was saved.").to_string()),
            Self::TooLarge => ("error", tr("The original message is too large to export as EML.").to_string()),
            Self::Incomplete | Self::Io | Self::Write(_) => {
                ("error", tr("The complete original could not be verified; nothing was saved.").to_string())
            }
            Self::Mail(error) => (
                error.code(),
                format!("{} {}", error.message(), tr("The complete original is unavailable; nothing was saved.")),
            ),
        };
        Box::new(Response::Error { code: code.into(), message })
    }
}

impl From<ImapError> for Error {
    fn from(error: ImapError) -> Self {
        Self::Mail(error.into())
    }
}

fn bounded(bytes: u64) -> Result<(), Error> {
    match bytes {
        0 => Err(Error::Incomplete),
        n if n > ORIGINAL_MAX_BYTES => Err(Error::TooLarge),
        _ => Ok(()),
    }
}

fn stable_original(mut item: LocalOriginal) -> LocalOriginal {
    item.flags.clear();
    item
}

/// Captures a local original under the store lock. A corrupt/missing/unprovable source is retried
/// once from the server; filesystem errors of our private spool are not hidden by a network retry.
fn local(store: &MailStore, place: &MessageRef) -> Result<Option<(File, u64, String)>, Error> {
    if store.resolve_message(&place.id).map_err(|_| Error::Io)?.as_ref() != Some(place) {
        return Err(Error::Changed);
    }
    let size = store.message_size(&place.id).map_err(|_| Error::Io)?;
    let Some(size) = size.filter(|&size| size > 0) else { return Ok(None) };
    let Some(original) = store.original_at(place).ok().flatten().map(stable_original) else { return Ok(None) };
    if original.bytes != size {
        return Ok(None);
    }
    bounded(size)?;
    let mut file = tempfile::tempfile().map_err(|_| Error::Io)?;
    let mut hash = Sha256::new();
    let mut offset = 0;
    while offset < size {
        let count = (size - offset).min(CHUNK as u64) as usize;
        let Some(piece) = store.original_chunk(original.seq, offset, count).ok().flatten() else { return Ok(None) };
        if piece.len() != count {
            return Ok(None);
        }
        file.write_all(&piece).map_err(|_| Error::Io)?;
        hash.update(&piece);
        offset += count as u64;
    }
    let eof = store.original_chunk(original.seq, size, 1).ok().flatten();
    let sha256 = hex(&hash.finalize());
    if !eof.is_some_and(|piece| piece.is_empty()) || sha256 != original.sha256 {
        return Ok(None);
    }
    if store.resolve_message(&place.id).map_err(|_| Error::Io)?.as_ref() != Some(place)
        || store.message_size(&place.id).map_err(|_| Error::Io)? != Some(size)
        || store.original_at(place).map_err(|_| Error::Io)?.map(stable_original).as_ref() != Some(&original)
    {
        return Err(Error::Changed);
    }
    file.rewind().map_err(|_| Error::Io)?;
    Ok(Some((file, size, sha256)))
}

/// Prepares exactly the message located through the caller's reachable stores. The permit is
/// non-waiting; all pieces and disk operations remain bounded and disk work stays off the runtime.
pub(crate) async fn prepare(mail: &MailService, located: &Located) -> Result<Prepared, Error> {
    let held = Arc::new(EXPORT.try_lock().map_err(|_| Error::Busy)?);
    let place = located.place.clone();
    let (local, held) =
        located.store.run(move |store| Ok((local(store, &place), held))).await.map_err(|_| Error::Io)?;
    let local = local?;
    if let Some((file, bytes, sha256)) = local {
        return Ok(Prepared { file, bytes, sha256, _held: held });
    }
    let place = located.place.clone();
    let initial = located.store.run(move |store| store.message_size(&place.id)).await.map_err(|_| Error::Io)?;
    let mut command = mail.command(&located.account).await.map_err(Error::Mail)?;
    // Refresh a reused selection too: the server may have renumbered the folder since last use.
    if command.session().select(&located.place.folder_path).await?.uid_validity != located.place.uidvalidity
        || !read::valid_selection(command.session(), &located.place).await.map_err(Error::Mail)?
    {
        return Err(Error::Gone);
    }
    let before = command.session().fetch_export_metadata(located.place.uid).await?.ok_or(Error::Gone)?;
    bounded(before.bytes)?;
    let worker_held = Arc::clone(&held);
    let mut file = tokio::task::spawn_blocking(move || {
        let _held = worker_held;
        tempfile::tempfile()
    })
    .await
    .map_err(|_| Error::Io)?
    .map_err(|_| Error::Io)?;
    let mut hash = Sha256::new();
    let mut body = BodyChunks::new(before.uid, before.bytes);
    loop {
        let piece = body.next(command.session()).await.map_err(|error| match error {
            BodyError::Vanished => Error::Gone,
            BodyError::Imap(error) => error.into(),
            BodyError::Truncated | BodyError::Oversized => Error::Incomplete,
        })?;
        let Some(piece) = piece else { break };
        // A cancelled caller must not release admission while its blocking write still owns bytes.
        let worker_held = Arc::clone(&held);
        (file, hash) = tokio::task::spawn_blocking(move || {
            let _held = worker_held;
            file.write_all(&piece).map_err(|_| Error::Io)?;
            hash.update(&piece);
            Ok::<_, Error>((file, hash))
        })
        .await
        .map_err(|_| Error::Io)??;
    }
    let after = command.session().fetch_export_metadata(located.place.uid).await?.ok_or(Error::Gone)?;
    if command.session().select(&located.place.folder_path).await?.uid_validity != located.place.uidvalidity {
        return Err(Error::Gone);
    }
    if before != after {
        return Err(Error::Changed);
    }
    let place = located.place.clone();
    let unchanged = located
        .store
        .run(move |store| {
            Ok(store.resolve_message(&place.id)?.as_ref() == Some(&place) && store.message_size(&place.id)? == initial)
        })
        .await
        .map_err(|_| Error::Io)?;
    if !unchanged {
        return Err(Error::Changed);
    }
    command.release();
    let worker_held = Arc::clone(&held);
    let file = tokio::task::spawn_blocking(move || {
        let _held = worker_held;
        file.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
        Ok::<_, Error>(file)
    })
    .await
    .map_err(|_| Error::Io)??;
    Ok(Prepared { file, bytes: before.bytes, sha256: hex(&hash.finalize()), _held: held })
}

/// Copies and verifies the spool a second time while writing the private destination temporary.
/// A short/long/corrupt spool must not replace an existing file or publish a new one.
pub fn copy(source: &mut File, target: &mut File, bytes: u64, sha256: &str) -> Result<(), Error> {
    bounded(bytes)?;
    source.rewind().map_err(|_| Error::Io)?;
    let mut left = bytes;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; CHUNK];
    while left > 0 {
        let count = left.min(CHUNK as u64) as usize;
        source.read_exact(&mut buffer[..count]).map_err(|_| Error::Incomplete)?;
        target.write_all(&buffer[..count]).map_err(|err| Error::Write(err.kind()))?;
        hash.update(&buffer[..count]);
        left -= count as u64;
    }
    if source.read(&mut buffer[..1]).map_err(|_| Error::Io)? != 0 || hex(&hash.finalize()) != sha256 {
        return Err(Error::Incomplete);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eml_errors_keep_codes_and_explain_the_missing_original() {
        assert!(bounded(ORIGINAL_MAX_BYTES).is_ok());
        assert!(matches!(bounded(0), Err(Error::Incomplete)));
        assert!(matches!(bounded(ORIGINAL_MAX_BYTES + 1), Err(Error::TooLarge)));
        for (error, expected) in [
            (Error::Mail(MailError::Offline("server secret".into())), "offline"),
            (Error::Mail(MailError::Auth), "auth"),
            (Error::Mail(MailError::NoPassword), "auth"),
            (Error::Gone, "not-found"),
            (Error::Changed, "conflict"),
            (Error::Incomplete, "error"),
            (Error::Io, "error"),
            (Error::Busy, "busy"),
        ] {
            let Response::Error { code, message } = *error.response() else { panic!("error") };
            assert_eq!(code, expected);
            assert!(!message.contains("server secret"));
            if matches!(error, Error::Mail(_)) {
                assert!(message.contains(tr("The complete original is unavailable; nothing was saved.")));
            }
        }
    }

    #[test]
    fn eml_copy_checks_hash_length_and_eof_in_bounded_chunks() {
        let original = vec![42u8; CHUNK * 2 + 5];
        let expected = hex(&Sha256::digest(&original));
        for mode in ["whole", "short", "extra", "hash", "write"] {
            let mut source = tempfile::tempfile().expect("spool");
            source.write_all(&original).expect("spool content");
            match mode {
                "short" => source.set_len(1).expect("short"),
                "extra" => source.write_all(b"extra").expect("extra"),
                "hash" => {
                    source.rewind().expect("rewind");
                    source.write_all(b"bad").expect("corruption");
                }
                _ => {}
            }
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("target");
            std::fs::write(&path, []).expect("file");
            let mut destination =
                std::fs::OpenOptions::new().read(true).write(mode != "write").open(&path).expect("open");
            let copied = copy(&mut source, &mut destination, original.len() as u64, &expected);
            assert_eq!(copied.is_ok(), mode == "whole", "{mode}: {copied:?}");
            if mode == "whole" {
                assert_eq!(std::fs::read(path).expect("copied"), original);
            }
        }
    }
}
