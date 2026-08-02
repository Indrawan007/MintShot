//! Selection rectangle data structure

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
        let width  = (x2 as i64 - x1 as i64).unsigned_abs() as u32;
        let height = (y2 as i64 - y1 as i64).unsigned_abs() as u32;
        Self { x, y, width, height }
    }

    /// Clamp to screen bounds so the rectangle never extends past the edges.
    pub fn clamped_to(&self, sw: u32, sh: u32) -> Self {
        let x = self.x.min(sw.saturating_sub(1));
        let y = self.y.min(sh.saturating_sub(1));
        let width  = self.width.min(sw.saturating_sub(x));
        let height = self.height.min(sh.saturating_sub(y));
        Self { x, y, width, height }
    }

    /// Check if selection has meaningful area
    pub fn is_valid(&self) -> bool {
        self.width >= 2 && self.height >= 2
    }
}
