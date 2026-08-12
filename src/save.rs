//! PNG file saving module
//!
//! FIXES:
//!   #5  — Atomic file creation with O_EXCL (no TOCTOU race)
//!   #10 — PNG encoded once to Vec<u8>, written to file from same buffer
//!          (caller can reuse bytes for clipboard — zero re-encoding)

use chrono::{DateTime, Local};
use log::info;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

const SAVE_DIR_NAME: &str = "Pictures/MintShot";

// ─── Directory helpers ────────────────────────────────────────────────────

fn get_save_dir() -> Result<PathBuf, Box<dyn Error>> {
    let home     = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let save_dir = home.join(SAVE_DIR_NAME);
    if !save_dir.exists() {
        fs::create_dir_all(&save_dir)?;
        info!("Created save directory: {}", save_dir.display());
    }
    Ok(save_dir)
}

fn generate_filename(now: &DateTime<Local>) -> String {
    format!("mintshot_{}.png", now.format("%Y%m%d_%H%M%S%.3f"))
}

// ─── Atomic file creation (Fix #5) ───────────────────────────────────────

/// Create a uniquely-named file using O_CREAT|O_EXCL (atomic).
///
/// This eliminates the TOCTOU race between `.exists()` and `.create()`.
/// Returns `(File, PathBuf)` for the caller to write into.
fn create_unique_file(
    save_dir: &std::path::Path,
    now:      &DateTime<Local>,
) -> Result<(fs::File, PathBuf), Box<dyn Error>> {

    // ── Attempt base name first ────────────────────────────────────────────
    let base = save_dir.join(generate_filename(now));
    match OpenOptions::new().write(true).create_new(true).open(&base) {
        Ok(f)  => return Ok((f, base)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }

    // ── Collision: append counter ──────────────────────────────────────────
    for counter in 1u32..=9_999 {
        let path = save_dir.join(format!(
            "mintshot_{}_{}.png",
            now.format("%Y%m%d_%H%M%S%.3f"),
            counter,
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f)  => return Ok((f, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Err("Cannot create a unique filename after 9 999 attempts".into())
}

// ─── PNG encoder (Fix #10) ────────────────────────────────────────────────

/// Encode RGBA pixels to a PNG byte vector.
///
/// Encoding is done ONCE here. The same `Vec<u8>` is written to disk
/// AND returned to the caller so the clipboard module can reuse it
/// directly — no second encode pass needed.
fn encode_png_to_vec(
    pixels: &[u8],
    width:  u32,
    height: u32,
) -> Result<Vec<u8>, Box<dyn Error>> {
    // Pre-size buffer: PNG overhead is small, start at ~half raw size
    let mut buf = Vec::with_capacity(pixels.len() / 2);

    let mut encoder = png::Encoder::new(&mut buf, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_filter(png::FilterType::Sub);

    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()?;

    Ok(buf)
}

// ─── Public API ───────────────────────────────────────────────────────────

/// Save RGBA pixels as a PNG file.
///
/// Returns `(file_path, png_bytes)`.
///
/// The caller (capture.rs) passes `png_bytes` directly to the clipboard
/// module, avoiding a second encode pass.
///
/// # Errors
/// - Home directory not available
/// - Filesystem permission error
/// - PNG encoding error
pub fn save_png(
    pixels: &[u8],
    width:  u32,
    height: u32,
) -> Result<(String, Vec<u8>), Box<dyn Error>> {

    let save_dir = get_save_dir()?;
    let now      = Local::now();

    // ── Encode once ────────────────────────────────────────────────────────
    let png_bytes = encode_png_to_vec(pixels, width, height)?;

    // ── Atomic create (Fix #5) ─────────────────────────────────────────────
    let (file, filepath) = create_unique_file(&save_dir, &now)?;

    // ── Write encoded bytes to file ────────────────────────────────────────
    let mut writer = BufWriter::with_capacity(64 * 1024, file);
    writer.write_all(&png_bytes)?;
    writer.flush()?;

    let path_str = filepath.to_string_lossy().to_string();
    info!("Saved: {} ({} bytes)", path_str, png_bytes.len());

    // ✅ Fix #10: return png_bytes for clipboard reuse
    Ok((path_str, png_bytes))
}
