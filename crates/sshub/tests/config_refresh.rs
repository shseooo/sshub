//! 앱 밖에서 `~/.ssh/config`를 고쳤을 때 앱이 알아채는지 (Phase 3-B).
//!
//! 임시 디렉터리 안에서만 논다 — `AppPaths::in_dir`는 config까지 그 안으로
//! 가둔다. 기본 경로로 돌리면 사용자의 진짜 `~/.ssh/config`를 건드린다.

use std::fs;

use gpui::TestAppContext;
use sshub_core::AppPaths;

fn setup(cx: &mut TestAppContext, config: &str) -> (tempfile::TempDir, AppPaths) {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let paths = AppPaths::in_dir(dir.path().to_path_buf());
    fs::create_dir_all(paths.ssh_config_file.parent().unwrap()).unwrap();
    fs::write(&paths.ssh_config_file, config).unwrap();
    cx.update(|cx| {
        sshub::state::init_with_paths(paths.clone(), cx);
    });
    (dir, paths)
}

fn server_names(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let state = sshub::state::app_state(cx);
        let names: Vec<String> =
            state.read(cx).servers.iter().map(|s| s.name.clone()).collect();
        names
    })
}

fn refresh(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        let state = sshub::state::app_state(cx);
        state.update(cx, |state, cx| state.refresh_from_disk(cx))
    })
}

#[gpui::test]
fn picks_up_a_host_added_by_hand_without_rewriting_the_file(cx: &mut TestAppContext) {
    let (_dir, paths) = setup(cx, "Host web\n  HostName 1.1.1.1\n");
    assert_eq!(server_names(cx), ["web"]);

    let edited = "Host web\n  HostName 1.1.1.1\n\nHost added-by-hand\n  HostName 2.2.2.2\n";
    fs::write(&paths.ssh_config_file, edited).unwrap();

    assert!(refresh(cx), "외부 편집을 감지해야 한다");
    assert_eq!(server_names(cx), ["added-by-hand", "web"]);
    // 사용자가 방금 저장한 파일을 앱이 되받아쓰면 그게 곧 손실이다.
    assert_eq!(fs::read_to_string(&paths.ssh_config_file).unwrap(), edited);
}

#[gpui::test]
fn reports_no_change_when_the_file_did_not_move(cx: &mut TestAppContext) {
    let (_dir, _paths) = setup(cx, "Host web\n  HostName 1.1.1.1\n");
    assert!(!refresh(cx));
    assert!(!refresh(cx));
    assert_eq!(server_names(cx), ["web"]);
}

#[gpui::test]
fn hand_written_multi_pattern_hosts_arrive_as_read_only_entries(cx: &mut TestAppContext) {
    let (_dir, paths) = setup(cx, "Host web\n  HostName 1.1.1.1\n");
    fs::write(
        &paths.ssh_config_file,
        "Host web\n  HostName 1.1.1.1\n\nHost a b\n  User multi\n\nHost *\n  ForwardAgent yes\n",
    )
    .unwrap();

    assert!(refresh(cx));
    let flags: Vec<(String, bool)> = cx.update(|cx| {
        let state = sshub::state::app_state(cx);
        let out: Vec<(String, bool)> = state
            .read(cx)
            .servers
            .iter()
            .map(|s| (s.name.clone(), s.read_only))
            .collect();
        out
    });
    assert_eq!(
        flags,
        [
            ("a".to_string(), true),
            ("b".to_string(), true),
            ("web".to_string(), false),
        ],
        "와일드카드 블록은 목록에 뜨지 않는다"
    );
}
