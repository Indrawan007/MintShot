//! Fullscreen translucent overlay for region selection
//!
//! UX Features:
//!   - Smooth dim overlay (8x8 stipple pattern)
//!   - Help text instructions on screen
//!   - Real-time mouse coordinates
//!   - Crosshair guides using XFillRectangle (no wide-line artifacts)
//!   - Selection with thick border + corner handles
//!   - Dimension + position info panel
//!   - Edge guide lines during selection
//!   - ESC / Right-click / Q to cancel
//!   - Left click + drag to select, release to confirm
//!   - Click without drag = ignored (user can try again)
//!   - Skip redundant redraws when cursor hasn't moved
//!
//! FIXES APPLIED:
//!   #1  — XDestroyImage after pixels copied (no use-after-free)
//!   #2  — RAII guard for all X11 resources (no leak on early return)
//!   #3  — Click without drag doesn't close overlay
//!   #6  — 8x8 stipple pattern for proper tiling
//!   #9  — XSetErrorHandler restored after use
//!   #12 — XTextWidth for accurate font measurement
//!   #16 — Info panel is ASCII-only: the loaded font is ISO8859-1, so the
//!         UTF-8 "×" reached the server as two latin-1 glyphs ("Ã—")
//!   #17 — Unsupported XImage formats (PseudoColor, sub-byte depth) are
//!         rejected with an error instead of silently yielding black pixels
//!   — Crosshair via XFillRectangle (no wide-line rendering artifacts)
//!   — Shadow + main line for high contrast on any background
//!   — Motion dedup: skip redraw when cursor position unchanged

use log::{info, warn};
use std::error::Error;
use std::ptr;
use x11::keysym;
use x11::xlib;

use crate::selection::{DesktopGeometry, SelectionRect};

// ─── Visual constants ──────────────────────────────────────────────────────

const SEL_BORDER_COLOR: u64 = 0x00_CC_66;
const SEL_BORDER_SHADOW: u64 = 0x00_00_00;
const CORNER_COLOR: u64 = 0xFF_FF_FF;
const CORNER_SIZE: i32 = 8;
const BORDER_WIDTH: u32 = 2;

const GUIDE_COLOR: u64 = 0xFF_FF_FF;
const GUIDE_SHADOW_COLOR: u64 = 0x00_00_00;
const GUIDE_WIDTH: u32 = 1;

const EDGE_GUIDE_COLOR: u64 = 0x80_80_80;
const EDGE_GUIDE_WIDTH: u32 = 1;

const PANEL_BG: u64 = 0x1A_1A_2E;
const PANEL_TEXT: u64 = 0xFF_FF_FF;
const PANEL_ACCENT: u64 = 0x00_CC_66;
const PANEL_DIM_TEXT: u64 = 0xAA_AA_AA;
const HELP_BG: u64 = 0x16_16_28;
const HELP_TEXT: u64 = 0xCC_CC_CC;
const HELP_KEY_BG: u64 = 0x33_33_55;
const HELP_KEY_TEXT: u64 = 0xFF_FF_FF;
const XC_CROSSHAIR: u32 = 34;

// ─── Font candidates ───────────────────────────────────────────────────────

const FONT_CANDIDATES: [&str; 3] = [
    "-*-fixed-bold-r-*-*-13-*-*-*-*-*-iso8859-1",
    "-*-*-*-r-*-*-13-*",
    "fixed",
];

// ─── Error handler ─────────────────────────────────────────────────────────

extern "C" fn ignore_x_error(_display: *mut xlib::Display, _error: *mut xlib::XErrorEvent) -> i32 {
    0
}

// ─── RAII Guard ────────────────────────────────────────────────────────────

struct XOverlayGuard {
    display: *mut xlib::Display,
    win: Option<xlib::Window>,
    gc: Option<xlib::GC>,
    cursor_font: Option<xlib::Cursor>,
    font: Option<*mut xlib::XFontStruct>,
    pixmaps: Vec<xlib::Pixmap>,
    bg_image: Option<*mut xlib::XImage>,
    pointer_grabbed: bool,
    keyboard_grabbed: bool,
}

impl XOverlayGuard {
    fn new(display: *mut xlib::Display) -> Self {
        Self {
            display,
            win: None,
            gc: None,
            cursor_font: None,
            font: None,
            pixmaps: Vec::with_capacity(3),
            bg_image: None,
            pointer_grabbed: false,
            keyboard_grabbed: false,
        }
    }
}

impl Drop for XOverlayGuard {
    fn drop(&mut self) {
        unsafe {
            if self.pointer_grabbed {
                xlib::XUngrabPointer(self.display, xlib::CurrentTime);
            }
            if self.keyboard_grabbed {
                xlib::XUngrabKeyboard(self.display, xlib::CurrentTime);
            }
            if let Some(c) = self.cursor_font {
                xlib::XFreeCursor(self.display, c);
            }
            if let Some(f) = self.font {
                if !f.is_null() {
                    xlib::XFreeFont(self.display, f);
                }
            }
            if let Some(gc) = self.gc {
                xlib::XFreeGC(self.display, gc);
            }
            for &pm in &self.pixmaps {
                xlib::XFreePixmap(self.display, pm);
            }
            if let Some(img) = self.bg_image {
                xlib::XDestroyImage(img);
            }
            if let Some(w) = self.win {
                xlib::XDestroyWindow(self.display, w);
            }
            xlib::XFlush(self.display);
        }
    }
}

// ─── Drag state ────────────────────────────────────────────────────────────

struct DragState {
    active: bool,
    start_x: i32,
    start_y: i32,
}

impl DragState {
    fn new() -> Self {
        Self {
            active: false,
            start_x: 0,
            start_y: 0,
        }
    }

    fn begin(&mut self, x: i32, y: i32) {
        self.active = true;
        self.start_x = x;
        self.start_y = y;
    }

    fn reset(&mut self) {
        self.active = false;
    }

    fn to_selection(&self, end_x: i32, end_y: i32) -> SelectionRect {
        SelectionRect::from_points(self.start_x, self.start_y, end_x, end_y)
    }
}

// ─── Cursor position ───────────────────────────────────────────────────────

struct CursorPos {
    x: i32,
    y: i32,
}

// ─── Public result type ────────────────────────────────────────────────────

pub struct CaptureResult {
    pub selection: SelectionRect,
    pub pixels: Vec<u8>,
}

// ─── Public entry point ────────────────────────────────────────────────────

/// The overlay spans the whole virtual desktop described by `geom`, so a
/// drag can start on one monitor and end on another.
pub fn show_selection_overlay(
    display: *mut xlib::Display,
    geom: &DesktopGeometry,
) -> Result<CaptureResult, Box<dyn Error>> {
    unsafe { run_overlay(display, geom) }
}

// ─── Rectangle-based line drawing helpers ──────────────────────────────────
// XDrawLine with width>1 causes rendering artifacts when mouse moves fast.
// XFillRectangle produces pixel-perfect axis-aligned lines every time.

/// Draw a horizontal line using a filled rectangle.
/// `y` is the center of the line. Thickness expands equally above/below.
#[inline]
unsafe fn fill_hline(
    display: *mut xlib::Display,
    dst: xlib::Drawable,
    gc: xlib::GC,
    x: i32,
    y: i32,
    w: u32,
    thickness: u32,
) {
    let t = thickness.max(1);
    let y0 = y - (t as i32 / 2);
    xlib::XFillRectangle(display, dst, gc, x, y0, w, t);
}

/// Draw a vertical line using a filled rectangle.
/// `x` is the center of the line. Thickness expands equally left/right.
#[inline]
unsafe fn fill_vline(
    display: *mut xlib::Display,
    dst: xlib::Drawable,
    gc: xlib::GC,
    x: i32,
    y: i32,
    h: u32,
    thickness: u32,
) {
    let t = thickness.max(1);
    let x0 = x - (t as i32 / 2);
    xlib::XFillRectangle(display, dst, gc, x0, y, t, h);
}

/// Set GC to solid line with given width (used for borders only).
#[inline]
unsafe fn set_solid_line(display: *mut xlib::Display, gc: xlib::GC, width: u32) {
    xlib::XSetLineAttributes(
        display,
        gc,
        width,
        xlib::LineSolid,
        xlib::CapButt,
        xlib::JoinMiter,
    );
}

// ─── Accurate font measurement ────────────────────────────────────────────

unsafe fn measure_text(font: *mut xlib::XFontStruct, text: &str) -> i32 {
    if font.is_null() || text.is_empty() {
        return text.len() as i32 * 7;
    }
    let c = std::ffi::CString::new(text).unwrap_or_default();
    xlib::XTextWidth(font, c.as_ptr(), text.len() as i32)
}

// ─── Main overlay logic ────────────────────────────────────────────────────

unsafe fn run_overlay(
    display: *mut xlib::Display,
    geom: &DesktopGeometry,
) -> Result<CaptureResult, Box<dyn Error>> {
    let root = geom.root;
    let sw = geom.width;
    let sh = geom.height;
    // Bounding-box origin in root coordinates. Negative when a monitor sits
    // left of / above the primary one.
    let ox = geom.x;
    let oy = geom.y;

    let screen = xlib::XDefaultScreen(display);
    let visual = xlib::XDefaultVisual(display, screen);
    let depth = xlib::XDefaultDepth(display, screen);
    let colormap = xlib::XDefaultColormap(display, screen);

    // ── Pre-compute keycodes ───────────────────────────────────────────────
    let escape_keycode = xlib::XKeysymToKeycode(display, keysym::XK_Escape as u64);
    let q_keycode = xlib::XKeysymToKeycode(display, keysym::XK_q as u64);
    let return_keycode = xlib::XKeysymToKeycode(display, keysym::XK_Return as u64);

    info!(
        "Key mappings — ESC: {}, Q: {}, Enter: {}",
        escape_keycode, q_keycode, return_keycode
    );

    // ── Capture screen BEFORE overlay ─────────────────────────────────────
    // Grab the whole virtual desktop, not just the default screen, so
    // monitors at negative root coordinates are included (Fix #21).
    let bg = xlib::XGetImage(
        display,
        root,
        ox,
        oy,
        sw,
        sh,
        xlib::XAllPlanes(),
        xlib::ZPixmap,
    );
    if bg.is_null() {
        return Err("XGetImage failed — cannot capture background".into());
    }

    // ── Install silent error handler (save previous) ──────────────────────
    let prev_handler = xlib::XSetErrorHandler(Some(ignore_x_error));

    // ── RAII guard — owns ALL resources ───────────────────────────────────
    let mut guard = XOverlayGuard::new(display);
    guard.bg_image = Some(bg);

    // ── Create overlay window ──────────────────────────────────────────────
    let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
    attrs.override_redirect = xlib::True;
    attrs.event_mask = xlib::ExposureMask
        | xlib::ButtonPressMask
        | xlib::ButtonReleaseMask
        | xlib::PointerMotionMask
        | xlib::KeyPressMask
        | xlib::KeyReleaseMask
        | xlib::FocusChangeMask;
    attrs.colormap = colormap;
    attrs.background_pixel = 0;

    let win = xlib::XCreateWindow(
        display,
        root,
        ox,
        oy,
        sw,
        sh,
        0,
        depth,
        xlib::InputOutput as u32,
        visual,
        xlib::CWOverrideRedirect | xlib::CWEventMask | xlib::CWColormap | xlib::CWBackPixel,
        &mut attrs,
    );
    guard.win = Some(win);

    xlib::XSelectInput(
        display,
        win,
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

    // ── Grab pointer with retry ────────────────────────────────────────────
    let cursor_font = xlib::XCreateFontCursor(display, XC_CROSSHAIR);
    guard.cursor_font = Some(cursor_font);

    let mut attempts = 0u32;
    loop {
        let r = xlib::XGrabPointer(
            display,
            win,
            xlib::True,
            (xlib::ButtonPressMask | xlib::ButtonReleaseMask | xlib::PointerMotionMask) as u32,
            xlib::GrabModeAsync,
            xlib::GrabModeAsync,
            win,
            cursor_font,
            xlib::CurrentTime,
        );
        if r == xlib::GrabSuccess {
            guard.pointer_grabbed = true;
            info!("Pointer grabbed");
            break;
        }
        attempts += 1;
        if attempts > 20 {
            xlib::XSetErrorHandler(prev_handler);
            return Err("Failed to grab pointer after 20 retries — \
                 another app may be holding a grab"
                .into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // ── Grab keyboard with retry ───────────────────────────────────────────
    attempts = 0;
    loop {
        let r = xlib::XGrabKeyboard(
            display,
            win,
            xlib::True,
            xlib::GrabModeAsync,
            xlib::GrabModeAsync,
            xlib::CurrentTime,
        );
        if r == xlib::GrabSuccess {
            guard.keyboard_grabbed = true;
            info!("Keyboard grabbed");
            break;
        }
        attempts += 1;
        if attempts > 20 {
            xlib::XSetErrorHandler(prev_handler);
            return Err("Failed to grab keyboard after 20 retries".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    xlib::XSetInputFocus(display, win, xlib::RevertToParent, xlib::CurrentTime);
    xlib::XSync(display, xlib::False);

    // ── Create GC + pixmap buffers ─────────────────────────────────────────
    let gc = xlib::XCreateGC(display, win, 0, ptr::null_mut());
    guard.gc = Some(gc);

    let buf = xlib::XCreatePixmap(display, win, sw, sh, depth as u32);
    let bg_clean = xlib::XCreatePixmap(display, win, sw, sh, depth as u32);
    let bg_dim = xlib::XCreatePixmap(display, win, sw, sh, depth as u32);
    guard.pixmaps.extend_from_slice(&[bg_clean, bg_dim, buf]);

    // ── Load font ──────────────────────────────────────────────────────────
    let mut font: *mut xlib::XFontStruct = ptr::null_mut();
    for name in FONT_CANDIDATES {
        let cname = std::ffi::CString::new(name).unwrap();
        let f = xlib::XLoadQueryFont(display, cname.as_ptr());
        if !f.is_null() {
            font = f;
            xlib::XSetFont(display, gc, (*font).fid);
            break;
        }
    }
    guard.font = Some(font);

    // ── Upload background pixmaps (once, server-side) ─────────────────────
    xlib::XPutImage(display, bg_clean, gc, bg, 0, 0, 0, 0, sw, sh);
    xlib::XCopyArea(display, bg_clean, bg_dim, gc, 0, 0, sw, sh, 0, 0);
    draw_dim_overlay(display, bg_dim, gc, sw, sh);
    xlib::XSync(display, xlib::False);

    // Initial draw
    full_redraw(display, buf, gc, bg_clean, bg_dim, sw, sh, font, None, None, (ox, oy));
    blit(display, buf, win, gc, sw, sh);
    xlib::XFlush(display);

    // ── Event loop ─────────────────────────────────────────────────────────
    let mut event: xlib::XEvent = std::mem::zeroed();
    let mut drag = DragState::new();
    let mut cursor: Option<CursorPos> = None;
    let mut result_sel: Option<SelectionRect> = None;

    'event_loop: loop {
        xlib::XNextEvent(display, &mut event);

        match event.get_type() {
            // ── Expose ─────────────────────────────────────────────────
            xlib::Expose => {
                blit(display, buf, win, gc, sw, sh);
            }

            // ── Focus ──────────────────────────────────────────────────
            xlib::FocusIn => {
                info!("Overlay gained focus");
            }
            xlib::FocusOut => {
                info!("Overlay lost focus — re-grabbing keyboard");
                xlib::XGrabKeyboard(
                    display,
                    win,
                    xlib::True,
                    xlib::GrabModeAsync,
                    xlib::GrabModeAsync,
                    xlib::CurrentTime,
                );
                xlib::XSetInputFocus(display, win, xlib::RevertToParent, xlib::CurrentTime);
            }

            // ── Button Press ───────────────────────────────────────────
            xlib::ButtonPress => {
                let btn = event.button;
                match btn.button {
                    1 => drag.begin(btn.x, btn.y),
                    3 => {
                        info!("Right-click — cancelling");
                        break 'event_loop;
                    }
                    _ => {}
                }
            }

            // ── Motion Notify ──────────────────────────────────────────
            xlib::MotionNotify => {
                // Drain motion queue — only latest position matters
                let mut mx = event.motion.x;
                let mut my = event.motion.y;
                while xlib::XCheckMaskEvent(display, xlib::PointerMotionMask, &mut event) != 0 {
                    mx = event.motion.x;
                    my = event.motion.y;
                }

                // Skip redraw if cursor hasn't actually moved
                if let Some(ref c) = cursor {
                    if c.x == mx && c.y == my {
                        continue;
                    }
                }

                match cursor.as_mut() {
                    Some(c) => {
                        c.x = mx;
                        c.y = my;
                    }
                    None => cursor = Some(CursorPos { x: mx, y: my }),
                }

                let sel = drag.active.then(|| drag.to_selection(mx, my));
                full_redraw(
                    display,
                    buf,
                    gc,
                    bg_clean,
                    bg_dim,
                    sw,
                    sh,
                    font,
                    sel.as_ref(),
                    cursor.as_ref(),
                    (ox, oy),
                );
                blit(display, buf, win, gc, sw, sh);
                xlib::XFlush(display);
            }

            // ── Button Release ─────────────────────────────────────────
            xlib::ButtonRelease => {
                let btn = event.button;
                if btn.button == 1 && drag.active {
                    let sel = drag.to_selection(btn.x, btn.y);
                    drag.reset();

                    if sel.is_valid() {
                        result_sel = Some(sel);
                        break 'event_loop;
                    } else {
                        warn!(
                            "Selection too small ({}x{}) — ignoring",
                            sel.width, sel.height
                        );
                        full_redraw(
                            display,
                            buf,
                            gc,
                            bg_clean,
                            bg_dim,
                            sw,
                            sh,
                            font,
                            None,
                            cursor.as_ref(),
                            (ox, oy),
                        );
                        blit(display, buf, win, gc, sw, sh);
                        xlib::XFlush(display);
                    }
                }
            }

            // ── Key Press ──────────────────────────────────────────────
            xlib::KeyPress => {
                let key_event = event.key;
                let keycode = key_event.keycode;

                log::debug!("KeyPress: keycode={}", keycode);

                // Keycode match (most reliable)
                if keycode == escape_keycode as u32 || keycode == q_keycode as u32 {
                    info!("Cancel key (keycode {})", keycode);
                    break 'event_loop;
                }

                // Keysym match (backup)
                let sym = xlib::XLookupKeysym(&mut event.key, 0);
                if sym == keysym::XK_Escape as u64
                    || sym == keysym::XK_q as u64
                    || sym == keysym::XK_Q as u64
                {
                    info!("Cancel key (keysym 0x{:x})", sym);
                    break 'event_loop;
                }

                // Enter to confirm active selection
                if drag.active
                    && (keycode == return_keycode as u32 || sym == keysym::XK_Return as u64)
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

    xlib::XSync(display, xlib::False);

    // ── Extract pixels BEFORE guard drops ──────────────────────────────────
    // Ok(None)    → user cancelled / selection degenerate
    // Ok(Some(_)) → pixels ready
    // Err(_)      → real failure (unsupported pixel format) — must not be
    //               reported as a cancellation, or exit code 0 hides it.
    let extracted: Result<Option<CaptureResult>, String> = match result_sel {
        None => Ok(None),
        Some(sel) => {
            let sel = sel.clamped_to(sw, sh);
            if !sel.is_valid() {
                Ok(None)
            } else {
                match extract_region_from_ximage(bg, &sel) {
                    Ok(pixels) => {
                        info!(
                            "Extracted {} bytes for {}x{} region",
                            pixels.len(),
                            sel.width,
                            sel.height
                        );
                        Ok(Some(CaptureResult {
                            selection: sel,
                            pixels,
                        }))
                    }
                    Err(e) => Err(e),
                }
            }
        }
    };

    // ── Restore previous X error handler ───────────────────────────────────
    xlib::XSetErrorHandler(prev_handler);

    // guard drops here → all X11 resources freed in correct order

    match extracted {
        Ok(Some(res)) => Ok(res),
        Ok(None) => Err("Selection cancelled".into()),
        Err(e) => Err(e.into()),
    }
}

// ─── Extract pixels from XImage ───────────────────────────────────────────

unsafe fn extract_region_from_ximage(
    image: *mut xlib::XImage,
    sel: &SelectionRect,
) -> Result<Vec<u8>, String> {
    let img = &*image;
    let data = img.data as *const u8;

    let bytes_per_line = img.bytes_per_line as usize;
    let bpp = (img.bits_per_pixel / 8) as usize;

    let red_mask = img.red_mask as u32;
    let green_mask = img.green_mask as u32;
    let blue_mask = img.blue_mask as u32;

    let red_shift = mask_shift(red_mask);
    let green_shift = mask_shift(green_mask);
    let blue_shift = mask_shift(blue_mask);

    log::info!(
        "XImage — bits_per_pixel={}, R=0x{:06X}(>>{}), G=0x{:06X}(>>{}), B=0x{:06X}(>>{})",
        img.bits_per_pixel,
        red_mask,
        red_shift,
        green_mask,
        green_shift,
        blue_mask,
        blue_shift,
    );

    // ── Reject formats the mask math below cannot decode (Fix #17) ────────
    // PseudoColor/StaticGray visuals report all-zero RGB masks, and a
    // sub-byte depth makes `bpp` zero. Both used to fall through to the
    // generic path and silently emit solid black pixels — the user got a
    // valid-looking PNG of nothing. Fail loudly instead.
    if bpp == 0 || red_mask == 0 || green_mask == 0 || blue_mask == 0 {
        return Err(format!(
            "XGetImage returned an unsupported pixel format — \
             bits_per_pixel={}, R=0x{:06X} G=0x{:06X} B=0x{:06X}. \
             A TrueColor/DirectColor visual is required.",
            img.bits_per_pixel, red_mask, green_mask, blue_mask
        ));
    }

    let img_w = img.width as u32;
    let img_h = img.height as u32;
    let sel_x = sel.x.min(img_w.saturating_sub(1));
    let sel_y = sel.y.min(img_h.saturating_sub(1));
    let sel_w = sel.width.min(img_w.saturating_sub(sel_x));
    let sel_h = sel.height.min(img_h.saturating_sub(sel_y));

    let mut pixels = Vec::with_capacity((sel_w * sel_h * 4) as usize);

    // ── Fast path: 32-bit BGRA ─────────────────────────────────────────
    if bpp == 4
        && red_mask == 0x00_FF_00_00
        && green_mask == 0x00_00_FF_00
        && blue_mask == 0x00_00_00_FF
    {
        for y in 0..sel_h as usize {
            let row = (sel_y as usize + y) * bytes_per_line;
            let src = data.add(row + sel_x as usize * 4);
            for x in 0..sel_w as usize {
                let p = src.add(x * 4);
                pixels.push(*p.add(2)); // R
                pixels.push(*p.add(1)); // G
                pixels.push(*p.add(0)); // B
                pixels.push(255); // A
            }
        }
        return Ok(pixels);
    }

    // ── Fast path: 24-bit BGR ──────────────────────────────────────────
    if bpp == 3
        && red_mask == 0x00_FF_00_00
        && green_mask == 0x00_00_FF_00
        && blue_mask == 0x00_00_00_FF
    {
        for y in 0..sel_h as usize {
            let row = (sel_y as usize + y) * bytes_per_line;
            let src = data.add(row + sel_x as usize * 3);
            for x in 0..sel_w as usize {
                let p = src.add(x * 3);
                pixels.push(*p.add(2)); // R
                pixels.push(*p.add(1)); // G
                pixels.push(*p.add(0)); // B
                pixels.push(255);
            }
        }
        return Ok(pixels);
    }

    // ── Generic path: mask-based extraction ────────────────────────────
    for y in 0..sel_h as usize {
        let row = (sel_y as usize + y) * bytes_per_line;
        for x in 0..sel_w as usize {
            let off = row + (sel_x as usize + x) * bpp;

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
                let mut p = 0u32;
                for i in 0..bpp.min(4) {
                    p |= (*data.add(off + i) as u32) << (i * 8);
                }
                p
            };

            pixels.push(((pixel & red_mask) >> red_shift) as u8);
            pixels.push(((pixel & green_mask) >> green_shift) as u8);
            pixels.push(((pixel & blue_mask) >> blue_shift) as u8);
            pixels.push(255);
        }
    }

    Ok(pixels)
}

fn mask_shift(mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let mut s = 0u32;
    let mut m = mask;
    while m & 1 == 0 {
        s += 1;
        m >>= 1;
    }
    s
}

// ─── Full redraw ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
unsafe fn full_redraw(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    bg_clean: xlib::Pixmap,
    bg_dim: xlib::Pixmap,
    sw: u32,
    sh: u32,
    font: *mut xlib::XFontStruct,
    selection: Option<&SelectionRect>,
    cursor: Option<&CursorPos>,
    origin: (i32, i32),
) {
    xlib::XCopyArea(display, bg_dim, buf, gc, 0, 0, sw, sh, 0, 0);

    match selection {
        Some(sel) if sel.is_valid() => {
            draw_selection(display, buf, gc, bg_clean, sw, sh, font, sel, cursor, origin);
        }
        _ => {
            if let Some(c) = cursor {
                draw_crosshair_guides(display, buf, gc, c, sw, sh);
                draw_coord_tooltip(display, buf, gc, font, c, sw, sh);
            }
        }
    }

    draw_help_bar(display, buf, gc, font, sw, selection.is_some());
}

// ─── Dim overlay (8x8 stipple) ────────────────────────────────────────────

unsafe fn draw_dim_overlay(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sw: u32,
    sh: u32,
) {
    let stipple_data: [u8; 8] = [
        0b1010_1010,
        0b0101_0101,
        0b1010_1010,
        0b0101_0101,
        0b1010_1010,
        0b0101_0101,
        0b1010_1010,
        0b0101_0101,
    ];

    let stipple =
        xlib::XCreateBitmapFromData(display, buf, stipple_data.as_ptr() as *const i8, 8, 8);

    xlib::XSetFillStyle(display, gc, xlib::FillStippled);
    xlib::XSetStipple(display, gc, stipple);
    xlib::XSetForeground(display, gc, 0x000000);
    xlib::XFillRectangle(display, buf, gc, 0, 0, sw, sh);
    xlib::XSetFillStyle(display, gc, xlib::FillSolid);
    xlib::XFreePixmap(display, stipple);
}

// ─── Crosshair guides (rectangle-based — no wide-line artifacts) ─────────

unsafe fn draw_crosshair_guides(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    c: &CursorPos,
    sw: u32,
    sh: u32,
) {
    // Shadow (offset +1, slightly thicker)
    xlib::XSetForeground(display, gc, GUIDE_SHADOW_COLOR);
    fill_hline(display, buf, gc, 0, c.y + 1, sw, GUIDE_WIDTH + 1);
    fill_vline(display, buf, gc, c.x + 1, 0, sh, GUIDE_WIDTH + 1);

    // Main crosshair
    xlib::XSetForeground(display, gc, GUIDE_COLOR);
    fill_hline(display, buf, gc, 0, c.y, sw, GUIDE_WIDTH);
    fill_vline(display, buf, gc, c.x, 0, sh, GUIDE_WIDTH);
}

// ─── Selection drawing ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
unsafe fn draw_selection(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    bg_clean: xlib::Pixmap,
    sw: u32,
    sh: u32,
    font: *mut xlib::XFontStruct,
    sel: &SelectionRect,
    cursor: Option<&CursorPos>,
    origin: (i32, i32),
) {
    let sx = sel.x as i32;
    let sy = sel.y as i32;
    let ex = sx + sel.width as i32;
    let ey = sy + sel.height as i32;
    let sw_i = sw as i32;
    let sh_i = sh as i32;

    // ── Edge guides (rectangle-based, 1px solid) ──────────────────────────
    xlib::XSetForeground(display, gc, EDGE_GUIDE_COLOR);

    // Vertical edges above/below selection
    fill_vline(display, buf, gc, sx, 0, sy.max(0) as u32, EDGE_GUIDE_WIDTH);
    fill_vline(display, buf, gc, ex, 0, sy.max(0) as u32, EDGE_GUIDE_WIDTH);
    fill_vline(
        display,
        buf,
        gc,
        sx,
        ey,
        (sh_i - ey).max(0) as u32,
        EDGE_GUIDE_WIDTH,
    );
    fill_vline(
        display,
        buf,
        gc,
        ex,
        ey,
        (sh_i - ey).max(0) as u32,
        EDGE_GUIDE_WIDTH,
    );

    // Horizontal edges left/right of selection
    fill_hline(display, buf, gc, 0, sy, sx.max(0) as u32, EDGE_GUIDE_WIDTH);
    fill_hline(display, buf, gc, 0, ey, sx.max(0) as u32, EDGE_GUIDE_WIDTH);
    fill_hline(
        display,
        buf,
        gc,
        ex,
        sy,
        (sw_i - ex).max(0) as u32,
        EDGE_GUIDE_WIDTH,
    );
    fill_hline(
        display,
        buf,
        gc,
        ex,
        ey,
        (sw_i - ex).max(0) as u32,
        EDGE_GUIDE_WIDTH,
    );

    // ── Restore clean background for selected region ──────────────────────
    xlib::XCopyArea(
        display, bg_clean, buf, gc, sx, sy, sel.width, sel.height, sx, sy,
    );

    // ── Shadow border ─────────────────────────────────────────────────────
    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    set_solid_line(display, gc, BORDER_WIDTH + 2);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // ── Green border ──────────────────────────────────────────────────────
    xlib::XSetForeground(display, gc, SEL_BORDER_COLOR);
    set_solid_line(display, gc, BORDER_WIDTH);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // Reset line width
    set_solid_line(display, gc, 1);

    // ── Corner handles ────────────────────────────────────────────────────
    draw_corner_handles(display, buf, gc, sel);

    // ── Crosshair inside selection during drag (rectangle-based) ──────────
    if let Some(c) = cursor {
        let inner_w = sel.width;
        let inner_h = sel.height;

        // Shadow
        xlib::XSetForeground(display, gc, GUIDE_SHADOW_COLOR);
        fill_hline(display, buf, gc, sx, c.y + 1, inner_w, GUIDE_WIDTH + 1);
        fill_vline(display, buf, gc, c.x + 1, sy, inner_h, GUIDE_WIDTH + 1);

        // Main line
        xlib::XSetForeground(display, gc, GUIDE_COLOR);
        fill_hline(display, buf, gc, sx, c.y, inner_w, GUIDE_WIDTH);
        fill_vline(display, buf, gc, c.x, sy, inner_h, GUIDE_WIDTH);
    }

    // ── Info panel ────────────────────────────────────────────────────────
    draw_info_panel(display, buf, gc, font, sel, sw, sh, origin);
}

// ─── Corner handles ───────────────────────────────────────────────────────

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
    let mid_x = sx + sel.width as i32 / 2;
    let mid_y = sy + sel.height as i32 / 2;

    let corners = [
        (sx - half, sy - half),    // top-left
        (ex - half, sy - half),    // top-right
        (sx - half, ey - half),    // bottom-left
        (ex - half, ey - half),    // bottom-right
        (mid_x - half, sy - half), // top-mid
        (mid_x - half, ey - half), // bottom-mid
        (sx - half, mid_y - half), // left-mid
        (ex - half, mid_y - half), // right-mid
    ];

    // Shadow
    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(
            display,
            buf,
            gc,
            cx - 1,
            cy - 1,
            (CORNER_SIZE + 2) as u32,
            (CORNER_SIZE + 2) as u32,
        );
    }

    // Fill
    xlib::XSetForeground(display, gc, CORNER_COLOR);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(
            display,
            buf,
            gc,
            *cx,
            *cy,
            CORNER_SIZE as u32,
            CORNER_SIZE as u32,
        );
    }
}

// ─── Coordinate tooltip ───────────────────────────────────────────────────

unsafe fn draw_coord_tooltip(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    font: *mut xlib::XFontStruct,
    c: &CursorPos,
    sw: u32,
    sh: u32,
) {
    let text = format!("{}, {}", c.x, c.y);
    let c_text = std::ffi::CString::new(text.as_str()).unwrap();
    let text_w = measure_text(font, &text) + 12;
    let text_h: i32 = 20;
    let pad: i32 = 15;

    let tx = if c.x + pad + text_w < sw as i32 {
        c.x + pad
    } else {
        c.x - pad - text_w
    };
    let ty = if c.y + pad + text_h < sh as i32 {
        c.y + pad
    } else {
        c.y - pad - text_h
    };

    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);
    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(
        display,
        buf,
        gc,
        tx + 6,
        ty + 14,
        c_text.as_ptr(),
        text.len() as i32,
    );
}

// ─── Info panel ───────────────────────────────────────────────────────────

unsafe fn draw_info_panel(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    font: *mut xlib::XFontStruct,
    sel: &SelectionRect,
    sw: u32,
    sh: u32,
    origin: (i32, i32),
) {
    // ASCII only (Fix #16): the loaded font is ISO8859-1, so any multi-byte
    // UTF-8 character reaches the server as separate latin-1 glyphs.
    let line1 = format!(" {}  x  {} px", sel.width, sel.height);
    // Report root coordinates, not overlay-relative ones, so the numbers
    // match xrandr on a multi-monitor desktop (Fix #21).
    let line2 = format!(
        " Position: ({}, {})",
        sel.x as i32 + origin.0,
        sel.y as i32 + origin.1
    );

    let content_w = measure_text(font, &line1).max(measure_text(font, &line2));
    let panel_w = (content_w + 50).max(240) as u32;
    let panel_h: u32 = 52;
    let margin: i32 = 8;

    // Center on the selection, clamped to both screen edges.
    let px = ((sel.x as i32 + sel.width as i32 / 2) - panel_w as i32 / 2)
        .max(margin)
        .min((sw as i32 - panel_w as i32 - margin).max(margin));
    // Prefer below the selection, else above — but never above y=0.
    let py = if sel.y + sel.height + panel_h + 12 < sh {
        (sel.y + sel.height) as i32 + 10
    } else {
        (sel.y as i32 - panel_h as i32 - 10).max(margin)
    };

    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px, py, panel_w, panel_h);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, px, py, panel_w, panel_h);

    // Dimension icon + text
    let c_line1 = std::ffi::CString::new(line1.as_str()).unwrap();
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 10, py + 8, 12, 12);
    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px + 13, py + 11, 6, 6);
    xlib::XSetForeground(display, gc, PANEL_TEXT);
    xlib::XDrawString(
        display,
        buf,
        gc,
        px + 26,
        py + 18,
        c_line1.as_ptr(),
        line1.len() as i32,
    );

    // Position icon + text
    let c_line2 = std::ffi::CString::new(line2.as_str()).unwrap();
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 14, py + 31, 4, 4);
    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(
        display,
        buf,
        gc,
        px + 26,
        py + 36,
        c_line2.as_ptr(),
        line2.len() as i32,
    );
}
// ─── Help bar ─────────────────────────────────────────────────────────────

unsafe fn draw_help_bar(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    font: *mut xlib::XFontStruct,
    sw: u32,
    has_selection: bool,
) {
    let bar_h: u32 = 28;

    xlib::XSetForeground(display, gc, HELP_BG);
    xlib::XFillRectangle(display, buf, gc, 0, 0, sw, bar_h);
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawLine(display, buf, gc, 0, bar_h as i32, sw as i32, bar_h as i32);

    // App name badge
    let app_name = "MintShot";
    let c_app = std::ffi::CString::new(app_name).unwrap();
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XDrawString(
        display,
        buf,
        gc,
        12,
        18,
        c_app.as_ptr(),
        app_name.len() as i32,
    );

    xlib::XSetForeground(display, gc, 0x44_44_66);
    xlib::XDrawLine(display, buf, gc, 80, 5, 80, bar_h as i32 - 5);

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
        let key_w = measure_text(font, key) + 10;

        xlib::XSetForeground(display, gc, HELP_KEY_BG);
        xlib::XFillRectangle(display, buf, gc, offset_x, 5, key_w as u32, 18);

        let c_key = std::ffi::CString::new(*key).unwrap();
        xlib::XSetForeground(display, gc, HELP_KEY_TEXT);
        xlib::XDrawString(
            display,
            buf,
            gc,
            offset_x + 5,
            18,
            c_key.as_ptr(),
            key.len() as i32,
        );

        offset_x += key_w + 4;

        let desc_w = measure_text(font, desc);
        let c_desc = std::ffi::CString::new(*desc).unwrap();
        xlib::XSetForeground(display, gc, HELP_TEXT);
        xlib::XDrawString(
            display,
            buf,
            gc,
            offset_x,
            18,
            c_desc.as_ptr(),
            desc.len() as i32,
        );

        offset_x += desc_w + 16;
    }
}

// ─── Blit helper ──────────────────────────────────────────────────────────

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

// ─── Tests ────────────────────────────────────────────────────────────────
// `mask_shift` is pure integer math — no X connection required.

#[cfg(test)]
mod tests {
    use super::mask_shift;

    #[test]
    fn mask_shift_returns_offset_of_lowest_set_bit() {
        assert_eq!(mask_shift(0x00_FF_00_00), 16); // R in a 32-bit BGRX layout
        assert_eq!(mask_shift(0x00_00_FF_00), 8);  // G
        assert_eq!(mask_shift(0x00_00_00_FF), 0);  // B
    }

    #[test]
    fn mask_shift_handles_rgb565() {
        assert_eq!(mask_shift(0xF800), 11);
        assert_eq!(mask_shift(0x07E0), 5);
        assert_eq!(mask_shift(0x001F), 0);
    }

    #[test]
    fn mask_shift_of_zero_mask_terminates() {
        // PseudoColor visuals report all-zero masks. The extraction path
        // rejects those before shifting, but the helper itself must not
        // spin forever on the degenerate input.
        assert_eq!(mask_shift(0), 0);
    }
}
