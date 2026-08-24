# 앱 규칙 (Rust · GPUI)

## MUST

- 레이어를 지킨다: 순수 로직은 `sshub-core`/`sshub-splits`, 터미널 모델은
  `sshub-terminal`, GPUI 코드는 `sshub`.
- 모든 alacritty import는 `sshub-terminal/src/backend.rs`에만 둔다(seam).
  upstream이 깨지면 이 파일만 고쳐 Zed 포크로 스왑한다.
- 위젯 키 바인딩은 `ui::init` 한 곳에서 등록한다. `clear_key_bindings()`가
  전역 키맵을 비우므로 리바인딩 경로는
  `clear_key_bindings` → `ui::init` → `keymap::register_all` 순서를 지킨다.
- 액션은 포커스된 요소의 디스패치 경로로 전달된다. 화면을 바꿀 때 포커스도
  함께 옮긴다 — 안 그러면 단축키가 조용히 죽는다.
- 엔티티 재진입을 만들지 않는다. 어떤 콜백이 워크스페이스를 다시 `update`할
  수 있으면 `cx.defer`로 미룬다 (ObjC 콜백 안의 panic은 그대로 abort다).
- 터미널 렌더링에서 와이드 문자는 셀마다 그린다(배치로 묶지 않는다).
- 조합 중(marked) 텍스트는 절대 PTY로 보내지 않는다.

## MUST NOT

- 뷰가 `sshub-core`의 파일을 직접 열지 않는다. `AppState`/`Store`를 통한다.
- 오래 걸리는 코어 호출(ssh-keygen·scrypt·lsof)을 메인 스레드에서 하지 않는다
  (`AppState::spawn_core`).
- 콘텐츠 위에 보이지 않는 전체 오버레이를 얹지 않는다 — 클릭을 삼킨다
  (창 드래그 스트립이 탭 바를 먹은 적 있다).

## SHOULD

- 순수 계산은 `#[cfg(test)]`로 검증 가능한 함수로 뽑는다.
- 주석은 "왜"를 설명한다.
