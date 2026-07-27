//! Global hotkey registration using X11 GrabKey
//!
//! Event-driven: zero CPU usage when idle (no polling).
//! Hotkey: Ctrl+Shift+S

use log::{error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use x11::keysym;
use x11::xlib;

/// Modifier mask for Ctrl+Shift
const CTRL_SHIFT_MASK: u32 = xlib::ControlMask | xlib::ShiftMask;

/// Listen for the Ctrl+Shift+S global hotkey.
///
/// Blocks on XNextEvent — zero CPU when idle.
/// Spawns a capture subprocess on each hotkey press.
pub fn listen_hotkey(running: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let display = xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err("Cannot open X display".into());
        }

        let root = xlib::XDefaultRootWindow(display);

        // Keycode for 'S'
        let keycode = xlib::XKeysymToKeycode(display, keysym::XK_s as u64);
        if keycode == 0 {
            xlib::XCloseDisplay(display);
            return Err("Cannot get keycode for 'S'".into());
        }

        // Grab with NumLock / CapsLock variants so the hotkey works regardless
        let modifiers = [
            CTRL_SHIFT_MASK,
            CTRL_SHIFT_MASK | xlib::Mod2Mask,                   // + NumLock
            CTRL_SHIFT_MASK | xlib::LockMask,                    // + CapsLock
            CTRL_SHIFT_MASK | xlib::Mod2Mask | xlib::LockMask,  // + both
        ];

        for &modifier in &modifiers {
            let result = xlib::XGrabKey(
                display,
                keycode as i32,
                modifier,
                root,
                xlib::True,
                xlib::GrabModeAsync,
                xlib::GrabModeAsync,
            );
            if result == 0 {
                error!("Could not grab key with modifier mask {}", modifier);
            }
        }

        xlib::XSync(display, xlib::False);
        info!("Global hotkey Ctrl+Shift+S registered successfully");

        let mut event: xlib::XEvent = std::mem::zeroed();

        while running.load(Ordering::Relaxed) {
            if xlib::XPending(display) > 0 {
                xlib::XNextEvent(display, &mut event);

                if event.get_type() == xlib::KeyPress {
                    let key_event = event.key;
                    // Strip NumLock / CapsLock before comparing
                    let clean_state =
                        key_event.state & !(xlib::Mod2Mask | xlib::LockMask);

                    if key_event.keycode == keycode as u32
                        && clean_state == CTRL_SHIFT_MASK
                    {
                        info!("Hotkey Ctrl+Shift+S detected — spawning capture...");

                        let exe = std::env::current_exe()?;
                        match std::process::Command::new(exe)
                            .arg("--capture")
                            .spawn()
                        {
                            Ok(child) => info!("Capture process spawned (pid {})", child.id()),
                            Err(e)    => error!("Failed to spawn capture process: {}", e),
                        }
                    }
                }
            } else {
                // Brief sleep — keeps daemon responsive without burning CPU
                thread::sleep(Duration::from_millis(50));
            }
        }

        // Cleanup grabs
        for &modifier in &modifiers {
            xlib::XUngrabKey(display, keycode as i32, modifier, root);
        }
        xlib::XCloseDisplay(display);
    }

    Ok(())
}
