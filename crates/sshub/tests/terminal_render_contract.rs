//! `TerminalElement::prepaint`가 **프레임마다** 하는 일의 계약.
//!
//! 렌더 결과 자체는 단언하지 않는다(gpui 테스트 플랫폼은 `NoopTextSystem`이다).
//! 대신 prepaint가 남기는 **관찰 가능한 부작용**을 고정한다:
//!   - PTY 크기를 화면에 맞춘다 (`set_size`)
//!   - 처음 실제로 레이아웃되면 `hydrated`를 세운다 — 이 플래그가 서야 스크롤백이
//!     저장된다(§7 no-clobber 게이트)
//!   - 그 프레임의 화면 스냅샷(`last_content`)이 PTY가 쓴 내용을 담는다
//!
//! 화면 스냅샷을 **복제 대신 빌려 쓰는** 최적화가 이 계약을 깨지 않아야 한다.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 레이아웃 파일을 건드린다.

use gpui::{px, size, TestAppContext, VisualTestContext};
use sshub::terminal_workspace::TerminalWorkspace;
use sshub_splits::SessionId;
use sshub_terminal::backend::Flags;
use sshub_terminal::Terminal;

fn boot(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.keep());
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });
}

fn terminal_of(session: &SessionId, cx: &mut VisualTestContext) -> gpui::Entity<Terminal> {
    cx.update(|_, cx| {
        sshub::session_registry::registry(cx)
            .read(cx)
            .get(session)
            .expect("살아 있는 세션")
    })
}

/// 프레임을 한 장 그리게 한다.
///
/// `inject_local`은 그리드에만 쓰고 알림을 쏘지 않는다(PTY 읽기가 아니다).
/// 스냅샷은 prepaint에서만 갱신되므로, 뷰를 dirty로 만들어 실제로 한 프레임
/// 돌려야 "그 프레임의 스냅샷"을 볼 수 있다.
fn draw_a_frame(window: gpui::WindowHandle<TerminalWorkspace>, cx: &mut VisualTestContext) {
    window.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.run_until_parked();
}

/// 스냅샷의 화면 텍스트. 와이드 문자의 스페이서 셀은 렌더러와 같은 규칙으로 건너뛴다.
fn snapshot_text(terminal: &gpui::Entity<Terminal>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        terminal
            .read(cx)
            .last_content
            .cells
            .iter()
            .filter(|c| !c.cell.flags.contains(Flags::WIDE_CHAR_SPACER))
            .map(|c| c.cell.c)
            .collect()
    })
}

#[gpui::test]
fn a_painted_frame_sizes_the_pty_and_marks_it_hydrated(cx: &mut TestAppContext) {
    boot(cx);
    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    let session = window
        .update(&mut vcx, |ws, _, _| ws.focused_pane().cloned())
        .unwrap()
        .expect("새 워크스페이스는 pane 하나로 시작한다");
    let terminal = terminal_of(&session, &mut vcx);

    let (hydrated, bounds) = vcx.update(|_, cx| {
        let t = terminal.read(cx);
        (t.hydrated, t.bounds())
    });
    assert!(
        hydrated,
        "한 프레임이라도 그려졌으면 hydrated — 이 게이트가 서야 스크롤백이 저장된다"
    );
    assert!(
        bounds.columns() > 1 && bounds.screen_lines() > 1,
        "prepaint가 PTY 크기를 화면에 맞춰야 한다 (실측 {}x{})",
        bounds.columns(),
        bounds.screen_lines(),
    );

    // 창을 줄이면 다음 프레임에서 PTY도 따라 줄어야 한다.
    let before = bounds.screen_lines();
    vcx.simulate_resize(size(px(900.), px(400.)));
    vcx.run_until_parked();
    let after = vcx.update(|_, cx| terminal.read(cx).bounds().screen_lines());
    assert!(
        after < before,
        "창이 작아졌는데 PTY 행 수가 그대로면 set_size가 안 걸린 것이다 ({before} → {after})",
    );
}

#[gpui::test]
fn the_frame_snapshot_carries_what_the_pty_wrote(cx: &mut TestAppContext) {
    boot(cx);
    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    let session = window
        .update(&mut vcx, |ws, _, _| ws.focused_pane().cloned())
        .unwrap()
        .expect("pane");
    let terminal = terminal_of(&session, &mut vcx);

    // 셸을 거치지 않고 그리드에 직접 넣는다 — 프롬프트 모양에 흔들리지 않는다.
    // 한글을 섞어 와이드 문자 경로까지 스냅샷에 실리는지 함께 본다.
    vcx.update(|_, cx| {
        terminal.update(cx, |t, _| {
            t.inject_local(b"\x1b[2J\x1b[Hsshub-\xed\x95\x9c\xea\xb8\x80-42")
        });
    });
    draw_a_frame(window, &mut vcx);

    let text = snapshot_text(&terminal, &mut vcx);
    assert!(
        text.contains("sshub-한글-42"),
        "그 프레임의 스냅샷에 PTY가 쓴 내용이 있어야 한다: {:?}",
        text.trim_end_matches(' '),
    );
}

#[gpui::test]
fn every_visible_pane_gets_its_own_sized_snapshot(cx: &mut TestAppContext) {
    // 분할이 있으면 pane마다 prepaint가 돈다. 스냅샷을 공유하거나 빌린 참조가
    // 엉키면 여기서 드러난다 — 두 pane이 서로 다른 내용을 들고 있어야 한다.
    boot(cx);
    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();

    vcx.simulate_keystrokes("cmd-d");
    vcx.run_until_parked();

    let sessions: Vec<SessionId> = window
        .update(&mut vcx, |ws, _, _| {
            let tab = ws
                .tabs()
                .iter()
                .find(|t| Some(&t.id) == ws.active_tab_id())
                .expect("활성 탭");
            sshub_splits::leaves(&tab.root)
                .into_iter()
                .map(|l| l.session_id.clone())
                .collect()
        })
        .unwrap();
    assert_eq!(sessions.len(), 2, "⌘D로 pane 둘");

    let left = terminal_of(&sessions[0], &mut vcx);
    let right = terminal_of(&sessions[1], &mut vcx);
    vcx.update(|_, cx| {
        left.update(cx, |t, _| t.inject_local(b"\x1b[2J\x1b[HLEFT-PANE"));
        right.update(cx, |t, _| t.inject_local(b"\x1b[2J\x1b[HRIGHT-PANE"));
    });
    draw_a_frame(window, &mut vcx);

    let left_text = snapshot_text(&left, &mut vcx);
    let right_text = snapshot_text(&right, &mut vcx);
    assert!(left_text.contains("LEFT-PANE"), "왼쪽 스냅샷: {left_text:?}");
    assert!(right_text.contains("RIGHT-PANE"), "오른쪽 스냅샷: {right_text:?}");
    assert!(
        !left_text.contains("RIGHT-PANE"),
        "pane끼리 스냅샷이 섞이면 안 된다: {left_text:?}",
    );

    // 나란히 놓였으니 각자 창보다 좁다 — pane마다 제 크기로 set_size가 걸린다.
    let (lw, rw) = vcx.update(|_, cx| {
        (
            left.read(cx).bounds().columns(),
            right.read(cx).bounds().columns(),
        )
    });
    assert!(lw > 1 && rw > 1, "두 pane 모두 크기가 잡혀야 한다 ({lw}, {rw})");
}
