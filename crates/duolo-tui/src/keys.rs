//! 把 crossterm 的按键事件编码成终端字节序列，原样发给 Agent。

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 是否是 Duolo 的前缀键 `Ctrl+]`。
/// 终端把它发成 0x1D，crossterm 会解析成 `Ctrl+]` 或 `Ctrl+5`，两种都认。
pub fn is_prefix(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char(']') | KeyCode::Char('5'))
}

/// 语音键 `Ctrl+Space`（终端发成 0x00）。
pub fn is_voice(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char(' ') | KeyCode::Char('@'))
}

/// `app_cursor` 为真时方向键使用应用光标模式（`ESC O A`）。
pub fn encode(key: &KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = key.modifiers;
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let shift = mods.contains(KeyModifiers::SHIFT);

    let mut out = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if alt {
                out.push(0x1b);
            }
            if ctrl {
                out.push(ctrl_byte(c)?);
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        KeyCode::Enter => out.extend_from_slice(if alt || shift { b"\x1b\r" } else { b"\r" }),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => {
            if alt {
                out.push(0x1b);
            }
            out.push(if ctrl { 0x08 } else { 0x7f });
        }
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => out.extend(cursor_key(b'A', mods, app_cursor)),
        KeyCode::Down => out.extend(cursor_key(b'B', mods, app_cursor)),
        KeyCode::Right => out.extend(cursor_key(b'C', mods, app_cursor)),
        KeyCode::Left => out.extend(cursor_key(b'D', mods, app_cursor)),
        KeyCode::Home => out.extend(cursor_key(b'H', mods, app_cursor)),
        KeyCode::End => out.extend(cursor_key(b'F', mods, app_cursor)),
        KeyCode::PageUp => out.extend(tilde_key(5, mods)),
        KeyCode::PageDown => out.extend(tilde_key(6, mods)),
        KeyCode::Insert => out.extend(tilde_key(2, mods)),
        KeyCode::Delete => out.extend(tilde_key(3, mods)),
        KeyCode::F(n) => out.extend(function_key(n)?),
        _ => return None,
    }
    Some(out)
}

fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// xterm 的修饰键参数：1 + Shift(1) + Alt(2) + Ctrl(4)。
fn modifier_param(mods: KeyModifiers) -> u8 {
    1 + u8::from(mods.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(mods.contains(KeyModifiers::ALT))
        + 4 * u8::from(mods.contains(KeyModifiers::CONTROL))
}

fn cursor_key(code: u8, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    let m = modifier_param(mods);
    if m > 1 {
        format!("\x1b[1;{m}{}", code as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', code]
    } else {
        vec![0x1b, b'[', code]
    }
}

fn tilde_key(n: u8, mods: KeyModifiers) -> Vec<u8> {
    let m = modifier_param(mods);
    if m > 1 {
        format!("\x1b[{n};{m}~").into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

fn function_key(n: u8) -> Option<Vec<u8>> {
    let seq = match n {
        1 => "\x1bOP",
        2 => "\x1bOQ",
        3 => "\x1bOR",
        4 => "\x1bOS",
        5 => "\x1b[15~",
        6 => "\x1b[17~",
        7 => "\x1b[18~",
        8 => "\x1b[19~",
        9 => "\x1b[20~",
        10 => "\x1b[21~",
        11 => "\x1b[23~",
        12 => "\x1b[24~",
        _ => return None,
    };
    Some(seq.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn plain_and_control_chars() {
        assert_eq!(encode(&key(KeyCode::Char('a'), KeyModifiers::NONE), false), Some(b"a".to_vec()));
        assert_eq!(encode(&key(KeyCode::Char('中'), KeyModifiers::NONE), false), Some("中".as_bytes().to_vec()));
        assert_eq!(encode(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), false), Some(vec![3]));
        assert_eq!(encode(&key(KeyCode::Char('b'), KeyModifiers::ALT), false), Some(vec![0x1b, b'b']));
    }

    #[test]
    fn special_keys() {
        assert_eq!(encode(&key(KeyCode::Enter, KeyModifiers::NONE), false), Some(b"\r".to_vec()));
        assert_eq!(encode(&key(KeyCode::Backspace, KeyModifiers::NONE), false), Some(vec![0x7f]));
        assert_eq!(encode(&key(KeyCode::BackTab, KeyModifiers::SHIFT), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(encode(&key(KeyCode::Up, KeyModifiers::NONE), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(&key(KeyCode::Up, KeyModifiers::NONE), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode(&key(KeyCode::Right, KeyModifiers::CONTROL), false), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(encode(&key(KeyCode::Delete, KeyModifiers::NONE), false), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn duolo_keys() {
        assert!(is_prefix(&key(KeyCode::Char(']'), KeyModifiers::CONTROL)));
        assert!(is_prefix(&key(KeyCode::Char('5'), KeyModifiers::CONTROL)));
        assert!(!is_prefix(&key(KeyCode::Char(']'), KeyModifiers::NONE)));
        assert!(is_voice(&key(KeyCode::Char(' '), KeyModifiers::CONTROL)));
    }
}
