//! Fullscreen translucent overlay for region selection
//!
//! - ESC / Right-click : cancel
//! - Left click + drag : select region
//! - Release           : confirm

use log::info;
use std::error::Error;
use std::ptr;
use x11::keysym;
use x11::xlib;

use crate::selection::SelectionRect;

/// Bright cyan border around the selection
const BORDER_COLOR: u64 = 0x00_FF_FF;
const BORDER_WIDTH: u32  = 2;

/// White crosshair guides
const CROSSHAIR_COLOR: u64 = 0xFF_FF_FF;

/// Standard X11 cursor font index for crosshair (XC_crosshair = 34)
const XC_CROSSHAIR: u32 = 34;

// ─── Drag state ───────────────────────────────────────────────────────────────

/// Tracks the in-progress mouse drag.
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

/// Mouse cursor position — None until the first MotionNotify arrives.
///
/// Using Option avoids the "assigned = 0 then overwritten before read" warning
/// that occurs when initialising with a dummy value.
struct CursorPos {
    x: i32,
    y: i32,
}

impl CursorPos {
    /// Update position from the latest motion event (after coalescing).
    fn update(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn show_selection_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    screen_width: u32,
    screen_height: u32,
) -> Result<SelectionRect, Box<dyn Error>> {
    unsafe { run_overlay(display, root, screen_width, screen_height) }
}

// ─── Internal implementation ──────────────────────────────────────────────────

unsafe fn run_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    screen_width: u32,
    screen_height: u32,
) -> Result<SelectionRect, Box<dyn Error>> {

    // ── Setup ─────────────────────────────────────────────────────────────────
    let screen   = xlib::XDefaultScreen(display);
    let visual   = xlib::XDefaultVisual(display, screen);
    let depth    = xlib::XDefaultDepth(display, screen);
    let colormap = xlib::XDefaultColormap(display, screen);

    let bg_image = xlib::XGetImage(
        display, root,
        0, 0, screen_width, screen_height,
        xlib::XAllPlanes(), xlib::ZPixmap,
    );
    if bg_image.is_null() {
        return Err("Failed to capture background".into());
    }

    let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
    attrs.override_redirect = xlib::True;
    attrs.event_mask        = xlib::ExposureMask
        | xlib::ButtonPressMask
        | xlib::ButtonReleaseMask
        | xlib::PointerMotionMask
        | xlib::KeyPressMask;
    attrs.colormap          = colormap;
    attrs.background_pixel  = 0;

    let overlay_win = xlib::XCreateWindow(
        display, root,
        0, 0, screen_width, screen_height,
        0, depth,
        xlib::InputOutput as u32,
        visual,
        xlib::CWOverrideRedirect | xlib::CWEventMask
            | xlib::CWColormap | xlib::CWBackPixel,
        &mut attrs,
    );

    xlib::XMapRaised(display, overlay_win);

    let crosshair = xlib::XCreateFontCursor(display, XC_CROSSHAIR);

    xlib::XGrabPointer(
        display, overlay_win, xlib::True,
        (xlib::ButtonPressMask
            | xlib::ButtonReleaseMask
            | xlib::PointerMotionMask) as u32,
        xlib::GrabModeAsync, xlib::GrabModeAsync,
        overlay_win, crosshair, xlib::CurrentTime,
    );
    xlib::XGrabKeyboard(
        display, overlay_win, xlib::True,
        xlib::GrabModeAsync, xlib::GrabModeAsync,
        xlib::CurrentTime,
    );

    let gc     = xlib::XCreateGC(display, overlay_win, 0, ptr::null_mut());
    let buffer = xlib::XCreatePixmap(
        display, overlay_win,
        screen_width, screen_height,
        depth as u32,
    );

    // Initial draw: no cursor position yet, no selection
    redraw(display, buffer, gc, bg_image,
           screen_width, screen_height, None, None);
    blit(display, buffer, overlay_win, gc, screen_width, screen_height);
    xlib::XFlush(display);

    // ── Event loop ────────────────────────────────────────────────────────────
    let mut event: xlib::XEvent          = std::mem::zeroed();
    let mut drag                          = DragState::new();

    // CursorPos is populated exclusively inside MotionNotify — no dummy
    // initial assignment that the compiler would flag as "never read".
    let mut cursor: Option<CursorPos>     = None;
    let mut result: Option<SelectionRect> = None;

    'event_loop: loop {
        xlib::XNextEvent(display, &mut event);

        match event.get_type() {

            // ── Expose ────────────────────────────────────────────────────────
            xlib::Expose => {
                blit(display, buffer, overlay_win, gc, screen_width, screen_height);
            }

            // ── Button Press ──────────────────────────────────────────────────
            xlib::ButtonPress => {
                let btn = event.button;
                match btn.button {
                    1 => drag.begin(btn.x, btn.y),
                    3 => break 'event_loop,
                    _ => {}
                }
            }

            // ── Motion Notify ─────────────────────────────────────────────────
            xlib::MotionNotify => {
                // Drain queued motion events — only the latest position matters
                let mut mx = event.motion.x;
                let mut my = event.motion.y;

                while xlib::XCheckMaskEvent(
                    display, xlib::PointerMotionMask, &mut event,
                ) != 0 {
                    mx = event.motion.x;
                    my = event.motion.y;
                }

                // Update (or initialise) the cursor tracker
                match cursor.as_mut() {
                    Some(c) => c.update(mx, my),
                    None    => cursor = Some(CursorPos { x: mx, y: my }),
                }

                let sel = drag.active
                    .then(|| drag.to_selection(mx, my));

                redraw(display, buffer, gc, bg_image,
                       screen_width, screen_height,
                       sel.as_ref(), cursor.as_ref());
                blit(display, buffer, overlay_win, gc, screen_width, screen_height);
                xlib::XFlush(display);
            }

            // ── Button Release ────────────────────────────────────────────────
            xlib::ButtonRelease => {
                let btn = event.button;
                if btn.button == 1 && drag.active {
                    let sel = drag.to_selection(btn.x, btn.y);
                    if sel.is_valid() {
                        result = Some(sel);
                    }
                    break 'event_loop;
                }
            }

            // ── Key Press ─────────────────────────────────────────────────────
            xlib::KeyPress => {
                let sym = xlib::XLookupKeysym(&mut event.key, 0);
                if sym == keysym::XK_Escape as u64 {
                    info!("Selection cancelled (ESC)");
                    break 'event_loop;
                }
            }

            _ => {}
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    xlib::XUngrabPointer(display, xlib::CurrentTime);
    xlib::XUngrabKeyboard(display, xlib::CurrentTime);
    xlib::XFreeCursor(display, crosshair);
    xlib::XFreeGC(display, gc);
    xlib::XFreePixmap(display, buffer);
    xlib::XDestroyImage(bg_image);
    xlib::XDestroyWindow(display, overlay_win);
    xlib::XFlush(display);

    result.ok_or_else(|| "Selection cancelled".into())
}

// ─── Drawing helpers ──────────────────────────────────────────────────────────

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

/// Redraw the full off-screen buffer then blit.
///
/// `cursor` is `None` before the first mouse move — in that case the
/// crosshair is simply not drawn (avoids a 0,0 artefact on startup).
unsafe fn redraw(
    display: *mut xlib::Display,
    buffer: xlib::Pixmap,
    gc: xlib::GC,
    bg: *mut xlib::XImage,
    screen_width: u32,
    screen_height: u32,
    selection: Option<&SelectionRect>,
    cursor: Option<&CursorPos>,
) {
    // 1. Full background
    xlib::XPutImage(
        display, buffer, gc, bg,
        0, 0, 0, 0,
        screen_width, screen_height,
    );

    // 2. Checkerboard stipple dim (~50 % black, no compositor needed)
    let stipple_data: [u8; 2] = [0xAA, 0x55];
    let stipple = xlib::XCreateBitmapFromData(
        display, buffer,
        stipple_data.as_ptr() as *const i8,
        8, 2,
    );
    xlib::XSetFillStyle(display, gc, xlib::FillStippled);
    xlib::XSetStipple(display, gc, stipple);
    xlib::XSetForeground(display, gc, 0x000000);
    xlib::XFillRectangle(display, buffer, gc, 0, 0, screen_width, screen_height);
    xlib::XSetFillStyle(display, gc, xlib::FillSolid);
    xlib::XFreePixmap(display, stipple);

    match selection {
        // 3a. Valid selection rectangle
        Some(sel) if sel.is_valid() => {
            // Restore original pixels inside selection (remove dim)
            xlib::XPutImage(
                display, buffer, gc, bg,
                sel.x as i32, sel.y as i32,
                sel.x as i32, sel.y as i32,
                sel.width, sel.height,
            );
            // Cyan border
            xlib::XSetForeground(display, gc, BORDER_COLOR);
            xlib::XSetLineAttributes(
                display, gc, BORDER_WIDTH,
                xlib::LineSolid, xlib::CapButt, xlib::JoinMiter,
            );
            xlib::XDrawRectangle(
                display, buffer, gc,
                sel.x as i32, sel.y as i32,
                sel.width, sel.height,
            );
            draw_size_label(display, buffer, gc, sel, screen_height);
        }

        // 3b. No selection — draw crosshair if cursor position is known
        _ => {
            if let Some(c) = cursor {
                draw_crosshair(
                    display, buffer, gc,
                    c.x, c.y,
                    screen_width, screen_height,
                );
            }
        }
    }
}

unsafe fn draw_crosshair(
    display: *mut xlib::Display,
    buffer: xlib::Pixmap,
    gc: xlib::GC,
    x: i32,
    y: i32,
    screen_width: u32,
    screen_height: u32,
) {
    xlib::XSetForeground(display, gc, CROSSHAIR_COLOR);
    xlib::XSetLineAttributes(
        display, gc, 1,
        xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter,
    );
    xlib::XDrawLine(display, buffer, gc, 0, y, screen_width as i32, y);
    xlib::XDrawLine(display, buffer, gc, x, 0, x, screen_height as i32);
    xlib::XSetLineAttributes(
        display, gc, 1,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter,
    );
}

unsafe fn draw_size_label(
    display: *mut xlib::Display,
    buffer: xlib::Pixmap,
    gc: xlib::GC,
    sel: &SelectionRect,
    screen_height: u32,
) {
    let text   = format!("{}×{}", sel.width, sel.height);
    let c_text = std::ffi::CString::new(text.as_str()).unwrap();

    let label_x = sel.x as i32 + 4;
    let label_y = if sel.y + sel.height + 20 < screen_height {
        (sel.y + sel.height + 16) as i32
    } else {
        sel.y as i32 - 6
    };

    // Dark pill background
    xlib::XSetForeground(display, gc, 0x000000);
    xlib::XFillRectangle(
        display, buffer, gc,
        label_x - 2, label_y - 14,
        (text.len() as u32) * 8 + 8, 20,
    );

    // White text
    xlib::XSetForeground(display, gc, 0xFFFFFF);
    xlib::XDrawString(
        display, buffer, gc,
        label_x + 2, label_y,
        c_text.as_ptr(),
        text.len() as i32,
    );
}
