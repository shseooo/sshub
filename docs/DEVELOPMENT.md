# sshub 개발 규칙 (Electron · React · TypeScript)

이 문서는 sshub를 일관되고 안전하게 개발하기 위한 규칙이다. 새 기능/수정 시
여기에 맞춰 작업하고, 규칙을 바꿔야 하면 이 문서를 먼저 고친다.

sshub는 v0.2.0에서 Tauri(WKWebView)에서 Electron(Chromium)으로 전환했다.
WKWebView가 한글/CJK 입력을 composition 이벤트 없이 전달해 xterm.js가 조합을
처리하지 못하던 문제를 해결하기 위해서다. 메인 프로세스는 Rust가 아닌
TypeScript로 작성되고 esbuild로 번들된다.

---

## 0. 핵심 원칙

- **안정성 · 속도 · 보안이 최우선.** 화려함보다 깨지지 않는 것.
- **패키지 매니저는 bun 고정.** npm/yarn 금지.
- **순수 로직과 부수효과(IO·React·IPC)를 분리**해 테스트 가능하게 둔다.
- 모든 사용자 노출 문자열은 **i18n(ko/en/ja 3종)** 에 동시에 추가한다.

---

## 1. TDD

작은 순수 함수는 테스트를 먼저 쓰고 구현한다. 특히 다음은 항상 테스트가 선다:

- 트리/상태 변환 (`paneTree`, split/merge/insert/remove)
- 파서 (`parseSshConfig` — `electron/lib/sshConfig.ts`)
- 직렬화·정규화 (`normalizeCombo`, key 타입 판별, 파일명 새니타이즈)
- 보안 경계 (경로 traversal 차단, 비밀 미저장, 암호화 라운드트립)

워크플로:

1. 실패하는 테스트 작성 (대상 파일 옆의 `*.test.ts`).
2. 통과시킬 최소 구현.
3. 리팩터링 — 테스트가 그물망.

> UI 컴포넌트 전체를 E2E로 검증하려 하지 말 것. **로직을 함수로 빼서**
> 단위 테스트하고, 컴포넌트는 그 함수를 호출만 하게 한다.
> (예: 분할 트리 연산은 `TerminalContext`에서 `export`된 순수 함수로 테스트한다.)

### 테스트 실행

```bash
bun run test          # Vitest 1회 (src/** + electron/** 모두)
bun run test:watch    # Vitest watch
```

회귀 버그를 고칠 때는 **그 버그를 재현하는 테스트를 먼저** 추가한다
(예: `comboFromEvent`가 `shift+meta+KeyD`를 내야 Cmd+Shift+D가 동작 —
`shortcuts.test.ts`에 고정).

---

## 2. 코딩 스타일

### 프론트엔드 (TypeScript / React)

- 함수형 컴포넌트 + 훅. 클래스 금지.
- 서버 상태는 **TanStack Query**(`useServers`, `useKeys`), 전역 UI 상태는
  **Context**(Terminal/Language/Shortcuts/Theme). 컴포넌트 로컬은 `useState`.
- IPC 호출은 컴포넌트에서 직접 하지 말고 **`src/lib/commands.ts`** 의 얇은
  래퍼를 통한다(타입 1곳 관리). 래퍼는 `src/lib/bridge.ts`의 `invoke`를 쓴다.
- 타입은 `src/types/`. DTO는 프론트/메인 프로세스 양쪽 camelCase로 1:1.
- 들여쓰기 2칸, 세미콜론 없음(기존 코드 관습), 작은따옴표.
- 드롭다운은 네이티브 `<select>` 대신 **`@/components/Select`** 를 쓴다(테마 일관성).
- 주석은 "무엇"이 아니라 "왜"를 적는다(되도록 영어, 기존 톤 유지).

### 메인 프로세스 (Electron · TypeScript)

- IPC 커맨드는 `electron/main.ts`의 단일 `invoke` 채널 switch에 등록하고,
  프론트 래퍼 함수(`src/lib/commands.ts`)를 함께 추가한다.
- 커맨드 에러는 throw로 전파한다(renderer에서 rejected Promise로 수신).
  사용자 메시지는 무엇을 어떻게 고칠지 알려준다(한국어 OK).
- 비즈니스 로직은 가능한 한 `electron/lib/`의 순수 함수로 빼서 동일 디렉터리의
  `*.test.ts`로 검증한다.
- 타입체크는 `bun run typecheck:electron`(`tsconfig.electron.json`).

---

## 3. 아키텍처

```
src/                     React 프론트엔드
  pages/                 라우트 화면 (Dashboard/ServerList/ServerEdit/KeyManager/Settings)
  components/            재사용 UI (Sidebar, TerminalHost, Select)
  contexts/              전역 상태 (Terminal/Language/Shortcuts/Theme)
  hooks/                 TanStack Query 래퍼 (useServers, useKeys)
  lib/                   bridge(IPC), commands(래퍼), terminalPool, shortcuts, theme
  i18n/                  ko/en/ja 사전 (단일 파일)
  types/                 공유 타입
electron/                Electron 메인 프로세스 (TypeScript, esbuild 번들)
  main.ts                앱 엔트리 · 단일 invoke IPC 디스패치 · node-pty · 다이얼로그 · vibrancy
  preload.ts             window.electronAPI.{invoke,on} 노출
  store.ts               JSON 파일 저장소 (원자적 쓰기: tmp+rename, fsync, 0600)
  keys.ts backup.ts sshConfigFile.ts scrollbackStore.ts terminalCwd.ts   부수효과 I/O
  lib/                   순수 로직 + 테스트: serverOps, keyOps, keyType, keyFiles,
                         ssh, sshConfig, bundleOps, crypto, scrollback, windowState
```

규칙:

- **단방향 의존:** pages → hooks/lib/contexts → commands → (IPC) → main.ts → store.
  역방향(예: lib가 page를 import) 금지.
- **IPC 경계는 얇게.** 메인 프로세스 커맨드는 입력 검증 + store/외부프로세스
  호출만. 복잡한 트리/상태 가공은 프론트의 순수 함수에서.
- **저장소는 store.ts 한 곳.** 다른 모듈이 JSON 파일을 직접 만지지 않는다.
- 터미널 세션(PTY)은 메인 프로세스의 세션 맵이 단일 소유. 세션 id = 이벤트 채널 접미사.
- 새 옵션 필드를 모델에 추가하면 optional(`?`) + 읽기 시 기본값 보정으로 구버전
  `sshub.json` 역호환을 보장한다(예: `proxyJump`).

---

## 4. 보안 (절대 규칙)

- **개인 키 평문을 JSON 저장소에 절대 쓰지 않는다.** 비밀은 `ssh_keys/` 아래
  **0600 파일**로만 존재. 스토어 로드 시 `pemData`를 스크럽한다.
- 사용자 입력으로 파일명을 만들 때 **새니타이즈**한다(`keyFileName` —
  `electron/lib/keyFiles.ts`, 영숫자/`-`/`_`만, 나머지는 `_`). 경로
  traversal(`../`) 차단을 테스트로 고정.
- **내보내기(export)는 비밀을 벗긴다.** 키 포함 내보내기는 AES-256-GCM(scrypt)
  passphrase 암호화 필수(`electron/lib/crypto.ts`).
- ssh는 의도한 인증만 시도하게 한다(`electron/lib/ssh.ts` `buildSshArgs`):
  - password → `PubkeyAuthentication=no` (MaxAuthTries 소진 방지)
  - key/pem → `-i <path> -o IdentitiesOnly=yes`
  - agent → `PreferredAuthentications=publickey`
- BrowserWindow는 `contextIsolation: true`·`nodeIntegration: false`를 유지하고,
  preload는 `invoke`/`on`만 노출한다.
- 비밀번호는 저장하지 않는다(터미널에서 직접 입력).

---

## 5. 에러 처리

- 메인 프로세스: 커맨드 핸들러의 throw는 renderer로 rejected Promise가 되어
  전달된다. 사용자에게 **무엇을 어떻게** 고칠지 알려주는 메시지(예: "암호로
  보호된 키라면 passphrase를 입력하세요").
- 프론트: `invoke`는 reject될 수 있으므로 mutation은 `onError`로 사용자에게 표시.
  복구 불가한 백그라운드 작업은 `.catch(() => {})`로 조용히 무시하되 **데이터
  무결성에 영향 주는 호출은 절대 삼키지 않는다**.
- 터미널: 연결 실패/종료는 xterm에 ANSI로 알린다(`term.connectFail`,
  `closedNotice`). 죽은 호스트는 `ConnectTimeout`로 빠르게 실패.
- 부분 실패 허용: 분할 패널 broadcast write 등은 일부 실패해도 나머지를 진행.

---

## 6. 안티패턴 (하지 말 것)

- ❌ npm/yarn 사용, `package-lock.json` 생성.
- ❌ 컴포넌트 안에 복잡한 순수 로직 인라인(테스트 불가). → `lib/`로 추출.
- ❌ 컴포넌트/훅에서 `window.electronAPI`/`invoke` 직접 호출. → `commands.ts` 래퍼.
- ❌ 네이티브 `<select>` 신규 추가. → `Select` 컴포넌트.
- ❌ 비밀(개인키/비밀번호)을 JSON store·로그·export에 평문 저장.
- ❌ 모델에 옵션 필드 추가하면서 기본값 보정 누락 → 구버전 데이터 로드 시 이상.
- ❌ i18n 키를 한 언어에만 추가(빌드는 통과해도 다른 언어에서 키가 노출됨).
- ❌ `~/.ssh/config`를 임의 되돌리기(사용자/린터가 의도적으로 둔 값).
- ❌ 키 이름과 파일명의 결합을 잊은 채 키 rename(파일도 같이 옮겨야 접속 경로가 맞음 —
  `update_ssh_key` 참고).

---

## 7. 플랫폼 주의 (macOS)

- 빌드 후 Finder/Dock 실행하려면 **ad-hoc 서명** 필요(`install.sh`가 수행):
  `codesign --force --deep --sign - release/mac-arm64/sshub.app`. hardened runtime
  플래그를 ad-hoc에 같이 주면 GUI 실행이 Gatekeeper에 막힐 수 있음.
- 투명도(vibrancy)는 BrowserWindow `vibrancy: 'hud'` + 투명 `backgroundColor`.
  `--background`에 alpha를 baked-in (이중 알파 합성 방지).
- 타이틀바 드래그는 CSS `-webkit-app-region: drag`(`.app-drag`)로 처리.
- 아이콘은 full-bleed 불투명(투명 마진이면 macOS Tahoe에서 회색 타일).
- node-pty는 Electron ABI로 리빌드되어야 한다(electron-builder/`@electron/rebuild`가
  수행 — 네이티브 모듈 로드 에러 시 의심).

---

## 8. 변경 체크리스트

기능/수정 PR 전 확인:

- [ ] `bun run build` (tsc + vite) 통과
- [ ] `bun run typecheck:electron` 통과
- [ ] `bun run test` 통과 (+ 새 로직엔 테스트 추가)
- [ ] 새 사용자 문자열 → ko/en/ja 3종 모두 추가
- [ ] 모델 새 옵션 필드 → optional + 읽기 시 기본값 보정
- [ ] 비밀 미저장/0600/export strip 확인
- [ ] 커밋 메시지는 한국어 요약 + `Co-Authored-By` 트레일러
