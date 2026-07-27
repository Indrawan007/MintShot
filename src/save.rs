//! PNG file saving module
//!
//! Saves screenshot to ~/Pictures/MintShot/ directory
//! Uses the `png` crate for efficient encoding with minimal memory allocation

use chrono::Local;
use log::info;
use std::error::Error;
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;

/// Default save directory under user's home
const SAVE_DIR_NAME: &str = "Pictures/MintShot";

/// Get the save directory path, creating it if needed
fn get_save_dir() -> Result<PathBuf, Box<dyn Error>> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let save_dir = home.join(SAVE_DIR_NAME);

    if !save_dir.exists() {
        fs::create_dir_all(&save_dir)?;
        info!("Created save directory: {}", save_dir.display());
    }

    Ok(save_dir)
}

/// Generate a unique filename with timestamp
fn generate_filename() -> String {
    let now = Local::now();
    format!("mintshot_{}.png", now.format("%Y%m%d_%H%M%S"))
}

/// Save RGBA pixel data as a PNG file
///
/// Uses streaming PNG encoder for memory efficiency - doesn't need to hold
/// the entire encoded image in memory.
pub fn save_png(
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<String, Box<dyn Error>> {
    let save_dir = get_save_dir()?;
    let filename = generate_filename();
    let filepath = save_dir.join(&filename);

    let file = fs::File::create(&filepath)?;
    let buf_writer = BufWriter::with_capacity(64 * 1024, file); // 64KB buffer

    let mut encoder = png::Encoder::new(buf_writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast); // Fast compression for responsiveness
    encoder.set_filter(png::FilterType::Sub); // Sub filter is fast and effective

    let mut writer = encoder.write_header()?;

    // Write image data row by row for memory efficiency
    writer.write_image_data(pixels)?;

    let path_str = filepath.to_string_lossy().to_string();
    info!("Saved screenshot: {}", path_str);

    Ok(path_str)
}
