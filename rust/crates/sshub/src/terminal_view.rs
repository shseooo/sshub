//! 포커스를 갖는 터미널 뷰 — 키/IME/마우스 입력의 주인 (DESIGN-terminal.md §4).
//!
//! IME(한글/일본어)가 최우선 품질 기준이다. 조합 중(marked) 텍스트는 **절대**
//! PTY로 내려가지 않는다. `replace_and_mark_text_in_range`는 오버레이 상태만
//! 갱신하고, 확정 시점에 불리는 `replace_text_in_range`에서만 `input()`한다.
//! Chromium 팬텀 IME 가드는 포팅하지 않는다 (DOM 전용 버그).

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    div, App, AppContext, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Hsla, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Pixels, Render, Styled, Subscription, UTF16Selection, Window,
};
use sshub_terminal::{Event as TerminalEvent, LinkTarget, SpawnSpec, Terminal, TerminalBuilder};

use crate::terminal_element::{
    offset_to_utf16, range_from_utf16, TerminalElement, LINE_HEIGHT_RATIO,
};
use crate::theme::theme;

/// 기본 터미널 폰트. 설정에서 덮어쓴다.
pub const DEFAULT_FONT_FAMILY: &str = "Menlo";

/// IME 조합 중 상태. 확정 전까지 PTY로 가지 않는다.
struct ImeState {
    text: String,
    /// `text` 안의 바이트 범위 (후보 선택 하이라이트).
    selection: Range<usize>,
}

pub struct TerminalView {
    terminal: Entity<Terminal>,
    focus_handle: FocusHandle,
    ime: Option<ImeState>,
    font_family: String,
    /// 마지막으로 페인트된 엘리먼트 영역 — `bounds_for_range`가 쓴다.
    last_bounds: Option<Bounds<Pixels>>,
    /// 로컬 세션인가 — 경로 링크(Finder 열기)는 로컬에서만 의미가 있다.
    local: bool,
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
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.subscribe(&terminal, Self::on_terminal_event),
            // 터미널이 notify하면(그리드 변경) 뷰도 다시 그린다.
            cx.observe(&terminal, |_, _, cx| cx.notify()),
        ];
        Ok(TerminalView {
            terminal,
            focus_handle,
            ime: None,
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            last_bounds: None,
            local: true,
            _subscriptions: subscriptions,
        })
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
                    return;
                }
                "v" => {
                    self.paste(cx);
                    return;
                }
                "a" => {
                    self.terminal.update(cx, |terminal, _| terminal.select_all());
                    cx.notify();
                    return;
                }
                _ => return,
            }
        }

        let handled = self
            .terminal
            .update(cx, |terminal, _| terminal.try_keystroke(keystroke, true));
        if handled {
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
        let bg = Hsla::from(terminal_theme.background);
        let element = TerminalElement::new(
            self.terminal.clone(),
            cx.entity(),
            self.focus_handle.clone(),
            focused,
            terminal_theme,
            self.font_family.clone(),
        );
        div()
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .size_full()
            .bg(bg)
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

/// 폰트 크기에서 줄 높이 (엘리먼트와 같은 규칙).
pub fn line_height_for(font_size: f32) -> f32 {
    font_size * LINE_HEIGHT_RATIO
}

#[cfg(test)]
mod tests {
    use super::*;

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
