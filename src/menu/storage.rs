//! Remembering the menu selection between visits.
//!
//! Browser: `localStorage`. Native: not stored (the game starts with the
//! defaults). The stored form is a short `key=value;...` line, so an old or
//! damaged entry only loses the values it cannot read.

/// Key in the browser's local storage.
const STORAGE_KEY: &str = "timefighter.menu";

/// Reads the stored line, if any.
pub fn load() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()?.local_storage().ok()??;
        storage.get_item(STORAGE_KEY).ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = STORAGE_KEY;
        None
    }
}

/// Stores the line; silently does nothing where storage is unavailable
/// (private windows, blocked site data, native builds).
pub fn save(line: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(STORAGE_KEY, line);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = line;
    }
}

/// `key=value` pairs of a stored line.
pub fn parse(line: &str) -> impl Iterator<Item = (&str, &str)> {
    line.split(';').filter_map(|pair| pair.split_once('='))
}
