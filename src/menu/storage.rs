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

/// One-shot flag the start page sets when the player picked mode and level
/// there: start the game right away instead of showing the menu.
const AUTOSTART_KEY: &str = "timefighter.autostart";

/// A start request from the start page, as `key=value` pairs (`mode`,
/// `level`): the page URL's query if it names a mode or level (fallback
/// when the browser keeps no storage), or an empty request when the
/// one-shot autostart flag is set. The flag is removed when read.
pub fn start_request() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let query = window.location().search().unwrap_or_default();
        if query_names_a_start(&query) {
            return Some(query.trim_start_matches('?').replace('&', ";"));
        }
        let storage = window.local_storage().ok()??;
        let flag = storage.get_item(AUTOSTART_KEY).ok()??;
        let _ = storage.remove_item(AUTOSTART_KEY);
        (flag == "1").then(String::new)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = AUTOSTART_KEY;
        None
    }
}

/// Whether a URL query (`?mode=protect&level=hard`) asks for a start.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn query_names_a_start(query: &str) -> bool {
    query
        .trim_start_matches('?')
        .split('&')
        .any(|pair| pair.starts_with("mode=") || pair.starts_with("level="))
}
