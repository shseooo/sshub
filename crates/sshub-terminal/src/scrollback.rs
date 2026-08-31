//! 스크롤백 영속화 — 그리드를 ANSI 텍스트로 직렬화한다 (DESIGN-terminal.md §7).
//!
//! raw PTY 링버퍼가 아니라 그리드를 직렬화하는 이유:
//!  - 기존 Electron 파일(SerializeAddon 출력)과 개념이 같아 호환된다
//!  - 유계·결정적이다 (raw ring은 alt-screen 쓰레기를 그대로 재생한다)
//!  - 복원이 `inject_local` feed 한 번으로 끝난다
//!
//! 출력은 SGR 런 단위로만 속성을 바꾸고, soft wrap(WRAPLINE)은 개행 없이
//! 이어 붙이며, 행말 공백 셀은 버린다.

use crate::backend::{
    AlacColor, Cell, Dimensions, EventListener, Flags, Line, NamedColor, Term,
};

pub use crate::backend::{LIVE_SCROLLBACK_LINES, PERSISTED_SCROLLBACK_LINES};

/// 직렬화가 재현하는 속성만 추린 것. 이 값이 바뀔 때만 SGR을 방출한다.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: AlacColor,
    bg: AlacColor,
    flags: Flags,
}

/// 재현 대상 플래그 (WIDE_CHAR/WRAPLINE 같은 레이아웃 플래그는 제외).
fn styled_flags(flags: Flags) -> Flags {
    flags
        & (Flags::BOLD
            | Flags::DIM
            | Flags::ITALIC
            | Flags::UNDERLINE
            | Flags::DOUBLE_UNDERLINE
            | Flags::INVERSE
            | Flags::HIDDEN
            | Flags::STRIKEOUT)
}

impl Style {
    fn of(cell: &Cell) -> Style {
        Style { fg: cell.fg, bg: cell.bg, flags: styled_flags(cell.flags) }
    }

    fn default_style() -> Style {
        Style {
            fg: AlacColor::Named(NamedColor::Foreground),
            bg: AlacColor::Named(NamedColor::Background),
            flags: Flags::empty(),
        }
    }

    fn is_default(&self) -> bool {
        *self == Style::default_style()
    }

    /// `ESC [ 0 ; … m` — 항상 reset으로 시작해 이전 상태에 의존하지 않는다.
    fn sgr(&self) -> String {
        if self.is_default() {
            return "\x1b[0m".to_string();
        }
        let mut params: Vec<String> = vec!["0".to_string()];
        if self.flags.contains(Flags::BOLD) {
            params.push("1".into());
        }
        if self.flags.contains(Flags::DIM) {
            params.push("2".into());
        }
        if self.flags.contains(Flags::ITALIC) {
            params.push("3".into());
        }
        if self.flags.contains(Flags::UNDERLINE) {
            params.push("4".into());
        }
        if self.flags.contains(Flags::DOUBLE_UNDERLINE) {
            params.push("21".into());
        }
        if self.flags.contains(Flags::INVERSE) {
            params.push("7".into());
        }
        if self.flags.contains(Flags::HIDDEN) {
            params.push("8".into());
        }
        if self.flags.contains(Flags::STRIKEOUT) {
            params.push("9".into());
        }
        if let Some(p) = color_params(self.fg, true) {
            params.push(p);
        }
        if let Some(p) = color_params(self.bg, false) {
            params.push(p);
        }
        format!("\x1b[{}m", params.join(";"))
    }
}

/// 색 하나를 SGR 파라미터로. 기본색이면 None (reset이 이미 처리).
fn color_params(color: AlacColor, is_fg: bool) -> Option<String> {
    let base = if is_fg { 30 } else { 40 };
    let bright_base = if is_fg { 90 } else { 100 };
    let extended = if is_fg { 38 } else { 48 };
    match color {
        AlacColor::Named(named) => {
            let idx = match named {
                NamedColor::Black => 0,
                NamedColor::Red => 1,
                NamedColor::Green => 2,
                NamedColor::Yellow => 3,
                NamedColor::Blue => 4,
                NamedColor::Magenta => 5,
                NamedColor::Cyan => 6,
                NamedColor::White => 7,
                NamedColor::BrightBlack => 8,
                NamedColor::BrightRed => 9,
                NamedColor::BrightGreen => 10,
                NamedColor::BrightYellow => 11,
                NamedColor::BrightBlue => 12,
                NamedColor::BrightMagenta => 13,
                NamedColor::BrightCyan => 14,
                NamedColor::BrightWhite => 15,
                // Foreground/Background/Cursor/Dim*/Bright(Fore|Back)ground —
                // 기본색으로 되돌리면 되므로 파라미터가 필요 없다.
                _ => return None,
            };
            Some(if idx < 8 {
                (base + idx).to_string()
            } else {
                (bright_base + idx - 8).to_string()
            })
        }
        AlacColor::Indexed(i) => Some(format!("{extended};5;{i}")),
        AlacColor::Spec(rgb) => Some(format!("{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b)),
    }
}

/// 셀이 "빈 칸"인가 — 문자가 공백이고 눈에 보이는 배경/속성이 없는 경우.
fn is_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && matches!(cell.bg, AlacColor::Named(NamedColor::Background))
        && !cell.flags.intersects(Flags::INVERSE | Flags::UNDERLINE | Flags::STRIKEOUT)
}

/// 히스토리 + 화면의 마지막 `max_lines` 행을 ANSI로 직렬화한다.
pub fn serialize<T: EventListener>(term: &Term<T>, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    let grid = term.grid();
    let columns = grid.columns();
    let bottom = grid.bottommost_line().0;
    let top = grid.topmost_line().0;

    // 뒤쪽의 완전히 빈 행은 저장하지 않는다. 상한(max_lines)은 이 "마지막 내용
    // 행"을 기준으로 세어야 빈 행이 예산을 잡아먹지 않는다.
    let mut last = None;
    for l in top..=bottom {
        let row = &grid[Line(l)];
        if (0..columns).any(|c| !is_blank(&row[crate::backend::Column(c)])) {
            last = Some(l);
        }
    }
    let Some(last) = last else {
        return String::new();
    };
    let start = (last - max_lines as i32 + 1).max(top);

    let mut out = String::new();
    let mut style = Style::default_style();

    for l in start..=last {
        let row = &grid[Line(l)];
        let wrapped = row[crate::backend::Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        // soft wrap 행은 꽉 찬 행이므로 트리밍하지 않는다.
        let width = if wrapped {
            columns
        } else {
            let mut w = 0;
            for c in 0..columns {
                if !is_blank(&row[crate::backend::Column(c)]) {
                    w = c + 1;
                }
            }
            w
        };

        for c in 0..width {
            let cell = &row[crate::backend::Column(c)];
            // 와이드 문자의 뒤쪽 자리는 앞 셀이 이미 표현한다.
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let cell_style = Style::of(cell);
            // 기본 스타일로 시작하므로 첫 셀이 기본이면 SGR을 내보내지 않는다.
            if cell_style != style {
                out.push_str(&cell_style.sgr());
                style = cell_style;
            }
            out.push(cell.c);
            if let Some(zw) = cell.zerowidth() {
                out.extend(zw.iter().copied());
            }
        }

        // soft wrap은 개행 없이 다음 행으로 이어진다.
        if !wrapped && l != last {
            // 다음 줄이 기본 배경으로 시작하도록 리셋 후 개행 (배경색이 줄 끝까지
            // 번지는 것을 막는다).
            if !style.is_default() {
                out.push_str("\x1b[0m");
                style = Style::default_style();
            }
            out.push_str("\r\n");
        }
    }

    if !style.is_default() {
        out.push_str("\x1b[0m");
    }
    out
}

/// 마지막 `max_lines` 내용 행을 **평문**으로 (드래그 미리보기용).
///
/// `serialize`와 같은 행 선택 규칙을 쓰되 SGR·soft wrap 이어붙이기는 하지
/// 않는다 — 화면에 잠깐 보여 줄 그림이라 색이나 줄 이어짐이 의미가 없고,
/// 행 하나가 곧 카드의 한 줄이어야 한다.
pub fn plain_tail<T: EventListener>(term: &Term<T>, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let grid = term.grid();
    let columns = grid.columns();
    let bottom = grid.bottommost_line().0;
    let top = grid.topmost_line().0;

    let mut last = None;
    for l in top..=bottom {
        let row = &grid[Line(l)];
        if (0..columns).any(|c| !is_blank(&row[crate::backend::Column(c)])) {
            last = Some(l);
        }
    }
    let Some(last) = last else {
        return Vec::new();
    };
    let start = (last - max_lines as i32 + 1).max(top);

    (start..=last)
        .map(|l| {
            let row = &grid[Line(l)];
            let mut width = 0;
            for c in 0..columns {
                if !is_blank(&row[crate::backend::Column(c)]) {
                    width = c + 1;
                }
            }
            let mut text = String::new();
            for c in 0..width {
                let cell = &row[crate::backend::Column(c)];
                // 와이드 문자의 뒤쪽 자리는 앞 셀이 이미 표현한다.
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(cell.c);
            }
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AnsiProcessor, TermConfig, TermSize, VoidListener};

    fn make_term(cols: usize, lines: usize, input: &[u8]) -> Term<VoidListener> {
        let config = TermConfig { scrolling_history: 200, ..Default::default() };
        let mut term = Term::new(config, &TermSize::new(cols, lines), VoidListener);
        let mut parser = AnsiProcessor::new();
        parser.advance(&mut term, input);
        term
    }

    /// 화면 전체를 행 단위 텍스트로 (행말 공백 제거, 뒤쪽 빈 행 제거).
    fn screen_lines<T: EventListener>(term: &Term<T>) -> Vec<String> {
        let grid = term.grid();
        let columns = grid.columns();
        let mut rows = Vec::new();
        for l in 0..grid.screen_lines() as i32 {
            let row = &grid[Line(l)];
            let mut s = String::new();
            for c in 0..columns {
                let cell = &row[crate::backend::Column(c)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                s.push(cell.c);
            }
            rows.push(s.trim_end().to_string());
        }
        while rows.last().map(|r| r.is_empty()).unwrap_or(false) {
            rows.pop();
        }
        rows
    }

    #[test]
    fn plain_text_round_trips() {
        let term = make_term(20, 5, b"hello\r\nworld\r\n");
        let ansi = serialize(&term, 100);
        let restored = make_term(20, 5, ansi.as_bytes());
        assert_eq!(screen_lines(&restored), vec!["hello", "world"]);
        assert_eq!(screen_lines(&restored), screen_lines(&term));
    }

    #[test]
    fn colored_text_round_trips() {
        let term = make_term(40, 5, b"\x1b[31mred\x1b[0m normal \x1b[1;34mbold blue\x1b[0m\r\n");
        let ansi = serialize(&term, 100);
        let restored = make_term(40, 5, ansi.as_bytes());
        assert_eq!(screen_lines(&restored), screen_lines(&term));

        // 색이 실제로 보존됐는지 셀 단위로 확인
        let orig = term.grid()[Line(0)][crate::backend::Column(0)].fg;
        let back = restored.grid()[Line(0)][crate::backend::Column(0)].fg;
        assert_eq!(orig, back);
        assert_eq!(back, AlacColor::Named(NamedColor::Red));
    }

    #[test]
    fn truecolor_and_indexed_colors_round_trip() {
        let input = b"\x1b[38;2;10;20;30mtc\x1b[0m \x1b[38;5;208midx\x1b[0m\r\n";
        let term = make_term(40, 5, input);
        let ansi = serialize(&term, 100);
        let restored = make_term(40, 5, ansi.as_bytes());
        assert_eq!(
            restored.grid()[Line(0)][crate::backend::Column(0)].fg,
            term.grid()[Line(0)][crate::backend::Column(0)].fg
        );
        assert_eq!(
            restored.grid()[Line(0)][crate::backend::Column(3)].fg,
            AlacColor::Indexed(208)
        );
    }

    #[test]
    fn cjk_wide_characters_round_trip() {
        let term = make_term(20, 5, "가나다 abc 漢字\r\n".as_bytes());
        let ansi = serialize(&term, 100);
        let restored = make_term(20, 5, ansi.as_bytes());
        assert_eq!(screen_lines(&restored), vec!["가나다 abc 漢字"]);
        assert_eq!(screen_lines(&restored), screen_lines(&term));
    }

    #[test]
    fn soft_wrapped_lines_stay_joined() {
        // 10열에 15글자 → 자동 줄바꿈(WRAPLINE)
        let term = make_term(10, 5, b"abcdefghijklmno\r\n");
        let ansi = serialize(&term, 100);
        // soft wrap 지점에 개행이 들어가면 안 된다
        assert!(!ansi.contains("abcdefghij\r\n"), "soft wrap을 hard newline으로 저장함: {ansi:?}");
        let restored = make_term(10, 5, ansi.as_bytes());
        assert_eq!(screen_lines(&restored), screen_lines(&term));
    }

    #[test]
    fn trailing_blanks_are_stripped() {
        let term = make_term(20, 5, b"hi        \r\n");
        let ansi = serialize(&term, 100);
        assert_eq!(ansi, "hi");
    }

    #[test]
    fn trailing_blank_rows_are_dropped() {
        let term = make_term(20, 10, b"one\r\ntwo\r\n\r\n\r\n");
        let ansi = serialize(&term, 100);
        assert_eq!(ansi, "one\r\ntwo");
    }

    #[test]
    fn max_lines_keeps_only_the_tail() {
        let mut input = Vec::new();
        for i in 0..20 {
            input.extend_from_slice(format!("line{i}\r\n").as_bytes());
        }
        let term = make_term(20, 5, &input);
        let ansi = serialize(&term, 3);
        let lines: Vec<&str> = ansi.split("\r\n").collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("line19"), "got {lines:?}");
    }

    #[test]
    fn zero_max_lines_is_empty() {
        let term = make_term(20, 5, b"hello\r\n");
        assert_eq!(serialize(&term, 0), "");
    }

    #[test]
    fn empty_terminal_serializes_to_nothing() {
        let term = make_term(20, 5, b"");
        assert_eq!(serialize(&term, 100), "");
    }

    #[test]
    fn history_beyond_the_screen_is_included() {
        let mut input = Vec::new();
        for i in 0..12 {
            input.extend_from_slice(format!("row{i}\r\n").as_bytes());
        }
        // 화면은 4행뿐이므로 나머지는 히스토리로 밀려난다
        let term = make_term(20, 4, &input);
        let ansi = serialize(&term, 100);
        assert!(ansi.contains("row0"), "히스토리가 빠짐: {ansi:?}");
        assert!(ansi.contains("row11"));
    }

    #[test]
    fn attributes_round_trip() {
        let term = make_term(40, 5, b"\x1b[1mbold\x1b[0m \x1b[4munder\x1b[0m \x1b[7minv\x1b[0m\r\n");
        let ansi = serialize(&term, 100);
        let restored = make_term(40, 5, ansi.as_bytes());
        let c = |t: &Term<VoidListener>, col: usize| {
            styled_flags(t.grid()[Line(0)][crate::backend::Column(col)].flags)
        };
        assert!(c(&restored, 0).contains(Flags::BOLD));
        assert!(c(&restored, 5).contains(Flags::UNDERLINE));
        assert!(c(&restored, 11).contains(Flags::INVERSE));
        assert_eq!(c(&restored, 0), c(&term, 0));
    }

    #[test]
    fn serialize_is_idempotent_across_a_second_round_trip() {
        let term = make_term(30, 6, b"\x1b[32mgreen\x1b[0m\r\nplain\r\n");
        let once = serialize(&term, 100);
        let twice = serialize(&make_term(30, 6, once.as_bytes()), 100);
        assert_eq!(once, twice);
    }

    #[test]
    fn persisted_cap_is_smaller_than_the_live_buffer() {
        assert_eq!(PERSISTED_SCROLLBACK_LINES, 1_000);
        assert_eq!(LIVE_SCROLLBACK_LINES, 20_000);
    }

    #[test]
    fn plain_tail_keeps_the_last_lines_without_escapes() {
        // 드래그 미리보기는 색이 아니라 "무엇이 떠 있는지"를 보여 준다.
        let term = make_term(
            20,
            6,
            b"one\r\ntwo\r\n\x1b[31mthree\x1b[0m\r\nfour   \r\n",
        );
        let tail = plain_tail(&term, 3);
        assert_eq!(tail, vec!["two", "three", "four"], "행말 공백·SGR 제거");
        assert!(
            tail.iter().all(|l| !l.contains('\x1b')),
            "평문이어야 카드에 그대로 그릴 수 있다"
        );
        assert_eq!(plain_tail(&term, 0), Vec::<String>::new());
    }

    #[test]
    fn plain_tail_of_an_empty_screen_is_empty() {
        let term = make_term(20, 6, b"");
        assert!(plain_tail(&term, 5).is_empty(), "빈 화면엔 보여 줄 게 없다");
    }
}
