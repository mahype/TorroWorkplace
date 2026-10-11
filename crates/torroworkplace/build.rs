//! Build script: embeds the icon and the version information into `torroworkplace.exe`.
//!
//! Explorer ("Properties > Details"), the taskbar and SmartScreen read name, version, publisher and icon from this
//! Windows resource. It is only built for Windows targets; on Linux and macOS this script does nothing.

use std::error::Error;
use std::path::PathBuf;

/// The icon, relative to this crate (made from `packaging/linux/icons/torro-workplace.svg`).
const ICON: &str = "../../packaging/windows/torro-workplace.ico";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");

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
        .set("ProductName", "Torro Workplace")
        .set("FileDescription", "Torro Workplace")
        .set("CompanyName", "Sven Wagener")
        .set("LegalCopyright", "Copyright (C) Sven Wagener")
        .set("OriginalFilename", "torroworkplace.exe");
    resource.compile()?;
    Ok(())
}
