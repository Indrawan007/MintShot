//! Main capture orchestration module
//!
//! 1. Capture full screen (fast XGetImage)
//! 2. Show translucent overlay for region selection
//! 3. Crop selected region
//! 4. Save to file and clipboard
//! 5. Show desktop notification

use log::info;
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

    // Step 4: Close display
    unsafe { x11::xlib::XCloseDisplay(display); }

    // Step 5: Save PNG
    let filepath = save::save_png(&pixels, selection.width, selection.height)?;

    // Step 6: Copy to clipboard
    match clipboard::copy_to_clipboard(&filepath) {
        Ok(())  => info!("Screenshot copied to clipboard"),
        Err(e)  => info!("Clipboard copy failed (non-fatal): {}", e),
    }

    // Step 7: Desktop notification
    send_notification(&filepath, selection.width, selection.height);

    Ok(filepath)
}

/// Capture a specific region from the root window
fn capture_region(
    display: *mut x11::xlib::Display,
    root: x11::xlib::Window,
    selection: &SelectionRect,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    unsafe {
        let image = x11::xlib::XGetImage(
            display, root,
            selection.x as i32, selection.y as i32,
            selection.width, selection.height,
            x11::xlib::XAllPlanes(),
            x11::xlib::ZPixmap,
        );

        if image.is_null() {
            return Err("XGetImage failed".into());
        }

        let img = &*image;
        let total_pixels = (selection.width * selection.height) as usize;
        let mut pixels = Vec::with_capacity(total_pixels * 4);

        let data           = img.data as *const u8;
        let bytes_per_line = img.bytes_per_line as usize;
        let bpp            = (img.bits_per_pixel / 8) as usize;

        for y in 0..selection.height as usize {
            let row = y * bytes_per_line;
            for x in 0..selection.width as usize {
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

/// Send a desktop notification about the saved screenshot
fn send_notification(filepath: &str, width: u32, height: u32) {
    let summary = "MintShot - Screenshot Saved";
    let body = format!(
        "{}×{} px\n{}",
        width, height, filepath
    );

    // Use notify-send (available on all Linux Mint installations)
    match std::process::Command::new("notify-send")
        .arg("--app-name=MintShot")
        .arg("--icon=accessories-screenshot")
        .arg("--urgency=low")
        .arg("--expire-time=3000")
        .arg(summary)
        .arg(&body)
        .spawn()
    {
        Ok(_)  => info!("Notification sent"),
        Err(e) => info!("notify-send unavailable (non-fatal): {}", e),
    }
}
