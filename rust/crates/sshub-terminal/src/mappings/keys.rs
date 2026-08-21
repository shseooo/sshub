//! gpui `Keystroke` → PTY로 보낼 escape 바이트.
//!
//! Zed `terminal/src/mappings/keys.rs` 포팅. 세 단계로 결정한다:
//!  1. (key, 수식키) 수동 테이블 — 모드 의존(APP_CURSOR, ALT_SCREEN, LFNL) 포함
//!  2. 수식키가 붙은 자동 CSI (`\x1b[1;{code}A` 류)
//!  3. ctrl 제어문자 → alt-as-meta ESC 접두 → `key_char` 폴백
//!
//! cmd(platform)가 눌린 조합은 여기서 `None`을 돌려준다 — cmd-c/cmd-v 같은 앱
//! 단축키를 뷰가 처리해야 하므로 터미널이 삼키면 안 된다.

use gpui::Keystroke;

use crate::backend::TermMode;

/// 수식키 조합을 alacritty 관례대로 셋으로 접는다. shift/alt/ctrl 중 둘 이상이면
/// `Other` — 수동 테이블은 건너뛰고 자동 CSI 경로로 간다.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Mods {
    None,
    Alt,
    Ctrl,
    Shift,
    Other,
}

impl Mods {
    fn new(ks: &Keystroke) -> Mods {
        match (ks.modifiers.shift, ks.modifiers.alt, ks.modifiers.control) {
            (false, false, false) => Mods::None,
            (true, false, false) => Mods::Shift,
            (false, true, false) => Mods::Alt,
            (false, false, true) => Mods::Ctrl,
            _ => Mods::Other,
        }
    }

    fn any(self) -> bool {
        self != Mods::None
    }
}

/// CSI 수식키 파라미터: 1 + (shift 1 | alt 2 | ctrl 4).
fn modifier_code(ks: &Keystroke) -> u32 {
    let mut code = 0;
    if ks.modifiers.shift {
        code |= 1;
    }
    if ks.modifiers.alt {
        code |= 2;
    }
    if ks.modifiers.control {
        code |= 4;
    }
    code + 1
}

/// 키스트로크가 만들어내는 바이트열. 보낼 것이 없으면 `None`.
pub fn to_esc_bytes(ks: &Keystroke, mode: TermMode, alt_is_meta: bool) -> Option<Vec<u8>> {
    to_esc_str(ks, mode, alt_is_meta).map(String::into_bytes)
}

/// 문자열 형태 (테스트가 읽기 쉽도록 분리).
pub fn to_esc_str(ks: &Keystroke, mode: TermMode, alt_is_meta: bool) -> Option<String> {
    // cmd 조합은 앱 단축키 — 터미널로 내려보내지 않는다.
    if ks.modifiers.platform {
        return None;
    }

    let mods = Mods::new(ks);
    let key = ks.key.as_str();
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let alt_screen = mode.contains(TermMode::ALT_SCREEN);

    // 1) 수동 테이블
    let manual: Option<&'static str> = match (key, mods) {
        ("tab", Mods::None) => Some("\x09"),
        ("tab", Mods::Shift) => Some("\x1b[Z"),
        ("escape", Mods::None) => Some("\x1b"),
        // LNM(LF/NL) 모드에서는 Enter가 CRLF를 보낸다.
        ("enter", Mods::None) if mode.contains(TermMode::LINE_FEED_NEW_LINE) => Some("\x0d\x0a"),
        ("enter", Mods::None) => Some("\x0d"),
        ("enter", Mods::Shift) => Some("\x0d"),
        ("enter", Mods::Alt) => Some("\x1b\x0d"),
        // 백스페이스는 DEL(0x7f) — 유닉스 터미널의 사실상 표준.
        ("backspace", Mods::None) => Some("\x7f"),
        ("backspace", Mods::Shift) => Some("\x7f"),
        ("backspace", Mods::Ctrl) => Some("\x08"),
        ("backspace", Mods::Alt) => Some("\x1b\x7f"),
        ("space", Mods::Ctrl) => Some("\x00"),

        // alt-screen(TUI) 안에서는 shift+이동키가 앱으로 전달되어야 한다.
        ("home", Mods::Shift) if alt_screen => Some("\x1b[1;2H"),
        ("end", Mods::Shift) if alt_screen => Some("\x1b[1;2F"),
        ("pageup", Mods::Shift) if alt_screen => Some("\x1b[5;2~"),
        ("pagedown", Mods::Shift) if alt_screen => Some("\x1b[6;2~"),

        ("home", Mods::None) if app_cursor => Some("\x1bOH"),
        ("home", Mods::None) => Some("\x1b[H"),
        ("end", Mods::None) if app_cursor => Some("\x1bOF"),
        ("end", Mods::None) => Some("\x1b[F"),
        ("up", Mods::None) if app_cursor => Some("\x1bOA"),
        ("up", Mods::None) => Some("\x1b[A"),
        ("down", Mods::None) if app_cursor => Some("\x1bOB"),
        ("down", Mods::None) => Some("\x1b[B"),
        ("right", Mods::None) if app_cursor => Some("\x1bOC"),
        ("right", Mods::None) => Some("\x1b[C"),
        ("left", Mods::None) if app_cursor => Some("\x1bOD"),
        ("left", Mods::None) => Some("\x1b[D"),

        ("insert", Mods::None) => Some("\x1b[2~"),
        ("delete", Mods::None) => Some("\x1b[3~"),
        ("pageup", Mods::None) => Some("\x1b[5~"),
        ("pagedown", Mods::None) => Some("\x1b[6~"),

        ("f1", Mods::None) => Some("\x1bOP"),
        ("f2", Mods::None) => Some("\x1bOQ"),
        ("f3", Mods::None) => Some("\x1bOR"),
        ("f4", Mods::None) => Some("\x1bOS"),
        ("f5", Mods::None) => Some("\x1b[15~"),
        ("f6", Mods::None) => Some("\x1b[17~"),
        ("f7", Mods::None) => Some("\x1b[18~"),
        ("f8", Mods::None) => Some("\x1b[19~"),
        ("f9", Mods::None) => Some("\x1b[20~"),
        ("f10", Mods::None) => Some("\x1b[21~"),
        ("f11", Mods::None) => Some("\x1b[23~"),
        ("f12", Mods::None) => Some("\x1b[24~"),
        ("f13", Mods::None) => Some("\x1b[25~"),
        ("f14", Mods::None) => Some("\x1b[26~"),
        ("f15", Mods::None) => Some("\x1b[28~"),
        ("f16", Mods::None) => Some("\x1b[29~"),
        ("f17", Mods::None) => Some("\x1b[31~"),
        ("f18", Mods::None) => Some("\x1b[32~"),
        ("f19", Mods::None) => Some("\x1b[33~"),
        ("f20", Mods::None) => Some("\x1b[34~"),
        _ => None,
    };
    if let Some(esc) = manual {
        return Some(esc.to_string());
    }

    // 2) 수식키가 붙은 자동 CSI
    if mods.any() {
        let code = modifier_code(ks);
        let modified: Option<String> = match key {
            "up" => Some(format!("\x1b[1;{code}A")),
            "down" => Some(format!("\x1b[1;{code}B")),
            "right" => Some(format!("\x1b[1;{code}C")),
            "left" => Some(format!("\x1b[1;{code}D")),
            "end" => Some(format!("\x1b[1;{code}F")),
            "home" => Some(format!("\x1b[1;{code}H")),
            "f1" => Some(format!("\x1b[1;{code}P")),
            "f2" => Some(format!("\x1b[1;{code}Q")),
            "f3" => Some(format!("\x1b[1;{code}R")),
            "f4" => Some(format!("\x1b[1;{code}S")),
            "f5" => Some(format!("\x1b[15;{code}~")),
            "f6" => Some(format!("\x1b[17;{code}~")),
            "f7" => Some(format!("\x1b[18;{code}~")),
            "f8" => Some(format!("\x1b[19;{code}~")),
            "f9" => Some(format!("\x1b[20;{code}~")),
            "f10" => Some(format!("\x1b[21;{code}~")),
            "f11" => Some(format!("\x1b[23;{code}~")),
            "f12" => Some(format!("\x1b[24;{code}~")),
            "insert" => Some(format!("\x1b[2;{code}~")),
            "delete" => Some(format!("\x1b[3;{code}~")),
            "pageup" => Some(format!("\x1b[5;{code}~")),
            "pagedown" => Some(format!("\x1b[6;{code}~")),
            _ => None,
        };
        if let Some(esc) = modified {
            return Some(esc);
        }
    }

    // 3a) ctrl 제어문자
    if ks.modifiers.control {
        if let Some(byte) = control_code(key) {
            let ctrl_char = char::from(byte).to_string();
            // ctrl+alt: alt-as-meta면 ESC를 앞에 붙인다.
            if ks.modifiers.alt && alt_is_meta {
                return Some(format!("\x1b{ctrl_char}"));
            }
            return Some(ctrl_char);
        }
    }

    // 3b) alt-as-meta: ESC + 타이핑된 문자
    if alt_is_meta && ks.modifiers.alt && !ks.modifiers.control {
        // macOS에서 option-s는 key_char가 "ß"지만 meta로 쓸 때는 원래 키("s")를
        // 보내야 한다 — key가 단일 문자일 때만 meta 취급.
        if key.chars().count() == 1 {
            let base = if ks.modifiers.shift { key.to_uppercase() } else { key.to_string() };
            return Some(format!("\x1b{base}"));
        }
    }

    // 3c) 평범한 타이핑
    if let Some(text) = &ks.key_char {
        if !text.is_empty() {
            return Some(text.clone());
        }
    }

    // key_char가 없어도 단일 문자 키는 그대로 보낸다 (레이아웃/플랫폼 편차 보정).
    if !ks.modifiers.control && !ks.modifiers.alt && key.chars().count() == 1 {
        return Some(if ks.modifiers.shift { key.to_uppercase() } else { key.to_string() });
    }

    None
}

/// ctrl 조합의 제어문자. 표준 ASCII 규칙(문자 & 0x1f) + 관용 매핑.
fn control_code(key: &str) -> Option<u8> {
    if key == "space" {
        return Some(0x00);
    }
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' | '-' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn ks(key: &str, m: Modifiers) -> Keystroke {
        // key_char는 플랫폼이 채워주는 값 — 수식키 없는 단일 문자에만 흉내낸다.
        let key_char = if !m.control && !m.alt && !m.platform && key.chars().count() == 1 {
            Some(if m.shift { key.to_uppercase() } else { key.to_string() })
        } else {
            None
        };
        Keystroke { modifiers: m, key: key.to_string(), key_char }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }
    fn ctrl() -> Modifiers {
        Modifiers { control: true, ..Default::default() }
    }
    fn alt() -> Modifiers {
        Modifiers { alt: true, ..Default::default() }
    }
    fn shift() -> Modifiers {
        Modifiers { shift: true, ..Default::default() }
    }
    fn cmd() -> Modifiers {
        Modifiers { platform: true, ..Default::default() }
    }

    fn esc(key: &str, m: Modifiers) -> Option<String> {
        to_esc_str(&ks(key, m), TermMode::NONE, true)
    }

    fn esc_mode(key: &str, m: Modifiers, mode: TermMode) -> Option<String> {
        to_esc_str(&ks(key, m), mode, true)
    }

    #[test]
    fn basic_control_keys() {
        let table = [
            ("enter", none(), "\x0d"),
            ("tab", none(), "\x09"),
            ("escape", none(), "\x1b"),
            ("backspace", none(), "\x7f"),
            ("tab", shift(), "\x1b[Z"),
            ("backspace", alt(), "\x1b\x7f"),
            ("backspace", ctrl(), "\x08"),
            ("space", ctrl(), "\x00"),
        ];
        for (key, m, want) in table {
            assert_eq!(esc(key, m).as_deref(), Some(want), "key={key}");
        }
    }

    #[test]
    fn enter_sends_crlf_in_line_feed_new_line_mode() {
        assert_eq!(
            esc_mode("enter", none(), TermMode::LINE_FEED_NEW_LINE).as_deref(),
            Some("\x0d\x0a")
        );
    }

    #[test]
    fn arrows_switch_on_app_cursor_mode() {
        let normal = [("up", "\x1b[A"), ("down", "\x1b[B"), ("right", "\x1b[C"), ("left", "\x1b[D")];
        for (key, want) in normal {
            assert_eq!(esc(key, none()).as_deref(), Some(want), "normal {key}");
        }
        let app = [("up", "\x1bOA"), ("down", "\x1bOB"), ("right", "\x1bOC"), ("left", "\x1bOD")];
        for (key, want) in app {
            assert_eq!(
                esc_mode(key, none(), TermMode::APP_CURSOR).as_deref(),
                Some(want),
                "app {key}"
            );
        }
    }

    #[test]
    fn home_end_follow_app_cursor_mode() {
        assert_eq!(esc("home", none()).as_deref(), Some("\x1b[H"));
        assert_eq!(esc("end", none()).as_deref(), Some("\x1b[F"));
        assert_eq!(esc_mode("home", none(), TermMode::APP_CURSOR).as_deref(), Some("\x1bOH"));
        assert_eq!(esc_mode("end", none(), TermMode::APP_CURSOR).as_deref(), Some("\x1bOF"));
    }

    #[test]
    fn shift_navigation_is_forwarded_only_on_the_alt_screen() {
        assert_eq!(esc_mode("home", shift(), TermMode::ALT_SCREEN).as_deref(), Some("\x1b[1;2H"));
        assert_eq!(esc_mode("pageup", shift(), TermMode::ALT_SCREEN).as_deref(), Some("\x1b[5;2~"));
        // alt-screen이 아니면 자동 CSI 경로(shift=2)로 떨어진다
        assert_eq!(esc("pageup", shift()).as_deref(), Some("\x1b[5;2~"));
    }

    #[test]
    fn function_keys_unmodified() {
        let table = [
            ("f1", "\x1bOP"),
            ("f4", "\x1bOS"),
            ("f5", "\x1b[15~"),
            ("f10", "\x1b[21~"),
            ("f12", "\x1b[24~"),
        ];
        for (key, want) in table {
            assert_eq!(esc(key, none()).as_deref(), Some(want), "key={key}");
        }
    }

    #[test]
    fn modifier_codes_follow_the_shift_alt_ctrl_bitmask() {
        // 1 + shift(1) + alt(2) + ctrl(4)
        let m = |s, a, c| Modifiers { shift: s, alt: a, control: c, ..Default::default() };
        assert_eq!(esc("up", m(true, false, false)).as_deref(), Some("\x1b[1;2A"));
        assert_eq!(esc("up", m(false, true, false)).as_deref(), Some("\x1b[1;3A"));
        assert_eq!(esc("up", m(true, true, false)).as_deref(), Some("\x1b[1;4A"));
        assert_eq!(esc("up", m(false, false, true)).as_deref(), Some("\x1b[1;5A"));
        assert_eq!(esc("up", m(true, true, true)).as_deref(), Some("\x1b[1;8A"));
        assert_eq!(esc("delete", m(false, false, true)).as_deref(), Some("\x1b[3;5~"));
        assert_eq!(esc("f5", m(true, false, false)).as_deref(), Some("\x1b[15;2~"));
    }

    #[test]
    fn ctrl_letters_map_to_control_codes() {
        let table = [("a", "\x01"), ("c", "\x03"), ("d", "\x04"), ("z", "\x1a")];
        for (key, want) in table {
            assert_eq!(esc(key, ctrl()).as_deref(), Some(want), "ctrl-{key}");
        }
    }

    #[test]
    fn ctrl_punctuation_maps_to_control_codes() {
        let table = [
            ("[", "\x1b"),
            ("\\", "\x1c"),
            ("]", "\x1d"),
            ("6", "\x1e"),
            ("_", "\x1f"),
            ("/", "\x1f"),
            ("?", "\x7f"),
            ("2", "\x00"),
        ];
        for (key, want) in table {
            assert_eq!(esc(key, ctrl()).as_deref(), Some(want), "ctrl-{key}");
        }
    }

    #[test]
    fn alt_as_meta_prefixes_escape() {
        assert_eq!(esc("b", alt()).as_deref(), Some("\x1bb"));
        assert_eq!(esc("f", alt()).as_deref(), Some("\x1bf"));
        let alt_shift = Modifiers { alt: true, shift: true, ..Default::default() };
        assert_eq!(esc("b", alt_shift).as_deref(), Some("\x1bB"));
    }

    #[test]
    fn alt_without_meta_falls_through_to_key_char() {
        // alt_is_meta=false면 플랫폼이 만들어준 문자를 그대로 보낸다.
        let mut k = ks("b", alt());
        k.key_char = Some("∫".to_string());
        assert_eq!(to_esc_str(&k, TermMode::NONE, false).as_deref(), Some("∫"));
    }

    #[test]
    fn ctrl_alt_combines_meta_prefix_with_the_control_code() {
        let m = Modifiers { control: true, alt: true, ..Default::default() };
        assert_eq!(esc("a", m).as_deref(), Some("\x1b\x01"));
    }

    #[test]
    fn plain_characters_pass_through() {
        assert_eq!(esc("a", none()).as_deref(), Some("a"));
        assert_eq!(esc("a", shift()).as_deref(), Some("A"));
    }

    #[test]
    fn cmd_combinations_are_not_sent_to_the_pty() {
        // cmd-c / cmd-v 는 뷰가 처리한다
        assert_eq!(esc("c", cmd()), None);
        assert_eq!(esc("v", cmd()), None);
        let cmd_shift = Modifiers { platform: true, shift: true, ..Default::default() };
        assert_eq!(esc("k", cmd_shift), None);
    }

    #[test]
    fn unknown_keys_yield_nothing() {
        assert_eq!(esc("capslock", none()), None);
        assert_eq!(esc("f1", cmd()), None);
    }

    #[test]
    fn bytes_helper_matches_the_string_form() {
        let k = ks("up", none());
        assert_eq!(to_esc_bytes(&k, TermMode::NONE, true), Some(b"\x1b[A".to_vec()));
    }
}
