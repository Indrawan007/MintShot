//! Selection rectangle + desktop geometry data structures

use x11::xlib;

/// Represents a rectangular screen selection
#[derive(Debug, Clone, Copy)]
pub struct SelectionRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SelectionRect {
    /// Create from two corner points (handles any drag direction)
    pub fn from_points(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        let x = x1.min(x2).max(0) as u32;
        let y = y1.min(y2).max(0) as u32;
        // Compute the distance in i64 to avoid i32 overflow on extreme drags.
        let width = (x2 as i64 - x1 as i64).unsigned_abs() as u32;
        let height = (y2 as i64 - y1 as i64).unsigned_abs() as u32;
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Clamp to screen bounds so the rectangle never extends past the edges.
    pub fn clamped_to(&self, sw: u32, sh: u32) -> Self {
        let x = self.x.min(sw.saturating_sub(1));
        let y = self.y.min(sh.saturating_sub(1));
        let width = self.width.min(sw.saturating_sub(x));
        let height = self.height.min(sh.saturating_sub(y));
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Check if selection has meaningful area
    pub fn is_valid(&self) -> bool {
        self.width >= 2 && self.height >= 2
    }
}

/// Full virtual-desktop geometry across all monitors (Fix #21).
///
/// `x`/`y` are the bounding-box origin in root coordinates and may be
/// negative when a monitor sits left of or above the primary one.
#[derive(Debug, Clone, Copy)]
pub struct DesktopGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Root window of the default screen, carried along so callers only have
    /// to pass one value around.
    pub root: xlib::Window,
}

/// Bounding box of a set of screen rectangles, each `(x, y, w, h)`.
///
/// Coordinates may be negative — a monitor left of or above the primary one
/// has a negative origin. Returns `None` for an empty input.
pub fn bounding_box(screens: &[(i32, i32, u32, u32)]) -> Option<(i32, i32, u32, u32)> {
    let mut iter = screens.iter();
    let &(x, y, w, h) = iter.next()?;
    let (mut min_x, mut min_y) = (x, y);
    let (mut max_x, mut max_y) = (x + w as i32, y + h as i32);
    for &(x, y, w, h) in iter {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w as i32);
        max_y = max_y.max(y + h as i32);
    }
    Some((min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32))
}

// ─── Tests ────────────────────────────────────────────────────────────────────
// Pure geometry, no X11 needed — these run under a plain `cargo test`.

#[cfg(test)]
mod tests {
    use super::{bounding_box, SelectionRect};

    fn rect(x: u32, y: u32, w: u32, h: u32) -> SelectionRect {
        SelectionRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn from_points_handles_left_to_right_drag() {
        let r = SelectionRect::from_points(10, 20, 110, 220);
        assert_eq!((r.x, r.y, r.width, r.height), (10, 20, 100, 200));
    }

    #[test]
    fn from_points_handles_right_to_left_drag() {
        // Same rectangle as above, dragged the other way.
        let r = SelectionRect::from_points(110, 220, 10, 20);
        assert_eq!((r.x, r.y, r.width, r.height), (10, 20, 100, 200));
    }

    #[test]
    fn from_points_clamps_negative_origin_to_zero() {
        // Drag starting off the top-left edge: origin floors at 0, but the
        // size is still measured between the two real points.
        let r = SelectionRect::from_points(-50, -60, 50, 60);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 100, 120));
    }

    #[test]
    fn from_points_of_single_click_has_no_area() {
        let r = SelectionRect::from_points(100, 100, 100, 100);
        assert_eq!((r.width, r.height), (0, 0));
        assert!(!r.is_valid());
    }

    #[test]
    fn is_valid_requires_two_pixels_per_axis() {
        assert!(!rect(0, 0, 1, 100).is_valid());
        assert!(!rect(0, 0, 100, 1).is_valid());
        assert!(rect(0, 0, 2, 2).is_valid());
    }

    #[test]
    fn clamped_to_truncates_selection_running_off_screen() {
        let r = rect(100, 100, 500, 500).clamped_to(300, 300);
        assert_eq!((r.x, r.y, r.width, r.height), (100, 100, 200, 200));
    }

    #[test]
    fn clamped_to_never_extends_past_the_last_pixel() {
        let r = rect(1000, 1000, 50, 50).clamped_to(300, 300);
        // Origin pins to the last pixel, leaving a 1x1 (invalid) remainder.
        assert_eq!((r.x, r.y, r.width, r.height), (299, 299, 1, 1));
        assert!(!r.is_valid());
    }

    #[test]
    fn clamped_to_leaves_full_screen_selection_intact() {
        let r = rect(0, 0, 1920, 1080).clamped_to(1920, 1080);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 1920, 1080));
    }

    #[test]
    fn clamped_to_is_idempotent() {
        let once = rect(250, 250, 400, 400).clamped_to(300, 300);
        let twice = once.clamped_to(300, 300);
        assert_eq!(
            (once.x, once.y, once.width, once.height),
            (twice.x, twice.y, twice.width, twice.height)
        );
    }

    #[test]
    fn bounding_box_of_single_screen_is_that_screen() {
        assert_eq!(
            bounding_box(&[(0, 0, 1920, 1080)]),
            Some((0, 0, 1920, 1080))
        );
    }

    #[test]
    fn bounding_box_of_empty_input_is_none() {
        assert_eq!(bounding_box(&[]), None);
    }

    #[test]
    fn bounding_box_spans_side_by_side_monitors() {
        // 1920x1080 primary with a 2560x1440 monitor to its right.
        let screens = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert_eq!(bounding_box(&screens), Some((0, 0, 4480, 1440)));
    }

    #[test]
    fn bounding_box_handles_negative_origin() {
        // Secondary monitor placed LEFT of and ABOVE the primary one — the
        // case that was unreachable before multi-monitor support.
        let screens = [(-1920, -200, 1920, 1080), (0, 0, 1920, 1080)];
        assert_eq!(bounding_box(&screens), Some((-1920, -200, 3840, 1280)));
    }

    #[test]
    fn bounding_box_handles_stacked_monitors() {
        let screens = [(0, 0, 1920, 1080), (0, 1080, 1920, 1080)];
        assert_eq!(bounding_box(&screens), Some((0, 0, 1920, 2160)));
    }

    #[test]
    fn bounding_box_covers_disjoint_gap() {
        // Monitors not touching: the box still spans the empty gap between
        // them, which is what an overlay window needs.
        let screens = [(0, 0, 800, 600), (2000, 1500, 800, 600)];
        assert_eq!(bounding_box(&screens), Some((0, 0, 2800, 2100)));
    }
}
