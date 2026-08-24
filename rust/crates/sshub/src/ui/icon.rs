//! 최소 아이콘 세트.
//!
//! NOTE(gpui 0.2.2): `gpui::svg()`는 경로를 `AssetSource`로 해석하므로
//! `Application::new().with_assets(..)` 로 에셋 소스를 등록해야만 동작한다.
//! 위젯 킷은 앱 부트스트랩에 의존하지 않아야 하므로 v1은 유니코드 글리프를 쓴다.
//! assets.rs가 생기면 `Icon::path()`를 추가해 svg로 승격한다.
use crate::theme::theme;
use gpui::{div, prelude::*, px, App, Hsla, IntoElement, RenderOnce, SharedString, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Check,
    ChevronDown,
    ChevronRight,
    Close,
    Plus,
    Trash,
    Pencil,
    Search,
    Info,
    Success,
    Error,
    Warning,
    Copy,
    Eye,
    EyeOff,
    Server,
    Key,
    Terminal,
    Settings,
    Spinner,
}

impl Icon {
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Check => "✓",
            Icon::ChevronDown => "⌄",
            Icon::ChevronRight => "›",
            Icon::Close => "✕",
            Icon::Plus => "+",
            Icon::Trash => "🗑",
            Icon::Pencil => "✎",
            Icon::Search => "⌕",
            Icon::Info => "ⓘ",
            Icon::Success => "✓",
            Icon::Error => "⚠",
            Icon::Warning => "⚠",
            Icon::Copy => "⧉",
            Icon::Eye => "👁",
            Icon::EyeOff => "⦸",
            Icon::Server => "▤",
            Icon::Key => "⚿",
            Icon::Terminal => "❯",
            Icon::Settings => "⚙",
            Icon::Spinner => "◐",
        }
    }
}

impl From<Icon> for SharedString {
    fn from(icon: Icon) -> Self {
        SharedString::new_static(icon.glyph())
    }
}

/// 색을 지정하지 않으면 `text_muted`.
#[derive(IntoElement)]
pub struct IconEl {
    icon: Icon,
    size: gpui::Pixels,
    color: Option<Hsla>,
}

pub fn icon(icon: Icon) -> IconEl {
    IconEl {
        icon,
        size: px(13.),
        color: None,
    }
}

impl IconEl {
    pub fn size(mut self, size: gpui::Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for IconEl {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| theme(cx).text_muted);
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .text_size(self.size)
            .text_color(color)
            .child(self.icon.glyph())
    }
}
