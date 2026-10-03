//! MintShot — partial screenshots for Arch Linux + Hyprland (Wayland only).

mod capture;
mod clipboard;
mod save;

use capture::CaptureError;
use log::{error, info};
use std::ffi::{OsStr, OsString};
use std::process;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp(None)
    .format_module_path(false)
    .init();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mode = match parse_args(&args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("mintshot: {}", message);
            eprintln!("Run 'mintshot --help' for usage. Shortcuts are configured in Hyprland.");
            process::exit(1);
        }
    };
    let code = match mode {
        Mode::Capture => run_capture(),
        Mode::Version => {
            println!("MintShot v{} — Arch Linux + Hyprland", env!("CARGO_PKG_VERSION"));
            0
        }
        Mode::Help => {
            print_help();
            0
        }
    };
    process::exit(code);
}

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Capture,
    Version,
    Help,
}

fn parse_args(args: &[OsString]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Capture),
        [argument] => match argument.to_str() {
            Some("--capture") => Ok(Mode::Capture),
            Some("--version" | "-v") => Ok(Mode::Version),
            Some("--help" | "-h") => Ok(Mode::Help),
            _ => Err(format!("unknown option '{}'", argument.to_string_lossy())),
        },
        [_, extra, ..] => Err(format!("unexpected extra argument '{}'", extra.to_string_lossy())),
    }
}

fn is_wayland_session(display: Option<&OsStr>, session_type: Option<&OsStr>) -> bool {
    display.is_some_and(|value| !value.is_empty())
        || session_type == Some(OsStr::new("wayland"))
}

fn run_capture() -> i32 {
    let display = std::env::var_os("WAYLAND_DISPLAY");
    let session_type = std::env::var_os("XDG_SESSION_TYPE");
    if !is_wayland_session(display.as_deref(), session_type.as_deref()) {
        error!("A Wayland session is required. Run MintShot inside Hyprland.");
        return 2;
    }

    match capture::take_partial_screenshot() {
        Ok(path) => {
            info!("Screenshot saved: {}", path);
            0
        }
        Err(CaptureError::Cancelled) => {
            info!("Screenshot cancelled by user.");
            0
        }
        Err(CaptureError::ScreenCaptureFailed(message)) => {
            error!("Screen capture failed: {}", message);
            error!("Check grim/slurp and Hyprland's screencopy/layer-shell support.");
            3
        }
        Err(CaptureError::SaveFailed(message)) => {
            error!("Failed to save screenshot: {}", message);
            error!("Check HOME, permissions and free space in ~/Pictures.");
            4
        }
    }
}

fn print_help() {
    println!(
        "\
MintShot v{} — Arch Linux + Hyprland (Wayland only)

USAGE:
  mintshot [--capture | --version | --help]

  (no args) / --capture   Select a region, save PNG and copy to clipboard
  --version / -v         Show version
  --help / -h            Show this help

REQUIRES:
  grim, slurp; wl-copy (wl-clipboard) for the image clipboard
  notify-send (libnotify) is optional

HYPRLAND SHORTCUT (~/.config/hypr/hyprland.conf):
  bind = CTRL SHIFT, S, exec, ~/.local/bin/mintshot --capture
  No daemon or autostart needed. Drag and release to capture; Escape cancels.

FILES:
  ~/Pictures/MintShot/   Unique timestamped PNG files

EXIT CODES:
  0   Success or cancelled
  1   Invalid option
  2   Wayland session not available
  3   Selection/capture failed
  4   File save failed

Clipboard failures do not discard the saved screenshot.",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_options_and_aliases_are_parsed() {
        assert_eq!(parse_args(&[]).unwrap(), Mode::Capture);
        for (argument, expected) in [
            ("--capture", Mode::Capture),
            ("--version", Mode::Version),
            ("-v", Mode::Version),
            ("--help", Mode::Help),
            ("-h", Mode::Help),
        ] {
            assert_eq!(parse_args(&[OsString::from(argument)]).unwrap(), expected);
        }
        assert!(parse_args(&[OsString::from("--daemon")]).is_err());
        assert!(parse_args(&[OsString::from("--capture"), OsString::from("--bad")]).is_err());
    }

    #[test]
    fn wayland_display_is_enough() {
        assert!(is_wayland_session(Some(OsStr::new("wayland-1")), None));
    }

    #[test]
    fn wayland_session_type_is_enough() {
        assert!(is_wayland_session(None, Some(OsStr::new("wayland"))));
    }

    #[test]
    fn headless_or_x11_sessions_are_rejected() {
        assert!(!is_wayland_session(None, None));
        assert!(!is_wayland_session(None, Some(OsStr::new("x11"))));
        assert!(!is_wayland_session(Some(OsStr::new("")), None));
    }
}
