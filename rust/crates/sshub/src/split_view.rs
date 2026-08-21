//! 분할 트리 렌더러 (DESIGN-terminal.md §5).
//!
//! `PaneNode`를 재귀적으로 flex 영역으로 그린다. 크기는 `sizes`(%)를 flex-grow
//! 가중치로 넘겨 **디바이더가 차지한 5px를 뺀 나머지**를 비율대로 나눈다
//! (퍼센트를 그대로 폭으로 주면 디바이더 폭만큼 매번 넘친다).
//!
//! 드래그·포커스 이동에 필요한 pane 사각형은 렌더 중 `canvas`로 수집한다
//! (레이아웃을 두 번 계산하지 않고 실제 페인트 결과를 그대로 쓴다).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    canvas, div, px, AnyElement, App, AppContext as _, Bounds, CursorStyle, Div, Entity, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use sshub_splits::{DropSide, PaneNode, SessionId, SplitDirection, SplitId, TabId, TerminalLeaf};

use crate::terminal_view::TerminalView;
use crate::theme::Theme;

/// 자식 pane의 최소 비율 — 이 아래로는 드래그해도 줄지 않는다.
pub const MIN_PANE_PERCENT: f32 = 5.0;
/// 디바이더 히트박스 두께.
pub const DIVIDER_PX: f32 = 5.0;

/// 프레임마다 갱신되는 화면 기하 — 방향 포커스 이동·드롭 위치 판정·디바이더
/// 드래그의 기준 길이가 전부 여기서 나온다.
#[derive(Default)]
pub struct WorkspaceGeometry {
    pub panes: HashMap<SessionId, Bounds<Pixels>>,
    pub splits: HashMap<SplitId, Bounds<Pixels>>,
    pub tabs: Vec<(TabId, Bounds<Pixels>)>,
}

pub type GeometryRef = Rc<RefCell<WorkspaceGeometry>>;

/// 탭을 끌고 있음 — 탭바에 놓으면 순서 변경, pane에 놓으면 탭 병합.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabDrag {
    pub tab_id: TabId,
}

/// pane을 끌고 있음 — 다른 pane에 놓으면 이동, 탭바에 놓으면 새 탭으로 분리.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneDrag {
    pub tab_id: TabId,
    pub session_id: SessionId,
}

/// 디바이더를 잡은 순간의 스냅샷 — 드래그 중에는 이 값 기준으로만 계산한다
/// (프레임마다 누적하면 반올림 오차가 쌓인다).
#[derive(Clone, Debug, PartialEq)]
pub struct DividerGrab {
    pub split_id: SplitId,
    /// 디바이더 왼쪽/위 자식의 인덱스.
    pub index: usize,
    pub direction: SplitDirection,
    pub sizes: Vec<f32>,
    pub start: Point<Pixels>,
    /// split 컨테이너의 해당 축 길이(px).
    pub extent: f32,
}

/// 방향 포커스 이동.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDir {
    Left,
    Right,
    Up,
    Down,
}

/// 트리 렌더링에 필요한 주변 정보 묶음.
pub struct PaneTreeCtx<'a> {
    pub tab_id: TabId,
    pub views: &'a HashMap<SessionId, Entity<TerminalView>>,
    pub focused: Option<&'a SessionId>,
    /// 이 탭이 브로드캐스트 중인가 — 모든 pane에 어센트 내부 보더를 그린다.
    pub broadcast: bool,
    pub geometry: GeometryRef,
    pub handlers: Rc<PaneHandlers>,
    pub theme: &'a Theme,
    /// 세션이 아직/더는 없을 때 보여줄 안내 (i18n `term.closedNotice`).
    pub missing_notice: SharedString,
}

/// 트리에서 위(워크스페이스)로 올라가는 동작들. 워크스페이스가 약한 핸들로
/// 자기 자신을 갱신하는 클로저를 넣는다.
pub type PaneCallback = Box<dyn Fn(SessionId, &mut Window, &mut App)>;
pub type DividerCallback = Box<dyn Fn(DividerGrab, &mut Window, &mut App)>;
pub type PaneDropCallback = Box<dyn Fn(PaneDrag, SessionId, &mut Window, &mut App)>;
pub type TabDropCallback = Box<dyn Fn(TabDrag, SessionId, &mut Window, &mut App)>;
pub type PaneLabelFn = Box<dyn Fn(&SessionId) -> SharedString>;

pub struct PaneHandlers {
    pub focus: PaneCallback,
    pub divider_down: DividerCallback,
    pub drop_pane: PaneDropCallback,
    pub drop_tab: TabDropCallback,
    pub drag_label: PaneLabelFn,
}

// ---------------------------------------------------------------------------
// 순수 계산 (테스트 대상)
// ---------------------------------------------------------------------------

/// 길이가 어긋나거나 합이 0인 `sizes`를 자식 수에 맞춰 정규화한다.
/// 저장된 레이아웃이 깨져 있어도 렌더가 무너지지 않게 하는 방어선이다.
pub fn normalized_sizes(sizes: &[f32], children: usize) -> Vec<f32> {
    if children == 0 {
        return Vec::new();
    }
    let usable: Vec<f32> = sizes
        .iter()
        .copied()
        .map(|s| if s.is_finite() && s > 0.0 { s } else { 0.0 })
        .collect();
    let total: f32 = usable.iter().take(children).sum();
    if usable.len() != children || total <= 0.0 {
        return vec![100.0 / children as f32; children];
    }
    usable.iter().take(children).map(|s| s / total * 100.0).collect()
}

/// 디바이더 드래그: `index`와 `index+1` 자식 사이에서만 비율을 주고받는다.
/// 픽셀 delta는 split의 축 길이 기준 %로 환산하고, 두 자식 모두
/// [`MIN_PANE_PERCENT`] 이상을 유지하도록 clamp한다. 합은 100으로 보존된다.
pub fn resize_split(sizes: &[f32], index: usize, delta_px: f32, extent_px: f32) -> Vec<f32> {
    let mut next = normalized_sizes(sizes, sizes.len());
    if index + 1 >= next.len() || extent_px <= 0.0 || !delta_px.is_finite() {
        return next;
    }
    let pair = next[index] + next[index + 1];
    if pair < MIN_PANE_PERCENT * 2.0 {
        return next; // 둘 다 최소치를 만족시킬 수 없다 — 손대지 않는다
    }
    let delta_pct = delta_px / extent_px * 100.0;
    let first = (next[index] + delta_pct).clamp(MIN_PANE_PERCENT, pair - MIN_PANE_PERCENT);
    next[index] = first;
    next[index + 1] = pair - first;
    next
}

/// pane 위 드롭 지점 → 삽입 방향. 중심에서 더 멀리 벗어난 축이 이긴다
/// (가로로 긴 pane의 위/아래 가장자리에서도 의도대로 잡히게 정규화한다).
pub fn drop_side(bounds: Bounds<Pixels>, at: Point<Pixels>) -> DropSide {
    let width = f32::from(bounds.size.width).max(1.0);
    let height = f32::from(bounds.size.height).max(1.0);
    let dx = (f32::from(at.x) - f32::from(bounds.center().x)) / width;
    let dy = (f32::from(at.y) - f32::from(bounds.center().y)) / height;
    if dx.abs() >= dy.abs() {
        if dx < 0.0 {
            DropSide::Left
        } else {
            DropSide::Right
        }
    } else if dy < 0.0 {
        DropSide::Top
    } else {
        DropSide::Bottom
    }
}

/// `dir` 방향에서 기하적으로 가장 가까운 pane.
///
/// 규칙: ① 그 방향에 실제로 놓인 pane만 후보 ② 수직(교차) 축이 겹치는 후보를
/// 우선 ③ 진행 축 거리 최소 ④ 그래도 같으면 중심 간 교차축 거리 최소.
/// ②가 없으면 대각선에 있는 pane이 바로 옆 pane을 제치는 일이 생긴다.
pub fn nearest_pane(
    panes: &[(SessionId, Bounds<Pixels>)],
    from: &SessionId,
    dir: FocusDir,
) -> Option<SessionId> {
    let origin = panes.iter().find(|(id, _)| id == from)?.1;
    let (ox0, oy0) = (f32::from(origin.origin.x), f32::from(origin.origin.y));
    let (ox1, oy1) = (
        ox0 + f32::from(origin.size.width),
        oy0 + f32::from(origin.size.height),
    );

    let mut best: Option<(SessionId, (bool, f32, f32))> = None;
    for (id, bounds) in panes {
        if id == from {
            continue;
        }
        let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (x1, y1) = (
            x0 + f32::from(bounds.size.width),
            y0 + f32::from(bounds.size.height),
        );

        // (진행 축 거리, 교차 축 겹침, 교차 축 중심 거리)
        let (axis_gap, overlap, cross) = match dir {
            FocusDir::Left => (ox0 - x1, overlap_of(y0, y1, oy0, oy1), center_gap(y0, y1, oy0, oy1)),
            FocusDir::Right => (x0 - ox1, overlap_of(y0, y1, oy0, oy1), center_gap(y0, y1, oy0, oy1)),
            FocusDir::Up => (oy0 - y1, overlap_of(x0, x1, ox0, ox1), center_gap(x0, x1, ox0, ox1)),
            FocusDir::Down => (y0 - oy1, overlap_of(x0, x1, ox0, ox1), center_gap(x0, x1, ox0, ox1)),
        };
        // 1px 여유: 인접 pane은 보통 디바이더 때문에 정확히 0이 아니다.
        if axis_gap < -1.0 {
            continue;
        }
        let key = (overlap <= 0.0, axis_gap.max(0.0), cross);
        if best.as_ref().is_none_or(|(_, b)| key < *b) {
            best = Some((id.clone(), key));
        }
    }
    best.map(|(id, _)| id)
}

fn overlap_of(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    a1.min(b1) - a0.max(b0)
}

fn center_gap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (((a0 + a1) / 2.0) - ((b0 + b1) / 2.0)).abs()
}

// ---------------------------------------------------------------------------
// 렌더링
// ---------------------------------------------------------------------------

/// 드래그 중 커서를 따라다니는 고스트.
pub struct DragGhost {
    pub label: SharedString,
}

impl gpui::Render for DragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = crate::theme::theme(cx);
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme.elevated)
            .border_1()
            .border_color(theme.accent)
            .text_xs()
            .text_color(theme.text)
            .child(self.label.clone())
    }
}

pub fn render_pane_tree(node: &PaneNode, ctx: &PaneTreeCtx<'_>) -> AnyElement {
    match node {
        PaneNode::Leaf(leaf) => render_leaf(leaf, ctx),
        PaneNode::Split(split) => {
            let sizes = normalized_sizes(&split.sizes, split.children.len());
            let row = split.direction == SplitDirection::Row;
            let mut flex = div().size_full().flex();
            flex = if row { flex.flex_row() } else { flex.flex_col() };

            for (index, child) in split.children.iter().enumerate() {
                if index > 0 {
                    flex = flex.child(render_divider(split.id.clone(), index - 1, split.direction, &sizes, ctx));
                }
                let mut region = div().overflow_hidden();
                // flex-grow 가중치 = 비율. basis 0으로 두어 내용 크기가 끼어들지 않게 한다.
                region.style().flex_grow = Some(sizes[index]);
                region.style().flex_shrink = Some(1.0);
                region.style().flex_basis = Some(px(0.0).into());
                flex = flex.child(region.child(render_pane_tree(child, ctx)));
            }

            let geometry = ctx.geometry.clone();
            let split_id = split.id.clone();
            div()
                .relative()
                .size_full()
                .child(
                    canvas(
                        move |bounds, _window, _cx| {
                            geometry.borrow_mut().splits.insert(split_id, bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(flex)
                .into_any_element()
        }
    }
}

fn render_divider(
    split_id: SplitId,
    index: usize,
    direction: SplitDirection,
    sizes: &[f32],
    ctx: &PaneTreeCtx<'_>,
) -> Div {
    let row = direction == SplitDirection::Row;
    let geometry = ctx.geometry.clone();
    let handlers = ctx.handlers.clone();
    let sizes = sizes.to_vec();
    let grab_split = split_id.clone();

    let mut divider = div()
        .flex_shrink_0()
        .bg(ctx.theme.border_subtle)
        .cursor(if row {
            CursorStyle::ResizeLeftRight
        } else {
            CursorStyle::ResizeUpDown
        })
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                let extent = geometry
                    .borrow()
                    .splits
                    .get(&grab_split)
                    .map(|b| {
                        if row {
                            f32::from(b.size.width)
                        } else {
                            f32::from(b.size.height)
                        }
                    })
                    .unwrap_or(0.0);
                (handlers.divider_down)(
                    DividerGrab {
                        split_id: grab_split.clone(),
                        index,
                        direction,
                        sizes: sizes.clone(),
                        start: event.position,
                        extent,
                    },
                    window,
                    cx,
                );
            },
        );
    if row {
        divider = divider.w(px(DIVIDER_PX)).h_full();
    } else {
        divider = divider.h(px(DIVIDER_PX)).w_full();
    }
    divider
}

fn render_leaf(leaf: &TerminalLeaf, ctx: &PaneTreeCtx<'_>) -> AnyElement {
    let session_id = leaf.session_id.clone();
    let focused = ctx.focused == Some(&session_id);
    let theme = ctx.theme;
    let geometry = ctx.geometry.clone();

    let border = if focused {
        theme.accent
    } else {
        theme.border_subtle
    };

    let record = {
        let geometry = geometry.clone();
        let id = session_id.clone();
        canvas(
            move |bounds, _window, _cx| {
                geometry.borrow_mut().panes.insert(id, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };

    let body: AnyElement = match ctx.views.get(&session_id) {
        Some(view) => div().size_full().child(view.clone()).into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme.text_muted)
            .child(ctx.missing_notice.clone())
            .into_any_element(),
    };

    let handlers = ctx.handlers.clone();
    let focus_id = session_id.clone();
    let drop_pane_handlers = ctx.handlers.clone();
    let drop_tab_handlers = ctx.handlers.clone();
    let pane_target = session_id.clone();
    let tab_target = session_id.clone();
    let grip_label = (ctx.handlers.drag_label)(&session_id);

    let mut container = div()
        .id(element_id("pane", session_id.as_str()))
        .relative()
        .size_full()
        .border_1()
        .border_color(border)
        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
            (handlers.focus)(focus_id.clone(), window, cx);
        })
        .on_drop::<PaneDrag>(move |drag, window, cx| {
            (drop_pane_handlers.drop_pane)(drag.clone(), pane_target.clone(), window, cx);
        })
        .on_drop::<TabDrag>(move |drag, window, cx| {
            (drop_tab_handlers.drop_tab)(drag.clone(), tab_target.clone(), window, cx);
        })
        .child(record)
        .child(body);

    // 브로드캐스트 표시: 내용 위에 2px 어센트 내부 보더 (§6). 리스너가 없는
    // 순수 페인트 레이어라 터미널 마우스 입력을 가리지 않는다.
    if ctx.broadcast {
        container = container.child(
            div()
                .absolute()
                .inset_0()
                .border_2()
                .border_color(theme.accent),
        );
    }

    // pane 드래그 손잡이 — pane 자체를 드래그 소스로 만들면 터미널 선택과
    // 충돌하므로 우상단 그립에서만 시작한다.
    let drag_payload = PaneDrag {
        tab_id: ctx.tab_id.clone(),
        session_id: session_id.clone(),
    };
    container.child(
        div()
            .id(element_id("grip", session_id.as_str()))
            .absolute()
            .top_0()
            .right_0()
            .w(px(14.0))
            .h(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(theme.text_disabled)
            .cursor(CursorStyle::OpenHand)
            .child("⠿")
            .on_drag(drag_payload, move |_payload, _offset, _window, cx| {
                let label = grip_label.clone();
                cx.new(|_| DragGhost { label })
            }),
    )
    .into_any_element()
}

pub(crate) fn element_id(prefix: &str, id: &str) -> gpui::ElementId {
    gpui::ElementId::Name(SharedString::from(format!("{prefix}:{id}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn resize_moves_percentage_between_neighbours_only() {
        let sizes = vec![50.0, 50.0];
        // 폭 1000px에서 100px 끌면 10%가 넘어간다.
        let next = resize_split(&sizes, 0, 100.0, 1000.0);
        assert_eq!(next, vec![60.0, 40.0]);
        assert!((next.iter().sum::<f32>() - 100.0).abs() < 0.001);

        let three = vec![40.0, 30.0, 30.0];
        let next = resize_split(&three, 1, -75.0, 1000.0);
        assert_eq!(next[0], 40.0, "이웃이 아닌 자식은 그대로");
        assert!((next[1] - 22.5).abs() < 0.001);
        assert!((next[2] - 37.5).abs() < 0.001);
        assert!((next.iter().sum::<f32>() - 100.0).abs() < 0.001);
    }

    #[test]
    fn resize_clamps_each_child_to_the_minimum() {
        let sizes = vec![50.0, 50.0];
        // 화면 밖까지 끌어도 5% 아래로는 내려가지 않는다.
        let next = resize_split(&sizes, 0, -10_000.0, 1000.0);
        assert_eq!(next, vec![MIN_PANE_PERCENT, 100.0 - MIN_PANE_PERCENT]);
        let next = resize_split(&sizes, 0, 10_000.0, 1000.0);
        assert_eq!(next, vec![100.0 - MIN_PANE_PERCENT, MIN_PANE_PERCENT]);
        assert!((next.iter().sum::<f32>() - 100.0).abs() < 0.001);
    }

    #[test]
    fn resize_ignores_degenerate_input() {
        let sizes = vec![50.0, 50.0];
        assert_eq!(resize_split(&sizes, 0, 10.0, 0.0), sizes, "길이 0인 split");
        assert_eq!(resize_split(&sizes, 1, 10.0, 100.0), sizes, "마지막 경계는 없음");
        assert_eq!(resize_split(&sizes, 0, f32::NAN, 100.0), sizes);
    }

    #[test]
    fn sizes_are_repaired_when_the_saved_layout_is_broken() {
        assert_eq!(normalized_sizes(&[], 2), vec![50.0, 50.0]);
        assert_eq!(normalized_sizes(&[0.0, 0.0], 2), vec![50.0, 50.0]);
        assert_eq!(normalized_sizes(&[1.0, 1.0, 2.0], 3), vec![25.0, 25.0, 50.0]);
        assert!(normalized_sizes(&[50.0], 0).is_empty());
    }

    /// 2×2 그리드: A B / C D
    fn grid() -> Vec<(SessionId, Bounds<Pixels>)> {
        vec![
            (SessionId::new("a"), bounds(0.0, 0.0, 100.0, 100.0)),
            (SessionId::new("b"), bounds(105.0, 0.0, 100.0, 100.0)),
            (SessionId::new("c"), bounds(0.0, 105.0, 100.0, 100.0)),
            (SessionId::new("d"), bounds(105.0, 105.0, 100.0, 100.0)),
        ]
    }

    #[test]
    fn directional_focus_picks_the_adjacent_pane() {
        let panes = grid();
        let a = SessionId::new("a");
        assert_eq!(nearest_pane(&panes, &a, FocusDir::Right), Some(SessionId::new("b")));
        assert_eq!(nearest_pane(&panes, &a, FocusDir::Down), Some(SessionId::new("c")));
        assert_eq!(nearest_pane(&panes, &a, FocusDir::Left), None, "왼쪽 끝");
        assert_eq!(nearest_pane(&panes, &a, FocusDir::Up), None, "위쪽 끝");

        let d = SessionId::new("d");
        assert_eq!(nearest_pane(&panes, &d, FocusDir::Left), Some(SessionId::new("c")));
        assert_eq!(nearest_pane(&panes, &d, FocusDir::Up), Some(SessionId::new("b")));
    }

    #[test]
    fn directional_focus_prefers_the_overlapping_neighbour_over_a_closer_diagonal() {
        // 왼쪽 열은 위/아래 두 pane, 오른쪽은 위쪽에만 걸친 한 pane.
        let panes = vec![
            (SessionId::new("top"), bounds(0.0, 0.0, 100.0, 50.0)),
            (SessionId::new("bottom"), bounds(0.0, 55.0, 100.0, 50.0)),
            (SessionId::new("right"), bounds(105.0, 0.0, 100.0, 50.0)),
        ];
        // right에서 왼쪽으로: top은 세로가 겹치고(50) bottom은 겹치지 않는다(-5).
        // 진행 축 거리는 둘 다 5px로 같지만 겹침이 있는 top이 이긴다.
        assert_eq!(
            nearest_pane(&panes, &SessionId::new("right"), FocusDir::Left),
            Some(SessionId::new("top"))
        );
        // 겹치지 않는 후보는 겹치는 후보에 항상 진다.
        assert_eq!(
            nearest_pane(&panes, &SessionId::new("top"), FocusDir::Right),
            Some(SessionId::new("right"))
        );
    }

    #[test]
    fn directional_focus_returns_none_for_an_unknown_pane() {
        assert_eq!(nearest_pane(&grid(), &SessionId::new("zzz"), FocusDir::Left), None);
    }

    #[test]
    fn drop_side_follows_the_dominant_axis() {
        let b = bounds(0.0, 0.0, 200.0, 100.0);
        assert_eq!(drop_side(b, point(px(10.0), px(50.0))), DropSide::Left);
        assert_eq!(drop_side(b, point(px(190.0), px(50.0))), DropSide::Right);
        assert_eq!(drop_side(b, point(px(100.0), px(5.0))), DropSide::Top);
        assert_eq!(drop_side(b, point(px(100.0), px(95.0))), DropSide::Bottom);
        // 가로로 긴 pane이라도 위/아래 가장자리는 정규화 덕분에 잡힌다.
        assert_eq!(drop_side(b, point(px(120.0), px(2.0))), DropSide::Top);
    }
}
