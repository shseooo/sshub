//! `Terminal` 엔티티 end-to-end: 실제 셸을 띄우고, 키를 넣고, 그리드 스냅샷에
//! 결과가 나타나는지 확인한다. 이벤트 펌프(배치 드레인 → notify)까지 함께
//! 검증되는 유일한 테스트라 GUI 없이도 "터미널이 실제로 동작한다"를 보증한다.
//!
//! 주의: gpui 테스트 executor의 타이머는 **가상 시계**라 즉시 만료된다. 자식
//! 프로세스는 진짜 시간이 필요하므로 `thread::sleep` + `run_until_parked()`로
//! 실시간을 흘려보내며 펌프를 돌린다.

use std::time::Duration;

use gpui::{AppContext as _, Entity, TestAppContext};
use sshub_terminal::backend::Flags;
use sshub_terminal::{SpawnSpec, Terminal, TerminalBuilder};

/// 스냅샷을 갱신하고 화면 텍스트를 뽑는다. 와이드 문자의 스페이서 셀은 건너뛴다
/// (렌더러도 같은 규칙 — 안 그러면 '한글'이 '한 글'로 보인다).
fn screen_text(terminal: &Entity<Terminal>, cx: &mut TestAppContext) -> String {
    terminal.update(cx, |t, cx| {
        t.sync(cx);
        t.last_content
            .cells
            .iter()
            .filter(|c| !c.cell.flags.contains(Flags::WIDE_CHAR_SPACER))
            .map(|c| c.cell.c)
            .collect()
    })
}

fn wait_for(terminal: &Entity<Terminal>, cx: &mut TestAppContext, needle: &str) -> String {
    for _ in 0..100 {
        cx.run_until_parked();
        let text = screen_text(terminal, cx);
        if text.contains(needle) {
            return text;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cx.run_until_parked();
    screen_text(terminal, cx)
}

fn spawn_shell(cx: &mut TestAppContext, restored: Option<String>) -> Entity<Terminal> {
    // -f: 사용자 zsh 설정을 읽지 않는다 (프롬프트 커스터마이즈가 테스트를 흔들지 않게).
    let mut spec = SpawnSpec::local_shell(None);
    spec.program = "/bin/zsh".into();
    spec.args = vec!["-f".into()];
    spec.restored_scrollback = restored;
    cx.update(|cx| cx.new(|cx| TerminalBuilder::new(spec).expect("PTY spawn").subscribe(cx)))
}

#[gpui::test]
fn runs_a_command_and_shows_its_output(cx: &mut TestAppContext) {
    let terminal = spawn_shell(cx, None);
    terminal.update(cx, |t, _| t.input(b"echo sshub_e2e_$((40+2))\r".to_vec()));

    let text = wait_for(&terminal, cx, "sshub_e2e_42");
    assert!(text.contains("sshub_e2e_42"), "셸 출력이 그리드에 없음:\n{text}");
}

#[gpui::test]
fn typed_keystrokes_reach_the_pty(cx: &mut TestAppContext) {
    let terminal = spawn_shell(cx, None);
    // 키스트로크 경로(to_esc_bytes)로 한 글자씩 — 스페이스 포함.
    terminal.update(cx, |t, _| {
        for key in ["e", "c", "h", "o", "space", "h", "i", "enter"] {
            let ks = gpui::Keystroke::parse(key).unwrap();
            assert!(t.try_keystroke(&ks, true), "{key} 매핑 실패");
        }
    });

    let text = wait_for(&terminal, cx, "hi");
    assert!(text.contains("echo hi"), "스페이스가 빠졌거나 입력이 안 감:\n{text}");
}

#[gpui::test]
fn cjk_output_keeps_its_cells(cx: &mut TestAppContext) {
    let terminal = spawn_shell(cx, None);
    terminal.update(cx, |t, _| t.input("echo 한글_漢字_ok\r".as_bytes().to_vec()));

    let text = wait_for(&terminal, cx, "한글_漢字_ok");
    assert!(text.contains("한글_漢字_ok"), "CJK 출력이 깨짐:\n{text}");
}

#[gpui::test]
fn locally_injected_bytes_never_reach_the_shell(cx: &mut TestAppContext) {
    // 배너·스크롤백 복원이 쓰는 경로 — 화면에는 보이지만 셸은 모른다.
    let terminal = spawn_shell(cx, None);
    terminal.update(cx, |t, _| t.inject_local(b"INJECTED_BANNER\r\n"));

    let text = wait_for(&terminal, cx, "INJECTED_BANNER");
    assert!(text.contains("INJECTED_BANNER"), "주입 텍스트가 화면에 없음");

    // 셸이 이 바이트를 명령으로 받았다면 "command not found"가 떴을 것이다.
    terminal.update(cx, |t, _| t.input(b"echo probe_done\r".to_vec()));
    let text = wait_for(&terminal, cx, "probe_done");
    assert!(!text.contains("command not found"), "주입 바이트가 PTY로 새어나감:\n{text}");
}

#[gpui::test]
fn scrollback_survives_a_serialize_restore_round_trip(cx: &mut TestAppContext) {
    let terminal = spawn_shell(cx, None);
    terminal.update(cx, |t, _| t.input(b"echo restore_me_123\r".to_vec()));
    let live = wait_for(&terminal, cx, "restore_me_123");
    assert!(live.contains("restore_me_123"), "원본 출력이 없음:\n{live}");

    let saved = terminal.update(cx, |t, _| t.serialize_scrollback(1000));
    assert!(saved.contains("restore_me_123"), "직렬화 결과에 출력이 없음");

    // 새 터미널에 복원 주입 → 같은 내용이 보여야 한다.
    let revived = spawn_shell(cx, Some(saved));
    let text = wait_for(&revived, cx, "restore_me_123");
    assert!(text.contains("restore_me_123"), "복원된 스크롤백이 없음:\n{text}");
}
