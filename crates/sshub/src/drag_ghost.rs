//! 탭을 끌 때 커서를 따라다니는 터미널 미리보기 (DESIGN-terminal.md §8.1).
//!
//! **왜 별도 창인가.** gpui는 드래그 고스트를 드래그가 시작된 창의 씬에 그린다.
//! 커서가 그 창을 벗어나는 순간 고스트가 잘려 사라지는데, 하필 "창 밖으로 꺼내
//! 새 창 만들기"가 정확히 그 상황이라 무엇을 끌고 있는지 보이지 않았다.
//!
//! **왜 화면 전체를 덮는가.** gpui 0.2.2에는 창을 옮기는 API가 없다
//! (`PlatformWindow`에 `resize`만 있고 위치 변경은 없다). 그래서 커서를 따라
//! 작은 창을 옮기는 대신, 디스플레이를 덮는 투명 패널을 띄우고 그 **안에서**
//! 카드의 위치만 바꾼다.
//!
//! **왜 모니터마다 한 장인가.** 한 장으로는 모니터 경계를 넘을 수 없다 — 카드가
//! 그 패널 안에 갇혀 거기서 멈춘다. 대신 디스플레이마다 한 장씩 띄우고 모두에게
//! **같은 전역 커서 좌표**를 준다. 각 패널은 카드를 자기 원점 기준으로 그리고
//! 자기 경계에서 잘라 내므로, 경계에 걸친 카드는 양쪽이 이어 붙어 그려진다
//! (한 장을 여러 모니터에 걸치게 만들 수는 없다 — gpui가 창을 만들 때 좌표를
//! 스크린 하나 기준으로 환산한다).
//!
//! 안전장치가 중요하다 — 이 패널이 남으면 화면 전체가 클릭을 먹는다:
//! - `WindowKind::PopUp` = macOS `NSPanel` + `NonactivatingPanel`이라 포커스를
//!   빼앗지 않는다(`focus: false`로 `orderFront`만 한다).
//! - 드래그를 시작한 창이 mouse-up을 반드시 받으므로([`crate::workspace`]의
//!   `tab_drag_watcher`) 거기서 닫는다.
//! - 그래도 못 닫는 경우를 대비해 [`SAFETY_TIMEOUT`] 후 스스로 닫힌다.

use std::time::{Duration, Instant};

use gpui::{
    div, point, prelude::*, px, App, Bounds, Context, Entity, Global, Pixels, Point, SharedString,
    Size, Task, Window, WindowBackgroundAppearance, WindowHandle, WindowKind, WindowOptions,
};

use crate::displays;

/// 커서가 이만큼 멈춰 있으면 드래그가 죽은 것으로 보고 패널을 치운다.
///
/// 총 드래그 시간이 아니라 **유휴** 시간이다 — 화면을 가로질러 천천히 끄는
/// 것은 정상이지만, 버튼을 누른 채 20초 동안 커서가 한 번도 안 움직이는 것은
/// mouse-up을 놓쳤다는 뜻이다.
pub const SAFETY_TIMEOUT: Duration = Duration::from_secs(20);

/// 카드 크기 — 터미널로 읽히되 커서 주변을 다 가리지는 않는 정도.
const CARD_W: f32 = 260.0;
const CARD_H: f32 = 156.0;
/// 커서가 카드의 어디를 잡고 있는가 (탭을 집은 느낌이 나도록 위쪽 모서리 근처).
const GRAB: (f32, f32) = (46.0, 14.0);
/// 미리보기에 넣을 터미널 행 수.
pub const PREVIEW_LINES: usize = 7;

/// 카드에 그릴 내용 — 드래그 시작 시점의 **스냅샷**이다. 끌고 다니는 동안
/// 살아 있는 터미널을 다시 그리면 그 PTY가 카드 크기로 리사이즈된다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhostContent {
    pub title: SharedString,
    pub lines: Vec<SharedString>,
}

impl GhostContent {
    pub fn new(title: impl Into<SharedString>, lines: Vec<String>) -> GhostContent {
        GhostContent {
            title: title.into(),
            lines: lines.into_iter().map(SharedString::from).collect(),
        }
    }
}

struct GhostView {
    content: GhostContent,
    /// 커서의 **전역** 좌표 (모든 패널이 같은 값을 받는다).
    cursor: Point<Pixels>,
    /// 이 패널의 전역 원점 — 카드를 패널 좌표로 내리는 데 쓴다.
    origin: Point<Pixels>,
    /// 데스크톱 전체(모든 디스플레이의 합집합). 카드를 여기에만 가둔다 —
    /// 패널 하나에 가두면 모니터 경계에서 멈춰 버린다.
    desktop: Bounds<Pixels>,
}

/// 살아 있는 고스트 패널. 전역인 이유는 드래그를 시작한 창이 아니라 **앱**이
/// 이것의 주인이기 때문이다(창을 넘나드는 드래그가 이 기능의 목적이다).
struct ActiveGhost {
    /// 디스플레이마다 한 장.
    panels: Vec<(WindowHandle<GhostView>, Entity<GhostView>)>,
    /// 안전 타이머를 다시 건 시각 — 마우스 이동마다 Task를 새로 만들지 않기
    /// 위해 절반이 지났을 때만 재장전한다.
    armed_at: Instant,
    _safety: Task<()>,
}
impl Global for ActiveGhost {}

/// "고스트를 이 드래그가 맡는다"는 표시. 패널이 실제로 열리기 **전에** 세운다.
///
/// 패널 생성은 한 틱 미뤄지는데(`begin`), 그 사이에 gpui가 창 안 기본 고스트를
/// 그리면 카드가 뜨기 직전 작은 칩이 한 번 깜빡인다. 게다가 패널 자신도 창이라
/// 첫 프레임에 기본 고스트를 자기 좌표(0,0)에 그려 버린다.
struct GhostArmed;
impl Global for GhostArmed {}

/// 고스트 패널이 이 드래그를 맡고 있는가. gpui가 창 안에 그리는 기본 고스트를
/// 숨길지 판단하는 데 쓴다(둘 다 그리면 창 안에서 두 겹으로 보인다).
pub fn is_active(cx: &App) -> bool {
    cx.has_global::<GhostArmed>()
}

/// 드래그 시작 — `at`은 커서의 화면 좌표.
///
/// 실패해도 조용히 넘어간다. 고스트는 장식이라 없다고 드래그가 막히면 안 된다
/// (그때는 gpui의 기본 창 안 고스트가 그대로 보인다).
pub fn begin(content: GhostContent, at: Point<Pixels>, cx: &mut App) {
    end(cx);
    if cx.displays().is_empty() {
        return;
    }
    cx.set_global(GhostArmed);
    // 창을 여는 일은 다음 틱으로 미룬다. 여기는 드래그 시작을 처리하는 ObjC
    // 마우스 콜백 안이고, 그 안에서 터지는 panic은 unwind가 불가능해 앱이
    // 그대로 abort된다 (.claude/rules/app.md).
    cx.defer(move |cx| open_panels(content, at, cx));
}

fn open_panels(content: GhostContent, at: Point<Pixels>, cx: &mut App) {
    let rects = displays::display_rects(cx);
    let Some(desktop) = displays::union(&rects) else {
        cx.remove_global_if_present::<GhostArmed>();
        return;
    };

    let panels: Vec<(WindowHandle<GhostView>, Entity<GhostView>)> = cx
        .displays()
        .into_iter()
        .filter_map(|display| {
            let id = display.id();
            let origin = displays::display_origin(id);
            let options = WindowOptions {
                // gpui는 창 좌표를 **그 디스플레이 기준**으로 환산하므로
                // 원점 (0,0) + 그 디스플레이 지정이 곧 "그 화면을 덮어라"다.
                // `display_id`를 빼면 전부 주 디스플레이에 겹쳐 열린다.
                window_bounds: Some(gpui::WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: display.bounds().size,
                })),
                display_id: Some(id),
                titlebar: None,
                // 포커스를 가져가면 소스 창이 비활성화되고 드래그가 그대로 끊긴다.
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            };
            // 뷰 핸들은 만드는 자리에서 챙긴다 — `WindowHandle::root`는
            // test-support 전용이라 릴리스 빌드에 없다.
            let content = content.clone();
            let mut created: Option<Entity<GhostView>> = None;
            let opened = cx.open_window(options, |_window, cx| {
                let view = cx.new(|_| GhostView {
                    content,
                    cursor: at,
                    origin,
                    desktop,
                });
                created = Some(view.clone());
                view
            });
            match (opened, created) {
                (Ok(handle), Some(view)) => Some((handle, view)),
                _ => None,
            }
        })
        .collect();

    if panels.is_empty() {
        // 패널이 없으면 gpui의 창 안 기본 고스트로 되돌린다.
        cx.remove_global_if_present::<GhostArmed>();
        return;
    }

    let safety = safety_timer(cx);
    cx.set_global(ActiveGhost {
        panels,
        armed_at: Instant::now(),
        _safety: safety,
    });
}

/// 타이머를 드랍하면 취소된다 — 재장전은 곧 이 Task를 새것으로 갈아 끼우는 일이다.
fn safety_timer(cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SAFETY_TIMEOUT).await;
        cx.update(|cx| end(cx)).ok();
    })
}

/// 커서가 움직였다 (`at`은 화면 좌표).
pub fn track(at: Point<Pixels>, cx: &mut App) {
    let Some(ghost) = cx.try_global::<ActiveGhost>() else {
        return;
    };
    let stale = ghost.armed_at.elapsed() * 2 >= SAFETY_TIMEOUT;
    let views: Vec<Entity<GhostView>> =
        ghost.panels.iter().map(|(_, view)| view.clone()).collect();
    // 모든 패널이 같은 전역 좌표를 받는다 — 카드가 모니터 경계를 넘어가는
    // 동안 양쪽이 각자의 몫을 그려 이어 붙는다.
    for view in views {
        view.update(cx, |view, cx| {
            if view.cursor != at {
                view.cursor = at;
                cx.notify();
            }
        });
    }
    if stale {
        let timer = safety_timer(cx);
        if cx.has_global::<ActiveGhost>() {
            let ghost = cx.global_mut::<ActiveGhost>();
            ghost.armed_at = Instant::now();
            ghost._safety = timer;
        }
    }
}

/// 드래그가 끝났다 — 패널을 반드시 치운다.
pub fn end(cx: &mut App) {
    cx.remove_global_if_present::<GhostArmed>();
    let Some(ghost) = cx.remove_global_if_present::<ActiveGhost>() else {
        return;
    };
    for (handle, _) in ghost.panels {
        handle.update(cx, |_, window, _| window.remove_window()).ok();
    }
}

/// `remove_global`은 없는 전역에 대해 패닉이라 존재 확인과 짝지어 둔다.
trait RemoveGlobalIfPresent {
    fn remove_global_if_present<G: Global>(&mut self) -> Option<G>;
}

impl RemoveGlobalIfPresent for App {
    fn remove_global_if_present<G: Global>(&mut self) -> Option<G> {
        if self.has_global::<G>() {
            Some(self.remove_global::<G>())
        } else {
            None
        }
    }
}

/// 카드의 좌상단 (패널 좌표).
///
/// 가두는 기준은 **데스크톱 전체**다. 패널 하나에 가두면 카드가 그 모니터
/// 경계에서 멈춰 "다른 모니터로 넘어갈 수 없는" 것처럼 보인다(실제 신고).
/// 결과가 패널 밖(음수 등)으로 나가는 것은 정상이다 — 그 부분은 이웃 패널이
/// 그린다.
fn card_origin(
    cursor: Point<Pixels>,
    origin: Point<Pixels>,
    desktop: Bounds<Pixels>,
) -> Point<Pixels> {
    let wanted = point(cursor.x - px(GRAB.0), cursor.y - px(GRAB.1));
    let card = Size { width: px(CARD_W), height: px(CARD_H) };
    displays::clamp_into(wanted, card, desktop) - origin
}

impl Render for GhostView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::theme(cx).clone();
        let at = card_origin(self.cursor, self.origin, self.desktop);

        let body = div()
            .flex()
            .flex_col()
            .flex_grow()
            .overflow_hidden()
            .px_2()
            .py_1()
            .font_family(theme.terminal.font_family.clone())
            .text_size(px(11.0))
            .text_color(theme.terminal.foreground)
            .children(self.content.lines.iter().map(|line| {
                div()
                    .h(px(15.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(line.clone())
            }));

        // 패널은 화면 전체지만 그리는 것은 카드 하나뿐이다. 나머지는 완전히
        // 투명해야 한다 — 배경을 칠하면 화면이 통째로 덮인다.
        div().size_full().relative().child(
            div()
                .absolute()
                .left(at.x)
                .top(at.y)
                .w(px(CARD_W))
                .h(px(CARD_H))
                .flex()
                .flex_col()
                .rounded_md()
                .overflow_hidden()
                .border_1()
                .border_color(theme.accent)
                .bg(theme.terminal.background_opaque)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .h(px(24.0))
                        .flex_shrink_0()
                        .px_2()
                        .bg(theme.surface)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.text)
                        .child(div().truncate().child(self.content.title.clone())),
                )
                .child(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    /// 주 모니터(1440p) 오른쪽에 4K가 붙은 데스크톱.
    fn desktop() -> Bounds<Pixels> {
        Bounds { origin: point(px(0.0), px(0.0)), size: size(px(6400.0), px(2160.0)) }
    }

    #[test]
    fn the_card_hangs_off_the_cursor() {
        let at = card_origin(point(px(500.0), px(400.0)), point(px(0.0), px(0.0)), desktop());
        assert_eq!(
            (f32::from(at.x), f32::from(at.y)),
            (500.0 - GRAB.0, 400.0 - GRAB.1)
        );
    }

    #[test]
    fn the_card_crosses_a_monitor_boundary_instead_of_stopping_at_it() {
        // 회귀 방지(실제 신고): 카드를 패널 하나에 가두면 커서가 두 번째
        // 모니터로 넘어가도 카드가 경계에서 멈춰 "넘어갈 수 없다"고 보인다.
        // 두 번째 모니터를 덮는 패널(원점 x=2560)이 자기 몫을 그려야 한다.
        let second = point(px(2560.0), px(0.0));
        let at = card_origin(point(px(2600.0), px(300.0)), second, desktop());
        assert_eq!(
            (f32::from(at.x), f32::from(at.y)),
            (2600.0 - 2560.0 - GRAB.0, 300.0 - GRAB.1),
        );

        // 커서가 아직 첫 모니터에 있어도 두 번째 패널은 **음수 좌표**로 그린다 —
        // 경계에 걸친 카드의 오른쪽 절반이 거기 보여야 이어져 보인다.
        let straddling = card_origin(point(px(2500.0), px(300.0)), second, desktop());
        assert!(
            f32::from(straddling.x) < 0.0,
            "이웃 패널로 새어 나가는 것이 정상이다 (패널이 알아서 자른다)",
        );
    }

    #[test]
    fn the_card_stays_inside_the_desktop_at_its_outer_edges() {
        let at = card_origin(point(px(6500.0), px(2200.0)), point(px(0.0), px(0.0)), desktop());
        assert_eq!(
            (f32::from(at.x), f32::from(at.y)),
            (6400.0 - CARD_W, 2160.0 - CARD_H),
            "데스크톱 바깥으로는 새지 않는다",
        );
    }

    #[test]
    fn a_display_left_of_the_primary_has_negative_global_coordinates() {
        let desktop = Bounds {
            origin: point(px(-1920.0), px(0.0)),
            size: size(px(4480.0), px(1440.0)),
        };
        let left_panel = point(px(-1920.0), px(0.0));
        let at = card_origin(point(px(-1000.0), px(200.0)), left_panel, desktop);
        assert_eq!(
            (f32::from(at.x), f32::from(at.y)),
            (-1000.0 + 1920.0 - GRAB.0, 200.0 - GRAB.1),
        );
    }
}
