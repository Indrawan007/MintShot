//! Region capture for Hyprland through grim and slurp.
//!
//! slurp owns the selection overlay; grim reads the compositor's screencopy
//! protocol. XWayland is deliberately not used. The encoded PNG is shared
//! between the unique-file saver and wl-copy without re-encoding.

use log::{info, warn};
use std::io::Cursor;
use std::process::{Command, Output, Stdio};

use std::fmt;
use crate::{clipboard, save};

#[derive(Debug)]
pub enum CaptureError {
    Cancelled,
    ScreenCaptureFailed(String),
    SaveFailed(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "screenshot cancelled by user"),
            Self::ScreenCaptureFailed(message) => write!(formatter, "screen capture failed: {}", message),
            Self::SaveFailed(message) => write!(formatter, "failed to save PNG: {}", message),
        }
    }
}

impl std::error::Error for CaptureError {}

const INSTALL_HINT: &str = "On Arch: sudo pacman -S --needed grim slurp wl-clipboard";

#[derive(Debug, PartialEq, Eq)]
struct Region {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Region {
    fn parse(text: &str) -> Result<Self, CaptureError> {
        let invalid = || {
            CaptureError::ScreenCaptureFailed(format!("Invalid region from slurp: {:?}", text))
        };
        let mut fields = text.split_whitespace();
        let position = fields.next().ok_or_else(invalid)?;
        let size = fields.next().ok_or_else(invalid)?;
        if fields.next().is_some() {
            return Err(invalid());
        }
        let (x, y) = position.split_once(',').ok_or_else(invalid)?;
        let (width, height) = size.split_once('x').ok_or_else(invalid)?;
        let x = x.parse::<i32>().map_err(|_| invalid())?;
        let y = y.parse::<i32>().map_err(|_| invalid())?;
        let width = width.parse::<u32>().map_err(|_| invalid())?;
        let height = height.parse::<u32>().map_err(|_| invalid())?;
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(invalid());
        }
        if x.checked_add(width as i32).is_none() || y.checked_add(height as i32).is_none() {
            return Err(invalid());
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn geometry(&self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

fn run_tool(tool: &str, args: &[&str]) -> Result<Output, CaptureError> {
    Command::new(tool)
        .args(args)
        // slurp can read predefined rectangles from stdin; this mode is
        // interactive selection only, so never consume the caller's stdin.
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            let hint = if error.kind() == std::io::ErrorKind::NotFound {
                format!(". {}", INSTALL_HINT)
            } else {
                String::new()
            };
            CaptureError::ScreenCaptureFailed(format!("Cannot run {}: {}{}", tool, error, hint))
        })
}

fn tool_failure(tool: &str, output: &Output) -> CaptureError {
    CaptureError::ScreenCaptureFailed(format!(
        "{} failed ({}): {}. Wayland capture requires screencopy and layer-shell support (Hyprland)",
        tool,
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn select_region() -> Result<Region, CaptureError> {
    let output = run_tool("slurp", &["-f", "%x,%y %wx%h"])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // slurp uses exit 1 both for Escape and real errors. Do not hide
        // connection/protocol failures as a successful cancellation.
        if output.status.code() == Some(1)
            && output.stdout.is_empty()
            && stderr.trim() == "selection cancelled"
        {
            return Err(CaptureError::Cancelled);
        }
        return Err(tool_failure("slurp", &output));
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|error| {
        CaptureError::ScreenCaptureFailed(format!("Invalid UTF-8 from slurp: {}", error))
    })?;
    Region::parse(text)
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), CaptureError> {
    let invalid_png = |error: png::DecodingError| {
        CaptureError::ScreenCaptureFailed(format!("grim returned an invalid PNG: {}", error))
    };
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.ignore_checksums(false);
    let mut reader = decoder.read_info().map_err(invalid_png)?;
    if reader.info().animation_control.is_some() {
        return Err(CaptureError::ScreenCaptureFailed(
            "grim returned an animated PNG; expected a still screenshot".into(),
        ));
    }
    let dimensions = (reader.info().width, reader.info().height);
    // read_info stops before pixel data. Check all rows and IEND as well,
    // otherwise truncated/corrupt PNGs can be saved and reported as success.
    // Row-wise decoding avoids allocating another full-size pixel buffer.
    while reader.next_row().map_err(invalid_png)?.is_some() {}
    reader.finish().map_err(invalid_png)?;
    // slurp uses logical coordinates; the PNG reports actual scaled pixels.
    Ok(dimensions)
}

pub fn take_partial_screenshot() -> Result<String, CaptureError> {
    let region = select_region()?;
    let geometry = region.geometry();
    info!("Wayland selection: {}", geometry);

    // No shell interpolation: the entire signed geometry is one argument.
    // slurp has unmapped its overlay before it exits, so it isn't captured.
    let output = run_tool("grim", &["-t", "png", "-g", &geometry, "-"])?;
    if !output.status.success() {
        return Err(tool_failure("grim", &output));
    }
    let (width, height) = png_dimensions(&output.stdout)?;
    let filepath = save::save_encoded_png(&output.stdout)
        .map_err(|error| CaptureError::SaveFailed(error.to_string()))?;

    // A missing/broken clipboard must not discard a successfully saved image.
    let clipboard_ok = match clipboard::copy_png(&output.stdout) {
        Ok(()) => {
            info!("Clipboard: wl-copy image/png ✓");
            true
        }
        Err(error) => {
            warn!("Wayland clipboard failed: {} (file still saved)", error);
            false
        }
    };
    send_notification(&filepath, width, height, clipboard_ok);
    Ok(filepath)
}

fn send_notification(filepath: &str, w: u32, h: u32, clipboard_ok: bool) {
    let status = if clipboard_ok {
        "📋 Copied to clipboard — Ctrl+V to paste!"
    } else {
        "⚠ Clipboard unavailable — file saved only"
    };

    let body = format!("{}×{} px\n{}\n{}", w, h, status, filepath);

    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name=MintShot",
            "--icon=accessories-screenshot",
            "--urgency=low",
            "--expire-time=4000",
            "MintShot — Screenshot Captured ✓",
            &body,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_monitor_coordinates_are_preserved() {
        let region = Region::parse("-1920,-1080 3840x2160\n").unwrap();
        assert_eq!(region.geometry(), "-1920,-1080 3840x2160");
    }

    #[test]
    fn malformed_or_empty_regions_are_rejected() {
        for text in [
            "",
            "0,0",
            "0,0 0x10",
            "0,0 10x0",
            "0,0 -1x10",
            "x,y 10x10",
            "0,0 10x10 extra",
            "0,0 2147483648x10",
            "2147483647,0 1x1",
            "0,2147483647 1x1",
            "0,0 10x10; touch /tmp/unwanted",
        ] {
            assert!(Region::parse(text).is_err(), "accepted {:?}", text);
        }
    }

    #[test]
    fn png_dimensions_use_pixels_not_logical_selection_size() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 6);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0; 4 * 6 * 4]).unwrap();
            writer.finish().unwrap();
        }
        assert_eq!(png_dimensions(&bytes).unwrap(), (4, 6));
        assert!(png_dimensions(b"not a PNG").is_err());
        assert!(png_dimensions(b"").is_err());
        // Valid header and pixel data, but no IEND: previously accepted.
        assert!(png_dimensions(&bytes[..bytes.len() - 12]).is_err());
        // A corrupt IDAT checksum must not be copied/saved as a good PNG.
        let mut corrupt = bytes.clone();
        let idat = corrupt.windows(4).position(|chunk| chunk == b"IDAT").unwrap();
        let length = u32::from_be_bytes(corrupt[idat - 4..idat].try_into().unwrap()) as usize;
        corrupt[idat + 4 + length] ^= 1;
        assert!(png_dimensions(&corrupt).is_err());
    }
    #[test]
    fn animated_payloads_are_not_accepted_as_still_screenshots() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(1, 0).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0; 4]).unwrap();
            writer.finish().unwrap();
        }
        assert!(png_dimensions(&bytes).is_err());
    }

}
