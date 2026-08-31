//! Zed풍 미니멀 다크 테마 토큰 (DESIGN-ui.md §4). dark 전용.
//! 어센트/터미널 fg·bg/반투명은 설정에서 오버라이드된다.
use gpui::{rgb, Hsla, Rgba, SharedString};

/// 반투명 슬라이더 상한(%). 이 위로는 글자가 배경에 묻혀 읽히지 않는다.
pub const TRANSLUCENCY_MAX: u8 = 40;

/// 반투명 설정(0..=`TRANSLUCENCY_MAX` %) → 알파. 0%면 1.0(불투명), 40%면 0.6.
/// 상한을 넘는 값은 잘라 낸다 — 손으로 고친 `sshub.json`도 여기서 방어된다.
pub fn translucency_alpha(translucency: u8) -> f32 {
    1.0 - f32::from(translucency.min(TRANSLUCENCY_MAX)) / 100.0
}

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
    /// 0..=`TRANSLUCENCY_MAX` (%) — 루트 bg와 터미널 표면에 알파로 구움
    /// (macOS `WindowBackgroundAppearance::Blurred` 배경이 비침).
    pub translucency: u8,
}

#[derive(Clone, Debug)]
pub struct TerminalTheme {
    /// 터미널 고정폭 폰트 패밀리 (기본은 내장 D2Coding — `crate::fonts`).
    ///
    /// `SharedString`인 이유: 위젯이 렌더마다 `Theme`를 통째로 클론한다. 나머지
    /// 필드는 전부 Copy라, 이 하나만 `String`이면 프레임마다 위젯 수만큼 힙
    /// 할당이 생긴다. Arc 백업이라 클론이 refcount 증가로 끝난다.
    pub font_family: SharedString,
    pub foreground: Rgba,
    /// 터미널 표면 색 — 반투명 알파가 구워져 있다. 창 배경이 비치는 곳은
    /// 사실상 여기뿐이라(터미널이 창의 대부분을 덮는다) 효과가 읽히는 층이다.
    pub background: Rgba,
    /// 같은 색의 알파 1.0 사본. 아래를 **가려야** 하는 곳 — IME 조합 오버레이,
    /// INVERSE 셀의 글자색 — 에서 쓴다. 여기에 알파가 섞이면 밑 글자가 비쳐
    /// 조합 중인 글자가 겹쳐 보인다.
    pub background_opaque: Rgba,
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
        Self::with_overrides(0x74ade8, 0, None, None, 14.0, crate::fonts::EMBEDDED_FAMILY.into())
    }

    pub fn with_overrides(
        accent: u32,
        translucency: u8,
        term_fg: Option<u32>,
        term_bg: Option<u32>,
        term_font_size: f32,
        term_font_family: SharedString,
    ) -> Self {
        let translucency = translucency.min(TRANSLUCENCY_MAX);
        let alpha = translucency_alpha(translucency);
        let accent_hsla = h(accent);
        let mut accent_wash = accent_hsla;
        accent_wash.a = 0.14;
        // 반투명 층은 한 픽셀에 **한 겹만** 올린다. 반투명 위에 반투명을 겹치면
        // 알파가 1-(1-a)²로 합성돼(0.6 두 겹 → 0.84) 슬라이더를 끝까지 올려도
        // 거의 비치지 않는다. 그래서 알파는 루트 bg와 터미널 표면에만 굽고,
        // 사이드바(surface)·카드(elevated)·모달·탭바·테두리는 불투명으로 둔다
        // (겹침 방지의 나머지 절반은 `workspace.rs` 렌더 트리가 맡는다).
        let mut bg = h(0x16181d);
        bg.a = alpha;
        let background_opaque = rgb(term_bg.unwrap_or(0x16181d));
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
                background: Rgba { a: alpha, ..background_opaque },
                background_opaque,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn theme_with(translucency: u8) -> Theme {
        Theme::with_overrides(0x74ade8, translucency, None, None, 14.0, "mono".into())
    }

    /// 폰트 패밀리·크기는 설정이 진실이고 테마가 그대로 실어 나른다.
    /// (클론 비용을 줄이려 타입을 바꿔도 이 통로는 그대로여야 한다.)
    #[test]
    fn the_terminal_font_comes_through_untouched() {
        let t = Theme::with_overrides(0x74ade8, 0, None, None, 15.5, "SF Mono".into());
        assert_eq!(t.terminal.font_family, "SF Mono");
        assert_eq!(t.terminal.font_size, 15.5);

        // 클론은 값이 같아야 한다 — 위젯이 렌더마다 테마를 복제한다.
        let copy = t.clone();
        assert_eq!(copy.terminal.font_family, t.terminal.font_family);
        assert_eq!(copy.terminal.palette, t.terminal.palette);
    }

    #[test]
    fn alpha_follows_the_percentage() {
        assert_eq!(translucency_alpha(0), 1.0);
        assert_eq!(translucency_alpha(20), 0.8);
        assert!((translucency_alpha(40) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn alpha_is_clamped_above_the_maximum() {
        assert_eq!(translucency_alpha(41), translucency_alpha(TRANSLUCENCY_MAX));
        assert_eq!(translucency_alpha(255), translucency_alpha(TRANSLUCENCY_MAX));
    }

    #[test]
    fn zero_translucency_keeps_every_layer_opaque() {
        let t = theme_with(0);
        assert_eq!(t.bg.a, 1.0);
        assert_eq!(t.terminal.background.a, 1.0);
        assert_eq!(t.terminal.background_opaque.a, 1.0);
    }

    #[test]
    fn translucency_reaches_the_root_bg_and_the_terminal_surface() {
        let t = theme_with(40);
        assert!((t.bg.a - 0.6).abs() < 1e-6);
        assert!((t.terminal.background.a - 0.6).abs() < 1e-6);
    }

    /// 사이드바·카드·테두리까지 비치면 글자가 읽히지 않는다 — 불투명 고정.
    #[test]
    fn opaque_tokens_stay_opaque_at_every_setting() {
        for translucency in [0u8, 20, 40, 255] {
            let t = theme_with(translucency);
            assert_eq!(t.surface.a, 1.0, "surface @ {translucency}");
            assert_eq!(t.elevated.a, 1.0, "elevated @ {translucency}");
            assert_eq!(t.border.a, 1.0, "border @ {translucency}");
            assert_eq!(t.border_subtle.a, 1.0, "border_subtle @ {translucency}");
            assert_eq!(t.text.a, 1.0, "text @ {translucency}");
            // IME 오버레이가 밑 글자를 가리려면 이쪽은 반드시 1.0이어야 한다.
            assert_eq!(t.terminal.background_opaque.a, 1.0, "term opaque @ {translucency}");
        }
    }

    /// 반투명이어도 색상 자체는 그대로여야 한다(알파만 다른 같은 색).
    #[test]
    fn terminal_background_keeps_its_colour_when_translucent() {
        let t = Theme::with_overrides(0x74ade8, 40, None, Some(0x102030), 14.0, "mono".into());
        let (a, b) = (t.terminal.background, t.terminal.background_opaque);
        assert_eq!((a.r, a.g, a.b), (b.r, b.g, b.b));
        assert_eq!(b.a, 1.0);
    }

    #[test]
    fn translucency_is_clamped_on_the_theme_too() {
        assert_eq!(theme_with(255).translucency, TRANSLUCENCY_MAX);
    }
}
