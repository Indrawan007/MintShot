//! Main capture orchestration module
//!
//! Coordinates the screenshot workflow:
//! 1. Capture full screen to buffer (fast XGetImage)
//! 2. Show translucent overlay
//! 3. Let user draw selection rectangle
//! 4. Crop selected region
//! 5. Save to file and clipboard

use log::info;
use std::error::Error;

use crate::clipboard;
use crate::overlay;
use crate::save;
use crate::selection::SelectionRect;

/// Take a partial screenshot - main entry point
///
/// Returns the path to the saved screenshot file
pub fn take_partial_screenshot() -> Result<String, Box<dyn Error>> {
    // Step 1: Get screen dimensions and capture full screen
    let (display, screen_width, screen_height, root) = unsafe {
        let display = x11::xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err("Cannot open X display".into());
        }
        let screen = x11::xlib::XDefaultScreen(display);
        let width = x11::xlib::XDisplayWidth(display, screen) as u32;
        let height = x11::xlib::XDisplayHeight(display, screen) as u32;
        let root = x11::xlib::XRootWindow(display, screen);
        (display, width, height, root)
    };

    info!(
        "Screen dimensions: {}x{}",
        screen_width, screen_height
    );

    // Step 2: Show overlay and get user selection
    let selection = overlay::show_selection_overlay(display, root, screen_width, screen_height)?;

    info!(
        "Selection: {}x{} at ({}, {})",
        selection.width, selection.height, selection.x, selection.y
    );

    if selection.width < 2 || selection.height < 2 {
        unsafe { x11::xlib::XCloseDisplay(display); }
        return Err("Selection too small, cancelled".into());
    }

    // Step 3: Capture the selected region from root window
    let pixels = capture_region(display, root, &selection)?;

    // Step 4: Close display - we're done with X11
    unsafe { x11::xlib::XCloseDisplay(display); }

    // Step 5: Encode to PNG and save
    let filepath = save::save_png(&pixels, selection.width, selection.height)?;

    // Step 6: Copy to clipboard
    match clipboard::copy_to_clipboard(&filepath) {
        Ok(()) => info!("Screenshot copied to clipboard"),
        Err(e) => info!("Clipboard copy failed (non-fatal): {}", e),
    }

    Ok(filepath)
}

/// Capture a specific region from the root window
///
/// Uses XGetImage which is the fastest method for X11 screen capture
fn capture_region(
    display: *mut x11::xlib::Display,
    root: x11::xlib::Window,
    selection: &SelectionRect,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    unsafe {
        let image = x11::xlib::XGetImage(
            display,
            root,
            selection.x as i32,
            selection.y as i32,
            selection.width,
            selection.height,
            x11::xlib::XAllPlanes(),
            x11::xlib::ZPixmap,
        );

        if image.is_null() {
            return Err("XGetImage failed".into());
        }

        let img = &*image;
        let total_pixels = (selection.width * selection.height) as usize;
        let mut pixels = Vec::with_capacity(total_pixels * 4); // RGBA

        // Convert BGRA (X11 native) to RGBA (PNG standard)
        // Direct pointer arithmetic for maximum performance
        let data = img.data as *const u8;
        let bytes_per_line = img.bytes_per_line as usize;
        let bits_per_pixel = img.bits_per_pixel as usize;
        let bytes_per_pixel = bits_per_pixel / 8;

        for y in 0..selection.height as usize {
            let row_offset = y * bytes_per_line;
            for x in 0..selection.width as usize {
                let pixel_offset = row_offset + x * bytes_per_pixel;

                let b = *data.add(pixel_offset);
                let g = *data.add(pixel_offset + 1);
                let r = *data.add(pixel_offset + 2);
                let a = if bytes_per_pixel == 4 {
                    *data.add(pixel_offset + 3)
                } else {
                    255
                };

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
