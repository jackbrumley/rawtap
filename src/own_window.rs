//! Windows: gives RawTap its own console window, taskbar button and icon.
//!
//! When Windows Terminal is the default terminal, Windows hands every new
//! console to it, so RawTap opened from the Start menu, Explorer or the
//! updater would be a Windows Terminal tab. In that case RawTap starts itself
//! again in a classic console window (`conhost.exe`) and exits. The installers'
//! Start menu shortcuts start RawTap through `conhost.exe` already, so this
//! only covers the other ways in.
//!
//! A console window started through `conhost.exe` shows conhost's own icon,
//! not RawTap's, so RawTap then puts its icon on the window itself; the
//! taskbar button takes it from there.
//!
//! In a window of its own, RawTap also opens at a comfortable size (the
//! console default, 120×30, cuts the start screen short and is too narrow
//! for the split view).
//!
//! RawTap started from a shell shares that shell's console and stays in it,
//! and leaves that window's icon and size alone.

/// The size RawTap's own window grows to, when the screen has room: wide
/// enough for the split view (130 columns), tall enough for the start screen
/// with a few favourites and ports.
const OWN_WINDOW_COLUMNS: u16 = 140;
const OWN_WINDOW_ROWS: u16 = 42;

/// Set on the restarted RawTap, so it can never start itself again.
const RESTARTED_ENV: &str = "RAWTAP_OWN_WINDOW";

/// What `ensure` did.
pub enum Outcome {
    /// Carry on in this console.
    Stay,
    /// RawTap is starting in its own window; this process should exit.
    Restarted,
    /// Something went wrong (the restart, the icon or the size): carry on
    /// here, and say why.
    Failed(String),
}

/// Restarts RawTap in its own console window when this console was made for
/// RawTap alone and handed to Windows Terminal; in a classic console window
/// of its own, shows RawTap's icon on it and makes it a comfortable size.
pub fn ensure() -> Outcome {
    let Some((window, class)) = own_console_window() else {
        return Outcome::Stay;
    };
    if class == "PseudoConsoleWindow" && std::env::var_os(RESTARTED_ENV).is_none() {
        return match restart_in_conhost() {
            Ok(()) => Outcome::Restarted,
            Err(error) => Outcome::Failed(format!(
                "Could not open RawTap in its own window, so it's running in this terminal: {error}"
            )),
        };
    }
    if class != "ConsoleWindowClass" {
        return Outcome::Stay;
    }
    if let Err(error) = show_icon(window) {
        return Outcome::Failed(format!(
            "Could not show RawTap's icon on its window: {error}"
        ));
    }
    if let Err(error) = grow_window(window) {
        return Outcome::Failed(format!("Could not make RawTap's window bigger: {error}"));
    }
    Outcome::Stay
}

/// Grows the window to `OWN_WINDOW_COLUMNS` × `OWN_WINDOW_ROWS`, never
/// shrinking it and never past what fits on the screen, then moves it back
/// onto the screen's work area if it now runs off the bottom or right.
fn grow_window(window: windows_sys::Win32::Foundation::HWND) -> Result<(), String> {
    use ratatui::crossterm::terminal::{SetSize, size};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::Console::{
        GetLargestConsoleWindowSize, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };

    let (columns, rows) = size().map_err(|e| format!("can't read its size: {e}"))?;
    // SAFETY: plain handle lookups; the largest size is 0×0 on failure.
    let largest = unsafe { GetLargestConsoleWindowSize(GetStdHandle(STD_OUTPUT_HANDLE)) };
    let fit = |want: u16, largest: i16| want.min(u16::try_from(largest).unwrap_or(0));
    let target = (
        columns.max(fit(OWN_WINDOW_COLUMNS, largest.X)),
        rows.max(fit(OWN_WINDOW_ROWS, largest.Y)),
    );
    if target == (columns, rows) {
        return Ok(());
    }
    ratatui::crossterm::execute!(std::io::stdout(), SetSize(target.0, target.1))
        .map_err(|e| format!("can't resize it to {}×{}: {e}", target.0, target.1))?;

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: rect,
        rcWork: rect,
        dwFlags: 0,
    };
    // SAFETY: `window` is the console window; the structs are sized for the calls.
    let found = unsafe {
        GetWindowRect(window, &mut rect) != 0
            && GetMonitorInfoW(
                MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) != 0
    };
    if !found {
        return Ok(());
    }
    let work = monitor.rcWork;
    let shift_x = (rect.right - work.right).clamp(0, (rect.left - work.left).max(0));
    let shift_y = (rect.bottom - work.bottom).clamp(0, (rect.top - work.top).max(0));
    if shift_x > 0 || shift_y > 0 {
        // SAFETY: moves the console window only; size and z-order unchanged.
        unsafe {
            SetWindowPos(
                window,
                std::ptr::null_mut(),
                rect.left - shift_x,
                rect.top - shift_y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }
    Ok(())
}

/// The console window and its class name, when RawTap is the only process on
/// its console (nobody started it from a shell).
fn own_console_window() -> Option<(windows_sys::Win32::Foundation::HWND, String)> {
    use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut processes = [0u32; 2];
    // SAFETY: the buffer and its length match.
    let count = unsafe { GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) };
    if count != 1 {
        return None;
    }
    // SAFETY: no arguments; returns null when there's no console window.
    let window = unsafe { GetConsoleWindow() };
    if window.is_null() {
        return None;
    }
    let mut class = [0u16; 64];
    // SAFETY: the buffer and its length match; `window` is a valid handle.
    let len = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    let len = usize::try_from(len).unwrap_or(0);
    Some((window, String::from_utf16_lossy(&class[..len])))
}

/// Puts rawtap.exe's icon on the console window, large and small. The icons
/// stay loaded for the rest of the run, while the window shows them.
fn show_icon(window: windows_sys::Win32::Foundation::HWND) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ICON_BIG, ICON_SMALL, SendMessageW, WM_SETICON,
    };

    let exe = std::env::current_exe().map_err(|e| format!("can't find rawtap.exe: {e}"))?;
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let mut large = std::ptr::null_mut();
    let mut small = std::ptr::null_mut();
    // SAFETY: `path` is null-terminated; each out pointer receives one icon.
    unsafe { ExtractIconExW(path.as_ptr(), 0, &mut large, &mut small, 1) };
    if large.is_null() || small.is_null() {
        return Err(format!("{} has no icon", exe.display()));
    }
    // SAFETY: `window` is the console window; the icons are valid handles.
    unsafe {
        SendMessageW(window, WM_SETICON, ICON_BIG as usize, large as isize);
        SendMessageW(window, WM_SETICON, ICON_SMALL as usize, small as isize);
    }
    Ok(())
}

fn restart_in_conhost() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find rawtap.exe: {e}"))?;
    let system_root = std::env::var_os("SystemRoot").ok_or("the SystemRoot variable isn't set")?;
    let conhost = std::path::Path::new(&system_root)
        .join("System32")
        .join("conhost.exe");
    std::process::Command::new(&conhost)
        .arg(exe)
        .args(std::env::args_os().skip(1))
        .env(RESTARTED_ENV, "1")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("can't start {}: {e}", conhost.display()))
}
