//! Native Wayland image clipboard through wl-copy.

use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

/// Copy an encoded PNG. wl-copy retains ownership after MintShot exits.
pub fn copy_png(png_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        // The background owner retains stderr; don't keep the caller's pipe alive.
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| -> Box<dyn Error> {
            if error.kind() == std::io::ErrorKind::NotFound {
                "wl-copy not found. Install: sudo pacman -S --needed wl-clipboard".into()
            } else {
                error.into()
            }
        })?;

    let write_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(png_bytes),
        None => Err(std::io::Error::other("wl-copy stdin unavailable")),
    };
    // Close stdin and reap the child even after a broken pipe.
    let status = child.wait()?;
    write_result?;
    if !status.success() {
        return Err(format!("wl-copy exited {}", status).into());
    }
    Ok(())
}

