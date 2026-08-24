//! FormField — 라벨 + 컨트롤 + (선택) 에러/힌트 텍스트.
use crate::theme::theme;
use gpui::{
    div, prelude::*, px, AnyElement, App, IntoElement, RenderOnce, SharedString, Window,
};

#[derive(IntoElement)]
pub struct FormField {
    label: Option<SharedString>,
    control: AnyElement,
    error: Option<SharedString>,
    hint: Option<SharedString>,
    required: bool,
}

impl FormField {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: Some(label.into()),
            control: control.into_any_element(),
            error: None,
            hint: None,
            required: false,
        }
    }

    /// 라벨 없는 필드 (체크박스 행 등).
    pub fn bare(control: impl IntoElement) -> Self {
        Self {
            label: None,
            control: control.into_any_element(),
            error: None,
            hint: None,
            required: false,
        }
    }

    /// `None`이면 에러 줄이 렌더되지 않는다 (검증 결과를 그대로 넘기기 쉽게).
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl RenderOnce for FormField {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx).clone();
        let has_error = self.error.is_some();

        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .w_full()
            .when_some(self.label, |el, label| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(3.))
                        .text_size(px(12.))
                        .text_color(t.text_muted)
                        .child(label)
                        .when(self.required, |el| {
                            el.child(div().text_color(t.danger).child("*"))
                        }),
                )
            })
            .child(self.control)
            .when_some(self.error, |el, error| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.danger)
                        .child(error),
                )
            })
            .when_some(self.hint.filter(|_| !has_error), |el, hint| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text_disabled)
                        .child(hint),
                )
            })
    }
}
