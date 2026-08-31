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

### 8.1 창을 넘나드는 탭 드래그 (구현 노트, 2026-08-31)

탭을 창 밖으로 끌면 새 창이 되고, 다른 창 위에 놓으면 그 창으로 옮겨진다.
**gpui의 `on_drop`으로는 둘 다 불가능하다** — 그래서 드래그를 시작한 창이
mouse-up을 직접 받아 처리한다:

- macOS는 마우스를 누른 창이 버튼을 뗄 때까지 이벤트를 독점한다(implicit
  capture). 목적 창은 커서가 자기 위에 있다는 사실조차 모르고
  (`Window::is_window_hovered`는 mac에서 `is_window_active`와 같다), 창 밖에는
  드롭 대상 요소 자체가 없다.
- `Workspace`가 캡처 페이즈 `MouseUpEvent` 리스너를 창 전체에 단다
  (`tab_drag_watcher` — 히트박스에 매이지 않도록 `canvas` 페인트에서
  `Window::on_mouse_event`로 등록). 창 밖/다른 창으로 처리했으면
  `stop_propagation`으로 버블의 `on_drop`을 막는다.
- 끌고 있던 탭 id는 `App::active_drag`가 비공개라 다시 읽을 수 없다. 탭바의
  `on_drag`가 `split_view::ActiveTabDrag` 전역에 남기고, 워처가 mouse-up마다
  지운다.
- 목적지 판정은 `WindowManager::window_at(전역 좌표)` — 창의 전역 사각형과
  **활성 순번**(z-order 근사)으로 맞힌다. 전역 좌표는 §8.3 참조.
- 세션 수명: `take_tab`(뷰만 버림) → `receive_tab`(같은 session id로 재부착).
  `SessionRegistry::close`는 절대 타지 않는다. 마지막 탭을 넘긴 소스 창은
  목적 창이 `sync_layout`을 끝낸 **뒤에** 닫는다 — 그래야 소스 창의 release
  훅이 도는 고아 세션 정리가 옮겨 간 세션을 죽이지 않는다.
- 탭이 하나뿐인 창에서 창 밖으로 끄는 것은 무시한다(이미 "그 탭의 창"이다).
- **어디에 꽂힐지 보여 준다**: 탭바에 삽입 캐럿(accent 세로선)을 세운다. 폭은
  좌우 음수 마진으로 상쇄해 레이아웃을 밀지 않는다 — 캐럿이 움직일 때마다 탭이
  2px씩 흔들리면 오히려 읽기 어렵다. 탭바를 벗어나면 감춘다(거기서는 pane 병합
  오버레이가 답이고, 둘이 같이 뜨면 더 헷갈린다). 목적 창은 드래그 중 마우스
  이벤트를 못 받으므로 **소스 창이 밀어 넣고**(`update_drop_hint`) 드래그가
  끝나거나 그 창을 벗어나면 거둔다(`clear_remote_hint`). 캐럿 위치와 실제 삽입은
  같은 함수(`tab_boundary_for`)를 쓴다 — 보인 자리와 꽂히는 자리가 다르면 표시가
  없느니만 못하다.
- 목적지 판정은 **자기 창 먼저**다(`workspace::tab_drop_target`). 드래그 중인
  창은 마우스를 누른 순간 맨 앞으로 올라오므로, 그 사각형 안이면 눈에 보이던
  것도 그 창이다. 이 순서를 뒤집으면 매니저 레코드가 한 프레임만 밀려도 **탭
  순서 변경**이 매번 새 창을 만든다 (`tests/tab_drag_reorder.rs`가 실제 마우스
  제스처로 고정한다).

### 8.2 드래그 미리보기 패널 (`drag_ghost`, 2026-08-31)

gpui는 드래그 고스트를 **드래그가 시작된 창의 씬**에 그린다. 커서가 그 창을
벗어나면 잘려 사라지는데, 하필 "창 밖으로 꺼내 새 창 만들기"가 정확히 그
상황이라 무엇을 끌고 있는지 보이지 않았다.

- gpui 0.2.2에는 **창을 옮기는 API가 없다**(`PlatformWindow`에 `resize`만
  있다). 그래서 커서를 따라 작은 창을 옮기는 대신, 디스플레이를 덮는 투명
  패널을 한 장 띄우고 그 안에서 **카드의 위치만** 바꾼다.
- 패널은 `WindowKind::PopUp`(= macOS `NSPanel` + `NonactivatingPanel`) +
  `focus: false`라 포커스를 빼앗지 않는다. 포커스를 가져가면 소스 창이
  비활성화되고 드래그가 그대로 끊긴다.
- 카드 내용은 드래그 시작 시점의 **스냅샷**(제목 + `Terminal::tail_lines`)이다.
  살아 있는 `TerminalView`를 그리면 터미널은 "그려진 크기 = `set_size`"라
  그 pane의 PTY가 카드 크기로 리사이즈된다.
- 패널이 떠 있는 동안 gpui의 창 안 기본 고스트는 그리지 않는다(`DragGhost`가
  `drag_ghost::is_active`를 본다) — 둘 다 그리면 창 안에서만 두 겹으로 보인다.
  이 표시는 패널을 열기 **전에** 세운다. 창 생성은 ObjC 마우스 콜백 밖으로
  미루므로(`cx.defer`) 그 사이 프레임과 패널의 첫 프레임에서 칩이 깜빡인다.
- **화면 전체를 덮는 창이라 남으면 클릭을 통째로 먹는다.** 소스 창이 mouse-up을
  반드시 받으므로 거기서 닫고(`tab_drag_watcher`), 그래도 못 닫는 경우를 대비해
  커서가 `SAFETY_TIMEOUT`(20초) 동안 멈춰 있으면 스스로 닫힌다(총 드래그
  시간이 아니라 **유휴** 시간 — 천천히 끄는 것은 정상이다).
- 패널은 **디스플레이마다 한 장**이고 모두 같은 전역 커서 좌표를 받는다. 한
  장으로는 카드가 그 모니터 안에 갇혀 경계에서 멈춘다(실제 신고). 카드를 가두는
  기준도 패널이 아니라 **데스크톱 전체**여야 한다. 창을 만들 때 `display_id`를
  빼먹으면 전부 주 디스플레이에 겹쳐 열린다(gpui 기본값이 primary다).

### 8.3 다중 모니터 전역 좌표 (`displays`, 2026-08-31)

gpui 0.2.2의 창 좌표는 **모니터마다 다른 공간**에 있다:

- `Window::bounds()`(mac/window.rs)는 x를 그 창이 놓인 스크린 기준으로
  상대화하고(`x - screen.origin.x`) y를 그 스크린 높이로 뒤집는다.
- `PlatformDisplay::bounds()`(mac/display.rs)는 크기만 주고 **원점을 버린다**
  (`origin: Default::default()`). 주석은 전역 좌표라고 해 놓고 0을 넣는다.

그래서 다른 모니터에 있는 두 창의 `bounds()`를 그대로 비교할 수 없다 — 탭을
다른 모니터로 끌어 분리하는 것이 정확히 그 상황이다. 디스플레이 원점만
CoreGraphics(`CGDisplayBounds`)에서 직접 가져와 전역 좌표로 올린다:

```text
전역 = Window::bounds().origin + display_origin(그 창이 놓인 디스플레이)
```

- 모르는 디스플레이에는 `CGRectNull`이 오고 그 원점은 **무한대**다. 그대로
  더하면 좌표가 통째로 무한이 되어 드롭 판정이 조용히 전부 실패한다 — 걸러서
  단일 모니터처럼 다룬다.
- **커서만은 이 환산을 쓰지 않는다.** 위 식은 "이 창이 어느 디스플레이에
  있는가"를 gpui의 **캐시된** `Window::display`에 의존하는데(window.rs가
  `display_id`를 들고 있다가 목록에서 찾는다), 그 값이 어긋나면 커서가 통째로
  원래 모니터 기준으로 나와 미리보기가 엉뚱한 모니터에 붙는다(실제 신고:
  B→A 드래그에서 카드가 마우스를 안 따라옴). 커서는 CoreGraphics
  `CGEventGetLocation`이 **전역 좌표로 직접** 준다 — 창도 디스플레이도 거치지
  않으므로 그 고리가 아예 없다(`displays::os_cursor`).
- 같은 이유로 "이 드롭이 자기 창 안인가"는 전역 사각형이 아니라 **창 좌표**
  `0..size`로 본다(`displays::is_inside`). 디스플레이 배치와 무관하게 정확하다.
- 창을 **열 때**는 반대로 내려야 한다. gpui는 좌표를 그 디스플레이 기준으로
  환산하므로, 전역 좌표에서 그 디스플레이 원점을 빼고 `display_id`와 함께
  넘긴다(`workspace::open_on` / `display_placement`).
- 저장되는 창 지오메트리(`WindowRecord.bounds`)는 **그대로 둔다** — 그 값은
  창을 열 때 쓰는 디스플레이 상대 좌표다. 전역 좌표는 화면 위에서 무언가를
  맞힐 때만 쓴다.
- **남은 한계**: 레코드에 디스플레이 정보가 없어서, 보조 모니터에 있던 창은
  다음 실행에서 주 모니터의 같은 상대 위치에 열린다. 고치려면
  `PlatformDisplay::uuid()`를 레코드에 넣어야 한다(설정 스키마 변경).

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
  → **완료**(아래 "워크스페이스 계층" 참조).
- 브로드캐스트, 분할/탭 UI, 다중 창 (§5·§6·§8).
  → 브로드캐스트·분할/탭 UI **완료**, 다중 창은 여전히 미구현.

---

## 12. 워크스페이스 계층 구현 노트 (2026-08-22, §5·§6·§7 UI)

`session_registry.rs` / `split_view.rs` / `tab_bar.rs` / `terminal_workspace.rs`
구현 중 설계와 달라진 지점.

### gpui 0.2.2 추가 확인

| 설계/가정 | 실제 |
|---|---|
| `Styled::cursor_*`에 `ColResize/RowResize` | 이름은 `CursorStyle::ResizeLeftRight` / `ResizeUpDown` (`.cursor(..)`로 지정). |
| flex 비율을 `%` 폭으로 지정 | 퍼센트 폭 + 5px 디바이더는 매 split마다 오버플로한다. `style().flex_grow = Some(pct)` + `flex_basis: 0`으로 **남은 공간**을 비율 분배한다. |
| 드롭 위치를 `on_drop` 인자에서 얻음 | `on_drop`은 페이로드만 준다. 위치는 `window.mouse_position()` + 프레임마다 `canvas`로 수집한 pane/탭 사각형(`WorkspaceGeometry`)으로 계산한다. |
| `ClickEvent`로 더블클릭 | 0.2.2에서 enum이라 `MouseDownEvent.click_count >= 2`로 판정(§11과 동일 이유). |
| 툴팁 | gpui에는 `tooltip(|_,_| AnyView)`만 있고 텍스트 툴팁 위젯이 없다 → 탭/버튼 툴팁은 생략. |

### 설계에서 의도적으로 조정한 것

- **스크롤백 직렬화 위치**: 설계는 "background에서 serialize"였으나
  `Terminal::serialize_scrollback`은 엔티티 `&self`가 필요해 메인에서 직렬화하고
  (1000행 상한, `FairMutex` 1회) **파일 쓰기만** `background_spawn`으로 보낸다.
- **`hydrated`를 세우는 곳**: `TerminalElement::prepaint`에서 첫 실제 레이아웃
  (`screen_lines() > 1`)에 `hydrated = true`. 레지스트리의 저장 경로는 이 값이
  false면 건너뛴다 — 한 번도 보이지 않은 pane의 빈 그리드가 저장된 히스토리를
  덮어쓰는 사고를 막는 유일한 게이트다.
- **브로드캐스트 팬아웃 지점**: 설계는 "워크스페이스가 키를 가로채 복제"였지만
  키·IME 확정·붙여넣기가 모두 `TerminalView`를 지나므로, 뷰에 선택적 싱크
  (`TerminalView::set_broadcast`, `BroadcastInput::{Keystroke,Text,Paste}`)를 달고
  워크스페이스가 **같은 탭의 나머지 leaf**에만 복제한다. IME 조합 중 텍스트는
  여전히 어디로도 가지 않는다(확정 시 `Text`로 1회).
- **cwd 상속원은 1회용**: `TerminalLeaf.cwd_from_session`은 세션 spawn 직후
  워크스페이스가 지운다. 남겨 두면 재연결·복원 때 남의 디렉터리를 물려받는다.
- **크로스 탭 pane 이동**: `move_pane`은 한 트리 안에서만 동작하므로 다른 탭으로
  끌면 `detach_pane`(임시 탭) → `merge_tab` 조합으로 처리한다. 결과 트리는 TS
  판과 같다.
- **pane 드래그 소스**: pane 전체를 드래그 가능하게 하면 터미널 텍스트 선택과
  충돌한다 → 우상단 14px 그립에서만 드래그가 시작된다.
- **세션 레지스트리는 전역 상태를 직접 읽지 않는다**: 서버/키 목록은
  `set_catalog`로 주입받는다(워크스페이스가 `StateEvent`에 맞춰 갱신). 덕분에
  레지스트리 단위 테스트가 `AppState`/실제 홈 디렉터리 없이 돈다.
- **레이아웃 영속 포맷**: `Settings.terminal_layout` = TS와 동일한
  `{tabs, activeIndex}`. 세션 id는 보존(스크롤백/cwd 파일 키), 탭/split id는
  `revive_ids`로 재발급. §8의 다중 창 레코드(`window_session::WindowRecord`)와
  같은 키를 쓰므로, 다중 창을 켤 때 **어느 쪽이 최종 writer인지 정리**해야 한다.

### 이 작업에서도 남은 것

- 검색 **바 UI**: `TerminalWorkspace::{set_pane_search, clear_pane_search}`와
  pane별 상태는 있으나 입력 UI와 enter/shift-enter 순환은 없다.
- 서버 선택 팝오버(`term.pickNewTab`/`pickSplitRight`/`pickSplitDown`):
  분할·새 탭은 항상 로컬 셸로 열린다. 서버 세션은
  `TerminalWorkspace::open_server_tab(server_id, label, ..)` 진입점으로만.
- pane 재연결/닫기의 **컨텍스트 메뉴**: `reconnect_pane`/`reconnect_tab`/
  `close_other_tabs`/`close_tabs_to_the_right`는 공개 메서드로 있으나 이를 부르는
  메뉴 UI가 없다(키맵에도 액션이 없다).
- 다중 창(§8)과 `cmd-Q` 종료 순서 연결: `TerminalWorkspace::shutdown` /
  `SessionRegistry::shutdown_all`(cwd 스냅샷 → flush → kill)은 준비됐지만
  앱 라이프사이클에 아직 연결되지 않았다.
