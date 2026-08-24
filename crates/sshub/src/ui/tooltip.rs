//! 잘린 라벨의 전체 내용을 보여주는 툴팁.
//!
//! gpui가 500ms 호버 지연을 이미 넣어 주므로(마우스가 스쳐 지나갈 때는 뜨지
//! 않는다) 여기서는 모양만 담당한다.

use gpui::{div, px, AnyView, App, AppContext as _, IntoElement, ParentElement, Render, SharedString, Styled, Window};

use crate::theme::theme;

pub struct TextTooltip {
    text: SharedString,
}

impl TextTooltip {
    /// `element.tooltip(...)`에 그대로 넘길 수 있는 형태로 만든다.
    pub fn view(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
        let text = text.into();
        cx.new(|_| TextTooltip { text }).into()
    }
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .px_2()
            .py_1()
            // 화면을 가로지르는 한 줄이 되지 않도록 상한만 둔다 — 넘치면 접힌다.
            .max_w(px(420.0))
            .rounded_sm()
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .text_xs()
            .text_color(t.text)
            .child(self.text.clone())
    }
}
