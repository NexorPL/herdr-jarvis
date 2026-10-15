//! Agent screens as herdr reads them with `format: ansi`: text with SGR color and style codes.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Lines with the agent's own colors; a style carries over line ends like in a terminal.
/// Escape sequences other than SGR (`ESC [ … m`) are dropped.
pub fn lines(text: &str) -> Vec<Line<'static>> {
    let mut style = Style::new();
    text.lines().map(|l| line(l, &mut style)).collect()
}

fn line(text: &str, style: &mut Style) -> Line<'static> {
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            run.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {}
            // OSC (titles, hyperlinks): up to BEL or ESC \; their text is not screen text.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
                continue;
            }
            // Charset and line-size selections take one more character.
            Some('(' | ')' | '*' | '+' | '#' | '%') => {
                chars.next();
                continue;
            }
            _ => continue,
        }
        let mut params = String::new();
        let mut last = None;
        for c in chars.by_ref() {
            if ('\x40'..='\x7e').contains(&c) {
                last = Some(c);
                break;
            }
            params.push(c);
        }
        if last == Some('m') {
            if !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), *style));
            }
            *style = sgr(*style, &params);
        }
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, *style));
    }
    Line::from(spans)
}

fn sgr(mut style: Style, params: &str) -> Style {
    // Colon sub-parameters (38:2::r:g:b, 38:5:n, 4:3) in the ';' form the loop below reads.
    let num = |p: &str| p.parse::<u16>().unwrap_or(0);
    let mut flat: Vec<u16> = Vec::new();
    for group in params.split(';') {
        let sub: Vec<&str> = group.split(':').collect();
        match (num(sub[0]), sub.len()) {
            (_, 1) => flat.push(num(group)),
            (c @ (38 | 48), _) if sub[1] == "2" && sub.len() >= 5 => {
                flat.extend([c, 2]);
                flat.extend(sub[sub.len() - 3..].iter().map(|p| num(p)));
            }
            (c @ (38 | 48), _) if sub[1] == "5" => flat.extend([c, 5, num(sub[2])]),
            (4, _) => flat.push(if sub[1] == "0" { 24 } else { 4 }),
            (c, _) => flat.push(c),
        }
    }
    let mut n = flat.into_iter();
    while let Some(p) = n.next() {
        style = match p {
            0 => Style::new(),
            1 => style.add_modifier(Modifier::BOLD),
            2 => style.add_modifier(Modifier::DIM),
            3 => style.add_modifier(Modifier::ITALIC),
            4 => style.add_modifier(Modifier::UNDERLINED),
            7 => style.add_modifier(Modifier::REVERSED),
            9 => style.add_modifier(Modifier::CROSSED_OUT),
            22 => style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style.remove_modifier(Modifier::ITALIC),
            24 => style.remove_modifier(Modifier::UNDERLINED),
            27 => style.remove_modifier(Modifier::REVERSED),
            29 => style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style.fg(Color::Indexed((p - 30) as u8)),
            90..=97 => style.fg(Color::Indexed((p - 90 + 8) as u8)),
            40..=47 => style.bg(Color::Indexed((p - 40) as u8)),
            100..=107 => style.bg(Color::Indexed((p - 100 + 8) as u8)),
            39 => style.fg(Color::Reset),
            49 => style.bg(Color::Reset),
            38 | 48 => {
                let color = match n.next() {
                    Some(5) => n.next().map(|i| Color::Indexed(i as u8)),
                    Some(2) => match (n.next(), n.next(), n.next()) {
                        (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r as u8, g as u8, b as u8)),
                        _ => None,
                    },
                    _ => None,
                };
                match (p, color) {
                    (38, Some(c)) => style.fg(c),
                    (48, Some(c)) => style.bg(c),
                    _ => style,
                }
            }
            _ => style,
        };
    }
    style
}

/// The line's text without styles.
pub fn plain(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_styles_become_spans() {
        let l = lines("  \x1b[0m\x1b[38;2;255;193;7m⏵⏵ auto\x1b[0m\x1b[1;31m on\x1b[22m!\x1b[0m");
        let s = &l[0].spans;
        assert_eq!(plain(&l[0]), "  ⏵⏵ auto on!");
        assert_eq!(s[1].style.fg, Some(Color::Rgb(255, 193, 7)));
        assert_eq!(s[2].style.fg, Some(Color::Indexed(1)));
        assert!(s[2].style.add_modifier.contains(Modifier::BOLD));
        assert!(!s[3].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(s[3].style.fg, Some(Color::Indexed(1)));
    }

    #[test]
    fn style_carries_over_lines_and_other_escapes_drop() {
        let l = lines("\x1b[48;5;22m+ added\n still\x1b[0m\x1b[2K done");
        assert_eq!(l[1].spans[0].style.bg, Some(Color::Indexed(22)));
        assert_eq!(plain(&l[1]), " still done");
        assert_eq!(l[1].spans[1].style, Style::new());
    }

    #[test]
    fn other_escapes_leave_no_text() {
        let l = lines("\x1b]8;;file:///x\x1b\\label\x1b]8;;\x1b\\ \x1b]0;title\x07(\x1b(Bok)");
        assert_eq!(plain(&l[0]), "label (ok)");
    }

    #[test]
    fn colon_sub_parameters() {
        let l = lines("\x1b[38:2::255:100:0ma\x1b[4:3mb\x1b[4:0mc");
        let s = &l[0].spans;
        assert_eq!(s[0].style.fg, Some(Color::Rgb(255, 100, 0)));
        assert!(s[1].style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!s[1].style.add_modifier.contains(Modifier::ITALIC));
        assert!(!s[2].style.add_modifier.contains(Modifier::UNDERLINED));
    }
}
