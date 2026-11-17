//! System clipboard. Tests use an in-memory stand-in so they don't overwrite yours.
//!
//! One clipboard is kept for the whole run. On Linux (X11) the copied text
//! is served by the program that copied it, so a clipboard dropped right
//! after copying gives the desktop's clipboard manager no time to take it,
//! and arboard prints a warning straight onto the terminal (over the TUI).

#[cfg(not(test))]
static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);

/// Runs `f` on the kept clipboard, opening it the first time. A failed
/// clipboard is dropped, so the next copy or paste opens a fresh one.
#[cfg(not(test))]
fn with_clipboard<T>(
    f: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, String> {
    let mut kept = CLIPBOARD
        .lock()
        .map_err(|_| "clipboard: unavailable after an earlier crash".to_string())?;
    let clipboard = match kept.as_mut() {
        Some(clipboard) => clipboard,
        None => kept.insert(arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))?),
    };
    f(clipboard).map_err(|e| {
        *kept = None;
        format!("clipboard: {e}")
    })
}

#[cfg(not(test))]
pub fn set(text: &str) -> Result<(), String> {
    with_clipboard(|c| c.set_text(text.to_string()))
}

#[cfg(not(test))]
pub fn get() -> Result<String, String> {
    with_clipboard(|c| c.get_text())
}

#[cfg(test)]
static TEST_CLIP: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

#[cfg(test)]
pub fn set(text: &str) -> Result<(), String> {
    *TEST_CLIP.lock().unwrap() = text.to_string();
    Ok(())
}

#[cfg(test)]
pub fn get() -> Result<String, String> {
    Ok(TEST_CLIP.lock().unwrap().clone())
}
