# sshub

`~/.ssh/config`와 연동되는 크로스플랫폼 SSH 관리 데스크톱 앱.
서버·키 관리, 인앱 PTY 분할 터미널, 기기 간 동기화, 다국어·테마 커스터마이즈를 제공합니다.

> Rust 네이티브 데스크톱 앱 (GPUI + alacritty_terminal).
> 웹뷰가 없어 한글/CJK 입력이 macOS 네이티브 IME 경로로 처리되고,
> 한글 고정폭 폰트(D2Coding)를 내장해 터미널 격자에 정확히 맞습니다.
>
> **`~/.ssh/config`를 직접 관리합니다** — 앱에서 만든 서버는 터미널에서
> `ssh <별칭>`으로 바로 접속되고, 손으로 고친 설정도 앱에 그대로 보입니다.

## 주요 기능

- **서버 관리** — CRUD, 그룹/태그/메모, 즐겨찾기, 검색·그룹 필터, 최근 연결 표시
- **SSH 키** — 생성(`ssh-keygen`)·가져오기(파일/붙여넣기, 공개키 자동 추출)·**편집**(이름/공개키/개인키 교체)·**passphrase 변경**(`ssh-keygen -p`), 개인 키 없으면 표시
- **인증 방식** — SSH 키(`-i`) / 비밀번호 / PEM / **SSH 에이전트**, **ProxyJump(점프 호스트, `-J`)** 지원
- **인앱 터미널** — 시스템 `ssh`를 PTY로 실행 (`ssh <별칭>`)
  - 다중 탭, **중첩 분할**(좌우·상하 혼합, 드래그 리사이즈)
  - **드래그**로 탭 순서 변경 · 분할→독립 탭 분리 · 탭 병합 (세션·스크롤백 유지)
  - 패널 포커스 이동, **동시 입력(broadcast)**, 탭/패널 **재연결**, 탭 우클릭 메뉴(닫기/다른 탭/오른쪽)
  - 라우트를 이동해도 세션 유지 + 다음 실행 시 레이아웃 복원
  - **한글/CJK 입력** 정상 (Chromium composition 이벤트)
- **단축키** — 새 탭/패널 닫기/분할/탭 이동/패널 포커스 이동/동시 입력, 설정에서 재바인딩
- **~/.ssh/config 동기화** — 양방향(덮어쓰기 전 자동 백업)
- **기기 간 내보내기/가져오기** — 서버/키 선택 또는 전체, 개인 키 포함 시 passphrase 암호화(AES-256-GCM)
- **다국어** — 한국어 / English / 日本語 (기본은 시스템 언어, 그 외 영어)
- **테마** — 강조색·터미널 글자/배경색·폰트·UI 투명도(macOS 블러)
- **보안** — 개인 키·서버 PEM 평문 미저장(`0600` 파일 분리), 파일명 새니타이즈, 내보내기 시 비밀 제거

## 기술 스택

| 계층      | 기술                                                        |
| --------- | ----------------------------------------------------------- |
| UI        | GPUI 0.2.2 (Zed의 GPU 가속 네이티브 UI 프레임워크)          |
| 터미널    | alacritty_terminal 0.26 (upstream) + PTY                    |
| 언어      | Rust 2021 (edition), macOS Apple Silicon                    |
| 저장       | `~/.ssh/config`(원본) + `sshub.json`(앱 메타데이터 사이드카) |
| 암호화    | scrypt + AES-256-GCM (백업 내보내기)                        |
| 폰트      | D2Coding 내장 (한글이 ASCII의 정확히 2배 폭)                |

크레이트는 네 개입니다.

| 크레이트         | 역할                                                   |
| ---------------- | ------------------------------------------------------ |
| `sshub-core`     | 순수 로직·영속성 (config 문서 모델, 키, 암호화, 설정)   |
| `sshub-splits`   | 분할 트리·탭 연산 (UI 의존 없음)                        |
| `sshub-terminal` | alacritty 래핑 터미널 모델 (모든 alacritty import 격리) |
| `sshub`          | GPUI 앱 (위젯·화면·터미널 렌더링·다중 창)               |

### 데이터 소유권

**접속 정보의 원본은 `~/.ssh/config`, 키의 원본은 `~/.ssh` 디렉터리입니다.**
`sshub.json`은 즐겨찾기·그룹·태그·메모 같은 앱 전용 메타데이터만 갖습니다.
그래서 손으로 고친 config가 앱에 그대로 보이고, 앱에서 만든 서버는 터미널에서
`ssh <별칭>`으로 바로 접속됩니다. config 편집은 건드리는 줄만 바꾸는 방식이라
`Include`·`Match`·주석·사용자가 쓴 지시어가 보존됩니다.

## 사전 요구사항

- **macOS** (Apple Silicon 기준)
- **Rust** (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Xcode** — GPUI가 Metal 셰이더를 컴파일합니다 (Command Line Tools만으로는 부족)
- 시스템 `ssh` / `ssh-keygen` (macOS 기본 포함)
- (선택) **bun** — i18n 문자열을 고칠 때만 필요

## 빠른 설치 (macOS)

```bash
./install.sh
```

사전 요구사항을 확인·설치하고, 릴리스 빌드 → `.app` 번들 조립 → ad-hoc 서명 →
`/Applications` 설치 → 실행까지 처리합니다. 재실행해도 안전합니다.

## 개발

```bash
cargo test --workspace          # 전체 테스트
cargo run -p sshub              # 개발 실행 (번들이 아니라 아이콘은 안 나옵니다)
cargo build --release --bin sshub
bun scripts/gen_i18n.mjs        # 문자열 변경 후 i18n 재생성
```

문자열은 `crates/sshub/src/i18n/strings.json`에 ko/en/ja를 함께 넣습니다.
`generated.rs`는 생성물이라 직접 고치지 않습니다 — 번역이 빠지면 생성 시점과
컴파일 시점 양쪽에서 걸립니다.

설계 배경은 `docs/DESIGN-{overview,core,terminal,ui}.md`에 있습니다.

## 프로덕션 빌드

```bash
cargo build --release --bin sshub
```

`./install.sh`가 이 바이너리로 `.app` 번들을 조립합니다(GPUI에는 번들러가 없어
`Info.plist`와 번들 구조를 직접 만듭니다).

### macOS: ad-hoc 서명

Apple Developer 계정이 없으면 ad-hoc 서명을 해야 **Finder/Dock에서 실행**됩니다
(`./install.sh`가 처리합니다). hardened runtime은 부여하지 않습니다 — 붙이면
Gatekeeper가 실행을 막습니다.

- 첫 실행 시 "확인되지 않은 개발자" 경고가 나오면 **우클릭 → 열기**.
- 다른 Mac으로 복사해 막히면: `xattr -dr com.apple.quarantine /Applications/sshub.app`

검증: `cargo test --workspace`

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

| 무엇 | 어디 |
| --- | --- |
| 서버 접속 정보 (원본) | `~/.ssh/config` |
| 개인/공개 키 (원본) | `~/.ssh/` (`0600`) |
| 앱 메타데이터 (즐겨찾기·그룹·태그·메모) | `~/Library/Application Support/sshub.json` |
| 언어·단축키·테마·창 상태 | `~/Library/Application Support/sshub_settings.json` |
| 터미널 스크롤백 | `~/Library/Application Support/sshub_scrollback/` (`0700`) |
| 서버별 PEM | `~/Library/Application Support/ssh_keys/` (`0600`) |

`~/.ssh/config`를 고칠 때마다 `config.bak.<타임스탬프>`로 백업하며 최신 10개를
유지합니다. 앱은 자기가 소유한 지시어만 갱신하고, 사용자가 쓴 줄은 지우지 않습니다.

## 프로젝트 구조

```
├── crates/
│   ├── sshub-core/            # 순수 로직·영속성
│   │   ├── ssh_config/        #   config 문서 모델(라운드트립 보존) · 병합
│   │   ├── store.rs           #   config ⨝ 사이드카 조인
│   │   ├── keys_io / key_scan #   ssh-keygen · ~/.ssh 키 발견
│   │   └── crypto / backup    #   AES-256-GCM(scrypt) 내보내기
│   ├── sshub-splits/          # 분할 트리·탭 순수 연산
│   ├── sshub-terminal/        # alacritty 래핑 (backend.rs가 유일한 import 지점)
│   └── sshub/                 # GPUI 앱
│       ├── ui/                #   자작 위젯 (TextInput·Select·ContextMenu…)
│       ├── views/             #   서버 목록/편집 · 키 관리 · 설정
│       ├── terminal_*.rs      #   터미널 엘리먼트·뷰·워크스페이스
│       └── i18n/strings.json  #   ko/en/ja 문자열 원본
├── docs/                      # 설계 문서 (DESIGN-*.md)
└── install.sh                 # .app 번들 조립 + ad-hoc 서명 + 설치
```

## 알아두기

- 인앱 터미널은 시스템 `ssh`를 PTY로 실행합니다. 비밀번호·호스트키 확인은 터미널 안에서 직접 입력합니다.
- 비밀번호 인증 서버는 키 제시 없이 바로 비밀번호 프롬프트로 가며, 연결 시 `ConnectTimeout`로 빠르게 실패합니다.
- 분할로 추가되는 패널과 `+` 탭은 로컬 셸(`$SHELL -l`)을 엽니다.
- 백업 암호화는 **AES-256-GCM(scrypt)**을 사용합니다. 개인 키를 포함해 내보낼 때만 passphrase로 암호화되고, 평문 export(JSON)는 비밀이 제거됩니다.
