//! Text editing at a cursor, for the popup forms; a prompt can hold line breaks.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Applies one key to `text` with the cursor at char index `cursor` (past the end means at the end):
/// arrows and Home/End (Ctrl+A/E) move, Ctrl+arrows move by word, Backspace/Delete remove a char,
/// Ctrl+Backspace or Ctrl+W a word, Ctrl+U the line before the cursor; other characters are typed.
/// Home, End and Ctrl+U work on the cursor's line, and Up and Down move between lines.
pub fn edit(text: &mut String, cursor: &mut usize, key: KeyEvent) {
    // Windows reports AltGr (Polish letters) as Ctrl+Alt: that is typing, not a Ctrl shortcut.
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    let mut s: Vec<char> = text.chars().collect();
    let at = (*cursor).min(s.len());
    let (word_start, word_end) = (word_start(&s, at), word_end(&s, at));
    // The cursor's line, for a value with line breaks (a prompt); otherwise the whole text.
    let (start, end) = (line_start(&s, at), line_end(&s, at));
    *cursor = match key.code {
        KeyCode::Left if ctrl => word_start,
        KeyCode::Left => at.saturating_sub(1),
        KeyCode::Right if ctrl => word_end,
        KeyCode::Right => (at + 1).min(s.len()),
        KeyCode::Home => start,
        KeyCode::End => end,
        KeyCode::Char('a') if ctrl => start,
        KeyCode::Char('e') if ctrl => end,
        // Up and down keep the column, or stop at the end of a shorter line.
        KeyCode::Up if start > 0 => {
            let prev = line_start(&s, start - 1);
            prev + (at - start).min(start - 1 - prev)
        }
        KeyCode::Down if end < s.len() => {
            let next_end = line_end(&s, end + 1);
            end + 1 + (at - start).min(next_end - end - 1)
        }
        KeyCode::Backspace if ctrl => {
            s.drain(word_start..at);
            word_start
        }
        KeyCode::Char('w') if ctrl => {
            s.drain(word_start..at);
            word_start
        }
        KeyCode::Char('u') if ctrl => {
            s.drain(start..at);
            start
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

fn line_start(s: &[char], i: usize) -> usize {
    s[..i].iter().rposition(|&c| c == '\n').map_or(0, |n| n + 1)
}

fn line_end(s: &[char], i: usize) -> usize {
    s[i..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(s.len(), |n| i + n)
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
    fn line_keys_work_on_the_cursor_line_and_arrows_move_between_lines() {
        let text = "first\nsecond line\nend";
        // Cursor on "second line" after "sec" (index 9).
        assert_eq!(run(text, 9, &[(KeyCode::Home, NONE)]).1, 6);
        assert_eq!(run(text, 9, &[(KeyCode::End, NONE)]).1, 17);
        assert_eq!(run(text, 9, &[(KeyCode::Char('a'), CTRL)]).1, 6);
        assert_eq!(run(text, 9, &[(KeyCode::Char('e'), CTRL)]).1, 17);
        assert_eq!(
            run(text, 9, &[(KeyCode::Char('u'), CTRL)]),
            ("first\nond line\nend".into(), 6)
        );
        // Up and down keep the column, or stop at a shorter line's end.
        assert_eq!(run(text, 9, &[(KeyCode::Up, NONE)]).1, 3);
        assert_eq!(run(text, 16, &[(KeyCode::Down, NONE)]).1, 21);
        assert_eq!(run(text, 2, &[(KeyCode::Up, NONE)]).1, 2);
        assert_eq!(run(text, 19, &[(KeyCode::Down, NONE)]).1, 19);
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
