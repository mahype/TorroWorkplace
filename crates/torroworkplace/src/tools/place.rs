//! Where a file of the assistant's own tools goes (task WZ.4, decision 16 in `docs/assistent.md`): the folder the
//! person set, else the system's desktop, else the home folder ([`folder`]); a name made from the title
//! ([`file_name`]); and a write that never replaces a file ([`write_new`]: `Report.pdf`, `Report-2.pdf`, …).
//!
//! Every root comes in as a parameter, so tests never touch the person's desktop or home folder. Nothing here
//! logs a title or a path.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A file name is at most this many characters before its extension.
pub const MAX_NAME_CHARS: usize = 80;

/// A file name is at most this many bytes (UTF-8) before its extension, whatever its characters: with `-999` and
/// `.pdf` it stays well below the 255 bytes (Linux, APFS) or UTF-16 units (NTFS) a name may have.
pub const MAX_NAME_BYTES: usize = 200;

/// The name of a file whose title leaves nothing usable.
pub const FALLBACK_NAME: &str = "Document";

/// The highest number a name gets before [`write_new`] gives up (`Report-999.pdf`).
pub const MAX_NUMBER: u32 = 999;

/// The folder a new file goes into: `setting` (the person's folder, `pdf_folder`) when it is a folder that exists
/// now; else `desktop` (`dirs::desktop_dir` in the core) when it exists; else `home`. A folder that does not exist
/// is passed over and never made: it may be on a drive that is not mounted.
pub fn folder(setting: Option<&Path>, desktop: Option<PathBuf>, home: &Path) -> PathBuf {
    if let Some(set) = setting.filter(|set| set.is_dir()) {
        return set.to_path_buf();
    }
    match desktop.filter(|desktop| desktop.is_dir()) {
        Some(desktop) => desktop,
        None => home.to_path_buf(),
    }
}

/// The file name for a document titled `title`, with `.pdf`: control characters and the characters no system
/// allows in a name (`/ \ : * ? " < > |`) become spaces, zero-width and bidi characters go (a name must read as
/// it is), runs of spaces one, dots and spaces at either end go (no
/// hidden file, no `..`), at most [`MAX_NAME_CHARS`] characters and [`MAX_NAME_BYTES`] bytes, cut between whole
/// characters (dots and spaces the cut leaves at the end go too); [`FALLBACK_NAME`] when nothing is left or the
/// name is one Windows keeps for devices (`CON`, `NUL`, `COM1`, `LPT¹` …, also with an extension).
pub fn file_name(title: &str) -> String {
    let replaced: String =
        title
            .chars()
            .filter(|c| !crate::mail::is_invisible(*c))
            .map(|c| {
                if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
    let joined = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut cut = String::new();
    for c in joined.chars().take(MAX_NAME_CHARS) {
        if cut.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        cut.push(c);
    }
    let stem = cut.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = if stem.is_empty() || reserved(stem) { FALLBACK_NAME } else { stem };
    format!("{stem}.pdf")
}

/// Windows' device names, which no file may have, whatever its case and extension (`nul.txt`, `CON .tar.gz`):
/// `CON`, `PRN`, `AUX`, `NUL`, the console's `CONIN$` and `CONOUT$`, and `COM` or `LPT` with one digit `0`–`9` or a
/// superscript `¹`, `²`, `³`.
fn reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let Some(port) = base.strip_prefix("COM").or_else(|| base.strip_prefix("LPT")) else {
        return false;
    };
    let mut digits = port.chars();
    matches!((digits.next(), digits.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
}

/// Writes `bytes` as a new file named `name` (from [`file_name`]) in `folder` and answers its path. An existing
/// file is never replaced: the name then gets `-2`, `-3` … up to [`MAX_NUMBER`] before its extension. The file is
/// made with `create_new`, so a file that appears meanwhile is not replaced either. Fails when every name is taken
/// or the folder cannot be written; a file that could not be written whole is removed again.
pub fn write_new(folder: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    for number in 1..=MAX_NUMBER {
        let candidate = match number {
            1 => folder.join(name),
            n => folder.join(format!("{stem}-{n}{extension}")),
        };
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };
        if let Err(err) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&candidate);
            return Err(err);
        }
        return Ok(candidate);
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "every name for the file is taken"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_the_set_one_then_the_desktop_then_home() {
        let root = tempfile::tempdir().expect("temp dir");
        let (set, desktop, home) = (root.path().join("PDFs"), root.path().join("Desktop"), root.path().join("home"));
        for dir in [&set, &desktop, &home] {
            fs::create_dir(dir).expect("made");
        }
        assert_eq!(folder(Some(&set), Some(desktop.clone()), &home), set);
        let gone = root.path().join("unmounted");
        assert_eq!(folder(Some(&gone), Some(desktop.clone()), &home), desktop, "a missing folder is passed over");
        assert_eq!(folder(None, Some(desktop.clone()), &home), desktop);
        assert_eq!(folder(None, Some(root.path().join("no desktop")), &home), home);
        assert_eq!(folder(None, None, &home), home);
        assert!(!gone.exists(), "nothing is made");
    }

    #[test]
    fn a_name_is_cleaned_from_the_title() {
        assert_eq!(file_name("Quarterly report"), "Quarterly report.pdf");
        assert_eq!(file_name("Offer: Müller/Söhne?\n2026"), "Offer Müller Söhne 2026.pdf");
        assert_eq!(file_name("../../etc/passwd"), "etc passwd.pdf");
        assert_eq!(file_name("..."), "Document.pdf");
        assert_eq!(file_name("  \t "), "Document.pdf");
        assert_eq!(file_name(".hidden"), "hidden.pdf");
        assert_eq!(file_name("con"), "Document.pdf");
        assert_eq!(file_name("COM1.txt"), "Document.pdf");
        assert_eq!(file_name("Combined"), "Combined.pdf");
        assert_eq!(file_name("Invoice\u{202E}fdp.exe"), "Invoicefdp.exe.pdf", "no bidi or zero-width characters");
        assert_eq!(file_name("A\u{200B}B\u{FEFF}"), "AB.pdf");
        let long = "ä".repeat(120);
        assert_eq!(file_name(&long), format!("{}.pdf", "ä".repeat(MAX_NAME_CHARS)));
    }

    /// Issue #225 (review): a long title of wide characters stays far below the 255 bytes a name may have on
    /// every system, also with `-999`, and no character is cut in half.
    #[test]
    fn a_name_stays_below_the_systems_limit_in_bytes() {
        let stem = |name: &str| name.strip_suffix(".pdf").expect("a pdf").to_string();
        let wide = stem(&file_name(&"😀".repeat(120)));
        assert_eq!(wide, "😀".repeat(MAX_NAME_BYTES / 4), "at most {MAX_NAME_BYTES} bytes");
        let odd = stem(&file_name(&format!("a{}", "😀".repeat(120))));
        assert!(odd.len() <= MAX_NAME_BYTES && odd.len() > MAX_NAME_BYTES - 4, "{}", odd.len());
        assert!(odd.ends_with('😀'), "whole characters only");
        let longest = format!("{wide}-{MAX_NUMBER}.pdf");
        assert!(longest.len() < 255, "{}", longest.len());
        // The system takes it, also numbered.
        let dir = tempfile::tempdir().expect("temp dir");
        let name = file_name(&"😀".repeat(120));
        write_new(dir.path(), &name, b"one").expect("written");
        let second = write_new(dir.path(), &name, b"two").expect("written");
        assert!(second.to_string_lossy().ends_with("-2.pdf"));
        // Cut where a space or a dot ends it: those go too.
        let spaced = format!("{} b", "a".repeat(MAX_NAME_CHARS - 1));
        assert_eq!(file_name(&spaced), format!("{}.pdf", "a".repeat(MAX_NAME_CHARS - 1)));
        assert_eq!(file_name("Report . ."), "Report.pdf");
    }

    /// Issue #225 (review): every name Windows keeps for devices, in any case and with any extension, also the
    /// superscript ports.
    #[test]
    fn every_device_name_of_windows_is_left_out() {
        let mut names: Vec<String> =
            ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].iter().map(|name| name.to_string()).collect();
        for port in ["COM", "LPT"] {
            names.extend(('0'..='9').chain(['¹', '²', '³']).map(|digit| format!("{port}{digit}")));
        }
        for name in &names {
            for title in [name.clone(), name.to_lowercase(), format!("{name}.txt"), format!("{} .tar.gz", name)] {
                assert_eq!(file_name(&title), "Document.pdf", "{title}");
            }
        }
        for kept in ["COM10", "LPT", "COMa", "Console", "COM¹²", "NULL", "AUXILIARY.txt", "CONIN", "CONOUT$X"] {
            assert_eq!(file_name(kept), format!("{kept}.pdf"));
        }
    }

    #[test]
    fn a_file_is_never_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = write_new(dir.path(), "Report.pdf", b"one").expect("written");
        let second = write_new(dir.path(), "Report.pdf", b"two").expect("written");
        let third = write_new(dir.path(), "Report.pdf", b"three").expect("written");
        assert_eq!(first, dir.path().join("Report.pdf"));
        assert_eq!(second, dir.path().join("Report-2.pdf"));
        assert_eq!(third, dir.path().join("Report-3.pdf"));
        assert_eq!(fs::read(&first).expect("read"), b"one", "the first one is kept");
        let missing = dir.path().join("missing");
        assert!(write_new(&missing, "Report.pdf", b"x").is_err(), "no folder, no file");
    }
}
