//! 내장 터미널 폰트.
//!
//! macOS에는 한글 **고정폭** 폰트가 없다(AppleGothic·Apple SD Gothic Neo 모두
//! 가변폭). 그래서 Menlo로 두면 한글이 폴백 폰트로 그려지고, 그 글자 폭이
//! 터미널 격자의 2셀과 달라 글자마다 여백이 남는다.
//!
//! D2Coding은 ASCII 0.5em / 한글 1.0em으로 **정확히 2배**라 격자에 빈틈없이
//! 맞는다. 사용자 환경에 의존하지 않도록 바이너리에 넣고 실행 시 등록한다.
//! (라이선스: SIL Open Font License 1.1 — assets/fonts/OFL.txt)

use std::borrow::Cow;

use gpui::App;

/// 내장 폰트 패밀리 이름 (TTF의 name 테이블 nameID 1과 일치해야 한다).
pub const EMBEDDED_FAMILY: &str = "D2Coding";

/// 내장 폰트를 못 쓸 때의 대체 — 한글 폭은 안 맞지만 ASCII는 정상이다.
pub const SYSTEM_FALLBACK: &str = "Menlo";

const REGULAR: &[u8] = include_bytes!("../assets/fonts/D2Coding.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/D2CodingBold.ttf");

/// 내장 폰트를 텍스트 시스템에 등록한다. 부트스트랩에서 1회.
/// 실패해도 앱은 떠야 하므로 결과만 알려주고 넘어간다.
pub fn register(cx: &mut App) -> bool {
    match cx
        .text_system()
        .add_fonts(vec![Cow::Borrowed(REGULAR), Cow::Borrowed(BOLD)])
    {
        Ok(()) => true,
        Err(error) => {
            eprintln!("sshub: 내장 폰트 등록 실패 — {SYSTEM_FALLBACK}로 대체합니다: {error}");
            false
        }
    }
}

/// 설정값(비어 있으면 내장 폰트)에서 실제 사용할 패밀리를 고른다.
pub fn resolve_family(configured: Option<&str>, embedded_ok: bool) -> String {
    match configured.map(str::trim).filter(|f| !f.is_empty()) {
        Some(family) => family.to_string(),
        None if embedded_ok => EMBEDDED_FAMILY.to_string(),
        None => SYSTEM_FALLBACK.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_font_is_bundled_and_looks_like_a_ttf() {
        // 0x00010000 = TrueType sfnt 시그니처. 파일이 LFS 포인터 등으로
        // 바뀌면 여기서 걸린다.
        assert!(REGULAR.len() > 1_000_000, "regular 폰트가 비어 있다");
        assert!(BOLD.len() > 1_000_000, "bold 폰트가 비어 있다");
        assert_eq!(&REGULAR[..4], &[0x00, 0x01, 0x00, 0x00]);
        assert_eq!(&BOLD[..4], &[0x00, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn configured_family_wins_over_the_embedded_one() {
        assert_eq!(resolve_family(Some("SF Mono"), true), "SF Mono");
        assert_eq!(resolve_family(Some("  "), true), EMBEDDED_FAMILY);
        assert_eq!(resolve_family(None, true), EMBEDDED_FAMILY);
    }

    #[test]
    fn falls_back_to_a_system_font_when_registration_failed() {
        assert_eq!(resolve_family(None, false), SYSTEM_FALLBACK);
        // 사용자가 고른 폰트는 등록 실패와 무관하게 존중한다.
        assert_eq!(resolve_family(Some("Monaco"), false), "Monaco");
    }
}
