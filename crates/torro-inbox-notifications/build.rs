//! Build script: embeds the icon and the version information into `torro-inbox-notifications.exe`.
//!
//! Explorer ("Properties > Details"), the taskbar and SmartScreen read name, version, publisher and icon from this
//! Windows resource. The taskbar groups windows by program and shows this icon, which is why App Feed has a
//! program (and a crate) of its own instead of being `torroinbox --view notifications` (decision 8 in
//! `docs/lebenszyklus.md`). It is only built for Windows targets; on Linux and macOS this script only names the
//! product for the test in `main.rs`.

use std::error::Error;
use std::path::PathBuf;

/// The icon, relative to this crate (a placeholder until the real artwork, task AP.7 of issue #279).
const ICON: &str = "../../packaging/windows/torro-inbox-notifications.ico";

/// The product name Windows shows: the entry's `display_name` (`AppId::INBOX_NOTIFICATIONS`), checked by a test in
/// `main.rs`.
const PRODUCT: &str = "Torro App Feed";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rustc-env=TORRO_PRODUCT_NAME={PRODUCT}");

    // The target decides, not the machine that builds: `cfg!(windows)` here would describe the host, which is wrong
    // for cross builds in both directions.
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }

    let icon = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join(ICON);
    let icon = icon.to_str().ok_or("the icon path is not valid UTF-8")?;

    // `FileVersion` and `ProductVersion` come from `CARGO_PKG_VERSION`. Windows shows `FileDescription` as the
    // application's name (task manager, "Open with"), so it is the plain product name.
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set("ProductName", PRODUCT)
        .set("FileDescription", PRODUCT)
        .set("CompanyName", "Sven Wagener")
        .set("LegalCopyright", "Copyright (C) Sven Wagener")
        .set("OriginalFilename", "torro-inbox-notifications.exe");
    resource.compile()?;
    Ok(())
}
