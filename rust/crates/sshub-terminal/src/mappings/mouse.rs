//! 마우스 리포트 인코딩 (Zed `terminal/src/mappings/mouse.rs` 포팅).
//!
//! 앱이 마우스 모드를 켜 놓았을 때만 리포트를 보낸다. shift가 눌려 있으면
//! 사용자가 "앱 말고 나에게 선택을 달라"는 뜻이므로 호출부가 리포트를 건너뛴다.

use gpui::{Modifiers, MouseButton};

use crate::backend::{AlacPoint, Line, TermMode};

/// 리포트 종류를 결정하는 세 가지 인코딩.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseFormat {
    Sgr,
    /// `utf8` = UTF8_MOUSE 확장 (223열 한계를 2015열로 넓힌다)
    Normal(bool),
}

impl MouseFormat {
    fn from_mode(mode: TermMode) -> MouseFormat {
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseFormat::Sgr
        } else {
            MouseFormat::Normal(mode.contains(TermMode::UTF8_MOUSE))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMotion {
    Press,
    Release,
    Drag,
    Move,
}

/// 리포트의 버튼 코드. 이동/드래그는 +32, 휠은 +64가 관례다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AlacMouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    LeftMove = 32,
    MiddleMove = 33,
    RightMove = 34,
    NoneMove = 35,
    ScrollUp = 64,
    ScrollDown = 65,
    Other = 99,
}

impl AlacMouseButton {
    fn from_press(button: MouseButton) -> AlacMouseButton {
        match button {
            MouseButton::Left => AlacMouseButton::Left,
            MouseButton::Middle => AlacMouseButton::Middle,
            MouseButton::Right => AlacMouseButton::Right,
            _ => AlacMouseButton::Other,
        }
    }

    fn from_move(pressed: Option<MouseButton>) -> AlacMouseButton {
        match pressed {
            Some(MouseButton::Left) => AlacMouseButton::LeftMove,
            Some(MouseButton::Middle) => AlacMouseButton::MiddleMove,
            Some(MouseButton::Right) => AlacMouseButton::RightMove,
            Some(_) => AlacMouseButton::Other,
            None => AlacMouseButton::NoneMove,
        }
    }

    fn is_other(self) -> bool {
        self == AlacMouseButton::Other
    }
}

/// 수식키 비트: shift 4, alt 8, ctrl 16.
fn modifier_bits(modifiers: Modifiers) -> u8 {
    let mut bits = 0;
    if modifiers.shift {
        bits += 4;
    }
    if modifiers.alt {
        bits += 8;
    }
    if modifiers.control {
        bits += 16;
    }
    bits
}

/// 앱이 이 이벤트를 원하는가? (MOUSE_MODE ∧ ¬shift — DESIGN-terminal.md §4)
pub fn should_report(mode: TermMode, modifiers: Modifiers) -> bool {
    mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift
}

/// 버튼 누름/뗌 리포트.
pub fn mouse_button_report(
    point: AlacPoint,
    button: MouseButton,
    modifiers: Modifiers,
    pressed: bool,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = AlacMouseButton::from_press(button);
    if button.is_other() {
        return None;
    }
    let motion = if pressed { MouseMotion::Press } else { MouseMotion::Release };
    mouse_report(point, button, modifiers, motion, mode)
}

/// 이동/드래그 리포트. MOUSE_MOTION(모든 이동) 또는 MOUSE_DRAG(버튼 눌린 이동)
/// 가 켜져 있을 때만 유효하다.
pub fn mouse_moved_report(
    point: AlacPoint,
    pressed_button: Option<MouseButton>,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = AlacMouseButton::from_move(pressed_button);
    if button.is_other() {
        return None;
    }
    let dragging = pressed_button.is_some();
    // 버튼을 안 누른 이동은 MOUSE_MOTION일 때만 보고한다.
    if !dragging && !mode.contains(TermMode::MOUSE_MOTION) {
        return None;
    }
    if dragging && !mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
        return None;
    }
    let motion = if dragging { MouseMotion::Drag } else { MouseMotion::Move };
    mouse_report(point, button, modifiers, motion, mode)
}

/// 휠 리포트 — 스크롤한 라인 수만큼 같은 리포트를 반복한다.
pub fn scroll_report(
    point: AlacPoint,
    scroll_lines: i32,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<Vec<u8>>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    let button = if scroll_lines > 0 { AlacMouseButton::ScrollUp } else { AlacMouseButton::ScrollDown };
    let report = mouse_report(point, button, modifiers, MouseMotion::Press, mode)?;
    let count = scroll_lines.unsigned_abs().max(1) as usize;
    Some(vec![report; count])
}

/// 마우스 모드가 아닌 alt-screen에서의 휠 — 위/아래 화살표로 바꿔 보낸다
/// (less/man 같은 페이저가 휠로 스크롤되게 하는 관례).
pub fn alt_scroll(scroll_lines: i32) -> Vec<u8> {
    let cmd = if scroll_lines > 0 { b'A' } else { b'B' };
    let count = scroll_lines.unsigned_abs() as usize;
    let mut out = Vec::with_capacity(count * 3);
    for _ in 0..count {
        out.extend_from_slice(&[0x1b, b'O', cmd]);
    }
    out
}

fn mouse_report(
    point: AlacPoint,
    button: AlacMouseButton,
    modifiers: Modifiers,
    motion: MouseMotion,
    mode: TermMode,
) -> Option<Vec<u8>> {
    // 히스토리(음수 라인)는 화면 좌표가 아니라 보고 대상이 아니다.
    if point.line < Line(0) {
        return None;
    }
    let code = button as u8 + modifier_bits(modifiers);
    match MouseFormat::from_mode(mode) {
        MouseFormat::Sgr => Some(sgr_mouse_report(point, code, motion)),
        MouseFormat::Normal(utf8) => {
            // Normal 인코딩에는 "버튼 없는 이동"을 표현할 자리가 없다.
            if motion == MouseMotion::Move && button == AlacMouseButton::NoneMove {
                return None;
            }
            normal_mouse_report(point, code, utf8)
        }
    }
}

fn sgr_mouse_report(point: AlacPoint, code: u8, motion: MouseMotion) -> Vec<u8> {
    let suffix = match motion {
        MouseMotion::Press | MouseMotion::Drag | MouseMotion::Move => 'M',
        MouseMotion::Release => 'm',
    };
    format!("\x1b[<{};{};{}{}", code, point.column.0 + 1, point.line.0 + 1, suffix).into_bytes()
}

fn normal_mouse_report(point: AlacPoint, code: u8, utf8: bool) -> Option<Vec<u8>> {
    let line = point.line.0 as usize;
    let column = point.column.0;
    let max = if utf8 { 2015 } else { 223 };
    if line >= max || column >= max {
        return None;
    }

    let mut msg = vec![0x1b, b'[', b'M', 32 + code];
    // 95를 넘어가면 단일 바이트로 표현할 수 없어 UTF-8 2바이트로 인코딩한다.
    let encode = |pos: usize, msg: &mut Vec<u8>| {
        let pos = 32 + 1 + pos;
        if utf8 && pos >= 127 {
            msg.push((0xC0 + pos / 64) as u8);
            msg.push((0x80 + (pos & 63)) as u8);
        } else {
            msg.push(pos as u8);
        }
    };
    encode(column, &mut msg);
    encode(line, &mut msg);
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Column;

    fn point(line: i32, col: usize) -> AlacPoint {
        AlacPoint::new(Line(line), Column(col))
    }

    fn no_mods() -> Modifiers {
        Modifiers::default()
    }

    fn sgr() -> TermMode {
        TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE
    }

    fn normal() -> TermMode {
        TermMode::MOUSE_REPORT_CLICK
    }

    fn s(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn sgr_press_and_release_use_uppercase_and_lowercase_terminators() {
        let press = mouse_button_report(point(0, 0), MouseButton::Left, no_mods(), true, sgr());
        assert_eq!(s(press.unwrap()), "\x1b[<0;1;1M");
        let release = mouse_button_report(point(0, 0), MouseButton::Left, no_mods(), false, sgr());
        assert_eq!(s(release.unwrap()), "\x1b[<0;1;1m");
    }

    #[test]
    fn sgr_coordinates_are_one_based() {
        let r = mouse_button_report(point(9, 4), MouseButton::Right, no_mods(), true, sgr());
        assert_eq!(s(r.unwrap()), "\x1b[<2;5;10M");
    }

    #[test]
    fn sgr_modifier_bits() {
        let m = Modifiers { shift: true, alt: true, control: true, ..Default::default() };
        // left(0) + shift(4) + alt(8) + ctrl(16) = 28
        let r = mouse_button_report(point(0, 0), MouseButton::Left, m, true, sgr());
        assert_eq!(s(r.unwrap()), "\x1b[<28;1;1M");
    }

    #[test]
    fn normal_encoding_offsets_by_32_plus_1() {
        let r = mouse_button_report(point(0, 0), MouseButton::Left, no_mods(), true, normal())
            .unwrap();
        assert_eq!(r, vec![0x1b, b'[', b'M', 32, 33, 33]);
        let r = mouse_button_report(point(2, 3), MouseButton::Middle, no_mods(), true, normal())
            .unwrap();
        assert_eq!(r, vec![0x1b, b'[', b'M', 33, 36, 35]);
    }

    #[test]
    fn normal_encoding_refuses_coordinates_past_223() {
        assert!(mouse_button_report(point(0, 223), MouseButton::Left, no_mods(), true, normal())
            .is_none());
        assert!(mouse_button_report(point(300, 0), MouseButton::Left, no_mods(), true, normal())
            .is_none());
    }

    #[test]
    fn utf8_mode_widens_the_coordinate_limit() {
        let mode = normal() | TermMode::UTF8_MOUSE;
        let r = mouse_button_report(point(0, 300), MouseButton::Left, no_mods(), true, mode);
        let r = r.unwrap();
        // 32 + 1 + 300 = 333 → 2바이트 인코딩
        assert_eq!(&r[..4], &[0x1b, b'[', b'M', 32]);
        assert_eq!(r.len(), 7);
    }

    #[test]
    fn history_lines_are_never_reported() {
        assert!(mouse_button_report(point(-1, 0), MouseButton::Left, no_mods(), true, sgr())
            .is_none());
    }

    #[test]
    fn drag_reports_add_32_to_the_button_code() {
        let mode = sgr() | TermMode::MOUSE_DRAG;
        let r = mouse_moved_report(point(0, 0), Some(MouseButton::Left), no_mods(), mode).unwrap();
        assert_eq!(s(r), "\x1b[<32;1;1M");
    }

    #[test]
    fn plain_motion_needs_mouse_motion_mode() {
        let drag_only = sgr() | TermMode::MOUSE_DRAG;
        assert!(mouse_moved_report(point(0, 0), None, no_mods(), drag_only).is_none());
        let motion = sgr() | TermMode::MOUSE_MOTION;
        let r = mouse_moved_report(point(0, 0), None, no_mods(), motion).unwrap();
        assert_eq!(s(r), "\x1b[<35;1;1M");
    }

    #[test]
    fn normal_encoding_drops_buttonless_motion() {
        let mode = normal() | TermMode::MOUSE_MOTION;
        assert!(mouse_moved_report(point(0, 0), None, no_mods(), mode).is_none());
    }

    #[test]
    fn scroll_report_repeats_once_per_line() {
        let up = scroll_report(point(0, 0), 3, no_mods(), sgr()).unwrap();
        assert_eq!(up.len(), 3);
        assert_eq!(s(up[0].clone()), "\x1b[<64;1;1M");
        let down = scroll_report(point(0, 0), -2, no_mods(), sgr()).unwrap();
        assert_eq!(down.len(), 2);
        assert_eq!(s(down[0].clone()), "\x1b[<65;1;1M");
    }

    #[test]
    fn scroll_report_is_none_outside_mouse_mode() {
        assert!(scroll_report(point(0, 0), 1, no_mods(), TermMode::NONE).is_none());
    }

    #[test]
    fn alt_scroll_emits_cursor_keys() {
        assert_eq!(s(alt_scroll(2)), "\x1bOA\x1bOA");
        assert_eq!(s(alt_scroll(-3)), "\x1bOB\x1bOB\x1bOB");
        assert_eq!(alt_scroll(0), Vec::<u8>::new());
    }

    #[test]
    fn should_report_respects_shift_override() {
        assert!(should_report(sgr(), no_mods()));
        let shift = Modifiers { shift: true, ..Default::default() };
        assert!(!should_report(sgr(), shift));
        assert!(!should_report(TermMode::NONE, no_mods()));
    }
}
