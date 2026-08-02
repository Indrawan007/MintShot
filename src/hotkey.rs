//! Global hotkey registration using X11 GrabKey
//!
//! Event-driven: zero CPU usage when idle (no polling).
//! Hotkey: Ctrl+Shift+S
//!
//! v1.1.2: Added display retry logic for early boot startup
//! v1.1.3: Detect BadAccess conflict via error handler (XGrabKey return is meaningless)

use log::{error, info, warn};
use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags};
use std::os::unix::io::BorrowedFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Number of X errors delivered while registering the hotkey.
/// XGrabKey() ALWAYS returns 1 (even on conflict) — the real result arrives
/// asynchronously as a BadAccess error through the error handler. So we count
/// errors instead of trusting the return value.
static X_ERRORS: AtomicUsize = AtomicUsize::new(0);

/// Xlib's default error handler exits the process on ANY X error — including
/// the expected `BadAccess` when Ctrl+Shift+S is already grabbed by another
/// client. Count errors instead so we can report the conflict gracefully.
extern "C" fn count_x_error(
    _display: *mut xlib::Display,
    error: *mut xlib::XErrorEvent,
) -> i32 {
    unsafe {
        let code = (*error).error_code;
        if code == xlib::BadAccess {
            warn!("X BadAccess error while grabbing hotkey (code {})", code);
        }
    }
    X_ERRORS.fetch_add(1, Ordering::Relaxed);
    0
}

/// Listen for the Ctrl+Shift+S global hotkey.
pub fn listen_hotkey(running: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    // ─── Wait for X display to be available ───────────────────────────────
    let display = wait_for_display(DISPLAY_WAIT_TIMEOUT_SECS)?;

    unsafe {
        xlib::XSetErrorHandler(Some(count_x_error));

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

        // NOTE: XGrabKey() returns 1 whether it succeeded or conflicted, so we
        // cannot trust its return value. Instead, reset the error counter, do
        // all grabs, sync, then check how many BadAccess errors were delivered.
        X_ERRORS.store(0, Ordering::Relaxed);
        for &modifier in &modifiers {
            xlib::XGrabKey(
                display,
                keycode as i32,
                modifier,
                root,
                xlib::True,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
            );
        }
        xlib::XSync(display, xlib::False);

        let grab_errors = X_ERRORS.load(Ordering::Relaxed);
        if grab_errors > 0 {
            if grab_errors >= modifiers.len() {
                xlib::XCloseDisplay(display);
                return Err(
                    "Failed to grab hotkey Ctrl+Shift+S — another app already has it bound"
                        .into(),
                );
            }
            warn!(
                "{} of {} hotkey combos are grabbed by another app — \
                 hotkey may not work with NumLock/CapsLock active",
                grab_errors,
                modifiers.len()
            );
        }

        info!("✓ Global hotkey Ctrl+Shift+S registered successfully");
        info!("Daemon is ready. Press Ctrl+Shift+S to take a screenshot.");

        // ─── Event loop ────────────────────────────────────────────────────
        // Block on poll() instead of busy-sleeping. POLLHUP/POLLERR/POLLNVAL
        // mean the X connection died (X server restarted / logged out) — the
        // old check `XConnectionNumber < 0` could never fire, so the daemon
        // used to hang forever in that case.
        let conn_fd = BorrowedFd::borrow_raw(xlib::XConnectionNumber(display));
        let mut event: xlib::XEvent = std::mem::zeroed();

        'event_loop: while running.load(Ordering::Relaxed) {
            let mut pfd = PollFd::new(&conn_fd, PollFlags::POLLIN);

            match poll(std::slice::from_mut(&mut pfd), 1000) {
                // Timeout — loop around to re-check the running flag.
                Ok(0) => continue,
                Ok(_) => {
                    let revents = pfd.revents().unwrap_or(PollFlags::empty());
                    if revents.intersects(
                        PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL,
                    ) {
                        error!("X display connection lost — exiting");
                        break 'event_loop;
                    }

                    // Drain all pending events, then go back to poll().
                    while xlib::XPending(display) > 0 {
                        xlib::XNextEvent(display, &mut event);

                        if event.get_type() == xlib::KeyPress {
                            let key_event = event.key;
                            let clean_state =
                                key_event.state & !(xlib::Mod2Mask | xlib::LockMask);

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
                                                info!("Capture process spawned (pid {})",
                                                      child.id());
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
                    }
                }
                // Signal interrupted the wait (e.g. SIGINT/SIGTERM) — loop
                // around so the running flag is re-checked for a clean exit.
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    error!("poll() error: {} — exiting", e);
                    break 'event_loop;
                }
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
        } else if attempt.is_multiple_of(10) {
            info!("Still waiting for X display... ({}s elapsed)", elapsed);
        }

        // Exponential backoff: 100ms → 200ms → 400ms → ... → max 2s
        let wait_ms = (100u64 * (1u64 << attempt.min(4))).min(2000);
        thread::sleep(Duration::from_millis(wait_ms));
    }
}
