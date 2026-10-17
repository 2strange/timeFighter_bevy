//! Pilot names and the local top-10 lists, without ECS.
//!
//! A name is 1 to 18 printable characters, any script: control characters
//! are dropped and the ends trimmed. A list is stored per mode and level
//! as one `name<TAB>score` line per entry, best first; names cannot hold a
//! tab or a line break, so the form needs no escaping. The start page
//! reads the same form.

/// Longest and shortest name, in characters.
pub const NAME_MIN: usize = 1;
pub const NAME_MAX: usize = 18;
/// Entries a local list keeps.
pub const LOCAL_TOP: usize = 10;

/// `text` without control characters, trimmed.
pub fn clean_name(text: &str) -> String {
    let kept: String = text.chars().filter(|c| !c.is_control()).collect();
    kept.trim().to_owned()
}

/// A valid name: already clean, 1-18 characters.
pub fn valid_name(name: &str) -> bool {
    let len = name.chars().count();
    (NAME_MIN..=NAME_MAX).contains(&len) && clean_name(name) == name
}

/// The name being typed, with a cursor (in characters).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameField {
    chars: Vec<char>,
    cursor: usize,
}

impl NameField {
    /// A field holding `text`, the cursor at its end.
    pub fn with_text(text: &str) -> Self {
        let chars: Vec<char> = text
            .chars()
            .filter(|c| !c.is_control())
            .take(NAME_MAX)
            .collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The text before and after the cursor.
    pub fn split(&self) -> (String, String) {
        (
            self.chars[..self.cursor].iter().collect(),
            self.chars[self.cursor..].iter().collect(),
        )
    }

    /// Types `text` at the cursor; control characters are skipped, and
    /// nothing goes past [`NAME_MAX`]. Returns whether anything changed.
    pub fn type_text(&mut self, text: &str) -> bool {
        let mut changed = false;
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.chars.len() >= NAME_MAX {
                break;
            }
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
            changed = true;
        }
        changed
    }

    /// Deletes before the cursor.
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.chars.remove(self.cursor);
        true
    }

    /// Deletes at the cursor.
    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.chars.len() {
            return false;
        }
        self.chars.remove(self.cursor);
        true
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// The name to save, if the text has a visible character.
    pub fn name(&self) -> Option<String> {
        let name = clean_name(&self.text());
        valid_name(&name).then_some(name)
    }
}

/// One line of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub score: u32,
}

/// Reads a stored list; damaged entries are skipped. Lists from before
/// names took any character (`NAME|score;...`) still read.
pub fn parse_list(text: &str) -> Vec<Entry> {
    let legacy = !text.contains('\t') && text.contains('|');
    let items: Vec<(&str, &str)> = if legacy {
        text.split(';')
            .filter_map(|item| item.split_once('|'))
            .collect()
    } else {
        text.lines()
            .filter_map(|line| line.split_once('\t'))
            .collect()
    };
    items
        .into_iter()
        .filter_map(|(name, score)| {
            let score = score.trim().parse().ok()?;
            valid_name(name).then(|| Entry {
                name: name.to_owned(),
                score,
            })
        })
        .collect()
}

pub fn format_list(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|e| format!("{}\t{}", e.name, e.score))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inserts `entry` below equal scores (the earlier one keeps its place) and
/// keeps the best `top`. Returns its place (0 = first), or `None` if it did
/// not make the list.
pub fn insert(entries: &mut Vec<Entry>, entry: Entry, top: usize) -> Option<usize> {
    let place = entries
        .iter()
        .position(|e| e.score < entry.score)
        .unwrap_or(entries.len());
    if place >= top {
        return None;
    }
    entries.insert(place, entry);
    entries.truncate(top);
    Some(place)
}

/// `text` as the inside of a JSON string.
pub fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_take_any_printable_characters_up_to_eighteen() {
        for good in [
            "Trendgegner",
            "ICE MAN",
            "Jürgen Weiß",
            "Ølaf-Ærø",
            "Пилот",
            "飛行士",
            "A",
            "<b>&\"x\";|",
            "eighteen-chars-xyz",
        ] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "",
            "   ",
            " ACE",
            "ACE ",
            "TAB\tNAME",
            "nineteen-characters",
            "LINE\nBREAK",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert_eq!(clean_name("  Mav\u{7}erick \n"), "Maverick");
    }

    #[test]
    fn the_field_edits_at_its_cursor() {
        let mut field = NameField::with_text("Trendgegner");
        assert_eq!(field.name().as_deref(), Some("Trendgegner"));
        field.home();
        field.type_text("Der ");
        assert_eq!(field.text(), "Der Trendgegner");
        field.right();
        assert!(field.backspace());
        assert_eq!(field.text(), "Der rendgegner");
        field.end();
        field.left();
        assert!(field.delete());
        assert_eq!(field.text(), "Der rendgegne");
        assert_eq!(field.split(), ("Der rendgegne".into(), String::new()));
        // Never past 18 characters; control characters never get in.
        field.type_text("\u{1b}ÄÖÜäöüß");
        assert_eq!(field.text().chars().count(), NAME_MAX);
        assert!(!field.type_text("x"));
        let mut blank = NameField::with_text("  ");
        assert_eq!(blank.name(), None, "only spaces");
        blank.type_text("x");
        assert_eq!(blank.name().as_deref(), Some("x"));
    }

    #[test]
    fn lists_keep_the_best_ten_with_any_names() {
        let mut list = Vec::new();
        for (name, score) in [("Jürgen|;", 900), ("飛行士", 500), ("A", 500)] {
            insert(
                &mut list,
                Entry {
                    name: name.into(),
                    score,
                },
                LOCAL_TOP,
            );
        }
        assert_eq!(list[2].name, "A", "ties keep their order");
        assert_eq!(parse_list(&format_list(&list)), list);
        assert_eq!(
            parse_list("ACE|900;bad;CAT|x;BOB|500"),
            [
                Entry {
                    name: "ACE".into(),
                    score: 900
                },
                Entry {
                    name: "BOB".into(),
                    score: 500
                }
            ],
            "the old form still reads"
        );
        for i in 0..12 {
            insert(
                &mut list,
                Entry {
                    name: format!("P{i}"),
                    score: 1_000 + i,
                },
                LOCAL_TOP,
            );
        }
        assert_eq!(list.len(), LOCAL_TOP);
        assert_eq!(list[0].score, 1_011);
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("Jürgen"), "Jürgen");
    }
}
