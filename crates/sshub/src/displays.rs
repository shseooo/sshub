//! 다중 모니터 전역 좌표 (DESIGN-terminal.md §8.3).
//!
//! **문제.** gpui 0.2.2의 창 좌표는 모니터마다 다른 공간에 있다:
//! - `Window::bounds()`(mac/window.rs)는 x를 **그 창이 놓인 스크린 기준**으로
//!   상대화하고(`x - screen.origin.x`) y를 그 스크린 높이로 뒤집는다.
//! - `PlatformDisplay::bounds()`(mac/display.rs)는 크기만 주고 **원점을 버린다**
//!   (`origin: Default::default()`) — 주석은 전역 좌표라고 해 놓고 실제로는
//!   0을 넣는다.
//!
//! 그래서 모니터가 둘이면 서로 다른 모니터에 있는 두 창의 `bounds()`를 그대로
//! 비교할 수 없다. 탭을 다른 모니터로 끌어 분리하는 것이 정확히 그 상황이다.
//!
//! **해결.** 디스플레이 원점만 CoreGraphics에서 직접 가져와 모든 좌표를
//! **전역 좌표**(주 디스플레이 좌상단이 (0,0), y는 아래로 증가 — gpui `Bounds`와
//! 같은 방향)로 올린다. 관계는 한 줄이다:
//!
//! ```text
//! 전역 = Window::bounds().origin + display_origin(그 창이 놓인 디스플레이)
//! ```
//!
//! (유도: `bounds()`가 빼는 `screen.origin.x`와 뒤집기의 잔차가 정확히
//! `CGDisplayBounds(screen).origin`이다.)
//!
//! 저장되는 창 지오메트리(`WindowRecord.bounds`)는 **바꾸지 않는다** — gpui가
//! 창을 열 때 기대하는 공간이 그 디스플레이 상대 좌표이기 때문이다. 전역 좌표는
//! 화면 위에서 무언가를 맞힐 때만 쓴다.

use gpui::{point, px, App, Bounds, DisplayId, Pixels, Point, Size, Window};

/// 디스플레이의 전역 원점.
///
/// macOS 밖에서는 (0,0) — 이 앱은 macOS 전용이고, 나머지 로직이 단일 모니터
/// 환경과 똑같이 동작하도록 하는 안전한 기본값이다.
#[cfg(target_os = "macos")]
pub fn display_origin(id: DisplayId) -> Point<Pixels> {
    // `CGDisplayBounds`는 주 디스플레이 좌상단이 원점인 전역 좌표를 준다 —
    // gpui `Bounds`와 방향이 같아서 그대로 더하면 된다.
    let bounds = unsafe { core_graphics::display::CGDisplayBounds(u32::from(id)) };
    // 모르는 디스플레이(드래그 도중 모니터를 뽑았다든지)에는 `CGRectNull`이
    // 오는데, 그 원점은 **무한대**다. 그대로 더하면 창 좌표가 통째로 NaN/무한이
    // 되어 드롭 판정이 조용히 전부 실패한다. 단일 모니터처럼 다룬다.
    if !bounds.origin.x.is_finite() || !bounds.origin.y.is_finite() {
        return point(px(0.0), px(0.0));
    }
    point(px(bounds.origin.x as f32), px(bounds.origin.y as f32))
}

#[cfg(not(target_os = "macos"))]
pub fn display_origin(_id: DisplayId) -> Point<Pixels> {
    point(px(0.0), px(0.0))
}

/// **OS가 직접 말해 주는** 커서의 전역 좌표.
///
/// 창 좌표에 디스플레이 원점을 더해 구할 수도 있지만, 그 길은 창이 어느
/// 디스플레이에 있는지를 gpui의 **캐시된** `Window::display`에 의존한다
/// (window.rs: `self.display_id`를 들고 있다가 비교한다). 그 값이 어긋나면
/// 커서 좌표가 통째로 **원래 모니터 기준**으로 나와, 드래그 미리보기가 엉뚱한
/// 모니터의 엉뚱한 자리에 붙는다(실제 신고). `NSEvent.mouseLocation`은 창도
/// 디스플레이도 거치지 않으므로 그 고리가 아예 없다.
#[cfg(target_os = "macos")]
pub fn os_cursor() -> Option<Point<Pixels>> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // `CGEventGetLocation`은 커서를 **전역 디스플레이 좌표**(주 디스플레이
    // 좌상단 원점, y 아래로)로 준다 — gpui `Bounds`와 방향이 같아 변환이 없다.
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let location = CGEvent::new(source).ok()?.location();
    if !location.x.is_finite() || !location.y.is_finite() {
        return None;
    }
    Some(point(px(location.x as f32), px(location.y as f32)))
}

#[cfg(not(target_os = "macos"))]
pub fn os_cursor() -> Option<Point<Pixels>> {
    None
}

/// 테스트가 커서 위치를 정해 두기 위한 덮어쓰기.
///
/// [`os_cursor`]는 **그 기계의 진짜 마우스**를 읽는다 — 그대로 두면 드래그를
/// 흉내 내는 테스트가 테스터의 커서가 어디 있느냐에 따라 통과했다 말았다 한다.
/// 프로덕션에서는 아무도 이 전역을 세우지 않는다.
pub struct CursorOverride(pub Point<Pixels>);
impl gpui::Global for CursorOverride {}

/// 커서의 전역 좌표. OS에 직접 물어보고, 못 얻으면 창 좌표로 환산한다.
pub fn cursor(window: &Window, local: Point<Pixels>, cx: &App) -> Point<Pixels> {
    if let Some(fixed) = cx.try_global::<CursorOverride>() {
        return fixed.0;
    }
    os_cursor().unwrap_or_else(|| to_global(window, local, cx))
}

/// 창 좌표가 이 창 **안**인가.
///
/// 전역 사각형끼리 비교하지 않는다 — 창의 로컬 좌표계에서 `0..size`를 보는 것이
/// 디스플레이 배치와 무관하게 언제나 정확하다. 창 안 드롭(탭 순서 변경)이
/// 좌표 환산 하나 어긋났다고 "창 밖"이 되면 안 된다.
pub fn is_inside(window: &Window, local: Point<Pixels>) -> bool {
    contains_local(window.bounds().size, local)
}

/// 모든 디스플레이의 전역 사각형.
pub fn display_rects(cx: &App) -> Vec<Bounds<Pixels>> {
    cx.displays()
        .into_iter()
        .map(|display| Bounds {
            origin: display_origin(display.id()),
            size: display.bounds().size,
        })
        .collect()
}

/// 이 창의 전역 좌상단.
pub fn window_origin(window: &Window, cx: &App) -> Point<Pixels> {
    let local = window.bounds().origin;
    match window.display(cx) {
        Some(display) => local + display_origin(display.id()),
        None => local,
    }
}

/// 이 창의 전역 사각형.
pub fn window_rect(window: &Window, cx: &App) -> Bounds<Pixels> {
    Bounds {
        origin: window_origin(window, cx),
        size: window.bounds().size,
    }
}

/// 창 좌표 → 전역 좌표 (마우스 이벤트 위치를 올릴 때).
pub fn to_global(window: &Window, local: Point<Pixels>, cx: &App) -> Point<Pixels> {
    window_origin(window, cx) + local
}

// ---------------------------------------------------------------------------
// 순수 계산 (테스트 대상)
// ---------------------------------------------------------------------------

/// 사각형들을 모두 담는 최소 사각형. 비었으면 `None`.
/// 데스크톱 전체 범위 = 고스트 카드를 가둘 한계다.
pub fn union(rects: &[Bounds<Pixels>]) -> Option<Bounds<Pixels>> {
    let mut iter = rects.iter();
    let first = iter.next()?;
    let (mut left, mut top) = (f32::from(first.origin.x), f32::from(first.origin.y));
    let (mut right, mut bottom) = (
        left + f32::from(first.size.width),
        top + f32::from(first.size.height),
    );
    for rect in iter {
        let (x, y) = (f32::from(rect.origin.x), f32::from(rect.origin.y));
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + f32::from(rect.size.width));
        bottom = bottom.max(y + f32::from(rect.size.height));
    }
    Some(Bounds {
        origin: point(px(left), px(top)),
        size: Size {
            width: px(right - left),
            height: px(bottom - top),
        },
    })
}

/// 이 전역 좌표를 담는 사각형의 인덱스 — 어느 모니터에 놓았는지.
///
/// 오른쪽·아래 변은 배타적이다. 나란히 붙은 두 모니터의 경계에서 양쪽이 모두
/// 잡히면 창이 엉뚱한 쪽에 열린다.
pub fn index_at(rects: &[Bounds<Pixels>], at: Point<Pixels>) -> Option<usize> {
    rects.iter().position(|rect| contains(rect, at))
}

/// 점이 사각형 안인가 — **오른쪽·아래 변은 배타적**이다.
///
/// 나란히 붙은 두 모니터(또는 딱 맞닿은 두 창)의 경계에서 양쪽이 모두 잡히면
/// 드롭이 어디로 갈지 좌표에 따라 흔들린다.
pub fn contains(rect: &Bounds<Pixels>, at: Point<Pixels>) -> bool {
    let (x, y) = (f32::from(at.x), f32::from(at.y));
    let (left, top) = (f32::from(rect.origin.x), f32::from(rect.origin.y));
    x >= left
        && y >= top
        && x < left + f32::from(rect.size.width)
        && y < top + f32::from(rect.size.height)
}

/// 창 좌표가 `0..size` 안인가 ([`is_inside`]의 순수 부분).
pub fn contains_local(size: Size<Pixels>, local: Point<Pixels>) -> bool {
    let (x, y) = (f32::from(local.x), f32::from(local.y));
    x >= 0.0 && y >= 0.0 && x < f32::from(size.width) && y < f32::from(size.height)
}

/// `at`을 `bounds` 안으로 밀어 넣는다 (`size`짜리 상자의 좌상단 기준).
pub fn clamp_into(at: Point<Pixels>, size: Size<Pixels>, bounds: Bounds<Pixels>) -> Point<Pixels> {
    let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let max_x = (left + f32::from(bounds.size.width) - f32::from(size.width)).max(left);
    let max_y = (top + f32::from(bounds.size.height) - f32::from(size.height)).max(top);
    point(
        px(f32::from(at.x).clamp(left, max_x)),
        px(f32::from(at.y).clamp(top, max_y)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    /// 주 디스플레이(1440p) 오른쪽에 4K를 붙인 배치.
    fn two_monitors() -> Vec<Bounds<Pixels>> {
        vec![
            Bounds {
                origin: point(px(0.0), px(0.0)),
                size: size(px(2560.0), px(1440.0)),
            },
            Bounds {
                origin: point(px(2560.0), px(0.0)),
                size: size(px(3840.0), px(2160.0)),
            },
        ]
    }

    #[test]
    fn the_desktop_is_the_union_of_every_display() {
        let all = union(&two_monitors()).expect("모니터가 있으면 범위가 있다");
        assert_eq!((f32::from(all.origin.x), f32::from(all.origin.y)), (0.0, 0.0));
        assert_eq!(
            (f32::from(all.size.width), f32::from(all.size.height)),
            (6400.0, 2160.0),
            "가장 넓은/높은 쪽까지 덮어야 카드가 모니터 경계를 넘어간다",
        );
        assert!(union(&[]).is_none());
    }

    #[test]
    fn a_display_to_the_left_of_the_primary_gives_a_negative_origin() {
        // 주 디스플레이 **왼쪽**에 놓인 모니터는 전역 x가 음수다. 창 위치를
        // u32로 다루면 여기서 무너진다.
        let rects = vec![
            Bounds { origin: point(px(-1920.0), px(0.0)), size: size(px(1920.0), px(1080.0)) },
            Bounds { origin: point(px(0.0), px(0.0)), size: size(px(2560.0), px(1440.0)) },
        ];
        let all = union(&rects).unwrap();
        assert_eq!(f32::from(all.origin.x), -1920.0);
        assert_eq!(f32::from(all.size.width), 4480.0);
        assert_eq!(index_at(&rects, point(px(-100.0), px(50.0))), Some(0));
    }

    #[test]
    fn a_point_lands_on_exactly_one_monitor() {
        let rects = two_monitors();
        assert_eq!(index_at(&rects, point(px(10.0), px(10.0))), Some(0));
        assert_eq!(index_at(&rects, point(px(3000.0), px(1800.0))), Some(1));
        // 경계는 오른쪽 배타 — 두 모니터가 동시에 잡히면 안 된다.
        assert_eq!(index_at(&rects, point(px(2560.0), px(10.0))), Some(1));
        assert_eq!(index_at(&rects, point(px(2559.0), px(10.0))), Some(0));
        // 주 모니터 아래쪽(보조가 더 큰 경우 생기는 빈 공간)은 어디에도 없다.
        assert_eq!(index_at(&rects, point(px(10.0), px(1800.0))), None);
    }

    #[test]
    fn the_card_is_clamped_to_the_desktop_not_to_one_monitor() {
        // 회귀 방지: 모니터 하나에 가두면 카드가 경계에서 멈춰 "넘어갈 수 없는"
        // 것처럼 보인다. 데스크톱 전체가 한계여야 경계를 그대로 통과한다.
        let desktop = union(&two_monitors()).unwrap();
        let card = size(px(260.0), px(156.0));

        let across = clamp_into(point(px(2500.0), px(100.0)), card, desktop);
        assert_eq!(
            (f32::from(across.x), f32::from(across.y)),
            (2500.0, 100.0),
            "모니터 경계 근처에서도 그대로 둔다",
        );

        let far = clamp_into(point(px(6500.0), px(2100.0)), card, desktop);
        assert_eq!(
            (f32::from(far.x), f32::from(far.y)),
            (6400.0 - 260.0, 2160.0 - 156.0),
            "데스크톱 바깥으로는 새지 않는다",
        );
    }

    #[test]
    fn a_drop_is_inside_its_own_window_by_local_coordinates_alone() {
        // 창 안 판정에 디스플레이 배치가 끼면 안 된다 — 탭 순서 변경이
        // 좌표 환산 하나 때문에 "창 밖"이 되어 새 창을 만든 적이 있다.
        let win = size(px(1200.0), px(800.0));
        assert!(contains_local(win, point(px(0.0), px(0.0))), "좌상단은 포함");
        assert!(contains_local(win, point(px(600.0), px(15.0))), "탭바 위");
        // 커서가 창을 벗어나면 창 좌표는 음수이거나 크기를 넘는다.
        assert!(!contains_local(win, point(px(-1.0), px(15.0))));
        assert!(!contains_local(win, point(px(600.0), px(-1.0))));
        assert!(!contains_local(win, point(px(1200.0), px(15.0))), "오른쪽 변은 배타");
        assert!(!contains_local(win, point(px(600.0), px(800.0))));
        assert!(!contains_local(win, point(px(4000.0), px(400.0))), "다른 모니터까지 나감");
    }

    #[test]
    fn clamping_into_a_space_smaller_than_the_card_does_not_invert() {
        let tiny = Bounds { origin: point(px(10.0), px(20.0)), size: size(px(100.0), px(80.0)) };
        let at = clamp_into(point(px(0.0), px(0.0)), size(px(260.0), px(156.0)), tiny);
        assert_eq!((f32::from(at.x), f32::from(at.y)), (10.0, 20.0));
    }
}
