# 백엔드 규칙 (Electron 메인 프로세스 · TypeScript)

## MUST

- IPC 커맨드는 `electron/main.ts`의 단일 `invoke` 채널 switch에 등록하고,
  프론트 래퍼 함수(`src/lib/commands.ts`)를 함께 추가한다.
- 커맨드 에러는 throw로 전파한다(renderer에서 rejected Promise로 수신).
  사용자 메시지는 무엇을 어떻게 고칠지 알려준다.
- 영속 데이터는 `electron/store.ts`를 통해서만 읽고 쓰며, 저장은
  원자적(tmp+rename)·0600을 유지한다.
- 모델에 새 옵션 필드를 추가할 때는 optional(`?`) + 읽기 시 기본값 보정으로
  구버전 `sshub.json` 역호환을 보장한다.
- ssh 실행 시 인증 방식별 옵션을 정확히 지정한다(`electron/lib/ssh.ts`
  `buildSshArgs`): `password` → `PubkeyAuthentication=no`, `key`/`pem` →
  `-i <path>` + `IdentitiesOnly=yes`, `agent` →
  `PreferredAuthentications=publickey`.
- PTY 세션은 메인 프로세스의 세션 맵이 단독 소유하고, 세션 id를 이벤트 채널
  접미사로 사용한다.
- preload(`electron/preload.ts`)는 `window.electronAPI.{invoke,on}`만 노출한다.
  Node API를 renderer에 직접 노출하지 않는다.
- BrowserWindow는 `contextIsolation: true`, `nodeIntegration: false`를 유지한다.

## MUST NOT

- `electron/store.ts` 외의 모듈이 `sshub.json`을 직접 조작하지 않는다.
- renderer 입력(경로·파일명 등)을 검증 없이 fs/spawn에 넘기지 않는다.

## SHOULD

- 비즈니스 로직은 가능한 한 `electron/lib/` 순수 함수로 분리하고 동일 디렉터리에
  `*.test.ts` 단위 테스트를 작성한다.

## MAY

- 결정 배경이 필요하면 `docs/DEVELOPMENT.md`를 참조한다.
