//! Main capture orchestration module
//!
//! 1. Show overlay for region selection
//! 2. Capture selected region (XGetImage)
//! 3. Save to PNG file
//! 4. Auto-copy to clipboard (ready to paste immediately)
//! 5. Desktop notification with result

use log::{info, warn};
use std::error::Error;

use crate::clipboard;
use crate::overlay;
use crate::save;
use crate::selection::SelectionRect;

/// Take a partial screenshot — main entry point
pub fn take_partial_screenshot() -> Result<String, Box<dyn Error>> {
    // Step 1: Open display and get screen info
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

    info!("Screen dimensions: {}x{}", screen_width, screen_height);

    // Step 2: Show overlay and get user selection
    let selection = overlay::show_selection_overlay(
        display, root, screen_width, screen_height,
    )?;

    info!(
        "Selection: {}x{} at ({}, {})",
        selection.width, selection.height, selection.x, selection.y
    );

    if selection.width < 2 || selection.height < 2 {
        unsafe { x11::xlib::XCloseDisplay(display); }
        return Err("Selection too small, cancelled".into());
    }

    // Step 3: Capture the selected region
    let pixels = capture_region(display, root, &selection)?;

    // Step 4: Close X display — done with screen access
    unsafe { x11::xlib::XCloseDisplay(display); }

    // Step 5: Save PNG file
    let filepath = save::save_png(&pixels, selection.width, selection.height)?;
    info!("Saved: {}", filepath);

    // Step 6: Auto-copy to clipboard (immediately ready to Ctrl+V)
    let clipboard_ok = match clipboard::copy_to_clipboard(&filepath) {
        Ok(()) => {
            info!("Screenshot copied to clipboard — ready to paste!");
            true
        }
        Err(e) => {
            warn!("Clipboard copy failed: {} (screenshot still saved to file)", e);
            false
        }
    };

    // Step 7: Desktop notification
    send_notification(&filepath, &selection, clipboard_ok);

    Ok(filepath)
}

/// Capture a specific region from the root window
fn capture_region(
    display: *mut x11::xlib::Display,
    root: x11::xlib::Window,
    sel: &SelectionRect,
) -> Result<Vec<u8>, Box<dyn Error>> {
    unsafe {
        let image = x11::xlib::XGetImage(
            display, root,
            sel.x as i32, sel.y as i32,
            sel.width, sel.height,
            x11::xlib::XAllPlanes(),
            x11::xlib::ZPixmap,
        );

        if image.is_null() {
            return Err("XGetImage failed".into());
        }

        let img       = &*image;
        let total_px  = (sel.width * sel.height) as usize;
        let mut pixels = Vec::with_capacity(total_px * 4);

        let data = img.data as *const u8;
        let bpl  = img.bytes_per_line as usize;
        let bpp  = (img.bits_per_pixel / 8) as usize;

        for y in 0..sel.height as usize {
            let row = y * bpl;
            for x in 0..sel.width as usize {
                let off = row + x * bpp;
                let b = *data.add(off);
                let g = *data.add(off + 1);
                let r = *data.add(off + 2);
                let a = if bpp == 4 { *data.add(off + 3) } else { 255 };
                pixels.push(r);
                pixels.push(g);
                pixels.push(b);
                pixels.push(a);
            }
        }

        x11::xlib::XDestroyImage(image);
        Ok(pixels)
    }
}

/// Send desktop notification with capture result
fn send_notification(filepath: &str, sel: &SelectionRect, clipboard_ok: bool) {
    let clipboard_status = if clipboard_ok {
        "📋 Copied to clipboard — ready to paste!"
    } else {
        "⚠ Clipboard unavailable — file saved only"
    };

    let body = format!(
        "{}×{} px\n{}\n{}",
        sel.width, sel.height,
        clipboard_status,
        filepath,
    );

    match std::process::Command::new("notify-send")
        .arg("--app-name=MintShot")
        .arg("--icon=accessories-screenshot")
        .arg("--urgency=low")
        .arg("--expire-time=4000")
        .arg("MintShot — Screenshot Captured! ✓")
        .arg(&body)
        .spawn()
    {
        Ok(_)  => info!("Notification sent"),
        Err(e) => info!("notify-send unavailable (non-fatal): {}", e),
    }
}
