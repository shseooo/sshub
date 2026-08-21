# 터미널 서브시스템 설계 (Zed 모방)

⚠️ 정정: 원 설계의 `gpui_platform::application()` 언급은 zed main 기준 —
crates.io gpui 0.2.2에는 `gpui_platform`이 없다. 진입점은 `gpui::Application`.
Element trait 시그니처(`inspector_id` 파라미터 유무 등)는 docs.rs/gpui/0.2.2 로 구현 시 재확인.
Zed 소스 레퍼런스는 태그 `gpui-v0.2.2` 고정.

## 1. 결정: upstream alacritty_terminal 0.26.0 + seam

- crates.io `alacritty_terminal = "0.26"` 사용. 필요한 것 전부 공개 API:
  `Term<T: EventListener>`, `term::Config`, `event::{Event,EventListener,WindowSize}`,
  `event_loop::{EventLoop,Msg,Notifier}`, `sync::FairMutex`, `tty::{new,Options,Shell}`,
  `Selection/SelectionType`, `RegexSearch`, grid 순회, `vte::ansi::Processor`(로컬 주입).
- 모든 alacritty 타입은 `sshub-terminal/src/backend.rs` 한 파일에만 import (Zed의
  `crate::alacritty` seam 모방). 문제 시 Cargo 한 줄로 Zed 포크(rev 4c12966) 스왑.
- Phase 0 스파이크: 헤드리스로 $SHELL spawn → EventLoop 펌프 → grid에 프롬프트 확인.
- Ctrl-C 시그널 마스크: PTY는 **메인 스레드에서 spawn** (Zed 포크의 child_signal_mask 불필요).

## 2. 모듈

```
crates/sshub-splits/src/lib.rs        # 순수 분할 트리 + 탭 연산 + 단위테스트 (gpui 의존 없음)
crates/sshub-terminal/src/
  lib.rs          # Terminal, TerminalBuilder, Event, TerminalContent
  backend.rs      # 유일한 alacritty import 지점
  pty_info.rs     # fg pid + cwd (libproc; lsof 폴백)
  mappings/keys.rs mappings/mouse.rs   # Zed 포팅 (alt-as-meta; kitty 프로토콜 없음)
  scrollback.rs   # grid→ANSI 직렬화 + feed 복원
  hyperlinks.rs   # URL 정규식 + PATH_RE (src/lib/filePaths.ts 포팅)
  selection_util.rs  # trim_selection_trailing (행별 /[ \t]+(\r?)$/ → $1)
  search.rs       # RegexSearch 래퍼
crates/sshub/src/
  session.rs workspace.rs tab_bar.rs split_view.rs
  terminal_view.rs terminal_element.rs
  scrollback_store.rs cwd_store.rs window_state.rs
```

## 3. 핵심 구조

```rust
pub struct SpawnSpec { program, args, cwd, env /*TERM=xterm-256color*/,
    banner: Option<String> /*로컬 주입*/, initial_size /*80x24*/ }

pub struct Terminal {
    term: Arc<FairMutex<Term<SshubListener>>>, notifier: Notifier,
    pty_info: PtyProcessInfo, pub last_content: TerminalContent,
    selection_phase, scroll_px: Pixels, hovered_hyperlink,
    matches: Vec<RangeInclusive<AlacPoint>>, hydrated: bool,
}
pub enum Event { Wakeup, Bell, TitleChanged(String), CloseTerminal, Open(..), SelectionsChanged }

impl Terminal {
    pub fn input(&mut self, bytes: Vec<u8>);
    pub fn try_keystroke(&mut self, ks: &Keystroke, alt_is_meta: bool) -> bool;
    pub fn set_size(&mut self, bounds: TerminalBounds);
    pub fn sync(&mut self, cx: &mut Context<Self>);      // 프레임당 1회 lock → last_content
    pub fn mouse_down/drag/up/scroll_wheel(..);
    pub fn copy(&mut self) -> Option<String>;            // trim 적용
    pub fn find_matches(..) -> Vec<..>;
    pub fn inject_local(&mut self, ansi: &[u8]);         // 배너/스크롤백 복원 — PTY로 안 감
    pub fn serialize_scrollback(&self, max_lines: usize) -> String;
}
```

이벤트 흐름: pty fd → alacritty EventLoop(전용 OS 스레드) → SshubListener → unbounded 채널 →
GPUI background task가 배치 드레인 → `terminal.update(cx)` → Wakeup이면 `cx.notify()` →
프레임당 1회 렌더. Electron의 8ms 코얼레싱은 이 배치 드레인+프레임 제한으로 자연 대체.

키보드: focused view의 on_key_down → `to_esc_str(keystroke, mode, alt_is_meta)` →
input(); 입력 시 하단 스크롤 + 선택 해제.

## 4. TerminalElement (gpui::Element)

- request_layout: 부모 채움.
- prepaint: (1) "m" shape로 cell_width, line_height = font_size*1.4;
  cols/rows = floor → 변경 시 set_size (FitAddon 대체, 첫 레이아웃이 hydration 트리거)
  (2) sync → LayoutState { bg rects(동일색 병합), batched_text, selection/match rects, cursor, hitbox }.
- **CJK 정렬 규칙 (필수)**: (fg,flags,underline) 동일한 연속 셀을 배치로 묶되 각 배치는
  **그리드 원점 (line, start_col)** 에 페인트 — shaped advance를 배치 간 누적하지 않음.
  WIDE_CHAR_SPACER 셀은 방출 생략. 골든 테스트: `가나다 abc 漢字` 배치 원점 = col × cell_width.
- paint: 쿼드(배경→검색→선택) → ShapedLine per 배치 → 커서(자체 CursorLayout ~80 LOC 재구현:
  Block=채운 쿼드+글자 재페인트, Bar=2px, Underline, 비포커스=hollow 4쿼드) →
  마우스 리스너 등록 + `window.handle_input(&focus, ElementInputHandler::new(bounds, view), cx)`.

### IME (한글/일본어 — 최우선 품질 기준)

`TerminalView`에 `EntityInputHandler` 구현: marked_text는 PTY로 절대 보내지 않고
(`replace_text_in_range`가 커밋 시점에만 input()), 엘리먼트가 커서 그리드 위치에
오버레이로 페인트(터미널 폰트, 두꺼운 밑줄, 배경=터미널 bg, 조합 중 블록 커서 숨김).
`bounds_for_range`가 후보창 위치 제공. Chromium 팬텀 IME 가드는 **포팅하지 않음**(DOM 전용 버그).

### 마우스/스크롤/링크/검색

- mappings/mouse.rs 포팅: SGR/Normal 리포트, MOUSE_MODE ∧ !shift → 리포트, 아니면 로컬 선택
  (클릭=Simple/더블=Semantic/트리플=Lines, 드래그 엣지 자동 스크롤).
- 휠: 픽셀 누적 → line_height 나눔 (Zed 방식).
- 복사: cmd-c → selection_text → trim → clipboard. 붙여넣기: bracketed-paste.
- 링크: cmd-호버에 URL 정규식+PATH_RE+OSC-8 → 밑줄; cmd+클릭 URL → cx.open_url;
  절대경로(로컬 세션, 존재 확인) → Finder (`cx.reveal_path` 없으면 `open -R`).
- 검색: pane별 검색바, RegexSearch는 background executor, 매치 rect 하이라이트, enter/shift-enter 순환.

## 5. sshub-splits (순수 — TS 시맨틱 정확 포팅 + 단위테스트)

```rust
pub enum SplitDirection { Row, Column }
pub enum PaneNode { Leaf(TerminalLeaf), Split(TerminalSplit) }
pub struct TerminalLeaf  { session_id, server_id: Option<i64>, label, cwd_from_session: Option<SessionId> }
pub struct TerminalSplit { id, direction, children: Vec<PaneNode>, sizes: Vec<f32> /*% ≈100*/ }
pub struct TerminalTab   { id, root: PaneNode, name: Option<String> }
pub fn tab_title(tab) -> &str;  // name ?? 첫 leaf label

pub fn leaves / split_at / remove_leaf / insert_at / reconnect_leaf / rename_leaf / set_split_sizes;
// split_at: leaf==target → 중첩 Split[50,50]; 같은 방향 split의 직계 자식 → idx+1 삽입+even(n);
// remove_leaf: 마지막 leaf → None; 단일 자식 split collapse; 생존자 재균등
// insert_at: split_at 동형이나 add=서브트리, before로 좌/우 (left/top⇒before, left/right⇒Row, top/bottom⇒Column)
pub fn reorder_tabs(tabs, id, to_index);  // 원본 배열 기준 boundary, from<to → to-1, clamp
pub fn tabs_except / tabs_up_to_inclusive / insert_at_index;  // tabOps.ts 포팅
```

워크스페이스 합성 연산 (TerminalContext.tsx 그대로): move_pane(유일 pane이면 중단),
detach_pane(leaves>1 필수, boundary 삽입), merge_tab(root 전체 graft), reconnect(새 세션 id,
구 PTY kill + 스크롤백 삭제), 탭 닫힘 시 active 폴백 = 마지막 탭.

디바이더: sizes 기반 flex 영역 + 5px 히트박스(ColResize/RowResize), 드래그 px→% 변환, 자식 ≥5% clamp.

## 6. 세션 (session.rs)

- SessionRegistry: HashMap<SessionId, Entity<Terminal>> — **앱 스코프** (창 아님).
  kill-before-respawn 가드.
- 로컬: $SHELL||/bin/zsh + ["-l"]; cwd: ① cwd_from 라이브 로컬 세션 →
  libproc `proc_pidinfo(PROC_PIDVNODEPATHINFO)` (lsof 폴백) ② 저장된 CwdStore.get(id)
  ③ $HOME. SSH는 항상 로컬 $HOME.
- SSH: "ssh" + build_ssh_args + 배너 inject_local (PTY에 안 씀).
- 종료 순서 (필수): 라이브 로컬 cwd 스냅샷 완료 → 스크롤백 flush → Msg::Shutdown → quit.
- **브로드캐스트** (기존 기능): Workspace.broadcast_tabs: HashSet<TabId>; 활성 시 키/IME 커밋/붙여넣기
  바이트를 해당 탭 전 leaf에 복제 (포커스 pane이 커서/IME 소유). 표시: 각 pane 어센트 2px 내부 보더 + 탭 배지.
- 닫기 확인: risky = leaves>1 ∨ server_id 존재 → 모달; close-others(탭>1), close-right(우측 존재)도.

## 7. 스크롤백 영속화 — 결정: grid→ANSI 직렬화 (raw ring 아님)

- 이유: 기존 파일과 개념 호환, 유계·결정적(raw ring은 alt-screen 쓰레기 재생), 복원은
  PTY spawn 전 `inject_local` feed로 끝.
- serialize(term, max_lines=1000): 마지막 N행(history+screen), 최소 SGR run 방출,
  WRAPLINE 존중(soft wrap은 연속), 행말 공백 셀 제거.
- 라이브 히스토리: `Config.scrolling_history = 20_000`; 영속 캡 1_000 유지.
- 디바운스: Wakeup당 1500ms 타이머 재장전 → background에서 serialize → save.
- no-clobber: `hydrated` 플래그 — 복원 주입 완료(또는 신규 세션) 후 true;
  종료 flush는 !hydrated 스킵. hydration은 첫 실제 레이아웃(가시+크기)에.
- 파일: sshub_scrollback/<sanitized>.txt, dir 0700/파일 0600, prune(live_ids) 시작 시.

## 8. 다중 창 (신규 기능)

| 전역 (App) | 창별 (Workspace 루트 뷰) |
|---|---|
| 서버/키 store, settings/테마 | tabs, active_tab |
| SessionRegistry (Entity<Terminal> 전부) | 포커스 pane |
| ScrollbackStore, CwdStore, WindowStateStore | 브로드캐스트 셋, 검색 UI |

- `cx.open_window(WindowOptions{..}, |window,cx| cx.new(|cx| Workspace::new(tabs,window,cx)))`.
- 액션: NewWindow, **MoveTabToNewWindow** — Terminal 엔티티가 앱 스코프라 PTY/grid 무손실 이동
  (terminalPool DOM 재부모화의 Rust 등가).
- 영속화: `Vec<WindowRecord{bounds, display_id, tabs: Vec<SavedTab>, active_tab}>` —
  SavedNode 형태 기존 유지(sessionId 보존). 시작 시 레코드별 창 복원.
- macOS: last-window-closed는 종료 아님(현행 유지), Dock 재활성화로 창 복원
  (`cx.on_reopen` — 스파이크에서 확인), cmd-Q → 종료 순서(§6).
- 창 닫기 = 그 창의 탭 닫기(위험 확인 통합).

## 9. 빌드 순서

0. 스파이크: 헤드리스 alacritty 0.26 검증 + gpui 0.2.2 API 확인(cargo doc).
1. 화면 에코 (fg 텍스트만, raw 키 passthrough) ← 마일스톤.
2. 렌더 완성(색/배경/커서/와이드/리사이즈/휠).
3. to_esc_str + IME(한글 게이트).
4. 선택/복사/붙여넣기/마우스 리포트/링크.
5. splits(순수+테스트) + split_view/tab_bar/드래그.
6. 세션(ssh/배너/cwd/재연결/확인/브로드캐스트).
7. 스크롤백 + 검색 UI. 8. 다중 창 + 종료 수명주기.

## 10. 리스크

- gpui 0.2.2 vs Zed 내부 드리프트(최대): docs.rs 시그니처만 신뢰, zed는 gpui-v0.2.2 태그로만 참조,
  작은 헬퍼(CursorLayout, min-contrast)는 재구현.
- alacritty upstream 격차: backend.rs로 격리, 한 줄 스왑.
- FairMutex 경합: prepaint 프레임당 1 lock, 직렬화는 background+Arc 클론.

---

## 11. 구현 시 확인된 API 편차 (2026-08-22, 터미널 엔진 구현)

설계 작성 시점의 가정과 실제 `gpui 0.2.2` / `alacritty_terminal 0.26.0` 로컬
소스가 달랐던 지점. **레지스트리 소스를 직접 읽어 확정한 값**이다.

### alacritty_terminal 0.26.0

| 설계/가정 | 실제 |
|---|---|
| `Selection::simple()/semantic()/lines()` | 없음. `Selection::new(SelectionType, Point, Side)` 하나뿐 (doc 주석이 낡음). |
| `Term::bell()` / `Term::reset_state()` 가 고유 메서드 | 아니다. `vte::ansi::Handler` 트레이트 메서드 — 호출하려면 `use ...vte::ansi::Handler`. |
| `Term::vi_mode()` 게터 | 없음. `term.mode().contains(TermMode::VI)`. |
| `Config.kitty_keyboard_protocol` | 필드명은 `kitty_keyboard`. |
| `Processor::advance_until_terminated` | `Processor`에는 없다(`vte::Parser`에만). 로컬 주입은 `Processor::advance(&mut term, bytes)`. |
| `Processor::new()` 로 바로 사용 | 타입 파라미터 `T: Timeout`이 **기본값이 있어도 추론되지 않는다**. `backend::AnsiProcessor = Processor<StdSyncHandler>` 별칭을 두었다. |
| `Rgb`가 튜플 구조체 | 이름 있는 필드 `Rgb { r, g, b }`. |
| `term::test::TermSize` 재사용 | 테스트 전용이라 못 쓴다 → `backend::TermSize`를 직접 정의(최소 2열/1행 클램프 포함). |
| `Event`에 `ChildExit` 없음 | 있다 — `ChildExit(ExitStatus)`. `Exit`와 함께 `CloseTerminal`로 접었다. |

`alacritty_terminal::vte`는 `lib.rs:20`에서 **정말로 재수출**되므로
`inject_local`은 설계대로 PTY 우회 주입이 가능하다 (`Term<T>: vte::ansi::Handler`).

### gpui 0.2.2

| 설계/가정 | 실제 |
|---|---|
| `Element::request_layout(id: Option<&GlobalElementId>, ...)` 만 | `inspector_id: Option<&InspectorElementId>` 파라미터가 **추가로** 있다(3개 메서드 전부). |
| `Pixels(pub f32)` | 필드가 `pub(crate)`. `f32::from(px)` 로 꺼낸다 (`sshub-terminal::fpx`). |
| `ClickEvent`가 구조체 | 0.2.2에서는 **enum** (`Mouse`/`Keyboard`). 클릭 횟수는 `MouseDownEvent.click_count`에서 직접 읽었다. |
| `cx.reveal_path` 없을 수 있음 → `open -R` 서브프로세스 | **`App::reveal_path(&Path)` 가 존재한다.** 서브프로세스 폴백은 불필요 — 쓰지 않았다. |
| `RenderableCursor: Debug` | 구현 안 함 → `TerminalContent`에서 `Debug` derive 제거. |
| `shape_line` 임의 텍스트 | `\n` 포함 시 `debug_assert!` — 행 단위로만 shape한다. |
| `TextRun.len` = 문자 수 | **바이트 길이**. IME 범위는 UTF-16 → 경계에서 명시 변환(`offset_from_utf16`/`offset_to_utf16`). |

페이즈 제약(런타임 assert)도 확인됨: `insert_hitbox`/`request_layout`은 prepaint,
`paint_quad`/`handle_input`/`on_mouse_event`/`set_cursor_style`는 paint 전용.

### 설계에서 의도적으로 조정한 것

- **배치 분할 규칙 강화**: 설계는 "(fg, flags, underline) 동일 셀 묶기"였으나,
  와이드/내로우가 한 배치에 섞이면 배치 **안쪽**에서 advance 드리프트가 남는다.
  `wide` 플래그를 배치 키에 넣어 CJK 런과 ASCII 런이 절대 같은 배치에 들어가지
  않게 했고, 공백 셀에서도 배치를 끊는다. 골든 테스트
  `cjk_batches_start_at_their_grid_column`이 `가나다 abc 漢字` → 배치 원점
  `(0, 7, 11)`을 고정한다.
- **커서 색 대비**: 설계의 "min-contrast 헬퍼"는 아직 없다. 블록 커서는
  액센트색 채움 + 글자를 **터미널 배경색**으로 덮어 그려 대비를 확보한다.
- **`Terminal::mouse_up`이 `cx`를 받는다**: 선택 확정 시
  `Event::SelectionsChanged`를 emit해야 해서 시그니처에 `&mut Context<Self>`가
  붙었다 (설계 §3 스케치와 다름).

### 아직 구현되지 않은 것 (이 작업 범위 밖)

- OSC 52 클립보드(`ClipboardStore`/`ClipboardLoad`)는 수신만 하고 무시한다.
- 드래그가 화면 밖으로 나갈 때의 **엣지 자동 스크롤**(§4).
- 검색 **UI**(검색바·enter/shift-enter 순환). 모델(`search::SearchQuery`,
  `Terminal::set_matches`)과 매치 rect 렌더링은 준비되어 있다.
- 스크롤백 **디바운스 저장**(1500ms)과 `hydrated` 게이트를 실제로 소비하는
  세션 계층 (`Terminal::hydrated`/`serialize_scrollback_for_disk`는 노출됨).
- 브로드캐스트, 분할/탭 UI, 다중 창 (§5·§6·§8).
