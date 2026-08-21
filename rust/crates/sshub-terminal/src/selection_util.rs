//! `src/lib/selection.ts` 포팅.
//!
//! 셀 기반 선택은 각 행을 "가장 오른쪽 선택 열"까지 공백으로 패딩해서 돌려준다.
//! TUI/박스드로잉 출력을 복사하면 행마다 공백이 줄줄이 딸려오므로, 사용자가 눈으로
//! 고른 것과 붙여넣기 결과가 같아지도록 행말 공백/탭만 제거한다.
//! Windows 줄끝의 `\r`는 보존한다 (원본 정규식 `/[ \t]+(\r?)$/` → `'$1'`).

/// 각 행의 행말 공백/탭을 제거한다. `\r`는 남긴다.
pub fn trim_selection_trailing(selection: &str) -> String {
    let mut out = String::with_capacity(selection.len());
    for (i, line) in selection.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // `\r`는 보존 대상이지만 잘라낼 공백 런의 **뒤**에 있다. 하나의 슬라이스로는
        // 표현할 수 없으므로 몸통과 `\r`를 따로 붙인다.
        let (body, cr) = match line.strip_suffix('\r') {
            Some(body) => (body, true),
            None => (line, false),
        };
        out.push_str(body.trim_end_matches([' ', '\t']));
        if cr {
            out.push('\r');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(input: &str) -> String {
        trim_selection_trailing(input)
    }

    #[test]
    fn strips_trailing_spaces_per_line() {
        assert_eq!(t("foo   \nbar\t\t\nbaz"), "foo\nbar\nbaz");
    }

    #[test]
    fn preserves_carriage_return() {
        assert_eq!(t("foo   \r\nbar\r"), "foo\r\nbar\r");
    }

    #[test]
    fn keeps_leading_and_interior_whitespace() {
        assert_eq!(t("   foo  bar   "), "   foo  bar");
    }

    #[test]
    fn blank_lines_collapse_to_empty() {
        assert_eq!(t("a\n    \nb"), "a\n\nb");
        assert_eq!(t("   \r"), "\r");
    }

    #[test]
    fn no_trailing_whitespace_is_identity() {
        let s = "가나다 abc 漢字\nnext line";
        assert_eq!(t(s), s);
    }

    #[test]
    fn multibyte_line_is_not_sliced_mid_char() {
        // 행말 공백 제거가 UTF-8 경계를 깨지 않는지 (패닉 회귀 가드)
        assert_eq!(t("漢字   "), "漢字");
        assert_eq!(t("🚀\t"), "🚀");
    }

    #[test]
    fn empty_input() {
        assert_eq!(t(""), "");
    }

    #[test]
    fn tabs_and_spaces_mixed() {
        assert_eq!(t("x \t \t"), "x");
    }
}
