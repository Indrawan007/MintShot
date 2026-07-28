//! Fullscreen translucent overlay for region selection
//!
//! UX Features:
//! - Smooth dim overlay (~30% opacity simulation)
//! - Help text instructions on screen
//! - Real-time mouse coordinates
//! - Crosshair guides that persist during drag
//! - Selection with thick border + corner handles
//! - Dimension + position info panel
//! - Edge guide lines during selection
//! - ESC / Right-click to cancel
//! - Left click + drag to select, release to confirm

use log::info;
use std::error::Error;
use std::ptr;
use x11::keysym;
use x11::xlib;

use crate::selection::SelectionRect;

// ─── Visual constants ─────────────────────────────────────────────────────────

const SEL_BORDER_COLOR: u64  = 0x00_CC_66;
const SEL_BORDER_SHADOW: u64 = 0x00_00_00;
const CORNER_COLOR: u64      = 0xFF_FF_FF;
const CORNER_SIZE: i32       = 8;
const BORDER_WIDTH: u32      = 2;
const GUIDE_COLOR: u64       = 0xFF_FF_FF;
const EDGE_GUIDE_COLOR: u64  = 0x80_80_80;
const PANEL_BG: u64          = 0x1A_1A_2E;
const PANEL_TEXT: u64         = 0xFF_FF_FF;
const PANEL_ACCENT: u64      = 0x00_CC_66;
const PANEL_DIM_TEXT: u64    = 0xAA_AA_AA;
const HELP_BG: u64           = 0x16_16_28;
const HELP_TEXT: u64         = 0xCC_CC_CC;
const HELP_KEY_BG: u64      = 0x33_33_55;
const HELP_KEY_TEXT: u64     = 0xFF_FF_FF;
const XC_CROSSHAIR: u32     = 34;

// ─── Drag state ───────────────────────────────────────────────────────────────

struct DragState {
    active:  bool,
    start_x: i32,
    start_y: i32,
}

impl DragState {
    fn new() -> Self {
        Self { active: false, start_x: 0, start_y: 0 }
    }

    fn begin(&mut self, x: i32, y: i32) {
        self.active  = true;
        self.start_x = x;
        self.start_y = y;
    }

    fn to_selection(&self, end_x: i32, end_y: i32) -> SelectionRect {
        SelectionRect::from_points(self.start_x, self.start_y, end_x, end_y)
    }
}

// ─── Cursor position ──────────────────────────────────────────────────────────

struct CursorPos {
    x: i32,
    y: i32,
}

// ─── Result from overlay ──────────────────────────────────────────────────────

/// Contains both the selection rectangle AND the raw screen pixel data
/// captured BEFORE the overlay was shown.
pub struct CaptureResult {
    pub selection: SelectionRect,
    pub pixels: Vec<u8>,   // RGBA pixels of selected region
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Show overlay, get selection, AND return cropped pixels from the
/// pre-overlay screenshot. This guarantees clean pixels without
/// any overlay artifacts.
pub fn show_selection_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    screen_width: u32,
    screen_height: u32,
) -> Result<CaptureResult, Box<dyn Error>> {
    unsafe { run_overlay(display, root, screen_width, screen_height) }
}

// ─── Main overlay logic ──────────────────────────────────────────────────────

unsafe fn run_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    sw: u32,
    sh: u32,
) -> Result<CaptureResult, Box<dyn Error>> {

    let screen   = xlib::XDefaultScreen(display);
    let visual   = xlib::XDefaultVisual(display, screen);
    let depth    = xlib::XDefaultDepth(display, screen);
    let colormap = xlib::XDefaultColormap(display, screen);

    // ─── Pre-compute keycodes for reliable key detection ──────────────────
    let escape_keycode = xlib::XKeysymToKeycode(display, keysym::XK_Escape as u64);
    let q_keycode      = xlib::XKeysymToKeycode(display, keysym::XK_q as u64);
    let return_keycode = xlib::XKeysymToKeycode(display, keysym::XK_Return as u64);

    info!("Key mappings: ESC={}, Q={}, Enter={}",
          escape_keycode, q_keycode, return_keycode);

    // ─── Capture screen BEFORE overlay ─────────────────────────────────────
    let bg = xlib::XGetImage(
        display, root, 0, 0, sw, sh,
        xlib::XAllPlanes(), xlib::ZPixmap,
    );
    if bg.is_null() {
        return Err("Failed to capture background".into());
    }

    // ─── Create overlay window ─────────────────────────────────────────────
    let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
    attrs.override_redirect = xlib::True;
    attrs.event_mask        = xlib::ExposureMask
        | xlib::ButtonPressMask
        | xlib::ButtonReleaseMask
        | xlib::PointerMotionMask
        | xlib::KeyPressMask
        | xlib::KeyReleaseMask
        | xlib::FocusChangeMask;
    attrs.colormap          = colormap;
    attrs.background_pixel  = 0;

    let win = xlib::XCreateWindow(
        display, root, 0, 0, sw, sh, 0, depth,
        xlib::InputOutput as u32, visual,
        xlib::CWOverrideRedirect | xlib::CWEventMask
            | xlib::CWColormap | xlib::CWBackPixel,
        &mut attrs,
    );

    // Explicitly select KeyPress events (redundant but safe)
    xlib::XSelectInput(
        display, win,
        xlib::ExposureMask
            | xlib::ButtonPressMask
            | xlib::ButtonReleaseMask
            | xlib::PointerMotionMask
            | xlib::KeyPressMask
            | xlib::KeyReleaseMask
            | xlib::FocusChangeMask,
    );

    xlib::XMapRaised(display, win);
    xlib::XSync(display, xlib::False);

    // ─── Grab pointer with retry ───────────────────────────────────────────
    let cursor_font = xlib::XCreateFontCursor(display, XC_CROSSHAIR);

    let mut grab_attempts = 0;
    loop {
        let result = xlib::XGrabPointer(
            display, win, xlib::True,
            (xlib::ButtonPressMask | xlib::ButtonReleaseMask
                | xlib::PointerMotionMask) as u32,
            xlib::GrabModeAsync, xlib::GrabModeAsync,
            win, cursor_font, xlib::CurrentTime,
        );

        if result == xlib::GrabSuccess {
            info!("Pointer grabbed successfully");
            break;
        }

        grab_attempts += 1;
        if grab_attempts > 20 {
            info!("Warning: XGrabPointer failed after 20 attempts (code {})", result);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // ─── Grab keyboard with retry (CRITICAL for ESC) ───────────────────────
    grab_attempts = 0;
    loop {
        let result = xlib::XGrabKeyboard(
            display, win, xlib::True,
            xlib::GrabModeAsync, xlib::GrabModeAsync,
            xlib::CurrentTime,
        );

        if result == xlib::GrabSuccess {
            info!("Keyboard grabbed successfully");
            break;
        }

        grab_attempts += 1;
        if grab_attempts > 20 {
            info!("Warning: XGrabKeyboard failed after 20 attempts (ESC may not work)");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // Force input focus to our window
    xlib::XSetInputFocus(display, win, xlib::RevertToParent, xlib::CurrentTime);
    xlib::XSync(display, xlib::False);

    // ─── Create GC & pixmap buffer ─────────────────────────────────────────
    let gc  = xlib::XCreateGC(display, win, 0, ptr::null_mut());
    let buf = xlib::XCreatePixmap(display, win, sw, sh, depth as u32);

    let font_name = std::ffi::CString::new(
        "-*-fixed-bold-r-*-*-13-*-*-*-*-*-iso8859-1"
    ).unwrap();
    let font = xlib::XLoadQueryFont(display, font_name.as_ptr());
    if !font.is_null() {
        xlib::XSetFont(display, gc, (*font).fid);
    }

    // Initial draw
    full_redraw(display, buf, gc, bg, sw, sh, None, None);
    blit(display, buf, win, gc, sw, sh);
    xlib::XFlush(display);

    // ─── Event loop ───────────────────────────────────────────────────────
    let mut event: xlib::XEvent              = std::mem::zeroed();
    let mut drag                              = DragState::new();
    let mut cursor: Option<CursorPos>         = None;
    let mut result_sel: Option<SelectionRect> = None;

    'event_loop: loop {
        xlib::XNextEvent(display, &mut event);

        match event.get_type() {
            // ── Expose ────────────────────────────────────────────────────
            xlib::Expose => {
                blit(display, buf, win, gc, sw, sh);
            }

            // ── Focus events (log for debug) ──────────────────────────────
            xlib::FocusIn => {
                info!("Window gained focus");
            }
            xlib::FocusOut => {
                info!("Window lost focus — re-grabbing keyboard");
                // Re-grab keyboard if focus is lost
                xlib::XGrabKeyboard(
                    display, win, xlib::True,
                    xlib::GrabModeAsync, xlib::GrabModeAsync,
                    xlib::CurrentTime,
                );
                xlib::XSetInputFocus(display, win, xlib::RevertToParent, xlib::CurrentTime);
            }

            // ── Button Press ──────────────────────────────────────────────
            xlib::ButtonPress => {
                let btn = event.button;
                match btn.button {
                    1 => drag.begin(btn.x, btn.y),
                    3 => {
                        info!("Right click — cancelling");
                        break 'event_loop;
                    }
                    _ => {}
                }
            }

            // ── Motion Notify ─────────────────────────────────────────────
            xlib::MotionNotify => {
                let mut mx = event.motion.x;
                let mut my = event.motion.y;
                while xlib::XCheckMaskEvent(
                    display, xlib::PointerMotionMask, &mut event,
                ) != 0 {
                    mx = event.motion.x;
                    my = event.motion.y;
                }

                match cursor.as_mut() {
                    Some(c) => { c.x = mx; c.y = my; }
                    None    => cursor = Some(CursorPos { x: mx, y: my }),
                }

                let sel = drag.active.then(|| drag.to_selection(mx, my));

                full_redraw(display, buf, gc, bg, sw, sh,
                            sel.as_ref(), cursor.as_ref());
                blit(display, buf, win, gc, sw, sh);
                xlib::XFlush(display);
            }

            // ── Button Release ────────────────────────────────────────────
            xlib::ButtonRelease => {
                let btn = event.button;
                if btn.button == 1 && drag.active {
                    let sel = drag.to_selection(btn.x, btn.y);
                    if sel.is_valid() {
                        result_sel = Some(sel);
                    }
                    break 'event_loop;
                }
            }

            // ── Key Press (ROBUST HANDLING) ───────────────────────────────
            xlib::KeyPress => {
                let key_event = event.key;
                let keycode = key_event.keycode;

                info!("KeyPress: keycode={}", keycode);

                // Method 1: Check by keycode (most reliable)
                if keycode == escape_keycode as u32 || keycode == q_keycode as u32 {
                    info!("Cancel key detected (keycode match)");
                    break 'event_loop;
                }

                // Method 2: Check via keysym (backup)
                let sym = xlib::XLookupKeysym(&mut event.key, 0);
                if sym == keysym::XK_Escape as u64
                    || sym == keysym::XK_q as u64
                    || sym == keysym::XK_Q as u64
                {
                    info!("Cancel key detected (keysym match): 0x{:x}", sym);
                    break 'event_loop;
                }

                // Enter to confirm active selection
                if drag.active
                    && (keycode == return_keycode as u32
                        || sym == keysym::XK_Return as u64)
                {
                    if let Some(c) = cursor.as_ref() {
                        let sel = drag.to_selection(c.x, c.y);
                        if sel.is_valid() {
                            info!("Selection confirmed via Enter");
                            result_sel = Some(sel);
                            break 'event_loop;
                        }
                    }
                }
            }

            _ => {}
        }
    }

    // Ensure X11 is fully synced
    xlib::XSync(display, xlib::False);

    // ─── Extract clean pixels ─────────────────────────────────────────────
    let result: Option<CaptureResult> = match result_sel {
        Some(sel) => {
            let pixels = extract_region_from_ximage(bg, &sel);
            info!("Extracted {} bytes for {}x{} region",
                  pixels.len(), sel.width, sel.height);
            Some(CaptureResult { selection: sel, pixels })
        }
        None => None,
    };

    // ─── Cleanup ───────────────────────────────────────────────────────────
    xlib::XUngrabPointer(display, xlib::CurrentTime);
    xlib::XUngrabKeyboard(display, xlib::CurrentTime);
    xlib::XFreeCursor(display, cursor_font);
    if !font.is_null() { xlib::XFreeFont(display, font); }
    xlib::XFreeGC(display, gc);
    xlib::XFreePixmap(display, buf);
    xlib::XDestroyImage(bg);
    xlib::XDestroyWindow(display, win);
    xlib::XFlush(display);

    result.ok_or_else(|| "Selection cancelled".into())
}

// ─── Extract clean pixels from XImage ─────────────────────────────────────────

/// Crop a region from the pre-captured XImage and convert to RGBA.
///
/// Handles X11's various pixel formats:
///   - 32-bit BGRA (most common on modern X11)
///   - 32-bit BGRX (alpha byte is garbage/zero → force to 255)
///   - 24-bit BGR
///
/// Uses the XImage's red/green/blue masks to correctly extract channels
/// regardless of the display's byte order or pixel format.
unsafe fn extract_region_from_ximage(
    image: *mut xlib::XImage,
    sel: &SelectionRect,
) -> Vec<u8> {
    let img = &*image;
    let total_px = (sel.width * sel.height) as usize;
    let mut pixels = Vec::with_capacity(total_px * 4);

    let data = img.data as *const u8;
    let bytes_per_line = img.bytes_per_line as usize;
    let bpp = (img.bits_per_pixel / 8) as usize;

    // Get channel masks from XImage — these tell us where R/G/B bits are
    let red_mask   = img.red_mask   as u32;
    let green_mask = img.green_mask as u32;
    let blue_mask  = img.blue_mask  as u32;

    // Calculate bit shifts for each channel
    let red_shift   = mask_shift(red_mask);
    let green_shift = mask_shift(green_mask);
    let blue_shift  = mask_shift(blue_mask);

    log::info!(
        "XImage format: bpp={}, R=0x{:08X} (shift {}), G=0x{:08X} (shift {}), B=0x{:08X} (shift {})",
        img.bits_per_pixel, red_mask, red_shift,
        green_mask, green_shift, blue_mask, blue_shift
    );

    // Clamp selection to image bounds
    let img_w = img.width as u32;
    let img_h = img.height as u32;
    let sel_x = sel.x.min(img_w.saturating_sub(1));
    let sel_y = sel.y.min(img_h.saturating_sub(1));
    let sel_w = sel.width.min(img_w.saturating_sub(sel_x));
    let sel_h = sel.height.min(img_h.saturating_sub(sel_y));

    for y in 0..sel_h as usize {
        let src_y = sel_y as usize + y;
        let row = src_y * bytes_per_line;

        for x in 0..sel_w as usize {
            let src_x = sel_x as usize + x;
            let off = row + src_x * bpp;

            // Read pixel as u32 (little-endian on all common platforms)
            let pixel: u32 = if bpp == 4 {
                (*data.add(off) as u32)
                    | ((*data.add(off + 1) as u32) << 8)
                    | ((*data.add(off + 2) as u32) << 16)
                    | ((*data.add(off + 3) as u32) << 24)
            } else if bpp == 3 {
                (*data.add(off) as u32)
                    | ((*data.add(off + 1) as u32) << 8)
                    | ((*data.add(off + 2) as u32) << 16)
            } else {
                // Fallback: read as much as we can
                let mut p: u32 = 0;
                for i in 0..bpp.min(4) {
                    p |= (*data.add(off + i) as u32) << (i * 8);
                }
                p
            };

            // Extract channels using masks
            let r = ((pixel & red_mask)   >> red_shift)   as u8;
            let g = ((pixel & green_mask) >> green_shift) as u8;
            let b = ((pixel & blue_mask)  >> blue_shift)  as u8;

            // Always force alpha to 255 — root window has no alpha channel
            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
            pixels.push(255);
        }
    }

    pixels
}

/// Calculate the right-shift needed to move the mask's LSB to bit 0
fn mask_shift(mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let mut shift = 0u32;
    let mut m = mask;
    while m & 1 == 0 {
        shift += 1;
        m >>= 1;
    }
    shift
}

// ─── Full redraw ──────────────────────────────────────────────────────────────

unsafe fn full_redraw(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    bg: *mut xlib::XImage,
    sw: u32,
    sh: u32,
    selection: Option<&SelectionRect>,
    cursor: Option<&CursorPos>,
) {
    xlib::XPutImage(display, buf, gc, bg, 0, 0, 0, 0, sw, sh);
    draw_dim_overlay(display, buf, gc, sw, sh);

    match selection {
        Some(sel) if sel.is_valid() => {
            draw_selection(display, buf, gc, bg, sw, sh, sel, cursor);
        }
        _ => {
            if let Some(c) = cursor {
                draw_crosshair_guides(display, buf, gc, c, sw, sh);
                draw_coord_tooltip(display, buf, gc, c, sw, sh);
            }
        }
    }

    draw_help_bar(display, buf, gc, sw, selection.is_some());
}

// ─── Dim overlay ──────────────────────────────────────────────────────────────

unsafe fn draw_dim_overlay(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sw: u32,
    sh: u32,
) {
    let stipple_data: [u8; 4] = [
        0b1000_1000,
        0b0010_0010,
        0b1000_1000,
        0b0010_0010,
    ];

    let stipple = xlib::XCreateBitmapFromData(
        display, buf,
        stipple_data.as_ptr() as *const i8,
        8, 4,
    );

    xlib::XSetFillStyle(display, gc, xlib::FillStippled);
    xlib::XSetStipple(display, gc, stipple);
    xlib::XSetForeground(display, gc, 0x000000);
    xlib::XFillRectangle(display, buf, gc, 0, 0, sw, sh);
    xlib::XSetFillStyle(display, gc, xlib::FillSolid);
    xlib::XFreePixmap(display, stipple);
}

// ─── Selection drawing ───────────────────────────────────────────────────────

unsafe fn draw_selection(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    bg: *mut xlib::XImage,
    sw: u32,
    sh: u32,
    sel: &SelectionRect,
    cursor: Option<&CursorPos>,
) {
    let sx = sel.x as i32;
    let sy = sel.y as i32;

    // Edge guides
    xlib::XSetForeground(display, gc, EDGE_GUIDE_COLOR);
    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter);

    xlib::XDrawLine(display, buf, gc, sx, 0, sx, sy);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32, 0,
                    sx + sel.width as i32, sy);
    xlib::XDrawLine(display, buf, gc, sx, sy + sel.height as i32,
                    sx, sh as i32);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32,
                    sy + sel.height as i32,
                    sx + sel.width as i32, sh as i32);
    xlib::XDrawLine(display, buf, gc, 0, sy, sx, sy);
    xlib::XDrawLine(display, buf, gc, 0, sy + sel.height as i32,
                    sx, sy + sel.height as i32);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32, sy,
                    sw as i32, sy);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32,
                    sy + sel.height as i32,
                    sw as i32, sy + sel.height as i32);

    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);

    // Restore clear pixels from pre-overlay capture
    xlib::XPutImage(
        display, buf, gc, bg,
        sx, sy, sx, sy, sel.width, sel.height,
    );

    // Shadow border
    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    xlib::XSetLineAttributes(display, gc, BORDER_WIDTH + 2,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // Green border
    xlib::XSetForeground(display, gc, SEL_BORDER_COLOR);
    xlib::XSetLineAttributes(display, gc, BORDER_WIDTH,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // Corner handles
    draw_corner_handles(display, buf, gc, sel);

    // Crosshair inside selection during drag
    if let Some(c) = cursor {
        xlib::XSetForeground(display, gc, GUIDE_COLOR);
        xlib::XSetLineAttributes(display, gc, 1,
            xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter);
        xlib::XDrawLine(display, buf, gc, sx, c.y,
                        sx + sel.width as i32, c.y);
        xlib::XDrawLine(display, buf, gc, c.x, sy,
                        c.x, sy + sel.height as i32);
        xlib::XSetLineAttributes(display, gc, 1,
            xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    }

    // Info panel
    draw_info_panel(display, buf, gc, sel, sh);
}

// ─── Corner handles ──────────────────────────────────────────────────────────

unsafe fn draw_corner_handles(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sel: &SelectionRect,
) {
    let sx = sel.x as i32;
    let sy = sel.y as i32;
    let ex = sx + sel.width as i32;
    let ey = sy + sel.height as i32;
    let half = CORNER_SIZE / 2;

    let corners = [
        (sx - half, sy - half),
        (ex - half, sy - half),
        (sx - half, ey - half),
        (ex - half, ey - half),
        (sx + sel.width as i32 / 2 - half, sy - half),
        (sx + sel.width as i32 / 2 - half, ey - half),
        (sx - half, sy + sel.height as i32 / 2 - half),
        (ex - half, sy + sel.height as i32 / 2 - half),
    ];

    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(display, buf, gc,
            cx - 1, cy - 1,
            (CORNER_SIZE + 2) as u32, (CORNER_SIZE + 2) as u32);
    }

    xlib::XSetForeground(display, gc, CORNER_COLOR);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(display, buf, gc,
            *cx, *cy,
            CORNER_SIZE as u32, CORNER_SIZE as u32);
    }
}

// ─── Crosshair guides ────────────────────────────────────────────────────────

unsafe fn draw_crosshair_guides(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    c: &CursorPos,
    sw: u32,
    sh: u32,
) {
    xlib::XSetForeground(display, gc, GUIDE_COLOR);
    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter);
    xlib::XDrawLine(display, buf, gc, 0, c.y, sw as i32, c.y);
    xlib::XDrawLine(display, buf, gc, c.x, 0, c.x, sh as i32);
    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
}

// ─── Coordinate tooltip ──────────────────────────────────────────────────────

unsafe fn draw_coord_tooltip(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    c: &CursorPos,
    sw: u32,
    sh: u32,
) {
    let text = format!("{}, {}", c.x, c.y);
    let c_text = std::ffi::CString::new(text.as_str()).unwrap();
    let text_w = (text.len() as i32) * 7 + 12;
    let text_h: i32 = 20;
    let pad = 15;

    let tx = if c.x + pad + text_w < sw as i32 { c.x + pad } else { c.x - pad - text_w };
    let ty = if c.y + pad + text_h < sh as i32 { c.y + pad } else { c.y - pad - text_h };

    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);
    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(display, buf, gc, tx + 6, ty + 14,
                      c_text.as_ptr(), text.len() as i32);
}

// ─── Info panel ──────────────────────────────────────────────────────────────

unsafe fn draw_info_panel(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sel: &SelectionRect,
    sh: u32,
) {
    let panel_w: u32 = 280;
    let panel_h: u32 = 52;
    let margin: i32  = 8;

    let px = ((sel.x as i32 + sel.width as i32 / 2) - panel_w as i32 / 2)
        .max(margin);

    let py = if sel.y + sel.height + panel_h + 12 < sh {
        (sel.y + sel.height) as i32 + 10
    } else {
        sel.y as i32 - panel_h as i32 - 10
    };

    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px, py, panel_w, panel_h);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, px, py, panel_w, panel_h);

    let line1 = format!(" {}  x  {} px", sel.width, sel.height);
    let c_line1 = std::ffi::CString::new(line1.as_str()).unwrap();

    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 10, py + 8, 12, 12);
    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px + 13, py + 11, 6, 6);

    xlib::XSetForeground(display, gc, PANEL_TEXT);
    xlib::XDrawString(display, buf, gc, px + 26, py + 18,
                      c_line1.as_ptr(), line1.len() as i32);

    let line2 = format!(" Position: ({}, {})", sel.x, sel.y);
    let c_line2 = std::ffi::CString::new(line2.as_str()).unwrap();

    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 14, py + 31, 4, 4);

    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(display, buf, gc, px + 26, py + 36,
                      c_line2.as_ptr(), line2.len() as i32);
}

// ─── Help bar ────────────────────────────────────────────────────────────────

unsafe fn draw_help_bar(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sw: u32,
    has_selection: bool,
) {
    let bar_h: u32 = 28;

    xlib::XSetForeground(display, gc, HELP_BG);
    xlib::XFillRectangle(display, buf, gc, 0, 0, sw, bar_h);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawLine(display, buf, gc, 0, bar_h as i32, sw as i32, bar_h as i32);

    let app_name = "MintShot";
    let c_app = std::ffi::CString::new(app_name).unwrap();
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XDrawString(display, buf, gc, 12, 18,
                      c_app.as_ptr(), app_name.len() as i32);

    xlib::XSetForeground(display, gc, 0x44_44_66);
    xlib::XDrawLine(display, buf, gc, 80, 5, 80, bar_h as i32 - 5);

    // Updated shortcuts including ESC, Q, Enter
    let instructions: &[(&str, &str)] = if has_selection {
        &[
            ("Release", "Capture"),
            ("Enter", "Confirm"),
            ("ESC", "Cancel"),
            ("Q", "Cancel"),
        ]
    } else {
        &[
            ("Click+Drag", "Select area"),
            ("ESC", "Cancel"),
            ("Q", "Cancel"),
            ("Right Click", "Cancel"),
        ]
    };

    let mut offset_x: i32 = 92;

    for (key, desc) in instructions {
        let key_w = (key.len() as i32) * 7 + 10;
        xlib::XSetForeground(display, gc, HELP_KEY_BG);
        xlib::XFillRectangle(display, buf, gc,
            offset_x, 5, key_w as u32, 18);

        let c_key = std::ffi::CString::new(*key).unwrap();
        xlib::XSetForeground(display, gc, HELP_KEY_TEXT);
        xlib::XDrawString(display, buf, gc, offset_x + 5, 18,
                          c_key.as_ptr(), key.len() as i32);

        offset_x += key_w + 4;

        let c_desc = std::ffi::CString::new(*desc).unwrap();
        xlib::XSetForeground(display, gc, HELP_TEXT);
        xlib::XDrawString(display, buf, gc, offset_x, 18,
                          c_desc.as_ptr(), desc.len() as i32);

        offset_x += (desc.len() as i32) * 7 + 16;
    }
}

// ─── Blit helper ──────────────────────────────────────────────────────────────

#[inline]
unsafe fn blit(
    display: *mut xlib::Display,
    src: xlib::Pixmap,
    dst: xlib::Window,
    gc: xlib::GC,
    w: u32,
    h: u32,
) {
    xlib::XCopyArea(display, src, dst, gc, 0, 0, w, h, 0, 0);
}
