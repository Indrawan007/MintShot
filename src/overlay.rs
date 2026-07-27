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

/// Selection border - bright green for maximum contrast on any background
const SEL_BORDER_COLOR: u64  = 0x00_CC_66;
/// Selection border outer glow (dark for contrast)
const SEL_BORDER_SHADOW: u64 = 0x00_00_00;
/// Corner handle color
const CORNER_COLOR: u64      = 0xFF_FF_FF;
/// Corner handle size (pixels)
const CORNER_SIZE: i32       = 8;
/// Selection border thickness
const BORDER_WIDTH: u32      = 2;

/// Crosshair guide color
const GUIDE_COLOR: u64       = 0xFF_FF_FF;
/// Edge guide color (lines from selection to screen edge)
const EDGE_GUIDE_COLOR: u64  = 0x80_80_80;

/// Info panel colors
const PANEL_BG: u64          = 0x1A_1A_2E;
const PANEL_TEXT: u64         = 0xFF_FF_FF;
const PANEL_ACCENT: u64      = 0x00_CC_66;
const PANEL_DIM_TEXT: u64    = 0xAA_AA_AA;

/// Help bar colors
const HELP_BG: u64           = 0x16_16_28;
const HELP_TEXT: u64         = 0xCC_CC_CC;
const HELP_KEY_BG: u64      = 0x33_33_55;
const HELP_KEY_TEXT: u64     = 0xFF_FF_FF;

/// X11 cursor constant (XC_crosshair = 34)
const XC_CROSSHAIR: u32 = 34;

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

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn show_selection_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    screen_width: u32,
    screen_height: u32,
) -> Result<SelectionRect, Box<dyn Error>> {
    unsafe { run_overlay(display, root, screen_width, screen_height) }
}

// ─── Main overlay logic ──────────────────────────────────────────────────────

unsafe fn run_overlay(
    display: *mut xlib::Display,
    root: xlib::Window,
    sw: u32,
    sh: u32,
) -> Result<SelectionRect, Box<dyn Error>> {

    let screen   = xlib::XDefaultScreen(display);
    let visual   = xlib::XDefaultVisual(display, screen);
    let depth    = xlib::XDefaultDepth(display, screen);
    let colormap = xlib::XDefaultColormap(display, screen);

    // Capture screen before overlay appears
    let bg = xlib::XGetImage(
        display, root, 0, 0, sw, sh,
        xlib::XAllPlanes(), xlib::ZPixmap,
    );
    if bg.is_null() {
        return Err("Failed to capture background".into());
    }

    // Create fullscreen overlay window
    let mut attrs: xlib::XSetWindowAttributes = std::mem::zeroed();
    attrs.override_redirect = xlib::True;
    attrs.event_mask        = xlib::ExposureMask
        | xlib::ButtonPressMask
        | xlib::ButtonReleaseMask
        | xlib::PointerMotionMask
        | xlib::KeyPressMask;
    attrs.colormap          = colormap;
    attrs.background_pixel  = 0;

    let win = xlib::XCreateWindow(
        display, root, 0, 0, sw, sh, 0, depth,
        xlib::InputOutput as u32, visual,
        xlib::CWOverrideRedirect | xlib::CWEventMask
            | xlib::CWColormap | xlib::CWBackPixel,
        &mut attrs,
    );

    xlib::XMapRaised(display, win);

    let cursor_font = xlib::XCreateFontCursor(display, XC_CROSSHAIR);
    xlib::XGrabPointer(
        display, win, xlib::True,
        (xlib::ButtonPressMask | xlib::ButtonReleaseMask
            | xlib::PointerMotionMask) as u32,
        xlib::GrabModeAsync, xlib::GrabModeAsync,
        win, cursor_font, xlib::CurrentTime,
    );
    xlib::XGrabKeyboard(
        display, win, xlib::True,
        xlib::GrabModeAsync, xlib::GrabModeAsync, xlib::CurrentTime,
    );

    let gc  = xlib::XCreateGC(display, win, 0, ptr::null_mut());
    let buf = xlib::XCreatePixmap(display, win, sw, sh, depth as u32);

    // Load a font for text rendering
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

    // ── Event loop ────────────────────────────────────────────────────────────
    let mut event: xlib::XEvent          = std::mem::zeroed();
    let mut drag                          = DragState::new();
    let mut cursor: Option<CursorPos>     = None;
    let mut result: Option<SelectionRect> = None;

    'event_loop: loop {
        xlib::XNextEvent(display, &mut event);

        match event.get_type() {
            xlib::Expose => {
                blit(display, buf, win, gc, sw, sh);
            }

            xlib::ButtonPress => {
                let btn = event.button;
                match btn.button {
                    1 => drag.begin(btn.x, btn.y),
                    3 => break 'event_loop,
                    _ => {}
                }
            }

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

            xlib::ButtonRelease => {
                let btn = event.button;
                if btn.button == 1 && drag.active {
                    let sel = drag.to_selection(btn.x, btn.y);
                    if sel.is_valid() {
                        // Flash feedback before closing
                        flash_capture(display, buf, win, gc, bg, sw, sh, &sel);
                        result = Some(sel);
                    }
                    break 'event_loop;
                }
            }

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
    xlib::XFreeCursor(display, cursor_font);
    if !font.is_null() { xlib::XFreeFont(display, font); }
    xlib::XFreeGC(display, gc);
    xlib::XFreePixmap(display, buf);
    xlib::XDestroyImage(bg);
    xlib::XDestroyWindow(display, win);
    xlib::XFlush(display);

    result.ok_or_else(|| "Selection cancelled".into())
}

// ─── Flash feedback ───────────────────────────────────────────────────────────

/// Brief white flash inside the selected area to confirm capture
unsafe fn flash_capture(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    win: xlib::Window,
    gc: xlib::GC,
    bg: *mut xlib::XImage,
    sw: u32,
    sh: u32,
    sel: &SelectionRect,
) {
    // Frame 1: White flash over selection
    full_redraw(display, buf, gc, bg, sw, sh, Some(sel), None);
    xlib::XSetForeground(display, gc, 0xFF_FF_FF);
    xlib::XFillRectangle(
        display, buf, gc,
        sel.x as i32, sel.y as i32, sel.width, sel.height,
    );
    blit(display, buf, win, gc, sw, sh);
    xlib::XFlush(display);
    std::thread::sleep(std::time::Duration::from_millis(80));

    // Frame 2: Restore normal selection view
    full_redraw(display, buf, gc, bg, sw, sh, Some(sel), None);
    blit(display, buf, win, gc, sw, sh);
    xlib::XFlush(display);
    std::thread::sleep(std::time::Duration::from_millis(80));
}

// ─── Full redraw ──────────────────────────────────────────────────────────────

/// Complete buffer redraw: background → dim → selection/crosshair → UI panels
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
    // 1. Background screenshot
    xlib::XPutImage(display, buf, gc, bg, 0, 0, 0, 0, sw, sh);

    // 2. Smooth dim overlay using double stipple (lighter ~30% dim)
    draw_dim_overlay(display, buf, gc, sw, sh);

    // 3. Selection or crosshair
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

    // 4. Help bar at top
    draw_help_bar(display, buf, gc, sw, selection.is_some());
}

// ─── Dim overlay ──────────────────────────────────────────────────────────────

/// Draw a smooth semi-transparent dim using fine stipple pattern (~30% opacity)
unsafe fn draw_dim_overlay(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sw: u32,
    sh: u32,
) {
    // Fine 4×4 stipple pattern — 4 out of 16 pixels black = ~25% dim
    // This looks much smoother than the 2-byte checkerboard
    let stipple_data: [u8; 4] = [
        0b1000_1000,  // row 0
        0b0010_0010,  // row 1
        0b1000_1000,  // row 2
        0b0010_0010,  // row 3
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

/// Draw the selected region with clear content, border, corners, guides, and info
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

    // ── Edge guide lines (from selection edges to screen edges) ────────────
    xlib::XSetForeground(display, gc, EDGE_GUIDE_COLOR);
    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter);

    // Top edge → screen top
    xlib::XDrawLine(display, buf, gc, sx, 0, sx, sy);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32, 0,
                    sx + sel.width as i32, sy);
    // Bottom edge → screen bottom
    xlib::XDrawLine(display, buf, gc, sx, sy + sel.height as i32,
                    sx, sh as i32);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32,
                    sy + sel.height as i32,
                    sx + sel.width as i32, sh as i32);
    // Left edge → screen left
    xlib::XDrawLine(display, buf, gc, 0, sy, sx, sy);
    xlib::XDrawLine(display, buf, gc, 0, sy + sel.height as i32,
                    sx, sy + sel.height as i32);
    // Right edge → screen right
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32, sy,
                    sw as i32, sy);
    xlib::XDrawLine(display, buf, gc, sx + sel.width as i32,
                    sy + sel.height as i32,
                    sw as i32, sy + sel.height as i32);

    xlib::XSetLineAttributes(display, gc, 1,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);

    // ── Restore clear pixels inside selection ─────────────────────────────
    xlib::XPutImage(
        display, buf, gc, bg,
        sx, sy, sx, sy, sel.width, sel.height,
    );

    // ── Outer shadow border (dark) ────────────────────────────────────────
    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    xlib::XSetLineAttributes(display, gc, BORDER_WIDTH + 2,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // ── Inner selection border (green) ────────────────────────────────────
    xlib::XSetForeground(display, gc, SEL_BORDER_COLOR);
    xlib::XSetLineAttributes(display, gc, BORDER_WIDTH,
        xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    xlib::XDrawRectangle(display, buf, gc, sx, sy, sel.width, sel.height);

    // ── Corner handles ────────────────────────────────────────────────────
    draw_corner_handles(display, buf, gc, sel);

    // ── Crosshair at cursor during drag ───────────────────────────────────
    if let Some(c) = cursor {
        xlib::XSetForeground(display, gc, GUIDE_COLOR);
        xlib::XSetLineAttributes(display, gc, 1,
            xlib::LineOnOffDash, xlib::CapButt, xlib::JoinMiter);

        // Only draw crosshair lines inside the selection area
        // Horizontal through cursor
        xlib::XDrawLine(display, buf, gc, sx, c.y,
                        sx + sel.width as i32, c.y);
        // Vertical through cursor
        xlib::XDrawLine(display, buf, gc, c.x, sy,
                        c.x, sy + sel.height as i32);

        xlib::XSetLineAttributes(display, gc, 1,
            xlib::LineSolid, xlib::CapButt, xlib::JoinMiter);
    }

    // ── Info panel below selection ────────────────────────────────────────
    draw_info_panel(display, buf, gc, sel, sh);
}

// ─── Corner handles ──────────────────────────────────────────────────────────

/// Draw small white squares at each corner of the selection
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
        (sx - half,  sy - half),   // top-left
        (ex - half,  sy - half),   // top-right
        (sx - half,  ey - half),   // bottom-left
        (ex - half,  ey - half),   // bottom-right
        // Midpoints
        (sx + sel.width as i32 / 2 - half, sy - half),  // top-mid
        (sx + sel.width as i32 / 2 - half, ey - half),  // bottom-mid
        (sx - half, sy + sel.height as i32 / 2 - half), // left-mid
        (ex - half, sy + sel.height as i32 / 2 - half), // right-mid
    ];

    // Shadow behind handles
    xlib::XSetForeground(display, gc, SEL_BORDER_SHADOW);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(display, buf, gc,
            cx - 1, cy - 1,
            (CORNER_SIZE + 2) as u32, (CORNER_SIZE + 2) as u32);
    }

    // White handle fill
    xlib::XSetForeground(display, gc, CORNER_COLOR);
    for (cx, cy) in &corners {
        xlib::XFillRectangle(display, buf, gc,
            *cx, *cy,
            CORNER_SIZE as u32, CORNER_SIZE as u32);
    }
}

// ─── Crosshair guides (no selection) ─────────────────────────────────────────

/// Full-screen dashed crosshair at cursor position (before drag starts)
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

/// Show mouse coordinates in a small tooltip near the cursor
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

    // Position: prefer bottom-right of cursor, but flip if near edges
    let pad = 15;
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

    // Dark rounded-ish background
    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);

    // Subtle border
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, tx, ty, text_w as u32, text_h as u32);

    // Coordinate text
    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(display, buf, gc, tx + 6, ty + 14,
                      c_text.as_ptr(), text.len() as i32);
}

// ─── Info panel ──────────────────────────────────────────────────────────────

/// Detailed info panel below (or above) the selection showing dimensions and position
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

    // Position: below selection, centered. Flip above if too close to bottom.
    let px = (sel.x as i32 + sel.width as i32 / 2) - panel_w as i32 / 2;
    let px = px.max(margin).min((sh as i32) - panel_w as i32 - margin); // clamp to screen

    let py = if sel.y + sel.height + panel_h + 12 < sh {
        (sel.y + sel.height) as i32 + 10
    } else {
        sel.y as i32 - panel_h as i32 - 10
    };

    // ── Panel background ──────────────────────────────────────────────────
    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px, py, panel_w, panel_h);

    // Border
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawRectangle(display, buf, gc, px, py, panel_w, panel_h);

    // ── Line 1: Dimensions with accent color ──────────────────────────────
    let line1 = format!(" {}  x  {} px", sel.width, sel.height);
    let c_line1 = std::ffi::CString::new(line1.as_str()).unwrap();

    // Size icon placeholder (small filled rect as icon)
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 10, py + 8, 12, 12);
    xlib::XSetForeground(display, gc, PANEL_BG);
    xlib::XFillRectangle(display, buf, gc, px + 13, py + 11, 6, 6);

    // Dimension text
    xlib::XSetForeground(display, gc, PANEL_TEXT);
    xlib::XDrawString(display, buf, gc, px + 26, py + 18,
                      c_line1.as_ptr(), line1.len() as i32);

    // ── Line 2: Position ──────────────────────────────────────────────────
    let line2 = format!(" Position: ({}, {})", sel.x, sel.y);
    let c_line2 = std::ffi::CString::new(line2.as_str()).unwrap();

    // Position icon (crosshair dot)
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XFillRectangle(display, buf, gc, px + 14, py + 31, 4, 4);

    xlib::XSetForeground(display, gc, PANEL_DIM_TEXT);
    xlib::XDrawString(display, buf, gc, px + 26, py + 36,
                      c_line2.as_ptr(), line2.len() as i32);
}

// ─── Help bar ────────────────────────────────────────────────────────────────

/// Top help bar with keyboard shortcuts and instructions
unsafe fn draw_help_bar(
    display: *mut xlib::Display,
    buf: xlib::Pixmap,
    gc: xlib::GC,
    sw: u32,
    has_selection: bool,
) {
    let bar_h: u32 = 28;

    // Semi-dark background bar
    xlib::XSetForeground(display, gc, HELP_BG);
    xlib::XFillRectangle(display, buf, gc, 0, 0, sw, bar_h);

    // Bottom border line
    xlib::XSetForeground(display, gc, 0x33_33_55);
    xlib::XDrawLine(display, buf, gc, 0, bar_h as i32, sw as i32, bar_h as i32);

    // ── App name ──────────────────────────────────────────────────────────
    let app_name = "MintShot";
    let c_app = std::ffi::CString::new(app_name).unwrap();
    xlib::XSetForeground(display, gc, PANEL_ACCENT);
    xlib::XDrawString(display, buf, gc, 12, 18,
                      c_app.as_ptr(), app_name.len() as i32);

    // ── Separator ─────────────────────────────────────────────────────────
    xlib::XSetForeground(display, gc, 0x44_44_66);
    xlib::XDrawLine(display, buf, gc, 80, 5, 80, bar_h as i32 - 5);

    // ── Instructions based on state ───────────────────────────────────────
    let instructions: &[(&str, &str)] = if has_selection {
        &[("Release", "Capture"), ("ESC", "Cancel"), ("Right Click", "Cancel")]
    } else {
        &[("Click+Drag", "Select area"), ("ESC", "Cancel"), ("Right Click", "Cancel")]
    };

    let mut offset_x: i32 = 92;

    for (key, desc) in instructions {
        // Key badge background
        let key_w = (key.len() as i32) * 7 + 10;
        xlib::XSetForeground(display, gc, HELP_KEY_BG);
        xlib::XFillRectangle(display, buf, gc,
            offset_x, 5, key_w as u32, 18);

        // Key text
        let c_key = std::ffi::CString::new(*key).unwrap();
        xlib::XSetForeground(display, gc, HELP_KEY_TEXT);
        xlib::XDrawString(display, buf, gc, offset_x + 5, 18,
                          c_key.as_ptr(), key.len() as i32);

        offset_x += key_w + 4;

        // Description text
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
