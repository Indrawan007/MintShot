//! Headless CLI tests for capture, saving and the native Wayland clipboard.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    png: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mintshot-wayland-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let bin = root.join("bin");
        let home = root.join("home with spaces");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        let mut png = Vec::new();
        {
            // Twice the mock region's logical size: test scaled monitors.
            let mut encoder = png::Encoder::new(&mut png, 8, 6);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[128; 8 * 6 * 4]).unwrap();
            writer.finish().unwrap();
        }
        fs::write(root.join("input.png"), &png).unwrap();
        let fixture = Self {
            root,
            bin,
            home,
            png,
        };
        fixture.script(
            "slurp",
            r#"printf '%s\n' "$@" > "$MOCK_LOG/slurp.args"
printf '%s\n' '-1920,-1080 4x3'
"#,
        );
        fixture.script(
            "grim",
            r#"printf '%s\n' "$@" > "$MOCK_LOG/grim.args"
/bin/cat "$TEST_PNG"
"#,
        );
        fixture.script(
            "wl-copy",
            r#"printf '%s\n' "$@" > "$MOCK_LOG/wl-copy.args"
/bin/cat > "$MOCK_LOG/clipboard.png"
"#,
        );
        fixture
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.bin.join(name);
        fs::write(&path, format!("#!/bin/sh\n{}", body)).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mintshot"));
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .env("MOCK_LOG", &self.root)
            .env("TEST_PNG", self.root.join("input.png"))
            .env("WAYLAND_DISPLAY", "wayland-test")
            .env("XDG_SESSION_TYPE", "wayland")
            // Deliberately unusable: the backend must not open XWayland.
            .env("DISPLAY", ":mintshot-test")
            .env("RUST_LOG", "info");
        command
    }

    fn run(&self) -> Output {
        self.command().arg("--capture").output().unwrap()
    }

    fn images(&self) -> Vec<PathBuf> {
        let path = self.home.join("Pictures/MintShot");
        if !path.exists() {
            return Vec::new();
        }
        fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    fn log(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(name)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pure_wayland_captures_saves_and_copies_the_same_png() {
    let fixture = Fixture::new();
    let output = fixture.command().env_remove("DISPLAY").output().unwrap();
    assert_code(&output, 0);
    let images = fixture.images();
    assert_eq!(images.len(), 1);
    assert_eq!(fs::read(&images[0]).unwrap(), fixture.png);
    assert_eq!(fs::read(fixture.root.join("clipboard.png")).unwrap(), fixture.png);
    assert_eq!(fixture.log("slurp.args"), "-f\n%x,%y %wx%h\n");
    assert_eq!(fixture.log("grim.args"), "-t\npng\n-g\n-1920,-1080 4x3\n-\n");
    assert_eq!(fixture.log("wl-copy.args"), "--type\nimage/png\n");
}

#[test]
fn display_variable_does_not_change_wayland_capture() {
    let fixture = Fixture::new();
    assert_code(&fixture.run(), 0);
    assert_eq!(fixture.images().len(), 1);
    assert!(fixture.root.join("grim.args").exists());
}

#[test]
fn wayland_session_type_is_also_respected() {
    let fixture = Fixture::new();
    let output = fixture.command().env_remove("WAYLAND_DISPLAY").output().unwrap();
    assert_code(&output, 0);
    assert_eq!(fixture.images().len(), 1);
}

#[test]
fn escape_is_a_clean_cancellation_without_capture_or_clipboard() {
    let fixture = Fixture::new();
    fixture.script("slurp", "printf 'selection cancelled\\n' >&2\nexit 1\n");
    let output = fixture.run();
    assert_code(&output, 0);
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled by user"));
    assert!(fixture.images().is_empty());
    assert!(!fixture.root.join("grim.args").exists());
    assert!(!fixture.root.join("clipboard.png").exists());
}

#[test]
fn slurp_connection_errors_are_not_silently_cancelled() {
    let fixture = Fixture::new();
    fixture.script("slurp", "printf 'failed to connect to display\\n' >&2\nexit 1\n");
    let output = fixture.run();
    assert_code(&output, 3);
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to connect"));
    assert!(fixture.images().is_empty());
    assert!(!fixture.root.join("grim.args").exists());
}

#[test]
fn invalid_geometry_cannot_be_forwarded_to_grim_or_a_shell() {
    let fixture = Fixture::new();
    fixture.script("slurp", "printf '%s\\n' '0,0 4x3; touch unwanted'\n");
    assert_code(&fixture.run(), 3);
    assert!(fixture.images().is_empty());
    assert!(!fixture.root.join("grim.args").exists());
}

#[test]
fn missing_capture_helpers_produce_actionable_errors() {
    for tool in ["slurp", "grim"] {
        let fixture = Fixture::new();
        fs::remove_file(fixture.bin.join(tool)).unwrap();
        let output = fixture.run();
        assert_code(&output, 3);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!("Cannot run {}", tool)));
        assert!(stderr.contains("pacman"));
        assert!(fixture.images().is_empty());
    }
}

#[test]
fn grim_errors_and_invalid_pngs_do_not_create_screenshots() {
    for body in [
        "printf 'compositor does not support screencopy\\n' >&2\nexit 1\n",
        "printf 'not a PNG'\n",
        "exit 0\n",
    ] {
        let fixture = Fixture::new();
        fixture.script("grim", body);
        assert_code(&fixture.run(), 3);
        assert!(fixture.images().is_empty());
        assert!(!fixture.root.join("clipboard.png").exists());
    }
}

#[test]
fn missing_wayland_clipboard_is_nonfatal() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.bin.join("wl-copy")).unwrap();
    let output = fixture.run();
    assert_code(&output, 0);
    assert_eq!(fixture.images().len(), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("wl-clipboard"));
}

#[test]
fn failing_wayland_clipboard_does_not_lose_the_saved_png() {
    let fixture = Fixture::new();
    fixture.script("wl-copy", "/bin/cat > /dev/null\nprintf 'clipboard unavailable\\n' >&2\nexit 2\n");
    let output = fixture.run();
    assert_code(&output, 0);
    assert_eq!(fs::read(&fixture.images()[0]).unwrap(), fixture.png);
    assert!(String::from_utf8_lossy(&output.stderr).contains("file still saved"));
}

#[test]
fn save_errors_are_fatal_but_do_not_attempt_clipboard_copy() {
    let fixture = Fixture::new();
    let home_file = fixture.root.join("not-a-directory");
    fs::write(&home_file, b"not a directory").unwrap();
    let output = fixture.command().env("HOME", home_file).output().unwrap();
    assert_code(&output, 4);
    assert!(!fixture.root.join("clipboard.png").exists());
}

#[test]
fn successive_captures_do_not_overwrite_saved_images() {
    let fixture = Fixture::new();
    assert_code(&fixture.run(), 0);
    assert_code(&fixture.run(), 0);
    assert_eq!(fixture.images().len(), 2);
}

#[test]
fn help_and_version_work_without_a_display_or_helper_tools() {
    let fixture = Fixture::new();
    for arg in ["--help", "--version"] {
        let output = fixture.command()
            .env("PATH", &fixture.home) // Empty directory: no helper tools.
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("XDG_SESSION_TYPE")
            .arg(arg).output().unwrap();
        assert_code(&output, 0);
        assert!(String::from_utf8_lossy(&output.stdout).contains("MintShot"));
    }
    assert!(fixture.images().is_empty());
}

#[test]
fn capture_without_a_display_reports_display_error() {
    let fixture = Fixture::new();
    let output = fixture.command()
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_SESSION_TYPE")
        .output().unwrap();
    assert_code(&output, 2);
    assert!(fixture.images().is_empty());
}

#[test]
fn display_without_a_wayland_session_is_rejected() {
    let fixture = Fixture::new();
    let output = fixture.command()
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_SESSION_TYPE", "x11")
        .output().unwrap();
    assert_code(&output, 2); // DISPLAY alone is not a Wayland session.
    assert!(!fixture.root.join("slurp.args").exists());
}

#[test]
fn removed_daemon_flag_is_rejected_without_starting_helpers() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--daemon").output().unwrap();
    assert_code(&output, 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option '--daemon'"));
    assert!(stderr.contains("Shortcuts are configured in Hyprland"));
    assert!(fixture.images().is_empty());
    assert!(!fixture.root.join("slurp.args").exists());
}

#[test]
fn capture_without_home_fails_safely_before_clipboard_copy() {
    let fixture = Fixture::new();
    let output = fixture.command().env_remove("HOME").output().unwrap();
    assert_code(&output, 4);
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOME is not set"));
    assert!(!fixture.root.join("clipboard.png").exists());
}

#[test]
fn extra_arguments_are_rejected_before_any_capture() {
    for arguments in [
        ["--capture", "--typo"],
        ["--version", "--capture"],
        ["--help", "extra"],
    ] {
        let fixture = Fixture::new();
        let output = fixture.command().args(arguments).output().unwrap();
        assert_code(&output, 1);
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected extra argument"));
        assert!(!fixture.root.join("slurp.args").exists());
        assert!(fixture.images().is_empty());
    }
}

#[test]
fn non_utf8_arguments_are_errors_not_panics() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let output = fixture.command().arg(OsString::from_vec(vec![0xff])).output().unwrap();
    assert_code(&output, 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
    assert!(!fixture.root.join("slurp.args").exists());
}

#[test]
fn truncated_or_corrupt_png_payloads_are_never_saved_or_copied() {
    for corruption in ["missing-iend", "short-idat", "bad-crc"] {
        let fixture = Fixture::new();
        let mut bytes = fixture.png.clone();
        let idat = bytes.windows(4).position(|chunk| chunk == b"IDAT").unwrap();
        let length = u32::from_be_bytes(bytes[idat - 4..idat].try_into().unwrap()) as usize;
        match corruption {
            "missing-iend" => bytes.truncate(bytes.len() - 12),
            "short-idat" => bytes.truncate(idat + 5),
            "bad-crc" => bytes[idat + 4 + length] ^= 1,
            _ => unreachable!(),
        }
        fs::write(fixture.root.join("input.png"), bytes).unwrap();
        let output = fixture.run();
        assert_code(&output, 3);
        assert!(fixture.images().is_empty());
        assert!(!fixture.root.join("clipboard.png").exists());
    }
}

#[test]
fn relative_home_is_rejected_without_creating_screenshot_directories() {
    let fixture = Fixture::new();
    let output = fixture.command().current_dir(&fixture.root)
        .env("HOME", "relative-home").output().unwrap();
    assert_code(&output, 4);
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOME must be an absolute path"));
    assert!(!fixture.root.join("relative-home").exists());
    assert!(!fixture.root.join("clipboard.png").exists());
}

#[test]
fn clipboard_exiting_before_reading_is_nonfatal() {
    let fixture = Fixture::new();
    fixture.script("wl-copy", "exit 2\n");
    let output = fixture.run();
    assert_code(&output, 0);
    assert_eq!(fixture.images().len(), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("file still saved"));
}

#[test]
fn silent_slurp_failures_are_not_reported_as_user_cancellation() {
    let fixture = Fixture::new();
    fixture.script("slurp", "exit 1\n");
    let output = fixture.run();
    assert_code(&output, 3);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("cancelled by user"));
    assert!(!fixture.root.join("grim.args").exists());
    assert!(fixture.images().is_empty());
}
