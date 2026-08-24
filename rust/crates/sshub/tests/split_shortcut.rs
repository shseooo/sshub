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
