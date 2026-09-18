//! ⌘W(closePane 액션)와 탭바 X 버튼의 동작 일치.
//!
//! 사용자 보고: 분할이 없는 탭에서 X를 누를 때와 ⌘W를 누를 때 결과가 달랐다.
//! X는 **보이는(활성) 탭**을 닫는데 ⌘W는 `focused_pane`이 가리키는 탭을 닫아,
//! 포커스 기록이 활성 탭과 어긋나면 다른 탭이 닫혔다. 이제 ⌘W의 대상은 활성
//! 탭이고, 분할이 없으면 X와 같은 `close_tab` 경로를 탄다. 분할된 탭에서는
//! 기존처럼 pane 하나만 닫는다.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 레이아웃을 덮어쓴다.

use gpui::{TestAppContext, VisualTestContext};
use sshub::terminal_workspace::TerminalWorkspace;
use sshub_splits::TabId;

fn setup(cx: &mut TestAppContext) -> (gpui::WindowHandle<TerminalWorkspace>, VisualTestContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.path().to_path_buf());
    // tempdir는 테스트가 끝날 때까지 살아 있어야 한다.
    Box::leak(Box::new(dir));
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });
    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let vcx = VisualTestContext::from_window(window.into(), cx);
    (window, vcx)
}

fn tab_ids(ws: &TerminalWorkspace) -> Vec<TabId> {
    ws.tabs().iter().map(|t| t.id.clone()).collect()
}

/// 포커스 기록이 다른 탭을 가리키더라도 ⌘W는 X처럼 **활성 탭**을 닫아야 한다.
#[gpui::test]
fn cmd_w_closes_the_active_tab_even_if_focus_record_points_elsewhere(cx: &mut TestAppContext) {
    let (window, mut vcx) = setup(cx);
    vcx.run_until_parked();

    // 탭 A(처음), B(새로 만든 것, 활성).
    window.update(&mut vcx, |ws, window, cx| ws.new_tab(window, cx)).unwrap();
    vcx.run_until_parked();
    let ids = window.update(&mut vcx, |ws, _, _| tab_ids(ws)).unwrap();
    assert_eq!(ids.len(), 2);
    let (tab_a, tab_b) = (ids[0].clone(), ids[1].clone());

    // 포커스 기록을 A의 pane으로 어긋나게 만든다 — 활성 탭은 B인 채로.
    // (실사용에서는 탭 전환·창 이동 등 포커스가 따라오지 않은 경로에서 생긴다.)
    let pane_a = window
        .update(&mut vcx, |ws, _, _| {
            sshub_splits::leaves(&ws.tabs()[0].root)[0].session_id.clone()
        })
        .unwrap();
    window
        .update(&mut vcx, |ws, window, cx| {
            ws.focus_pane(pane_a, window, cx);
            ws.select_tab_at(Some(1), window, cx);
        })
        .unwrap();
    vcx.run_until_parked();
    let active = window.update(&mut vcx, |ws, _, _| ws.active_tab_id().cloned()).unwrap();
    assert_eq!(active.as_ref(), Some(&tab_b), "전제: 활성 탭은 B");

    // X 버튼과 같은 결과: 활성 탭 B가 사라지고 A만 남는다.
    window.update(&mut vcx, |ws, window, cx| ws.close_focused_pane(window, cx)).unwrap();
    vcx.run_until_parked();
    let ids = window.update(&mut vcx, |ws, _, _| tab_ids(ws)).unwrap();
    assert_eq!(ids, vec![tab_a], "⌘W는 X처럼 활성 탭을 닫아야 한다");
}

/// 분할이 없는 탭: ⌘W와 X는 같은 경로 — 확인 없이 그 탭이 사라진다.
#[gpui::test]
fn cmd_w_on_a_single_pane_tab_matches_the_x_button(cx: &mut TestAppContext) {
    let (window, mut vcx) = setup(cx);
    vcx.run_until_parked();
    window.update(&mut vcx, |ws, window, cx| ws.new_tab(window, cx)).unwrap();
    vcx.run_until_parked();
    let ids = window.update(&mut vcx, |ws, _, _| tab_ids(ws)).unwrap();
    assert_eq!(ids.len(), 2);

    vcx.simulate_keystrokes("cmd-w");
    vcx.run_until_parked();
    let after = window.update(&mut vcx, |ws, _, _| tab_ids(ws)).unwrap();
    assert_eq!(after, vec![ids[0].clone()], "활성 탭(두 번째)만 사라져야 한다");

    // 같은 상황에서 X를 눌렀을 때와 결과가 같은지 — 남은 탭을 X로 닫는다.
    window
        .update(&mut vcx, |ws, window, cx| ws.close_tab(after[0].clone(), window, cx))
        .unwrap();
    vcx.run_until_parked();
    assert!(window.update(&mut vcx, |ws, _, _| ws.tabs().is_empty()).unwrap());
}

/// 분할된 탭에서는 예전처럼 pane 하나만 닫는다 (확인 모달 → Enter).
#[gpui::test]
fn cmd_w_on_a_split_tab_closes_only_the_focused_pane(cx: &mut TestAppContext) {
    let (window, mut vcx) = setup(cx);
    vcx.run_until_parked();
    vcx.simulate_keystrokes("cmd-d");
    vcx.run_until_parked();
    let panes = window
        .update(&mut vcx, |ws, _, _| sshub_splits::leaves(&ws.tabs()[0].root).len())
        .unwrap();
    assert_eq!(panes, 2, "전제: 분할된 탭");

    vcx.simulate_keystrokes("cmd-w");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    let (tabs, panes) = window
        .update(&mut vcx, |ws, _, _| {
            (ws.tabs().len(), sshub_splits::leaves(&ws.tabs()[0].root).len())
        })
        .unwrap();
    assert_eq!((tabs, panes), (1, 1), "탭은 남고 pane 하나만 닫혀야 한다");
}
