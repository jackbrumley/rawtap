//! Windows: gives RawTap its own console window, taskbar button and icon.
//!
//! When Windows Terminal is the default terminal, Windows hands every new
//! console to it, so RawTap opened from the Start menu, Explorer or the
//! updater would be a Windows Terminal tab. In that case RawTap starts itself
//! again in a classic console window (`conhost.exe`) and exits. The installers'
//! Start menu shortcuts start RawTap through `conhost.exe` already, so this
//! only covers the other ways in.
//!
//! RawTap started from a shell shares that shell's console and stays in it.

/// Set on the restarted RawTap, so it can never start itself again.
const RESTARTED_ENV: &str = "RAWTAP_OWN_WINDOW";

/// What `ensure` did.
pub enum Outcome {
    /// Carry on in this console.
    Stay,
    /// RawTap is starting in its own window; this process should exit.
    Restarted,
    /// It should have restarted but couldn't: carry on here, and say why.
    Failed(String),
}

/// Restarts RawTap in its own console window when this console was made for
/// RawTap alone and handed to Windows Terminal.
pub fn ensure() -> Outcome {
    if std::env::var_os(RESTARTED_ENV).is_some() || !in_own_terminal_console() {
        return Outcome::Stay;
    }
    match restart_in_conhost() {
        Ok(()) => Outcome::Restarted,
        Err(error) => Outcome::Failed(format!(
            "Could not open RawTap in its own window, so it's running in this terminal: {error}"
        )),
    }
}

/// True when RawTap is the only process on its console (nobody started it
/// from a shell) and the console is a Windows Terminal pseudoconsole.
fn in_own_terminal_console() -> bool {
    use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut processes = [0u32; 2];
    // SAFETY: the buffer and its length match.
    let count = unsafe { GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) };
    if count != 1 {
        return false;
    }
    // SAFETY: no arguments; returns null when there's no console window.
    let window = unsafe { GetConsoleWindow() };
    if window.is_null() {
        return false;
    }
    let mut class = [0u16; 64];
    // SAFETY: the buffer and its length match; `window` is a valid handle.
    let len = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    let len = usize::try_from(len).unwrap_or(0);
    String::from_utf16_lossy(&class[..len]) == "PseudoConsoleWindow"
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
