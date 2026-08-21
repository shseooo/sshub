//! ConfirmDialog + ModalOverlay.
//!
//! Workspace가 `Option<ActiveModal { view: AnyView, prev_focus }>`를 들고,
//! 렌더 트리 마지막에 `ModalOverlay`로 감싼다(§2). 여기서는 오버레이 렌더 헬퍼와
//! 확인 다이얼로그 엔티티만 제공한다.
use crate::theme::{theme, with_alpha};
use crate::ui::button::{Button, ButtonVariant};
use gpui::{
    actions, black, div, prelude::*, px, AnyView, App, Context, DismissEvent, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseDownEvent, RenderOnce, SharedString, Window,
};

actions!(sshub_confirm_dialog, [ConfirmDialogConfirm, ConfirmDialogCancel]);

type ResultHandler = Box<dyn FnOnce(bool, &mut Window, &mut App) + 'static>;

pub struct ConfirmDialog {
    focus_handle: FocusHandle,
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    cancel_label: SharedString,
    danger: bool,
    on_result: Option<ResultHandler>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        cancel_label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            danger: false,
            on_result: None,
        }
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// 확인/취소 어느 쪽으로 닫히든 **정확히 한 번** 호출된다.
    pub fn on_result(mut self, handler: impl FnOnce(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_result = Some(Box::new(handler));
        self
    }

    /// 다이얼로그가 화면에 붙은 뒤 호출 — 키 액션이 여기로 오게 한다.
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    fn finish(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handler) = self.on_result.take() {
            handler(confirmed, window, cx);
        }
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &ConfirmDialogConfirm, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(true, window, cx);
    }

    fn cancel(&mut self, _: &ConfirmDialogCancel, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(false, window, cx);
    }
}

impl EventEmitter<DismissEvent> for ConfirmDialog {}

impl Focusable for ConfirmDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfirmDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let confirm_variant = if self.danger {
            ButtonVariant::Danger
        } else {
            ButtonVariant::Primary
        };

        div()
            .key_context("ConfirmDialog")
            .track_focus(&self.focus_handle)
            .occlude()
            // 오버레이 클릭이 다이얼로그를 통과해 닫지 않도록 삼킨다.
            .on_mouse_down(gpui::MouseButton::Left, |_: &MouseDownEvent, _, _| {})
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .flex()
            .flex_col()
            .gap(px(14.))
            .w(px(400.))
            .p(px(20.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(t.text)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(t.text_muted)
                    .child(self.message.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("confirm-dialog-cancel", self.cancel_label.clone())
                            .variant(ButtonVariant::Secondary)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.finish(false, window, cx);
                            })),
                    )
                    .child(
                        Button::new("confirm-dialog-confirm", self.confirm_label.clone())
                            .variant(confirm_variant)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.finish(true, window, cx);
                            })),
                    ),
            )
    }
}

/// 임의의 뷰를 화면 중앙에 띄우는 오클루딩 오버레이.
/// 렌더 트리의 **마지막 자식**으로 넣어야 형제 위에 그려진다.
#[derive(IntoElement)]
pub struct ModalOverlay {
    view: AnyView,
    on_backdrop_click: Option<Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>>,
}

impl ModalOverlay {
    pub fn new(view: impl Into<AnyView>) -> Self {
        Self {
            view: view.into(),
            on_backdrop_click: None,
        }
    }

    /// 배경 클릭 시 호출 (보통 모달 dismiss).
    pub fn on_backdrop_click(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_backdrop_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ModalOverlay {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let _ = theme(cx);
        let backdrop = with_alpha(black(), 0.45);

        let el = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop)
            .child(self.view);

        match self.on_backdrop_click {
            Some(handler) => el.on_mouse_down(
                gpui::MouseButton::Left,
                move |ev, window, cx| handler(ev, window, cx),
            ),
            None => el,
        }
    }
}
