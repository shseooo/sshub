//! 창이 둘일 때의 복원 불변식: **한 창을 만든다고 다른 창의 파일을 지우지 않는다.**
//!
//! 회귀: `TerminalWorkspace::new`가 자기 창의 세션 id만으로 스크롤백/cwd를
//! prune했다. 시작 시 창을 순서대로 복원하므로, 먼저 만들어진 창이 아직
//! 만들어지지 않은 창의 파일을 전부 지웠고 → 두 번째 창은 pwd(그리고 히스토리)를
//! 잃은 채 홈에서 열렸다. 정리는 창을 전부 연 뒤 `main`이 한 번만 한다.
//!
//! 임시 디렉터리를 쓴다 — 기본 경로로 돌리면 사용자의 실제 데이터를 건드린다.

use std::fs;

use gpui::{TestAppContext, VisualTestContext};
use sshub::terminal_workspace::TerminalWorkspace;

/// 다른 창(아직 복원되지 않은 창)이 남겨 둔 세션 id.
const OTHER_WINDOW_SESSION: &str = "session-of-the-other-window";

fn boot(cx: &mut TestAppContext) -> sshub_core::AppPaths {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = sshub_core::AppPaths::in_dir(dir.keep());

    // 지난 실행이 남긴 상태를 흉내 낸다 — 레지스트리가 읽기 전에 깔아 둔다.
    fs::create_dir_all(&paths.app_data).unwrap();
    fs::create_dir_all(&paths.scrollback_dir).unwrap();
    fs::write(
        &paths.terminal_cwd_file,
        format!(r#"{{"{OTHER_WINDOW_SESSION}":"/tmp"}}"#),
    )
    .unwrap();
    fs::write(
        paths.scrollback_dir.join(format!("{OTHER_WINDOW_SESSION}.txt")),
        "이전 실행의 출력\n",
    )
    .unwrap();

    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
        sshub::theme::init(cx);
        sshub::ui::init(cx);
        sshub::keymap::register_all(cx, &sshub_core::settings::default_shortcuts());
        sshub::session_registry::init(&paths, cx);
    });
    paths
}

#[gpui::test]
fn opening_a_window_keeps_another_windows_saved_cwd_and_scrollback(cx: &mut TestAppContext) {
    let paths = boot(cx);

    let window = cx.add_window(|window, cx| TerminalWorkspace::new(window, cx));
    VisualTestContext::from_window(window.into(), cx).run_until_parked();

    let cwds = fs::read_to_string(&paths.terminal_cwd_file).unwrap_or_default();
    assert!(
        cwds.contains(OTHER_WINDOW_SESSION),
        "다른 창의 저장된 cwd가 사라졌다 — 그 창은 홈에서 열린다: {cwds:?}"
    );
    assert!(
        paths.scrollback_dir.join(format!("{OTHER_WINDOW_SESSION}.txt")).exists(),
        "다른 창의 스크롤백 파일이 사라졌다"
    );
}
