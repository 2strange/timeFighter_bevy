//! The menu as data: rows of options, a cursor, and keyboard navigation.
//! Pure logic without ECS, so it is easy to test.

/// One choice of a row.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    /// Greyed out ("coming soon"); cannot be selected.
    pub enabled: bool,
}

impl Choice {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
        }
    }

    pub fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: false,
        }
    }
}

/// A row: a label and either choices or a button.
#[derive(Clone, Debug, PartialEq)]
pub struct Row<Id> {
    pub id: Id,
    pub label: String,
    /// Empty for a button row.
    pub choices: Vec<Choice>,
    pub selected: usize,
}

impl<Id> Row<Id> {
    pub fn is_button(&self) -> bool {
        self.choices.is_empty()
    }

    /// The selection moved by `step` to the next enabled choice, if any.
    pub fn step(&self, step: isize) -> Option<usize> {
        let len = self.choices.len() as isize;
        let mut i = self.selected as isize;
        loop {
            i += step;
            if i < 0 || i >= len {
                return None;
            }
            if self.choices[i as usize].enabled {
                return Some(i as usize);
            }
        }
    }
}

/// Keys the menu reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
}

/// What a key press does to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action<Id> {
    /// Move the cursor to this row.
    Focus(Id),
    /// Select choice `index` in row `id`.
    Select(Id, usize),
    /// Start the game.
    Start,
    None,
}

/// The effect of `nav` with the cursor on `cursor` (an index into `rows`).
pub fn navigate<Id: Copy>(rows: &[Row<Id>], cursor: usize, nav: Nav) -> Action<Id> {
    let Some(row) = rows.get(cursor) else {
        return Action::None;
    };
    match nav {
        Nav::Up => cursor
            .checked_sub(1)
            .map_or(Action::None, |i| Action::Focus(rows[i].id)),
        Nav::Down => rows
            .get(cursor + 1)
            .map_or(Action::None, |next| Action::Focus(next.id)),
        Nav::Left => row
            .step(-1)
            .map_or(Action::None, |i| Action::Select(row.id, i)),
        Nav::Right => row
            .step(1)
            .map_or(Action::None, |i| Action::Select(row.id, i)),
        // Enter starts from any row: the choices are already applied.
        Nav::Confirm => Action::Start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row<u8>> {
        vec![
            Row {
                id: 0,
                label: "Mode".into(),
                choices: vec![Choice::new("A"), Choice::disabled("B"), Choice::new("C")],
                selected: 0,
            },
            Row {
                id: 1,
                label: "Start".into(),
                choices: vec![],
                selected: 0,
            },
        ]
    }

    #[test]
    fn left_and_right_skip_disabled_choices_and_stop_at_the_ends() {
        let rows = rows();
        assert_eq!(navigate(&rows, 0, Nav::Right), Action::Select(0, 2));
        assert_eq!(navigate(&rows, 0, Nav::Left), Action::None);
        let mut at_end = rows.clone();
        at_end[0].selected = 2;
        assert_eq!(navigate(&at_end, 0, Nav::Left), Action::Select(0, 0));
        assert_eq!(navigate(&at_end, 0, Nav::Right), Action::None);
    }

    #[test]
    fn only_disabled_choices_left_means_no_move() {
        let mut rows = rows();
        rows[0].choices = vec![Choice::new("A"), Choice::disabled("B")];
        assert_eq!(navigate(&rows, 0, Nav::Right), Action::None);
    }

    #[test]
    fn up_down_move_between_rows_and_enter_starts() {
        let rows = rows();
        assert_eq!(navigate(&rows, 0, Nav::Down), Action::Focus(1));
        assert_eq!(navigate(&rows, 1, Nav::Down), Action::None);
        assert_eq!(navigate(&rows, 1, Nav::Up), Action::Focus(0));
        assert_eq!(navigate(&rows, 0, Nav::Up), Action::None);
        assert_eq!(navigate(&rows, 0, Nav::Confirm), Action::Start);
        // A button row has nothing to select.
        assert_eq!(navigate(&rows, 1, Nav::Right), Action::None);
    }
}
