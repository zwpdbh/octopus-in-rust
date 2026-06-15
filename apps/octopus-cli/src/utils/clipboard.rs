use std::sync::Mutex;

static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut arboard::Clipboard) -> Result<R, arboard::Error>,
{
    let mut guard = CLIPBOARD
        .lock()
        .map_err(|e| format!("Clipboard lock poisoned: {}", e))?;
    if guard.is_none() {
        *guard = Some(
            arboard::Clipboard::new().map_err(|e| format!("Failed to open clipboard: {}", e))?,
        );
    }
    let cb = guard.as_mut().unwrap();
    f(cb).map_err(|e| format!("Clipboard error: {}", e))
}

/// Copy text to the system clipboard.
pub fn copy_text(text: &str) -> Result<(), String> {
    with_clipboard(|cb| cb.set_text(text))
}

/// Paste text from the system clipboard.
pub fn paste_text() -> Result<String, String> {
    with_clipboard(|cb| cb.get_text())
}
