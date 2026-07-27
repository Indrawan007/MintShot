//! Clipboard module
//!
//! Copies the screenshot file path and image data to system clipboard
//! Uses arboard for cross-desktop clipboard support

use log::info;
use std::error::Error;

/// Copy screenshot to clipboard as image
///
/// Reads the PNG file and sets it as clipboard image content
pub fn copy_to_clipboard(filepath: &str) -> Result<(), Box<dyn Error>> {
    // Read the image file
    let img = image::open(filepath)?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels = rgba.into_raw();

    // Set clipboard image
    let mut clipboard = arboard::Clipboard::new()?;

    let img_data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(pixels),
    };

    clipboard.set_image(img_data)?;
    info!("Image copied to clipboard");

    Ok(())
}
