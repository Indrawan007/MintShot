//! Main capture orchestration module
//!
//! Flow:
//! 1. Show overlay → captures screen BEFORE overlay appears
//! 2. User selects region
//! 3. Pixels are cropped from pre-overlay image (guaranteed clean)
//! 4. Save PNG + copy to clipboard + notification

use log::{info, warn};
use std::error::Error;

use crate::clipboard;
use crate::overlay;
use crate::save;

/// Take a partial screenshot — main entry point
pub fn take_partial_screenshot() -> Result<String, Box<dyn Error>> {
    // Step 1: Open display
    let (display, screen_width, screen_height, root) = unsafe {
        let display = x11::xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err("Cannot open X display".into());
        }
        let screen = x11::xlib::XDefaultScreen(display);
        let width  = x11::xlib::XDisplayWidth(display, screen) as u32;
        let height = x11::xlib::XDisplayHeight(display, screen) as u32;
        let root   = x11::xlib::XRootWindow(display, screen);
        (display, width, height, root)
    };

    info!("Screen: {}x{}", screen_width, screen_height);

    // Step 2: Show overlay → returns BOTH selection AND clean pixels
    // The overlay captures the screen BEFORE showing itself, then crops
    // from that clean capture. No overlay artifacts possible.
    let capture_result = overlay::show_selection_overlay(
        display, root, screen_width, screen_height,
    )?;

    let sel = &capture_result.selection;

    info!(
        "Selection: {}x{} at ({}, {})",
        sel.width, sel.height, sel.x, sel.y
    );

    // Step 3: Close display — done with X11
    unsafe { x11::xlib::XCloseDisplay(display); }

    // Step 4: Save PNG from the clean pixels
    let filepath = save::save_png(
        &capture_result.pixels,
        sel.width,
        sel.height,
    )?;
    info!("Saved: {}", filepath);

    // Step 5: Auto-copy to clipboard
    let clipboard_ok = match clipboard::copy_to_clipboard(&filepath) {
        Ok(()) => {
            info!("Copied to clipboard — ready to paste!");
            true
        }
        Err(e) => {
            warn!("Clipboard failed: {} (file still saved)", e);
            false
        }
    };

    // Step 6: Desktop notification
    send_notification(&filepath, sel.width, sel.height, clipboard_ok);

    Ok(filepath)
}

/// Desktop notification with result
fn send_notification(filepath: &str, w: u32, h: u32, clipboard_ok: bool) {
    let status = if clipboard_ok {
        "📋 Copied to clipboard — Ctrl+V to paste!"
    } else {
        "⚠ Clipboard unavailable — file saved"
    };

    let body = format!("{}×{} px\n{}\n{}", w, h, status, filepath);

    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=MintShot")
        .arg("--icon=accessories-screenshot")
        .arg("--urgency=low")
        .arg("--expire-time=4000")
        .arg("MintShot — Screenshot Captured ✓")
        .arg(&body)
        .spawn();
}
