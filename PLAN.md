# Connectunnel - 크로스플랫폼 SSH 관리 앱

\+

+> **작성일:** 2026-06-12

+> **상태:** 개발 준비 완료

\+

+---

\+

+## 📋 개요

\+

+`~/.ssh/config`를 관리하고, UI에서 서버를 선택하여 한 클릭으로 SSH 연결할 수 있는 크로스플랫폼 데스크톱 애플리케이션입니다.

\+

+## 🎯 핵심 기능

\+

+1. **SSH Config CRUD** - `~/.ssh/config` 읽기/쓰기/편집/동기화

+2. **서버 관리** - 서버 정보 저장, 그룹/태그, 즐겨찾기

+3. **SSH 터미널** - xterm.js 기반 터미널 에뮬레이터

+4. **SSH Key 관리** - 키 생성, 수입, 암호화 저장

+5. **인증 방식** - Key, Password, PEM 모두 지원 (비밀번호 자동 입력 및 모달)

\+

+---

\+

+## 🛠️ 기술 스택

\+

+| 계층 | 기술 | 버전 |

+|------|------|------|

+| 데스크톱 프레임워크 | Tauri | v2.3 |

+| 백엔드 | Rust | latest |

+| 프론트엔드 | React | 19.2 |

+| 빌드 도구 | Vite | v6.2 |

+| 언어 | TypeScript | v5.7 |

+| UI 프레임워크 | Tailwind CSS | v3.4 + shadcn/ui |

+| 터미널 | xterm.js | v5.3 |

+| 라우팅 | React Router | v7.1 |

+| 상태 관리 | TanStack Query | v5.66 |

+| 유효성 검증 | Zod | v3.24 |

+| 데이터베이스 | SQLite | rusqlite v0.32 |

+| 아이콘 | Lucide React | v0.475 |

\+

+---

\+

+## 🏗️ 프로젝트 구조

\+

+```

+connectunnel/

+├── PLAN.md # 이 파일

+├── package.json # Node.js 의존성

+├── vite.config.ts # Vite 설정

+├── tsconfig.json # TypeScript 설정

+├── tailwind.config.js # Tailwind CSS 설정

+├── components.json # shadcn/ui 설정

+│

+├── index.html # HTML 진입점

+├── src/ # 프론트엔드 (React 19.2 + TypeScript)

+│ ├── main.tsx # 앱 진입점 (createRoot)

+│ ├── App.tsx # 루트 컴포넌트 + 라우팅

+│ │

+│ ├── pages/ # 페이지 컴포넌트

+│ │ ├── Dashboard.tsx # 대시보드 (최근 연결, 즐겨찾기)

+│ │ ├── ServerList.tsx # 서버 목록 (검색, 필터)

+│ │ ├── ServerEdit.tsx # 서버 추가/수정 폼

+│ │ ├── KeyManager.tsx # SSH 키 목록/생성/수입

+│ │ ├── TerminalPage.tsx # 터미널 페이지 (탭 지원)

+│ │ └── Settings.tsx # 앱 설정

+│ │

+│ ├── components/ # 재사용 컴포넌트

+│ │ ├── ui/ # shadcn/ui 컴포넌트

+│ │ │ ├── button.tsx

+│ │ │ ├── input.tsx

+│ │ │ ├── dialog.tsx

+│ │ │ ├── select.tsx

+│ │ │ ├── textarea.tsx

+│ │ │ ├── card.tsx

+│ │ │ ├── badge.tsx

+│ │ │ ├── tabs.tsx

+│ │ │ ├── toast.tsx

+│ │ │ └── command.tsx # 커맨드 팔레트

+│ │ ├── Sidebar.tsx # 사이드바 네비게이션

+│ │ ├── ServerCard.tsx # 서버 카드 (연결 버튼 포함)

+│ │ ├── KeyCard.tsx # SSH 키 카드

+│ │ ├── TerminalView.tsx # xterm.js 래퍼

+│ │ ├── PasswordModal.tsx # 비밀번호 입력 모달

+│ │ └── ConfirmDialog.tsx # 확인 다이얼로그

+│ │

+│ ├── hooks/ # 커스텀 훅

+│ │ ├── useServers.ts # 서버 CRUD (TanStack Query)

+│ │ ├── useKeys.ts # SSH 키 관리

+│ │ ├── useSshConfig.ts # ssh config 관리

+│ │ ├── useTerminal.ts # 터미널 세션 관리

+│ │ └── useToast.ts # 토스트 알림

+│ │

+│ ├── types/ # TypeScript 타입

+│ │ ├── server.ts # Server, AuthType 등

+│ │ ├── key.ts # SshKey 등

+│ │ └── terminal.ts # TerminalSession 등

+│ │

+│ ├── lib/ # 유틸리티

+│ │ ├── tauriCommands.ts # Tauri IPC 호출 헬퍼

+│ │ └── utils.ts # cn() 등 공통 유틸

+│ │

+│ └── contexts/ # React Context

+│ └── TerminalContext.tsx # 터미널 세션 컨텍스트

+│

+└── src-tauri/ # Rust 백엔드

\+ ├── Cargo.toml # Rust 의존성

\+ ├── Cargo.lock # 의존성 잠금

\+ ├── tauri.conf.json # Tauri 설정

\+ ├── build.rs # 빌드 스크립트

\+ │

\+ ├── src/

\+ │ ├── main.rs # Tauri 진입점

\+ │ ├── lib.rs # 명령어 등록

\+ │ │

\+ │ ├── commands/ # Tauri IPC 명령어

\+ │ │ ├── mod.rs # 모듈匯總

\+ │ │ ├── server.rs # 서버 CRUD 명령

\+ │ │ ├── key.rs # SSH 키 관리 명령

\+ │ │ ├── ssh_config.rs # ~/.ssh/config 조작

\+ │ │ └── terminal.rs # SSH 세션 관리 (pty)

\+ │ │

\+ │ ├── models/ # 데이터 모델

\+ │ │ ├── mod.rs

\+ │ │ ├── server.rs # Server 구조체

\+ │ │ └── key.rs # SshKey 구조체

\+ │ │

\+ │ ├── db/ # 데이터베이스

\+ │ │ └── mod.rs # SQLite 초기화/쿼리

\+ │ │

\+ │ └── ssh/ # SSH 관련 로직

\+ │ ├── config_parser.rs # ssh config 파서

\+ │ ├── config_writer.rs # ssh config 작성기

\+ │ └── session.rs # PTY 기반 SSH 세션

\+ │

\+ ├── capabilities/

\+ │ └── default.json # Tauri capabilities

\+ │

\+ └── icons/

\+ ├── icon.png # 앱 아이콘 (1024x1024)

\+ └── icon.icns # macOS 아이콘

\+

+```

\+

+---

\+

+## 📦 Frontend 의존성 (package.json)

\+

+```json

+{

\+ "name": "connectunnel",

\+ "version": "0.1.0",

\+ "private": true,

\+ "scripts": {

\+ "dev": "vite",

\+ "build": "tsc && vite build",

\+ "preview": "vite preview",

\+ "tauri": "tauri"

\+ },

\+ "dependencies": {

\+ "react": "^19.2.0",

\+ "react-dom": "^19.2.0",

\+ "react-router-dom": "^7.1.0",

\+ "@tauri-apps/api": "^2.3.0",

\+ "@tanstack/react-query": "^5.66.0",

\+ "xterm": "^5.3.0",

\+ "xterm-addon-fit": "^0.8.0",

\+ "xterm-addon-web-links": "^0.6.0",

\+ "lucide-react": "^0.475.0",

\+ "zod": "^3.24.0",

\+ "class-variance-authority": "^0.7.1",

\+ "clsx": "^2.1.0",

\+ "tailwind-merge": "^2.6.0",

\+ "tailwindcss-animate": "^1.0.7"

\+ },

\+ "devDependencies": {

\+ "@tauri-apps/cli": "^2.3.0",

\+ "@types/node": "^22.13.0",

\+ "@types/react": "^19.2.0",

\+ "@types/react-dom": "^19.2.0",

\+ "@vitejs/plugin-react": "^4.3.0",

\+ "autoprefixer": "^10.4.20",

\+ "postcss": "^8.5.0",

\+ "tailwindcss": "^3.4.17",

\+ "typescript": "^5.7.0",

\+ "vite": "^6.2.0"

\+ }

+}

+```

\+

+---

\+

+## 🦀 Rust 백엔드 의존성 (Cargo.toml)

\+

+```toml

+[package]

+name = "connectunnel"

+version = "0.1.0"

+edition = "2021"

\+

+[dependencies]

+tauri = { version = "2.3", features = ["shell-open"] }

+tauri-plugin-shell = "2"

+tauri-plugin-sql = { version = "2", features = ["sqlite"] }

+tauri-plugin-fs = "2"

+tauri-plugin-dialog = "2"

+tauri-plugin-store = "2"

+tauri-plugin-notification = "2"

\+

+ssh-key = "0.6"

+ssh-encoding = "0.1"

+tokio = { version = "1", features = ["full"] }

+tokio-pty = "0.7"

\+

+serde = { version = "1", features = ["derive"] }

+serde_json = "1"

+rusqlite = { version = "0.32", features = ["bundled"] }

+bcrypt = "0.15"

+rand = "0.8"

+dirs = "5"

+lazy_static = "1"

+```

\+

+---

\+

+## 🗄️ 데이터베이스 스키마

\+

+### servers 테이블

\+

+```sql

+CREATE TABLE servers (

\+ id INTEGER PRIMARY KEY AUTOINCREMENT,

\+ name TEXT NOT NULL,

\+ host TEXT NOT NULL,

\+ port INTEGER DEFAULT 22,

\+ username TEXT NOT NULL,

\+ auth_type TEXT NOT NULL DEFAULT 'key' CHECK(auth_type IN ('key', 'password', 'pem')),

\+ key_id INTEGER, -- ssh_keys.id 참조 (key 인증)

\+ pem_data TEXT, -- PEM 데이터 base64 (pem 인증)

\+ password_hash TEXT, -- bcrypt 해시 (password 인증)

\+ password_saved INTEGER DEFAULT 0,-- 비밀번호 저장 여부

\+ group_name TEXT DEFAULT '',

\+ tags TEXT DEFAULT '[]', -- JSON 배열

\+ is_favorite INTEGER DEFAULT 0,

\+ notes TEXT DEFAULT '',

\+ created_at DATETIME DEFAULT CURRENT_TIMESTAMP,

\+ updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,

\+ FOREIGN KEY (key_id) REFERENCES ssh_keys(id) ON DELETE SET NULL

+);

\+

+CREATE INDEX idx_servers_group ON servers(group_name);

+CREATE INDEX idx_servers_favorite ON servers(is_favorite);

+CREATE INDEX idx_servers_name ON servers(name);

+```

\+

+### ssh_keys 테이블

\+

+```sql

+CREATE TABLE ssh_keys (

\+ id INTEGER PRIMARY KEY AUTOINCREMENT,

\+ name TEXT NOT NULL,

\+ public_key TEXT NOT NULL,

\+ private_key_encrypted BLOB NOT NULL, -- AES-256 암호화

\+ pem_data TEXT, -- PEM 원본 (base64)

\+ key_type TEXT NOT NULL CHECK(key_type IN ('ed25519', 'rsa', 'ecdsa', 'dsa')),

\+ key_size INTEGER DEFAULT 4096, -- RSA 키 사이즈 (2048/4096)

\+ passphrase_protected INTEGER DEFAULT 0,

\+ created_at DATETIME DEFAULT CURRENT_TIMESTAMP

+);

\+

+CREATE INDEX idx_keys_type ON ssh_keys(key_type);

+```

\+

+### ssh_config_entries 테이블 (~/.ssh/config 캐시)

\+

+```sql

+CREATE TABLE ssh_config_entries (

\+ id INTEGER PRIMARY KEY AUTOINCREMENT,

\+ host_pattern TEXT NOT NULL,

\+ hostname TEXT,

\+ user TEXT,

\+ port INTEGER,

\+ identity_file TEXT,

\+ forward_agent TEXT,

\+ local_forward TEXT, -- JSON: LocalForward 목록

\+ remote_forward TEXT, -- JSON: RemoteForward 목록

\+ other_options TEXT, -- JSON: 기타 옵션

\+ synced INTEGER DEFAULT 1

+);

\+

+CREATE INDEX idx_config_host ON ssh_config_entries(host_pattern);

+```

\+

+---

\+

+## 🔐 인증 방식 상세

\+

+### 1. SSH Key 인증

+- 저장된 private key 파일 경로를 사용

+- `~/.ssh/config`의 IdentityFile과 동기화

+- passphrase가 설정된 키는 macOS Keychain에서 자동 처리

\+

+### 2. Password 인증

+- 서버 연결 시 비밀번호가 필요하면:

\+ 1. 저장된 비밀번호가 있으면 자동 입력

\+ 2. 없으면 UI 모달에서 입력 요청

\+ 3. "비밀번호 저장" 체크박스로 선택적 저장 (bcrypt 해시)

+- PTY 출력에서 "password:" 패턴 감지하여 트리거

\+

+### 3. PEM 인증

+- PEM 파일 내용을 SQLite에 저장

+- 임시 파일로 작성 후 ssh -i 옵션으로 사용

+- 연결 후 임시 파일 삭제

\+

+---

\+

+## 🖥️ UI/UX 설계

\+

+### 레이아웃

\+

+```

+┌──────────────────────────────────────────────────────────────┐

+│ 🔗 Connectunnel [_] [□] [X] │

+├────────────┬─────────────────────────────────────────────────┤

+│ SIDEBAR │ MAIN CONTENT AREA │

+│ │ │

+│ 🏠 홈 │ ┌───────────────────────────────────────────┐ │

+│ ⭐ 즐겨찾기│ │ 최근 연결 │ │

+│ 🖥️ 서버 │ ┌─────────────────────────────────────────┐│ │

+│ 🔑 키 │ │ WebServer 192.168.1.100 ││ │

+│ ⚙️ 설정 │ │ user@web [연결] [편집] [삭제] ││ │

+│ │ ├─────────────────────────────────────────┤│ │

+│ │ │ DBServer 10.0.0.50 ││ │

+│ │ │ admin@db [연결] [편집] [삭제] ││ │

+│ │ └─────────────────────────────────────────┘│ │

+│ │ [＋ 서버 추가] │ │

+│ │ └───────────────────────────────────────────┘ │

+└────────────┴─────────────────────────────────────────────────┘

+```

\+

+### 터미널 화면

\+

+```

+┌──────────────────────────────────────────────────────────────┐

+│ 🔗 Connectunnel [_] [□] [X] │

+├──────────────────────────────────────────────────────────────┤

+│ [＋ 새 탭] │

+├──────────────────────────────────────────────────────────────┤

+│ [x] WebServer - user@192.168.1.100 [분리] [로그] [종료] │

+├──────────────────────────────────────────────────────────────┤

+│ $ │

+│ Last login: Mon Jun 12 10:30:00 2026 from 192.168.1.1 │

+│ $ █ │

+│ │

+│ │

+│ │

+│ (xterm.js 렌더링 영역 - 전체 높이 사용) │

+│ │

+└──────────────────────────────────────────────────────────────┘

+```

\+

+### 비밀번호 입력 모달

\+

+```

+┌─────────────────────────────────────┐

+│ 비밀번호 입력 │

+│ │

+│ WebServer에 연결하려면 │

+│ 비밀번호가 필요합니다. │

+│ │

+│ [_____________________________] │

+│ 비밀번호 │

+│ │

+│ ☐ 비밀번호 저장 (암호화) │

+│ │

+│ [취소] [연결] │

+└─────────────────────────────────────┘

+```

\+

+---

\+

+## 📝 개발 단계

\+

+### Phase 1: 기반 설정

+- [x] 계획서 작성

+- [ ] Tauri v2.3 + React 19.2 + Vite 6 프로젝트 초기화

+- [ ] Tailwind CSS + shadcn/ui 설정

+- [ ] SQLite 데이터베이스 스키마 생성

\+

+### Phase 2: 서버 관리

+- [ ] 서버 CRUD API (Rust 명령어 + React 훅)

+- [ ] 서버 목록 페이지 (검색, 필터, 정렬)

+- [ ] 서버 추가/수정 폼 (Zod 유효성 검증)

+- [ ] 즐겨찾기, 그룹 기능

\+

+### Phase 3: SSH Config

+- [ ] ~/.ssh/config 파서 구현

+- [ ] config → 서버 목록 동기화

+- [ ] 서버 → config 업데이트

+- [ ] config 직접 편집 모드

\+

+### Phase 4: SSH Key 관리

+- [ ] 키 생성 (Ed25519/RSA/ECDSA)

+- [ ] 키 수입 (텍스트/파일 업로드)

+- [ ] 키 암호화 저장 (AES-256)

+- [ ] macOS Keychain 연동

\+

+### Phase 5: 터미널

+- [ ] xterm.js + FitAddon 설정

+- [ ] PTY 기반 SSH 세션 (tokio-pty)

+- [ ] 비밀번호 자동 입력

+- [ ] 비밀번호 모달 (UI에서 입력)

+- [ ] 다중 탭 지원

\+

+### Phase 6: Polish

+- [ ] 다크모드 지원

+- [ ] 키보드 단축키 (Cmd+P 커맨드 팔레트)

+- [ ] 설정 페이지

+- [ ] 앱 아이콘 생성

+- [ ] 빌드 설정 (macOS/Windows/Linux)

\+

+---

\+

+## 📝 참고 사항

\+

+- Tauri v2는 React 19와 호환됨 (DOM API 변경 사항 대응 필요)

+- macOS에서는 `ssh-add`를 통해 Keychain과 연동 가능

+- Windows에서는 OpenSSH Client (Win10+) 또는 WSL 사용

+- Linux는 시스템 ssh를 그대로 사용
