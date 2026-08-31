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
    canvas, div, point, px, relative, size, AnyElement, App, AppContext as _, Bounds, CursorStyle, Div, Entity,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window,
};
use sshub_splits::{DropSide, PaneNode, SessionId, SplitDirection, SplitId, TabId, TerminalLeaf};

use crate::terminal_view::TerminalView;
use crate::theme::{with_alpha, Theme};
use crate::ui::tooltip::TextTooltip;
use crate::ui::TextInput;

/// 자식 pane의 최소 비율 — 이 아래로는 드래그해도 줄지 않는다.
pub const MIN_PANE_PERCENT: f32 = 5.0;
/// 디바이더 히트박스 두께.
pub const DIVIDER_PX: f32 = 5.0;
/// pane 헤더 높이 — 원본 Electron `TerminalHost`의 `h-6`(24px)와 같다.
pub const PANE_HEADER_PX: f32 = 24.0;
/// 드롭 미리보기 오버레이의 불투명도 — 아래 터미널 내용이 비쳐야 어디에
/// 꽂히는지 판단할 수 있다.
const DROP_OVERLAY_ALPHA: f32 = 0.16;

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

/// 지금 끌고 있는 탭 (앱 스코프).
///
/// gpui는 `App::active_drag`를 공개하지 않아서 드롭 시점에 페이로드를 다시
/// 읽을 수 없다. 창 밖 드롭은 어떤 `on_drop`도 타지 않으므로(드롭 대상이
/// 없다) 창 셸이 mouse-up을 직접 받아 처리해야 하고, 그때 "무엇을 끌고
/// 있었는지"의 유일한 출처가 이 전역이다. 드래그 시작에서 쓰고, 창 셸이
/// mouse-up마다 지운다.
pub struct ActiveTabDrag(pub TabId);
impl gpui::Global for ActiveTabDrag {}

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
    /// 이 탭의 leaf 수 — 헤더 표시 여부의 유일한 근거([`pane_header_visible`]).
    pub leaf_count: usize,
    /// 드래그가 올라와 있는 pane과 그때의 삽입 방향 (드롭 미리보기).
    pub drag_over: Option<(SessionId, DropSide)>,
    /// 라벨을 편집 중인 pane과 그 입력 위젯 (탭바의 인라인 rename과 같은 방식).
    pub renaming: Option<(&'a SessionId, &'a Entity<TextInput>)>,
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
/// 드래그 호버 보고. `Some(side)`는 진입/이동, `None`은 그 pane에서 벗어남이다.
/// pane별로 자기 자신에 대해서만 보고하므로 리스너 호출 순서에 의존하지 않는다.
pub type DragOverCallback = Box<dyn Fn(SessionId, Option<DropSide>, &mut Window, &mut App)>;
/// 우클릭 — 포인터의 **창 좌표**를 그대로 넘긴다(컨텍스트 메뉴가 거기 뜬다).
pub type PaneMenuCallback = Box<dyn Fn(SessionId, Point<Pixels>, &mut Window, &mut App)>;

pub struct PaneHandlers {
    pub focus: PaneCallback,
    /// 헤더 더블클릭 — 인라인 라벨 변경 시작.
    pub rename: PaneCallback,
    /// 우클릭 — pane 컨텍스트 메뉴.
    pub context_menu: PaneMenuCallback,
    pub divider_down: DividerCallback,
    pub drop_pane: PaneDropCallback,
    pub drop_tab: TabDropCallback,
    pub drag_over: DragOverCallback,
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

/// pane 헤더를 그릴지 — 분할되지 않은 탭에서는 터미널 위 24px을 쓰지 않는다
/// (원본 Electron `showHeader = leaves(tab.root).length > 1`과 동일 규칙).
pub fn pane_header_visible(leaf_count: usize) -> bool {
    leaf_count > 1
}

/// 드롭 미리보기가 덮을 사각형 — `side` 쪽 **절반**이다.
///
/// 실제 렌더는 `relative(0.5)` CSS로 같은 모양을 그리므로, 이 함수는 그
/// 기하 계약을 테스트로 고정해 두는 자리다(어느 한쪽만 바뀌면 테스트가 깨진다).
pub fn drop_overlay(bounds: Bounds<Pixels>, side: DropSide) -> Bounds<Pixels> {
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let (half_w, half_h) = (w / 2.0, h / 2.0);
    match side {
        DropSide::Left => Bounds::new(point(px(x), px(y)), size(px(half_w), px(h))),
        DropSide::Right => Bounds::new(point(px(x + half_w), px(y)), size(px(half_w), px(h))),
        DropSide::Top => Bounds::new(point(px(x), px(y)), size(px(w), px(half_h))),
        DropSide::Bottom => Bounds::new(point(px(x), px(y + half_h)), size(px(w), px(half_h))),
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
        // 화면 전체를 덮는 고스트 패널이 떠 있으면 여기서는 아무것도 그리지
        // 않는다 — 둘 다 그리면 창 안에서만 미리보기가 두 겹으로 보인다.
        if crate::drag_ghost::is_active(cx) {
            return div().into_any_element();
        }
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
            .into_any_element()
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
    let menu_handlers = ctx.handlers.clone();
    let menu_id = session_id.clone();
    let drop_pane_handlers = ctx.handlers.clone();
    let drop_tab_handlers = ctx.handlers.clone();
    let pane_target = session_id.clone();
    let tab_target = session_id.clone();
    let over_pane_handlers = ctx.handlers.clone();
    let over_pane_id = session_id.clone();
    let over_tab_handlers = ctx.handlers.clone();
    let over_tab_id = session_id.clone();

    let mut container = div()
        .id(element_id("pane", session_id.as_str()))
        .relative()
        .size_full()
        .border_1()
        .border_color(border)
        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
            (handlers.focus)(focus_id.clone(), window, cx);
        })
        // 우클릭 — 컨텍스트 메뉴. 터미널 본문의 마우스 리스너는 왼쪽 버튼에만
        // 선택을 시작하므로(`Terminal::mouse_down`) 메뉴를 열어도 드래그 선택이
        // 같이 시작되지 않는다. 메뉴 자체는 `deferred`로 그려져 터미널 위에 뜬다.
        .on_mouse_down(
            MouseButton::Right,
            move |event: &MouseDownEvent, window, cx| {
                (menu_handlers.context_menu)(menu_id.clone(), event.position, window, cx);
            },
        )
        // 드래그 중 포인터 위치는 `on_drag_move`로만 알 수 있다(`drag_over`는
        // 요소 전체 스타일만 바꿔 어느 쪽 절반인지 표현하지 못한다).
        .on_drag_move::<PaneDrag>(move |event, window, cx| {
            report_drag_over(&over_pane_handlers, &over_pane_id, event.bounds, event.event.position, window, cx);
        })
        .on_drag_move::<TabDrag>(move |event, window, cx| {
            report_drag_over(&over_tab_handlers, &over_tab_id, event.bounds, event.event.position, window, cx);
        })
        .on_drop::<PaneDrag>(move |drag, window, cx| {
            (drop_pane_handlers.drop_pane)(drag.clone(), pane_target.clone(), window, cx);
        })
        .on_drop::<TabDrag>(move |drag, window, cx| {
            (drop_tab_handlers.drop_tab)(drag.clone(), tab_target.clone(), window, cx);
        })
        .child(record);

    // 분할된 탭에서만 헤더를 얹는다 — 단일 pane에서는 터미널이 전체를 쓴다.
    container = if pane_header_visible(ctx.leaf_count) {
        // basis 0으로 고정해야 터미널의 내용 크기가 레이아웃에 끼어들어
        // 헤더를 밀어내지 못한다(split의 region과 같은 이유).
        let mut rest = div().overflow_hidden();
        rest.style().flex_grow = Some(1.0);
        rest.style().flex_shrink = Some(1.0);
        rest.style().flex_basis = Some(px(0.0).into());
        container.child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(render_pane_header(leaf, ctx, focused))
                .child(rest.child(body)),
        )
    } else {
        container.child(body)
    };

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

    // 드롭 미리보기 — 실제로 꽂힐 절반을 어센트 워시 + 2px 모서리 선으로 덮는다.
    if let Some((_, side)) = ctx.drag_over.as_ref().filter(|(id, _)| *id == session_id) {
        container = container.child(drop_overlay_element(*side, theme));
    }

    container.into_any_element()
}

/// pane 헤더 — 라벨 + 드래그 손잡이. 바 전체가 드래그 소스다(원본 Electron의
/// `draggable` 헤더). 터미널 본문을 드래그 소스로 만들면 텍스트 선택과 충돌한다.
fn render_pane_header(leaf: &TerminalLeaf, ctx: &PaneTreeCtx<'_>, focused: bool) -> Stateful<Div> {
    let theme = ctx.theme;
    let session_id = leaf.session_id.clone();
    let label = (ctx.handlers.drag_label)(&session_id);

    let renaming = ctx
        .renaming
        .filter(|(id, _)| *id == &session_id)
        .map(|(_, input)| input.clone());

    let title: AnyElement = match renaming {
        Some(input) => div().w(px(140.0)).flex_shrink_0().child(input).into_any_element(),
        // 탭과 같은 규칙 — nowrap 없이 overflow만 걸면 긴 이름이 접혀 헤더 높이를
        // 밀어낸다. 전체 이름은 헤더 호버 툴팁에서 본다.
        None => div()
            .flex_grow()
            .min_w(px(0.))
            .truncate()
            .text_xs()
            .text_color(if focused { theme.text } else { theme.text_muted })
            .child(label.clone())
            .into_any_element(),
    };

    let click_handlers = ctx.handlers.clone();
    let click_id = session_id.clone();
    let ghost = label.clone();
    let drag_payload = PaneDrag {
        tab_id: ctx.tab_id.clone(),
        session_id: session_id.clone(),
    };

    let tooltip_label = label.clone();
    div()
        .id(element_id("pane-header", session_id.as_str()))
        // 잘린 이름 전체 보기 (gpui가 500ms 호버 지연을 준다).
        .tooltip(move |_window, cx| TextTooltip::view(tooltip_label.clone(), cx))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .h(px(PANE_HEADER_PX))
        .w_full()
        .px_2()
        .bg(if focused { theme.selected } else { theme.surface })
        .border_b_1()
        .border_color(if focused { theme.accent } else { theme.border_subtle })
        .cursor(CursorStyle::OpenHand)
        .child(title)
        .child(div().flex_shrink_0().text_xs().text_color(theme.text_disabled).child("⠿"))
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                if event.click_count >= 2 {
                    // 상위 컨테이너의 focus 핸들러가 이어 돌면 방금 띄운 입력에서
                    // 포커스를 뺏어가 편집이 즉시 취소된다.
                    cx.stop_propagation();
                    (click_handlers.rename)(click_id.clone(), window, cx);
                } else {
                    (click_handlers.focus)(click_id.clone(), window, cx);
                }
            },
        )
        .on_drag(drag_payload, move |_payload, _offset, _window, cx| {
            let label = ghost.clone();
            cx.new(|_| DragGhost { label })
        })
}

/// [`drop_overlay`]와 같은 절반을 CSS(`relative(0.5)`)로 그린다.
fn drop_overlay_element(side: DropSide, theme: &Theme) -> Div {
    let base = div()
        .absolute()
        .bg(with_alpha(theme.accent, DROP_OVERLAY_ALPHA))
        .border_color(theme.accent);
    match side {
        DropSide::Left => base.top_0().left_0().h_full().w(relative(0.5)).border_l_2(),
        DropSide::Right => base.top_0().right_0().h_full().w(relative(0.5)).border_r_2(),
        DropSide::Top => base.top_0().left_0().w_full().h(relative(0.5)).border_t_2(),
        DropSide::Bottom => base.bottom_0().left_0().w_full().h(relative(0.5)).border_b_2(),
    }
}

/// `on_drag_move`는 요소 밖의 이동까지 전부 받으므로(gpui가 등록한 전역 mouse
/// move 리스너다) 여기서 bounds로 직접 걸러야 한다.
fn report_drag_over(
    handlers: &Rc<PaneHandlers>,
    session: &SessionId,
    bounds: Bounds<Pixels>,
    at: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let side = bounds.contains(&at).then(|| drop_side(bounds, at));
    (handlers.drag_over)(session.clone(), side, window, cx);
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

    /// 포인터 한 점 → (어느 pane, 어느 절반). 드롭 미리보기가 판단하는 것과
    /// 같은 경로다: 포함하는 pane을 찾고 그 안에서 `drop_side`를 적용한다.
    fn pane_at(
        panes: &[(SessionId, Bounds<Pixels>)],
        at: Point<Pixels>,
    ) -> Option<(SessionId, DropSide)> {
        panes
            .iter()
            .find(|(_, b)| b.contains(&at))
            .map(|(id, b)| (id.clone(), drop_side(*b, at)))
    }

    #[test]
    fn pointer_maps_to_the_pane_under_it_and_the_nearer_half() {
        let panes = grid();
        // 왼쪽 위 pane의 오른쪽 가장자리 → a의 Right.
        assert_eq!(
            pane_at(&panes, point(px(95.0), px(50.0))),
            Some((SessionId::new("a"), DropSide::Right))
        );
        // 같은 x라도 아래 행이면 c가 잡힌다.
        assert_eq!(
            pane_at(&panes, point(px(95.0), px(150.0))),
            Some((SessionId::new("c"), DropSide::Right))
        );
        // 오른쪽 아래 pane의 위쪽 가장자리 → d의 Top.
        assert_eq!(
            pane_at(&panes, point(px(155.0), px(110.0))),
            Some((SessionId::new("d"), DropSide::Top))
        );
        // 디바이더 틈(101..105)은 어느 pane에도 속하지 않는다 — 미리보기 없음.
        assert_eq!(pane_at(&panes, point(px(102.0), px(50.0))), None);
    }

    #[test]
    fn the_header_only_appears_once_a_tab_is_split() {
        assert!(!pane_header_visible(0), "빈 탭");
        assert!(!pane_header_visible(1), "단일 pane — 터미널이 전체를 쓴다");
        assert!(pane_header_visible(2));
        assert!(pane_header_visible(9));
    }

    #[test]
    fn drop_overlay_covers_exactly_the_matching_half() {
        let b = bounds(10.0, 20.0, 200.0, 100.0);
        assert_eq!(drop_overlay(b, DropSide::Left), bounds(10.0, 20.0, 100.0, 100.0));
        assert_eq!(drop_overlay(b, DropSide::Right), bounds(110.0, 20.0, 100.0, 100.0));
        assert_eq!(drop_overlay(b, DropSide::Top), bounds(10.0, 20.0, 200.0, 50.0));
        assert_eq!(drop_overlay(b, DropSide::Bottom), bounds(10.0, 70.0, 200.0, 50.0));

        // 각 절반은 원본의 정확히 절반이고, 마주보는 쌍은 원본을 빈틈없이 채운다.
        for side in [DropSide::Left, DropSide::Right, DropSide::Top, DropSide::Bottom] {
            let half = drop_overlay(b, side);
            let area = f32::from(half.size.width) * f32::from(half.size.height);
            assert!((area - 200.0 * 100.0 / 2.0).abs() < 0.001, "{side:?}");
            assert!(b.contains(&half.origin), "{side:?} 오버레이는 pane 안에서 시작한다");
        }
        assert_eq!(
            f32::from(drop_overlay(b, DropSide::Left).size.width)
                + f32::from(drop_overlay(b, DropSide::Right).size.width),
            f32::from(b.size.width)
        );
    }

    /// 미리보기가 가리키는 절반과 실제 삽입 방향이 같은 축을 쓰는지 —
    /// 둘이 어긋나면 "왼쪽에 하이라이트, 오른쪽에 삽입" 같은 거짓말이 된다.
    #[test]
    fn the_previewed_half_agrees_with_the_insert_axis() {
        let b = bounds(0.0, 0.0, 200.0, 100.0);
        let left = drop_overlay(b, drop_side(b, point(px(5.0), px(50.0))));
        assert_eq!(left.origin.x, b.origin.x, "왼쪽 절반은 왼쪽 모서리에 붙는다");
        let bottom = drop_overlay(b, drop_side(b, point(px(100.0), px(98.0))));
        assert_eq!(bottom.origin.y, px(50.0), "아래쪽 절반은 중앙에서 시작한다");
    }
}
