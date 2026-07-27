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
        let x      = x1.min(x2).max(0) as u32;
        let y      = y1.min(y2).max(0) as u32;
        let width  = (x2 - x1).unsigned_abs();
        let height = (y2 - y1).unsigned_abs();
        Self { x, y, width, height }
    }

    /// Check if selection has meaningful area
    pub fn is_valid(&self) -> bool {
        self.width >= 2 && self.height >= 2
    }
}
