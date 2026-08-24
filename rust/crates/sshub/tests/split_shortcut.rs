//! ⌘D 분할이 키맵 등록 → 액션 처리 → 세션 기동까지 실제로 이어지는지 확인한다.
//! 사용자가 "분할이 안 된다"고 보고한 경로의 회귀 테스트.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 레이아웃/서버 파일을
//! 덮어쓴다(실제로 한 번 그랬다).

use gpui::{TestAppContext, VisualTestContext};
use sshub::terminal_workspace::TerminalWorkspace;
use sshub_splits::SessionId;

fn active_leaves(ws: &TerminalWorkspace) -> Vec<SessionId> {
    ws.tabs()
        .iter()
        .find(|t| Some(&t.id) == ws.active_tab_id())
        .map(|t| {
            sshub_splits::leaves(&t.root)
                .into_iter()
                .map(|l| l.session_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[gpui::test]
fn cmd_d_splits_the_focused_pane_and_starts_a_shell(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.path().to_path_buf());

    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });

    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    let before = window.update(&mut vcx, |ws, _, _| active_leaves(ws)).unwrap();
    assert_eq!(before.len(), 1, "새 워크스페이스는 pane 하나로 시작한다");

    vcx.simulate_keystrokes("cmd-d");
    vcx.run_until_parked();

    let after = window.update(&mut vcx, |ws, _, _| active_leaves(ws)).unwrap();
    assert_eq!(after.len(), 2, "⌘D가 pane을 좌우로 나눠야 한다");

    // 모델만 갈라지고 세션이 안 뜨면 화면은 빈 칸으로 남는다.
    let live = vcx.update(|_, cx| {
        let registry = sshub::session_registry::registry(cx);
        let registry = registry.read(cx);
        after.iter().filter(|id| registry.is_live(id)).count()
    });
    assert_eq!(live, 2, "분할된 pane마다 셸이 떠 있어야 한다");

    // 화면 배치까지 실제로 갈라졌는지 — 모델만 나뉘고 렌더가 한 칸이면 사용자
    // 눈에는 "분할이 안 된 것"이다.
    let rects = window
        .update(&mut vcx, |ws, _, _| ws.pane_bounds())
        .unwrap();
    assert_eq!(rects.len(), 2, "pane 두 개가 각자 영역을 가져야 한다");
    let (a, b) = (rects[0].1, rects[1].1);
    assert!(f32::from(a.size.width) > 1.0 && f32::from(b.size.width) > 1.0, "폭이 0인 pane: {a:?} {b:?}");
    let overlap = a.origin.x.max(b.origin.x) < (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    assert!(!overlap, "좌우 분할인데 영역이 겹친다: {a:?} {b:?}");

    vcx.simulate_keystrokes("cmd-shift-d");
    vcx.run_until_parked();
    let after_down = window.update(&mut vcx, |ws, _, _| active_leaves(ws)).unwrap();
    assert_eq!(after_down.len(), 3, "⇧⌘D는 위아래로 한 번 더 나눈다");
}

/// 탭을 다른 탭의 pane 위로 끌어다 놓으면 그 자리에서 분할되어야 한다.
/// (사용자 보고: "터미널 탭을 드래그해서 다른 터미널의 분할로 넣기"가 안 됨.
///  원인은 탭을 누르는 순간 활성화돼 자기 자신에게 병합되던 것 — 선택을 클릭
///  시점으로 옮겨 고쳤고, 모델 쪽 동작을 여기서 고정한다.)
#[gpui::test]
fn dragging_a_tab_onto_another_tabs_pane_merges_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.path().to_path_buf());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });

    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    // 탭 2개: A(처음), B(새로 만든 것 — 만들면 B가 활성이 된다)
    window.update(&mut vcx, |ws, window, cx| ws.new_tab(window, cx)).unwrap();
    vcx.run_until_parked();

    let (tab_a, tab_b, pane_a) = window
        .update(&mut vcx, |ws, _, _| {
            let a = ws.tabs()[0].clone();
            let b = ws.tabs()[1].id.clone();
            let pane = sshub_splits::leaves(&a.root)[0].session_id.clone();
            (a.id, b, pane)
        })
        .unwrap();
    assert_ne!(tab_a, tab_b);

    // B를 A의 pane 위로 드롭
    window
        .update(&mut vcx, |ws, window, cx| {
            ws.drop_on_pane(
                pane_a.clone(),
                Some(sshub::split_view::TabDrag { tab_id: tab_b.clone() }),
                None,
                window,
                cx,
            )
        })
        .unwrap();
    vcx.run_until_parked();

    let (tab_count, leaves_in_a) = window
        .update(&mut vcx, |ws, _, _| {
            let leaves = ws
                .tabs()
                .iter()
                .find(|t| t.id == tab_a)
                .map(|t| sshub_splits::leaves(&t.root).len())
                .unwrap_or(0);
            (ws.tabs().len(), leaves)
        })
        .unwrap();

    assert_eq!(tab_count, 1, "옮겨온 탭은 사라져야 한다");
    assert_eq!(leaves_in_a, 2, "받는 탭이 둘로 분할돼야 한다");
}

/// ⌘1..⌘9 탭 이동. ⌘9만 "9번째"가 아니라 **마지막 탭**이다(macOS/Chrome/Zed 관례).
#[gpui::test]
fn cmd_digit_jumps_to_that_tab_and_cmd_9_to_the_last(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.path().to_path_buf());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });

    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    // 탭 3개 (새 워크스페이스는 1개로 시작한다).
    for _ in 0..2 {
        window.update(&mut vcx, |ws, window, cx| ws.new_tab(window, cx)).unwrap();
    }
    vcx.run_until_parked();

    let ids = window
        .update(&mut vcx, |ws, _, _| {
            ws.tabs().iter().map(|t| t.id.clone()).collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(ids.len(), 3);

    vcx.simulate_keystrokes("cmd-2");
    vcx.run_until_parked();
    let active = window.update(&mut vcx, |ws, _, _| ws.active_tab_id().cloned()).unwrap();
    assert_eq!(active.as_ref(), Some(&ids[1]), "⌘2는 두 번째 탭");

    // 탭만 바뀌고 포커스가 따라오지 않으면 키 입력이 안 보이는 pane으로 흘러간다.
    let focused = window.update(&mut vcx, |ws, _, _| ws.focused_pane().cloned()).unwrap();
    let second_panes = window
        .update(&mut vcx, |ws, _, _| {
            sshub_splits::leaves(&ws.tabs()[1].root)
                .into_iter()
                .map(|l| l.session_id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert!(
        focused.is_some_and(|f| second_panes.contains(&f)),
        "선택된 탭의 pane이 포커스를 받아야 한다"
    );

    vcx.simulate_keystrokes("cmd-9");
    vcx.run_until_parked();
    let active = window.update(&mut vcx, |ws, _, _| ws.active_tab_id().cloned()).unwrap();
    assert_eq!(active.as_ref(), Some(&ids[2]), "⌘9는 9번째가 아니라 마지막 탭");

    vcx.simulate_keystrokes("cmd-1");
    vcx.run_until_parked();
    let active = window.update(&mut vcx, |ws, _, _| ws.active_tab_id().cloned()).unwrap();
    assert_eq!(active.as_ref(), Some(&ids[0]), "⌘1은 첫 탭");

    // 없는 번호는 아무 일도 하지 않는다.
    vcx.simulate_keystrokes("cmd-7");
    vcx.run_until_parked();
    let active = window.update(&mut vcx, |ws, _, _| ws.active_tab_id().cloned()).unwrap();
    assert_eq!(active.as_ref(), Some(&ids[0]), "탭이 없으면 그대로");
}

/// 워크스페이스 update 안에서 붙여넣기를 호출해도 죽지 않아야 한다.
///
/// 컨텍스트 메뉴의 "붙여넣기"가 정확히 이 형태였다. 붙여넣기는 브로드캐스트를
/// 발생시키고 그 싱크가 워크스페이스를 다시 update하는데, 이미 update 중이면
/// gpui가 재진입 panic을 낸다. ObjC 마우스 콜백 안에서 터지면 unwind가 불가능해
/// 앱이 그대로 abort된다(사용자 크래시 리포트: panic_cannot_unwind).
#[gpui::test]
fn pasting_from_inside_a_workspace_update_does_not_abort(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.path().to_path_buf());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("붙여넣기 테스트".into()));
    });

    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    let session = window
        .update(&mut vcx, |ws, _, _| active_leaves(ws)[0].clone())
        .unwrap();

    // 메뉴 콜백과 같은 중첩: 워크스페이스를 대여한 채 뷰의 paste를 호출한다.
    window
        .update(&mut vcx, |ws, _, cx| {
            let view = ws.pane_view(&session).expect("pane 뷰");
            view.update(cx, |view, cx| view.paste(cx));
        })
        .unwrap();
    vcx.run_until_parked();

    // 여기까지 왔으면 재진입 panic 없이 살아남은 것. 세션도 멀쩡해야 한다.
    let live = vcx.update(|_, cx| {
        sshub::session_registry::registry(cx).read(cx).is_live(&session)
    });
    assert!(live, "붙여넣기 후 세션이 죽었다");
}
