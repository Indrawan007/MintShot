//! Global hotkey registration using X11 GrabKey
//!
//! Event-driven: zero CPU usage when idle (no polling).
//! Hotkey: Ctrl+Shift+S
//!
//! v1.1.2: Added display retry logic for early boot startup

use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use x11::keysym;
use x11::xlib;

const CTRL_SHIFT_MASK: u32 = xlib::ControlMask | xlib::ShiftMask;

/// Maximum time to wait for X display to become available (60 seconds).
/// This handles the case where the daemon is started before the X server
/// is ready (e.g. during early boot via systemd user service).
const DISPLAY_WAIT_TIMEOUT_SECS: u64 = 60;

/// Listen for the Ctrl+Shift+S global hotkey.
pub fn listen_hotkey(running: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    // ─── Wait for X display to be available ───────────────────────────────
    let display = wait_for_display(DISPLAY_WAIT_TIMEOUT_SECS)?;

    unsafe {
        let root = xlib::XDefaultRootWindow(display);

        // Get keycode for 'S'
        let keycode = xlib::XKeysymToKeycode(display, keysym::XK_s as u64);
        if keycode == 0 {
            xlib::XCloseDisplay(display);
            return Err("Cannot get keycode for 'S'".into());
        }

        info!("Registering hotkey Ctrl+Shift+S (keycode={})", keycode);

        // Grab with all modifier combinations
        let modifiers = [
            CTRL_SHIFT_MASK,
            CTRL_SHIFT_MASK | xlib::Mod2Mask,
            CTRL_SHIFT_MASK | xlib::LockMask,
            CTRL_SHIFT_MASK | xlib::Mod2Mask | xlib::LockMask,
        ];

        let mut grab_success = false;
        for &modifier in &modifiers {
            // Set X error handler to catch grab conflicts
            let result = xlib::XGrabKey(
                display,
                keycode as i32,
                modifier,
                root,
                xlib::True,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
            );

            if result != 0 {
                grab_success = true;
            } else {
                warn!("Could not grab key with modifier mask {}", modifier);
            }
        }

        if !grab_success {
            xlib::XCloseDisplay(display);
            return Err("Failed to grab hotkey — another app may have Ctrl+Shift+S bound".into());
        }

        xlib::XSync(display, xlib::False);
        info!("✓ Global hotkey Ctrl+Shift+S registered successfully");
        info!("Daemon is ready. Press Ctrl+Shift+S to take a screenshot.");

        // ─── Event loop ────────────────────────────────────────────────────
        let mut event: xlib::XEvent = std::mem::zeroed();
        let mut consecutive_errors = 0u32;

        while running.load(Ordering::Relaxed) {
            let pending = xlib::XPending(display);

            if pending > 0 {
                consecutive_errors = 0;
                xlib::XNextEvent(display, &mut event);

                if event.get_type() == xlib::KeyPress {
                    let key_event = event.key;
                    let clean_state = key_event.state & !(xlib::Mod2Mask | xlib::LockMask);

                    if key_event.keycode == keycode as u32
                        && clean_state == CTRL_SHIFT_MASK
                    {
                        info!("🎯 Hotkey Ctrl+Shift+S detected — spawning capture...");

                        match std::env::current_exe() {
                            Ok(exe) => {
                                match std::process::Command::new(&exe)
                                    .arg("--capture")
                                    .spawn()
                                {
                                    Ok(child) => {
                                        info!("Capture process spawned (pid {})", child.id());
                                    }
                                    Err(e) => {
                                        error!("Failed to spawn capture: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Cannot get current exe path: {}", e);
                            }
                        }
                    }
                }
            } else {
                // Sleep briefly to avoid busy-waiting
                thread::sleep(Duration::from_millis(50));

                // Health check: verify display is still alive every ~5 seconds
                if consecutive_errors > 100 {
                    // Try a benign X operation to check connection
                    if xlib::XConnectionNumber(display) < 0 {
                        error!("X display connection lost!");
                        break;
                    }
                    consecutive_errors = 0;
                }
                consecutive_errors += 1;
            }
        }

        // Cleanup
        info!("Cleaning up hotkey grabs...");
        for &modifier in &modifiers {
            xlib::XUngrabKey(display, keycode as i32, modifier, root);
        }
        xlib::XSync(display, xlib::False);
        xlib::XCloseDisplay(display);
    }

    Ok(())
}

/// Wait for X display to become available, with exponential backoff.
///
/// Returns the opened display pointer, or an error if timeout is reached.
///
/// This is critical for systemd user service startup — the service may
/// start before the X server is fully initialized.
fn wait_for_display(timeout_secs: u64) -> Result<*mut xlib::Display, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let mut attempt = 0u32;

    loop {
        // Check DISPLAY environment variable exists
        let display_env = std::env::var("DISPLAY").unwrap_or_default();

        if !display_env.is_empty() {
            unsafe {
                let display = xlib::XOpenDisplay(std::ptr::null());
                if !display.is_null() {
                    if attempt > 0 {
                        info!("✓ X display opened after {} attempts ({:?})",
                              attempt + 1, start.elapsed());
                    } else {
                        info!("✓ X display opened: {}", display_env);
                    }
                    return Ok(display);
                }
            }
        }

        attempt += 1;
        let elapsed = start.elapsed().as_secs();

        if elapsed >= timeout_secs {
            return Err(format!(
                "X display not available after {} seconds (attempts: {}). \
                DISPLAY={}. Is the X server running?",
                timeout_secs, attempt, display_env
            ).into());
        }

        // Log periodically so user knows daemon is alive
        if attempt == 1 {
            info!("Waiting for X display to become available...");
            info!("DISPLAY env: '{}'", display_env);
        } else if attempt % 10 == 0 {
            info!("Still waiting for X display... ({}s elapsed)", elapsed);
        }

        // Exponential backoff: 100ms → 200ms → 400ms → ... → max 2s
        let wait_ms = (100u64 * (1u64 << attempt.min(4))).min(2000);
        thread::sleep(Duration::from_millis(wait_ms));
    }
}
