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

use log::{error, info};
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    // Minimal logger — no timestamps, no module paths
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp(None)
    .format_module_path(false)
    .init();

    info!("MintShot v1.0.0 — Partial Screenshot Tool");

    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--capture") => run_capture(),
        Some("--daemon")  => run_daemon(),
        _                 => run_capture(), // default: take screenshot now
    }
}

/// Take one screenshot and exit.
fn run_capture() {
    info!("Starting capture session...");
    match capture::take_partial_screenshot() {
        Ok(path) => info!("Screenshot saved: {}", path),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("cancelled") {
                info!("Screenshot cancelled by user");
            } else {
                error!("Screenshot failed: {}", e);
                process::exit(1);
            }
        }
    }
}

/// Run as a background daemon that listens for Ctrl+Shift+S.
fn run_daemon() {
    let running = Arc::new(AtomicBool::new(true));

    // React to SIGINT / SIGTERM by flipping the flag to false
    signal_hook::flag::register(
        signal_hook::consts::SIGINT,
        Arc::clone(&running),
    )
    .expect("Failed to register SIGINT handler");

    signal_hook::flag::register(
        signal_hook::consts::SIGTERM,
        Arc::clone(&running),
    )
    .expect("Failed to register SIGTERM handler");

    info!("MintShot daemon started. Listening for Ctrl+Shift+S…");
    info!("Send SIGINT (Ctrl+C) or SIGTERM to stop.");

    match hotkey::listen_hotkey(running) {
        Ok(())  => info!("MintShot daemon stopped cleanly."),
        Err(e)  => {
            error!("Hotkey listener error: {}", e);
            process::exit(1);
        }
    }
}
