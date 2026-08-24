//! Zed풍 미니멀 다크 테마 토큰 (DESIGN-ui.md §4). dark 전용.
//! 어센트/터미널 fg·bg/반투명은 설정에서 오버라이드된다.
use gpui::{rgb, rgba, Hsla, Rgba};

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Hsla,
    pub surface: Hsla,
    pub elevated: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub border: Hsla,
    pub border_subtle: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_disabled: Hsla,
    pub accent: Hsla,
    pub accent_wash: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub terminal: TerminalTheme,
    /// 0..=40 (%) — bg에 알파로 구움 (macOS Blurred 배경이 비침)
    pub translucency: u8,
}

#[derive(Clone, Debug)]
pub struct TerminalTheme {
    /// 터미널 고정폭 폰트 패밀리 (기본은 내장 D2Coding — `crate::fonts`).
    pub font_family: String,
    pub foreground: Rgba,
    pub background: Rgba,
    pub cursor: Rgba,
    /// ANSI 0-7, 8-15 (One Dark)
    pub palette: [Rgba; 16],
    pub font_size: f32,
}

fn h(color: u32) -> Hsla {
    Hsla::from(rgb(color))
}

pub const ACCENT_PRESETS: [(&str, u32); 4] = [
    ("blue", 0x74ade8),
    ("green", 0xa1c181),
    ("amber", 0xdec184),
    ("magenta", 0xb477cf),
];

impl Theme {
    pub fn default_dark() -> Self {
        Self::with_overrides(0x74ade8, 0, None, None, 14.0, crate::fonts::EMBEDDED_FAMILY.to_string())
    }

    pub fn with_overrides(
        accent: u32,
        translucency: u8,
        term_fg: Option<u32>,
        term_bg: Option<u32>,
        term_font_size: f32,
        term_font_family: String,
    ) -> Self {
        let translucency = translucency.min(40);
        let accent_hsla = h(accent);
        let mut accent_wash = accent_hsla;
        accent_wash.a = 0.14;
        // 반투명은 루트 bg에만 알파를 구움 (카드/터미널은 불투명 유지)
        let bg_alpha = 1.0 - f32::from(translucency) / 100.0;
        let bg = Hsla::from(rgba((0x16181d << 8) | ((bg_alpha * 255.0) as u32)));
        Self {
            bg,
            surface: h(0x1c1e24),
            elevated: h(0x22252c),
            hover: h(0x282b33),
            selected: h(0x2e323b),
            border: h(0x2d313a),
            border_subtle: h(0x23262d),
            text: h(0xd6d9de),
            text_muted: h(0x8b909a),
            text_disabled: h(0x565b65),
            accent: accent_hsla,
            accent_wash,
            danger: h(0xd07277),
            success: h(0x98c379),
            warning: h(0xdec184),
            terminal: TerminalTheme {
                font_family: term_font_family,
                foreground: rgb(term_fg.unwrap_or(0xc8ccd4)),
                background: rgb(term_bg.unwrap_or(0x16181d)),
                cursor: rgb(accent),
                palette: [
                    rgb(0x282c34),
                    rgb(0xe06c75),
                    rgb(0x98c379),
                    rgb(0xe5c07b),
                    rgb(0x61afef),
                    rgb(0xc678dd),
                    rgb(0x56b6c2),
                    rgb(0xabb2bf),
                    rgb(0x5c6370),
                    rgb(0xe06c75),
                    rgb(0x98c379),
                    rgb(0xe5c07b),
                    rgb(0x61afef),
                    rgb(0xc678dd),
                    rgb(0x56b6c2),
                    rgb(0xffffff),
                ],
                font_size: term_font_size.clamp(10.0, 24.0),
            },
            translucency,
        }
    }
}

impl gpui::Global for Theme {}

/// 활성 테마 참조 (모든 뷰 render에서 사용).
pub fn theme(cx: &gpui::App) -> &Theme {
    cx.global::<Theme>()
}

/// 전역 테마 등록 (앱/예제 부트스트랩에서 1회 호출).
pub fn init(cx: &mut gpui::App) {
    if !cx.has_global::<Theme>() {
        cx.set_global(Theme::default_dark());
    }
}

/// 알파를 덮어쓴 사본 (선택 하이라이트·워시 등).
pub fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}
