# sshub

`~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 기기 간 동기화, 다국어·테마 커스터마이즈를 제공합니다.

> CRT 인광 콘솔 테마의 단일 창 데스크톱 앱 (Tauri 2 · React · Rust).

## 주요 기능

- **서버 관리** — CRUD, 그룹/태그/메모, 즐겨찾기, 검색·그룹 필터, 최근 연결 표시
- **SSH 키** — 생성(`ssh-keygen`)·가져오기(파일/붙여넣기, 공개키 자동 추출)·**편집**(이름/공개키/개인키 교체)·**passphrase 변경**(`ssh-keygen -p`), 개인 키 없으면 표시
- **인증 방식** — SSH 키(`-i`) / 비밀번호 / PEM / **SSH 에이전트**, **ProxyJump(점프 호스트, `-J`)** 지원
- **인앱 터미널** — 시스템 `ssh`를 PTY로 실행
  - 다중 탭, **중첩 분할**(좌우·상하 혼합, 드래그 리사이즈)
  - **드래그**로 탭 순서 변경 · 분할→독립 탭 분리 · 탭 병합 (세션·스크롤백 유지)
  - 패널 포커스 이동, **동시 입력(broadcast)**, 탭/패널 **재연결**, 탭 우클릭 메뉴(닫기/다른 탭/오른쪽)
  - 라우트를 이동해도 세션 유지 + 다음 실행 시 레이아웃 복원
- **단축키** — 새 탭/패널 닫기/분할/탭 이동/패널 포커스 이동/동시 입력, 설정에서 재바인딩
- **~/.ssh/config 동기화** — 양방향(덮어쓰기 전 자동 백업)
- **기기 간 내보내기/가져오기** — 서버/키 선택 또는 전체, 개인 키 포함 시 passphrase 암호화
- **다국어** — 한국어 / English / 日本語 (기본은 시스템 언어, 그 외 영어)
- **테마** — 강조색·배경 톤·터미널 글자/배경색·UI 투명도(macOS 비브런시)
- **상태 기억** — 창 크기·위치, 사이드바 접힘, 터미널 레이아웃
- **시작 메뉴** — 앱을 켤 때 처음 열 메뉴 선택(설정 → General)
- **보안** — 개인 키·서버 PEM 평문 미저장(`0600` 파일 분리), 파일명 새니타이즈, 내보내기 시 비밀 제거, CSP 적용

## 기술 스택

| 계층       | 기술                                                   |
| ---------- | ------------------------------------------------------ |
| 데스크톱   | Tauri v2 (macOS private API · window-vibrancy)         |
| 백엔드     | Rust (JSON 파일 저장소, portable-pty, cocoon 암호화)   |
| 프론트엔드 | React 19 + TypeScript 5.9                              |
| 빌드       | Vite 8, bun                                            |
| UI         | Tailwind CSS v4 (CSS-first, `tailwind.config.js` 없음) |
| 터미널     | @xterm/xterm 6 + Rust PTY                              |
| 상태 관리  | TanStack Query v5                                      |
| 테스트     | Vitest (프론트) · `cargo test` (Rust)                  |

## 사전 요구사항

- **bun** 1.x — `curl -fsSL https://bun.sh/install | bash`
- **Rust** (stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **macOS**: Xcode Command Line Tools — `xcode-select --install`
- **Linux**: `webkit2gtk-4.1`, `libappindicator3` 등 [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) 참고
- **Windows**: WebView2 (Win10+ 기본 포함), MSVC Build Tools

> 패키지 매니저는 **bun**입니다. `tauri.conf.json`의 beforeBuildCommand가 `bun run build`로 고정되어 있으므로 npm/yarn을 섞어 쓰지 마세요. (Node.js는 필요 없음 — bun이 모든 빌드 스크립트를 실행)

## 빠른 설치 (macOS)

빌드 도구(Xcode CLT·Rust·bun) 점검·설치 → 릴리스 빌드 → ad-hoc 서명 → `/Applications` 설치까지 한 번에:

```bash
./install.sh
```

- 이미 설치된 도구는 건너뛰고, 재실행해도 안전합니다.
- Xcode Command Line Tools만 없을 때는 설치창 특성상 한 번 끊기고 "완료 후 재실행" 안내가 나옵니다.

## 개발 실행

```bash
bun install        # 의존성 설치
bun run tauri dev  # 개발 모드 (vite dev 서버 + Rust 핫 리로드)
```

- 프론트엔드 dev 서버는 `http://localhost:1420` (포트 고정, strictPort)
- 프론트만 빠르게 확인하려면 `bun run dev` (단, Tauri API는 브라우저에서 동작 안 함)

## 프로덕션 빌드

```bash
bun run tauri build
```

산출물 (macOS 기준):

```
src-tauri/target/release/bundle/macos/sshub.app
src-tauri/target/release/bundle/dmg/sshub_<버전>_<아키텍처>.dmg
```

### macOS: 빌드 후 ad-hoc 서명

Apple Developer 계정이 없으면 빌드 후 ad-hoc 서명을 해야 **Finder/Dock에서 실행**됩니다. (위 `./install.sh`가 이 서명 + `/Applications` 설치까지 자동으로 처리합니다.)

```bash
codesign --force --deep --sign - src-tauri/target/release/bundle/macos/sshub.app
```

- 첫 실행 시 "확인되지 않은 개발자" 경고가 나오면 **우클릭 → 열기**, 또는 시스템 설정 → 개인 정보 보호 및 보안 → "그래도 열기".
- 다른 Mac으로 복사해 막히면: `xattr -dr com.apple.quarantine /Applications/sshub.app`
- 경고 없이 배포하려면 Apple Developer Program($99/년) Developer ID 서명 + 공증 필요.

프론트엔드만 빌드/타입체크: `bun run build` (`tsc && vite build`) · Rust만 검사: `cd src-tauri && cargo check`

테스트: `bun run test` (Vitest) · `cd src-tauri && cargo test`

## 단축키 (터미널)

| 동작                | 기본 키            | 비고                     |
| ------------------- | ------------------ | ------------------------ |
| 새 탭(로컬)         | `Cmd+T`            | 설정에서 변경 가능       |
| 패널 닫기           | `Cmd+W`            | 설정에서 변경 가능       |
| 옆으로 분할         | `Cmd+D`            | 설정에서 변경 가능       |
| 아래로 분할         | `Cmd+Shift+D`      | 설정에서 변경 가능       |
| 동시 입력 토글      | `Cmd+Shift+I`      | 설정에서 변경 가능       |
| 패널 포커스 이동    | `Cmd+Opt+방향키`   | 설정에서 변경 가능       |
| 탭 이동             | `Cmd+1`~`Cmd+9`    | 고정                     |

## 데이터 위치 (macOS)

- 서버/키 메타데이터: `~/Library/Application Support/sshub.json` (직접 열어 확인/백업 가능, 비밀 없음)
- 생성/가져온 개인 키: `~/Library/Application Support/ssh_keys/` (`0600`)
- 언어·단축키·테마 등 UI 설정: 브라우저 `localStorage`
- `서버 → SSH Config` 동기화 시 기존 `~/.ssh/config`는 `config.bak.<타임스탬프>`로 자동 백업됩니다.

## 프로젝트 구조

```
├── src/                       # React 프론트엔드
│   ├── pages/                 # Dashboard, ServerList/Edit, KeyManager, Settings
│   ├── components/            # Sidebar, TerminalHost(분할 터미널)
│   ├── contexts/              # Terminal/Language/Shortcuts/Theme
│   ├── hooks/                 # useServers, useKeys
│   ├── lib/                   # tauriCommands, shortcuts, theme
│   ├── i18n/                  # ko / en / ja 사전
│   └── types/
└── src-tauri/
    ├── src/lib.rs             # 앱 엔트리 (플러그인/상태/커맨드/메뉴/비브런시)
    ├── src/store.rs           # JSON 파일 저장소 (원자적 쓰기: tmp + rename, 0600)
    ├── src/models.rs          # 데이터 모델 (serde camelCase)
    └── src/commands/          # IPC: server / key / ssh_config / terminal / backup
```

## 알아두기

- 인앱 터미널은 시스템 `ssh`를 PTY로 실행합니다. 비밀번호·호스트키 확인은 터미널 안에서 직접 입력합니다.
- 비밀번호 인증 서버는 키 제시 없이 바로 비밀번호 프롬프트로 가며, 연결 시 `ConnectTimeout`로 빠르게 실패합니다.
- 분할로 추가되는 패널과 `+` 탭은 로컬 셸(`$SHELL -l`)을 엽니다.
- 터미널 세션은 라우트를 이동해도 유지되며, 앱을 다시 켜면 마지막 탭/분할 레이아웃이 복원됩니다(라이브 세션은 재연결).
