//! System clipboard. Tests use an in-memory stand-in so they don't overwrite yours.

#[cfg(not(test))]
pub fn set(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .map_err(|e| format!("clipboard: {e}"))
}

#[cfg(not(test))]
pub fn get() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("clipboard: {e}"))
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
