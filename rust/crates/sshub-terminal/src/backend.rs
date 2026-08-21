//! 유일한 alacritty_terminal import 지점 (seam — DESIGN-terminal.md §1).
//!
//! 이 크레이트의 다른 모듈은 `use crate::backend::…` 만 쓴다. upstream이 깨지면
//! Cargo 한 줄로 Zed 포크(rev 4c12966)로 스왑하고 여기만 손보면 된다.
#![allow(unused_imports)]

pub use alacritty_terminal::event::{
    Event as AlacEvent, EventListener, Notify, OnResize, VoidListener, WindowSize,
};
pub use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
pub use alacritty_terminal::grid::{Dimensions, Grid, GridIterator, Indexed, Row, Scroll};
pub use alacritty_terminal::index::{
    Boundary, Column, Direction, Line, Point as AlacPoint, Side,
};
pub use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
pub use alacritty_terminal::sync::FairMutex;
pub use alacritty_terminal::term::cell::{Cell, Flags, Hyperlink};
pub use alacritty_terminal::term::color::{Colors, COUNT as COLOR_COUNT};
pub use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
pub use alacritty_terminal::term::{
    cell, point_to_viewport, viewport_to_point, Config as TermConfig, Osc52, RenderableContent,
    RenderableCursor, Term, TermMode,
};
pub use alacritty_terminal::tty;
pub use alacritty_terminal::vte::ansi::{
    Color as AlacColor, CursorShape, CursorStyle, Handler, NamedColor, Processor, Rgb,
    StdSyncHandler,
};

/// `Processor`의 `Timeout` 타입 파라미터는 기본값이 있어도 추론되지 않는다 —
/// 로컬 주입에 쓰는 구체 타입을 고정해 둔다.
pub type AnsiProcessor = Processor<StdSyncHandler>;

/// `Term::new`에 넘길 최소 크기 타입. alacritty의 `term::test::TermSize`는
/// 테스트 전용이라 우리 것을 둔다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl TermSize {
    pub fn new(columns: usize, screen_lines: usize) -> TermSize {
        // alacritty는 2열·1행 미만에서 패닉한다 (MIN_COLUMNS / MIN_SCREEN_LINES).
        TermSize { columns: columns.max(2), screen_lines: screen_lines.max(1) }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// 라이브 스크롤백(히스토리) 라인 수. 영속 캡(1_000)과는 별개 — 출력 중
/// 위로 스크롤해도 최상단으로 튀지 않도록 넉넉히 잡는다.
pub const LIVE_SCROLLBACK_LINES: usize = 20_000;

/// 디스크에 저장하는 스크롤백 라인 상한 (Electron 판과 동일).
pub const PERSISTED_SCROLLBACK_LINES: usize = 1_000;
