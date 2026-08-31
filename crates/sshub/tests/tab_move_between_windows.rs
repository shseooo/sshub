//! 창을 넘나드는 탭 이동의 핵심 불변식: **PTY가 살아서 따라간다**.
//!
//! `SessionRegistry`가 앱 스코프라 터미널 엔티티는 창 밖에 산다. 옮기는 쪽은
//! 뷰만 버리고(`take_tab`), 받는 쪽은 같은 session id로 붙는다(`receive_tab`).
//! 여기서 세션이 죽으면 사용자는 화면·스크롤백·실행 중이던 명령을 전부 잃는다.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 레이아웃/서버 파일을
//! 덮어쓴다.

use gpui::{TestAppContext, VisualTestContext};
use sshub::terminal_workspace::TerminalWorkspace;
use sshub_splits::SessionId;

fn boot(cx: &mut TestAppContext) -> sshub_core::AppPaths {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // TempDir을 흘려도 테스트 프로세스가 끝나면 OS가 치운다. 경로만 있으면 된다.
    let paths = sshub_core::AppPaths::in_dir(dir.keep());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });
    paths
}

fn sessions(ws: &TerminalWorkspace) -> Vec<Vec<SessionId>> {
    ws.tabs()
        .iter()
        .map(|tab| {
            sshub_splits::leaves(&tab.root)
                .into_iter()
                .map(|l| l.session_id.clone())
                .collect()
        })
        .collect()
}

#[gpui::test]
fn a_tab_dragged_into_another_window_keeps_its_live_pty(cx: &mut TestAppContext) {
    boot(cx);

    let source = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let target = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(source.into(), cx);
    vcx.run_until_parked();

    // 소스 창에 탭을 하나 더 만들어, 옮길 탭과 남을 탭을 구분한다.
    source
        .update(&mut vcx, |ws, window, cx| ws.new_tab(window, cx))
        .unwrap();
    vcx.run_until_parked();

    let (moved_id, moved_session) = source
        .update(&mut vcx, |ws, _, _| {
            let tabs = ws.tabs();
            assert_eq!(tabs.len(), 2, "소스 창은 탭 두 개로 시작한다");
            (tabs[1].id.clone(), sessions(ws)[1][0].clone())
        })
        .unwrap();

    // 옮기기 전의 터미널 엔티티 — 이 값이 바뀌면 셸이 다시 뜬 것이다.
    let before = vcx.update(|_, cx| {
        sshub::session_registry::registry(cx)
            .read(cx)
            .get(&moved_session)
            .expect("옮길 탭에는 살아 있는 세션이 있다")
            .entity_id()
    });

    let tab = source
        .update(&mut vcx, |ws, window, cx| {
            ws.take_tab(&moved_id, window, cx).expect("탭을 뗀다")
        })
        .unwrap();

    // 뗀 직후에도 세션은 살아 있어야 한다 — `take_tab`은 뷰만 버린다.
    // (여기서 죽으면 `close_tab`을 잘못 부른 것이다.)
    assert!(
        vcx.update(|_, cx| sshub::session_registry::registry(cx)
            .read(cx)
            .is_live(&moved_session)),
        "탭을 떼는 것만으로 PTY를 죽이면 안 된다"
    );

    // 목적 창의 **맨 앞**에 꽂는다 (드롭 지점이 첫 탭 왼쪽 절반이었던 경우).
    target
        .update(&mut vcx, |ws, window, cx| {
            ws.receive_tab(tab, Some(0), window, cx)
        })
        .unwrap();
    vcx.run_until_parked();

    source
        .update(&mut vcx, |ws, _, _| {
            assert_eq!(ws.tabs().len(), 1, "옮긴 탭은 소스 창에서 사라진다");
            assert!(
                !sessions(ws).iter().any(|s| s.contains(&moved_session)),
                "옮긴 세션이 소스 창에 남아 있으면 두 창이 한 PTY를 공유한다"
            );
        })
        .unwrap();

    target
        .update(&mut vcx, |ws, _, _| {
            let tabs = sessions(ws);
            assert_eq!(tabs.len(), 2, "목적 창은 자기 탭 + 받은 탭");
            assert_eq!(tabs[0], vec![moved_session.clone()], "경계 0 → 맨 앞");
            assert_eq!(
                ws.active_tab_id(),
                Some(&ws.tabs()[0].id),
                "받은 탭이 활성화된다"
            );
        })
        .unwrap();

    let after = vcx.update(|_, cx| {
        sshub::session_registry::registry(cx)
            .read(cx)
            .get(&moved_session)
            .expect("옮긴 뒤에도 세션이 살아 있어야 한다")
            .entity_id()
    });
    assert_eq!(
        before, after,
        "같은 터미널 엔티티를 재사용해야 grid·스크롤백이 그대로 따라간다"
    );
}

#[gpui::test]
fn receiving_a_tab_appends_when_no_boundary_is_given(cx: &mut TestAppContext) {
    boot(cx);

    let source = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let target = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(source.into(), cx);
    vcx.run_until_parked();

    let (id, session) = source
        .update(&mut vcx, |ws, _, _| {
            (ws.tabs()[0].id.clone(), sessions(ws)[0][0].clone())
        })
        .unwrap();
    let existing = target
        .update(&mut vcx, |ws, _, _| sessions(ws)[0][0].clone())
        .unwrap();

    let tab = source
        .update(&mut vcx, |ws, window, cx| {
            ws.take_tab(&id, window, cx).expect("탭을 뗀다")
        })
        .unwrap();

    // 마지막 탭을 떼면 창은 **빈 채로** 남는다 — 창을 닫을지 기본 탭을 채울지는
    // 창 셸이 정한다(드래그는 닫고, 메뉴는 채운다).
    source
        .update(&mut vcx, |ws, _, _| {
            assert!(ws.tabs().is_empty(), "take_tab은 빈 창을 스스로 메우지 않는다");
        })
        .unwrap();

    target
        .update(&mut vcx, |ws, window, cx| ws.receive_tab(tab, None, window, cx))
        .unwrap();
    vcx.run_until_parked();

    target
        .update(&mut vcx, |ws, _, _| {
            assert_eq!(sessions(ws), vec![vec![existing], vec![session]], "끝에 붙는다");
        })
        .unwrap();
}

/// 드래그로 **다른 창에 옮길 때**도 어디에 꽂힐지 보여야 한다.
///
/// 목적 창은 드래그 중 마우스 이벤트를 받지 못하므로(macOS implicit capture)
/// 소스 창이 캐럿을 대신 밀어 넣는다. 그 캐럿이 선 자리와 실제로 꽂히는 자리가
/// 같은지가 이 테스트의 요지다.
#[gpui::test]
fn a_drag_over_another_window_shows_where_the_tab_will_land(cx: &mut TestAppContext) {
    use gpui::{point, px, Modifiers, MouseButton};
    use sshub::views::Page;
    use sshub::workspace;
    use sshub_core::window_state::WindowBounds;

    boot(cx);
    cx.update(|cx| {
        sshub::window_manager::init(cx);
    });

    let bounds = |x: i32| WindowBounds { width: 1200, height: 800, x: Some(x), y: Some(60) };
    let source = cx.update(|cx| workspace::open(None, bounds(0), cx)).expect("소스 창");
    let target = cx.update(|cx| workspace::open(None, bounds(1400), cx)).expect("목적 창");

    let mut vcx = VisualTestContext::from_window(source.into(), cx);
    vcx.run_until_parked();
    for window in [source, target] {
        window
            .update(&mut vcx, |this, w, cx| this.set_page(Page::Terminal, w, cx))
            .unwrap();
    }
    // 소스 창에 탭을 하나 더 — 마지막 탭을 넘기면 창이 닫혀서 확인이 어렵다.
    source
        .update(&mut vcx, |this, w, cx| {
            let terminal = this.terminal().clone();
            terminal.update(cx, |terminal, cx| terminal.new_tab(w, cx));
        })
        .unwrap();
    vcx.run_until_parked();

    let (moved_id, moved_session, tab_bounds) = source
        .update(&mut vcx, |this, _, cx| {
            let terminal = this.terminal().read(cx);
            let tabs = terminal.tabs();
            assert_eq!(tabs.len(), 2);
            (
                tabs[1].id.clone(),
                sshub_splits::leaves(&tabs[1].root)[0].session_id.clone(),
                terminal.tab_bounds(),
            )
        })
        .unwrap();
    let grab = tab_bounds
        .iter()
        .find(|(id, _)| *id == moved_id)
        .expect("옮길 탭의 기하")
        .1;

    // 목적 창의 **첫 탭 왼쪽 절반** 위에 커서를 세운다 → 맨 앞(경계 0).
    let drop_at = target
        .update(&mut vcx, |this, _, cx| {
            let first = this.terminal().read(cx).tab_bounds()[0].1;
            point(
                px(1400.0) + first.origin.x + first.size.width * 0.25,
                px(60.0) + first.center().y,
            )
        })
        .unwrap();
    vcx.update(|_, cx| cx.set_global(sshub::displays::CursorOverride(drop_at)));

    vcx.simulate_mouse_down(grab.center(), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(
        point(grab.center().x + px(8.0), grab.center().y),
        MouseButton::Left,
        Modifiers::none(),
    );
    // 창 밖으로 나간 좌표 — 실제 판정은 위 CursorOverride가 맡는다.
    vcx.simulate_mouse_move(point(px(2000.0), px(15.0)), MouseButton::Left, Modifiers::none());

    let caret = target
        .update(&mut vcx, |this, _, cx| this.terminal().read(cx).tab_insert())
        .unwrap();
    assert_eq!(caret, Some(0), "목적 창의 탭바에 캐럿이 서야 한다");

    vcx.simulate_mouse_up(point(px(2000.0), px(15.0)), MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();

    target
        .update(&mut vcx, |this, _, cx| {
            let terminal = this.terminal().read(cx);
            let first = &terminal.tabs()[0];
            assert_eq!(
                sshub_splits::leaves(&first.root)[0].session_id,
                moved_session,
                "캐럿이 섰던 자리(맨 앞)에 그대로 꽂혀야 한다",
            );
            assert_eq!(terminal.tab_insert(), None, "드롭 후 캐럿은 사라진다");
        })
        .unwrap();
}
