//! Main capture orchestration module
//!
//! Flow:
//!   1. Open display (RAII — auto-closed)
//!   2. Show overlay → pre-overlay pixels + selection rect
//!   3. Save PNG → file path + png_bytes (encoded once)
//!   4. Copy png_bytes to clipboard (no re-encode)
//!   5. Desktop notification

use log::{info, warn};
use std::fmt;
use x11::xinerama;
use x11::xlib;

use crate::clipboard;
use crate::overlay;
use crate::save;
use crate::selection::{bounding_box, DesktopGeometry};

// ─── Typed error ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CaptureError {
    /// User pressed ESC / Q / Right-click — normal exit, not an error.
    Cancelled,

    /// XOpenDisplay returned null — DISPLAY not set or X server not running.
    DisplayNotFound(String),

    /// XGetImage failed — screen capture step failed.
    ScreenCaptureFailed(String),

    /// PNG encoding or file write failed.
    SaveFailed(String),

    /// Any other X11 or OS error.
    Other(String),
    // NOTE: Clipboard errors are intentionally NOT a CaptureError variant.
    // Clipboard failure is non-fatal — the file is already saved to disk.
    // Clipboard errors are logged as warnings inside take_partial_screenshot()
    // and do NOT abort the capture flow.
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "screenshot cancelled by user"),
            Self::DisplayNotFound(msg) => write!(f, "X display not available: {}", msg),
            Self::ScreenCaptureFailed(msg) => write!(f, "screen capture failed: {}", msg),
            Self::SaveFailed(msg) => write!(f, "failed to save PNG: {}", msg),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<Box<dyn std::error::Error>> for CaptureError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        let msg = e.to_string();
        if msg.contains("cancelled") || msg.contains("Selection cancelled") {
            Self::Cancelled
        } else if msg.contains("X display") || msg.contains("XOpenDisplay") {
            Self::DisplayNotFound(msg)
        } else if msg.contains("XGetImage") || msg.contains("capture background") {
            Self::ScreenCaptureFailed(msg)
        } else {
            Self::Other(msg)
        }
    }
}

// ─── RAII display wrapper ─────────────────────────────────────────────────────

/// Owns an X11 Display connection.
/// Automatically calls `XCloseDisplay` when dropped.
struct XDisplay(*mut xlib::Display);

impl XDisplay {
    fn open() -> Result<Self, CaptureError> {
        let d = unsafe { xlib::XOpenDisplay(std::ptr::null()) };
        if d.is_null() {
            let display_env = std::env::var("DISPLAY").unwrap_or_else(|_| "(unset)".into());
            Err(CaptureError::DisplayNotFound(format!(
                "XOpenDisplay failed. DISPLAY={}. Is the X server running?",
                display_env
            )))
        } else {
            Ok(Self(d))
        }
    }

    fn as_ptr(&self) -> *mut xlib::Display {
        self.0
    }

    /// Bounding box of every active Xinerama screen (Fix #21).
    ///
    /// `XDisplayWidth`/`XDisplayHeight` describe the *default screen* only.
    /// On a multi-monitor setup that meant the overlay covered a single
    /// monitor, and monitors placed left of or above the primary one — whose
    /// root coordinates are negative — were unreachable entirely.
    ///
    /// Falls back to the default screen when Xinerama is absent or inactive,
    /// which keeps single-head systems behaving exactly as before.
    fn desktop_geometry(&self) -> DesktopGeometry {
        let (default_w, default_h, root) = unsafe {
            let screen = xlib::XDefaultScreen(self.0);
            (
                xlib::XDisplayWidth(self.0, screen) as u32,
                xlib::XDisplayHeight(self.0, screen) as u32,
                xlib::XRootWindow(self.0, screen),
            )
        };

        let fallback = DesktopGeometry {
            x: 0,
            y: 0,
            width: default_w,
            height: default_h,
            root,
        };

        unsafe {
            let mut event_base: std::os::raw::c_int = 0;
            let mut error_base: std::os::raw::c_int = 0;
            if xinerama::XineramaQueryExtension(self.0, &mut event_base, &mut error_base) == 0 {
                info!("Xinerama extension not present — single-screen mode");
                return fallback;
            }
            if xinerama::XineramaIsActive(self.0) == 0 {
                info!("Xinerama not active — single-screen mode");
                return fallback;
            }

            let mut count: std::os::raw::c_int = 0;
            let infos = xinerama::XineramaQueryScreens(self.0, &mut count);
            if infos.is_null() || count <= 0 {
                warn!("XineramaQueryScreens returned nothing — single-screen mode");
                return fallback;
            }

            let mut screens = Vec::with_capacity(count as usize);
            for i in 0..count as isize {
                let s = &*infos.offset(i);
                let rect = (
                    s.x_org as i32,
                    s.y_org as i32,
                    s.width.max(0) as u32,
                    s.height.max(0) as u32,
                );
                info!(
                    "  screen {}: {}x{} at ({}, {})",
                    s.screen_number, rect.2, rect.3, rect.0, rect.1
                );
                screens.push(rect);
            }
            xlib::XFree(infos as *mut std::os::raw::c_void);

            let (x, y, w, h) = match bounding_box(&screens) {
                Some(b) => b,
                None => return fallback,
            };

            // A degenerate box would produce a 0-area XGetImage.
            if w == 0 || h == 0 {
                warn!("Xinerama bounding box is empty — single-screen mode");
                return fallback;
            }

            DesktopGeometry {
                x,
                y,
                width: w,
                height: h,
                root,
            }
        }
    }
}

impl Drop for XDisplay {
    fn drop(&mut self) {
        unsafe {
            xlib::XCloseDisplay(self.0);
        }
        info!("X display closed");
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Take a partial screenshot — main entry point.
///
/// Returns `Ok(filepath)` on success, or a typed `CaptureError`
/// so `main.rs` can handle each failure mode distinctly.
pub fn take_partial_screenshot() -> Result<String, CaptureError> {
    // Step 1: Open display (RAII — auto-closed at end of scope)
    let display = XDisplay::open()?;
    let geom = display.desktop_geometry();
    info!(
        "Desktop: {}x{} at ({}, {})",
        geom.width, geom.height, geom.x, geom.y
    );

    // Step 2: Show overlay — returns clean pixels + selection rect
    let capture_result =
        overlay::show_selection_overlay(display.as_ptr(), &geom).map_err(CaptureError::from)?;

    let sel = &capture_result.selection;
    info!(
        "Selection: {}x{} at ({}, {})",
        sel.width, sel.height, sel.x, sel.y
    );

    // Display auto-closes here (Drop) — after overlay is fully done
    drop(display);

    // Step 3: Save PNG — returns (filepath, png_bytes)
    // png_bytes encoded ONCE here, reused by clipboard
    let (filepath, png_bytes) = save::save_png(&capture_result.pixels, sel.width, sel.height)
        .map_err(|e| CaptureError::SaveFailed(e.to_string()))?;

    info!("Saved: {}", filepath);

    // Step 4: Copy to clipboard — reuse png_bytes (no re-encode)
    // Clipboard failure is non-fatal — file is already saved
    let clipboard_ok = match clipboard::copy_to_clipboard(
        &png_bytes,
        &capture_result.pixels,
        sel.width,
        sel.height,
        &filepath,
    ) {
        Ok(()) => {
            info!("Copied to clipboard — Ctrl+V ready!");
            true
        }
        Err(e) => {
            warn!("Clipboard failed: {} (file still saved)", e);
            false
        }
    };

    // Step 5: Desktop notification (non-blocking)
    send_notification(&filepath, sel.width, sel.height, clipboard_ok);

    Ok(filepath)
}

// ─── Desktop notification ─────────────────────────────────────────────────────

fn send_notification(filepath: &str, w: u32, h: u32, clipboard_ok: bool) {
    let status = if clipboard_ok {
        "📋 Copied to clipboard — Ctrl+V to paste!"
    } else {
        "⚠ Clipboard unavailable — file saved only"
    };

    let body = format!("{}×{} px\n{}\n{}", w, h, status, filepath);

    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name=MintShot",
            "--icon=accessories-screenshot",
            "--urgency=low",
            "--expire-time=4000",
            "MintShot — Screenshot Captured ✓",
            &body,
        ])
        .spawn();
}
