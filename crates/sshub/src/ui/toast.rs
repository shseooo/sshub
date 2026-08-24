//! Toast — 우하단 스택. 타이머/스택 소유는 Workspace가 하고,
//! 여기서는 데이터 모델 + 순수 렌더 헬퍼만 제공한다.
use crate::theme::theme;
use crate::ui::icon::Icon;
use gpui::{div, prelude::*, px, App, IntoElement, SharedString, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Error,
}

impl ToastKind {
    pub fn icon(self) -> Icon {
        match self {
            ToastKind::Info => Icon::Info,
            ToastKind::Success => Icon::Success,
            ToastKind::Error => Icon::Error,
        }
    }

    /// 기본 자동 dismiss 시간(ms). Error는 더 오래 남긴다.
    pub fn default_duration_ms(self) -> u64 {
        match self {
            ToastKind::Error => 6_000,
            _ => 3_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: SharedString,
}

impl Toast {
    pub fn new(id: u64, kind: ToastKind, message: impl Into<SharedString>) -> Self {
        Self {
            id,
            kind,
            message: message.into(),
        }
    }

    pub fn info(id: u64, message: impl Into<SharedString>) -> Self {
        Self::new(id, ToastKind::Info, message)
    }

    pub fn success(id: u64, message: impl Into<SharedString>) -> Self {
        Self::new(id, ToastKind::Success, message)
    }

    pub fn error(id: u64, message: impl Into<SharedString>) -> Self {
        Self::new(id, ToastKind::Error, message)
    }
}

/// 토스트 1개.
pub fn render_toast(toast: &Toast, _window: &mut Window, cx: &App) -> impl IntoElement {
    let t = theme(cx);
    let accent = match toast.kind {
        ToastKind::Info => t.accent,
        ToastKind::Success => t.success,
        ToastKind::Error => t.danger,
    };

    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(8.))
        .min_w(px(220.))
        .max_w(px(360.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(8.))
        .border_1()
        .border_color(t.border)
        .bg(t.elevated)
        .child(
            div()
                .flex_none()
                .text_size(px(13.))
                .text_color(accent)
                .child(toast.kind.icon().glyph()),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(13.))
                .text_color(t.text)
                .child(toast.message.clone()),
        )
}

/// 토스트 스택 (우하단 고정). 비어 있으면 빈 컨테이너.
pub fn render_toast_stack(
    toasts: &[Toast],
    window: &mut Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let children: Vec<_> = toasts
        .iter()
        .map(|toast| render_toast(toast, window, cx).into_any_element())
        .collect();

    div()
        .absolute()
        .bottom(px(16.))
        .right(px(16.))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.))
        .children(children)
}
