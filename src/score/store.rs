//! Key-value storage of the score module: the browser's `localStorage`,
//! natively (and in tests) a map that lives as long as the app.
//!
//! Keys shared with the page's script (`web/index.html`):
//! - `timefighter.name`: the pilot name
//! - `timefighter.top.<mode>.<level>`: local top 10, one `name<TAB>score`
//!   line per entry
//! - `timefighter.outbox`: JSON array of scores for the online list; the
//!   page sends them and empties it
//! - `timefighter.online`: the answer for the last one sent, as lines:
//!   id, rank all time, rank this week, then the top list like a local one
//!   (or id, `error`, reason)

use std::collections::HashMap;

use bevy::prelude::*;

pub const NAME_KEY: &str = "timefighter.name";
pub const OUTBOX_KEY: &str = "timefighter.outbox";
pub const ONLINE_KEY: &str = "timefighter.online";
/// Scores waiting in the outbox at most (older ones are dropped).
const OUTBOX_MAX: usize = 5;

pub fn top_key(mode: &str, level: &str) -> String {
    format!("timefighter.top.{mode}.{level}")
}

#[derive(Resource, Debug, Default)]
pub struct Store {
    /// Native builds and tests; unused in the browser.
    memory: HashMap<String, String>,
}

impl Store {
    pub fn get(&self, key: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = &self.memory;
            let storage = web_sys::window()?.local_storage().ok()??;
            storage.get_item(key).ok()?
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.memory.get(key).cloned()
        }
    }

    /// Stores silently where possible (private windows may refuse).
    pub fn set(&mut self, key: &str, value: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = &mut self.memory;
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item(key, value);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.memory.insert(key.to_owned(), value.to_owned());
        }
    }

    /// Appends a JSON object to the outbox array.
    pub fn push_outbox(&mut self, item: &str) {
        let mut items = split_outbox(&self.get(OUTBOX_KEY).unwrap_or_default());
        items.push(item.to_owned());
        let skip = items.len().saturating_sub(OUTBOX_MAX);
        let joined = items[skip..].join(",");
        self.set(OUTBOX_KEY, &format!("[{joined}]"));
    }
}

/// The objects of an outbox array written by [`Store::push_outbox`].
/// Braces inside strings (a name may hold any character) do not count.
pub fn split_outbox(array: &str) -> Vec<String> {
    let inner = array.trim().trim_start_matches('[').trim_end_matches(']');
    let mut items = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    let (mut in_string, mut escaped) = (false, false);
    for (i, c) in inner.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0
                    && let Some(s) = start.take()
                {
                    items.push(inner[s..=i].to_owned());
                }
            }
            _ => {}
        }
    }
    items
}

/// The page's answer for submission `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineAnswer {
    Ranked {
        all_time: Option<u32>,
        week: Option<u32>,
        top: String,
    },
    Failed(String),
}

/// Reads [`ONLINE_KEY`] if it answers submission `id`.
pub fn parse_online(text: &str, id: u64) -> Option<OnlineAnswer> {
    let mut lines = text.lines();
    if lines.next()?.trim().parse::<u64>().ok()? != id {
        return None;
    }
    let second = lines.next().unwrap_or_default().trim();
    if second == "error" {
        return Some(OnlineAnswer::Failed(
            lines.next().unwrap_or("not sent").trim().to_owned(),
        ));
    }
    let all_time = second.parse().ok();
    let week = lines.next().and_then(|l| l.trim().parse().ok());
    Some(OnlineAnswer::Ranked {
        all_time,
        week,
        top: lines.collect::<Vec<_>>().join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_keeps_the_last_five_objects() {
        let mut store = Store::default();
        for i in 0..7 {
            // A name with braces, a quote and a backslash.
            store.push_outbox(&format!(
                r#"{{"id":{i},"name":"a}}{{\"b\\","breakdown":{{"victory":true}}}}"#
            ));
        }
        let items = split_outbox(&store.get(OUTBOX_KEY).unwrap());
        assert_eq!(items.len(), 5);
        assert!(items[0].starts_with("{\"id\":2,"), "{items:?}");
        assert!(items[4].ends_with("}}"));
        // The page empties it with "[]".
        store.set(OUTBOX_KEY, "[]");
        store.push_outbox("{\"id\":9}");
        assert_eq!(store.get(OUTBOX_KEY).unwrap(), "[{\"id\":9}]");
    }

    #[test]
    fn online_answers_count_only_for_their_submission() {
        let text = "7\n12\n3\nACE\t900\nJürgen | ;\t500";
        assert_eq!(parse_online(text, 6), None);
        assert_eq!(
            parse_online(text, 7),
            Some(OnlineAnswer::Ranked {
                all_time: Some(12),
                week: Some(3),
                top: "ACE\t900\nJürgen | ;\t500".into()
            })
        );
        assert_eq!(
            parse_online("7\nerror\nrate limited", 7),
            Some(OnlineAnswer::Failed("rate limited".into()))
        );
        // Not in the top 100: no rank.
        assert!(matches!(
            parse_online("7\n-\n-\n", 7),
            Some(OnlineAnswer::Ranked {
                all_time: None,
                week: None,
                ..
            })
        ));
    }
}
