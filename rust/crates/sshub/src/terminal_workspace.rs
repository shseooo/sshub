//! 터미널 워크스페이스 (DESIGN-terminal.md §5·§6) — 탭/분할/포커스/브로드캐스트를
//! 소유하는 창 스코프 뷰.
//!
//! 트리 연산 자체는 전부 `sshub-splits`(순수)가 하고, 여기서는
//! "어떤 연산을 언제 부르는지"와 세션 수명(레지스트리)·영속화만 다룬다.
//! 터미널 엔티티는 [`crate::session_registry`](앱 스코프)가 들고 있어 탭이
//! 창을 옮겨도 살아남는다.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    div, App, AppContext as _, Bounds, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, SharedString, Styled, Subscription,
    Window,
};
use serde::{Deserialize, Serialize};
use sshub_splits::{
    detach_pane, insert_at_index, leaves, merge_tab, move_pane, remove_leaf, rename_leaf,
    reorder_tabs, revive_ids, set_split_sizes, split_at, tab_title, tabs_except,
    tabs_from_inclusive, tabs_up_to_inclusive, DropSide, PaneNode, SessionId, SplitDirection,
    SplitId, TabId, TerminalLeaf, TerminalTab,
};
use sshub_terminal::search::SearchQuery;

use crate::i18n::{tr, Lang, TrKey};
use crate::keymap::{
    display_combo, ClosePane, FocusDown, FocusLeft, FocusRight, FocusUp, FontDecrease,
    FontIncrease, NewTab, SelectTab1, SelectTab2, SelectTab3, SelectTab4, SelectTab5, SelectTab6,
    SelectTab7, SelectTab8, SelectTab9, SplitDown, SplitRight, ToggleBroadcast,
};
use crate::session::pane_label;
use crate::session_registry::{self, SessionRegistry};
use crate::split_view::{
    drop_side, nearest_pane, render_pane_tree, resize_split, DividerGrab, FocusDir, GeometryRef,
    PaneDrag, PaneHandlers, PaneTreeCtx, TabDrag, WorkspaceGeometry,
};
use crate::state::{app_state, AppState, StateEvent};
use crate::tab_bar::{drop_boundary, render_tab_bar, TabBarCtx, TabBarHandlers};
use crate::terminal_view::{BroadcastInput, BroadcastSink, TerminalView};
use crate::theme::{theme, Theme};
use crate::ui::{ConfirmDialog, ContextMenu, ContextMenuItem, ModalOverlay, TextInput};

/// 검색 매치 상한 — 화면 하이라이트용이라 무제한으로 모을 이유가 없다.
const MAX_SEARCH_MATCHES: usize = 1000;

/// pane별 검색 상태 (§4). 바 UI는 아직 없고 모델만 살아 있다.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct PaneSearch {
    pub query: String,
    pub visible: bool,
}

/// 확인이 필요한 닫기 동작.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingClose {
    Pane(SessionId),
    Tab(TabId),
    Others(TabId),
    Right(TabId),
    Left(TabId),
}

/// `Settings.terminal_layout` 영속 포맷 — TS `{tabs, activeIndex}`와 동일.
/// (탭/split id는 transient라 저장하지 않고 복원 시 재발급한다.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLayout {
    pub tabs: Vec<TerminalTab>,
    #[serde(default)]
    pub active_index: usize,
}

/// 워크스페이스 → 창 셸 상향 이벤트.
///
/// 레이아웃을 **누가 저장하는지**가 다중 창의 핵심이다. 창이 여럿이면 각
/// 워크스페이스가 `Settings.terminal_layout`에 직접 쓸 수 없다 — 마지막에 쓴
/// 창이 나머지를 덮어쓰기 때문. 그래서 워크스페이스는 "바뀌었다"만 알리고,
/// 실제 저장은 창 셸을 거쳐 앱 스코프 `WindowManager`가 전담한다
/// (DESIGN-terminal.md §12 "어느 쪽이 최종 writer인지 정리").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceEvent {
    LayoutChanged,
}

pub struct TerminalWorkspace {
    registry: Entity<SessionRegistry>,
    state: Entity<AppState>,
    lang: Lang,
    tabs: Vec<TerminalTab>,
    active_tab: Option<TabId>,
    focused_pane: Option<SessionId>,
    broadcast_tabs: HashSet<TabId>,
    views: HashMap<SessionId, Entity<TerminalView>>,
    search: HashMap<SessionId, PaneSearch>,
    geometry: GeometryRef,
    rename: Option<(TabId, Entity<TextInput>)>,
    /// pane 라벨 인라인 편집 (헤더 더블클릭). 탭 이름 편집과 서로 독립이다.
    pane_rename: Option<(SessionId, Entity<TextInput>)>,
    /// 드래그가 지나가는 pane과 삽입 방향 — 드롭 미리보기의 유일한 근거.
    drag_over: Option<(SessionId, DropSide)>,
    confirm: Option<Entity<ConfirmDialog>>,
    /// 열려 있는 우클릭 메뉴 (pane·탭 공용 — 동시에 둘은 뜨지 않는다).
    menu: Option<Entity<ContextMenu>>,
    /// 그 메뉴의 dismiss 구독. 메뉴를 새로 열 때 교체되며(= 옛 구독 폐기),
    /// `_subscriptions`에 쌓아 두면 메뉴를 열 때마다 죽은 구독이 누적된다.
    menu_sub: Option<Subscription>,
    divider: Option<DividerGrab>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for TerminalWorkspace {}
impl EventEmitter<WorkspaceEvent> for TerminalWorkspace {}

impl TerminalWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_seeded(None, window, cx)
    }

    /// 창별 시드로 시작한다 (다중 창). `seed`는 `save_layout`과 같은
    /// `{tabs, activeIndex}` 값이며, 없으면 구버전 단일 레이아웃(설정)으로
    /// 떨어진다. 시드를 `load_layout`에 태우는 이유는 id 복원 규칙(세션 id는
    /// 보존, 탭/split id는 재발급)을 한 곳에만 두기 위해서다.
    pub fn new_seeded(
        seed: Option<serde_json::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = app_state(cx);
        let (paths, settings_lang, layout, font_size) = {
            let state = state.read(cx);
            (
                state.paths.clone(),
                state.settings.language.clone(),
                state.settings.terminal_layout.clone(),
                state.settings.appearance.terminal.font_size,
            )
        };
        let registry = session_registry::try_registry(cx)
            .unwrap_or_else(|| session_registry::init(&paths, cx));
        let catalog = {
            let state = state.read(cx);
            (state.servers.clone(), state.keys.clone())
        };
        registry.update(cx, |reg, _| reg.set_catalog(catalog.0, catalog.1));

        let lang = settings_lang
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or_else(Lang::detect);

        // 폰트 크기는 설정이 진실 — 전역 테마에 반영해 두고 시작한다.
        if (theme(cx).terminal.font_size - font_size).abs() > f32::EPSILON {
            let mut next = theme(cx).clone();
            next.terminal.font_size = font_size.clamp(10.0, 24.0);
            cx.set_global(next);
        }

        let state_sub = cx.subscribe(&state, |this: &mut Self, state, event, cx| match event {
            StateEvent::ServersChanged | StateEvent::KeysChanged => {
                let (servers, keys) = {
                    let state = state.read(cx);
                    (state.servers.clone(), state.keys.clone())
                };
                this.registry.update(cx, |reg, _| reg.set_catalog(servers, keys));
            }
            StateEvent::SettingsChanged => cx.notify(),
        });

        let mut workspace = TerminalWorkspace {
            registry,
            state,
            lang,
            tabs: Vec::new(),
            active_tab: None,
            focused_pane: None,
            broadcast_tabs: HashSet::new(),
            views: HashMap::new(),
            search: HashMap::new(),
            geometry: Rc::new(RefCell::new(WorkspaceGeometry::default())),
            rename: None,
            pane_rename: None,
            drag_over: None,
            confirm: None,
            menu: None,
            menu_sub: None,
            divider: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![state_sub],
        };

        // 창 시드가 있으면 그것이 진실 — 설정의 단일 레이아웃은 무시한다.
        let layout = seed.or(layout);
        if let Some((tabs, active)) = layout.as_ref().and_then(load_layout) {
            workspace.tabs = tabs;
            workspace.active_tab = active;
        }
        if workspace.tabs.is_empty() {
            workspace.push_local_tab(None);
        }
        // 시작 시 죽은 세션의 파일을 정리한다 (§7).
        let live = workspace.all_session_ids();
        workspace
            .registry
            .update(cx, |reg, _| reg.prune_scrollback(&live));
        workspace.sync_sessions(window, cx);
        workspace.focus_first_pane(window, cx);
        workspace
    }

    // ---- 조회 -------------------------------------------------------------

    pub fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    pub fn active_tab_id(&self) -> Option<&TabId> {
        self.active_tab.as_ref()
    }

    pub fn focused_pane(&self) -> Option<&SessionId> {
        self.focused_pane.as_ref()
    }

    pub fn is_broadcasting(&self, tab: &TabId) -> bool {
        self.broadcast_tabs.contains(tab)
    }

    pub fn pane_search(&self, session: &SessionId) -> Option<&PaneSearch> {
        self.search.get(session)
    }

    fn tab_index(&self, id: &TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == *id)
    }

    fn active_index(&self) -> Option<usize> {
        self.active_tab.as_ref().and_then(|id| self.tab_index(id))
    }

    fn tab_of_pane(&self, session: &SessionId) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| leaves(&t.root).iter().any(|l| l.session_id == *session))
            .map(|t| t.id.clone())
    }

    fn all_session_ids(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(|t| leaves(&t.root).into_iter().map(|l| l.session_id.0.clone()))
            .collect()
    }

    // ---- 탭/pane 생성 ------------------------------------------------------

    fn new_local_leaf(&self, cwd_from: Option<SessionId>) -> TerminalLeaf {
        let mut leaf = TerminalLeaf::new(
            SessionId::new(new_id()),
            None,
            pane_label(
                None,
                tr(self.lang, TrKey::TermLocal),
                tr(self.lang, TrKey::TermNewConnection),
            ),
        );
        leaf.cwd_from_session = cwd_from;
        leaf
    }

    fn push_local_tab(&mut self, cwd_from: Option<SessionId>) -> TabId {
        let leaf = self.new_local_leaf(cwd_from);
        let tab = TerminalTab {
            id: TabId::new(new_id()),
            root: PaneNode::Leaf(leaf.clone()),
            name: None,
        };
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.active_tab = Some(id.clone());
        self.focused_pane = Some(leaf.session_id);
        id
    }

    /// 새 로컬 탭 — 포커스된 pane의 cwd를 물려받는다.
    pub fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd_from = self.focused_pane.clone();
        self.push_local_tab(cwd_from);
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 서버 세션을 새 탭으로 연다 (사이드바/서버 목록에서 호출).
    pub fn open_server_tab(&mut self, server_id: i64, label: String, window: &mut Window, cx: &mut Context<Self>) {
        let leaf = TerminalLeaf::new(SessionId::new(new_id()), Some(server_id), label);
        let tab = TerminalTab {
            id: TabId::new(new_id()),
            root: PaneNode::Leaf(leaf.clone()),
            name: None,
        };
        self.active_tab = Some(tab.id.clone());
        self.focused_pane = Some(leaf.session_id);
        self.tabs.push(tab);
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 포커스된 pane을 분할하고 새 로컬 셸을 띄운다 (cwd는 원본에서 상속).
    pub fn split(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(tab_id), Some(source)) = (self.active_tab.clone(), self.focused_pane.clone())
        else {
            return;
        };
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let addition = self.new_local_leaf(Some(source.clone()));
        let new_session = addition.session_id.clone();
        let root = std::mem::replace(&mut self.tabs[index].root, PaneNode::Leaf(addition.clone()));
        self.tabs[index].root = split_at(
            root,
            &source,
            direction,
            addition,
            &SplitId::new(new_id()),
        );
        self.focused_pane = Some(new_session);
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    // ---- 닫기 -------------------------------------------------------------

    /// 포커스된 pane 닫기 — 위험하면(여러 pane 또는 서버 세션) 확인 모달.
    pub fn close_focused_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.focused_pane.clone() else {
            return;
        };
        let risky = self
            .tab_of_pane(&session)
            .and_then(|tab| self.tab_index(&tab))
            .is_some_and(|i| is_risky_close(&self.tabs[i].root));
        if risky {
            self.ask(PendingClose::Pane(session), TrKey::TermConfirmCloseTab, window, cx);
        } else {
            self.apply_close(PendingClose::Pane(session), window, cx);
        }
    }

    pub fn close_tab(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let risky = self
            .tab_index(&tab_id)
            .is_some_and(|i| is_risky_close(&self.tabs[i].root));
        if risky {
            self.ask(PendingClose::Tab(tab_id), TrKey::TermConfirmCloseTab, window, cx);
        } else {
            self.apply_close(PendingClose::Tab(tab_id), window, cx);
        }
    }

    pub fn close_other_tabs(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.ask(PendingClose::Others(tab_id), TrKey::TermConfirmCloseOthers, window, cx);
        }
    }

    pub fn close_tabs_to_the_right(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let has_right = self
            .tab_index(&tab_id)
            .is_some_and(|i| i + 1 < self.tabs.len());
        if has_right {
            self.ask(PendingClose::Right(tab_id), TrKey::TermConfirmCloseRight, window, cx);
        }
    }

    /// 왼쪽 탭 전부 닫기 — 오른쪽 형제와 같은 확인 절차를 탄다.
    pub fn close_tabs_to_the_left(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let has_left = self.tab_index(&tab_id).is_some_and(|i| i > 0);
        if has_left {
            // i18n에 "왼쪽 탭을 모두 닫을까요?" 문구가 없어 항목 라벨을 그대로
            // 확인 문구로 쓴다 (새 사용자 문자열을 만들지 않기 위해).
            self.ask(PendingClose::Left(tab_id), TrKey::TermCloseLeft, window, cx);
        }
    }

    fn ask(
        &mut self,
        pending: PendingClose,
        message: TrKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                tr(self.lang, TrKey::TermConfirmCloseTitle),
                tr(self.lang, message),
                tr(self.lang, TrKey::TermConfirmCloseAction),
                tr(self.lang, TrKey::CommonCancel),
                cx,
            )
            .danger(true)
            .on_result(move |confirmed, window, cx| {
                this.update(cx, |this, cx| {
                    this.confirm = None;
                    if confirmed {
                        this.apply_close(pending, window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
        });
        dialog.read(cx).focus(window);
        self.confirm = Some(dialog);
        cx.notify();
    }

    fn apply_close(&mut self, pending: PendingClose, window: &mut Window, cx: &mut Context<Self>) {
        match pending {
            PendingClose::Pane(session) => {
                let Some(tab_id) = self.tab_of_pane(&session) else {
                    return;
                };
                let Some(index) = self.tab_index(&tab_id) else {
                    return;
                };
                let root = std::mem::replace(
                    &mut self.tabs[index].root,
                    PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
                );
                match remove_leaf(root, &session) {
                    Some(root) => {
                        self.tabs[index].root = root;
                        // 남은 pane 중 첫 번째로 포커스를 옮긴다.
                        self.focused_pane = leaves(&self.tabs[index].root)
                            .first()
                            .map(|l| l.session_id.clone());
                    }
                    None => self.remove_tab_at(index),
                }
            }
            PendingClose::Tab(tab_id) => {
                if let Some(index) = self.tab_index(&tab_id) {
                    self.remove_tab_at(index);
                }
            }
            PendingClose::Others(tab_id) => {
                self.tabs = tabs_except(std::mem::take(&mut self.tabs), &tab_id);
                self.active_tab = Some(tab_id);
            }
            PendingClose::Right(tab_id) => {
                self.tabs = tabs_up_to_inclusive(std::mem::take(&mut self.tabs), &tab_id);
                if self.active_index().is_none() {
                    self.active_tab = self.tabs.last().map(|t| t.id.clone());
                }
            }
            PendingClose::Left(tab_id) => {
                self.tabs = tabs_from_inclusive(std::mem::take(&mut self.tabs), &tab_id);
                if self.active_index().is_none() {
                    self.active_tab = self.tabs.first().map(|t| t.id.clone());
                }
            }
        }
        // 여러 탭을 한 번에 닫으면 포커스가 사라진 세션을 가리킬 수 있다.
        // 비워 두면 `focus_active_pane`이 활성 탭의 첫 pane으로 되돌린다.
        if self
            .focused_pane
            .as_ref()
            .is_some_and(|s| self.tab_of_pane(s).is_none())
        {
            self.focused_pane = None;
        }
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 탭 제거 + active 폴백(= 마지막 탭). TS `closeTab`과 동일.
    fn remove_tab_at(&mut self, index: usize) {
        let removed = self.tabs.remove(index);
        self.broadcast_tabs.remove(&removed.id);
        if self.active_tab.as_ref() == Some(&removed.id) {
            self.active_tab = self.tabs.last().map(|t| t.id.clone());
            self.focused_pane = self
                .active_index()
                .and_then(|i| leaves(&self.tabs[i].root).first().map(|l| l.session_id.clone()));
        }
    }

    // ---- 재연결 -----------------------------------------------------------

    /// pane 재연결: 새 세션 id로 갈아끼우고 옛 PTY/스크롤백을 버린다.
    pub fn reconnect_pane(&mut self, session: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab_id) = self.tab_of_pane(&session) else {
            return;
        };
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let fresh = SessionId::new(new_id());
        let root = std::mem::replace(
            &mut self.tabs[index].root,
            PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
        );
        self.tabs[index].root =
            sshub_splits::reconnect_leaf(root, &session, &fresh);
        self.drop_session(&session, cx);
        if self.focused_pane.as_ref() == Some(&session) {
            self.focused_pane = Some(fresh);
        }
        self.sync_sessions(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 탭 전체 재연결 — 모든 pane에 새 세션 id를 발급한다.
    pub fn reconnect_tab(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let old: Vec<SessionId> = leaves(&self.tabs[index].root)
            .into_iter()
            .map(|l| l.session_id.clone())
            .collect();
        let root = std::mem::replace(
            &mut self.tabs[index].root,
            PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
        );
        self.tabs[index].root =
            sshub_splits::reconnect_all(root, &mut || SessionId::new(new_id()));
        for id in old {
            self.drop_session(&id, cx);
        }
        self.focused_pane = leaves(&self.tabs[index].root)
            .first()
            .map(|l| l.session_id.clone());
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn drop_session(&mut self, session: &SessionId, cx: &mut Context<Self>) {
        self.views.remove(session);
        self.search.remove(session);
        self.registry.update(cx, |reg, cx| reg.close(session, cx));
    }

    // ---- 이름 변경 --------------------------------------------------------

    pub fn start_rename(&mut self, tab_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let title = tab_title(&self.tabs[index]).to_string();
        let input = cx.new(|cx| TextInput::new(window, cx).with_text(title));
        let editing = tab_id.clone();
        let sub = cx.subscribe(&input, move |this: &mut Self, input, event, cx| {
            use crate::ui::InputEvent;
            match event {
                InputEvent::Submitted => {
                    let text = input.read(cx).text().to_string();
                    this.commit_rename(&editing, &text, cx);
                }
                InputEvent::Blurred => {
                    this.rename = None;
                    cx.notify();
                }
                InputEvent::Changed => {}
            }
        });
        self._subscriptions.push(sub);
        window.focus(&input.read(cx).focus_handle(cx));
        self.rename = Some((tab_id, input));
        cx.notify();
    }

    fn commit_rename(&mut self, tab_id: &TabId, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim();
        if let Some(index) = self.tab_index(tab_id) {
            // 빈 이름은 "이름 없음"으로 되돌린다 — 그러면 첫 pane 라벨을 따른다.
            self.tabs[index].name = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
        self.rename = None;
        self.persist_layout(cx);
        cx.notify();
    }

    /// pane 라벨 변경 (트리 내 leaf).
    pub fn rename_pane(&mut self, session: &SessionId, label: &str, cx: &mut Context<Self>) {
        let Some(tab_id) = self.tab_of_pane(session) else {
            return;
        };
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let root = std::mem::replace(
            &mut self.tabs[index].root,
            PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
        );
        self.tabs[index].root = rename_leaf(root, session, label);
        self.persist_layout(cx);
        cx.notify();
    }

    /// pane 헤더 더블클릭 — 인라인 라벨 편집. 탭 이름 편집과 같은 수명 규칙을
    /// 쓴다(Submit이면 반영, Blur면 취소).
    pub fn start_pane_rename(
        &mut self,
        session: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(label) = self
            .tabs
            .iter()
            .flat_map(|tab| leaves(&tab.root))
            .find(|leaf| leaf.session_id == session)
            .map(|leaf| leaf.label.clone())
        else {
            return;
        };
        let input = cx.new(|cx| TextInput::new(window, cx).with_text(label));
        let editing = session.clone();
        let sub = cx.subscribe(&input, move |this: &mut Self, input, event, cx| {
            use crate::ui::InputEvent;
            match event {
                InputEvent::Submitted => {
                    let text = input.read(cx).text().to_string();
                    this.commit_pane_rename(&editing, &text, cx);
                }
                InputEvent::Blurred => {
                    this.pane_rename = None;
                    cx.notify();
                }
                InputEvent::Changed => {}
            }
        });
        self._subscriptions.push(sub);
        window.focus(&input.read(cx).focus_handle(cx));
        self.pane_rename = Some((session, input));
        cx.notify();
    }

    fn commit_pane_rename(&mut self, session: &SessionId, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim();
        // 빈 라벨은 탭 제목(`tab.name ?? 첫 leaf.label`)까지 비워버리므로 무시한다.
        if !trimmed.is_empty() {
            self.rename_pane(session, trimmed, cx);
        }
        self.pane_rename = None;
        cx.notify();
    }

    // ---- 드래그 재배치 ------------------------------------------------------

    /// 드래그 호버 갱신. pane은 **자기 자신에 대해서만** 보고하므로, 이탈
    /// 보고는 현재 하이라이트가 그 pane일 때만 지운다 — 그래야 리스너 호출
    /// 순서(들어온 pane이 먼저인지 나간 pane이 먼저인지)와 무관하게 안정적이다.
    fn set_drag_over(&mut self, session: SessionId, side: Option<DropSide>, cx: &mut Context<Self>) {
        let next = match side {
            Some(side) => Some((session, side)),
            None if self.drag_over.as_ref().is_some_and(|(id, _)| *id == session) => None,
            None => return,
        };
        if self.drag_over != next {
            self.drag_over = next;
            cx.notify();
        }
    }

    fn reorder_or_detach_on_tab_bar(&mut self, boundary: usize, drag: Option<TabDrag>, pane: Option<PaneDrag>, window: &mut Window, cx: &mut Context<Self>) {
        self.drag_over = None;
        if let Some(TabDrag { tab_id }) = drag {
            self.tabs = reorder_tabs(std::mem::take(&mut self.tabs), &tab_id, boundary);
        } else if let Some(PaneDrag { tab_id, session_id }) = pane {
            let new_tab = TabId::new(new_id());
            self.tabs = detach_pane(
                std::mem::take(&mut self.tabs),
                &tab_id,
                &session_id,
                Some(boundary),
                new_tab.clone(),
            );
            if self.tab_index(&new_tab).is_some() {
                self.active_tab = Some(new_tab);
                self.focused_pane = Some(session_id);
            }
        }
        self.sync_sessions(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 드래그 드롭의 모델 연산 — 탭 전체(`drag`) 또는 pane 하나(`pane`)를
    /// `target` pane 옆에 붙인다. 드롭 방향은 현재 포인터 위치에서 정한다.
    /// (테스트에서 직접 호출하려고 공개해 둔다 — 드래그 제스처는 자동화가 어렵다.)
    pub fn drop_on_pane(&mut self, target: SessionId, drag: Option<TabDrag>, pane: Option<PaneDrag>, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.side_for(&target, window);
        // 미리보기는 여기서 소임을 다한다 — 다음 드래그가 첫 이동 이벤트를
        // 받기 전에 옛 하이라이트가 깜빡이지 않도록 즉시 지운다.
        self.drag_over = None;
        let Some(dst_tab) = self.tab_of_pane(&target) else {
            return;
        };
        if let Some(TabDrag { tab_id }) = drag {
            if tab_id == dst_tab {
                return;
            }
            self.tabs = merge_tab(
                std::mem::take(&mut self.tabs),
                &tab_id,
                &dst_tab,
                &target,
                side,
                &SplitId::new(new_id()),
            );
            self.active_tab = Some(dst_tab);
        } else if let Some(PaneDrag { tab_id, session_id }) = pane {
            if session_id == target {
                return;
            }
            if tab_id == dst_tab {
                let Some(index) = self.tab_index(&dst_tab) else {
                    return;
                };
                let root = std::mem::replace(
                    &mut self.tabs[index].root,
                    PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
                );
                self.tabs[index].root =
                    move_pane(root, &session_id, &target, side, &SplitId::new(new_id()));
            } else {
                // 다른 탭의 pane: 원본에서 떼어내 임시 탭으로 만든 뒤 병합한다
                // (detach + merge 조합이 TS 판의 크로스 탭 이동과 동일한 결과).
                let staging = TabId::new(new_id());
                self.tabs = detach_pane(
                    std::mem::take(&mut self.tabs),
                    &tab_id,
                    &session_id,
                    None,
                    staging.clone(),
                );
                if self.tab_index(&staging).is_some() {
                    self.tabs = merge_tab(
                        std::mem::take(&mut self.tabs),
                        &staging,
                        &dst_tab,
                        &target,
                        side,
                        &SplitId::new(new_id()),
                    );
                }
            }
            self.focused_pane = Some(session_id);
            self.active_tab = Some(dst_tab);
        }
        self.sync_sessions(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    fn side_for(&self, target: &SessionId, window: &mut Window) -> DropSide {
        self.geometry
            .borrow()
            .panes
            .get(target)
            .map(|bounds| drop_side(*bounds, window.mouse_position()))
            .unwrap_or(DropSide::Right)
    }

    // ---- 컨텍스트 메뉴 -----------------------------------------------------

    /// 사용자가 지정한 단축키의 표시 라벨. 미지정이면 기본값으로 떨어진다
    /// (`keymap::register_all`이 실제로 등록하는 값과 같은 규칙).
    fn shortcut_hint(&self, action: &str, cx: &App) -> Option<SharedString> {
        let settings = &self.state.read(cx).settings;
        let defaults = sshub_core::settings::default_shortcuts();
        let combo = settings
            .shortcuts
            .get(action)
            .filter(|c| crate::keymap::is_valid_combo(c))
            .or_else(|| defaults.get(action))?;
        Some(SharedString::from(display_combo(combo)))
    }

    fn open_menu(
        &mut self,
        at: Point<Pixels>,
        items: Vec<ContextMenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| ContextMenu::new(at, items, cx));
        let dismiss = cx.subscribe_in(
            &menu,
            window,
            |this: &mut Self, _menu, _: &DismissEvent, window, cx| {
                this.menu = None;
                // 항목 동작이 확인 모달을 띄웠다면 포커스를 되찾아 오면 안 된다
                // — 방금 연 모달에서 포커스를 빼앗는 꼴이 된다.
                if this.confirm.is_none() {
                    this.focus_active_pane(window, cx);
                }
                cx.notify();
            },
        );
        menu.read(cx).focus(window);
        self.menu_sub = Some(dismiss);
        self.menu = Some(menu);
        cx.notify();
    }

    /// pane 우클릭 메뉴. 복사/붙여넣기는 **우클릭한 pane**을 대상으로 해야 하므로
    /// 먼저 그 pane에 포커스를 준다.
    pub fn open_pane_menu(
        &mut self,
        session: SessionId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane(session.clone(), window, cx);
        let lang = self.lang;
        let this = cx.entity().downgrade();
        // ⌘C/⌘V는 터미널 뷰가 직접 처리하는 고정 키다(리바인딩 대상 아님).
        let copy_hint = Some(SharedString::from(display_combo("cmd-c")));
        let paste_hint = Some(SharedString::from(display_combo("cmd-v")));
        let split_right_hint = self.shortcut_hint("splitRight", cx);
        let split_down_hint = self.shortcut_hint("splitDown", cx);
        let close_hint = self.shortcut_hint("closePane", cx);

        let items = vec![
            {
                let this = this.clone();
                ContextMenuItem::entry(tr(lang, TrKey::ShortcutSplitRight), move |window, cx| {
                    this.update(cx, |this, cx| this.split(SplitDirection::Row, window, cx)).ok();
                })
                .hint(split_right_hint)
            },
            {
                let this = this.clone();
                ContextMenuItem::entry(tr(lang, TrKey::ShortcutSplitDown), move |window, cx| {
                    this.update(cx, |this, cx| this.split(SplitDirection::Column, window, cx)).ok();
                })
                .hint(split_down_hint)
            },
            ContextMenuItem::separator(),
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermCopy), move |_window, cx| {
                    this.update(cx, |this, cx| {
                        if let Some(view) = this.views.get(&session).cloned() {
                            view.update(cx, |view, cx| view.copy(cx));
                        }
                    })
                    .ok();
                })
                .hint(copy_hint)
            },
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermPaste), move |_window, cx| {
                    this.update(cx, |this, cx| {
                        if let Some(view) = this.views.get(&session).cloned() {
                            view.update(cx, |view, cx| view.paste(cx));
                        }
                    })
                    .ok();
                })
                .hint(paste_hint)
            },
            ContextMenuItem::separator(),
            {
                let this = this.clone();
                let session = session.clone();
                ContextMenuItem::entry(tr(lang, TrKey::TermReconnect), move |window, cx| {
                    this.update(cx, |this, cx| this.reconnect_pane(session.clone(), window, cx)).ok();
                })
            },
            ContextMenuItem::entry(tr(lang, TrKey::ShortcutClosePane), move |window, cx| {
                this.update(cx, |this, cx| this.close_focused_pane(window, cx)).ok();
            })
            .hint(close_hint),
        ];
        self.open_menu(at, items, window, cx);
    }

    /// 탭 우클릭 메뉴. **활성 탭을 바꾸지 않는다** — 우클릭한 탭이 대상이다.
    /// 아무 일도 하지 않을 항목(끝 탭의 "오른쪽 닫기" 등)은 지우지 않고 비활성으로
    /// 남긴다 — 항목 위치가 흔들리면 근육 기억이 깨진다.
    pub fn open_tab_menu(
        &mut self,
        tab_id: TabId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let lang = self.lang;
        let last = self.tabs.len() - 1;
        let only_tab = self.tabs.len() <= 1;
        let this = cx.entity().downgrade();

        let items = vec![
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseTab), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tab(id.clone(), window, cx)).ok();
                })
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseOthers), move |window, cx| {
                    this.update(cx, |this, cx| this.close_other_tabs(id.clone(), window, cx)).ok();
                })
                .disabled(only_tab)
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseRight), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tabs_to_the_right(id.clone(), window, cx))
                        .ok();
                })
                .disabled(index == last)
            },
            {
                let (this, id) = (this.clone(), tab_id.clone());
                ContextMenuItem::entry(tr(lang, TrKey::TermCloseLeft), move |window, cx| {
                    this.update(cx, |this, cx| this.close_tabs_to_the_left(id.clone(), window, cx))
                        .ok();
                })
                .disabled(index == 0)
            },
            ContextMenuItem::separator(),
            ContextMenuItem::entry(tr(lang, TrKey::TermReconnect), move |window, cx| {
                this.update(cx, |this, cx| this.reconnect_tab(tab_id.clone(), window, cx)).ok();
            }),
        ];
        self.open_menu(at, items, window, cx);
    }

    // ---- 포커스 -----------------------------------------------------------

    pub fn focus_pane(&mut self, session: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tab_of_pane(&session) {
            self.active_tab = Some(tab);
        }
        self.focused_pane = Some(session.clone());
        if let Some(view) = self.views.get(&session) {
            window.focus(&view.read(cx).focus_handle(cx));
        }
        cx.notify();
    }

    fn focus_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.focused_pane.clone() else {
            return self.focus_first_pane(window, cx);
        };
        if let Some(view) = self.views.get(&session) {
            window.focus(&view.read(cx).focus_handle(cx));
        }
    }

    fn focus_first_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active_index() else {
            return;
        };
        let Some(first) = leaves(&self.tabs[index].root).first().map(|l| l.session_id.clone()) else {
            return;
        };
        self.focused_pane = Some(first.clone());
        if let Some(view) = self.views.get(&first) {
            window.focus(&view.read(cx).focus_handle(cx));
        }
    }

    /// 탭 직접 선택. `index`가 `None`이면 **마지막 탭**(⌘9의 macOS/Chrome/Zed
    /// 관례). 없는 번호는 아무 일도 하지 않는다.
    pub fn select_tab_at(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let target = match index {
            Some(i) => self.tabs.get(i),
            None => self.tabs.last(),
        };
        let Some(id) = target.map(|t| t.id.clone()) else {
            return;
        };
        self.active_tab = Some(id);
        // 탭만 바꾸고 포커스를 두면 키 입력이 안 보이는 pane으로 흘러간다.
        self.focus_first_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// 방향 포커스 이동 — 현재 탭 안에서 기하적으로 가장 가까운 pane.
    pub fn focus_direction(&mut self, dir: FocusDir, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.focused_pane.clone() else {
            return;
        };
        let panes = self.visible_pane_bounds();
        if let Some(next) = nearest_pane(&panes, &current, dir) {
            self.focus_pane(next, window, cx);
        }
    }

    /// 활성 탭에 속한 pane들의 화면 사각형 (렌더 순서 = 결정적).
    fn visible_pane_bounds(&self) -> Vec<(SessionId, Bounds<Pixels>)> {
        let Some(index) = self.active_index() else {
            return Vec::new();
        };
        let geometry = self.geometry.borrow();
        leaves(&self.tabs[index].root)
            .into_iter()
            .filter_map(|leaf| {
                geometry
                    .panes
                    .get(&leaf.session_id)
                    .map(|b| (leaf.session_id.clone(), *b))
            })
            .collect()
    }

    // ---- 브로드캐스트 ------------------------------------------------------

    pub fn toggle_broadcast(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab.clone() else {
            return;
        };
        if !self.broadcast_tabs.remove(&tab) {
            self.broadcast_tabs.insert(tab);
        }
        cx.notify();
    }

    /// 포커스 pane이 방금 보낸 입력을 같은 탭의 나머지 pane에 복제한다 (§6).
    fn fan_out(&mut self, source: SessionId, input: &BroadcastInput, cx: &mut App) {
        let Some(tab_id) = self.tab_of_pane(&source) else {
            return;
        };
        if !self.broadcast_tabs.contains(&tab_id) {
            return;
        }
        let Some(index) = self.tab_index(&tab_id) else {
            return;
        };
        let targets = broadcast_targets(&self.tabs[index].root, &source);
        let registry = self.registry.read(cx);
        let terminals: Vec<_> = targets.iter().filter_map(|id| registry.get(id)).collect();
        for terminal in terminals {
            terminal.update(cx, |terminal, _| match input {
                BroadcastInput::Keystroke(keystroke) => {
                    terminal.try_keystroke(keystroke, true);
                }
                BroadcastInput::Text(text) => terminal.input(text.as_bytes().to_vec()),
                BroadcastInput::Paste(text) => terminal.paste(text),
            });
        }
    }

    // ---- 검색 (모델만 — 바 UI는 미구현, §11) --------------------------------

    pub fn set_pane_search(&mut self, session: &SessionId, query: &str, cx: &mut Context<Self>) {
        let entry = self.search.entry(session.clone()).or_default();
        entry.query = query.to_string();
        entry.visible = true;
        let Some(terminal) = self.registry.read(cx).get(session) else {
            return;
        };
        let matches = match SearchQuery::new(query) {
            Some(mut q) => terminal.read(cx).find_matches(&mut q, MAX_SEARCH_MATCHES),
            None => Vec::new(),
        };
        terminal.update(cx, |terminal, _| terminal.set_matches(matches));
        cx.notify();
    }

    pub fn clear_pane_search(&mut self, session: &SessionId, cx: &mut Context<Self>) {
        self.search.remove(session);
        if let Some(terminal) = self.registry.read(cx).get(session) {
            terminal.update(cx, |terminal, _| terminal.set_matches(Vec::new()));
        }
        cx.notify();
    }

    // ---- 폰트 -------------------------------------------------------------

    pub fn adjust_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (theme(cx).terminal.font_size + delta).clamp(10.0, 24.0);
        let mut updated = theme(cx).clone();
        updated.terminal.font_size = next;
        cx.set_global(updated);
        self.state.update(cx, |state, cx| {
            state.update_settings(|s| s.appearance.terminal.font_size = next, cx);
        });
        cx.notify();
    }

    // ---- 세션 동기화 / 영속화 ----------------------------------------------

    /// 트리 ↔ 살아 있는 세션/뷰를 맞춘다. 트리에서 사라진 세션은 닫고,
    /// 새로 생긴 leaf는 PTY를 띄운다. 모든 트리 변경 뒤에 호출한다.
    fn sync_sessions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live: HashSet<SessionId> = self
            .tabs
            .iter()
            .flat_map(|t| {
                leaves(&t.root)
                    .into_iter()
                    .map(|l| l.session_id.clone())
                    .collect::<Vec<_>>()
            })
            .collect();

        let stale: Vec<SessionId> = self
            .views
            .keys()
            .filter(|id| !live.contains(*id))
            .cloned()
            .collect();
        for id in stale {
            self.drop_session(&id, cx);
        }

        let pending: Vec<TerminalLeaf> = self
            .tabs
            .iter()
            .flat_map(|t| leaves(&t.root).into_iter().cloned().collect::<Vec<_>>())
            .filter(|leaf| !self.views.contains_key(&leaf.session_id))
            .collect();
        for leaf in pending {
            self.start_leaf(&leaf, window, cx);
        }

        // cwd 상속원은 1회용이다 — 두 번째 spawn에서 남의 디렉터리를 물려받지
        // 않도록 소비 후 지운다.
        for tab in &mut self.tabs {
            clear_cwd_sources(&mut tab.root);
        }
    }

    fn start_leaf(&mut self, leaf: &TerminalLeaf, _window: &mut Window, cx: &mut Context<Self>) {
        let session = leaf.session_id.clone();
        let cwd_from = leaf.cwd_from_session.clone();
        // 이미 살아 있는 세션이면 **다시 start하지 않는다**. `start`는 같은 id의
        // 옛 PTY를 kill하므로(레지스트리 §start), 다른 창에서 넘어온 탭이 여기서
        // 죽어버린다. 살아 있는 엔티티에 뷰만 새로 붙이면 grid·스크롤백이 그대로다.
        let started = match self.registry.read(cx).get(&session) {
            Some(terminal) => Ok(terminal),
            None => self.registry.update(cx, |reg, cx| {
                reg.start(&session, leaf.server_id, cwd_from.as_ref(), cx)
            }),
        };
        let Ok(terminal) = started else {
            return; // 실패한 pane은 뷰 없이 안내 문구를 보여준다
        };

        let this = cx.entity().downgrade();
        let source = session.clone();
        let sink: BroadcastSink = Rc::new(move |input, cx| {
            this.update(cx, |this, cx| this.fan_out(source.clone(), input, cx))
                .ok();
        });

        let local = leaf.server_id.is_none();
        let view = cx.new(|cx| {
            let mut view = TerminalView::from_terminal(terminal, cx);
            view.set_local(local);
            view.set_broadcast(Some(sink));
            view
        });
        self.views.insert(session, view);
    }

    /// 탭 구성이 바뀌었음을 창 셸에 알린다.
    ///
    /// **설정을 직접 쓰지 않는다.** 창이 여러 개면 각자 `terminal_layout`에
    /// 쓰는 순간 서로를 덮어쓰므로, 저장은 셸 → `WindowManager`(앱 스코프)가
    /// 창 레코드 단위로 모아서 한다. 현재 탭은 `tabs()`/`active_tab_id()`로
    /// 읽어 간다.
    pub fn persist_layout(&self, cx: &mut Context<Self>) {
        cx.emit(WorkspaceEvent::LayoutChanged);
    }

    /// 이 워크스페이스의 저장 페이로드 (`{tabs, activeIndex}`).
    pub fn layout_value(&self) -> serde_json::Value {
        save_layout(&self.tabs, self.active_tab.as_ref())
    }

    /// 활성 탭을 이 창에서 **떼어낸다** — 세션은 죽이지 않는다.
    ///
    /// 다른 창으로 옮기기 위한 경로라 `close_tab`과 결정적으로 다르다:
    /// `SessionRegistry::close`는 PTY를 kill하고 스크롤백까지 지우므로 절대
    /// 부르면 안 되고, 뷰만 버린다. `Entity<Terminal>`은 앱 스코프 레지스트리가
    /// 계속 들고 있으므로 새 창이 같은 session id로 다시 붙으면 PTY·grid가
    /// 그대로 살아 있다 (DESIGN-terminal.md §8).
    pub fn detach_active_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TerminalTab> {
        let index = self.active_index()?;
        let tab = self.tabs.remove(index);
        self.broadcast_tabs.remove(&tab.id);
        for leaf in leaves(&tab.root) {
            self.views.remove(&leaf.session_id);
            self.search.remove(&leaf.session_id);
        }
        if self.active_tab.as_ref() == Some(&tab.id) {
            self.active_tab = self.tabs.last().map(|t| t.id.clone());
            self.focused_pane = self
                .active_index()
                .and_then(|i| leaves(&self.tabs[i].root).first().map(|l| l.session_id.clone()));
        }
        // 창을 빈 채로 두지 않는다 — 탭 0개인 창은 조작할 수단이 없다.
        if self.tabs.is_empty() {
            self.push_local_tab(None);
        }
        self.sync_sessions(window, cx);
        self.focus_active_pane(window, cx);
        self.persist_layout(cx);
        cx.notify();
        Some(tab)
    }

    /// 앱 종료 경로 (§6): 라이브 cwd 스냅샷 → 스크롤백 flush → PTY kill.
    pub fn shutdown(&mut self, cx: &mut App) {
        self.registry.update(cx, |reg, cx| reg.shutdown_all(cx));
    }

    // ---- 액션 핸들러 -------------------------------------------------------

    fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(window, cx);
    }

    fn on_close_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        self.close_focused_pane(window, cx);
    }

    fn on_split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        self.split(SplitDirection::Row, window, cx);
    }

    fn on_split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.split(SplitDirection::Column, window, cx);
    }

    fn on_toggle_broadcast(&mut self, _: &ToggleBroadcast, _window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_broadcast(cx);
    }

    fn on_font_increase(&mut self, _: &FontIncrease, _window: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font(1.0, cx);
    }

    fn on_font_decrease(&mut self, _: &FontDecrease, _window: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font(-1.0, cx);
    }

    fn on_select_tab_1(&mut self, _: &SelectTab1, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(0), window, cx);
    }

    fn on_select_tab_2(&mut self, _: &SelectTab2, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(1), window, cx);
    }

    fn on_select_tab_3(&mut self, _: &SelectTab3, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(2), window, cx);
    }

    fn on_select_tab_4(&mut self, _: &SelectTab4, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(3), window, cx);
    }

    fn on_select_tab_5(&mut self, _: &SelectTab5, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(4), window, cx);
    }

    fn on_select_tab_6(&mut self, _: &SelectTab6, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(5), window, cx);
    }

    fn on_select_tab_7(&mut self, _: &SelectTab7, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(6), window, cx);
    }

    fn on_select_tab_8(&mut self, _: &SelectTab8, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(Some(7), window, cx);
    }

    /// ⌘9는 9번째가 아니라 **마지막** 탭이다 (macOS/Chrome/Zed 관례).
    fn on_select_tab_9(&mut self, _: &SelectTab9, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_at(None, window, cx);
    }

    fn on_focus_left(&mut self, _: &FocusLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(FocusDir::Left, window, cx);
    }

    fn on_focus_right(&mut self, _: &FocusRight, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(FocusDir::Right, window, cx);
    }

    fn on_focus_up(&mut self, _: &FocusUp, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(FocusDir::Up, window, cx);
    }

    fn on_focus_down(&mut self, _: &FocusDown, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_direction(FocusDir::Down, window, cx);
    }

    // ---- 디바이더 드래그 ----------------------------------------------------

    fn on_divider_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(grab) = self.divider.clone() else {
            return;
        };
        let delta = match grab.direction {
            SplitDirection::Row => f32::from(event.position.x) - f32::from(grab.start.x),
            SplitDirection::Column => f32::from(event.position.y) - f32::from(grab.start.y),
        };
        let sizes = resize_split(&grab.sizes, grab.index, delta, grab.extent);
        let Some(index) = self.active_index() else {
            return;
        };
        let root = std::mem::replace(
            &mut self.tabs[index].root,
            PaneNode::Leaf(TerminalLeaf::new(SessionId::default(), None, String::new())),
        );
        self.tabs[index].root = set_split_sizes(root, &grab.split_id, sizes);
        cx.notify();
    }

    fn on_divider_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.divider.take().is_some() {
            self.persist_layout(cx);
            cx.notify();
        }
    }

    // ---- 렌더 헬퍼 ---------------------------------------------------------

    fn pane_handlers(&self, cx: &mut Context<Self>) -> Rc<PaneHandlers> {
        let focus_this = cx.entity().downgrade();
        let rename_this = cx.entity().downgrade();
        let menu_this = cx.entity().downgrade();
        let divider_this = cx.entity().downgrade();
        let drop_pane_this = cx.entity().downgrade();
        let drop_tab_this = cx.entity().downgrade();
        let drag_over_this = cx.entity().downgrade();
        let labels: HashMap<SessionId, SharedString> = self
            .tabs
            .iter()
            .flat_map(|t| {
                leaves(&t.root)
                    .into_iter()
                    .map(|l| (l.session_id.clone(), SharedString::from(l.label.clone())))
                    .collect::<Vec<_>>()
            })
            .collect();

        Rc::new(PaneHandlers {
            focus: Box::new(move |session, window, cx| {
                focus_this
                    .update(cx, |this, cx| this.focus_pane(session, window, cx))
                    .ok();
            }),
            rename: Box::new(move |session, window, cx| {
                rename_this
                    .update(cx, |this, cx| this.start_pane_rename(session, window, cx))
                    .ok();
            }),
            context_menu: Box::new(move |session, at, window, cx| {
                menu_this
                    .update(cx, |this, cx| this.open_pane_menu(session, at, window, cx))
                    .ok();
            }),
            divider_down: Box::new(move |grab, _window, cx| {
                divider_this
                    .update(cx, |this, cx| {
                        this.divider = Some(grab);
                        cx.notify();
                    })
                    .ok();
            }),
            drop_pane: Box::new(move |drag, target, window, cx| {
                drop_pane_this
                    .update(cx, |this, cx| {
                        this.drop_on_pane(target, None, Some(drag), window, cx)
                    })
                    .ok();
            }),
            drop_tab: Box::new(move |drag, target, window, cx| {
                drop_tab_this
                    .update(cx, |this, cx| {
                        this.drop_on_pane(target, Some(drag), None, window, cx)
                    })
                    .ok();
            }),
            drag_over: Box::new(move |session, side, _window, cx| {
                drag_over_this
                    .update(cx, |this, cx| this.set_drag_over(session, side, cx))
                    .ok();
            }),
            drag_label: Box::new(move |session| {
                labels.get(session).cloned().unwrap_or_default()
            }),
        })
    }

    fn tab_bar_handlers(&self, cx: &mut Context<Self>) -> Rc<TabBarHandlers> {
        let select = cx.entity().downgrade();
        let close = cx.entity().downgrade();
        let rename = cx.entity().downgrade();
        let tab_menu = cx.entity().downgrade();
        let new_tab = cx.entity().downgrade();
        let drop_tab = cx.entity().downgrade();
        let drop_pane = cx.entity().downgrade();

        Rc::new(TabBarHandlers {
            select: Box::new(move |tab_id, window, cx| {
                select
                    .update(cx, |this, cx| {
                        this.active_tab = Some(tab_id);
                        this.focus_first_pane(window, cx);
                        this.persist_layout(cx);
                        cx.notify();
                    })
                    .ok();
            }),
            close: Box::new(move |tab_id, window, cx| {
                close
                    .update(cx, |this, cx| this.close_tab(tab_id, window, cx))
                    .ok();
            }),
            rename: Box::new(move |tab_id, window, cx| {
                rename
                    .update(cx, |this, cx| this.start_rename(tab_id, window, cx))
                    .ok();
            }),
            context_menu: Box::new(move |tab_id, at, window, cx| {
                tab_menu
                    .update(cx, |this, cx| this.open_tab_menu(tab_id, at, window, cx))
                    .ok();
            }),
            new_tab: Box::new(move |window, cx| {
                new_tab.update(cx, |this, cx| this.new_tab(window, cx)).ok();
            }),
            drop_tab: Box::new(move |drag, window, cx| {
                drop_tab
                    .update(cx, |this, cx| {
                        let boundary = this.tab_bar_boundary(window);
                        this.reorder_or_detach_on_tab_bar(boundary, Some(drag), None, window, cx);
                    })
                    .ok();
            }),
            drop_pane: Box::new(move |drag, window, cx| {
                drop_pane
                    .update(cx, |this, cx| {
                        let boundary = this.tab_bar_boundary(window);
                        this.reorder_or_detach_on_tab_bar(boundary, None, Some(drag), window, cx);
                    })
                    .ok();
            }),
        })
    }

    /// 마지막 렌더에서 각 pane이 차지한 영역 (좌→우, 위→아래 순).
    /// 분할이 화면까지 반영됐는지 검증하는 데 쓴다.
    pub fn pane_bounds(&self) -> Vec<(SessionId, Bounds<Pixels>)> {
        let geometry = self.geometry.borrow();
        let mut panes: Vec<(SessionId, Bounds<Pixels>)> =
            geometry.panes.iter().map(|(id, b)| (id.clone(), *b)).collect();
        panes.sort_by(|(_, a), (_, b)| {
            (a.origin.y, a.origin.x)
                .partial_cmp(&(b.origin.y, b.origin.x))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        panes
    }

    /// 탭바 위 드롭 지점 → 삽입 경계. 기하는 현재 탭 순서대로 정렬해서 쓴다
    /// (geometry 맵의 순서는 페인트 순서라 신뢰하지 않는다).
    fn tab_bar_boundary(&self, window: &mut Window) -> usize {
        let geometry = self.geometry.borrow();
        let ordered: Vec<(TabId, Bounds<Pixels>)> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                geometry
                    .tabs
                    .iter()
                    .find(|(id, _)| *id == tab.id)
                    .map(|(id, b)| (id.clone(), *b))
            })
            .collect();
        drop_boundary(&ordered, window.mouse_position().x)
    }
}

impl Focusable for TerminalWorkspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme: Theme = theme(cx).clone();
        let pane_handlers = self.pane_handlers(cx);
        let tab_handlers = self.tab_bar_handlers(cx);
        let active = self.active_index();
        let dragging = self.divider.is_some();

        let tab_bar = render_tab_bar(&TabBarCtx {
            tabs: &self.tabs,
            active: self.active_tab.as_ref(),
            broadcast: &self.broadcast_tabs,
            renaming: self.rename.as_ref().map(|(id, input)| (id, input)),
            geometry: self.geometry.clone(),
            handlers: tab_handlers,
            theme: &theme,
        });

        // 드래그가 끝났는데(취소·창 밖 드롭) 마지막 호버가 남아 있을 수 있다.
        // 활성 드래그가 없으면 미리보기는 무조건 그리지 않는다.
        let drag_over = self.drag_over.clone().filter(|_| cx.has_active_drag());

        let body = match active {
            Some(index) => {
                let tab = &self.tabs[index];
                render_pane_tree(
                    &tab.root,
                    &PaneTreeCtx {
                        tab_id: tab.id.clone(),
                        views: &self.views,
                        focused: self.focused_pane.as_ref(),
                        broadcast: self.broadcast_tabs.contains(&tab.id),
                        geometry: self.geometry.clone(),
                        handlers: pane_handlers,
                        theme: &theme,
                        leaf_count: leaves(&tab.root).len(),
                        drag_over,
                        renaming: self.pane_rename.as_ref().map(|(id, input)| (id, input)),
                        missing_notice: SharedString::from(tr(self.lang, TrKey::TermClosedNotice)),
                    },
                )
            }
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_8()
                .text_sm()
                .text_color(theme.text_muted)
                .child(tr(self.lang, TrKey::TermEmptyHint))
                .into_any_element(),
        };

        let mut root = div()
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.bg)
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_pane))
            .on_action(cx.listener(Self::on_split_right))
            .on_action(cx.listener(Self::on_split_down))
            .on_action(cx.listener(Self::on_toggle_broadcast))
            .on_action(cx.listener(Self::on_font_increase))
            .on_action(cx.listener(Self::on_font_decrease))
            .on_action(cx.listener(Self::on_focus_left))
            .on_action(cx.listener(Self::on_focus_right))
            .on_action(cx.listener(Self::on_focus_up))
            .on_action(cx.listener(Self::on_focus_down))
            .on_action(cx.listener(Self::on_select_tab_1))
            .on_action(cx.listener(Self::on_select_tab_2))
            .on_action(cx.listener(Self::on_select_tab_3))
            .on_action(cx.listener(Self::on_select_tab_4))
            .on_action(cx.listener(Self::on_select_tab_5))
            .on_action(cx.listener(Self::on_select_tab_6))
            .on_action(cx.listener(Self::on_select_tab_7))
            .on_action(cx.listener(Self::on_select_tab_8))
            .on_action(cx.listener(Self::on_select_tab_9))
            .child(tab_bar)
            .child(div().flex_grow().overflow_hidden().child(body));

        if dragging {
            // 드래그 중에만 전역 리스너를 단다 — 평소에는 마우스 이동마다
            // 워크스페이스가 깨어날 이유가 없다.
            root = root
                .on_mouse_move(cx.listener(Self::on_divider_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_divider_up));
        }

        // 컨텍스트 메뉴는 `deferred`로 자기 자신을 최상단에 올린다 —
        // 여기서는 트리에 매달아 두기만 하면 된다(순서는 z-order와 무관).
        if let Some(menu) = self.menu.clone() {
            root = root.child(menu);
        }
        if let Some(dialog) = self.confirm.clone() {
            root = root.child(ModalOverlay::new(dialog));
        }
        root
    }
}

// ---------------------------------------------------------------------------
// 순수 로직 (테스트 대상)
// ---------------------------------------------------------------------------

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 닫기 확인이 필요한가 — pane이 여럿이거나 서버 세션이 하나라도 있으면
/// 실수로 SSH 연결을 끊는 비용이 크다 (§6).
pub fn is_risky_close(root: &PaneNode) -> bool {
    let panes = leaves(root);
    panes.len() > 1 || panes.iter().any(|l| l.server_id.is_some())
}

/// 브로드캐스트 대상 = 같은 탭의 **나머지** leaf (입력 소스는 제외 — 자기
/// 자신에게 다시 쓰면 글자가 두 번 들어간다).
pub fn broadcast_targets(root: &PaneNode, source: &SessionId) -> Vec<SessionId> {
    leaves(root)
        .into_iter()
        .map(|l| l.session_id.clone())
        .filter(|id| id != source)
        .collect()
}

/// 분할 직후 1회만 유효한 cwd 상속원을 지운다.
fn clear_cwd_sources(node: &mut PaneNode) {
    match node {
        PaneNode::Leaf(leaf) => leaf.cwd_from_session = None,
        PaneNode::Split(split) => split.children.iter_mut().for_each(clear_cwd_sources),
    }
}

/// 탭 구성 → `Settings.terminal_layout` 값.
pub fn save_layout(tabs: &[TerminalTab], active: Option<&TabId>) -> serde_json::Value {
    let active_index = active
        .and_then(|id| tabs.iter().position(|t| t.id == *id))
        .unwrap_or(0);
    serde_json::to_value(SavedLayout {
        tabs: tabs.to_vec(),
        active_index,
    })
    .unwrap_or(serde_json::Value::Null)
}

/// `Settings.terminal_layout` → 탭 구성. transient id(탭/split/구버전 세션)는
/// 새로 발급한다 (`revive_ids`).
pub fn load_layout(value: &serde_json::Value) -> Option<(Vec<TerminalTab>, Option<TabId>)> {
    let saved: SavedLayout = serde_json::from_value(value.clone()).ok()?;
    let mut tabs = saved.tabs;
    if tabs.is_empty() {
        return None;
    }
    for tab in &mut tabs {
        tab.id = TabId::new(new_id());
        revive_ids(
            &mut tab.root,
            &mut || SessionId::new(new_id()),
            &mut || SplitId::new(new_id()),
        );
    }
    let active = tabs
        .get(saved.active_index)
        .or_else(|| tabs.first())
        .map(|t| t.id.clone());
    Some((tabs, active))
}

/// 탭 배열에 탭을 경계 삽입 — 외부(다른 창)에서 넘어온 탭을 받을 때 쓴다.
pub fn insert_tab(tabs: Vec<TerminalTab>, tab: TerminalTab, at: Option<usize>) -> Vec<TerminalTab> {
    insert_at_index(tabs, tab, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(session: &str, server: Option<i64>) -> PaneNode {
        PaneNode::Leaf(TerminalLeaf::new(SessionId::new(session), server, "label"))
    }

    fn split(children: Vec<PaneNode>) -> PaneNode {
        PaneNode::Split(sshub_splits::TerminalSplit {
            id: SplitId::new("sp"),
            direction: SplitDirection::Row,
            sizes: vec![50.0, 50.0],
            children,
        })
    }

    #[test]
    fn a_lone_local_pane_closes_without_asking() {
        assert!(!is_risky_close(&leaf("a", None)));
    }

    #[test]
    fn closing_is_risky_with_multiple_panes_or_a_server_session() {
        assert!(is_risky_close(&split(vec![leaf("a", None), leaf("b", None)])));
        assert!(is_risky_close(&leaf("a", Some(3))), "SSH 세션은 단독이어도 확인");
        assert!(is_risky_close(&split(vec![leaf("a", None), leaf("b", Some(1))])));
    }

    #[test]
    fn broadcast_targets_exclude_the_source_pane() {
        let root = split(vec![leaf("a", None), leaf("b", None), leaf("c", None)]);
        assert_eq!(
            broadcast_targets(&root, &SessionId::new("b")),
            vec![SessionId::new("a"), SessionId::new("c")]
        );
        // 단독 pane이면 복제할 곳이 없다.
        assert!(broadcast_targets(&leaf("a", None), &SessionId::new("a")).is_empty());
    }

    #[test]
    fn layout_round_trips_through_the_settings_value() {
        let tabs = vec![
            TerminalTab {
                id: TabId::new("t1"),
                root: split(vec![leaf("a", None), leaf("b", Some(7))]),
                name: Some("build".into()),
            },
            TerminalTab {
                id: TabId::new("t2"),
                root: leaf("c", None),
                name: None,
            },
        ];
        let value = save_layout(&tabs, Some(&TabId::new("t2")));

        // 실제 저장 경로와 같은 모양인지 — Settings를 거쳐 왕복시킨다.
        let mut settings = sshub_core::settings::Settings::default();
        settings.terminal_layout = Some(value);
        let json = serde_json::to_string(&settings).unwrap();
        let restored: sshub_core::settings::Settings = serde_json::from_str(&json).unwrap();

        let (loaded, active) = load_layout(restored.terminal_layout.as_ref().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name.as_deref(), Some("build"));
        assert_eq!(tab_title(&loaded[1]), "label");
        // 세션 id는 보존, 탭/split id는 새로 발급된다.
        assert_eq!(
            leaves(&loaded[0].root)
                .iter()
                .map(|l| l.session_id.as_str().to_string())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(leaves(&loaded[0].root)[1].server_id, Some(7));
        assert_ne!(loaded[0].id, TabId::new("t1"), "탭 id는 transient");
        assert_eq!(active.as_ref(), Some(&loaded[1].id), "activeIndex가 유지된다");
    }

    #[test]
    fn a_tab_moved_to_another_window_keeps_its_session_ids() {
        // 창 간 탭 이동의 핵심 불변식. 이동 시드는 `save_layout`과 같은 모양이고
        // `load_layout`이 **세션 id를 보존**하므로, 새 창의 `sync_sessions`가
        // 같은 id로 레지스트리를 조회해 살아 있는 PTY를 그대로 재사용한다.
        // (세션 id가 재발급되면 새 셸이 뜨면서 grid/스크롤백이 날아간다.)
        let moved = TerminalTab {
            id: TabId::new("t1"),
            root: split(vec![leaf("a", None), leaf("b", Some(7))]),
            name: Some("작업".into()),
        };
        let seed = serde_json::json!({ "tabs": [moved], "activeIndex": 0 });

        let (loaded, active) = load_layout(&seed).expect("시드는 유효한 레이아웃");
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            leaves(&loaded[0].root)
                .iter()
                .map(|l| l.session_id.as_str().to_string())
                .collect::<Vec<_>>(),
            vec!["a", "b"],
            "세션 id가 보존돼야 PTY를 재사용한다"
        );
        assert_eq!(leaves(&loaded[0].root)[1].server_id, Some(7), "서버 세션도 그대로");
        assert_eq!(loaded[0].name.as_deref(), Some("작업"), "탭 이름을 들고 간다");
        assert_ne!(loaded[0].id, TabId::new("t1"), "탭 id만 새 창에서 재발급");
        assert_eq!(active.as_ref(), Some(&loaded[0].id));
    }

    #[test]
    fn saved_layout_uses_the_typescript_field_names() {
        let tabs = vec![TerminalTab {
            id: TabId::new("t1"),
            root: leaf("a", None),
            name: None,
        }];
        let value = save_layout(&tabs, Some(&TabId::new("t1")));
        let object = value.as_object().unwrap();
        assert!(object.contains_key("tabs"), "tabs 키");
        assert!(object.contains_key("activeIndex"), "camelCase activeIndex");
        let leaf = &object["tabs"][0]["root"];
        assert_eq!(leaf["type"], "leaf");
        assert_eq!(leaf["sessionId"], "a");
        assert!(leaf.get("cwdFromSession").is_none(), "transient 필드는 저장 안 함");
    }

    #[test]
    fn a_broken_or_empty_layout_is_ignored() {
        assert!(load_layout(&serde_json::json!({"tabs": [], "activeIndex": 0})).is_none());
        assert!(load_layout(&serde_json::json!({"nope": 1})).is_none());
    }

    #[test]
    fn out_of_range_active_index_falls_back_to_the_first_tab() {
        let value = serde_json::json!({
            "tabs": [{"root": {"type": "leaf", "sessionId": "a", "serverId": null, "label": "x"}}],
            "activeIndex": 9
        });
        let (tabs, active) = load_layout(&value).unwrap();
        assert_eq!(active, Some(tabs[0].id.clone()));
    }

    #[test]
    fn cwd_sources_are_consumed_once() {
        let mut root = PaneNode::Split(sshub_splits::TerminalSplit {
            id: SplitId::new("sp"),
            direction: SplitDirection::Row,
            sizes: vec![50.0, 50.0],
            children: vec![leaf("a", None), {
                let mut l = TerminalLeaf::new(SessionId::new("b"), None, "l");
                l.cwd_from_session = Some(SessionId::new("a"));
                PaneNode::Leaf(l)
            }],
        });
        clear_cwd_sources(&mut root);
        assert!(leaves(&root).iter().all(|l| l.cwd_from_session.is_none()));
    }
}
