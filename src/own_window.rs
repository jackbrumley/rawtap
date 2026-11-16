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
//! RawTap started from a shell shares that shell's console and stays in it,
//! and leaves that window's icon alone.

/// Set on the restarted RawTap, so it can never start itself again.
const RESTARTED_ENV: &str = "RAWTAP_OWN_WINDOW";

/// What `ensure` did.
pub enum Outcome {
    /// Carry on in this console.
    Stay,
    /// RawTap is starting in its own window; this process should exit.
    Restarted,
    /// Something went wrong (the restart or the icon): carry on here, and
    /// say why.
    Failed(String),
}

/// Restarts RawTap in its own console window when this console was made for
/// RawTap alone and handed to Windows Terminal; in a classic console window
/// of its own, shows RawTap's icon on it.
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
    if class == "ConsoleWindowClass"
        && let Err(error) = show_icon(window)
    {
        return Outcome::Failed(format!(
            "Could not show RawTap's icon on its window: {error}"
        ));
    }
    Outcome::Stay
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
