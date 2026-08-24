# GPUI 앱 셸 + 위젯 킷 설계

## 0. 검증된 gpui 0.2.2 API (docs.rs 확인 완료)

- crates.io 최신 = 0.2.2 (2025-10-22), zed 태그 `gpui-v0.2.2` = 커밋 69e2130295c2…
  **레퍼런스는 이 태그 고정** (main은 gpui_platform 분리 후라 0.2.2와 다름).
  진입점 `gpui::Application::new().run(...)`.
- `EntityInputHandler` 8메서드 + `ElementInputHandler::new(bounds, entity)` +
  `window.handle_input(&focus, …, cx)` — examples/input.rs 패턴 (IME 지원).
- `KeyBinding::new(keystrokes, action, context)`; `App::bind_keys`; **`App::clear_key_bindings()`
  존재** → 런타임 리바인딩 = 전체 clear + 재등록. `Keystroke::parse/unparse`("cmd-shift-d" 형식),
  `is_ime_in_progress`, `observe_keystrokes`, `on_keyboard_layout_change`.
- `WindowBackgroundAppearance::{Opaque,Transparent,Blurred}` + `Window::set_background_appearance`
  (런타임 토글), `TitlebarOptions{title, appears_transparent, traffic_light_position}`,
  `window_control_area`(드래그 영역), `Window::set_client_inset`.
- 오버레이: `anchored()`(snap_to_window) + `deferred()`(형제 위 페인트); `uniform_list`.
- 포커스: `tab_index/tab_stop/tab_group`, `Window::{focus_next,focus_prev}` — 폼 Tab 내비 내장.
- `ClipboardItem`, `Window::prompt`, `PathPromptOptions` 존재 — `App::prompt_for_paths/
  prompt_for_new_path`는 구현 시 docs.rs 확인, 폴백 `rfd` 크레이트.

## 1. 앱 크레이트 레이아웃

```
crates/sshub/src/
  main.rs workspace.rs titlebar.rs theme.rs settings.rs state.rs keymap.rs
  i18n/{mod.rs, generated.rs}      # generated는 scripts/gen_i18n.mjs 1회 실행 후 커밋
  ui/{mod,button,checkbox,text_input,text_area,select,modal,list,toast,form,icon}.rs
  assets.rs
  views/{sidebar,server_list,server_edit,key_manager,settings_page,terminal_host}.rs
  (+ DESIGN-terminal.md의 terminal/session 파일들)
```

## 2. 위젯 킷

- 상태 없는 리프 컨트롤 = `RenderOnce`(Button{variant: Primary|Secondary|Ghost|Danger,
  disabled, loading, on_click}, Checkbox, ListItem, FormField), 포커스/편집 상태 보유 = Entity 뷰.
- **TextInput** (최난도 — examples/input.rs 포팅): focus_handle, text, placeholder,
  selected_range(Range<usize>), selection_reversed, marked_range(IME), last_layout: ShapedLine,
  masked(passphrase). `EntityInputHandler` 8메서드 UTF-16 매핑 정확히.
  이벤트 Changed/Submitted/Blurred. 편집 액션(Backspace/Delete/화살표/SelectAll/Copy/Cut/Paste/
  Home/End ~15개)은 keymap.rs에서 context "TextInput"으로 1회 바인딩.
  TextElement(커스텀 Element): prepaint shape_line, paint에서 ElementInputHandler 등록 +
  선택/마킹 밑줄 쿼드 + 커서.
- **TextArea** (pem/notes): 같은 골격, last_layout: Vec<WrappedLine> (shape_text + wrap width),
  수직 이동은 라인 인덱스, overflow_y_scroll + 커서 오토스크롤. v1 범위: wrap 전용,
  클릭 위치 지정, shift-화살표 선택, 복사/붙여넣기, IME.
- **Select**: options, selected_ix, open, active_ix; 트리거(라벨+셰브론) + open 시
  `deferred(anchored().snap_to_window_with_margin(px(8.)).child(menu))` (React portal 등가).
  on_mouse_down_out 닫기; Up/Down/Enter/Escape/Home/End context "Select".
  사용처: 그룹 필터, 키 선택, 시작 페이지, authType.
- **Modal/ConfirmDialog**: Workspace.modal: Option<ActiveModal{view: AnyView, prev_focus}> —
  마지막에 absolute size_full occlude 오버레이로 렌더. ConfirmDialog{title,message,confirm/
  cancel_label,danger,on_result: FnOnce(bool,..)}; Enter/Escape context "ConfirmDialog";
  열릴 때 포커스 진입, 닫히면 prev_focus 복귀. KeyManager 다이얼로그·Settings 모달도 같은 레이어.
- **Toast**: Workspace 토스트 스택(자동 dismiss 타이머); Settings는 기존처럼 인라인 메시지 라인.
- 폼 검증: `fn validate(form) -> Vec<(Field, TrKey)>` — zod 규칙 재현
  (name/host/username 비공백 trim, port 1..=65535 정수), 필드 아래 danger 색 표시.

## 3. 앱 셸

```rust
Application::new().run(|cx| {
    settings::init(cx); theme::init(cx); state::init(cx); keymap::register_all(cx);
    cx.open_window(WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some("sshub".into()),
            appears_transparent: true, traffic_light_position: Some(point(px(12.), px(12.))) }),
        window_background: settings.background_appearance(),
        window_min_size: Some(size(px(760.), px(480.))), ..Default::default()
    }, |window, cx| cx.new(|cx| Workspace::new(window, cx)));
});
```
- 타이틀바: 36px 드래그 스트립; 사이드바 헤더 좌측 ~76px 패딩(신호등).
- **반투명 유지**: Blurred + 루트 bg 알파, 런타임 토글. 카드/터미널은 불투명.
- 라우팅: `enum Page { Servers, ServerEdit{id:Option<i64>}, Terminal, Keys, Settings }`.
  gpui는 엔티티 수명 ≠ 렌더 — PTY는 terminal_host 엔티티(1회 생성, 드랍 금지) 소유,
  Terminal 페이지에서만 렌더해도 세션 유지. 페이지 뷰는 내비 시 재생성.
- 사이드바: 208px ↔ 56px 접힘(설정 저장), 5 페이지 svg 아이콘 + i18n 라벨, 활성 accent wash.

## 4. 테마 — Zed풍 미니멀 다크 (dark 전용)

폐기: CRT glow/스캔라인/bg 프리셋(green/warm/black)/phosphor 네이밍.
유지: 어센트(프리셋+커스텀 hex), 터미널 fg/bg 오버라이드, 폰트 크기 10..=24, 반투명 0..=40.

| 토큰 | 값 | 용도 |
|---|---|---|
| bg | #16181d | 창 루트(반투명 알파 여기) |
| surface | #1c1e24 | 카드·사이드바 |
| elevated | #22252c | 팝오버·모달·셀렉트 메뉴 |
| hover | #282b33 | 행/버튼 호버 |
| selected | #2e323b | 활성 행 |
| border | #2d313a | 카드·인풋 보더 |
| border_subtle | #23262d | 디바이더 |
| text | #d6d9de | 본문 |
| text_muted | #8b909a | 보조 |
| text_disabled | #565b65 | 비활성 |
| accent | #74ade8 (기본, 오버라이드 가능) | 포커스 링·활성 내비·프라이머리 버튼 |
| accent_wash | accent 14% | 선택 내비 bg |
| danger | #d07277 | 삭제·에러 |
| success | #98c379 | 완료 |
| warning | #dec184 | 주의 |

어센트 프리셋: blue #74ade8, green #a1c181, amber #dec184, magenta #b477cf (+커스텀 hex).
터미널 16색 (One Dark, Zed 기본): `#282c34 #e06c75 #98c379 #e5c07b #61afef #c678dd #56b6c2 #abb2bf`
/ bright `#5c6370 #e06c75 #98c379 #e5c07b #61afef #c678dd #56b6c2 #ffffff`;
기본 fg `#c8ccd4`, bg `#16181d` (termFg/termBg 설정으로 오버라이드).

## 5. i18n

**1회 codegen + 커밋** (build.rs 아님 — Rust 빌드가 Node에 의존하면 안 됨).
`rust/scripts/gen_i18n.mjs`가 src/i18n/index.ts (192 키 × ko/en/ja) 파싱 →
`i18n/generated.rs`:
```rust
pub enum Lang { Ko, En, Ja }
pub enum TrKey { NavDashboard, /* …192 variants… */ }
pub const fn tr(lang: Lang, key: TrKey) -> &'static str;
```
컴파일 타임 전수성(번역 누락 = 빌드 에러). `t(cx, key)` (settings에서 Lang),
`tf(cx, key, &[("err", &s)])` — `{param}` 치환.

## 6. 단축키

- `actions!(sshub, [NewTab, ClosePane, SplitRight, SplitDown, ToggleBroadcast,
  FontIncrease, FontDecrease, FocusLeft, FocusRight, FocusUp, FocusDown]);`
- **keymap.rs가 전체 키맵 단독 소유**: 위젯 바인딩(TextInput/Select/ConfirmDialog context) +
  사용자 단축키(context "Workspace"). 리바인딩 = clear_key_bindings() + register_all 재실행
  (clear가 전부 지우므로 중앙화 필수).
- 터미널 계약: 앱 단축키는 전부 cmd 포함; 터미널 element는 키맵 미스매치 키를
  raw key_down/IME로 처리 → 일반 키는 PTY로, cmd 콤보는 액션.
- 리바인드 UI: 캡처 행 포커스 → on_key_down에서 keystroke 읽기(단독 수정자 무시) →
  `Keystroke::unparse()` 직렬화, Escape 취소, 충돌 검사 → 설정 저장 → 키맵 재빌드.
- 구 포맷 변환 테이블(meta+KeyT→cmd-t, Equal→=, ArrowLeft→left …) — 백업 import용.

## 7. 상태 아키텍처 (TanStack Query 등가)

```rust
pub struct AppState { servers: Vec<Server>, keys: Vec<SshKey>, loading, last_error }
pub enum StateEvent { ServersChanged, KeysChanged }
struct AppStateHandle(Entity<AppState>); impl Global for AppStateHandle {}
```
- 읽기: 뷰가 global에서 Entity 획득, `cx.observe` → notify → 렌더.
- 쓰기: mutation 메서드가 `cx.background_spawn`으로 core 호출 → 완료 시 필드 갱신 +
  emit + notify + **해당 컬렉션 refetch** (refetch-on-mutation이 무효화 전략의 전부).
- 긴 작업(config sync, export/import)은 완료 콜백으로 페이지 인라인 메시지.

## 8. 리스크·순서

1. TextInput+IME (최고 위험) — examples/input.rs를 example 바이너리로 먼저 포팅, 한글/로마지 수동 검증.
2. TextArea wrap 레이아웃. 3. Select 오버레이 (스크롤 페이지 내 anchored+deferred).
4. clear_key_bindings 리바인드 후 위젯 바인딩 생존 테스트. 5. Blurred+불투명 pane 비주얼.
6. zed 소스 복사 시 태그 고정. 7. 파일 다이얼로그 (prompt_for_paths 확인, 폴백 rfd).

빌드 순서: theme+settings+i18n → 셸/사이드바/라우팅 → Button/Checkbox/List/Modal →
ServerList+Dashboard(텍스트 입력 불필요) → TextInput → ServerEdit+KeyManager → Select →
Settings+리바인드 → 터미널 호스트 통합.

---

## 9. 구현 노트 — gpui 0.2.2 실측 (위젯 킷 구현 중 발견)

`crates/sshub/src/ui/` 구현 시 레지스트리 소스(`gpui-0.2.2/`)를 grep해 확인한 것들.
docs.rs만 보고 추정했던 것과 다른 지점 위주.

### 텍스트 레이아웃

- `TextSystem::shape_text(text, font_size, runs, wrap_width, line_clamp)` — **인자 5개**,
  반환 `Result<SmallVec<[WrappedLine; 1]>>`. `'\n'` 기준으로 하드 라인당 `WrappedLine` 1개.
  `runs`의 `len` 합은 개행 포함 전체 텍스트 길이여야 한다.
- `WrappedLine`은 `WrappedLineLayout`으로 Deref. 쓸모 있는 메서드:
  - `position_for_index(index, line_height) -> Option<Point<Pixels>>` (라인 로컬 좌표)
  - `closest_index_for_position(pos, line_height) -> Result<usize, usize>`
    — **Err에도 클램프된 인덱스가 들어 있다** → `.unwrap_or_else(|ix| ix)`
  - `wrap_boundaries` / `size(line_height)` (높이 = `line_height * (wrap_boundaries.len()+1)`)
- `WrappedLine::paint(origin, line_height, TextAlign, Option<Bounds>, window, cx)` —
  `ShapedLine::paint(origin, line_height, window, cx)`보다 인자 2개 많다.
- 수동 스크롤 클리핑은 `window.with_content_mask(Some(ContentMask { bounds }), |window| …)`.

### Element / 입력

- `Element::{request_layout, prepaint, paint}`의 2번째 인자는
  `Option<&InspectorElementId>` (0.2.2에서 추가됨).
- `ElementInputHandler::new(bounds, entity)`는 **매 프레임 `paint`에서**
  `window.handle_input(&focus_handle, …, cx)`로 다시 등록해야 IME가 붙는다.
- `Context::on_focus_out(&handle, window, |this, FocusOutEvent, window, cx| …)`는
  `Subscription`을 반환한다 — 뷰에 보관하지 않으면 즉시 드랍되어 동작하지 않는다.
  (`Blurred` 이벤트가 이걸로 구현됨.)
- `unicode-segmentation`은 examples/input.rs가 쓰지만 gpui 재수출이 아니다.
  워크스페이스 의존을 늘리지 않으려고 grapheme 대신 **char 경계**를 쓴다
  (한글/CJK/라틴 동일, ZWJ 이모지 시퀀스만 문자 단위로 쪼개짐).

### 액션 / 키맵

- `actions!(namespace, [ … ])` — 액션 이름은 네임스페이스 단위로 전역 유일해야 하고
  중복 등록은 `App` 생성 시 panic. TextInput/TextArea가 `backspace`·`left` 등
  같은 키를 다른 컨텍스트로 쓰므로 **위젯마다 별도 네임스페이스**가 필요하다
  (`sshub_text_input` / `sshub_text_area` / `sshub_select` / `sshub_confirm_dialog`).
- `KeyBinding::new(keystrokes, action, Option<&str>)` — 컨텍스트가 3번째 인자.
- 전체 위젯 바인딩은 `ui::init(cx)` 한 함수에만 있다. `clear_key_bindings()`가
  전역 키맵을 통째로 비우므로 keymap.rs는 clear → `ui::init` → 사용자 바인딩 순서로 재빌드.

### 스타일 / 오버레이

- `overflow_y_scroll()`은 `StatefulInteractiveElement` — **`.id()` 먼저** 붙여야 한다.
- `Styled`에 `visible()`/`invisible()`이 **없다**. ListItem의 trailing 액션은
  `group()` + `group_hover(name, …)`로 **색 전환**(dim → text)으로 처리했다.
- `deferred(child).with_priority(n)` + `anchored().snap_to_window_with_margin(px(8.))`
  (`Pixels: Into<Edges<Pixels>>` 성립). `anchored()`는 자기가 레이아웃된 자리에
  붙으므로 드롭다운은 `.offset(point(px(0.), 트리거_높이))`가 필요하다.
- `gpui::rgba(hex: u32)`는 **0xRRGGBBAA**(알파가 하위 바이트). `impl From<Rgba> for Hsla`가
  있으므로 반투명 루트 bg는 `Hsla::from(rgba((rgb << 8) | alpha))`로 굽는다.
- `svg()`의 path는 `AssetSource`(`Application::with_assets`)를 통해 해석된다.
  위젯 킷이 앱 부트스트랩에 의존하지 않도록 v1 아이콘은 **유니코드 글리프**
  (`ui::icon::Icon::glyph()`). assets.rs가 생기면 `Icon::path()`로 승격.

### 보안 결정

- masked TextInput은 **Copy/Cut을 클립보드로 내보내지 않는다**(패스프레이즈 유출 방지).
  편집·IME·삭제는 실제 텍스트로 정상 동작하고, 화면 오프셋만
  `mask_offset`/`unmask_offset`(문자 인덱스 × 3바이트)으로 매핑한다.

## 10. 터미널 폰트 — 한글 정렬

macOS에는 한글 **고정폭** 폰트가 없다(AppleGothic·Apple SD Gothic Neo·Arial
Unicode 전부 가변폭). Menlo에는 한글 글리프가 없어 폴백이 일어나고, 그 폰트의
한글 폭이 터미널 2셀과 달라 글자마다 여백이 남는다.

그래서 **D2Coding을 바이너리에 내장**한다(`crates/sshub/assets/fonts/`,
SIL OFL 1.1 — OFL.txt 동봉). ASCII 0.5em / 한글 1.0em으로 정확히 2배라 격자에
빈틈없이 맞는다. `fonts::register(cx)`가 부트스트랩에서 regular·bold를 등록하고,
설정 `appearance.terminal.fontFamily`로 덮어쓸 수 있다(비어 있으면 내장 폰트).

주의:
- 폰트 파일 교체 시 `tests/embedded_font.rs`가 패밀리 이름과 2:1 폭 비율을 검증한다.
- gpui 테스트 플랫폼은 `NoopTextSystem`이라 폰트 등록·메트릭을 런타임으로
  검증할 수 없다. 파일 자체를 파싱해 확인한다.
- 내장 때문에 릴리스 바이너리가 약 10MB → 19MB로 커진다.
