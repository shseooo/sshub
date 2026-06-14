# CLAUDE.md

Claude Code 가이드. 작업 전 이 파일과 `.claude/rules/*`, 상세 배경은
[`docs/DEVELOPMENT.md`]를 읽는다.

## 프로젝트

**sshub** — `~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 기기 간 동기화, 다국어·테마.
스택: Tauri v2 (Rust) · React 19 + TypeScript · Vite 8 · Tailwind v4 · @xterm/xterm.
로컬 디렉터리명은 `connectunnel`이지만 제품/리포명은 **sshub**.

## 명령어

```bash
bun install
bun run tauri dev          # 개발 (vite dev + Rust 핫리로드)
bun run build              # 프론트 빌드/타입체크 (tsc && vite build)
bun run test               # 프론트 테스트 (Vitest)
bun run test:watch
bun run tauri build        # 프로덕션 번들
cd src-tauri && cargo check   # Rust 타입체크
cd src-tauri && cargo test    # Rust 테스트
```

## 구조

- `src/pages/` 화면, `src/components/` 재사용 UI(Sidebar, TerminalHost=분할터미널, Select),
  `src/contexts/` 전역상태(Terminal/Language/Shortcuts/Theme), `src/hooks/` Query 래퍼,
  `src/lib/` 순수로직+IPC래퍼(`tauriCommands`/`shortcuts`/`theme`), `src/i18n/`(ko/en/ja),
  `src/types/`.
- `src-tauri/src/`: `lib.rs`(엔트리·커맨드등록), `store.rs`(JSON 저장소, 원자적쓰기·0600),
  `models.rs`(serde camelCase), `commands/`(server·key·ssh_config·terminal·backup).
- 데이터: `~/Library/Application Support/sshub.json`(비밀 없음), 개인키 `ssh_keys/`(0600),
  UI 설정은 localStorage.

## MUST

- 패키지 매니저는 bun 을 사용한다.
- 변경을 완료로 보고하기 전에 `bun run build`, `bun run test`,
  `cargo check && cargo test`를 모두 통과시킨다.
- Tauri 호출은 `src/lib/tauriCommands.ts` 래퍼를 통하고, 새 커맨드는 `lib.rs`
  `invoke_handler`에 등록한다.
- 사용자 문자열은 i18n 3종(ko/en/ja)에 동시에 추가한다.
- 드롭다운은 `@/components/Select`를 사용한다.
- 모델 새 옵션 필드에는 `#[serde(default)]`를 붙인다(구버전 `sshub.json` 역호환).
- 회귀 버그는 재현 테스트를 먼저 추가한다.
- 비밀(개인 키)은 `ssh_keys/` 0600 파일로만 존재시킨다.
- 사용자 입력 파일명은 `key_file_name`으로 새니타이즈한다(경로 traversal 차단).
- 내보내기는 비밀을 제거하고, 키 포함 시 cocoon passphrase 암호화를 적용한다.
- ssh 인증 옵션을 정확히 지정한다: password→`PubkeyAuthentication=no`,
  key/pem→`-i + IdentitiesOnly=yes`, agent→`PreferredAuthentications=publickey`.
- macOS 웹뷰 HTML5 드래그앤드롭을 쓰려면 `dragDropEnabled: false`를 지정한다.
- 빌드 후 macOS 실행을 위해 ad-hoc 서명을 적용한다:
  `codesign --force --deep --sign - src-tauri/target/release/bundle/macos/sshub.app`.
- 키 rename 시 개인 키 파일도 함께 이동한다(`update_ssh_key`).
- 탭 표시 이름은 `tab.name ?? 첫 leaf.label`을 따른다.

## MUST NOT

- `npm`/`yarn`을 사용하지 않는다.
- 컴포넌트/훅에서 `invoke`를 직접 호출하지 않는다.
- 네이티브 `<select>`를 새로 추가하지 않는다.
- 개인 키 평문을 JSON/로그/내보내기에 기록하지 않는다.
- 비밀번호를 저장하지 않는다.
- ad-hoc 서명에 hardened runtime 플래그를 함께 부여하지 않는다(GUI 실행 차단 위험).
- `tauri.conf.json`·`~/.ssh/config`의 사용자/린터 지정 값을 임의로 되돌리지 않는다.
- 대화형 `rm`/`git -i`를 사용하지 않는다.

## SHOULD

- 복잡한 순수 로직은 `src/lib/`/Rust 순수함수로 분리해 테스트 가능하게 만든다.
- 커밋 메시지는 한국어 요약을 사용하고, 파일 삭제는 `git rm`/`find -delete`를 쓴다.

## NOTE

- import 시 키 타입은 라벨일 뿐 접속에 영향이 없다. 생성 시에는 의미가 있다
  (ssh-keygen `-t`).

[`docs/DEVELOPMENT.md`]: docs/DEVELOPMENT.md
