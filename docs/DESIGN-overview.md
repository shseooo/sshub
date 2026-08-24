# sshub Rust 재작성 — 개요

sshub(Electron + React)를 Rust 네이티브 GPUI 앱으로 전환한다. 터미널은 Zed의 구현을
모방(alacritty_terminal + 커스텀 GPUI Element). 기존 기능 전부 이식 + 신규: 다중 창.

## 확정 결정

1. 순수 GPUI 직접 구현 (gpui-component 미사용) — 모든 위젯 자작.
2. 레포 루트가 곧 Cargo 워크스페이스다 (Electron 코드는 이전 완료 후 삭제 —
   필요하면 `git log`에서 되찾는다).
3. Zed풍 미니멀 다크 디자인 (CRT/phosphor 룩 폐기, 반투명 설정은 유지).

## 검증된 스택 (2026-08-21 crates.io 확인)

- `gpui = "0.2.2"` — crates.io 최신 (2025-10-22). `gpui_platform`은 **crates.io에 없음**
  (zed main 전용). 진입점은 `gpui::Application::new().run(...)`.
  레퍼런스 소스는 zed 태그 `gpui-v0.2.2` (커밋 69e2130295c2649963eb639fc70b4f2ee8ea1624)
  기준으로 읽을 것 — zed main의 예제·terminal 코드는 0.2.2와 API가 다름.
- `alacritty_terminal = "0.26.0"` — upstream crates.io 사용 (Zed 포크 아님).
  단, 모든 alacritty 타입은 `backend.rs` seam 모듈에 격리해 필요 시
  `git = "https://github.com/zed-industries/alacritty", rev = "4c129667ce56611becdc82de6e28218c80e2e88f"`
  로 한 줄 스왑 가능하게 유지.
- Rust 1.96 stable, macOS CommandLineTools.

## 워크스페이스

```
.
├── Cargo.toml                  # workspace
├── docs/                       # 이 설계 문서들
└── crates/
    ├── sshub-core/             # 순수 로직+영속성 (DESIGN-core.md)
    ├── sshub-splits/           # 분할 트리·탭 순수 연산 (DESIGN-terminal.md §5)
    ├── sshub-terminal/         # alacritty 래핑 터미널 모델 (DESIGN-terminal.md)
    └── sshub/                  # GPUI 앱 (DESIGN-ui.md + terminal element)
```

## 데이터 호환 (절대 규칙)

기존 Electron 사용자가 무중단 업그레이드해야 한다. 경로·포맷·권한·에러 문자열을
바이트 수준으로 유지 (상세는 DESIGN-core.md §6 체크리스트):
- `~/Library/Application Support/sshub.json` (appData 바로 아래, sshub/ 하위 아님)
- `ssh_keys/`, `sshub_scrollback/`, `sshub_terminal_cwd.json`, `sshub_window.json`
- 백업 envelope `sshub-enc-v1` (scrypt 14/8/1/32 + AES-256-GCM) — Node↔Rust 상호 복호화
- localStorage 설정만 클린 스타트 (신규 `sshub_settings.json` v1)

## 빌드/검증 명령

```bash
cargo build && cargo test          # 전 크레이트
cargo run -p sshub                            # 앱 실행
```

## 마일스톤

0. ✅ 스캐폴드 + 헤드리스 터미널 스파이크 (alacritty 0.26 API 검증)
1. ✅ sshub-core + sshub-splits (compat 픽스처 게이트 통과 — Node 산출물 바이트 일치)
2. ✅ 화면에 에코되는 터미널 (TerminalElement + terminal_demo 예제)
3. ✅ 렌더링(색/커서/와이드문자/리사이즈/스크롤) + 키·IME(한글 조합 오버레이)
4. ✅ 선택/복사/마우스/하이퍼링크 + 분할/탭 UI (split_view·tab_bar)
5. ✅ 세션(ssh/cwd 상속/배너/재연결) + 스크롤백 영속화(hydrated 가드)
6. ✅ 위젯 킷 + 페이지 5종 + 설정/단축키/i18n/테마
7. 🔄 다중 창 + 앱 셸 통합(workspace/main) + 종료 순서
8. ✅ 패키징 스크립트 (install.sh — .app 번들 + ad-hoc 서명)

미구현(후속): pane 검색 UI(모델은 있음), 새 탭/분할 시 서버 선택 팝오버,
컨텍스트 메뉴(재연결·다른 탭 닫기), OSC 52 클립보드, 드래그 엣지 자동 스크롤.

## 테스트 현황 (2026-08-22)

전체 `cargo test --workspace` 430+ 통과, 경고 0.
- sshub-core 152 + Node 호환 10 + ssh-keygen 통합 10
- sshub-splits 62 / sshub-terminal 88 + e2e 5 + 스파이크 1
- sshub(앱) 102 — 위젯·뷰·키맵·세션·분할·창 상태
