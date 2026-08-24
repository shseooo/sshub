//! TextInput — 단일 행 텍스트 입력 (gpui 0.2.2 `examples/input.rs` 포팅).
//!
//! 원본 대비 추가된 것:
//! - 테마 토큰 적용(포커스 링·선택 색·플레이스홀더)
//! - `masked` 모드: 화면에는 `•`만 그리고 clipboard/IME/편집은 실제 텍스트로 (§2)
//! - `EventEmitter<InputEvent>` (Changed / Submitted / Blurred)
//! - Enter/Escape 액션
//!
//! 키 바인딩은 `ui::init()` 한 곳에서만 등록한다(context `"TextInput"`).
use std::ops::Range;

use crate::theme::{theme, with_alpha};
use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, Hsla, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, Subscription, TextRun, UTF16Selection, UnderlineStyle, Window,
};

actions!(
    sshub_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        Copy,
        Cut,
        Paste,
        Enter,
        Escape,
        ShowCharacterPalette,
    ]
);

/// 마스킹에 쓰는 글리프. UTF-8 3바이트.
pub const MASK_CHAR: char = '•';
const MASK_LEN: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Changed,
    Submitted,
    Blurred,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    masked: bool,
    disabled: bool,
    _subscriptions: Vec<Subscription>,
}

// ---------------------------------------------------------------------------
// 순수 헬퍼 (테스트 대상)
// ---------------------------------------------------------------------------

/// UTF-8 바이트 오프셋 → UTF-16 코드 유닛 오프셋.
pub fn offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for ch in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

/// UTF-16 코드 유닛 오프셋 → UTF-8 바이트 오프셋.
pub fn offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

/// 실제 텍스트를 같은 *문자 수*의 `•` 문자열로 치환.
pub fn masked_text(text: &str) -> String {
    let mut out = String::with_capacity(text.chars().count() * MASK_LEN);
    for _ in text.chars() {
        out.push(MASK_CHAR);
    }
    out
}

/// 실제 텍스트의 UTF-8 오프셋 → 마스킹된 표시 텍스트의 UTF-8 오프셋.
/// 문자 경계가 아닌 오프셋은 그 문자의 시작으로 내림한다.
pub fn mask_offset(text: &str, offset: usize) -> usize {
    let mut chars_before = 0;
    let mut byte = 0;
    for ch in text.chars() {
        if byte >= offset {
            break;
        }
        byte += ch.len_utf8();
        chars_before += 1;
    }
    chars_before * MASK_LEN
}

/// 마스킹된 표시 텍스트의 UTF-8 오프셋 → 실제 텍스트의 UTF-8 오프셋.
pub fn unmask_offset(text: &str, display_offset: usize) -> usize {
    let char_ix = display_offset / MASK_LEN;
    text.char_indices()
        .nth(char_ix)
        .map(|(ix, _)| ix)
        .unwrap_or(text.len())
}

/// `offset` 직전 문자 경계 (grapheme 대신 char 경계 — `unicode-segmentation`
/// 의존을 피하려는 선택. 한글/CJK/라틴에는 동일하게 동작하고 ZWJ 이모지 시퀀스만
/// 문자 단위로 쪼개진다).
pub fn previous_boundary(text: &str, offset: usize) -> usize {
    text.char_indices()
        .rev()
        .find_map(|(ix, _)| (ix < offset).then_some(ix))
        .unwrap_or(0)
}

/// `offset` 직후 문자 경계.
pub fn next_boundary(text: &str, offset: usize) -> usize {
    text.char_indices()
        .find_map(|(ix, _)| (ix > offset).then_some(ix))
        .unwrap_or(text.len())
}

// ---------------------------------------------------------------------------

impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let blur = cx.on_focus_out(&focus_handle, window, |this, _event, _window, cx| {
            this.is_selecting = false;
            cx.emit(InputEvent::Blurred);
        });
        Self {
            focus_handle,
            content: SharedString::default(),
            placeholder: SharedString::default(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            masked: false,
            disabled: false,
            _subscriptions: vec![blur],
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_text(mut self, text: impl Into<SharedString>) -> Self {
        self.content = text.into();
        self.selected_range = self.content.len()..self.content.len();
        self
    }

    pub fn with_masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn text(&self) -> &SharedString {
        &self.content
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    /// 프로그램적 갱신 — `InputEvent::Changed`를 발생시키지 않는다
    /// (폼 초기화가 검증 루프를 다시 도는 것을 막기 위함).
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.content = SharedString::default();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
        cx.notify();
    }

    // -- 표시 텍스트 매핑 (masked 모드) --------------------------------------

    fn display_text(&self) -> SharedString {
        if self.masked {
            SharedString::from(masked_text(&self.content))
        } else {
            self.content.clone()
        }
    }

    /// 실제 오프셋 → 표시 오프셋.
    fn to_display(&self, offset: usize) -> usize {
        if self.masked {
            mask_offset(&self.content, offset)
        } else {
            offset
        }
    }

    /// 표시 오프셋 → 실제 오프셋.
    fn from_display(&self, offset: usize) -> usize {
        if self.masked {
            unmask_offset(&self.content, offset)
        } else {
            offset
        }
    }

    // -- 액션 ---------------------------------------------------------------

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(previous_boundary(&self.content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(next_boundary(&self.content, self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(next_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submitted);
    }

    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        window.blur();
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        // masked 입력은 클립보드로 새어 나가지 않게 한다.
        if self.masked || self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.selected_range.is_empty() {
            return;
        }
        if !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // 단일 행이므로 개행은 공백으로 접는다.
            let flattened = text.replace(['\n', '\r'], " ");
            self.replace_text_in_range(None, &flattened, window, cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    // -- 마우스 -------------------------------------------------------------

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    // -- 커서/선택 ----------------------------------------------------------

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let display_ix = line.closest_index_for_x(position.x - bounds.left());
        self.from_display(display_ix)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        offset_to_utf16(&self.content, range.start)..offset_to_utf16(&self.content, range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        offset_from_utf16(&self.content, range_utf16.start)
            ..offset_from_utf16(&self.content, range_utf16.end)
    }
}

impl EventEmitter<InputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content.get(range)?.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.marked_range.as_ref()?;
        Some(
            offset_to_utf16(&self.content, marked.start)
                ..offset_to_utf16(&self.content, marked.end),
        )
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range.take();
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        self.selection_reversed = false;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = self.to_display(range.start);
        let end = self.to_display(range.end);
        Some(Bounds::from_corners(
            point(bounds.left() + last_layout.x_for_index(start), bounds.top()),
            point(bounds.left() + last_layout.x_for_index(end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let display_ix = last_layout.index_for_x(point.x - line_point.x)?;
        Some(offset_to_utf16(&self.content, self.from_display(display_ix)))
    }
}

// ---------------------------------------------------------------------------
// TextElement — 실제 텍스트/선택/커서 페인팅
// ---------------------------------------------------------------------------

struct TextElement {
    input: Entity<TextInput>,
    selection_color: Hsla,
    cursor_color: Hsla,
    placeholder_color: Hsla,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let display = input.display_text();
        let is_empty = input.content.is_empty();
        // 선택/커서/마킹은 모두 *표시* 오프셋으로 변환해서 쓴다.
        let selected_range =
            input.to_display(input.selected_range.start)..input.to_display(input.selected_range.end);
        let cursor = input.to_display(input.cursor_offset());
        let marked_range = input
            .marked_range
            .as_ref()
            .map(|r| input.to_display(r.start)..input.to_display(r.end));
        let style = window.text_style();

        let (display_text, text_color) = if is_empty {
            (input.placeholder.clone(), self.placeholder_color)
        } else {
            (display, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match marked_range.as_ref().filter(|_| !is_empty) {
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end.saturating_sub(marked.start),
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len().saturating_sub(marked.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let (selection, cursor) = if selected_range.is_empty() || is_empty {
            let cursor_x = line.x_for_index(if is_empty { 0 } else { cursor });
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    self.cursor_color,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    self.selection_color,
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .ok();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let focused = self.focus_handle.is_focused(window);
        let border = if focused { t.accent } else { t.border };

        div()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_to_home))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .flex()
            .w_full()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(border)
            .bg(if self.disabled { t.surface } else { t.elevated })
            .text_size(px(13.))
            .text_color(if self.disabled {
                t.text_disabled
            } else {
                t.text
            })
            .child(TextElement {
                input: cx.entity(),
                selection_color: with_alpha(t.accent, 0.30),
                cursor_color: t.accent,
                placeholder_color: t.text_disabled,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_roundtrip_ascii() {
        let s = "hello";
        for i in 0..=s.len() {
            assert_eq!(offset_to_utf16(s, i), i);
            assert_eq!(offset_from_utf16(s, i), i);
        }
    }

    #[test]
    fn utf16_mapping_korean() {
        // '한' = UTF-8 3바이트, UTF-16 1유닛.
        let s = "한글ab";
        assert_eq!(offset_to_utf16(s, 0), 0);
        assert_eq!(offset_to_utf16(s, 3), 1);
        assert_eq!(offset_to_utf16(s, 6), 2);
        assert_eq!(offset_to_utf16(s, 7), 3);
        assert_eq!(offset_to_utf16(s, 8), 4);

        assert_eq!(offset_from_utf16(s, 0), 0);
        assert_eq!(offset_from_utf16(s, 1), 3);
        assert_eq!(offset_from_utf16(s, 2), 6);
        assert_eq!(offset_from_utf16(s, 4), 8);
    }

    #[test]
    fn utf16_mapping_surrogate_pair() {
        // '😀' = UTF-8 4바이트, UTF-16 2유닛(서로게이트 페어).
        let s = "a😀b";
        assert_eq!(offset_to_utf16(s, 1), 1);
        assert_eq!(offset_to_utf16(s, 5), 3);
        assert_eq!(offset_to_utf16(s, 6), 4);
        assert_eq!(offset_from_utf16(s, 3), 5);
        assert_eq!(offset_from_utf16(s, 4), 6);
    }

    #[test]
    fn masked_text_preserves_char_count() {
        assert_eq!(masked_text(""), "");
        assert_eq!(masked_text("abc"), "•••");
        // 한글 3글자 → 불릿 3개 (바이트 수가 아니라 문자 수 기준).
        assert_eq!(masked_text("비밀번").chars().count(), 3);
    }

    #[test]
    fn mask_offset_maps_char_index_times_three() {
        let s = "ab한";
        assert_eq!(mask_offset(s, 0), 0);
        assert_eq!(mask_offset(s, 1), 3);
        assert_eq!(mask_offset(s, 2), 6);
        assert_eq!(mask_offset(s, 5), 9); // '한' 뒤 = 3번째 문자 끝
        assert_eq!(mask_offset(s, s.len()), masked_text(s).len());
    }

    #[test]
    fn unmask_offset_is_inverse_of_mask_offset() {
        for s in ["", "abc", "한글ab", "a😀b"] {
            for (byte_ix, _) in s.char_indices().chain([(s.len(), ' ')]) {
                let display = mask_offset(s, byte_ix);
                assert_eq!(unmask_offset(s, display), byte_ix, "s={s:?} ix={byte_ix}");
            }
        }
    }

    #[test]
    fn boundaries_respect_multibyte_chars() {
        let s = "한글";
        assert_eq!(next_boundary(s, 0), 3);
        assert_eq!(next_boundary(s, 3), 6);
        assert_eq!(previous_boundary(s, 6), 3);
        assert_eq!(previous_boundary(s, 3), 0);
        assert_eq!(previous_boundary(s, 0), 0);
    }
}
