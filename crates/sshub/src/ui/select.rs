//! Select — 트리거 + 드롭다운 메뉴 (React portal 등가 = `deferred(anchored(..))`).
//!
//! 키 바인딩은 `ui::init()`에서 context `"Select"`로 등록한다.
use crate::theme::theme;
use crate::ui::icon::Icon;
use gpui::{
    actions, anchored, deferred, div, point, prelude::*, px, App, Context, ElementId, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, SharedString, Window,
};

actions!(
    sshub_select,
    [
        SelectUp,
        SelectDown,
        SelectConfirm,
        SelectCancel,
        SelectFirst,
        SelectLast,
    ]
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectEvent {
    Changed(usize),
}

// --- 인덱스 산술 (순수 · 테스트 대상) --------------------------------------

/// 아래 방향 이동. 목록이 비면 `None`, 끝에서는 처음으로 순환.
pub fn next_ix(current: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        None => 0,
        Some(ix) if ix + 1 >= len => 0,
        Some(ix) => ix + 1,
    })
}

/// 위 방향 이동. 처음에서는 끝으로 순환.
pub fn prev_ix(current: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        None => len - 1,
        Some(0) => len - 1,
        Some(ix) => ix - 1,
    })
}

/// 옵션 목록이 바뀐 뒤 선택 인덱스를 유효 범위로 보정.
pub fn clamp_ix(current: Option<usize>, len: usize) -> Option<usize> {
    match current {
        Some(ix) if ix < len => Some(ix),
        _ if len == 0 => None,
        _ => None,
    }
}

// ---------------------------------------------------------------------------

pub struct Select {
    id: ElementId,
    focus_handle: FocusHandle,
    options: Vec<SelectOption>,
    selected_ix: Option<usize>,
    /// 키보드 하이라이트 (드롭다운 열려 있을 때만 의미 있음)
    active_ix: Option<usize>,
    open: bool,
    placeholder: SharedString,
    disabled: bool,
}

impl Select {
    pub fn new(id: impl Into<ElementId>, options: Vec<SelectOption>, cx: &mut Context<Self>) -> Self {
        Self {
            id: id.into(),
            focus_handle: cx.focus_handle(),
            options,
            selected_ix: None,
            active_ix: None,
            open: false,
            placeholder: SharedString::new_static("—"),
            disabled: false,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_selected_ix(mut self, ix: Option<usize>) -> Self {
        self.selected_ix = clamp_ix(ix, self.options.len());
        self
    }

    /// value가 일치하는 옵션을 선택 (없으면 선택 해제).
    pub fn with_selected_value(mut self, value: &str) -> Self {
        self.selected_ix = self.options.iter().position(|o| o.value == value);
        self
    }

    pub fn options(&self) -> &[SelectOption] {
        &self.options
    }

    pub fn selected_ix(&self) -> Option<usize> {
        self.selected_ix
    }

    pub fn selected(&self) -> Option<&SelectOption> {
        self.selected_ix.and_then(|ix| self.options.get(ix))
    }

    pub fn selected_value(&self) -> Option<&SharedString> {
        self.selected().map(|o| &o.value)
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        if disabled {
            self.open = false;
        }
        cx.notify();
    }

    pub fn set_options(&mut self, options: Vec<SelectOption>, cx: &mut Context<Self>) {
        self.selected_ix = clamp_ix(self.selected_ix, options.len());
        self.options = options;
        self.active_ix = self.selected_ix;
        cx.notify();
    }

    /// 프로그램적 선택 — 이벤트를 발생시키지 않는다.
    pub fn set_selected_ix(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        self.selected_ix = clamp_ix(ix, self.options.len());
        cx.notify();
    }

    fn commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.options.len() {
            return;
        }
        let changed = self.selected_ix != Some(ix);
        self.selected_ix = Some(ix);
        self.active_ix = Some(ix);
        self.open = false;
        if changed {
            cx.emit(SelectEvent::Changed(ix));
        }
        cx.notify();
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.open = !self.open;
        if self.open {
            self.active_ix = self.selected_ix.or(Some(0));
            window.focus(&self.focus_handle);
        }
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    // -- 액션 ---------------------------------------------------------------

    fn on_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.active_ix = self.selected_ix.or(Some(0));
        } else {
            self.active_ix = prev_ix(self.active_ix, self.options.len());
        }
        cx.notify();
    }

    fn on_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.active_ix = self.selected_ix.or(Some(0));
        } else {
            self.active_ix = next_ix(self.active_ix, self.options.len());
        }
        cx.notify();
    }

    fn on_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        if !self.options.is_empty() {
            self.active_ix = Some(0);
            cx.notify();
        }
    }

    fn on_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        if !self.options.is_empty() {
            self.active_ix = Some(self.options.len() - 1);
            cx.notify();
        }
    }

    fn on_confirm(&mut self, _: &SelectConfirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            if let Some(ix) = self.active_ix {
                self.commit(ix, cx);
            }
        } else {
            self.toggle(window, cx);
        }
    }

    fn on_cancel(&mut self, _: &SelectCancel, _: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
    }
}

impl EventEmitter<SelectEvent> for Select {}

impl Focusable for Select {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let focused = self.focus_handle.is_focused(window);
        let label = self
            .selected()
            .map(|o| o.label.clone())
            .unwrap_or_else(|| self.placeholder.clone());
        let label_color = if self.disabled {
            t.text_disabled
        } else if self.selected().is_some() {
            t.text
        } else {
            t.text_muted
        };

        let trigger = div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(px(30.))
            .px(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(if focused && !self.disabled {
                t.accent
            } else {
                t.border
            })
            .bg(if self.disabled { t.surface } else { t.elevated })
            .text_size(px(13.))
            .text_color(label_color)
            .child(div().flex_1().min_w(px(0.)).child(label))
            .child(
                div()
                    .flex_none()
                    .text_color(t.text_muted)
                    .child(Icon::ChevronDown.glyph()),
            )
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            });

        let menu = self.open.then(|| {
            let items: Vec<_> = self
                .options
                .iter()
                .enumerate()
                .map(|(ix, option)| {
                    let active = self.active_ix == Some(ix);
                    let selected = self.selected_ix == Some(ix);
                    div()
                        .id(("select-option", ix))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .w_full()
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(13.))
                        .text_color(t.text)
                        .when(active, |el| el.bg(t.hover))
                        .hover(move |s| s.bg(t.hover))
                        .child(div().flex_1().min_w(px(0.)).child(option.label.clone()))
                        .when(selected, |el| {
                            el.child(
                                div()
                                    .flex_none()
                                    .text_color(t.accent)
                                    .child(Icon::Check.glyph()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _window, cx| this.commit(ix, cx)))
                        .into_any_element()
                })
                .collect();

            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.))
                    // 트리거(30px) 아래로 내려 붙인다.
                    .offset(point(px(0.), px(34.)))
                    .child(
                        div()
                            // overflow_y_scroll은 Stateful<Div>에만 있다 (id 필요).
                            .id("select-menu")
                            .occlude()
                            .min_w(px(160.))
                            .max_h(px(280.))
                            .overflow_y_scroll()
                            .p(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.elevated)
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(items),
                    ),
            )
            .with_priority(1)
        });

        div()
            .key_context("Select")
            .track_focus(&self.focus_handle)
            .relative()
            .w_full()
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_first))
            .on_action(cx.listener(Self::on_last))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_cancel))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _window, cx| this.close(cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _window, cx| this.close(cx)),
            )
            .child(trigger)
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_ix_wraps_and_handles_empty() {
        assert_eq!(next_ix(None, 0), None);
        assert_eq!(next_ix(Some(3), 0), None);
        assert_eq!(next_ix(None, 3), Some(0));
        assert_eq!(next_ix(Some(0), 3), Some(1));
        assert_eq!(next_ix(Some(2), 3), Some(0));
        // 범위를 벗어난 인덱스도 첫 항목으로 되감는다.
        assert_eq!(next_ix(Some(9), 3), Some(0));
    }

    #[test]
    fn prev_ix_wraps_and_handles_empty() {
        assert_eq!(prev_ix(None, 0), None);
        assert_eq!(prev_ix(None, 3), Some(2));
        assert_eq!(prev_ix(Some(0), 3), Some(2));
        assert_eq!(prev_ix(Some(2), 3), Some(1));
    }

    #[test]
    fn clamp_ix_drops_out_of_range() {
        assert_eq!(clamp_ix(Some(0), 3), Some(0));
        assert_eq!(clamp_ix(Some(2), 3), Some(2));
        assert_eq!(clamp_ix(Some(3), 3), None);
        assert_eq!(clamp_ix(Some(0), 0), None);
        assert_eq!(clamp_ix(None, 3), None);
    }

    #[test]
    fn full_cycle_returns_to_start() {
        let len = 4;
        let mut ix = Some(0);
        for _ in 0..len {
            ix = next_ix(ix, len);
        }
        assert_eq!(ix, Some(0));
        for _ in 0..len {
            ix = prev_ix(ix, len);
        }
        assert_eq!(ix, Some(0));
    }
}
