//! 우클릭 컨텍스트 메뉴 — 포인터 위치에 뜨는 팝업 (터미널 pane·탭바 공용).
//!
//! z-order와 창 밖 넘침은 `deferred(anchored(..))`가 해결한다(Select와 같은 방식).
//! `deferred`는 형제 트리를 전부 그린 **뒤에** 그리므로 터미널 위로 올라오고,
//! 마우스 이벤트도 bubble 단계에서 역순으로 먼저 받는다. `anchored`의
//! `snap_to_window_with_margin`은 창 가장자리에서 메뉴가 잘리지 않게 당겨준다.
//!
//! 키 바인딩은 `ui::init()`에서 context `"ContextMenu"`로 등록한다 —
//! `clear_key_bindings()` 후에도 살아남으려면 그 한 곳에만 있어야 한다.
use crate::theme::theme;
use gpui::{
    actions, anchored, deferred, div, prelude::*, px, App, Context, DismissEvent, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseDownEvent, Pixels, Point, SharedString, Window,
};

actions!(
    sshub_context_menu,
    [
        ContextMenuUp,
        ContextMenuDown,
        ContextMenuConfirm,
        ContextMenuCancel,
    ]
);

/// 항목을 고르면 실행되는 동작. 실행 후 메뉴는 스스로 닫힌다.
pub type MenuHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;

pub enum ContextMenuItem {
    Separator,
    Entry {
        label: SharedString,
        /// 단축키 힌트(⌘D 등). 대응하는 단축키가 없으면 `None`.
        hint: Option<SharedString>,
        /// 눌러도 아무 일이 없는 항목은 지운 대신 흐리게 남긴다 — 위치가
        /// 고정돼야 근육 기억이 깨지지 않는다.
        disabled: bool,
        handler: MenuHandler,
    },
}

impl ContextMenuItem {
    pub fn entry(
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Entry {
            label: label.into(),
            hint: None,
            disabled: false,
            handler: Box::new(handler),
        }
    }

    pub fn separator() -> Self {
        Self::Separator
    }

    pub fn hint(mut self, value: Option<SharedString>) -> Self {
        if let Self::Entry { hint, .. } = &mut self {
            *hint = value;
        }
        self
    }

    pub fn disabled(mut self, value: bool) -> Self {
        if let Self::Entry { disabled, .. } = &mut self {
            *disabled = value;
        }
        self
    }

    /// 키보드 하이라이트가 머무를 수 있는 항목인가.
    fn selectable(&self) -> bool {
        matches!(self, Self::Entry { disabled: false, .. })
    }
}

// --- 하이라이트 인덱스 산술 (순수 · 테스트 대상) ----------------------------

/// 하이라이트 이동 — 구분선/비활성 항목은 건너뛰고 양끝에서 순환한다.
/// `selectable[i]`가 true인 인덱스만 후보다. 고를 수 있는 항목이 없으면 `None`.
pub fn step_highlight(selectable: &[bool], current: Option<usize>, forward: bool) -> Option<usize> {
    let len = selectable.len();
    if len == 0 || !selectable.iter().any(|s| *s) {
        return None;
    }
    // 아직 아무것도 고르지 않았으면 아래로는 첫 항목, 위로는 마지막 항목부터
    // 보이도록 가상의 시작점을 반대쪽 끝에 둔다.
    let start = match current {
        Some(ix) if ix < len => ix,
        _ if forward => len - 1,
        _ => 0,
    };
    (1..=len)
        .map(|step| {
            if forward {
                (start + step) % len
            } else {
                (start + len - (step % len)) % len
            }
        })
        .find(|ix| selectable[*ix])
}

// ---------------------------------------------------------------------------

pub struct ContextMenu {
    focus_handle: FocusHandle,
    items: Vec<ContextMenuItem>,
    /// 창 좌표(= `MouseDownEvent::position`)의 팝업 좌상단.
    position: Point<Pixels>,
    highlight: Option<usize>,
}

impl ContextMenu {
    pub fn new(
        position: Point<Pixels>,
        items: Vec<ContextMenuItem>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            items,
            position,
            highlight: None,
        }
    }

    /// 화면에 붙인 직후 호출 — 키 액션이 이 메뉴로 오게 한다.
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    fn selectable_flags(&self) -> Vec<bool> {
        self.items.iter().map(ContextMenuItem::selectable).collect()
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ContextMenuItem::Entry {
            disabled, handler, ..
        }) = self.items.get(ix)
        else {
            return;
        };
        if *disabled {
            return;
        }
        handler(window, cx);
        cx.emit(DismissEvent);
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn on_up(&mut self, _: &ContextMenuUp, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight = step_highlight(&self.selectable_flags(), self.highlight, false);
        cx.notify();
    }

    fn on_down(&mut self, _: &ContextMenuDown, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight = step_highlight(&self.selectable_flags(), self.highlight, true);
        cx.notify();
    }

    fn on_confirm(&mut self, _: &ContextMenuConfirm, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlight {
            self.activate(ix, window, cx);
        }
    }

    fn on_cancel(&mut self, _: &ContextMenuCancel, _: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(cx);
    }
}

impl EventEmitter<DismissEvent> for ContextMenu {}

impl Focusable for ContextMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ContextMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();

        let rows: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| match item {
                ContextMenuItem::Separator => div()
                    .my(px(3.))
                    .h(px(1.))
                    .w_full()
                    .bg(t.border_subtle)
                    .into_any_element(),
                ContextMenuItem::Entry {
                    label,
                    hint,
                    disabled,
                    ..
                } => {
                    let active = self.highlight == Some(ix) && !*disabled;
                    div()
                        .id(("context-menu-item", ix))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(16.))
                        .w_full()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .text_color(if *disabled { t.text_disabled } else { t.text })
                        .when(active, |el| el.bg(t.hover))
                        .child(div().flex_1().min_w(px(0.)).child(label.clone()))
                        .children(hint.clone().map(|hint| {
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(t.text_disabled)
                                .child(hint)
                        }))
                        .when(!*disabled, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.bg(t.hover))
                                // 하이라이트를 마우스에 맞춰 둬야 키보드로
                                // 이어서 움직일 때 엉뚱한 데서 출발하지 않는다.
                                .on_mouse_move(cx.listener(move |this, _, _window, cx| {
                                    if this.highlight != Some(ix) {
                                        this.highlight = Some(ix);
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.activate(ix, window, cx)
                                }))
                        })
                        .into_any_element()
                }
            })
            .collect();

        // 포커스/키 컨텍스트를 팝업 자신에 둔다 — `on_mouse_down_out`은 이
        // 요소의 사각형만 보므로, 감싸는 래퍼에 달면 메뉴 안 클릭까지
        // "바깥"으로 취급돼 항목이 눌리기 전에 닫혀 버린다.
        let popup = div()
            .key_context("ContextMenu")
            .track_focus(&self.focus_handle)
            .id("context-menu")
            .occlude()
            .min_w(px(180.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_cancel))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                this.dismiss(cx)
            }))
            .children(rows);

        div().child(
            deferred(
                anchored()
                    .position(self.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(popup),
            )
            // Select 드롭다운(1)보다 위 — 컨텍스트 메뉴는 항상 최상단이다.
            .with_priority(2),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_highlight_wraps_around() {
        let all = [true, true, true];
        assert_eq!(step_highlight(&all, None, true), Some(0));
        assert_eq!(step_highlight(&all, Some(0), true), Some(1));
        assert_eq!(step_highlight(&all, Some(2), true), Some(0));
        assert_eq!(step_highlight(&all, None, false), Some(2));
        assert_eq!(step_highlight(&all, Some(0), false), Some(2));
        assert_eq!(step_highlight(&all, Some(2), false), Some(1));
    }

    #[test]
    fn step_highlight_skips_separators_and_disabled() {
        // [항목, 구분선, 비활성, 항목]
        let flags = [true, false, false, true];
        assert_eq!(step_highlight(&flags, Some(0), true), Some(3));
        assert_eq!(step_highlight(&flags, Some(3), true), Some(0));
        assert_eq!(step_highlight(&flags, Some(0), false), Some(3));
        assert_eq!(step_highlight(&flags, None, true), Some(0));
        assert_eq!(step_highlight(&flags, None, false), Some(3));
    }

    #[test]
    fn step_highlight_gives_up_when_nothing_is_selectable() {
        assert_eq!(step_highlight(&[], None, true), None);
        assert_eq!(step_highlight(&[false, false], Some(0), true), None);
        assert_eq!(step_highlight(&[false, false], None, false), None);
    }

    #[test]
    fn step_highlight_stays_put_with_a_single_candidate() {
        let flags = [false, true, false];
        assert_eq!(step_highlight(&flags, Some(1), true), Some(1));
        assert_eq!(step_highlight(&flags, Some(1), false), Some(1));
    }

    #[test]
    fn out_of_range_highlight_restarts_from_the_edge() {
        // 항목 수가 줄어든 뒤에도 하이라이트가 살아 있을 수 있다.
        let flags = [true, true];
        assert_eq!(step_highlight(&flags, Some(9), true), Some(0));
        assert_eq!(step_highlight(&flags, Some(9), false), Some(1));
    }

    #[test]
    fn separators_are_never_selectable() {
        assert!(!ContextMenuItem::separator().selectable());
        assert!(ContextMenuItem::entry("a", |_, _| {}).selectable());
        assert!(!ContextMenuItem::entry("a", |_, _| {}).disabled(true).selectable());
    }
}
