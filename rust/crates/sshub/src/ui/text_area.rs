//! TextArea — 여러 줄 입력 (pem 본문 / notes).
//!
//! TextInput과 같은 골격이지만 레이아웃이 `Vec<WrappedLine>`(shape_text + wrap width).
//! v1 범위(§2): **wrap 전용, 가로 스크롤 없음**, 클릭 위치 지정, shift-화살표 선택,
//! 위/아래 = 표시 행(wrap 포함) 단위 이동, 복사/붙여넣기, IME.
//! 세로 스크롤은 커서를 따라가는 수동 오프셋.
use std::ops::Range;

use crate::theme::{theme, with_alpha};
use crate::ui::text_input::{
    next_boundary, offset_from_utf16, offset_to_utf16, previous_boundary, InputEvent,
};
use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem,
    ContentMask, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ScrollWheelEvent, SharedString, Style, Subscription, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine,
};

actions!(
    sshub_text_area,
    [
        AreaBackspace,
        AreaDelete,
        AreaLeft,
        AreaRight,
        AreaUp,
        AreaDown,
        AreaSelectLeft,
        AreaSelectRight,
        AreaSelectUp,
        AreaSelectDown,
        AreaSelectAll,
        AreaHome,
        AreaEnd,
        AreaCopy,
        AreaCut,
        AreaPaste,
        AreaNewline,
        AreaEscape,
    ]
);

/// 콘텐츠 좌표계(스크롤 미적용)에서의 줄 레이아웃.
struct AreaLayout {
    lines: Vec<WrappedLine>,
    /// 각 하드 라인의 content 내 시작 바이트 오프셋
    line_starts: Vec<usize>,
    line_height: Pixels,
    wrap_width: Pixels,
    content_height: Pixels,
}

impl AreaLayout {
    /// 하드 라인별 세로 높이(랩된 행 수 × line_height).
    fn line_height_of(&self, ix: usize) -> Pixels {
        self.lines
            .get(ix)
            .map(|line| self.line_height * (line.wrap_boundaries.len() + 1))
            .unwrap_or(self.line_height)
    }

    /// 하드 라인 시작의 y (콘텐츠 좌표).
    fn line_top(&self, ix: usize) -> Pixels {
        (0..ix.min(self.lines.len())).fold(px(0.), |acc, i| acc + self.line_height_of(i))
    }

    /// content 오프셋 → (하드 라인 인덱스, 라인 내 오프셋)
    fn split_offset(&self, offset: usize) -> (usize, usize) {
        let mut ix = 0;
        for (i, start) in self.line_starts.iter().enumerate() {
            if *start > offset {
                break;
            }
            ix = i;
        }
        (ix, offset - self.line_starts[ix])
    }

    /// content 오프셋 → 콘텐츠 좌표 위치 (행 좌상단).
    fn position_for_offset(&self, offset: usize) -> Option<Point<Pixels>> {
        let (line_ix, local) = self.split_offset(offset);
        let line = self.lines.get(line_ix)?;
        let local_pos = line
            .position_for_index(local, self.line_height)
            .unwrap_or_else(|| point(px(0.), px(0.)));
        Some(point(local_pos.x, self.line_top(line_ix) + local_pos.y))
    }

    /// 콘텐츠 좌표 → content 오프셋 (가장 가까운 위치).
    fn offset_for_position(&self, pos: Point<Pixels>, content_len: usize) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        if pos.y < px(0.) {
            return 0;
        }
        let mut top = px(0.);
        for (ix, line) in self.lines.iter().enumerate() {
            let height = self.line_height_of(ix);
            if pos.y < top + height {
                let local = point(pos.x, pos.y - top);
                let local_ix = line
                    .closest_index_for_position(local, self.line_height)
                    .unwrap_or_else(|ix| ix);
                return (self.line_starts[ix] + local_ix).min(content_len);
            }
            top += height;
        }
        content_len
    }
}

pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<AreaLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    disabled: bool,
    /// 위로 스크롤한 양(px, >= 0)
    scroll_offset: Pixels,
    /// 다음 prepaint에서 커서를 화면 안으로 끌어오기
    autoscroll: bool,
    _subscriptions: Vec<Subscription>,
}

impl TextArea {
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
            disabled: false,
            scroll_offset: px(0.),
            autoscroll: false,
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

    pub fn text(&self) -> &SharedString {
        &self.content
    }

    /// 프로그램적 갱신 — `InputEvent::Changed`를 발생시키지 않는다.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_offset = px(0.);
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.set_text(SharedString::default(), cx);
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
    }

    // -- 커서/선택 ----------------------------------------------------------

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.autoscroll = true;
        cx.notify();
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
        self.autoscroll = true;
        cx.notify();
    }

    /// 표시 행 단위 위/아래 — 커서 픽셀 위치를 ±line_height 이동시켜 재해석한다.
    /// (wrap된 행 안에서도 자연스럽게 동작)
    fn offset_for_vertical_move(&self, up: bool) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        let pos = layout.position_for_offset(self.cursor_offset())?;
        let delta = if up {
            -layout.line_height
        } else {
            layout.line_height
        };
        let target = point(pos.x, pos.y + delta);
        if target.y < px(0.) {
            return Some(0);
        }
        if target.y >= layout.content_height {
            return Some(self.content.len());
        }
        Some(layout.offset_for_position(target, self.content.len()))
    }

    fn line_bounds_for_cursor(&self) -> (usize, usize) {
        let Some(layout) = self.last_layout.as_ref() else {
            return (0, self.content.len());
        };
        let (line_ix, _) = layout.split_offset(self.cursor_offset());
        let start = layout.line_starts[line_ix];
        let end = layout
            .line_starts
            .get(line_ix + 1)
            // 다음 하드 라인 시작 - 개행 1바이트
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.content.len());
        (start, end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        let local = point(
            position.x - bounds.left(),
            position.y - bounds.top() + self.scroll_offset,
        );
        layout.offset_for_position(local, self.content.len())
    }

    // -- 액션 ---------------------------------------------------------------

    fn left(&mut self, _: &AreaLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(previous_boundary(&self.content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &AreaRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(next_boundary(&self.content, self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &AreaUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_vertical_move(true) {
            self.move_to(offset, cx);
        }
    }

    fn down(&mut self, _: &AreaDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_vertical_move(false) {
            self.move_to(offset, cx);
        }
    }

    fn select_left(&mut self, _: &AreaSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &AreaSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_up(&mut self, _: &AreaSelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_vertical_move(true) {
            self.select_to(offset, cx);
        }
    }

    fn select_down(&mut self, _: &AreaSelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.offset_for_vertical_move(false) {
            self.select_to(offset, cx);
        }
    }

    fn select_all(&mut self, _: &AreaSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &AreaHome, _: &mut Window, cx: &mut Context<Self>) {
        let (start, _) = self.line_bounds_for_cursor();
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &AreaEnd, _: &mut Window, cx: &mut Context<Self>) {
        let (_, end) = self.line_bounds_for_cursor();
        self.move_to(end, cx);
    }

    fn backspace(&mut self, _: &AreaBackspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(previous_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &AreaDelete, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(next_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn newline(&mut self, _: &AreaNewline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn escape(&mut self, _: &AreaEscape, window: &mut Window, cx: &mut Context<Self>) {
        window.blur();
        cx.notify();
    }

    fn copy(&mut self, _: &AreaCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
    }

    fn cut(&mut self, _: &AreaCut, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &AreaPaste, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // 여러 줄 위젯이므로 개행을 보존한다 (CRLF만 정규화).
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.replace_text_in_range(None, &normalized, window, cx);
        }
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

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        let viewport = self
            .last_bounds
            .map(|b| b.size.height)
            .unwrap_or(layout.content_height);
        let max = (layout.content_height - viewport).max(px(0.));
        let delta = event.delta.pixel_delta(layout.line_height).y;
        self.scroll_offset = (self.scroll_offset - delta).clamp(px(0.), max);
        let _ = window;
        cx.notify();
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        offset_to_utf16(&self.content, range.start)..offset_to_utf16(&self.content, range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        offset_from_utf16(&self.content, range_utf16.start)
            ..offset_from_utf16(&self.content, range_utf16.end)
    }
}

impl EventEmitter<InputEvent> for TextArea {}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextArea {
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
        self.autoscroll = true;
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
        self.autoscroll = true;
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
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for_offset(range.start)?;
        let end = layout.position_for_offset(range.end)?;
        let top = bounds.top() + start.y - self.scroll_offset;
        Some(Bounds::from_corners(
            point(bounds.left() + start.x, top),
            point(
                bounds.left() + if end.y > start.y { layout.wrap_width } else { end.x },
                top + layout.line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point_in_window: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.index_for_mouse_position(point_in_window);
        Some(offset_to_utf16(&self.content, offset))
    }
}

// ---------------------------------------------------------------------------
// TextAreaElement
// ---------------------------------------------------------------------------

struct TextAreaElement {
    input: Entity<TextArea>,
    selection_color: Hsla,
    cursor_color: Hsla,
    placeholder_color: Hsla,
}

struct AreaPrepaint {
    layout: Option<AreaLayout>,
    scroll_offset: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = AreaPrepaint;

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
        style.size.height = relative(1.).into();
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
        let content = input.content.clone();
        let is_empty = content.is_empty();
        let selected_range = input.selected_range.clone();
        let cursor_offset = input.cursor_offset();
        let marked_range = input.marked_range.clone();
        let mut scroll_offset = input.scroll_offset;
        let autoscroll = input.autoscroll;
        let style = window.text_style();
        let line_height = window.line_height();

        let (display_text, text_color) = if is_empty {
            (input.placeholder.clone(), self.placeholder_color)
        } else {
            (content.clone(), style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs: Vec<TextRun> = match marked_range.as_ref().filter(|_| !is_empty) {
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
        let wrap_width = bounds.size.width.max(px(1.));
        let lines: Vec<WrappedLine> = window
            .text_system()
            .shape_text(display_text, font_size, &runs, Some(wrap_width), None)
            .map(|lines| lines.into_iter().collect())
            .unwrap_or_default();

        // 하드 라인 시작 오프셋은 표시 텍스트에서 직접 계산한다
        // (shape_text가 '\n' 기준으로 라인을 나눈다).
        let source: &str = if is_empty { "" } else { content.as_ref() };
        let mut line_starts = vec![0usize];
        for (ix, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(ix + 1);
            }
        }
        line_starts.truncate(lines.len().max(1));
        while line_starts.len() < lines.len() {
            line_starts.push(source.len());
        }

        let content_height = lines
            .iter()
            .fold(px(0.), |acc, line| {
                acc + line_height * (line.wrap_boundaries.len() + 1)
            })
            .max(line_height);

        let layout = AreaLayout {
            lines,
            line_starts,
            line_height,
            wrap_width,
            content_height,
        };

        // 커서 오토스크롤 — 커서 행이 뷰포트 안에 들어오게 한다.
        let max_scroll = (content_height - bounds.size.height).max(px(0.));
        if autoscroll && !is_empty {
            if let Some(pos) = layout.position_for_offset(cursor_offset) {
                if pos.y < scroll_offset {
                    scroll_offset = pos.y;
                } else if pos.y + line_height > scroll_offset + bounds.size.height {
                    scroll_offset = pos.y + line_height - bounds.size.height;
                }
            }
        }
        scroll_offset = scroll_offset.clamp(px(0.), max_scroll);

        let origin = point(bounds.left(), bounds.top() - scroll_offset);

        // 선택 하이라이트: 시작/끝 행 + 중간 행 풀폭 (v1 근사).
        let mut selections = Vec::new();
        if !selected_range.is_empty() && !is_empty {
            if let (Some(start), Some(end)) = (
                layout.position_for_offset(selected_range.start),
                layout.position_for_offset(selected_range.end),
            ) {
                if (start.y - end.y).abs() < px(0.5) {
                    selections.push(fill(
                        Bounds::from_corners(
                            point(origin.x + start.x, origin.y + start.y),
                            point(origin.x + end.x, origin.y + start.y + line_height),
                        ),
                        self.selection_color,
                    ));
                } else {
                    selections.push(fill(
                        Bounds::from_corners(
                            point(origin.x + start.x, origin.y + start.y),
                            point(origin.x + wrap_width, origin.y + start.y + line_height),
                        ),
                        self.selection_color,
                    ));
                    let mut y = start.y + line_height;
                    while y + px(0.5) < end.y {
                        selections.push(fill(
                            Bounds::from_corners(
                                point(origin.x, origin.y + y),
                                point(origin.x + wrap_width, origin.y + y + line_height),
                            ),
                            self.selection_color,
                        ));
                        y += line_height;
                    }
                    selections.push(fill(
                        Bounds::from_corners(
                            point(origin.x, origin.y + end.y),
                            point(origin.x + end.x, origin.y + end.y + line_height),
                        ),
                        self.selection_color,
                    ));
                }
            }
        }

        let cursor = if selected_range.is_empty() {
            let pos = if is_empty {
                Some(point(px(0.), px(0.)))
            } else {
                layout.position_for_offset(cursor_offset)
            };
            pos.map(|pos| {
                fill(
                    Bounds::new(
                        point(origin.x + pos.x, origin.y + pos.y),
                        size(px(2.), line_height),
                    ),
                    self.cursor_color,
                )
            })
        } else {
            None
        };

        AreaPrepaint {
            layout: Some(layout),
            scroll_offset,
            cursor,
            selections,
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

        let layout = prepaint.layout.take().unwrap();
        let scroll_offset = prepaint.scroll_offset;
        let focused = focus_handle.is_focused(window);
        let selections = std::mem::take(&mut prepaint.selections);
        let cursor = prepaint.cursor.take();

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for selection in selections {
                window.paint_quad(selection);
            }

            let mut y = px(0.);
            for (ix, line) in layout.lines.iter().enumerate() {
                let height = layout.line_height * (line.wrap_boundaries.len() + 1);
                let screen_y = bounds.top() + y - scroll_offset;
                // 뷰포트 밖 줄은 그리지 않는다.
                if screen_y + height >= bounds.top() && screen_y <= bounds.bottom() {
                    line.paint(
                        point(bounds.left(), screen_y),
                        layout.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
                }
                y += height;
                let _ = ix;
            }

            if focused {
                if let Some(cursor) = cursor {
                    window.paint_quad(cursor);
                }
            }
        });

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.scroll_offset = scroll_offset;
            input.autoscroll = false;
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let focused = self.focus_handle.is_focused(window);

        div()
            .key_context("TextArea")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::escape))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .flex()
            .w_full()
            .p(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(if focused { t.accent } else { t.border })
            .bg(if self.disabled { t.surface } else { t.elevated })
            .text_size(px(13.))
            .text_color(if self.disabled {
                t.text_disabled
            } else {
                t.text
            })
            .child(TextAreaElement {
                input: cx.entity(),
                selection_color: with_alpha(t.accent, 0.30),
                cursor_color: t.accent,
                placeholder_color: t.text_disabled,
            })
    }
}

#[cfg(test)]
mod tests {
    /// 하드 라인 시작 오프셋 계산 — prepaint의 line_starts 로직과 동일한 규칙.
    fn line_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        for (ix, ch) in text.char_indices() {
            if ch == '\n' {
                starts.push(ix + 1);
            }
        }
        starts
    }

    fn split_offset(starts: &[usize], offset: usize) -> (usize, usize) {
        let mut ix = 0;
        for (i, start) in starts.iter().enumerate() {
            if *start > offset {
                break;
            }
            ix = i;
        }
        (ix, offset - starts[ix])
    }

    #[test]
    fn line_starts_counts_hard_lines() {
        assert_eq!(line_starts(""), vec![0]);
        assert_eq!(line_starts("abc"), vec![0]);
        assert_eq!(line_starts("ab\ncd"), vec![0, 3]);
        assert_eq!(line_starts("a\n\nb"), vec![0, 2, 3]);
        // 끝의 개행도 빈 마지막 라인을 만든다.
        assert_eq!(line_starts("a\n"), vec![0, 2]);
    }

    #[test]
    fn line_starts_handles_multibyte() {
        // '한'=3바이트 → 두 번째 라인 시작은 4.
        let text = "한\nb";
        assert_eq!(line_starts(text), vec![0, 4]);
    }

    #[test]
    fn split_offset_maps_into_line_local_offsets() {
        let text = "ab\ncd\nef";
        let starts = line_starts(text);
        assert_eq!(split_offset(&starts, 0), (0, 0));
        assert_eq!(split_offset(&starts, 2), (0, 2));
        assert_eq!(split_offset(&starts, 3), (1, 0));
        assert_eq!(split_offset(&starts, 5), (1, 2));
        assert_eq!(split_offset(&starts, 6), (2, 0));
        assert_eq!(split_offset(&starts, 8), (2, 2));
    }
}
