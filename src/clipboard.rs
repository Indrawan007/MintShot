//! Clipboard module
//!
//! Copies screenshot image directly to system clipboard as PNG image data.
//! Uses xclip (standard on Linux Mint) for reliable clipboard persistence —
//! the image stays in clipboard even after MintShot exits.
//!
//! Fallback chain:
//!   1. xclip (preferred — handles image/png natively)
//!   2. xsel  (fallback)
//!   3. arboard (Rust-native, but clipboard clears on process exit)

use log::{info, warn};
use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

/// Copy the screenshot PNG file to system clipboard as an image.
///
/// After this call, the user can immediately Ctrl+V paste the screenshot
/// into any application (Discord, Telegram, LibreOffice, GIMP, browser, etc.)
pub fn copy_to_clipboard(filepath: &str) -> Result<(), Box<dyn Error>> {
    // Try xclip first (most reliable on Linux Mint / Cinnamon)
    if copy_with_xclip(filepath).is_ok() {
        info!("Copied to clipboard via xclip (image/png)");
        return Ok(());
    }

    // Try xsel as fallback
    if copy_with_xsel(filepath).is_ok() {
        info!("Copied to clipboard via xsel");
        return Ok(());
    }

    // Last resort: xdotool + xdg approach
    if copy_with_xdg(filepath).is_ok() {
        info!("Copied to clipboard via xdg");
        return Ok(());
    }

    // If all external tools fail, try arboard (but warn it may not persist)
    warn!("External clipboard tools not found, using arboard (may not persist)");
    copy_with_arboard(filepath)
}

/// Copy using xclip — pipes raw PNG data as image/png MIME type
///
/// This is the gold standard for Linux clipboard image copying.
/// The image persists in clipboard after our process exits because
/// xclip runs as a background daemon holding the selection.
fn copy_with_xclip(filepath: &str) -> Result<(), Box<dyn Error>> {
    let png_data = std::fs::read(filepath)?;

    let mut child = Command::new("xclip")
        .args([
            "-selection", "clipboard",
            "-target", "image/png",
            "-i",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            info!("xclip not available: {}", e);
            e
        })?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(&png_data)?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(format!("xclip exited with status: {}", status).into());
    }

    Ok(())
}

/// Copy using xsel (fallback — less common but available on some systems)
fn copy_with_xsel(filepath: &str) -> Result<(), Box<dyn Error>> {
    let png_data = std::fs::read(filepath)?;

    let mut child = Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            info!("xsel not available: {}", e);
            e
        })?;

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(&png_data)?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(format!("xsel exited with status: {}", status).into());
    }

    Ok(())
}

/// Copy using xdg-mime / xclip combination
fn copy_with_xdg(filepath: &str) -> Result<(), Box<dyn Error>> {
    let status = Command::new("xclip")
        .args([
            "-selection", "clipboard",
            "-t", "image/png",
            filepath,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| {
            info!("xclip (file mode) not available: {}", e);
            e
        })?;

    if !status.success() {
        return Err("xclip file mode failed".into());
    }

    Ok(())
}

/// Rust-native clipboard via arboard (last resort)
///
/// WARNING: On X11, arboard clipboard content is lost when the process exits
/// unless a clipboard manager is running. This is a known X11 limitation.
fn copy_with_arboard(filepath: &str) -> Result<(), Box<dyn Error>> {
    let img = image::open(filepath)?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels = rgba.into_raw();

    let mut clipboard = arboard::Clipboard::new()?;

    let img_data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(pixels),
    };

    clipboard.set_image(img_data)?;

    // Keep clipboard alive briefly so clipboard manager can grab it
    std::thread::sleep(std::time::Duration::from_millis(500));

    info!("Copied via arboard (may not persist without clipboard manager)");
    Ok(())
}
