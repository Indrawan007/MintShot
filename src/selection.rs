//! Selection rectangle data structure
//!
//! Minimal struct to represent user's screen selection area

/// Represents a rectangular screen selection
#[derive(Debug, Clone, Copy)]
pub struct SelectionRect {
    /// X coordinate of top-left corner
    pub x: u32,
    /// Y coordinate of top-left corner
    pub y: u32,
    /// Width of selection
    pub width: u32,
    /// Height of selection
    pub height: u32,
}

impl SelectionRect {
    /// Create a new selection rectangle from two corner points
    ///
    /// Handles any drag direction (top-left to bottom-right, or reverse)
    pub fn from_points(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        let x = x1.min(x2).max(0) as u32;
        let y = y1.min(y2).max(0) as u32;
        let width = (x2 - x1).unsigned_abs();
        let height = (y2 - y1).unsigned_abs();

        SelectionRect { x, y, width, height }
    }

    /// Check if selection has meaningful area
    pub fn is_valid(&self) -> bool {
        self.width >= 2 && self.height >= 2
    }
}
