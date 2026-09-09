//! Global hotkey registration using X11 GrabKey
//!
//! Event-driven: zero CPU usage when idle (no polling).
//! Hotkey: Ctrl+Shift+S
//!
//! FIXES APPLIED:
//!   #1  — Signal handler sets running=false (not true)
//!   #7  — attempt % 10 instead of is_multiple_of (stable Rust)
//!   #9  — XSetErrorHandler restored after use
//!   #18 — Ignorable state bits (NumLock/CapsLock/Mod5/held mouse buttons)
//!         no longer make a delivered KeyPress fail the modifier comparison
//!   — resolve_capture_executable() fallback chain instead of current_exe() only
//!   — Handles binary replacement during upgrade (no "file not found" after install)

use log::{error, info, warn};
use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags};
use std::os::unix::io::BorrowedFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use x11::keysym;
use x11::xlib;

// ─── Hotkey configuration ─────────────────────────────────────────────────────

const CTRL_SHIFT_MASK: u32 = xlib::ControlMask | xlib::ShiftMask;

/// State bits that must NOT invalidate a hotkey press (Fix #18).
///
/// The X server matches a passive key grab on *keyboard* modifiers only, so
/// the KeyPress is delivered even when one of these bits is set — but
/// `XKeyEvent.state` still carries them, and a strict
/// `state == CTRL_SHIFT_MASK` comparison would silently swallow the press.
///
///   Mod2Mask (0x10) — NumLock       LockMask (0x02) — CapsLock
///   Mod5Mask (0x80) — ISO Level3 / AltGr on some layouts
///   Button1Mask..Button5Mask (0x1F00) — a mouse button held down while
///                     the hotkey is pressed
///
/// Values verified against x11 crate v2.21.0 (the version pinned in
/// Cargo.lock): all are `c_uint`.
///
/// Mod1/Mod3/Mod4 (Alt, Mod3, Super) stay significant — Super+Ctrl+Shift+S
/// is a genuinely different combination.
const IGNORABLE_STATE_MASK: u32 = xlib::Mod2Mask
    | xlib::LockMask
    | xlib::Mod5Mask
    | xlib::Button1Mask
    | xlib::Button2Mask
    | xlib::Button3Mask
    | xlib::Button4Mask
    | xlib::Button5Mask;

/// Maximum time to wait for X display to become available (60 seconds).
/// Handles daemon started before X server is ready (e.g. early boot via
/// systemd user service or XDG autostart with delay).
const DISPLAY_WAIT_TIMEOUT_SECS: u64 = 60;

// ─── X error tracking ────────────────────────────────────────────────────────

/// Number of X errors delivered while registering the hotkey.
/// XGrabKey() ALWAYS returns 1 (even on conflict) — the real result arrives
/// asynchronously as a BadAccess error through the error handler. So we count
/// errors instead of trusting the return value.
static X_ERRORS: AtomicUsize = AtomicUsize::new(0);

/// Count X errors instead of letting Xlib's default handler exit(1).
/// This lets us detect BadAccess (hotkey conflict) gracefully.
extern "C" fn count_x_error(_display: *mut xlib::Display, error: *mut xlib::XErrorEvent) -> i32 {
    unsafe {
        let code = (*error).error_code;
        if code == xlib::BadAccess {
            warn!("X BadAccess error while grabbing hotkey (code {})", code);
        }
    }
    X_ERRORS.fetch_add(1, Ordering::Relaxed);
    0
}

// ─── Error type ───────────────────────────────────────────────────────────────

/// Error kinds so the caller can distinguish a hotkey conflict (another app
/// already owns the key) from a real failure. A conflict is an expected
/// condition — NOT an error that should trigger a restart loop.
#[derive(Debug)]
pub enum HotkeyError {
    Conflict,
    Other(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => write!(f, "hotkey already in use by another application"),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for HotkeyError {}

// ─── Executable resolver ──────────────────────────────────────────────────────

/// Resolve the path to the MintShot executable for spawning `--capture`.
///
/// After an upgrade, `std::env::current_exe()` may point to a deleted binary
/// because the daemon was started from the old binary before `install.sh`
/// replaced it via mv. This function tries multiple fallback strategies:
///
///   1. `current_exe()` — works if binary wasn't replaced
///   2. `argv[0]`       — works if daemon was started with absolute path
///   3. Well-known install locations:
///      - `~/.local/bin/mintshot`  (user install)
///      - `/usr/bin/mintshot`      (system .deb install)
///      - `/usr/local/bin/mintshot`
///   4. `$PATH` lookup via `which`
fn resolve_capture_executable() -> Result<PathBuf, String> {
    // 1. Try current_exe() — most common case
    if let Ok(path) = std::env::current_exe() {
        // On Linux, current_exe() reads /proc/self/exe.
        // After binary replacement, this may show "(deleted)" in readlink
        // output, but the path itself may still be valid if the new binary
        // is at the same location.
        if path.exists() {
            return Ok(path);
        }

        // The exe was replaced — try the same path without "(deleted)"
        let path_str = path.to_string_lossy();
        let cleaned = path_str.trim_end_matches(" (deleted)");
        let cleaned_path = PathBuf::from(cleaned);
        if cleaned_path.exists() {
            info!("current_exe() was stale, using cleaned path: {}", cleaned);
            return Ok(cleaned_path);
        }

        warn!(
            "current_exe() points to missing path: {} — trying fallbacks",
            path.display()
        );
    }

    // 2. Try argv[0] — may be an absolute path
    if let Some(arg0) = std::env::args().next() {
        let p = PathBuf::from(&arg0);

        // Absolute path
        if p.is_absolute() && p.exists() {
            info!("Resolved via argv[0]: {}", p.display());
            return Ok(p);
        }

        // Relative path with directory component (e.g. ./target/release/mintshot)
        if arg0.contains('/') {
            if let Ok(abs) = std::fs::canonicalize(&p) {
                if abs.exists() {
                    info!("Resolved via argv[0] canonicalize: {}", abs.display());
                    return Ok(abs);
                }
            }
        }
    }

    // 3. Well-known install locations
    let mut candidates = Vec::with_capacity(4);

    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(&home).join(".local/bin/mintshot"));
    }
    candidates.push(PathBuf::from("/usr/bin/mintshot"));
    candidates.push(PathBuf::from("/usr/local/bin/mintshot"));

    for candidate in &candidates {
        if candidate.exists() {
            info!("Resolved via known location: {}", candidate.display());
            return Ok(candidate.clone());
        }
    }

    // 4. Last resort: search $PATH
    if let Ok(output) = std::process::Command::new("which").arg("mintshot").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                let p = PathBuf::from(&path_str);
                if p.exists() {
                    info!("Resolved via 'which': {}", p.display());
                    return Ok(p);
                }
            }
        }
    }

    Err(format!(
        "Cannot find mintshot executable. Tried: current_exe, argv[0], {:?}, $PATH",
        candidates
    ))
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Listen for the Ctrl+Shift+S global hotkey.
///
/// Blocks until `running` is set to false (via signal handler) or the
/// X display connection is lost.
pub fn listen_hotkey(running: Arc<AtomicBool>) -> Result<(), HotkeyError> {
    // ─── Wait for X display to be available ───────────────────────────────
    let display = wait_for_display(DISPLAY_WAIT_TIMEOUT_SECS)
        .map_err(|e| HotkeyError::Other(e.to_string()))?;

    unsafe {
        // Install our error counter (Fix #9: save previous handler)
        let prev_handler = xlib::XSetErrorHandler(Some(count_x_error));

        let root = xlib::XDefaultRootWindow(display);

        // Get keycode for 'S'
        let keycode = xlib::XKeysymToKeycode(display, keysym::XK_s as u64);
        if keycode == 0 {
            xlib::XSetErrorHandler(prev_handler);
            xlib::XCloseDisplay(display);
            return Err(HotkeyError::Other("Cannot get keycode for 'S'".into()));
        }

        info!("Registering hotkey Ctrl+Shift+S (keycode={})", keycode);

        // Grab with all modifier combinations (NumLock, CapsLock, both)
        let modifiers = [
            CTRL_SHIFT_MASK,
            CTRL_SHIFT_MASK | xlib::Mod2Mask, // NumLock
            CTRL_SHIFT_MASK | xlib::LockMask, // CapsLock
            CTRL_SHIFT_MASK | xlib::Mod2Mask | xlib::LockMask, // Both
        ];

        // Reset error counter, do all grabs, sync, then check for BadAccess.
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

        // Restore previous error handler (Fix #9)
        xlib::XSetErrorHandler(prev_handler);

        // Check grab results
        let grab_errors = X_ERRORS.load(Ordering::Relaxed);
        if grab_errors > 0 {
            if grab_errors >= modifiers.len() {
                // ALL grabs failed — another app owns Ctrl+Shift+S completely
                xlib::XCloseDisplay(display);
                return Err(HotkeyError::Conflict);
            }
            warn!(
                "{} of {} hotkey combos grabbed by another app — \
                 hotkey may not work with NumLock/CapsLock active",
                grab_errors,
                modifiers.len()
            );
        }

        info!("✓ Global hotkey Ctrl+Shift+S registered successfully");
        info!("Daemon is ready. Press Ctrl+Shift+S to take a screenshot.");

        // ─── Resolve capture executable once at startup ────────────────────
        // Also re-resolve on spawn failure (handles upgrade mid-session).
        let mut capture_exe = resolve_capture_executable().ok();
        if let Some(ref exe) = capture_exe {
            info!("Capture executable: {}", exe.display());
        } else {
            warn!("Could not resolve capture executable at startup — will retry on hotkey");
        }

        // ─── Event loop ────────────────────────────────────────────────────
        let conn_fd = BorrowedFd::borrow_raw(xlib::XConnectionNumber(display));
        let mut event: xlib::XEvent = std::mem::zeroed();

        // Single-flight guard: abaikan hotkey selama satu capture masih hidup.
        // Menangkal X key auto-repeat (tahan tombol = N overlay bertumpuk)
        // dan double-press yang cepat.
        let capture_running = Arc::new(AtomicBool::new(false));

        'event_loop: while running.load(Ordering::Relaxed) {
            let mut pfd = PollFd::new(&conn_fd, PollFlags::POLLIN);

            // Check Xlib internal queue FIRST — events may already be
            // buffered from XSync, in which case poll() would block forever.
            let poll_result = if xlib::XEventsQueued(display, 0) > 0 {
                Ok(1)
            } else {
                poll(std::slice::from_mut(&mut pfd), 1000) // 1s timeout
            };

            match poll_result {
                // Timeout — loop around to re-check running flag
                Ok(0) => continue,

                Ok(_) => {
                    let revents = pfd.revents().unwrap_or(PollFlags::empty());
                    if revents
                        .intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                    {
                        error!("X display connection lost — exiting");
                        break 'event_loop;
                    }

                    // Drain all pending events
                    while xlib::XPending(display) > 0 {
                        xlib::XNextEvent(display, &mut event);

                        if event.get_type() == xlib::KeyPress {
                            let key_event = event.key;
                            let clean_state = key_event.state & !IGNORABLE_STATE_MASK;

                            if key_event.keycode == keycode as u32 && clean_state == CTRL_SHIFT_MASK
                            {
                                if capture_running.swap(true, Ordering::SeqCst) {
                                    info!("Hotkey ignored — capture already in progress");
                                } else {
                                    info!("🎯 Hotkey Ctrl+Shift+S detected — spawning capture...");
                                    spawn_capture(&mut capture_exe, Arc::clone(&capture_running));
                                }
                            }
                        }
                    }
                }

                // Signal interrupted poll (SIGINT/SIGTERM) — re-check running
                Err(Errno::EINTR) => continue,

                Err(e) => {
                    error!("poll() error: {} — exiting", e);
                    break 'event_loop;
                }
            }
        }

        // ─── Cleanup ───────────────────────────────────────────────────────
        info!("Cleaning up hotkey grabs...");
        for &modifier in &modifiers {
            xlib::XUngrabKey(display, keycode as i32, modifier, root);
        }
        xlib::XSync(display, xlib::False);
        xlib::XCloseDisplay(display);
    }

    Ok(())
}

// ─── Spawn capture subprocess ────────────────────────────────────────────────

/// Spawn `mintshot --capture` as a child process.
///
/// If the cached executable path fails (e.g. binary was replaced during
/// upgrade), re-resolves the path and retries once.
///
/// `capture_running` is cleared when the child exits (or immediately if
/// spawn fails), so the next hotkey press is accepted again.
fn spawn_capture(cached_exe: &mut Option<PathBuf>, capture_running: Arc<AtomicBool>) {
    // First attempt: use cached path
    if let Some(ref exe) = cached_exe {
        match spawn_and_reap(exe, Arc::clone(&capture_running)) {
            Ok(pid) => {
                info!("Capture process spawned (pid {})", pid);
                return;
            }
            Err(e) => {
                warn!(
                    "Cached exe {} failed: {} — re-resolving...",
                    exe.display(),
                    e
                );
            }
        }
    }

    // Re-resolve and retry
    match resolve_capture_executable() {
        Ok(new_exe) => {
            info!("Re-resolved capture exe: {}", new_exe.display());
            match spawn_and_reap(&new_exe, Arc::clone(&capture_running)) {
                Ok(pid) => {
                    info!("Capture process spawned (pid {})", pid);
                    // Update cache for next time
                    *cached_exe = Some(new_exe);
                }
                Err(e) => {
                    error!("Failed to spawn capture from {}: {}", new_exe.display(), e);
                    *cached_exe = None;
                    // Spawn gagal — izinkan hotkey berikutnya langsung retry.
                    capture_running.store(false, Ordering::SeqCst);
                }
            }
        }
        Err(e) => {
            error!("Cannot find mintshot executable: {}", e);
            error!("Try reinstalling: bash install.sh");
            *cached_exe = None;
            capture_running.store(false, Ordering::SeqCst);
        }
    }
}

/// Spawn satu capture child dan reap di detached thread.
///
/// Tanpa ini tiap capture meninggalkan zombie (daemon tidak pernah wait()).
/// Thread memblokir di `wait()` sampai capture selesai (sukses/cancel/crash),
/// lalu membuka kembali single-flight guard.
fn spawn_and_reap(exe: &PathBuf, capture_running: Arc<AtomicBool>) -> std::io::Result<u32> {
    let mut child = std::process::Command::new(exe).arg("--capture").spawn()?;
    let pid = child.id();
    thread::spawn(move || {
        match child.wait() {
            Ok(status) if status.success() => {
                info!("Capture process {} finished", pid);
            }
            Ok(status) => {
                // Cancel keluar 0 — non-zero berarti error betulan.
                warn!("Capture process {} exited: {}", pid, status);
            }
            Err(e) => {
                warn!("Failed to wait for capture process {}: {}", pid, e);
            }
        }
        capture_running.store(false, Ordering::SeqCst);
    });
    Ok(pid)
}

// ─── Wait for X display ──────────────────────────────────────────────────────

/// Wait for X display to become available, with exponential backoff.
///
/// Returns the opened display pointer, or an error if timeout is reached.
///
/// Critical for systemd user service or XDG autostart — the daemon may
/// start before the X server is fully initialized.
fn wait_for_display(timeout_secs: u64) -> Result<*mut xlib::Display, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    let mut attempt = 0u32;

    loop {
        let display_env = std::env::var("DISPLAY").unwrap_or_default();

        if !display_env.is_empty() {
            unsafe {
                let display = xlib::XOpenDisplay(std::ptr::null());
                if !display.is_null() {
                    if attempt > 0 {
                        info!(
                            "✓ X display opened after {} attempts ({:?})",
                            attempt + 1,
                            start.elapsed()
                        );
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
            )
            .into());
        }

        // Log periodically (Fix #7: plain modulo, not `is_multiple_of` —
        // that method only exists on recent stable, and the comment above
        // already claimed we avoid it).
        if attempt == 1 {
            info!("Waiting for X display to become available...");
            info!("DISPLAY env: '{}'", display_env);
        } else if attempt % 10 == 0 {
            info!("Still waiting for X display... ({}s elapsed)", elapsed);
        }

        // Exponential backoff: 100ms → 200ms → 400ms → … → max 2s
        let wait_ms = (100u64 * (1u64 << attempt.min(4))).min(2000);
        thread::sleep(Duration::from_millis(wait_ms));
    }
}
