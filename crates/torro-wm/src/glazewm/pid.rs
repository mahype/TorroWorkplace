//! The process of a window, found from its window handle.
//!
//! Task X.8 in `docs/tasks/plattformen.md`. GlazeWM's `WindowDto` has no process id, only the
//! `HWND` as `handle`. Windows answers "which process owns this window" with
//! `GetWindowThreadProcessId`; no safe crate wraps it, so this file holds the one FFI call of
//! `torro-wm` (see the lint note in `Cargo.toml`).

/// The id of the process that owns the window `handle` (an `HWND`); `None` when the window no
/// longer exists or the handle is not one.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn process_id(handle: isize) -> Option<u32> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid: u32 = 0;
    // SAFETY: The call only reads the handle value and writes one `u32` through the pointer, which
    // points to a live local. A stale or invalid handle is no undefined behaviour: the call
    // returns 0 and leaves `pid` at 0.
    let thread = unsafe { GetWindowThreadProcessId(handle as HWND, &mut pid) };
    (thread != 0 && pid != 0).then_some(pid)
}

/// The id of the process that owns the window `handle`; always `None` here, since only Windows
/// has window handles. GlazeWM runs nowhere else, so this exists to keep the backend compiling
/// (and its tests running) on every system.
#[cfg(not(windows))]
pub fn process_id(handle: isize) -> Option<u32> {
    let _ = handle;
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_handle_that_is_no_window_has_no_process() {
        assert_eq!(process_id(0), None);
        assert_eq!(process_id(-1), None);
    }
}
