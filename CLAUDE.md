# CLAUDE.md

Claude Code 가이드. 작업 전 이 파일과 `.claude/rules/*`, 설계 배경은
`docs/DESIGN-*.md`를 읽는다.

## 프로젝트

**sshub** — `~/.ssh/config`를 직접 관리하는 macOS 네이티브 SSH 관리 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 다중 창, 백업/복원, 다국어·테마.

스택: Rust + **GPUI 0.2.2**(crates.io) + **alacritty_terminal 0.26**(upstream).
웹뷰가 없으므로 CJK IME는 macOS 네이티브 경로(NSTextInputClient)를 탄다.
로컬 디렉터리명은 `connectunnel`이지만 제품/리포명은 **sshub**.

이력: Tauri → Electron(v0.2.0, WKWebView 한글 IME 문제) → Rust/GPUI(v0.4.0).
Electron·React 코드는 이전 완료 후 삭제했다 — 필요하면 `git log`에서 되찾는다.

## 명령어

```bash
cargo test --workspace       # 전체 테스트 (이것이 기본 검증)
cargo build --release --bin sshub
cargo run -p sshub           # 개발 실행 (번들이 아니라 아이콘은 안 나온다)
./install.sh                 # 릴리스 빌드 + .app 번들 + ad-hoc 서명 + /Applications
bun scripts/gen_i18n.mjs     # 문자열 변경 후 i18n 재생성
```

## 구조

- `crates/sshub-core/` — 순수 로직·영속성. **비 UI 전부.** `store`(config⨝사이드카),
  `ssh_config/{document,parse,file}`, `crypto`, `keys_io`, `key_scan`, `sidecar`,
  `ssh_args`, `settings`, `scrollback`, `terminal_cwd`, `window_state`.
- `crates/sshub-splits/` — 분할 트리·탭 순수 연산 (gpui 의존 없음).
- `crates/sshub-terminal/` — alacritty 래핑 터미널 모델. **모든 alacritty import는
  `backend.rs` 한 곳**(seam)에만 둔다.
- `crates/sshub/` — GPUI 앱. `ui/`(자작 위젯), `views/`(화면), `terminal_element`,
  `terminal_workspace`, `session_registry`, `workspace`, `window_manager`.
- `docs/DESIGN-{overview,core,terminal,ui}.md` — 설계 단일 진실. 각 문서 말미의
  "구현 노트"에 gpui 실측 API 차이가 누적돼 있다.

## 데이터 소유권 (가장 중요한 규칙)

**접속 정보의 원본은 `~/.ssh/config`, 키의 원본은 `~/.ssh` 디렉터리다.**
`sshub.json`(v3)은 앱 전용 메타데이터 사이드카일 뿐이다.

| 항목 | 소유자 |
|---|---|
| Host·HostName·Port·User·IdentityFile·ProxyJump | `~/.ssh/config` |
| 사용자가 쓴 그 외 지시어·주석·Include·Match | `~/.ssh/config` (보존만) |
| 개인/공개 키 파일 | `~/.ssh/` |
| 즐겨찾기·그룹·태그·메모·최근접속·id | `sshub.json` |
| 서버별 PEM(`pem_server_<id>`) | 앱 데이터 디렉터리 |

## MUST

- `~/.ssh/config` 편집은 `ssh_config::document`를 통해 **외과적으로** 한다.
  파싱→쓰기가 바이트 동일해야 하고(`Document::parse(t).to_string() == t`),
  건드리는 줄만 교체한다. 전체 렌더는 금지 — 그 방식으로 실제 데이터를 날린 적 있다.
- 앱이 소유한 지시어라도 **값이 없다고 사용자 줄을 지우지 않는다**. "앱이 이 키를
  안 쓴다"와 "이 호스트에 이 키가 없어야 한다"는 다른 말이다.
- 새 Host 블록은 첫 와일드카드 블록 **앞에** 넣는다 (ssh는 먼저 만난 값을 쓴다).
- config 쓰기는 타임스탬프 백업 → 원자적 쓰기 → **권한 보존**을 지킨다.
- 키 이름 = `~/.ssh` 안의 파일명. 이 불변식은 스토어 경계에서 새니타이즈로 강제한다.
- 서버 식별 id는 안정적이어야 한다 — 저장된 터미널 레이아웃의 `serverId`와
  `pem_server_<id>` 파일명이 참조한다.
- 변경을 완료로 보고하기 전에 `cargo test --workspace`와
  `cargo build --release --bin sshub`를 통과시킨다.
- 사용자 문자열은 `crates/sshub/src/i18n/strings.json`에 ko/en/ja 3종을 함께
  넣고 `bun scripts/gen_i18n.mjs`로 재생성한다. `generated.rs`는 직접 고치지 않는다.
- gpui API는 **로컬 레지스트리 소스**(`~/.cargo/registry/src/*/gpui-0.2.2/`)를
  grep해 확인한다. Zed 소스를 참고할 땐 태그 `gpui-v0.2.2` 기준으로만 본다
  (main은 API가 다르다).
- 개인 키는 0600, `~/.ssh`는 0700을 유지한다.

## MUST NOT

- 테스트에서 실제 `~/.ssh`나 `~/Library/Application Support/`를 읽거나 쓰지 않는다.
  반드시 `tempfile` + `AppPaths::in_dir`를 쓴다 (실제로 사용자 데이터를 건드린 적 있다).
- `Store::new`에 기본 경로를 만들지 않는다 — 경로 3종을 항상 명시한다.
- 개인 키 평문을 JSON 저장소·로그·내보내기에 남기지 않는다. 비밀번호는 저장하지 않는다.
- ad-hoc 서명에 hardened runtime을 부여하지 않는다 (GUI 실행이 막힌다).
- 다중 패턴(`Host a b c`)·와일드카드 블록을 편집하지 않는다 (읽기 전용).

## SHOULD

- 복잡한 순수 로직은 `sshub-core`/`sshub-splits`로 분리해 단위 테스트한다.
- 회귀 버그는 재현 테스트를 먼저 쓰고, **수정을 되돌려 실제로 실패하는지** 확인한다.
- 커밋 메시지는 한국어 요약 + 왜 그렇게 고쳤는지.
- 파일 삭제는 `git rm` 또는 `find -delete`를 쓴다.

## NOTE

- gpui 빌드는 Metal 셰이더 컴파일에 full Xcode가 필요하다. `.cargo/config.toml`이
  `DEVELOPER_DIR`를 지정하고, `runtime_shaders` feature로 우회 중이다.
- gpui 테스트 플랫폼은 `NoopTextSystem`이라 폰트 등록·메트릭을 런타임 검증할 수 없다.
- gpui 테스트 executor의 타이머는 가상 시계다 — 자식 프로세스를 기다리려면
  `thread::sleep` + `run_until_parked()`.
- 단축키 표기 정식 순서는 `Keystroke::unparse` 기준 `fn-ctrl-alt-cmd-shift-key`.
- 한글은 D2Coding을 바이너리에 내장해 쓴다(ASCII의 정확히 2배 폭). 와이드 문자는
  배치로 묶지 않고 셀마다 그린다 — 묶으면 폴백 폰트 advance로 격자가 어긋난다.
