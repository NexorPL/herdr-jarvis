//! Single-line text editing at a cursor, for the popup forms.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Applies one key to `text` with the cursor at char index `cursor` (past the end means at the end):
/// arrows and Home/End (Ctrl+A/E) move, Ctrl+arrows move by word, Backspace/Delete remove a char,
/// Ctrl+Backspace or Ctrl+W a word, Ctrl+U everything before the cursor; other characters are typed.
pub fn edit(text: &mut String, cursor: &mut usize, key: KeyEvent) {
    // Windows reports AltGr (Polish letters) as Ctrl+Alt: that is typing, not a Ctrl shortcut.
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let mut s: Vec<char> = text.chars().collect();
    let at = (*cursor).min(s.len());
    let (word_start, word_end) = (word_start(&s, at), word_end(&s, at));
    *cursor = match key.code {
        KeyCode::Left if ctrl => word_start,
        KeyCode::Left => at.saturating_sub(1),
        KeyCode::Right if ctrl => word_end,
        KeyCode::Right => (at + 1).min(s.len()),
        KeyCode::Home => 0,
        KeyCode::End => s.len(),
        KeyCode::Char('a') if ctrl => 0,
        KeyCode::Char('e') if ctrl => s.len(),
        KeyCode::Backspace if ctrl => {
            s.drain(word_start..at);
            word_start
        }
        KeyCode::Char('w') if ctrl => {
            s.drain(word_start..at);
            word_start
        }
        KeyCode::Char('u') if ctrl => {
            s.drain(..at);
            0
        }
        KeyCode::Backspace if at > 0 => {
            s.remove(at - 1);
            at - 1
        }
        KeyCode::Delete if at < s.len() => {
            s.remove(at);
            at
        }
        KeyCode::Char(c) if !ctrl => {
            s.insert(at, c);
            at + 1
        }
        _ => at,
    };
    *text = s.into_iter().collect();
}

fn word_start(s: &[char], mut i: usize) -> usize {
    while i > 0 && s[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !s[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

fn word_end(s: &[char], mut i: usize) -> usize {
    while i < s.len() && s[i].is_whitespace() {
        i += 1;
    }
    while i < s.len() && !s[i].is_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn run(text: &str, cursor: usize, keys: &[(KeyCode, KeyModifiers)]) -> (String, usize) {
        let (mut text, mut cursor) = (text.to_string(), cursor);
        for (code, mods) in keys {
            edit(&mut text, &mut cursor, KeyEvent::new(*code, *mods));
        }
        (text, cursor)
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CTRL: KeyModifiers = KeyModifiers::CONTROL;

    #[test]
    fn arrows_move_and_typing_inserts_at_the_cursor() {
        let keys = [
            (KeyCode::Left, NONE),
            (KeyCode::Left, NONE),
            (KeyCode::Char('i'), NONE),
        ];
        assert_eq!(run("png", usize::MAX, &keys), ("ping".into(), 2));
        let keys = [
            (KeyCode::Home, NONE),
            (KeyCode::Right, NONE),
            (KeyCode::Char('x'), NONE),
        ];
        assert_eq!(run("ab", 2, &keys), ("axb".into(), 2));
        assert_eq!(run("ab", 0, &[(KeyCode::Left, NONE)]), ("ab".into(), 0));
        assert_eq!(run("ab", 2, &[(KeyCode::Right, NONE)]), ("ab".into(), 2));
    }

    #[test]
    fn backspace_and_delete_work_around_the_cursor() {
        assert_eq!(
            run("pinng", 3, &[(KeyCode::Backspace, NONE)]),
            ("ping".into(), 2)
        );
        assert_eq!(
            run("pinng", 3, &[(KeyCode::Delete, NONE)]),
            ("ping".into(), 3)
        );
        assert_eq!(
            run("ab", 0, &[(KeyCode::Backspace, NONE)]),
            ("ab".into(), 0)
        );
        assert_eq!(run("ab", 2, &[(KeyCode::Delete, NONE)]), ("ab".into(), 2));
    }

    #[test]
    fn ctrl_moves_and_deletes_by_word() {
        let text = "cargo run --release";
        assert_eq!(run(text, usize::MAX, &[(KeyCode::Left, CTRL)]).1, 10);
        assert_eq!(run(text, 0, &[(KeyCode::Right, CTRL)]).1, 5);
        assert_eq!(
            run(text, 9, &[(KeyCode::Backspace, CTRL)]),
            ("cargo  --release".into(), 6)
        );
        assert_eq!(
            run(text, 9, &[(KeyCode::Char('w'), CTRL)]).0,
            "cargo  --release"
        );
        assert_eq!(
            run(text, 9, &[(KeyCode::Char('u'), CTRL)]),
            (" --release".into(), 0)
        );
        assert_eq!(
            run(text, 9, &[(KeyCode::Char('a'), CTRL)]),
            (text.into(), 0)
        );
        assert_eq!(
            run(text, 0, &[(KeyCode::Char('e'), CTRL)]),
            (text.into(), 19)
        );
        assert_eq!(run(text, 0, &[(KeyCode::End, NONE)]).1, 19);
    }

    #[test]
    fn altgr_letters_are_typed_not_treated_as_ctrl() {
        // Windows reports AltGr as Ctrl+Alt: Polish ą is AltGr+a.
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(
            run("a", 1, &[(KeyCode::Char('ą'), altgr)]),
            ("aą".into(), 2)
        );
        assert_eq!(
            run("zł", 1, &[(KeyCode::Char('ó'), altgr)]),
            ("zół".into(), 2)
        );
    }
}
