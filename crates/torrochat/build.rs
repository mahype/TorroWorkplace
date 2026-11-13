//! Embeds Torro Chat's existing app icon and version information into the Windows executable.
//! Other targets leave resource embedding to their native packaging tools.

use std::error::Error;
use std::path::PathBuf;

const ICON: &str = "../../packaging/windows/torro-chat.ico";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    // Test the target, not this build script's host, so cross compilation embeds resources too.
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let icon = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join(ICON);
    let icon = icon.to_str().ok_or("the icon path is not valid UTF-8")?;
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set("ProductName", "Torro Chat")
        .set("FileDescription", "Torro Chat")
        .set("CompanyName", "Sven Wagener")
        .set("LegalCopyright", "Copyright (C) Sven Wagener")
        .set("OriginalFilename", "torrochat.exe");
    resource.compile()?;
    Ok(())
}
