//! Clipboard module
//!
//! FIXES:
//!   #4  — xsel removed (text-only, cannot carry image/png binary)
//!   #10 — Accepts pre-encoded PNG bytes — no second encode pass
//!
//! Fallback chain (all image-capable):
//!   1. xclip  — image/png via stdin (best: persists after process exit)
//!   2. arboard — Rust-native RGBA image (clears on exit without clip mgr)
//!   3. xclip as filepath text — last resort, pastes file path not image

use log::{info, warn};
use std::borrow::Cow;
use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

// ─── Public API ───────────────────────────────────────────────────────────

/// Copy screenshot to clipboard.
///
/// `png_bytes` — pre-encoded PNG data from `save::save_png()`.
///               Reusing saves a second encode pass (Fix #10).
///
/// `pixels`, `width`, `height` — raw RGBA; used only by arboard fallback.
///
/// `filepath` — used as last-resort text fallback.
pub fn copy_to_clipboard(
    png_bytes: &[u8],
    pixels:    &[u8],
    width:     u32,
    height:    u32,
    filepath:  &str,
) -> Result<(), Box<dyn Error>> {

    // 1. xclip image/png — persists after our process exits
    if copy_with_xclip_image(png_bytes).is_ok() {
        info!("Clipboard: xclip image/png ✓");
        return Ok(());
    }

    // 2. arboard — Rust-native, works without external tools
    //    Clipboard clears on exit unless a clipboard manager is running,
    //    but that's acceptable as a fallback.
    if copy_with_arboard(pixels, width, height).is_ok() {
        info!("Clipboard: arboard RGBA ✓ (may not persist without clipboard manager)");
        return Ok(());
    }

    // 3. Last resort — filepath as text (user can open manually)
    warn!("Image clipboard unavailable — copying filepath as text");
    copy_filepath_as_text(filepath)?;
    info!("Clipboard: filepath text ✓ ({})", filepath);
    Ok(())
}

// ─── xclip image/png ──────────────────────────────────────────────────────

/// Pipe raw PNG bytes into xclip as image/png.
///
/// xclip forks into the background and holds the selection, so the
/// image survives after MintShot exits — this is the gold standard.
fn copy_with_xclip_image(png_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard", "-target", "image/png", "-i"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| { info!("xclip not found: {}", e); e })?;

    // Write PNG bytes then close stdin so xclip sees EOF
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(png_bytes)?;
        // stdin drop = EOF signal to xclip
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(format!("xclip exited {}", status).into());
    }
    Ok(())
}

// ─── arboard (Fix #4: replaces xsel which is text-only) ──────────────────

/// Copy via arboard (Rust-native).
///
/// Arboard on X11 owns the selection only while the process lives.
/// We sleep briefly so clipboard managers (Clipman, Parcellite, etc.)
/// have time to intercept and persist the content.
fn copy_with_arboard(
    pixels: &[u8],
    width:  u32,
    height: u32,
) -> Result<(), Box<dyn Error>> {
    let mut cb = arboard::Clipboard::new()?;

    cb.set_image(arboard::ImageData {
        width:  width  as usize,
        height: height as usize,
        bytes:  Cow::Borrowed(pixels),
    })?;

    // Give clipboard manager ~600 ms to snapshot the content
    std::thread::sleep(std::time::Duration::from_millis(600));
    Ok(())
}

// ─── Filepath text fallback ───────────────────────────────────────────────

/// ✅ Fix #4: xsel IS suitable for plain text.
/// Copy the saved filepath as text — last resort so the user knows
/// where the file is even if image clipboard failed entirely.
fn copy_filepath_as_text(filepath: &str) -> Result<(), Box<dyn Error>> {
    // Try xclip text mode first
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(filepath.as_bytes());
        }
        let _ = c.wait();
        return Ok(());
    }

    // Try xsel for text (this IS valid — xsel handles text fine)
    child = Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if let Ok(mut c) = child {
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(filepath.as_bytes());
        }
        let _ = c.wait();
        return Ok(());
    }

    Err("No clipboard tool available for text fallback".into())
}
