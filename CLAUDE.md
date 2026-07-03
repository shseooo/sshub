# CLAUDE.md

Claude Code 가이드. 작업 전 이 파일과 `.claude/rules/*`, 상세 배경은
[`docs/DEVELOPMENT.md`]를 읽는다.

## 프로젝트

**sshub** — `~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 백업/복원, 다국어·테마.
스택: Electron (메인 프로세스도 TypeScript, esbuild 번들) · React 19 + TypeScript ·
Vite 8 · Tailwind v4 · @xterm/xterm 6 + node-pty · TanStack Query.
로컬 디렉터리명은 `connectunnel`이지만 제품/리포명은 **sshub**.
v0.2.0에서 Tauri → Electron 전환 완료(WKWebView가 CJK IME composition 이벤트를
발화하지 않아 xterm 한글 입력이 깨지는 문제 — Chromium은 정상).

## 명령어

```bash
bun install
bun run app                # 개발 (vite dev + Electron 동시 실행/자동 종료)
bun run build              # 프론트 타입체크+빌드 (tsc && vite build)
bun run typecheck:electron # 메인 프로세스 타입체크 (tsconfig.electron.json)
bun run test               # Vitest (src/** + electron/** 테스트 전부)
bun run test:watch
bun run dist               # 프로덕션 패키징 (electron-builder → release/, mac dmg)
./install.sh               # 빌드 + ad-hoc 서명 + /Applications 설치
```

## 구조

- `electron/`(메인 프로세스): `main.ts`(BrowserWindow·node-pty·단일 `invoke` IPC
  채널의 커맨드 switch), `preload.ts`(`window.electronAPI.{invoke,on}`만 노출),
  `store.ts`(JSON 저장소, 원자적 tmp+rename·0600), `keys.ts`(ssh-keygen I/O),
  `backup.ts`, `sshConfigFile.ts`, `scrollbackStore.ts`, `terminalCwd.ts`.
- `electron/lib/`(순수 로직 + 단위테스트): `serverOps`·`keyOps`·`keyType`·
  `keyFiles`(파일명 새니타이즈)·`ssh`(`buildSshArgs`)·`sshConfig`(파서/렌더러)·
  `bundleOps`·`crypto`(AES-256-GCM/scrypt)·`scrollback`·`windowState`.
- `src/`: `pages/` 화면, `components/` 재사용 UI(Sidebar, TerminalHost=분할터미널,
  Select), `contexts/` 전역상태(Terminal/Language/Shortcuts/Theme), `hooks/` Query
  래퍼(`useServers`/`useKeys`), `lib/`(`bridge.ts`=IPC 브리지,
  `commands.ts`=커맨드 래퍼, `terminalPool.ts`=xterm 인스턴스 풀·IME 팬텀 가드),
  `i18n/`(ko/en/ja), `types/`.
- 데이터: `~/Library/Application Support/sshub.json`(비밀 없음), 개인키
  `ssh_keys/`(0600), UI 설정은 localStorage.

## MUST

- 패키지 매니저는 bun 을 사용한다.
- 변경을 완료로 보고하기 전에 `bun run build`, `bun run typecheck:electron`,
  `bun run test`를 모두 통과시킨다.
- IPC 호출은 `src/lib/commands.ts`(또는 `bridge.ts`) 래퍼를 통하고, 새 커맨드는
  `electron/main.ts`의 `invoke` switch에 등록하며 래퍼 함수를 함께 추가한다.
- 사용자 문자열은 i18n 3종(ko/en/ja)에 동시에 추가한다.
- 드롭다운은 `@/components/Select`를 사용한다.
- 모델에 새 옵션 필드를 추가할 때는 optional(`?`) + 읽기 시 기본값 보정으로 구버전
  `sshub.json` 역호환을 보장한다.
- 회귀 버그는 재현 테스트를 먼저 추가한다.
- 비밀(개인 키)은 `ssh_keys/` 0600 파일로만 존재시킨다.
- 사용자 입력 파일명은 `keyFileName`(electron/lib/keyFiles.ts)으로 새니타이즈한다
  (경로 traversal 차단).
- 내보내기는 비밀을 제거하고, 키 포함 시 AES-256-GCM(scrypt) passphrase 암호화를
  적용한다(`electron/lib/crypto.ts`).
- ssh 인증 옵션을 정확히 지정한다(`buildSshArgs`): password→`PubkeyAuthentication=no`,
  key/pem→`-i + IdentitiesOnly=yes`, agent→`PreferredAuthentications=publickey`.
- BrowserWindow 보안 설정을 유지한다: `contextIsolation: true`,
  `nodeIntegration: false`, preload는 `invoke`/`on`만 노출.
- 빌드 후 macOS 실행을 위해 ad-hoc 서명을 적용한다(`install.sh`가 수행):
  `codesign --force --deep --sign - release/mac-arm64/sshub.app`.
- 키 rename 시 개인 키 파일도 함께 이동한다(`update_ssh_key`).
- 탭 표시 이름은 `tab.name ?? 첫 leaf.label`을 따른다.

## MUST NOT

- `npm`/`yarn`을 사용하지 않는다.
- 컴포넌트/훅에서 `window.electronAPI`/`invoke`를 직접 호출하지 않는다.
- 네이티브 `<select>`를 새로 추가하지 않는다.
- 개인 키 평문을 JSON 저장소/로그/내보내기에 기록하지 않는다.
- 비밀번호를 저장하지 않는다.
- ad-hoc 서명에 hardened runtime 플래그를 함께 부여하지 않는다(GUI 실행 차단 위험).
- `~/.ssh/config`의 사용자/린터 지정 값을 임의로 되돌리지 않는다.
- 대화형 `rm`/`git -i`를 사용하지 않는다.

## SHOULD

- 복잡한 순수 로직은 `src/lib/`·`electron/lib/` 순수함수로 분리해 테스트 가능하게
  만든다.
- 커밋 메시지는 한국어 요약을 사용하고, 파일 삭제는 `git rm`/`find -delete`를 쓴다.

## NOTE

- import 시 키 타입은 라벨일 뿐 접속에 영향이 없다. 생성 시에는 의미가 있다
  (ssh-keygen `-t`).
- node-pty는 Electron ABI 리빌드가 필요하다(electron-builder/`@electron/rebuild`가
  수행 — 네이티브 모듈 에러 시 의심).
- 탭 전환 시 Chromium이 방금 확정한 IME 음절을 새 터미널에 재삽입하는 문제는
  `terminalPool.ts`의 `ignorePhantom` 가드가 처리한다.

[`docs/DEVELOPMENT.md`]: docs/DEVELOPMENT.md
