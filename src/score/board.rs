//! Pilot names and the local top-10 lists, without ECS.
//!
//! A list is stored per mode and level as `NAME|score;NAME|score;...`,
//! best first; the start page reads the same form. Names are restricted to
//! a small alphabet, so neither `|` nor `;` can occur in them.

/// Longest and shortest name.
pub const NAME_MIN: usize = 3;
pub const NAME_MAX: usize = 12;
/// Entries a local list keeps.
pub const LOCAL_TOP: usize = 10;

/// Whether `c` may appear in a name (after upper-casing).
pub fn name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, ' ' | '-' | '_')
}

/// A valid name: 3-12 allowed characters, no space at either end, not two
/// in a row.
pub fn valid_name(name: &str) -> bool {
    let len = name.chars().count();
    (NAME_MIN..=NAME_MAX).contains(&len)
        && name.chars().all(name_char)
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && !name.contains("  ")
}

/// The name being typed on the result screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameField {
    pub text: String,
}

impl NameField {
    /// Adds a typed character (upper-cased); others and overlong input are
    /// ignored. Returns whether the text changed.
    pub fn type_char(&mut self, c: char) -> bool {
        let c = c.to_ascii_uppercase();
        if !name_char(c) || self.text.chars().count() >= NAME_MAX {
            return false;
        }
        if c == ' ' && (self.text.is_empty() || self.text.ends_with(' ')) {
            return false;
        }
        self.text.push(c);
        true
    }

    pub fn backspace(&mut self) -> bool {
        self.text.pop().is_some()
    }

    /// The name to submit, if the text is one.
    pub fn name(&self) -> Option<String> {
        let name = self.text.trim_end().to_owned();
        valid_name(&name).then_some(name)
    }
}

/// One line of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub score: u32,
}

/// Reads a stored list; damaged entries are skipped.
pub fn parse_list(line: &str) -> Vec<Entry> {
    line.split(';')
        .filter_map(|item| {
            let (name, score) = item.split_once('|')?;
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
        .map(|e| format!("{}|{}", e.name, e.score))
        .collect::<Vec<_>>()
        .join(";")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_take_letters_digits_space_dash_underscore() {
        for good in [
            "ACE",
            "MAVERICK",
            "RED-5",
            "TOP_GUN",
            "ICE MAN",
            "A1B2C3D4E5F6",
        ] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "AB",
            "THIRTEEN-CHAR",
            "ace",
            "ÄRGER",
            " ACE",
            "ACE ",
            "TWO  SPACES",
            "A|B",
            "A;B",
            "<B>",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn typing_upper_cases_and_skips_what_does_not_fit() {
        let mut field = NameField::default();
        for c in " ice  man!x".chars() {
            field.type_char(c);
        }
        assert_eq!(field.text, "ICE MANX");
        assert_eq!(field.name().as_deref(), Some("ICE MANX"));
        for c in "ABCDEFGHIJ".chars() {
            field.type_char(c);
        }
        assert_eq!(field.text.chars().count(), NAME_MAX);
        field.text = "AB ".into();
        assert_eq!(field.name(), None, "too short once trimmed");
        assert!(field.backspace());
        assert_eq!(field.text, "AB");
    }

    #[test]
    fn lists_keep_the_best_ten_and_ties_keep_their_order() {
        let mut list = parse_list("ACE|900;BOB|500;bad;CAT|x;DAN|500");
        assert_eq!(list.len(), 3, "damaged entries skipped: {list:?}");
        let place = insert(
            &mut list,
            Entry {
                name: "EVE".into(),
                score: 500,
            },
            LOCAL_TOP,
        );
        assert_eq!(place, Some(3), "after the earlier 500s");
        for i in 0..10 {
            insert(
                &mut list,
                Entry {
                    name: format!("P{i:02}"),
                    score: 1_000 + i,
                },
                LOCAL_TOP,
            );
        }
        assert_eq!(list.len(), LOCAL_TOP);
        assert_eq!(list[0].score, 1_009);
        let low = insert(
            &mut list,
            Entry {
                name: "LOW".into(),
                score: 1,
            },
            LOCAL_TOP,
        );
        assert_eq!(low, None);
        assert_eq!(parse_list(&format_list(&list)), list);
    }
}
