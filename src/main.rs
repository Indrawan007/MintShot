//! MintShot — Lightweight Partial Screenshot Tool for Linux Mint
//!
//! Hotkey : Ctrl+Shift+S
//! Modes  : direct capture (default) | background daemon (--daemon)

mod capture;
mod clipboard;
mod hotkey;
mod overlay;
mod save;
mod selection;

use log::{error, info, warn};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp(None)
    .format_module_path(false)
    .init();

    info!(
        "MintShot {} — Partial Screenshot Tool",
        env!("CARGO_PKG_VERSION")
    );

    // Validate display server before doing anything
    check_display_server();

    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        // Explicit capture or no args → take screenshot
        Some("--capture") | None => run_capture(),

        // Background hotkey daemon
        Some("--daemon") => run_daemon(),

        // Version info
        Some("--version") | Some("-v") => {
            println!("MintShot v{}", env!("CARGO_PKG_VERSION"));
            println!("Lightweight partial screenshot tool for Linux");
            process::exit(0);
        }

        // Help
        Some("--help") | Some("-h") => {
            print_help();
            process::exit(0);
        }

        // Unknown argument — do not silently ignore
        Some(unknown) => {
            eprintln!("mintshot: unknown option '{}'", unknown);
            eprintln!("Run 'mintshot --help' for usage.");
            process::exit(1);
        }
    }
}

// ─── Display server check ──────────────────────────────────────────────────

fn check_display_server() {
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    let x11     = std::env::var("DISPLAY").is_ok();

    match (wayland, x11) {
        (true, false) => {
            eprintln!("ERROR: Pure Wayland session detected.");
            eprintln!("MintShot requires X11 or XWayland.");
            eprintln!("Hint: Enable XWayland, or set DISPLAY=:0");
            process::exit(1);
        }
        (true, true) => {
            warn!("Wayland + XWayland detected — using XWayland mode.");
        }
        (false, true) => {
            info!("X11 session detected.");
        }
        (false, false) => {
            eprintln!("ERROR: No display server detected.");
            eprintln!("DISPLAY and WAYLAND_DISPLAY are both unset.");
            process::exit(1);
        }
    }
}

// ─── Capture mode ─────────────────────────────────────────────────────────

/// Take one screenshot and exit.
fn run_capture() {
    info!("Starting capture session...");

    match capture::take_partial_screenshot() {
        // ── Success ───────────────────────────────────────────────────────
        Ok(path) => {
            info!("Screenshot saved: {}", path);
            process::exit(0);
        }

        // ── User cancelled — normal exit ──────────────────────────────────
        Err(capture::CaptureError::Cancelled) => {
            info!("Screenshot cancelled by user.");
            process::exit(0);
        }

        // ── X display not available ────────────────────────────────────────
        Err(capture::CaptureError::DisplayNotFound(msg)) => {
            error!("Display not available: {}", msg);
            error!("Make sure DISPLAY is set and X server is running.");
            process::exit(2);
        }

        // ── Screen capture failed ──────────────────────────────────────────
        Err(capture::CaptureError::ScreenCaptureFailed(msg)) => {
            error!("Screen capture failed: {}", msg);
            error!("Try: DISPLAY=:0 mintshot");
            process::exit(3);
        }

        // ── PNG save failed ────────────────────────────────────────────────
        Err(capture::CaptureError::SaveFailed(msg)) => {
            error!("Failed to save screenshot: {}", msg);
            error!("Check disk space: df -h ~/Pictures");
            process::exit(4);
        }

        // ── Any other error ────────────────────────────────────────────────
        Err(e) => {
            error!("Screenshot failed: {}", e);
            process::exit(1);
        }
    }
}

// ─── Help text ────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        "MintShot v{} — Lightweight Partial Screenshot Tool",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("  mintshot [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  (no args)    Take a screenshot immediately");
    println!("  --capture    Same as no args");
    println!("  --daemon     Run as background hotkey listener");
    println!("  --version    Show version");
    println!("  --help       Show this help");
    println!();
    println!("HOTKEY (daemon mode):");
    println!("  Ctrl+Shift+S    Take screenshot");
    println!();
    println!("CONTROLS (during capture):");
    println!("  Click+Drag      Select region");
    println!("  Release         Confirm & save");
    println!("  Enter           Confirm current selection");
    println!("  ESC / Q         Cancel");
    println!("  Right Click     Cancel");
    println!();
    println!("FILES:");
    println!("  ~/Pictures/MintShot/    Screenshot save directory");
    println!();
    println!("EXIT CODES:");
    println!("  0    Success or user cancelled");
    println!("  1    General error");
    println!("  2    X display not available");
    println!("  3    Screen capture failed");
    println!("  4    File save failed");
    println!();
    println!("Screenshots are auto-copied to clipboard (Ctrl+V ready).");
}

// ─── Daemon mode ──────────────────────────────────────────────────────────

/// Run as background daemon listening for Ctrl+Shift+S.
fn run_daemon() {
    let running = Arc::new(AtomicBool::new(true));

    // Signal handlers — set flag to FALSE so event loop exits cleanly
    let r1 = Arc::clone(&running);
    let r2 = Arc::clone(&running);

    unsafe {
        signal_hook::low_level::register(
            signal_hook::consts::SIGINT,
            move || { r1.store(false, Ordering::SeqCst); },
        )
        .expect("Failed to register SIGINT handler");

        signal_hook::low_level::register(
            signal_hook::consts::SIGTERM,
            move || { r2.store(false, Ordering::SeqCst); },
        )
        .expect("Failed to register SIGTERM handler");
    }

    info!("MintShot daemon started. Listening for Ctrl+Shift+S…");
    info!("Send SIGINT (Ctrl+C) or SIGTERM to stop.");

    match hotkey::listen_hotkey(running) {
        Ok(()) => {
            info!("MintShot daemon stopped cleanly.");
        }
        Err(hotkey::HotkeyError::Conflict) => {
            // Another app owns Ctrl+Shift+S — expected condition.
            // Exit 0 so a supervisor does not restart us in a loop.
            warn!(
                "Ctrl+Shift+S is already bound by another application."
            );
            warn!("Daemon exits without retry.");
        }
        Err(hotkey::HotkeyError::Other(e)) => {
            error!("Hotkey listener error: {}", e);
            process::exit(1);
        }
    }
}
