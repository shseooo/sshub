//! 포커스를 갖는 터미널 뷰 — 키/IME/마우스 입력의 주인 (DESIGN-terminal.md §4).
//!
//! IME(한글/일본어)가 최우선 품질 기준이다. 조합 중(marked) 텍스트는 **절대**
//! PTY로 내려가지 않는다. `replace_and_mark_text_in_range`는 오버레이 상태만
//! 갱신하고, 확정 시점에 불리는 `replace_text_in_range`에서만 `input()`한다.
//! Chromium 팬텀 IME 가드는 포팅하지 않는다 (DOM 전용 버그).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    div, App, AppContext, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, Keystroke,
    ParentElement, Pixels, Render, Styled, Subscription, UTF16Selection, Window,
};
use sshub_terminal::{Event as TerminalEvent, LinkTarget, SpawnSpec, Terminal, TerminalBuilder};

use crate::terminal_element::{
    offset_to_utf16, range_from_utf16, TerminalElement, LINE_HEIGHT_RATIO,
};
use crate::theme::theme;

/// 기본 터미널 폰트 — 앱에 내장한 D2Coding(한글이 ASCII 정확히 2배 폭).
/// 설정에서 덮어쓸 수 있다.
pub const DEFAULT_FONT_FAMILY: &str = crate::fonts::EMBEDDED_FAMILY;

/// IME 조합 중 상태. 확정 전까지 PTY로 가지 않는다.
struct ImeState {
    text: String,
    /// `text` 안의 바이트 범위 (후보 선택 하이라이트).
    selection: Range<usize>,
}

/// 이 뷰가 **방금 자기 터미널로 보낸** 입력. 브로드캐스트(동시 입력)를 켠
/// 워크스페이스가 같은 탭의 나머지 pane에 복제한다 (DESIGN-terminal.md §6).
/// 포커스된 pane이 커서/IME를 소유하고, 복제는 그 결과만 따라간다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BroadcastInput {
    Keystroke(Keystroke),
    /// IME 확정 등 그대로 써야 하는 텍스트.
    Text(String),
    /// 붙여넣기 — 대상 터미널에서도 bracketed-paste로 감싼다.
    Paste(String),
}

pub type BroadcastSink = Rc<dyn Fn(&BroadcastInput, &mut App)>;

pub struct TerminalView {
    terminal: Entity<Terminal>,
    focus_handle: FocusHandle,
    ime: Option<ImeState>,
    font_family: String,
    /// 마지막으로 페인트된 엘리먼트 영역 — `bounds_for_range`가 쓴다.
    last_bounds: Option<Bounds<Pixels>>,
    /// 로컬 세션인가 — 경로 링크(Finder 열기)는 로컬에서만 의미가 있다.
    local: bool,
    broadcast: Option<BroadcastSink>,
    _subscriptions: Vec<Subscription>,
}

/// 뷰가 상위(워크스페이스)로 올리는 이벤트.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewEvent {
    TitleChanged(Option<String>),
    Closed,
}

impl EventEmitter<ViewEvent> for TerminalView {}

impl TerminalView {
    pub fn new(spec: SpawnSpec, cx: &mut Context<Self>) -> anyhow::Result<TerminalView> {
        // PTY spawn은 호출 스레드(= 메인)에서 일어난다.
        let builder = TerminalBuilder::new(spec)?;
        let terminal = cx.new(|cx| builder.subscribe(cx));
        Ok(Self::from_terminal(terminal, cx))
    }

    /// 이미 살아 있는 터미널에 뷰를 붙인다. 세션 레지스트리가 엔티티를 **앱
    /// 스코프**로 소유하므로(§6·§8) 창/탭을 옮겨도 같은 PTY를 계속 본다.
    pub fn from_terminal(terminal: Entity<Terminal>, cx: &mut Context<Self>) -> TerminalView {
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.subscribe(&terminal, Self::on_terminal_event),
            // 터미널이 notify하면(그리드 변경) 뷰도 다시 그린다.
            cx.observe(&terminal, |_, _, cx| cx.notify()),
        ];
        TerminalView {
            terminal,
            focus_handle,
            ime: None,
            // 빈 값 = 테마(설정)를 따른다. `set_font_family`로 pane별 고정도 가능.
            font_family: String::new(),
            last_bounds: None,
            local: true,
            broadcast: None,
            _subscriptions: subscriptions,
        }
    }

    /// 이 뷰가 보낸 입력을 넘겨받을 싱크 (동시 입력). `None`이면 복제하지 않는다.
    pub fn set_broadcast(&mut self, sink: Option<BroadcastSink>) {
        self.broadcast = sink;
    }

    fn emit_broadcast(&self, input: BroadcastInput, cx: &mut App) {
        if let Some(sink) = &self.broadcast {
            sink(&input, cx);
        }
    }

    pub fn terminal(&self) -> &Entity<Terminal> {
        &self.terminal
    }

    pub fn set_font_family(&mut self, family: impl Into<String>) {
        self.font_family = family.into();
    }

    pub fn set_local(&mut self, local: bool) {
        self.local = local;
    }

    /// 조합 중 텍스트 (엘리먼트가 오버레이로 그린다).
    pub fn marked_text(&self) -> Option<&str> {
        self.ime.as_ref().map(|ime| ime.text.as_str())
    }

    /// 조합 중 텍스트 안에서 IME가 "지금 고르는 중"이라고 표시한 바이트 범위.
    /// 엘리먼트가 이 구간만 두꺼운 밑줄로 그린다 (일본어 변환 절 선택 등).
    pub fn marked_selection(&self) -> Option<Range<usize>> {
        let ime = self.ime.as_ref()?;
        (!ime.selection.is_empty()).then(|| ime.selection.clone())
    }

    fn on_terminal_event(
        &mut self,
        _terminal: Entity<Terminal>,
        event: &TerminalEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::Wakeup | TerminalEvent::SelectionsChanged => cx.notify(),
            TerminalEvent::Bell => {}
            TerminalEvent::TitleChanged(title) => {
                cx.emit(ViewEvent::TitleChanged(title.clone()));
            }
            TerminalEvent::CloseTerminal => {
                cx.emit(ViewEvent::Closed);
                cx.notify();
            }
            TerminalEvent::Open(target) => self.open_target(target.clone(), cx),
        }
    }

    fn open_target(&mut self, target: LinkTarget, cx: &mut Context<Self>) {
        match target {
            LinkTarget::Url(url) => cx.open_url(&url),
            LinkTarget::Path(path) => {
                // 원격 세션의 경로는 우리 디스크의 파일이 아니다.
                if !self.local {
                    return;
                }
                let expanded = expand_tilde(&path);
                // 존재 확인은 클릭 시점에 — 휴리스틱 매치의 오탐을 막는다.
                if expanded.exists() {
                    cx.reveal_path(&expanded);
                }
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // 조합 중에는 IME가 입력의 주인 — 키를 가로채지 않는다.
        if self.ime.is_some() {
            return;
        }
        let keystroke = &event.keystroke;

        if keystroke.modifiers.platform {
            match keystroke.key.as_str() {
                "c" => {
                    self.copy(cx);
                    cx.stop_propagation();
                }
                "v" => {
                    self.paste(cx);
                    cx.stop_propagation();
                }
                "a" => {
                    self.terminal.update(cx, |terminal, _| terminal.select_all());
                    cx.stop_propagation();
                    cx.notify();
                }
                // 나머지 cmd 조합은 앱 단축키 — 키맵이 가져가도록 흘려보낸다.
                _ => {}
            }
            return;
        }

        // 평범한 문자 입력은 IME에 양보한다. macOS gpui는 출력 가능한 키를
        // **IME보다 먼저** 이 핸들러로 보내고, 우리가 소비하면(propagate=false)
        // IME에 아예 전달하지 않는다. 여기서 가로채면 한글 조합이 시작조차 못
        // 하고 원시 키('ㅎ' 자리의 'g')가 셸로 들어간다. 확정된 글자는
        // `replace_text_in_range`로 되돌아온다.
        if is_text_input(keystroke) {
            return;
        }

        let handled = self
            .terminal
            .update(cx, |terminal, _| terminal.try_keystroke(keystroke, true));
        if handled {
            // 소비했음을 알려야 gpui가 같은 키를 IME로 다시 넘기지 않는다.
            cx.stop_propagation();
            self.emit_broadcast(BroadcastInput::Keystroke(keystroke.clone()), cx);
            cx.notify();
        }
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        // 행말 패딩 제거는 모델(`trim_selection_trailing`)이 이미 적용한다.
        let text = self.terminal.update(cx, |terminal, _| terminal.copy());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.terminal.update(cx, |terminal, _| terminal.paste(&text));
        self.emit_broadcast(BroadcastInput::Paste(text), cx);
        cx.notify();
    }

    /// 커서 셀의 화면 좌표 — IME 후보창 위치.
    fn cursor_bounds(
        &self,
        element_bounds: Bounds<Pixels>,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let terminal = self.terminal.read(cx);
        let content = &terminal.last_content;
        let tb = content.terminal_bounds;
        let row = content.cursor.point.line.0 + content.display_offset as i32;
        if row < 0 || row >= tb.screen_lines() as i32 {
            return None;
        }
        let origin = gpui::point(
            element_bounds.origin.x + tb.cell_width * (content.cursor.point.column.0 as f32),
            element_bounds.origin.y + tb.line_height * (row as f32),
        );
        Some(Bounds::new(origin, gpui::size(tb.cell_width, tb.line_height)))
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => Path::new(&home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let terminal_theme = theme(cx).terminal.clone();
        // 폰트 패밀리는 **테마에서** 읽는다. pane 생성 시점의 값을 들고 있으면
        // 설정에서 폰트를 바꿔도 이미 열린 터미널은 그대로라 재시작이 필요해진다
        // (크기·색은 이미 테마 경로를 타므로 패밀리만 예외로 둘 이유가 없다).
        let font_family = effective_font_family(&self.font_family, &terminal_theme.font_family);
        let element = TerminalElement::new(
            self.terminal.clone(),
            cx.entity(),
            self.focus_handle.clone(),
            focused,
            terminal_theme,
            font_family,
        );
        div()
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .size_full()
            // 표면 색은 **부모**(`TerminalWorkspace` 루트)가 한 겹만 칠한다.
            // 여기서 또 칠하면 반투명 알파가 두 번 합성돼(0.6² → 0.84) 창
            // 반투명이 거의 보이지 않는다. 기본 배경 셀은 어차피 그리지 않으므로
            // (`build_bg_runs`) 이 뷰는 배경 없이 글자만 얹는다.
            .on_key_down(cx.listener(Self::on_key_down))
            .child(element)
    }
}

// ---------------------------------------------------------------------------
// IME
// ---------------------------------------------------------------------------

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        // 터미널에는 편집 가능한 텍스트 버퍼가 없다 — 조합 중 텍스트만 노출한다.
        let ime = self.ime.as_ref()?;
        let range = range_from_utf16(&ime.text, &range_utf16);
        let start = range.start.min(ime.text.len());
        let end = range.end.min(ime.text.len());
        *adjusted_range = Some(offset_to_utf16(&ime.text, start)..offset_to_utf16(&ime.text, end));
        Some(ime.text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = self
            .ime
            .as_ref()
            .map(|ime| offset_to_utf16(&ime.text, ime.text.len()))
            .unwrap_or(0);
        Some(UTF16Selection { range: len..len, reversed: false })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let ime = self.ime.as_ref()?;
        Some(0..offset_to_utf16(&ime.text, ime.text.len()))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ime = None;
        cx.notify();
    }

    /// 확정 — **여기서만** PTY로 내려간다.
    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime = None;
        if !text.is_empty() {
            self.terminal
                .update(cx, |terminal, _| terminal.input(text.as_bytes().to_vec()));
            self.emit_broadcast(BroadcastInput::Text(text.to_string()), cx);
        }
        cx.notify();
    }

    /// 조합 중 — 오버레이 상태만 바꾸고 PTY로는 절대 보내지 않는다.
    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if new_text.is_empty() {
            self.ime = None;
        } else {
            let selection = new_selected_range
                .map(|r| range_from_utf16(new_text, &r))
                .unwrap_or(new_text.len()..new_text.len());
            self.ime = Some(ImeState { text: new_text.to_string(), selection });
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.last_bounds = Some(element_bounds);
        self.cursor_bounds(element_bounds, cx)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 터미널은 임의 지점의 문자 인덱스를 IME에 제공하지 않는다.
        None
    }
}

/// pane이 실제로 쓸 폰트 패밀리.
///
/// pane별 고정값이 있으면 그것을, 없으면 테마(=설정)를 따른다. 생성 시점 값을
/// 들고 있으면 설정에서 폰트를 바꿔도 이미 열린 터미널은 안 바뀐다 — 크기·색은
/// 테마 경로를 타는데 패밀리만 예외였던 것이 실제 버그였다.
pub fn effective_font_family(pane_override: &str, theme_family: &str) -> String {
    if pane_override.trim().is_empty() {
        theme_family.to_string()
    } else {
        pane_override.to_string()
    }
}

/// IME가 처리해야 할 "평범한 문자 입력"인가.
///
/// 수식키가 붙은 키는 터미널이 제어 시퀀스로 직접 매핑해야 하고(ctrl-c 등),
/// enter·tab·escape·backspace는 `key_char`가 있어도 터미널 규약이 따로 있다
/// (enter는 LF가 아니라 CR). 그 외 `key_char`가 있는 키만 IME에 넘긴다.
pub fn is_text_input(keystroke: &Keystroke) -> bool {
    let mods = &keystroke.modifiers;
    if mods.control || mods.alt || mods.function || mods.platform {
        return false;
    }
    if matches!(
        keystroke.key.as_str(),
        "enter" | "tab" | "escape" | "backspace" | "delete"
    ) {
        return false;
    }
    keystroke.key_char.as_deref().is_some_and(|c| !c.is_empty())
}

/// 폰트 크기에서 줄 높이 (엘리먼트와 같은 규칙).
pub fn line_height_for(font_size: f32) -> f32 {
    font_size * LINE_HEIGHT_RATIO
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    /// `key_char`는 플랫폼이 채워 주는 값 — 테스트에서는 흉내 낸다.
    fn typed(source: &str, ch: &str) -> Keystroke {
        let mut k = ks(source);
        k.key_char = Some(ch.to_string());
        k
    }

    #[test]
    fn font_family_follows_the_theme_unless_a_pane_pins_one() {
        // 설정에서 폰트를 바꾸면 이미 열린 pane도 따라와야 한다.
        assert_eq!(effective_font_family("", "D2Coding"), "D2Coding");
        assert_eq!(effective_font_family("   ", "Menlo"), "Menlo");
        // pane별 고정값은 존중한다.
        assert_eq!(effective_font_family("SF Mono", "D2Coding"), "SF Mono");
    }

    #[test]
    fn plain_characters_are_left_to_the_ime() {
        // 한글 조합의 출발점 — 여기서 가로채면 IME가 시작조차 못 한다.
        assert!(is_text_input(&typed("g", "g")));
        assert!(is_text_input(&typed("shift-a", "A")));
        assert!(is_text_input(&typed("1", "1")));
        assert!(is_text_input(&typed("space", " ")));
    }

    #[test]
    fn control_sequences_stay_with_the_terminal() {
        // 터미널 규약이 따로 있는 키들 — IME에 넘기면 CR 대신 LF가 가는 식으로 깨진다.
        for key in ["enter", "tab", "escape", "backspace", "delete"] {
            assert!(!is_text_input(&typed(key, "\n")), "{key}");
        }
        assert!(!is_text_input(&ks("ctrl-c")));
        assert!(!is_text_input(&ks("alt-b")));
        assert!(!is_text_input(&ks("cmd-c")));
        assert!(!is_text_input(&ks("up")), "화살표는 key_char가 없다");
    }

    #[test]
    fn keys_without_a_character_are_not_text() {
        let mut k = ks("f5");
        k.key_char = None;
        assert!(!is_text_input(&k));

        let mut empty = ks("g");
        empty.key_char = Some(String::new());
        assert!(!is_text_input(&empty));
    }

    #[test]
    fn tilde_expands_to_home() {
        std::env::set_var("HOME", "/Users/tester");
        assert_eq!(expand_tilde("~/x/y"), PathBuf::from("/Users/tester/x/y"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
        // `~` 단독은 확장 대상이 아니다 (경로 정규식이 `~/`만 매치한다)
        assert_eq!(expand_tilde("~notauser"), PathBuf::from("~notauser"));
    }

    #[test]
    fn line_height_matches_the_element_ratio() {
        assert_eq!(line_height_for(10.0), 14.0);
    }
}
