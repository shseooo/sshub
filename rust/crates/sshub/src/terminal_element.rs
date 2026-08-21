//! 터미널 그리드를 그리는 커스텀 `gpui::Element` (DESIGN-terminal.md §4).
//!
//! **CJK 정렬 규칙 (필수)**: 같은 스타일의 연속 셀을 배치로 묶되, 각 배치는
//! 자기 **그리드 원점 `(line, start_col)`** 에 페인트한다. shaped advance를
//! 배치 사이로 누적하지 않는다 — 그래야 한글/한자 폭이 폰트마다 달라도 열이
//! 어긋나지 않는다. `WIDE_CHAR_SPACER` 셀은 아예 방출하지 않고, 와이드/내로우가
//! 섞이는 지점에서도 배치를 끊어 배치 **안쪽** 드리프트까지 없앤다.
//!
//! 레이어 순서: 배경 rect → 검색 매치 rect → 선택 rect → 텍스트 배치 →
//! 커서 → IME 조합 텍스트 오버레이.

use std::ops::Range;

use gpui::{
    fill, point, px, relative, size, App, Bounds, CursorStyle, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, FocusHandle, Font, FontStyle, FontWeight, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollWheelEvent,
    ShapedLine, Size, Style, TextRun, UnderlineStyle, Window,
};
use sshub_terminal::backend::{AlacColor, Flags, Match, NamedColor, SelectionRange};
use sshub_terminal::{IndexedCell, Terminal, TerminalBounds, TerminalContent};

use crate::terminal_view::TerminalView;
use crate::theme::TerminalTheme;

/// 폰트 크기 대비 줄 높이 배수 (Zed 관례).
pub const LINE_HEIGHT_RATIO: f32 = 1.4;

// ---------------------------------------------------------------------------
// 색 변환 (순수 — 단위 테스트 대상)
// ---------------------------------------------------------------------------

fn rgba_to_hsla(c: gpui::Rgba) -> Hsla {
    Hsla::from(c)
}

/// 256색 큐브 인덱스 → RGB. xterm 표준(16..231 = 6³ 큐브, 232.. = 그레이스케일).
fn indexed_to_hsla(index: u8, theme: &TerminalTheme) -> Hsla {
    match index {
        0..=15 => rgba_to_hsla(theme.palette[index as usize]),
        16..=231 => {
            let i = index - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            let r = steps[(i / 36) as usize];
            let g = steps[((i % 36) / 6) as usize];
            let b = steps[(i % 6) as usize];
            Hsla::from(gpui::Rgba {
                r: r as f32 / 255.0,
                g: g as f32 / 255.0,
                b: b as f32 / 255.0,
                a: 1.0,
            })
        }
        _ => {
            let level = 8 + (index - 232) as u32 * 10;
            let v = level as f32 / 255.0;
            Hsla::from(gpui::Rgba { r: v, g: v, b: v, a: 1.0 })
        }
    }
}

/// alacritty 색 → gpui 색.
pub fn convert_color(color: &AlacColor, theme: &TerminalTheme) -> Hsla {
    match color {
        AlacColor::Named(named) => match named {
            NamedColor::Foreground | NamedColor::BrightForeground => {
                rgba_to_hsla(theme.foreground)
            }
            NamedColor::Background => rgba_to_hsla(theme.background),
            NamedColor::Cursor => rgba_to_hsla(theme.cursor),
            NamedColor::DimForeground => {
                let mut c = rgba_to_hsla(theme.foreground);
                c.a *= 0.7;
                c
            }
            other => {
                // Dim* 은 기본 8색으로, Bright* 는 8..15로 접는다.
                let idx = match other {
                    NamedColor::Black => 0,
                    NamedColor::Red => 1,
                    NamedColor::Green => 2,
                    NamedColor::Yellow => 3,
                    NamedColor::Blue => 4,
                    NamedColor::Magenta => 5,
                    NamedColor::Cyan => 6,
                    NamedColor::White => 7,
                    NamedColor::BrightBlack => 8,
                    NamedColor::BrightRed => 9,
                    NamedColor::BrightGreen => 10,
                    NamedColor::BrightYellow => 11,
                    NamedColor::BrightBlue => 12,
                    NamedColor::BrightMagenta => 13,
                    NamedColor::BrightCyan => 14,
                    NamedColor::BrightWhite => 15,
                    NamedColor::DimBlack => 0,
                    NamedColor::DimRed => 1,
                    NamedColor::DimGreen => 2,
                    NamedColor::DimYellow => 3,
                    NamedColor::DimBlue => 4,
                    NamedColor::DimMagenta => 5,
                    NamedColor::DimCyan => 6,
                    NamedColor::DimWhite => 7,
                    _ => return rgba_to_hsla(theme.foreground),
                };
                rgba_to_hsla(theme.palette[idx])
            }
        },
        AlacColor::Indexed(i) => indexed_to_hsla(*i, theme),
        AlacColor::Spec(rgb) => Hsla::from(gpui::Rgba {
            r: rgb.r as f32 / 255.0,
            g: rgb.g as f32 / 255.0,
            b: rgb.b as f32 / 255.0,
            a: 1.0,
        }),
    }
}

// ---------------------------------------------------------------------------
// 배치 계산 (순수 — CJK 정렬 골든 테스트 대상)
// ---------------------------------------------------------------------------

/// 한 번에 shape할 셀 묶음. `start_col`이 페인트 원점을 정한다.
#[derive(Clone, Debug, PartialEq)]
pub struct CellBatch {
    pub line: i32,
    pub start_col: usize,
    pub text: String,
    pub fg: AlacColor,
    pub flags: Flags,
    /// 이 배치가 와이드 문자로만 이루어졌는가 (셀 2칸/글자).
    pub wide: bool,
}

/// 배치를 끊는 기준. 이 값이 달라지면 새 배치가 시작된다.
#[derive(Clone, Copy, PartialEq)]
struct BatchKey {
    fg: AlacColor,
    flags: Flags,
    wide: bool,
}

fn styled_flags(flags: Flags) -> Flags {
    flags
        & (Flags::BOLD
            | Flags::ITALIC
            | Flags::UNDERLINE
            | Flags::DOUBLE_UNDERLINE
            | Flags::STRIKEOUT
            | Flags::DIM
            | Flags::INVERSE
            | Flags::HIDDEN)
}

/// 셀 스냅샷 → 텍스트 배치. 그리드 순서(줄 오름차순, 열 오름차순)를 가정한다.
pub fn build_batches(cells: &[IndexedCell]) -> Vec<CellBatch> {
    let mut out: Vec<CellBatch> = Vec::new();
    let mut current: Option<(BatchKey, CellBatch)> = None;
    let mut expected_col: usize = 0;
    let mut current_line: i32 = i32::MIN;

    for indexed in cells {
        let cell = &indexed.cell;
        // 와이드 문자의 뒤쪽 자리는 앞 셀이 이미 그린다 — 절대 방출하지 않는다.
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let line = indexed.point.line.0;
        let col = indexed.point.column.0;
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        let key = BatchKey { fg: cell.fg, flags: styled_flags(cell.flags), wide };

        // 공백은 그릴 것이 없다 — 배치를 끊어 뒤 배치가 자기 열에서 시작하게 한다.
        let blank = cell.c == ' ' && cell.zerowidth().is_none();

        let contiguous = line == current_line && col == expected_col;
        let same_style = current.as_ref().map(|(k, _)| *k == key).unwrap_or(false);

        if blank || !contiguous || !same_style {
            if let Some((_, batch)) = current.take() {
                out.push(batch);
            }
        }

        if blank {
            current_line = line;
            expected_col = col + if wide { 2 } else { 1 };
            continue;
        }

        match current.as_mut() {
            Some((_, batch)) => {
                batch.text.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    batch.text.extend(zw.iter().copied());
                }
            }
            None => {
                let mut text = String::new();
                text.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    text.extend(zw.iter().copied());
                }
                current = Some((
                    key,
                    CellBatch {
                        line,
                        start_col: col,
                        text,
                        fg: cell.fg,
                        flags: styled_flags(cell.flags),
                        wide,
                    },
                ));
            }
        }
        current_line = line;
        expected_col = col + if wide { 2 } else { 1 };
    }

    if let Some((_, batch)) = current.take() {
        out.push(batch);
    }
    out
}

/// 같은 배경색이 연속한 셀을 묶는다 (쿼드 수 감소).
#[derive(Clone, Debug, PartialEq)]
pub struct BgRun {
    pub line: i32,
    pub start_col: usize,
    pub width_cols: usize,
    pub bg: AlacColor,
}

/// 기본 배경이 아닌 런만 돌려준다. INVERSE 셀은 fg/bg가 뒤집힌다.
pub fn build_bg_runs(cells: &[IndexedCell]) -> Vec<BgRun> {
    let mut out: Vec<BgRun> = Vec::new();
    for indexed in cells {
        let cell = &indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let inverse = cell.flags.contains(Flags::INVERSE);
        let bg = if inverse { cell.fg } else { cell.bg };
        if !inverse && matches!(bg, AlacColor::Named(NamedColor::Background)) {
            continue;
        }
        let line = indexed.point.line.0;
        let col = indexed.point.column.0;
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        match out.last_mut() {
            Some(run)
                if run.line == line
                    && run.start_col + run.width_cols == col
                    && run.bg == bg =>
            {
                run.width_cols += width;
            }
            _ => out.push(BgRun { line, start_col: col, width_cols: width, bg }),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 커서
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Block,
    Bar,
    Underline,
    /// 비포커스 — 테두리만.
    Hollow,
}

/// gpui에 커서 프리미티브가 없어 직접 그린다 (DESIGN-terminal.md §4).
pub struct CursorLayout {
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    color: Hsla,
    kind: CursorKind,
    /// 블록 커서 위에 다시 그릴 글자 (배경색으로).
    text: Option<ShapedLine>,
}

impl CursorLayout {
    pub fn paint(&self, window: &mut Window, cx: &mut App) {
        let origin = self.origin;
        match self.kind {
            CursorKind::Block => {
                window.paint_quad(fill(
                    Bounds::new(origin, size(self.cell_width, self.line_height)),
                    self.color,
                ));
                if let Some(text) = &self.text {
                    // 채운 블록 위에 글자를 배경색으로 덮어 그린다.
                    let _ = text.paint(origin, self.line_height, window, cx);
                }
            }
            CursorKind::Bar => {
                window.paint_quad(fill(
                    Bounds::new(origin, size(px(2.0), self.line_height)),
                    self.color,
                ));
            }
            CursorKind::Underline => {
                window.paint_quad(fill(
                    Bounds::new(
                        point(origin.x, origin.y + self.line_height - px(2.0)),
                        size(self.cell_width, px(2.0)),
                    ),
                    self.color,
                ));
            }
            CursorKind::Hollow => {
                // 테두리 4개 — 1px 쿼드로 직접 그린다.
                let w = self.cell_width;
                let h = self.line_height;
                let t = px(1.0);
                let quads = [
                    Bounds::new(origin, size(w, t)),
                    Bounds::new(point(origin.x, origin.y + h - t), size(w, t)),
                    Bounds::new(origin, size(t, h)),
                    Bounds::new(point(origin.x + w - t, origin.y), size(t, h)),
                ];
                for b in quads {
                    window.paint_quad(fill(b, self.color));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Element
// ---------------------------------------------------------------------------

pub struct TerminalElement {
    terminal: Entity<Terminal>,
    view: Entity<TerminalView>,
    focus: FocusHandle,
    focused: bool,
    theme: TerminalTheme,
    font_family: String,
}

/// prepaint가 만들어 paint가 소비하는 한 프레임짜리 레이아웃.
pub struct LayoutState {
    hitbox: Hitbox,
    bg_quads: Vec<PaintQuad>,
    match_quads: Vec<PaintQuad>,
    selection_quads: Vec<PaintQuad>,
    /// (shaped line, 페인트 원점) — 원점은 항상 그리드에서 계산된다.
    text_batches: Vec<(ShapedLine, Point<Pixels>)>,
    link_underlines: Vec<PaintQuad>,
    cursor: Option<CursorLayout>,
    ime: Option<ImeOverlay>,
    terminal_bounds: TerminalBounds,
    /// cmd-호버로 링크 위에 있는가 — 커서 모양을 바꾼다.
    over_link: bool,
}

struct ImeOverlay {
    background: PaintQuad,
    underline: PaintQuad,
    line: ShapedLine,
    origin: Point<Pixels>,
}

impl TerminalElement {
    pub fn new(
        terminal: Entity<Terminal>,
        view: Entity<TerminalView>,
        focus: FocusHandle,
        focused: bool,
        theme: TerminalTheme,
        font_family: String,
    ) -> TerminalElement {
        TerminalElement { terminal, view, focus, focused, theme, font_family }
    }

    fn font(&self) -> Font {
        Font {
            family: self.font_family.clone().into(),
            features: Default::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }

    fn text_run(&self, text: &str, color: Hsla, flags: Flags) -> TextRun {
        let mut font = self.font();
        if flags.contains(Flags::BOLD) {
            font.weight = FontWeight::BOLD;
        }
        if flags.contains(Flags::ITALIC) {
            font.style = FontStyle::Italic;
        }
        let underline = if flags.intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE) {
            Some(UnderlineStyle { thickness: px(1.0), color: Some(color), wavy: false })
        } else {
            None
        };
        let strikethrough = if flags.contains(Flags::STRIKEOUT) {
            Some(gpui::StrikethroughStyle { thickness: px(1.0), color: Some(color) })
        } else {
            None
        };
        TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline,
            strikethrough,
        }
    }

    /// 그리드 좌표(뷰포트 기준 행) → 픽셀 원점.
    fn cell_origin(
        bounds: &Bounds<Pixels>,
        tb: &TerminalBounds,
        viewport_line: i32,
        col: usize,
    ) -> Point<Pixels> {
        point(
            bounds.origin.x + tb.cell_width * (col as f32),
            bounds.origin.y + tb.line_height * (viewport_line as f32),
        )
    }

    /// 그리드 line(히스토리 포함) → 화면 행 인덱스. 화면 밖이면 None.
    fn viewport_line(line: i32, display_offset: usize, screen_lines: usize) -> Option<i32> {
        let row = line + display_offset as i32;
        if row < 0 || row >= screen_lines as i32 {
            None
        } else {
            Some(row)
        }
    }

    fn selection_quads(
        selection: &SelectionRange,
        content: &TerminalContent,
        bounds: &Bounds<Pixels>,
        color: Hsla,
    ) -> Vec<PaintQuad> {
        let tb = &content.terminal_bounds;
        let columns = tb.columns();
        let screen_lines = tb.screen_lines();
        let mut quads = Vec::new();
        for line in selection.start.line.0..=selection.end.line.0 {
            let Some(row) = Self::viewport_line(line, content.display_offset, screen_lines) else {
                continue;
            };
            let (from, to) = if selection.is_block {
                (selection.start.column.0, selection.end.column.0)
            } else {
                let from =
                    if line == selection.start.line.0 { selection.start.column.0 } else { 0 };
                let to = if line == selection.end.line.0 {
                    selection.end.column.0
                } else {
                    columns.saturating_sub(1)
                };
                (from, to)
            };
            if to < from {
                continue;
            }
            let origin = Self::cell_origin(bounds, tb, row, from);
            let width = tb.cell_width * ((to - from + 1) as f32);
            quads.push(fill(Bounds::new(origin, size(width, tb.line_height)), color));
        }
        quads
    }

    fn match_quads(
        matches: &[Match],
        content: &TerminalContent,
        bounds: &Bounds<Pixels>,
        color: Hsla,
    ) -> Vec<PaintQuad> {
        let tb = &content.terminal_bounds;
        let screen_lines = tb.screen_lines();
        let columns = tb.columns();
        let mut quads = Vec::new();
        for m in matches {
            for line in m.start().line.0..=m.end().line.0 {
                let Some(row) = Self::viewport_line(line, content.display_offset, screen_lines)
                else {
                    continue;
                };
                let from = if line == m.start().line.0 { m.start().column.0 } else { 0 };
                let to = if line == m.end().line.0 {
                    m.end().column.0
                } else {
                    columns.saturating_sub(1)
                };
                if to < from {
                    continue;
                }
                let origin = Self::cell_origin(bounds, tb, row, from);
                let width = tb.cell_width * ((to - from + 1) as f32);
                quads.push(fill(Bounds::new(origin, size(width, tb.line_height)), color));
            }
        }
        quads
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = LayoutState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // 부모를 그대로 채운다.
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> LayoutState {
        let font_size = px(self.theme.font_size);
        let line_height = font_size * LINE_HEIGHT_RATIO;

        // 셀 폭은 폰트의 monospace advance에서 얻는다 (FitAddon 대체).
        let font = self.font();
        let font_id = window.text_system().resolve_font(&font);
        let cell_width = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or_else(|_| font_size * 0.6);

        let terminal_bounds = TerminalBounds::new(line_height, cell_width, bounds.size);

        // 프레임당 lock 1회: 크기 반영 + 스냅샷.
        let (content, matches, hovered) = self.terminal.update(cx, |terminal, cx| {
            terminal.set_size(terminal_bounds);
            // 첫 실제 레이아웃 = hydration 완료 (DESIGN-terminal.md §7).
            // 이 플래그가 서야 세션 계층이 스크롤백을 저장한다 — 한 번도 뜨지
            // 않은 터미널의 빈 버퍼가 저장된 히스토리를 덮어쓰지 않게 하는 게이트.
            if !terminal.hydrated && terminal_bounds.screen_lines() > 1 {
                terminal.hydrated = true;
            }
            terminal.sync(cx);
            (
                terminal.last_content.clone(),
                terminal.matches().to_vec(),
                terminal.hovered_link().cloned(),
            )
        });

        let tb = content.terminal_bounds;
        let screen_lines = tb.screen_lines();

        // 1) 배경
        let mut bg_quads = Vec::new();
        for run in build_bg_runs(&content.cells) {
            let Some(row) = Self::viewport_line(run.line, content.display_offset, screen_lines)
            else {
                continue;
            };
            let origin = Self::cell_origin(&bounds, &tb, row, run.start_col);
            let width = tb.cell_width * (run.width_cols as f32);
            bg_quads.push(fill(
                Bounds::new(origin, size(width, tb.line_height)),
                convert_color(&run.bg, &self.theme),
            ));
        }

        // 2) 검색 매치 / 3) 선택
        let mut match_color = rgba_to_hsla(self.theme.foreground);
        match_color.a = 0.22;
        let match_quads = Self::match_quads(&matches, &content, &bounds, match_color);

        let mut selection_color = rgba_to_hsla(self.theme.cursor);
        selection_color.a = 0.30;
        let selection_quads = match &content.selection {
            Some(range) => Self::selection_quads(range, &content, &bounds, selection_color),
            None => Vec::new(),
        };

        // 4) 텍스트 — 배치마다 그리드 원점에 페인트한다 (CJK 정렬의 핵심).
        let mut text_batches = Vec::new();
        for batch in build_batches(&content.cells) {
            if batch.flags.contains(Flags::HIDDEN) {
                continue;
            }
            let Some(row) = Self::viewport_line(batch.line, content.display_offset, screen_lines)
            else {
                continue;
            };
            let inverse = batch.flags.contains(Flags::INVERSE);
            let mut color = if inverse {
                rgba_to_hsla(self.theme.background)
            } else {
                convert_color(&batch.fg, &self.theme)
            };
            if batch.flags.contains(Flags::DIM) {
                color.a *= 0.7;
            }
            let run = self.text_run(&batch.text, color, batch.flags);
            let shaped = window.text_system().shape_line(
                batch.text.clone().into(),
                font_size,
                &[run],
                None,
            );
            let origin = Self::cell_origin(&bounds, &tb, row, batch.start_col);
            text_batches.push((shaped, origin));
        }

        // cmd-호버 링크 밑줄
        let mut link_underlines = Vec::new();
        if let Some(link) = hovered.as_ref() {
            let line = link.range.start().line.0;
            if let Some(row) = Self::viewport_line(line, content.display_offset, screen_lines) {
                let from = link.range.start().column.0;
                let to = link.range.end().column.0;
                let origin = Self::cell_origin(&bounds, &tb, row, from);
                let width = tb.cell_width * ((to.saturating_sub(from) + 1) as f32);
                link_underlines.push(fill(
                    Bounds::new(
                        point(origin.x, origin.y + tb.line_height - px(1.0)),
                        size(width, px(1.0)),
                    ),
                    rgba_to_hsla(self.theme.foreground),
                ));
            }
        }

        // 5) 커서 — IME 조합 중에는 숨긴다.
        let (ime_text, ime_selection) = {
            let view = self.view.read(cx);
            (view.marked_text().map(str::to_string), view.marked_selection())
        };
        let cursor = if ime_text.is_some() {
            None
        } else {
            build_cursor(
                self,
                &content,
                &bounds,
                font_size,
                window,
            )
        };

        // 6) IME 조합 오버레이 — 커서 그리드 위치에 터미널 폰트로.
        let ime = ime_text.and_then(|text| {
            if text.is_empty() {
                return None;
            }
            let row = Self::viewport_line(
                content.cursor.point.line.0,
                content.display_offset,
                screen_lines,
            )?;
            let origin = Self::cell_origin(&bounds, &tb, row, content.cursor.point.column.0);
            let color = rgba_to_hsla(self.theme.foreground);
            let run = self.text_run(&text, color, Flags::empty());
            let line = window.text_system().shape_line(
                text.clone().into(),
                font_size,
                &[run],
                None,
            );
            let width = line.width.max(tb.cell_width);
            // IME가 절을 고르고 있으면 그 구간만 두껍게 — 아니면 전체에 밑줄.
            let (underline_x, underline_w, thickness) = match &ime_selection {
                Some(sel) if sel.end <= text.len() => {
                    let from = line.x_for_index(sel.start);
                    let to = line.x_for_index(sel.end);
                    (from, (to - from).max(px(1.0)), px(2.0))
                }
                _ => (px(0.0), width, px(1.0)),
            };
            Some(ImeOverlay {
                background: fill(
                    Bounds::new(origin, size(width, tb.line_height)),
                    rgba_to_hsla(self.theme.background),
                ),
                // 조합 중임을 알리는 두꺼운 밑줄
                underline: fill(
                    Bounds::new(
                        point(origin.x + underline_x, origin.y + tb.line_height - thickness),
                        size(underline_w, thickness),
                    ),
                    color,
                ),
                line,
                origin,
            })
        });

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        LayoutState {
            hitbox,
            bg_quads,
            match_quads,
            selection_quads,
            text_batches,
            link_underlines,
            cursor,
            ime,
            terminal_bounds: tb,
            over_link: hovered.is_some(),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        layout: &mut LayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // IME 후보창 위치/커밋을 받으려면 paint 단계에서 입력 핸들러를 걸어야 한다.
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for quad in &layout.bg_quads {
                window.paint_quad(quad.clone());
            }
            for quad in &layout.match_quads {
                window.paint_quad(quad.clone());
            }
            for quad in &layout.selection_quads {
                window.paint_quad(quad.clone());
            }
            for (line, origin) in &layout.text_batches {
                // 배치 원점은 그리드에서 계산된 값 — 누적 advance가 아니다.
                let _ = line.paint(*origin, layout.terminal_bounds.line_height, window, cx);
            }
            for quad in &layout.link_underlines {
                window.paint_quad(quad.clone());
            }
            if let Some(cursor) = &layout.cursor {
                cursor.paint(window, cx);
            }
            if let Some(ime) = &layout.ime {
                window.paint_quad(ime.background.clone());
                let _ =
                    ime.line.paint(ime.origin, layout.terminal_bounds.line_height, window, cx);
                window.paint_quad(ime.underline.clone());
            }
        });

        let cursor_style =
            if layout.over_link { CursorStyle::PointingHand } else { CursorStyle::IBeam };
        window.set_cursor_style(cursor_style, &layout.hitbox);

        self.register_mouse_listeners(bounds, &layout.hitbox, window);
    }
}

fn build_cursor(
    element: &TerminalElement,
    content: &TerminalContent,
    bounds: &Bounds<Pixels>,
    font_size: Pixels,
    window: &mut Window,
) -> Option<CursorLayout> {
    use sshub_terminal::backend::CursorShape;

    let tb = content.terminal_bounds;
    let row = TerminalElement::viewport_line(
        content.cursor.point.line.0,
        content.display_offset,
        tb.screen_lines(),
    )?;
    let kind = if !element.focused {
        CursorKind::Hollow
    } else {
        match content.cursor.shape {
            CursorShape::Block => CursorKind::Block,
            CursorShape::Beam => CursorKind::Bar,
            CursorShape::Underline => CursorKind::Underline,
            CursorShape::HollowBlock => CursorKind::Hollow,
            CursorShape::Hidden => return None,
        }
    };
    let origin =
        TerminalElement::cell_origin(bounds, &tb, row, content.cursor.point.column.0);
    let color = rgba_to_hsla(element.theme.cursor);

    // 블록 커서는 채운 뒤 글자를 배경색으로 덮어 그린다 (가독성).
    let text = if kind == CursorKind::Block && content.cursor_char != ' ' {
        let s = content.cursor_char.to_string();
        let run = element.text_run(&s, rgba_to_hsla(element.theme.background), Flags::empty());
        Some(window.text_system().shape_line(s.into(), font_size, &[run], None))
    } else {
        None
    };

    Some(CursorLayout {
        origin,
        cell_width: tb.cell_width,
        line_height: tb.line_height,
        color,
        kind,
        text,
    })
}

impl TerminalElement {
    fn register_mouse_listeners(
        &self,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
    ) {
        let origin = bounds.origin;
        let hitbox_id = hitbox.id;

        // 클릭 — 포커스 + 선택 시작, cmd면 링크 열기.
        {
            let terminal = self.terminal.clone();
            let focus = self.focus.clone();
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox_id.is_hovered(window) {
                    return;
                }
                window.focus(&focus);
                if e.modifiers.platform && e.button == MouseButton::Left {
                    let opened =
                        terminal.update(cx, |terminal, cx| terminal.open_hovered_link(cx));
                    if opened {
                        return;
                    }
                }
                terminal.update(cx, |terminal, _| {
                    terminal.mouse_down(
                        e.position,
                        origin,
                        e.button,
                        e.click_count,
                        e.modifiers,
                    );
                });
                cx.refresh_windows();
            });
        }

        // 드래그 / cmd-호버
        {
            let terminal = self.terminal.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox_id.is_hovered(window) {
                    return;
                }
                let changed = terminal.update(cx, |terminal, _| {
                    if e.pressed_button.is_some() {
                        terminal.mouse_drag(e.position, origin, e.pressed_button, e.modifiers);
                    }
                    terminal.update_hovered_link(e.position, origin, e.modifiers.platform)
                });
                if changed || e.pressed_button.is_some() {
                    cx.refresh_windows();
                }
            });
        }

        // 버튼 뗌 — 선택 확정
        {
            let terminal = self.terminal.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let _ = window;
                terminal.update(cx, |terminal, cx| {
                    terminal.mouse_up(e.position, origin, e.button, e.modifiers, cx);
                });
            });
        }

        // 휠
        {
            let terminal = self.terminal.clone();
            let line_height = self.theme.font_size * LINE_HEIGHT_RATIO;
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox_id.is_hovered(window) {
                    return;
                }
                let delta = e.delta.pixel_delta(px(line_height));
                terminal.update(cx, |terminal, _| {
                    terminal.scroll_wheel(delta, e.position, origin, e.modifiers);
                });
                cx.refresh_windows();
            });
        }
    }
}

/// `TerminalBounds`가 `Size<Pixels>`를 받으므로 편의 생성자.
pub fn bounds_for(size_px: Size<Pixels>, cell_width: Pixels, line_height: Pixels) -> TerminalBounds {
    TerminalBounds::new(line_height, cell_width, size_px)
}

/// IME UTF-16 ↔ 바이트 오프셋 (뷰가 쓴다).
pub fn range_from_utf16(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
    offset_from_utf16(text, range_utf16.start)..offset_from_utf16(text, range_utf16.end)
}

pub fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
}

pub fn offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for c in text.chars() {
        if utf16 >= offset {
            break;
        }
        utf8 += c.len_utf8();
        utf16 += c.len_utf16();
    }
    utf8
}

pub fn offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for c in text.chars() {
        if utf8 >= offset {
            break;
        }
        utf8 += c.len_utf8();
        utf16 += c.len_utf16();
    }
    utf16
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshub_terminal::backend::{AlacPoint, Cell, Column, Line};

    fn cell(c: char, col: usize, flags: Flags) -> IndexedCell {
        let mut cell = Cell::default();
        cell.c = c;
        cell.flags = flags;
        IndexedCell { point: AlacPoint::new(Line(0), Column(col)), cell }
    }

    /// `가나다 abc 漢字` — 각 배치 원점이 정확히 `col`에서 시작해야 한다.
    #[test]
    fn cjk_batches_start_at_their_grid_column() {
        // 가(0,1) 나(2,3) 다(4,5) 공백(6) a(7) b(8) c(9) 공백(10) 漢(11,12) 字(13,14)
        let mut cells = Vec::new();
        let wide = Flags::WIDE_CHAR;
        let spacer = Flags::WIDE_CHAR_SPACER;
        let push_wide = |cells: &mut Vec<IndexedCell>, ch: char, col: usize| {
            cells.push(cell(ch, col, wide));
            cells.push(cell(' ', col + 1, spacer));
        };
        push_wide(&mut cells, '가', 0);
        push_wide(&mut cells, '나', 2);
        push_wide(&mut cells, '다', 4);
        cells.push(cell(' ', 6, Flags::empty()));
        cells.push(cell('a', 7, Flags::empty()));
        cells.push(cell('b', 8, Flags::empty()));
        cells.push(cell('c', 9, Flags::empty()));
        cells.push(cell(' ', 10, Flags::empty()));
        push_wide(&mut cells, '漢', 11);
        push_wide(&mut cells, '字', 13);

        let batches = build_batches(&cells);
        let summary: Vec<(usize, &str)> =
            batches.iter().map(|b| (b.start_col, b.text.as_str())).collect();
        assert_eq!(summary, vec![(0, "가나다"), (7, "abc"), (11, "漢字")]);
        // 와이드/내로우 배치가 섞이지 않는다
        assert!(batches[0].wide);
        assert!(!batches[1].wide);
        assert!(batches[2].wide);
    }

    #[test]
    fn wide_char_spacers_are_never_emitted() {
        let cells = vec![
            cell('漢', 0, Flags::WIDE_CHAR),
            cell(' ', 1, Flags::WIDE_CHAR_SPACER),
            cell('x', 2, Flags::empty()),
        ];
        let batches = build_batches(&cells);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].text, "漢");
        assert_eq!(batches[1].start_col, 2);
        assert!(!batches.iter().any(|b| b.text.contains(' ')));
    }

    #[test]
    fn style_changes_split_batches_at_the_right_column() {
        let red = AlacColor::Named(NamedColor::Red);
        let mut cells = vec![
            cell('a', 0, Flags::empty()),
            cell('b', 1, Flags::empty()),
            cell('c', 2, Flags::empty()),
        ];
        cells[2].cell.fg = red;
        let batches = build_batches(&cells);
        assert_eq!(batches.len(), 2);
        assert_eq!((batches[0].start_col, batches[0].text.as_str()), (0, "ab"));
        assert_eq!((batches[1].start_col, batches[1].text.as_str()), (2, "c"));
    }

    #[test]
    fn blank_cells_break_batches_so_columns_stay_aligned() {
        let cells = vec![
            cell('a', 0, Flags::empty()),
            cell(' ', 1, Flags::empty()),
            cell(' ', 2, Flags::empty()),
            cell('b', 3, Flags::empty()),
        ];
        let batches = build_batches(&cells);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].start_col, 0);
        assert_eq!(batches[1].start_col, 3);
    }

    #[test]
    fn empty_input_produces_no_batches() {
        assert!(build_batches(&[]).is_empty());
    }

    #[test]
    fn bg_runs_merge_adjacent_same_color_and_skip_default() {
        let red = AlacColor::Named(NamedColor::Red);
        let mut cells = vec![
            cell('a', 0, Flags::empty()),
            cell('b', 1, Flags::empty()),
            cell('c', 2, Flags::empty()),
        ];
        cells[0].cell.bg = red;
        cells[1].cell.bg = red;
        let runs = build_bg_runs(&cells);
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].start_col, runs[0].width_cols), (0, 2));
    }

    #[test]
    fn inverse_cells_use_the_foreground_as_background() {
        let mut cells = vec![cell('x', 0, Flags::INVERSE)];
        cells[0].cell.fg = AlacColor::Named(NamedColor::Green);
        let runs = build_bg_runs(&cells);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].bg, AlacColor::Named(NamedColor::Green));
    }

    #[test]
    fn indexed_colors_follow_the_xterm_cube() {
        let theme = crate::theme::Theme::default_dark().terminal;
        // 16 = 큐브의 (0,0,0) = 검정
        let black = indexed_to_hsla(16, &theme);
        assert!(black.l < 0.01);
        // 231 = (5,5,5) = 흰색
        let white = indexed_to_hsla(231, &theme);
        assert!(white.l > 0.99);
        // 0..15 는 팔레트 그대로
        assert_eq!(indexed_to_hsla(1, &theme), Hsla::from(theme.palette[1]));
    }

    #[test]
    fn named_colors_map_onto_the_palette() {
        let theme = crate::theme::Theme::default_dark().terminal;
        assert_eq!(
            convert_color(&AlacColor::Named(NamedColor::Red), &theme),
            Hsla::from(theme.palette[1])
        );
        assert_eq!(
            convert_color(&AlacColor::Named(NamedColor::BrightWhite), &theme),
            Hsla::from(theme.palette[15])
        );
        assert_eq!(
            convert_color(&AlacColor::Named(NamedColor::Foreground), &theme),
            Hsla::from(theme.foreground)
        );
    }

    #[test]
    fn viewport_line_clips_offscreen_rows() {
        assert_eq!(TerminalElement::viewport_line(0, 0, 24), Some(0));
        assert_eq!(TerminalElement::viewport_line(-3, 3, 24), Some(0));
        assert_eq!(TerminalElement::viewport_line(-1, 0, 24), None);
        assert_eq!(TerminalElement::viewport_line(24, 0, 24), None);
    }

    #[test]
    fn utf16_offset_conversion_round_trips_for_cjk() {
        let text = "가나다";
        assert_eq!(offset_to_utf16(text, 3), 1);
        assert_eq!(offset_from_utf16(text, 1), 3);
        assert_eq!(range_from_utf16(text, &(0..2)), 0..6);
        assert_eq!(range_to_utf16(text, &(0..6)), 0..2);
    }
}
