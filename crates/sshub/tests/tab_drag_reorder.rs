//! 탭을 끌어 **같은 창 안에서** 순서를 바꾸는 경로의 회귀 테스트.
//!
//! 창을 넘나드는 드래그(§8.1)를 붙이면서 창 셸이 mouse-up을 캡처 페이즈에서
//! 가로채게 됐다. 그 판정이 창 안 드롭을 "창 밖"으로 잘못 읽으면, 순서를
//! 바꾸려던 드래그가 매번 창을 새로 만들어 버린다. 그래서 모델 함수가 아니라
//! **실제 마우스 제스처**로 확인한다 — 가로채기까지 지나가야 의미가 있다.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 레이아웃 파일을
//! 덮어쓴다.

use gpui::{point, px, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use sshub::views::Page;
use sshub::workspace::{self, Workspace};
use sshub_core::window_state::WindowBounds;
use sshub_splits::TabId;

const BOUNDS: WindowBounds = WindowBounds {
    width: 1200,
    height: 800,
    x: Some(80),
    y: Some(60),
};

fn boot(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.keep());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
        sshub::window_manager::init(cx);
    });
}

fn tab_ids(workspace: &Workspace, cx: &gpui::App) -> Vec<TabId> {
    workspace
        .terminal()
        .read(cx)
        .tabs()
        .iter()
        .map(|tab| tab.id.clone())
        .collect()
}

#[gpui::test]
fn dragging_a_tab_onto_another_reorders_it_within_the_window(cx: &mut TestAppContext) {
    boot(cx);
    let window = cx
        .update(|cx| workspace::open(None, BOUNDS, cx))
        .expect("창");
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    // 탭바는 터미널 화면에서만 그려진다(기본 시작 화면은 서버 목록).
    window
        .update(&mut vcx, |this, w, cx| this.set_page(Page::Terminal, w, cx))
        .unwrap();
    window
        .update(&mut vcx, |this, w, cx| {
            let terminal = this.terminal().clone();
            terminal.update(cx, |terminal, cx| {
                terminal.new_tab(w, cx);
                terminal.new_tab(w, cx);
            });
        })
        .unwrap();
    vcx.run_until_parked();

    let (before, bounds) = window
        .update(&mut vcx, |this, _, cx| {
            (tab_ids(this, cx), this.terminal().read(cx).tab_bounds())
        })
        .unwrap();
    assert_eq!(before.len(), 3, "탭 셋으로 시작한다");
    assert_eq!(bounds.len(), 3, "탭바가 그려져 기하가 잡혀야 한다");

    let first = bounds[0].1;
    let last = bounds[2].1;
    // 마지막 탭의 **오른쪽 절반** → 맨 뒤로 (`drop_boundary`).
    let drop_at = point(
        last.origin.x + last.size.width * 0.75,
        last.center().y,
    );

    vcx.simulate_mouse_down(first.center(), MouseButton::Left, Modifiers::none());
    // gpui의 드래그 임계치(2px)를 넘겨야 드래그가 시작된다.
    vcx.simulate_mouse_move(
        point(first.center().x + px(8.0), first.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );
    // 이 시점에 드래그가 시작됐고, 커서를 따라다니는 미리보기가 떠 있어야 한다.
    assert!(
        vcx.update(|_, cx| sshub::drag_ghost::is_active(cx)),
        "드래그가 시작되면 고스트 패널이 뜬다"
    );
    vcx.simulate_mouse_move(drop_at, MouseButton::Left, Modifiers::none());

    // 놓기 **전에** 어디로 갈지 보여야 한다. 캐럿은 맨 뒤(경계 3)에 서 있어야
    // 하고, 아래 실제 결과와 같은 값이어야 한다 — 표시와 결과가 다르면 표시가
    // 없느니만 못하다.
    let caret = window
        .update(&mut vcx, |this, _, cx| this.terminal().read(cx).tab_insert())
        .unwrap();
    assert_eq!(caret, Some(3), "마지막 탭 오른쪽 절반 → 맨 뒤 캐럿");

    vcx.simulate_mouse_up(drop_at, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();

    let after = window.update(&mut vcx, |this, _, cx| tab_ids(this, cx)).unwrap();
    assert_eq!(
        after,
        vec![before[1].clone(), before[2].clone(), before[0].clone()],
        "첫 탭이 맨 뒤로 갔어야 한다 (창이 새로 열리면 여기서 순서가 그대로다)",
    );

    // 같은 창 안 드롭이므로 창은 하나 그대로여야 한다. 하나라도 늘었다면
    // 창 밖 드롭으로 오판해 탭을 떼어낸 것이다.
    let windows = vcx.update(|_, cx| {
        sshub::window_manager::manager(cx).read(cx).ids().len()
    });
    assert_eq!(windows, 1, "순서 변경이 창을 새로 만들면 안 된다");

    // 캐럿은 드래그가 끝나면 사라진다 — 남으면 탭바에 파란 선이 박혀 있다.
    let caret = window
        .update(&mut vcx, |this, _, cx| this.terminal().read(cx).tab_insert())
        .unwrap();
    assert_eq!(caret, None, "드롭 후에는 캐럿을 거둔다");

    // 화면 전체를 덮는 고스트 패널은 드래그가 끝나면 반드시 사라져야 한다 —
    // 남으면 그 뒤로 모든 클릭을 삼킨다.
    assert!(
        !vcx.update(|_, cx| sshub::drag_ghost::is_active(cx)),
        "드래그가 끝나면 고스트 패널이 닫혀야 한다"
    );
}

#[gpui::test]
fn the_caret_follows_the_cursor_across_the_tab_bar_and_hides_off_it(cx: &mut TestAppContext) {
    boot(cx);
    let window = cx
        .update(|cx| workspace::open(None, BOUNDS, cx))
        .expect("창");
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    window
        .update(&mut vcx, |this, w, cx| this.set_page(Page::Terminal, w, cx))
        .unwrap();
    window
        .update(&mut vcx, |this, w, cx| {
            let terminal = this.terminal().clone();
            terminal.update(cx, |terminal, cx| {
                terminal.new_tab(w, cx);
                terminal.new_tab(w, cx);
            });
        })
        .unwrap();
    vcx.run_until_parked();

    let bounds = window
        .update(&mut vcx, |this, _, cx| this.terminal().read(cx).tab_bounds())
        .unwrap();
    let first = bounds[0].1;
    let second = bounds[1].1;

    let caret = |vcx: &mut VisualTestContext| {
        window
            .update(vcx, |this, _, cx| this.terminal().read(cx).tab_insert())
            .unwrap()
    };

    vcx.simulate_mouse_down(first.center(), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(
        point(first.center().x + px(8.0), first.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );

    // 두 번째 탭의 왼쪽 절반 → 그 **앞**(경계 1).
    vcx.simulate_mouse_move(
        point(second.origin.x + second.size.width * 0.25, second.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(caret(&mut vcx), Some(1), "탭 왼쪽 절반이면 그 앞");

    // 오른쪽 절반으로 넘어가면 캐럿도 한 칸 넘어간다.
    vcx.simulate_mouse_move(
        point(second.origin.x + second.size.width * 0.75, second.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(caret(&mut vcx), Some(2), "오른쪽 절반이면 그 뒤");

    // 탭바를 벗어나면(터미널 본문 위) 캐럿을 감춘다 — 거기서는 pane 병합
    // 미리보기가 답이라, 둘이 동시에 뜨면 어디로 가는지 더 헷갈린다.
    vcx.simulate_mouse_move(
        point(second.center().x, second.center().y + px(300.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(caret(&mut vcx), None, "탭바 밖에서는 캐럿을 감춘다");

    vcx.simulate_mouse_up(
        point(second.center().x, second.center().y + px(300.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
}
