//! Save grim's encoded PNG with an atomic, unique timestamped filename.

use chrono::{DateTime, Local};
use log::{info, warn};
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

fn get_save_dir() -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or("HOME is not set; cannot determine the screenshot directory")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME must be an absolute path".into());
    }
    let save_dir = home.join("Pictures/MintShot");
    fs::create_dir_all(&save_dir)?;
    Ok(save_dir)
}

/// O_CREAT|O_EXCL prevents overwrites and filename collision races.
fn create_unique_file(
    save_dir: &Path,
    now: &DateTime<Local>,
) -> Result<(fs::File, PathBuf), Box<dyn Error>> {
    let timestamp = now.format("%Y%m%d_%H%M%S%.3f");
    for counter in 0u32..=9_999 {
        let filename = if counter == 0 {
            format!("mintshot_{}.png", timestamp)
        } else {
            format!("mintshot_{}_{}.png", timestamp, counter)
        };
        let path = save_dir.join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("Cannot create a unique filename after 10 000 attempts".into())
}

/// Only called for a newly-created file owned by this capture. Do not leave
/// empty/partial screenshots behind if writing or flushing fails (e.g. ENOSPC).
fn write_png(file: fs::File, path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut writer = BufWriter::with_capacity(64 * 1024, file);
    let result = writer.write_all(bytes).and_then(|()| writer.flush());
    drop(writer);
    if let Err(error) = result {
        if let Err(cleanup_error) = fs::remove_file(path) {
            warn!("Could not remove partial screenshot {}: {}", path.display(), cleanup_error);
        }
        return Err(error.into());
    }
    Ok(())
}

/// Save the exact PNG from grim, without pixel conversion or re-encoding.
pub fn save_encoded_png(png_bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let save_dir = get_save_dir()?;
    let (file, filepath) = create_unique_file(&save_dir, &Local::now())?;
    write_png(file, &filepath, png_bytes)?;

    let path = filepath.to_string_lossy().to_string();
    info!("Saved: {} ({} bytes)", path, png_bytes.len());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("mintshot-save-{}-{}", std::process::id(), nonce));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn timestamp_collisions_do_not_overwrite_previous_images() {
        let directory = TestDir::new();
        let now = Local::now();
        let (first, first_path) = create_unique_file(&directory.0, &now).unwrap();
        write_png(first, &first_path, b"first image").unwrap();
        let (second, second_path) = create_unique_file(&directory.0, &now).unwrap();
        write_png(second, &second_path, b"second image").unwrap();
        assert_ne!(first_path, second_path);
        assert_eq!(fs::read(first_path).unwrap(), b"first image".to_vec());
        assert_eq!(fs::read(second_path).unwrap(), b"second image".to_vec());
    }

    #[test]
    fn failed_flush_removes_the_incomplete_file() {
        let directory = TestDir::new();
        let (file, path) = create_unique_file(&directory.0, &Local::now()).unwrap();
        drop(file);
        // A read-only fd deterministically fails during the buffered flush,
        // without needing to fill a disk or depend on the test runner's uid.
        let read_only = fs::File::open(&path).unwrap();
        assert!(write_png(read_only, &path, b"partial PNG").is_err());
        assert!(!path.exists());
    }
}
