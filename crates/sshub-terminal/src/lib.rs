//! sshub-terminal — alacritty를 감싼 터미널 **모델** (rust/docs/DESIGN-terminal.md).
//!
//! 렌더링은 하지 않는다. gpui `Entity<Terminal>`로 살면서
//!  - PTY를 소유하고 (spawn은 호출 스레드 = 메인 스레드 — Ctrl-C 시그널 마스크 회피)
//!  - alacritty EventLoop(전용 OS 스레드)가 보내는 이벤트를 배치 드레인해 적용하고
//!  - 프레임당 한 번 `sync()`로 그리드 스냅샷(`TerminalContent`)을 만든다.
//!
//! 모든 alacritty 타입은 [`backend`] 한 곳에서만 import한다.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use gpui::{px, Context, EventEmitter, Keystroke, Modifiers, MouseButton, Pixels, Point, Size, Task};

pub mod backend;
pub mod hyperlinks;
pub mod mappings;
pub mod pty_info;
pub mod scrollback;
pub mod search;
pub mod selection_util;

use backend::{
    AlacEvent, AlacPoint, Cell, Column, Dimensions, EventLoop, EventLoopSender, EventListener,
    FairMutex, Flags, Line, Match, Msg, Notify, Notifier, RenderableCursor, Selection,
    AnsiProcessor, SelectionRange, SelectionType, Side, Term, TermConfig, TermMode, WindowSize,
    LIVE_SCROLLBACK_LINES, PERSISTED_SCROLLBACK_LINES,
};
use hyperlinks::{LinkKind, LinkMatch};
use pty_info::PtyProcessInfo;
use search::SearchQuery;
use selection_util::trim_selection_trailing;

/// `Pixels`의 내부 f32 (필드가 private이라 `From`을 거친다).
#[inline]
fn fpx(p: Pixels) -> f32 {
    f32::from(p)
}

/// 터미널이 뷰로 올려보내는 이벤트.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// 그리드가 변했다 — 다시 그려라.
    Wakeup,
    Bell,
    TitleChanged(Option<String>),
    /// 자식 프로세스가 끝났다.
    CloseTerminal,
    /// cmd+클릭으로 링크를 열어달라.
    Open(LinkTarget),
    SelectionsChanged,
}

/// cmd+클릭 대상.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// 절대 경로 — 로컬 세션에서만, 존재 확인은 뷰가 한다.
    Path(String),
}

/// 마우스가 올라가 있는 링크 (cmd 홀드 시 밑줄).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoveredLink {
    pub target: LinkTarget,
    pub range: RangeInclusive<AlacPoint>,
}

/// 새 터미널을 띄우는 데 필요한 전부.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    /// PTY로 **쓰지 않고** 로컬 주입만 하는 배너 (SSH 접속 안내 등).
    pub banner: Option<String>,
    /// 복원할 스크롤백 ANSI — 배너보다 먼저 주입된다.
    pub restored_scrollback: Option<String>,
    pub initial_bounds: TerminalBounds,
}

impl SpawnSpec {
    /// 로컬 로그인 셸.
    pub fn local_shell(cwd: Option<PathBuf>) -> SpawnSpec {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        SpawnSpec {
            program,
            args: vec!["-l".to_string()],
            cwd,
            env: HashMap::new(),
            banner: None,
            restored_scrollback: None,
            initial_bounds: TerminalBounds::default(),
        }
    }
}

/// 픽셀 치수 + 셀 치수. 그리드 크기의 단일 소스.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalBounds {
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub size: Size<Pixels>,
}

impl TerminalBounds {
    pub fn new(line_height: Pixels, cell_width: Pixels, size: Size<Pixels>) -> TerminalBounds {
        TerminalBounds { cell_width, line_height, size }
    }

    pub fn columns(&self) -> usize {
        if fpx(self.cell_width) <= 0.0 {
            return 80;
        }
        ((fpx(self.size.width) / fpx(self.cell_width)).floor() as usize).max(2)
    }

    pub fn screen_lines(&self) -> usize {
        if fpx(self.line_height) <= 0.0 {
            return 24;
        }
        ((fpx(self.size.height) / fpx(self.line_height)).floor() as usize).max(1)
    }
}

impl Default for TerminalBounds {
    fn default() -> Self {
        // 80x24 — 실제 값은 첫 레이아웃에서 덮어쓴다.
        TerminalBounds {
            cell_width: px(8.0),
            line_height: px(16.0),
            size: Size { width: px(640.0), height: px(384.0) },
        }
    }
}

impl Dimensions for TerminalBounds {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        TerminalBounds::screen_lines(self)
    }

    fn columns(&self) -> usize {
        TerminalBounds::columns(self)
    }
}

impl From<TerminalBounds> for WindowSize {
    fn from(b: TerminalBounds) -> WindowSize {
        WindowSize {
            num_lines: b.screen_lines() as u16,
            num_cols: b.columns() as u16,
            cell_width: fpx(b.cell_width) as u16,
            cell_height: fpx(b.line_height) as u16,
        }
    }
}

/// 렌더러가 읽는 한 프레임짜리 그리드 스냅샷. `FairMutex`를 프레임당 한 번만
/// 잡기 위해 존재한다 (paint 중에는 절대 lock하지 않는다).
/// `Debug`는 없다 — alacritty `RenderableCursor`가 구현하지 않는다.
#[derive(Clone)]
pub struct TerminalContent {
    pub cells: Vec<IndexedCell>,
    pub mode: TermMode,
    pub display_offset: usize,
    pub selection: Option<SelectionRange>,
    pub selection_text: Option<String>,
    pub cursor: RenderableCursor,
    pub cursor_char: char,
    pub terminal_bounds: TerminalBounds,
    pub last_hovered_link: Option<HoveredLink>,
}

impl Default for TerminalContent {
    fn default() -> Self {
        TerminalContent {
            cells: Vec::new(),
            mode: TermMode::default(),
            display_offset: 0,
            selection: None,
            selection_text: None,
            cursor: RenderableCursor {
                shape: backend::CursorShape::Block,
                point: AlacPoint::new(Line(0), Column(0)),
            },
            cursor_char: ' ',
            terminal_bounds: TerminalBounds::default(),
            last_hovered_link: None,
        }
    }
}

/// 그리드 좌표가 붙은 셀 사본.
#[derive(Clone, Debug)]
pub struct IndexedCell {
    pub point: AlacPoint,
    pub cell: Cell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionPhase {
    Idle,
    Selecting,
}

/// alacritty EventLoop → gpui를 잇는 채널 어댑터.
#[derive(Clone)]
pub struct SshubListener(UnboundedSender<AlacEvent>);

impl EventListener for SshubListener {
    fn send_event(&self, event: AlacEvent) {
        // 수신자가 사라졌으면(터미널 drop) 조용히 버린다.
        let _ = self.0.unbounded_send(event);
    }
}

/// PTY를 띄우고 `Terminal`을 만드는 2단계 빌더.
/// 1단계(`new`)는 **호출 스레드에서** PTY를 spawn하고, 2단계(`subscribe`)가
/// gpui 엔티티 컨텍스트를 받아 이벤트 펌프를 건다.
pub struct TerminalBuilder {
    terminal: Terminal,
    events_rx: UnboundedReceiver<AlacEvent>,
}

impl TerminalBuilder {
    pub fn new(spec: SpawnSpec) -> anyhow::Result<TerminalBuilder> {
        let mut env = spec.env.clone();
        env.entry("TERM".to_string()).or_insert_with(|| "xterm-256color".to_string());
        env.entry("COLORTERM".to_string()).or_insert_with(|| "truecolor".to_string());

        let options = backend::tty::Options {
            shell: Some(backend::tty::Shell::new(spec.program.clone(), spec.args.clone())),
            working_directory: spec.cwd.clone(),
            drain_on_exit: false,
            env,
        };

        let bounds = spec.initial_bounds;
        let pty = backend::tty::new(&options, bounds.into(), 0)?;
        // EventLoop에 소유권을 넘기기 전에 fd/pid를 복사해 둔다.
        let pty_fd = pty.file().as_raw_fd();
        let shell_pid = pty.child().id();

        let (tx, events_rx) = unbounded();
        let listener = SshubListener(tx);

        let config = TermConfig {
            scrolling_history: LIVE_SCROLLBACK_LINES,
            ..TermConfig::default()
        };
        let term = Term::new(config, &bounds, listener.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(Arc::clone(&term), listener, pty, false, false)?;
        let pty_tx = event_loop.channel();
        // JoinHandle을 버리면 스레드는 detach된다 — 종료는 Msg::Shutdown으로 한다.
        let _io_thread = event_loop.spawn();

        let mut terminal = Terminal {
            term,
            pty_tx,
            parser: AnsiProcessor::new(),
            pty_info: PtyProcessInfo::new(pty_fd, shell_pid),
            last_content: TerminalContent {
                terminal_bounds: bounds,
                ..TerminalContent::default()
            },
            bounds,
            selection_phase: SelectionPhase::Idle,
            scroll_px: px(0.0),
            hovered_link: None,
            matches: Vec::new(),
            hydrated: false,
            title: None,
            child_exited: false,
            _pump: None,
        };

        // 복원 스크롤백 → 배너 순서. 둘 다 PTY로는 가지 않는다.
        if let Some(saved) = &spec.restored_scrollback {
            if !saved.is_empty() {
                terminal.inject_local(saved.as_bytes());
                terminal.inject_local(b"\r\n");
            }
        }
        if let Some(banner) = &spec.banner {
            terminal.inject_local(banner.as_bytes());
        }

        Ok(TerminalBuilder { terminal, events_rx })
    }

    /// 이벤트 펌프를 걸고 완성된 `Terminal`을 돌려준다.
    pub fn subscribe(mut self, cx: &mut Context<Terminal>) -> Terminal {
        let mut events_rx = std::mem::replace(&mut self.events_rx, unbounded().1);
        let pump = cx.spawn(async move |terminal, cx| {
            while let Some(first) = events_rx.next().await {
                // 배치 드레인: 한 번 깨어난 김에 쌓인 것을 모두 가져간다.
                // Electron 판의 8ms 코얼레싱을 이 배치 + 프레임 제한이 대체한다.
                let mut batch = vec![first];
                while batch.len() < MAX_EVENT_BATCH {
                    match events_rx.try_recv() {
                        Ok(event) => batch.push(event),
                        // 비었거나 송신자가 전부 사라짐 — 다음 await로 돌아간다.
                        Err(_) => break,
                    }
                }
                let applied = terminal.update(cx, |terminal, cx| {
                    let mut notify = false;
                    for event in batch.drain(..) {
                        notify |= terminal.apply_event(event, cx);
                    }
                    if notify {
                        cx.notify();
                        // 그리드가 변했다. 세션 계층이 이걸 받아 스크롤백 저장을
                        // 디바운스한다(§7) — 이 emit이 없으면 저장은 종료 훅에서만
                        // 일어나고, 강제 종료·크래시에서 히스토리가 통째로 날아간다.
                        //
                        // **배치당 한 번**이라는 점이 중요하다. alacritty 이벤트마다
                        // 알리면 출력이 쏟아질 때 저장 타이머를 초당 수백 번 다시
                        // 감게 된다. 여기서 이미 코얼레싱된 뒤라 그 비용이 없다.
                        cx.emit(Event::Wakeup);
                    }
                });
                if applied.is_err() {
                    break; // 엔티티가 사라졌다
                }
            }
        });
        self.terminal._pump = Some(pump);
        self.terminal
    }
}

/// 한 번에 적용할 alacritty 이벤트 상한 — UI 스레드를 오래 붙잡지 않기 위함.
const MAX_EVENT_BATCH: usize = 512;

pub struct Terminal {
    term: Arc<FairMutex<Term<SshubListener>>>,
    pty_tx: EventLoopSender,
    /// 로컬 주입 전용 파서 (PTY 스트림의 파서와 별개).
    parser: AnsiProcessor,
    pty_info: PtyProcessInfo,
    pub last_content: TerminalContent,
    bounds: TerminalBounds,
    selection_phase: SelectionPhase,
    /// 정밀 트랙패드 스크롤 잔여 픽셀.
    scroll_px: Pixels,
    hovered_link: Option<HoveredLink>,
    matches: Vec<Match>,
    /// 스크롤백 복원이 끝났는가 — false면 종료 flush를 건너뛴다(no-clobber).
    pub hydrated: bool,
    title: Option<String>,
    child_exited: bool,
    _pump: Option<Task<()>>,
}

impl EventEmitter<Event> for Terminal {}

impl Terminal {
    // ---- 입력 -------------------------------------------------------------

    /// 바이트를 PTY로 보낸다. 입력하면 하단으로 스크롤하고 선택을 해제한다.
    pub fn input(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut term = self.term.lock();
            term.scroll_display(backend::Scroll::Bottom);
            term.selection = None;
        }
        self.scroll_px = px(0.0);
        let notifier = Notifier(self.pty_tx.clone());
        notifier.notify(bytes);
    }

    /// 키스트로크를 처리했으면 true. false면 뷰가 앱 단축키로 넘긴다.
    pub fn try_keystroke(&mut self, keystroke: &Keystroke, alt_is_meta: bool) -> bool {
        let mode = self.last_content.mode;
        match mappings::keys::to_esc_bytes(keystroke, mode, alt_is_meta) {
            Some(bytes) => {
                self.input(bytes);
                true
            }
            None => false,
        }
    }

    /// 붙여넣기 — bracketed paste 모드를 존중한다.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // 붙여넣는 텍스트 안의 CR은 셸이 즉시 실행해 버리므로 그대로 둔다
        // (터미널 관례 — 사용자가 복사한 개행은 개행이다).
        let bytes = if self.last_content.mode.contains(TermMode::BRACKETED_PASTE) {
            let mut out = b"\x1b[200~".to_vec();
            out.extend_from_slice(text.as_bytes());
            out.extend_from_slice(b"\x1b[201~");
            out
        } else {
            text.as_bytes().to_vec()
        };
        self.input(bytes);
    }

    /// PTY로 **쓰지 않고** 그리드에 직접 ANSI를 먹인다 (배너/스크롤백 복원).
    /// alacritty가 재수출하는 vte 파서를 터미널마다 하나씩 들고 쓴다.
    pub fn inject_local(&mut self, ansi: &[u8]) {
        if ansi.is_empty() {
            return;
        }
        let mut term = self.term.lock();
        self.parser.advance(&mut *term, ansi);
    }

    // ---- 크기 -------------------------------------------------------------

    /// 레이아웃이 바뀌었을 때 호출. 실제로 그리드 치수가 변할 때만 반영한다.
    pub fn set_size(&mut self, bounds: TerminalBounds) {
        let changed = bounds.columns() != self.bounds.columns()
            || bounds.screen_lines() != self.bounds.screen_lines()
            || bounds.cell_width != self.bounds.cell_width
            || bounds.line_height != self.bounds.line_height;
        self.bounds = bounds;
        if !changed {
            return;
        }
        self.term.lock().resize(bounds);
        let _ = self.pty_tx.send(Msg::Resize(bounds.into()));
    }

    pub fn bounds(&self) -> TerminalBounds {
        self.bounds
    }

    // ---- 프레임 동기화 ------------------------------------------------------

    /// 프레임당 1회. lock을 한 번만 잡고 렌더용 스냅샷을 만든다.
    pub fn sync(&mut self, _cx: &mut Context<Self>) {
        let term = self.term.lock();
        let content = term.renderable_content();
        // display_iter는 소비되므로 스칼라 필드를 먼저 빼 둔다.
        let cursor = content.cursor;
        let display_offset = content.display_offset;
        let mode = content.mode;
        let selection = content.selection;

        let mut cells = Vec::with_capacity(content.display_iter.size_hint().0);
        for indexed in content.display_iter {
            cells.push(IndexedCell { point: indexed.point, cell: indexed.cell.clone() });
        }

        // 커서 자리의 글자 — 블록 커서 위에 다시 그려야 한다.
        let cursor_char = term.grid()[cursor.point].c;
        let selection_text = if selection.is_some() { term.selection_to_string() } else { None };
        drop(term);

        self.last_content = TerminalContent {
            cells,
            mode,
            display_offset,
            selection,
            selection_text,
            cursor,
            cursor_char,
            terminal_bounds: self.bounds,
            last_hovered_link: self.hovered_link.clone(),
        };
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn child_exited(&self) -> bool {
        self.child_exited
    }

    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn set_matches(&mut self, matches: Vec<Match>) {
        self.matches = matches;
    }

    // ---- 마우스 -----------------------------------------------------------

    /// 뷰포트 픽셀 좌표 → 그리드 좌표 (히스토리 오프셋 반영).
    pub fn grid_point(&self, position: Point<Pixels>, origin: Point<Pixels>) -> AlacPoint {
        let bounds = self.bounds;
        let rel_x = (fpx(position.x) - fpx(origin.x)).max(0.0);
        let rel_y = (fpx(position.y) - fpx(origin.y)).max(0.0);
        let col = ((rel_x / fpx(bounds.cell_width)).floor() as usize).min(bounds.columns() - 1);
        let row = ((rel_y / fpx(bounds.line_height)).floor() as usize)
            .min(bounds.screen_lines().saturating_sub(1));
        backend::viewport_to_point(
            self.last_content.display_offset,
            AlacPoint::<usize>::new(row, Column(col)),
        )
    }

    /// 셀의 어느 쪽에 가까운지 — 선택 경계 판정에 쓴다.
    fn grid_side(&self, position: Point<Pixels>, origin: Point<Pixels>) -> Side {
        let cell_w = fpx(self.bounds.cell_width);
        let rel_x = (fpx(position.x) - fpx(origin.x)).max(0.0);
        if (rel_x % cell_w) > cell_w / 2.0 {
            Side::Right
        } else {
            Side::Left
        }
    }

    pub fn mouse_down(
        &mut self,
        position: Point<Pixels>,
        origin: Point<Pixels>,
        button: MouseButton,
        click_count: usize,
        modifiers: Modifiers,
    ) {
        let point = self.grid_point(position, origin);
        let mode = self.last_content.mode;

        if mappings::mouse::should_report(mode, modifiers) {
            if let Some(bytes) =
                mappings::mouse::mouse_button_report(point, button, modifiers, true, mode)
            {
                self.write_raw(bytes);
            }
            return;
        }

        if button != MouseButton::Left {
            return;
        }

        // 클릭 횟수로 선택 종류가 갈린다 (DESIGN-terminal.md §4).
        let ty = match click_count {
            0 | 1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        let side = self.grid_side(position, origin);
        let mut term = self.term.lock();
        term.selection = Some(Selection::new(ty, point, side));
        drop(term);
        self.selection_phase = SelectionPhase::Selecting;
    }

    pub fn mouse_drag(
        &mut self,
        position: Point<Pixels>,
        origin: Point<Pixels>,
        pressed_button: Option<MouseButton>,
        modifiers: Modifiers,
    ) {
        let point = self.grid_point(position, origin);
        let mode = self.last_content.mode;

        if mappings::mouse::should_report(mode, modifiers) {
            if let Some(bytes) =
                mappings::mouse::mouse_moved_report(point, pressed_button, modifiers, mode)
            {
                self.write_raw(bytes);
            }
            return;
        }

        if self.selection_phase != SelectionPhase::Selecting {
            return;
        }
        let side = self.grid_side(position, origin);
        let mut term = self.term.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn mouse_up(
        &mut self,
        position: Point<Pixels>,
        origin: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let point = self.grid_point(position, origin);
        let mode = self.last_content.mode;

        if mappings::mouse::should_report(mode, modifiers) {
            if let Some(bytes) =
                mappings::mouse::mouse_button_report(point, button, modifiers, false, mode)
            {
                self.write_raw(bytes);
            }
            return;
        }

        if self.selection_phase == SelectionPhase::Selecting {
            self.selection_phase = SelectionPhase::Idle;
            cx.emit(Event::SelectionsChanged);
        }
    }

    /// 휠. `delta_px`는 이미 픽셀로 환산된 값(뷰가 `ScrollDelta::pixel_delta`로 변환).
    pub fn scroll_wheel(
        &mut self,
        delta_px: Point<Pixels>,
        position: Point<Pixels>,
        origin: Point<Pixels>,
        modifiers: Modifiers,
    ) {
        let line_height = fpx(self.bounds.line_height).max(1.0);
        self.scroll_px = px(fpx(self.scroll_px) + fpx(delta_px.y));
        let lines = (fpx(self.scroll_px) / line_height).trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_px = px(fpx(self.scroll_px) - lines as f32 * line_height);

        let mode = self.last_content.mode;
        let point = self.grid_point(position, origin);

        if mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift {
            if let Some(reports) = mappings::mouse::scroll_report(point, lines, modifiers, mode) {
                for bytes in reports {
                    self.write_raw(bytes);
                }
            }
            return;
        }

        // alt-screen은 히스토리가 없다 — 페이저가 반응하도록 화살표로 바꿔 보낸다.
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            self.write_raw(mappings::mouse::alt_scroll(lines));
            return;
        }

        self.term.lock().scroll_display(backend::Scroll::Delta(lines));
    }

    /// 리포트/응답 바이트 — 스크롤·선택을 건드리지 않고 그대로 쓴다.
    fn write_raw(&self, bytes: Vec<u8>) {
        let notifier = Notifier(self.pty_tx.clone());
        notifier.notify(bytes);
    }

    // ---- 선택 / 클립보드 ---------------------------------------------------

    /// 선택 텍스트 (행말 패딩 제거 적용).
    pub fn copy(&mut self) -> Option<String> {
        let text = self.term.lock().selection_to_string()?;
        let trimmed = trim_selection_trailing(&text);
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub fn has_selection(&self) -> bool {
        self.last_content.selection.is_some()
    }

    pub fn clear_selection(&mut self) {
        self.term.lock().selection = None;
    }

    pub fn select_all(&mut self) {
        let mut term = self.term.lock();
        let start = AlacPoint::new(term.topmost_line(), Column(0));
        let end = AlacPoint::new(term.bottommost_line(), Column(term.columns() - 1));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    // ---- 스크롤 -----------------------------------------------------------

    pub fn scroll_to_bottom(&mut self) {
        self.term.lock().scroll_display(backend::Scroll::Bottom);
        self.scroll_px = px(0.0);
    }

    pub fn scroll_page_up(&mut self) {
        self.term.lock().scroll_display(backend::Scroll::PageUp);
    }

    pub fn scroll_page_down(&mut self) {
        self.term.lock().scroll_display(backend::Scroll::PageDown);
    }

    // ---- 링크 -------------------------------------------------------------

    /// cmd 홀드 중 hover — 링크가 바뀌었으면 true(리페인트 필요).
    pub fn update_hovered_link(
        &mut self,
        position: Point<Pixels>,
        origin: Point<Pixels>,
        cmd_held: bool,
    ) -> bool {
        let next = if cmd_held {
            let point = self.grid_point(position, origin);
            self.link_at(point)
        } else {
            None
        };
        let changed = next != self.hovered_link;
        self.hovered_link = next;
        changed
    }

    pub fn hovered_link(&self) -> Option<&HoveredLink> {
        self.hovered_link.as_ref()
    }

    /// cmd+클릭 — 열 대상이 있으면 이벤트를 올린다.
    pub fn open_hovered_link(&mut self, cx: &mut Context<Self>) -> bool {
        match self.hovered_link.clone() {
            Some(link) => {
                cx.emit(Event::Open(link.target));
                true
            }
            None => false,
        }
    }

    /// 그리드 한 점의 링크. OSC-8 하이퍼링크가 있으면 그것이 우선.
    fn link_at(&self, point: AlacPoint) -> Option<HoveredLink> {
        let term = self.term.lock();
        let columns = term.columns();
        if columns == 0 {
            return None;
        }

        // 1) OSC-8 — 앱이 명시적으로 링크라고 표시한 셀
        if let Some(hyperlink) = term.grid()[point].hyperlink() {
            let uri = hyperlink.uri().to_string();
            let mut start = point.column.0;
            let mut end = point.column.0;
            let row_has = |c: usize| {
                term.grid()[AlacPoint::new(point.line, Column(c))]
                    .hyperlink()
                    .map(|h| h.uri() == uri)
                    .unwrap_or(false)
            };
            while start > 0 && row_has(start - 1) {
                start -= 1;
            }
            while end + 1 < columns && row_has(end + 1) {
                end += 1;
            }
            return Some(HoveredLink {
                target: LinkTarget::Url(uri),
                range: AlacPoint::new(point.line, Column(start))
                    ..=AlacPoint::new(point.line, Column(end)),
            });
        }

        // 2) 줄 텍스트에 정규식 — char 인덱스 ↔ Column 매핑을 함께 만든다
        let (text, columns_of_char) = line_text(&term, point.line);
        let char_index = columns_of_char.iter().position(|c| *c == point.column)?;
        let m: LinkMatch = hyperlinks::link_at(&text, char_index)?;
        let start_col = *columns_of_char.get(m.start)?;
        let end_col = *columns_of_char.get(m.end - 1)?;
        let target = match m.kind {
            LinkKind::Url => LinkTarget::Url(m.text),
            LinkKind::Path => LinkTarget::Path(m.text),
        };
        Some(HoveredLink {
            target,
            range: AlacPoint::new(point.line, start_col)..=AlacPoint::new(point.line, end_col),
        })
    }

    // ---- 검색 -------------------------------------------------------------

    pub fn find_matches(&self, query: &mut SearchQuery, limit: usize) -> Vec<Match> {
        let term = self.term.lock();
        query.find_matches(&term, limit)
    }

    /// background executor에서 검색하기 위한 grid 핸들 복제.
    pub fn term_handle(&self) -> Arc<FairMutex<Term<SshubListener>>> {
        Arc::clone(&self.term)
    }

    // ---- 스크롤백 / cwd ---------------------------------------------------

    pub fn serialize_scrollback(&self, max_lines: usize) -> String {
        scrollback::serialize(&self.term.lock(), max_lines)
    }

    /// 영속 캡을 적용한 직렬화.
    pub fn serialize_scrollback_for_disk(&self) -> String {
        self.serialize_scrollback(PERSISTED_SCROLLBACK_LINES)
    }

    /// 마지막 `max_lines` 행의 평문 — 탭을 끌 때 보여 주는 미리보기용.
    pub fn tail_lines(&self, max_lines: usize) -> Vec<String> {
        scrollback::plain_tail(&self.term.lock(), max_lines)
    }

    /// **블로킹 가능** — background executor에서 호출할 것.
    pub fn refresh_cwd(&mut self) -> Option<String> {
        self.pty_info.refresh_cwd()
    }

    pub fn cached_cwd(&self) -> Option<&str> {
        self.pty_info.cached_cwd()
    }

    pub fn shell_pid(&self) -> u32 {
        self.pty_info.shell_pid()
    }

    /// PTY 종료 — 재연결/앱 종료 시.
    pub fn kill(&mut self) {
        let _ = self.pty_tx.send(Msg::Shutdown);
    }

    // ---- 이벤트 적용 ------------------------------------------------------

    /// 하나 적용하고 "리페인트 필요"를 돌려준다.
    fn apply_event(&mut self, event: AlacEvent, cx: &mut Context<Self>) -> bool {
        match event {
            AlacEvent::Wakeup => true,
            AlacEvent::MouseCursorDirty | AlacEvent::CursorBlinkingChange => true,
            AlacEvent::Bell => {
                cx.emit(Event::Bell);
                false
            }
            AlacEvent::Title(title) => {
                self.title = Some(title);
                cx.emit(Event::TitleChanged(self.title.clone()));
                false
            }
            AlacEvent::ResetTitle => {
                self.title = None;
                cx.emit(Event::TitleChanged(None));
                false
            }
            AlacEvent::PtyWrite(text) => {
                // DA/DSR 등 터미널 자신의 응답 — 스크롤/선택을 건드리면 안 된다.
                self.write_raw(text.into_bytes());
                false
            }
            AlacEvent::TextAreaSizeRequest(format) => {
                let reply = format(self.bounds.into());
                self.write_raw(reply.into_bytes());
                false
            }
            AlacEvent::ColorRequest(index, format) => {
                let color = self.term.lock().colors()[index];
                // 정의되지 않은 색은 응답하지 않는다 (앱이 기본값을 쓰게).
                if let Some(rgb) = color {
                    self.write_raw(format(rgb).into_bytes());
                }
                false
            }
            AlacEvent::Exit | AlacEvent::ChildExit(_) => {
                self.child_exited = true;
                cx.emit(Event::CloseTerminal);
                true
            }
            // OSC 52 클립보드는 아직 연결하지 않는다 (DESIGN §4 범위 밖).
            AlacEvent::ClipboardStore(_, _) | AlacEvent::ClipboardLoad(_, _) => false,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // PTY 스레드를 남기지 않는다.
        let _ = self.pty_tx.send(Msg::Shutdown);
    }
}

/// 한 행의 텍스트와 "char 인덱스 → Column" 매핑.
/// WIDE_CHAR_SPACER는 건너뛰므로 char 하나가 셀 하나에 대응한다.
fn line_text<T: EventListener>(term: &Term<T>, line: Line) -> (String, Vec<Column>) {
    let columns = term.columns();
    let mut text = String::with_capacity(columns);
    let mut map = Vec::with_capacity(columns);
    let grid = term.grid();
    for c in 0..columns {
        let column = Column(c);
        let cell = &grid[AlacPoint::new(line, column)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        text.push(cell.c);
        map.push(column);
    }
    (text, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_bounds_floor_to_whole_cells() {
        let b = TerminalBounds::new(
            px(20.0),
            px(10.0),
            Size { width: px(105.0), height: px(85.0) },
        );
        assert_eq!(b.columns(), 10);
        assert_eq!(b.screen_lines(), 4);
    }

    #[test]
    fn terminal_bounds_never_go_below_the_alacritty_minimum() {
        let b = TerminalBounds::new(px(20.0), px(10.0), Size { width: px(1.0), height: px(1.0) });
        assert_eq!(b.columns(), 2);
        assert_eq!(b.screen_lines(), 1);
    }

    #[test]
    fn terminal_bounds_convert_to_window_size() {
        let b = TerminalBounds::new(
            px(16.0),
            px(8.0),
            Size { width: px(640.0), height: px(384.0) },
        );
        let ws: WindowSize = b.into();
        assert_eq!(ws.num_cols, 80);
        assert_eq!(ws.num_lines, 24);
        assert_eq!(ws.cell_width, 8);
        assert_eq!(ws.cell_height, 16);
    }

    #[test]
    fn zero_sized_cells_fall_back_to_80x24() {
        let b = TerminalBounds::new(px(0.0), px(0.0), Size { width: px(0.0), height: px(0.0) });
        assert_eq!(b.columns(), 80);
        assert_eq!(b.screen_lines(), 24);
    }

    #[test]
    fn local_shell_spec_uses_a_login_shell() {
        let spec = SpawnSpec::local_shell(None);
        assert_eq!(spec.args, vec!["-l".to_string()]);
        assert!(!spec.program.is_empty());
    }

    #[test]
    fn line_text_skips_wide_char_spacers_and_maps_columns() {
        use backend::{AnsiProcessor, TermConfig, TermSize, VoidListener};
        let mut term =
            Term::new(TermConfig::default(), &TermSize::new(20, 3), VoidListener);
        let mut parser = AnsiProcessor::new();
        parser.advance(&mut term, "가A".as_bytes());

        let (text, map) = line_text(&term, Line(0));
        assert!(text.starts_with("가A"));
        // '가'는 2셀을 먹으므로 다음 char는 Column(2)
        assert_eq!(map[0], Column(0));
        assert_eq!(map[1], Column(2));
    }
}
