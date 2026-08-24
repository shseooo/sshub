//! 번들된 한글 폰트 검증.
//!
//! 한글이 ASCII의 **정확히 2배** 폭이어야 터미널 격자와 맞는다. 이 비율이
//! 깨지면 한글 텍스트가 커서보다 짧아 보이고 조합 글자가 몇 칸 뒤에 뜬 것처럼
//! 보인다(실제로 겪은 증상). 폰트 파일을 갈아끼울 때 여기서 걸린다.
//!
//! gpui 테스트 플랫폼은 폰트를 등록하지 않는 no-op 텍스트 시스템이라 런타임
//! 등록은 검증할 수 없다 — 파일 자체를 본다.

const REGULAR: &[u8] = include_bytes!("../assets/fonts/D2Coding.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/D2CodingBold.ttf");

fn family_of(face: &ttf_parser::Face<'_>) -> String {
    // FAMILY 항목은 플랫폼별로 여러 개다. Macintosh 인코딩은 to_string()이
    // None을 주므로 "읽히는 첫 항목"을 골라야 한다.
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::FAMILY)
        .find_map(|n| n.to_string())
        .unwrap_or_default()
}

fn advance(face: &ttf_parser::Face<'_>, ch: char) -> u16 {
    let gid = face
        .glyph_index(ch)
        .unwrap_or_else(|| panic!("'{ch}' 글리프가 폰트에 없다"));
    face.glyph_hor_advance(gid)
        .unwrap_or_else(|| panic!("'{ch}' 가로 폭을 읽지 못했다"))
}

fn check(data: &[u8], label: &str) {
    let face = ttf_parser::Face::parse(data, 0).expect("TTF 파싱 실패");
    assert_eq!(
        family_of(&face),
        sshub::fonts::EMBEDDED_FAMILY,
        "{label}: 패밀리 이름이 코드 상수와 다르면 gpui가 폰트를 못 찾는다"
    );

    let ascii = advance(&face, 'A') as f32;
    assert!(ascii > 0.0, "{label}: ASCII 폭이 0");
    for ch in ['한', '글', '漢', '가'] {
        let ratio = advance(&face, ch) as f32 / ascii;
        assert!(
            (ratio - 2.0).abs() < 0.001,
            "{label}: '{ch}' 폭 비율이 2가 아님 ({ratio})"
        );
    }

    // 고정폭 확인 — ASCII끼리 폭이 다르면 터미널 격자가 어긋난다.
    for ch in ['i', 'W', '.', '0'] {
        assert_eq!(advance(&face, ch) as f32, ascii, "{label}: '{ch}'가 고정폭이 아님");
    }
}

#[test]
fn bundled_regular_font_is_grid_aligned_for_hangul() {
    check(REGULAR, "regular");
}

#[test]
fn bundled_bold_font_matches_the_regular_metrics() {
    // 볼드가 다른 폭이면 굵은 글자에서만 정렬이 깨진다.
    check(BOLD, "bold");
    let r = ttf_parser::Face::parse(REGULAR, 0).unwrap();
    let b = ttf_parser::Face::parse(BOLD, 0).unwrap();
    assert_eq!(r.units_per_em(), b.units_per_em());
    assert_eq!(advance(&r, 'A'), advance(&b, 'A'));
}
