# sshub

`~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 기기 간 동기화, 다국어·테마 커스터마이즈를 제공합니다.

> CRT 인광 콘솔 테마의 단일 창 데스크톱 앱 (Electron · Chromium · React · Node).

> **v0.2.0 — Tauri(WKWebView) → Electron(Chromium) 전환.**
> macOS WKWebView가 한글/CJK 입력을 composition 이벤트 없이 `insertReplacementText`로
> 전달해 xterm.js가 조합을 처리하지 못하던 문제(예: "상세"→"ㅅㅅ")를, Chromium 기반
> Electron으로 전환해 해결했습니다. (VS Code가 동일한 xterm.js로 정상 동작하는 것과 같은 원리.)

## 주요 기능

- **서버 관리** — CRUD, 그룹/태그/메모, 즐겨찾기, 검색·그룹 필터, 최근 연결 표시
- **SSH 키** — 생성(`ssh-keygen`)·가져오기(파일/붙여넣기, 공개키 자동 추출)·**편집**(이름/공개키/개인키 교체)·**passphrase 변경**(`ssh-keygen -p`), 개인 키 없으면 표시
- **인증 방식** — SSH 키(`-i`) / 비밀번호 / PEM / **SSH 에이전트**, **ProxyJump(점프 호스트, `-J`)** 지원
- **인앱 터미널** — 시스템 `ssh`를 PTY(node-pty)로 실행
  - 다중 탭, **중첩 분할**(좌우·상하 혼합, 드래그 리사이즈)
  - **드래그**로 탭 순서 변경 · 분할→독립 탭 분리 · 탭 병합 (세션·스크롤백 유지)
  - 패널 포커스 이동, **동시 입력(broadcast)**, 탭/패널 **재연결**, 탭 우클릭 메뉴(닫기/다른 탭/오른쪽)
  - 라우트를 이동해도 세션 유지 + 다음 실행 시 레이아웃 복원
  - **한글/CJK 입력** 정상 (Chromium composition 이벤트)
- **단축키** — 새 탭/패널 닫기/분할/탭 이동/패널 포커스 이동/동시 입력, 설정에서 재바인딩
- **~/.ssh/config 동기화** — 양방향(덮어쓰기 전 자동 백업)
- **기기 간 내보내기/가져오기** — 서버/키 선택 또는 전체, 개인 키 포함 시 passphrase 암호화(AES-256-GCM)
- **다국어** — 한국어 / English / 日本語 (기본은 시스템 언어, 그 외 영어)
- **테마** — 강조색·배경 톤·터미널 글자/배경색·UI 투명도(macOS vibrancy)
- **보안** — 개인 키·서버 PEM 평문 미저장(`0600` 파일 분리), 파일명 새니타이즈, 내보내기 시 비밀 제거

## 기술 스택

| 계층       | 기술                                                       |
| ---------- | ---------------------------------------------------------- |
| 데스크톱   | Electron 42 (Chromium · macOS vibrancy)                    |
| 백엔드     | Node (TypeScript) — JSON 파일 저장소, node-pty, AES-256-GCM |
| 프론트엔드 | React 19 + TypeScript 5.9                                  |
| 빌드       | Vite 8, esbuild(메인), electron-builder, bun               |
| UI         | Tailwind CSS v4 (CSS-first, `tailwind.config.js` 없음)     |
| 터미널     | @xterm/xterm 6 + node-pty                                  |
| 상태 관리  | TanStack Query v5                                          |
| 테스트     | Vitest (프론트 + Electron 백엔드 순수 로직)                |

프론트엔드(React/xterm)는 그대로 두고, `src/lib/bridge.ts`가 IPC(`invoke`/`listen`)·파일
다이얼로그·`homeDir`을 **Electron(`window.electronAPI`)** 또는 **Tauri**로 라우팅합니다.
따라서 호출부는 쉘에 비종속이며, 기존 Tauri 빌드도 폴백으로 동작합니다.

## 사전 요구사항

- **macOS** (Apple Silicon 기준)
- **Xcode Command Line Tools** — node-pty 네이티브 모듈 컴파일용 (`xcode-select --install`)
- **bun** — 패키지 매니저 / 빌드 (`curl -fsSL https://bun.sh/install | bash`)
- 시스템 `ssh` / `ssh-keygen` (macOS 기본 포함)

## 빠른 설치 (macOS)

```bash
./install.sh
```

사전 요구사항을 확인·설치하고, 릴리스 `.app`을 빌드한 뒤 ad-hoc 서명 → `/Applications`
설치 → 실행까지 자동으로 처리합니다. 재실행해도 안전합니다.

> 기존 Tauri 빌드 스크립트는 `install-tauri.sh`로 남겨두었습니다.

## 개발 실행

Electron 개발은 **두 프로세스**가 필요합니다 (vite dev 서버 + Electron):

```bash
bun install
bun run dev        # 터미널 1: vite dev 서버 (http://localhost:1420, 포트 고정)
bun run electron   # 터미널 2: main/preload 번들 후 Electron 실행 (dev 서버 로드)
```

- `bun run electron`은 `electron:build`(esbuild로 `electron/main.ts`·`preload.ts` → `electron/out/`)를 먼저 돌립니다.
- 메인 프로세스 코드(`electron/`)를 고치면 `bun run electron`을 다시 실행하고, 프론트(`src/`)는 vite HMR로 반영됩니다(또는 창에서 `⌘R`).

## 프로덕션 빌드

```bash
bun run dist       # bun run build (dist) + electron:build + electron-builder --mac
```

산출물 (Apple Silicon 기준):

```
release/mac-arm64/sshub.app
release/sshub-<버전>-arm64.dmg
```

### macOS: 빌드 후 ad-hoc 서명

Apple Developer 계정이 없으면 ad-hoc 서명을 해야 **Finder/Dock에서 실행**됩니다.
(`./install.sh`가 이 서명 + `/Applications` 설치까지 자동 처리합니다.)

```bash
codesign --force --deep --sign - release/mac-arm64/sshub.app
```

- 첫 실행 시 "확인되지 않은 개발자" 경고가 나오면 **우클릭 → 열기**.
- 다른 Mac으로 복사해 막히면: `xattr -dr com.apple.quarantine /Applications/sshub.app`
- hardened runtime 플래그는 부여하지 않습니다(ad-hoc 서명에 함께 주면 GUI 실행이 막힘).

검증: `bun run test` (Vitest) · `bun run typecheck:electron` (Electron 백엔드 타입체크) · `bun run build` (프론트 `tsc && vite build`)

## 단축키 (터미널)

| 동작             | 기본 키          | 비고               |
| ---------------- | ---------------- | ------------------ |
| 새 탭(로컬)      | `Cmd+T`          | 설정에서 변경 가능 |
| 패널 닫기        | `Cmd+W`          | 설정에서 변경 가능 |
| 옆으로 분할      | `Cmd+D`          | 설정에서 변경 가능 |
| 아래로 분할      | `Cmd+Shift+D`    | 설정에서 변경 가능 |
| 동시 입력 토글   | `Cmd+Shift+I`    | 설정에서 변경 가능 |
| 패널 포커스 이동 | `Cmd+Opt+방향키` | 설정에서 변경 가능 |
| 탭 이동          | `Cmd+1`~`Cmd+9`  | 고정               |

## 데이터 위치 (macOS)

- 서버/키 메타데이터: `~/Library/Application Support/sshub.json` (비밀 없음, Tauri 빌드와 **동일 경로**라 데이터 그대로 인계)
- 생성/가져온 개인 키 · 서버 PEM: `~/Library/Application Support/ssh_keys/` (`0600`)
- 언어·단축키·테마 등 UI 설정: `localStorage`
- `서버 → SSH Config` 동기화 시 기존 `~/.ssh/config`는 `config.bak.<타임스탬프>`로 자동 백업됩니다.

## 프로젝트 구조

```
├── src/                       # React 프론트엔드 (쉘 비종속)
│   ├── pages/                 # Dashboard, ServerList/Edit, KeyManager, Settings
│   ├── components/            # Sidebar, TerminalHost(분할 터미널)
│   ├── contexts/              # Terminal/Language/Shortcuts/Theme
│   ├── lib/                   # bridge(IPC 라우팅), tauriCommands, terminalPool, theme
│   ├── i18n/                  # ko / en / ja 사전
│   └── types/
├── electron/                  # Electron 메인 프로세스 (Node 백엔드)
│   ├── main.ts                # 앱 엔트리 · IPC 디스패치 · node-pty · 다이얼로그
│   ├── preload.ts             # window.electronAPI 노출
│   ├── store.ts               # JSON 파일 저장소 (원자적 tmp+rename, 0600)
│   ├── keys.ts / backup.ts / sshConfigFile.ts   # 키관리 / 백업 / ssh_config I/O
│   └── lib/                   # 순수 로직 + 테스트: serverOps, keyOps, keyType,
│                              #   keyFiles, ssh, sshConfig, bundleOps, crypto
└── src-tauri/                 # (구) Tauri 백엔드 — 폴백으로 유지, 추후 제거 예정
```

## 알아두기

- 인앱 터미널은 시스템 `ssh`를 PTY(node-pty)로 실행합니다. 비밀번호·호스트키 확인은 터미널 안에서 직접 입력합니다.
- 비밀번호 인증 서버는 키 제시 없이 바로 비밀번호 프롬프트로 가며, 연결 시 `ConnectTimeout`로 빠르게 실패합니다.
- 분할로 추가되는 패널과 `+` 탭은 로컬 셸(`$SHELL -l`)을 엽니다.
- 백업 암호화는 `cocoon`(Tauri)에서 **AES-256-GCM(scrypt)**으로 바뀌었습니다. 평문 export(JSON)는 양쪽 호환되지만, 기존 cocoon 암호화 export 파일은 더 이상 가져올 수 없습니다.
