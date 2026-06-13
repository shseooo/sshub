# 백엔드 규칙 (Rust · Tauri v2)

## MUST

- IPC 커맨드는 `src-tauri/src/commands/<도메인>.rs`에 두고 `lib.rs`의
  `invoke_handler`에 등록한다.
- 커맨드는 프론트에 `Result<T, String>`을 반환하고, 에러는 `?`로 전파하며 락/IO
  에러는 `.map_err(|e| e.to_string())`로 변환한다.
- 영속 데이터는 `store.rs`를 통해서만 읽고 쓰며, 저장은 원자적(tmp+rename)·0600을
  유지한다.
- 모델에 새 옵션 필드를 추가할 때 `#[serde(default)]`를 붙여 구버전 `sshub.json`
  역호환을 보장한다.
- ssh 실행 시 인증 방식별 옵션을 정확히 지정한다: `password` →
  `PubkeyAuthentication=no`, `key`/`pem` → `-i <path>` + `IdentitiesOnly=yes`,
  `agent` → `PreferredAuthentications=publickey`.
- PTY 세션은 `TerminalSessions` 맵이 단독 소유하고, 세션 id를 이벤트 채널 접미사로
  사용한다.

## MUST NOT

- 프로덕션 경로에서 `unwrap()`/`expect()`를 사용하지 않는다.
- `store.rs` 외의 모듈이 `sshub.json`을 직접 조작하지 않는다.

## SHOULD

- 비즈니스 로직은 가능한 한 순수 함수로 분리하고 `#[cfg(test)]` 단위 테스트를 작성한다.
- 사용자 메시지는 무엇을 어떻게 고칠지 알려준다.

## MAY

- 테스트 코드에서는 `unwrap()`/`expect()`를 사용한다.
