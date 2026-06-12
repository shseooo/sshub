# Connectunnel (sshub)

`~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버 목록 관리, SSH 키 생성/가져오기, 인앱 PTY 터미널(다중 탭)을 제공합니다.

## 기술 스택

| 계층 | 기술 |
|------|------|
| 데스크톱 | Tauri v2 |
| 백엔드 | Rust (JSON 파일 저장소, portable-pty) |
| 프론트엔드 | React 19 + TypeScript 5.9 |
| 빌드 | Vite 8, bun |
| UI | Tailwind CSS v4 (CSS-first, `tailwind.config.js` 없음) |
| 터미널 | @xterm/xterm 6 + Rust PTY |
| 상태 관리 | TanStack Query v5 |

## 사전 요구사항

- **bun** 1.x — `curl -fsSL https://bun.sh/install | bash`
- **Rust** (stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **macOS**: Xcode Command Line Tools — `xcode-select --install`
- **Linux**: `webkit2gtk-4.1`, `libappindicator3` 등 [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) 참고
- **Windows**: WebView2 (Win10+ 기본 포함), MSVC Build Tools

> 패키지 매니저는 **bun**입니다. `tauri.conf.json`의 beforeBuildCommand가 `bun run build`로 고정되어 있으므로 npm/yarn을 섞어 쓰지 마세요.

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

프론트엔드만 빌드/타입체크: `bun run build` (`tsc && vite build`)
Rust만 검사: `cd src-tauri && cargo check`

## 프로젝트 구조

```
├── src/                      # React 프론트엔드
│   ├── pages/                # Dashboard, ServerList/Edit, KeyManager, TerminalPage, Settings
│   ├── hooks/                # useServers, useKeys (TanStack Query 래퍼)
│   ├── lib/tauriCommands.ts  # Tauri invoke 헬퍼 (커맨드 1:1 매핑)
│   └── types/                # Server, SshKey, TerminalTab 타입
└── src-tauri/
    ├── src/lib.rs            # 앱 엔트리 (플러그인/상태/커맨드 등록)
    ├── src/store.rs          # JSON 파일 저장소 (원자적 쓰기: tmp + rename)
    ├── src/models.rs         # 데이터 모델 (serde camelCase 직렬화)
    └── src/commands/         # IPC 커맨드: server / key / ssh_config / terminal
```

## 데이터 위치 (macOS)

- 서버/키 데이터: `~/Library/Application Support/sshub.json` (직접 열어서 확인/백업 가능)
- 생성/가져온 SSH 키: `~/Library/Application Support/ssh_keys/`
- `서버 → SSH Config` 동기화 시 기존 `~/.ssh/config`는 `config.bak.<타임스탬프>`로 자동 백업됩니다.

## 알아두기

- 인앱 터미널은 시스템 `ssh`를 PTY로 실행합니다. 비밀번호/호스트키 확인은 터미널 안에서 직접 입력하면 됩니다.
- 터미널 페이지를 벗어나면(라우트 이동) 열려 있던 세션은 종료됩니다.
- `+` 탭 버튼은 로컬 셸(`$SHELL -l`)을 엽니다.
