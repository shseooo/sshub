//! 탭 스트립 (DESIGN-terminal.md §5).
//!
//! 표시 이름은 `tab_title`(= `tab.name ?? 첫 leaf.label`) 규칙을 따른다.
//! 드래그 의미는 소스에 따라 다르다:
//! - 탭 → 탭바: 순서 변경 (`reorder_tabs`)
//! - 탭 → pane: 탭 병합 (`merge_tab`)
//! - pane → 탭바: 분리해 새 탭 (`detach_pane`)
//! - pane → pane: 이동 (`move_pane`)

use std::collections::HashSet;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    canvas, div, px, AnyElement, App, AppContext as _, Bounds, CursorStyle, Entity, InteractiveElement, IntoElement,
    ClickEvent, MouseButton, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use sshub_splits::{tab_title, TabId, TerminalTab};

use crate::split_view::{element_id, DragGhost, GeometryRef, PaneDrag, TabDrag};
use crate::theme::Theme;
use crate::ui::TextInput;

pub const TAB_BAR_HEIGHT: f32 = 30.0;

pub type TabCallback = Box<dyn Fn(TabId, &mut Window, &mut App)>;
pub type BarCallback = Box<dyn Fn(&mut Window, &mut App)>;
pub type TabDropCallback = Box<dyn Fn(TabDrag, &mut Window, &mut App)>;
pub type PaneDropCallback = Box<dyn Fn(PaneDrag, &mut Window, &mut App)>;

pub struct TabBarHandlers {
    pub select: TabCallback,
    pub close: TabCallback,
    /// 더블클릭 — 인라인 이름 변경 시작.
    pub rename: TabCallback,
    pub new_tab: BarCallback,
    /// 드롭 지점은 워크스페이스가 현재 마우스 위치에서 삽입 경계로 환산한다.
    pub drop_tab: TabDropCallback,
    pub drop_pane: PaneDropCallback,
}

pub struct TabBarCtx<'a> {
    pub tabs: &'a [TerminalTab],
    pub active: Option<&'a TabId>,
    pub broadcast: &'a HashSet<TabId>,
    /// 이름을 편집 중인 탭과 그 입력 위젯.
    pub renaming: Option<(&'a TabId, &'a Entity<TextInput>)>,
    pub geometry: GeometryRef,
    pub handlers: Rc<TabBarHandlers>,
    pub theme: &'a Theme,
}

/// 드롭 x좌표 → 삽입 경계(0..=len). 탭의 왼쪽 절반이면 그 앞, 오른쪽 절반이면
/// 그 뒤. `reorder_tabs`/`insert_at_index`가 기대하는 **원본 배열 기준** 경계다.
pub fn drop_boundary(tabs: &[(TabId, Bounds<Pixels>)], x: Pixels) -> usize {
    for (index, (_, bounds)) in tabs.iter().enumerate() {
        if f32::from(x) < f32::from(bounds.center().x) {
            return index;
        }
    }
    tabs.len()
}

pub fn render_tab_bar(ctx: &TabBarCtx<'_>) -> AnyElement {
    let theme = ctx.theme;
    let mut strip = div()
        .id("tab-bar")
        .flex()
        .flex_row()
        .items_center()
        .h(px(TAB_BAR_HEIGHT))
        .w_full()
        .flex_shrink_0()
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .overflow_hidden();

    for tab in ctx.tabs {
        strip = strip.child(render_tab(tab, ctx));
    }

    let new_tab = ctx.handlers.clone();
    strip = strip.child(
        div()
            .id("tab-bar-new")
            .flex_shrink_0()
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.text_muted)
            .cursor(CursorStyle::PointingHand)
            .hover(|s| s.text_color(theme.text))
            .child("+")
            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                (new_tab.new_tab)(window, cx);
            }),
    );

    // 탭 사이 빈 공간도 드롭 대상 — 끝으로 옮기거나 분리할 때 쓴다.
    let drop_tab = ctx.handlers.clone();
    let drop_pane = ctx.handlers.clone();
    strip
        .child(div().flex_grow().h_full())
        .on_drop::<TabDrag>(move |drag, window, cx| {
            (drop_tab.drop_tab)(drag.clone(), window, cx);
        })
        .on_drop::<PaneDrag>(move |drag, window, cx| {
            (drop_pane.drop_pane)(drag.clone(), window, cx);
        })
        .into_any_element()
}

fn render_tab(tab: &TerminalTab, ctx: &TabBarCtx<'_>) -> AnyElement {
    let theme = ctx.theme;
    let active = ctx.active == Some(&tab.id);
    let broadcasting = ctx.broadcast.contains(&tab.id);
    let title = SharedString::from(tab_title(tab).to_string());

    let geometry = ctx.geometry.clone();
    let record_id = tab.id.clone();
    let record = canvas(
        move |bounds, _window, _cx| {
            let mut geometry = geometry.borrow_mut();
            geometry.tabs.retain(|(id, _)| *id != record_id);
            geometry.tabs.push((record_id.clone(), bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    let renaming = ctx
        .renaming
        .filter(|(id, _)| *id == &tab.id)
        .map(|(_, input)| input.clone());

    let label: AnyElement = match renaming {
        Some(input) => div().w(px(120.0)).child(input).into_any_element(),
        None => div()
            .max_w(px(180.0))
            .overflow_hidden()
            .text_sm()
            .text_color(if active { theme.text } else { theme.text_muted })
            .child(title.clone())
            .into_any_element(),
    };

    let handlers = ctx.handlers.clone();
    let select_id = tab.id.clone();
    let rename_id = tab.id.clone();
    let close_handlers = ctx.handlers.clone();
    let close_id = tab.id.clone();
    let drop_tab = ctx.handlers.clone();
    let drop_pane = ctx.handlers.clone();
    let ghost = title.clone();

    let mut root = div()
        .id(element_id("tab", tab.id.as_str()))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .px_2()
        .h_full()
        .border_r_1()
        .border_color(theme.border)
        .cursor(CursorStyle::PointingHand)
        .when(active, |el| el.bg(theme.selected))
        .child(record)
        .child(label)
        // 선택은 **클릭**(버튼 뗌) 시점에 한다. 누르자마자 활성화하면 탭을 끌어
        // 다른 탭의 pane에 떨어뜨릴 때 이미 그 탭이 활성이라 "자기 자신에게
        // 병합"이 되어 아무 일도 일어나지 않는다. gpui는 드래그가 시작되면
        // pending mouse-down을 가져가므로 드래그 중에는 클릭이 발생하지 않는다.
        .on_click(move |event: &ClickEvent, window, cx| {
            let double = matches!(event, ClickEvent::Mouse(m) if m.down.click_count >= 2);
            if double {
                (handlers.rename)(rename_id.clone(), window, cx);
            } else {
                (handlers.select)(select_id.clone(), window, cx);
            }
        })
        .on_drag(
            TabDrag {
                tab_id: tab.id.clone(),
            },
            move |_payload, _offset, _window, cx| {
                let label = ghost.clone();
                cx.new(|_| DragGhost { label })
            },
        )
        .on_drop::<TabDrag>(move |drag, window, cx| {
            (drop_tab.drop_tab)(drag.clone(), window, cx);
        })
        .on_drop::<PaneDrag>(move |drag, window, cx| {
            (drop_pane.drop_pane)(drag.clone(), window, cx);
        });

    if broadcasting {
        // 브로드캐스트 배지 (§6) — 문자열을 늘리지 않기 위해 글리프만 쓴다.
        root = root.child(
            div()
                .text_xs()
                .text_color(theme.accent)
                .child("⇉"),
        );
    }

    root.child(
        div()
            .id(element_id("tab-close", tab.id.as_str()))
            .px_1()
            .text_xs()
            .text_color(theme.text_disabled)
            .hover(|s| s.text_color(theme.danger))
            .child("×")
            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                // 닫기 클릭이 탭 선택까지 일으키면 방금 닫은 탭이 잠깐 활성화된다.
                cx.stop_propagation();
                (close_handlers.close)(close_id.clone(), window, cx);
            }),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn tab_bounds(ids: &[&str]) -> Vec<(TabId, Bounds<Pixels>)> {
        ids.iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    TabId::new(*id),
                    Bounds::new(
                        point(px(i as f32 * 100.0), px(0.0)),
                        size(px(100.0), px(30.0)),
                    ),
                )
            })
            .collect()
    }

    #[test]
    fn drop_boundary_splits_each_tab_at_its_midpoint() {
        let tabs = tab_bounds(&["a", "b", "c"]);
        assert_eq!(drop_boundary(&tabs, px(10.0)), 0, "첫 탭 왼쪽 절반 → 맨 앞");
        assert_eq!(drop_boundary(&tabs, px(60.0)), 1, "첫 탭 오른쪽 절반 → a 뒤");
        assert_eq!(drop_boundary(&tabs, px(140.0)), 1, "두 번째 탭 왼쪽 절반");
        assert_eq!(drop_boundary(&tabs, px(160.0)), 2);
        assert_eq!(drop_boundary(&tabs, px(999.0)), 3, "빈 공간 → 맨 뒤");
        assert_eq!(drop_boundary(&[], px(10.0)), 0);
    }
}
