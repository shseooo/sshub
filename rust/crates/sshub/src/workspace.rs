//! 창별 루트 뷰 (DESIGN-ui.md §3, DESIGN-terminal.md §8).
//!
//! 창 하나가 소유하는 것: 현재 페이지, 사이드바, 모달 레이어, 토스트 스택,
//! 그리고 **항상 살아 있는** `TerminalWorkspace` 하나.
//!
//! 터미널을 `Page::Terminal`에서만 그리지만 엔티티는 `new`에서 한 번 만들고
//! 절대 드랍하지 않는다 — gpui는 "렌더되지 않음 ≠ 드랍"이므로 페이지를 옮겨도
//! PTY가 살아 있다. 페이지 뷰(대시보드/서버/키/설정)는 반대로 내비 때마다
//! 새로 만든다(상태를 들고 있을 이유가 없다).

use gpui::{
    actions, div, point, prelude::*, px, size, AnyView, App, Bounds, Context, Entity, FocusHandle,
    Focusable, MouseButton, MouseDownEvent, Subscription, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowHandle, WindowOptions,
};
use sshub_core::window_state::WindowBounds as SavedBounds;
use sshub_splits::SessionId;

use crate::i18n::{tr, TrKey};
use crate::keymap::NewWindow;
use crate::session::pane_label;
use crate::session_registry;
use crate::state::app_state;
use crate::terminal_workspace::{TerminalWorkspace, WorkspaceEvent};
use crate::theme::theme;
use crate::ui::toast::render_toast_stack;
use crate::ui::{ModalOverlay, Toast};
use crate::views::{
    current_lang, key_manager::KeyManagerView, server_edit::ServerEditView,
    server_list::ServerListView, settings_page::SettingsView,
    sidebar::Sidebar, Page, ViewEvent,
};
use crate::window_manager::{self, WindowId};
use crate::window_session::DEFAULT_BOUNDS;

actions!(sshub_workspace, [MoveTabToNewWindow]);

/// 신호등 버튼을 덮지 않는 최소 높이.
const TITLEBAR_HEIGHT: f32 = 36.;
/// 새 창을 기존 창 위에 정확히 겹쳐 놓지 않기 위한 캐스케이드 오프셋.
const CASCADE_OFFSET: i32 = 28;

/// 열려 있는 모달. `prev_focus`는 닫을 때 포커스를 되돌리기 위한 것 —
/// 모달이 닫혔는데 포커스가 허공에 남으면 키 입력이 전부 죽는다.
pub struct ActiveModal {
    pub view: AnyView,
    pub prev_focus: Option<FocusHandle>,
}

/// 현재 페이지의 뷰. `Page::Terminal`일 때는 `None` — 터미널은 별도 필드가
/// 항상 들고 있으므로 여기 낄 필요가 없다.
enum ActiveView {
    Servers(Entity<ServerListView>),
    ServerEdit(Entity<ServerEditView>),
    Keys(Entity<KeyManagerView>),
    Settings(Entity<SettingsView>),
}

pub struct Workspace {
    window_id: WindowId,
    page: Page,
    sidebar: Entity<Sidebar>,
    active: Option<ActiveView>,
    /// 창이 사는 동안 절대 드랍하지 않는다.
    terminal: Entity<TerminalWorkspace>,
    /// `Connect` 이벤트는 `&mut Window`가 필요해 다음 렌더로 미룬다.
    pending_connect: Option<i64>,
    modal: Option<ActiveModal>,
    toasts: Vec<Toast>,
    next_toast_id: u64,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// 창 하나를 연다. 부트스트랩(복원)과 `NewWindow`/탭 이동이 모두 이 경로를 쓴다.
pub fn open(
    seed: Option<serde_json::Value>,
    bounds: SavedBounds,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Workspace>> {
    let window_id =
        window_manager::manager(cx).update(cx, |manager, _| manager.register(bounds.clone()));
    let options = window_options(&bounds, cx);
    let handle = cx.open_window(options, |window, cx| {
        cx.new(|cx| Workspace::new(window_id, seed, window, cx))
    })?;
    handle
        .update(cx, |workspace, window, cx| {
            let handle = workspace.terminal.read(cx).focus_handle(cx);
            window.focus(&handle);
        })
        .ok();
    Ok(handle)
}

/// 저장된 지오메트리를 이어 여는 다음 창의 위치 (캐스케이드).
pub fn cascaded(from: Option<&SavedBounds>) -> SavedBounds {
    let base = from.cloned().unwrap_or(DEFAULT_BOUNDS);
    SavedBounds {
        x: base.x.map(|x| x + CASCADE_OFFSET),
        y: base.y.map(|y| y + CASCADE_OFFSET),
        ..base
    }
}

fn window_options(bounds: &SavedBounds, cx: &App) -> WindowOptions {
    let dims = size(px(bounds.width as f32), px(bounds.height as f32));
    // x/y는 둘 다 있을 때만 신뢰한다(sanitize_bounds가 보장) — 없으면 화면 중앙.
    let placed = match (bounds.x, bounds.y) {
        (Some(x), Some(y)) => Bounds::new(point(px(x as f32), px(y as f32)), dims),
        _ => Bounds::centered(None, dims, cx),
    };
    let translucency = app_state(cx).read(cx).settings.appearance.translucency;
    WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(placed)),
        titlebar: Some(TitlebarOptions {
            title: Some("sshub".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(12.))),
        }),
        // 반투명은 루트 bg 알파에 이미 구워져 있다 — 여기선 뒤를 비출지만 정한다.
        window_background: if translucency > 0 {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        },
        window_min_size: Some(size(px(760.), px(480.))),
        ..Default::default()
    }
}

impl Workspace {
    pub fn new(
        window_id: WindowId,
        seed: Option<serde_json::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = app_state(cx);
        let page = Page::from_start_page(&state.read(cx).settings.start_page);
        let sidebar = cx.new(|cx| Sidebar::new(page, cx));
        let terminal = cx.new(|cx| TerminalWorkspace::new_seeded(seed, window, cx));

        let subscriptions = vec![
            cx.subscribe(&sidebar, |this: &mut Self, _, event: &ViewEvent, cx| {
                this.handle_view_event(*event, cx);
            }),
            // 레이아웃 저장의 유일한 경로 — 워크스페이스는 설정을 직접 쓰지 않는다.
            cx.subscribe(
                &terminal,
                |this: &mut Self, _, event: &WorkspaceEvent, cx| match event {
                    WorkspaceEvent::LayoutChanged => this.sync_layout(cx),
                },
            ),
            cx.observe_window_bounds(window, |this, window, cx| {
                this.sync_bounds(window, cx);
            }),
            // 창이 닫히면(=이 뷰가 드랍되면) 레코드를 지우고 고아 세션을 정리한다.
            // DESIGN-terminal.md §8 "창 닫기 = 그 창의 탭 닫기".
            cx.on_release(|this: &mut Self, cx| {
                let window_id = this.window_id;
                let Some(manager) = window_manager::try_manager(cx) else {
                    return;
                };
                if manager.read(cx).is_quitting() {
                    return;
                }
                manager.update(cx, |manager, cx| {
                    manager.unregister(window_id);
                    manager.persist_now(cx);
                });
                let live = manager.read(cx).live_session_ids();
                close_orphaned_sessions(&live, cx);
            }),
        ];

        let mut workspace = Workspace {
            window_id,
            page,
            sidebar,
            active: None,
            terminal,
            pending_connect: None,
            modal: None,
            toasts: Vec::new(),
            next_toast_id: 0,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        // 시드로 받은 탭을 매니저 레코드에 즉시 반영해 둔다 — 첫 저장이
        // 일어나기 전에 종료되더라도 창이 빈 채로 기록되지 않도록.
        workspace.sync_layout(cx);
        workspace.sync_bounds(window, cx);
        workspace.navigate(page, window, cx);
        workspace
    }

    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    pub fn terminal(&self) -> &Entity<TerminalWorkspace> {
        &self.terminal
    }

    // ---- 매니저 동기화 -----------------------------------------------------

    fn sync_layout(&mut self, cx: &mut Context<Self>) {
        let terminal = self.terminal.read(cx);
        let tabs = terminal.tabs().to_vec();
        let active = terminal
            .active_tab_id()
            .and_then(|id| tabs.iter().position(|tab| tab.id == *id))
            .unwrap_or(0);
        let window_id = self.window_id;
        // 매니저가 없는 환경(예제/테스트)에서는 조용히 넘어간다.
        if let Some(manager) = window_manager::try_manager(cx) {
            manager.update(cx, |manager, cx| {
                manager.update_layout(window_id, tabs, active);
                manager.persist(cx);
            });
        }
    }

    fn sync_bounds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = saved_bounds(window);
        let window_id = self.window_id;
        if let Some(manager) = window_manager::try_manager(cx) {
            manager.update(cx, |manager, cx| {
                manager.update_bounds(window_id, bounds);
                manager.persist(cx);
            });
        }
    }

    // ---- 라우팅 ------------------------------------------------------------

    fn handle_view_event(&mut self, event: ViewEvent, cx: &mut Context<Self>) {
        match event {
            ViewEvent::Navigate(page) => {
                // 실제 뷰 생성은 `&mut Window`가 있는 렌더에서 한다.
                self.page = page;
                cx.notify();
            }
            ViewEvent::Connect(server_id) => {
                self.page = Page::Terminal;
                self.pending_connect = Some(server_id);
                cx.notify();
            }
        }
    }

    fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.set_active(page, cx));

        self.active = match page {
            // 터미널은 상시 엔티티가 그린다 — 페이지 뷰가 없다.
            Page::Terminal => None,
            Page::Servers => {
                let view = cx.new(|cx| ServerListView::new(window, cx));
                self.watch(&view, cx);
                Some(ActiveView::Servers(view))
            }
            Page::ServerEdit { id } => {
                let view = cx.new(|cx| ServerEditView::new(id, window, cx));
                self.watch(&view, cx);
                Some(ActiveView::ServerEdit(view))
            }
            Page::Keys => Some(ActiveView::Keys(cx.new(KeyManagerView::new))),
            Page::Settings => {
                let view = cx.new(|cx| SettingsView::new(window, cx));
                self.watch(&view, cx);
                Some(ActiveView::Settings(view))
            }
        };
        cx.notify();
    }

    /// 페이지 뷰의 `ViewEvent`를 받아 라우팅한다. 구독은 뷰와 수명을 같이한다.
    fn watch<V>(&mut self, view: &Entity<V>, cx: &mut Context<Self>)
    where
        V: gpui::EventEmitter<ViewEvent> + 'static,
    {
        let subscription = cx.subscribe(view, |this: &mut Self, _, event: &ViewEvent, cx| {
            this.handle_view_event(*event, cx);
        });
        self._subscriptions.push(subscription);
    }

    /// 현재 페이지와 살아 있는 뷰가 어긋났는지 (내비 이벤트는 Window가 없어
    /// 페이지만 바꿔 두고, 실제 생성은 렌더에서 따라잡는다).
    fn view_is_stale(&self) -> bool {
        !matches!(
            (&self.active, self.page),
            (Some(ActiveView::Servers(_)), Page::Servers)
                | (Some(ActiveView::ServerEdit(_)), Page::ServerEdit { .. })
                | (Some(ActiveView::Keys(_)), Page::Keys)
                | (Some(ActiveView::Settings(_)), Page::Settings)
                | (None, Page::Terminal)
        )
    }

    // ---- 모달 / 토스트 -----------------------------------------------------

    /// 모달을 띄운다. 이전 포커스를 기억해 두었다가 닫을 때 되돌린다.
    pub fn open_modal(&mut self, view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = Some(ActiveModal {
            view: view.into(),
            prev_focus: window.focused(cx),
        });
        cx.notify();
    }

    pub fn dismiss_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = self.modal.take() {
            if let Some(handle) = modal.prev_focus {
                window.focus(&handle);
            }
            cx.notify();
        }
    }

    pub fn has_modal(&self) -> bool {
        self.modal.is_some()
    }

    /// 토스트를 띄우고 자기 수명이 지나면 스스로 사라지게 한다.
    pub fn push_toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        let duration = std::time::Duration::from_millis(toast.kind.default_duration_ms());
        let toast = Toast { id, ..toast };
        self.toasts.push(toast);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|t| t.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // ---- 액션 --------------------------------------------------------------

    fn on_new_window(&mut self, _: &NewWindow, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = cascaded(Some(&saved_bounds(window)));
        cx.defer(move |cx| {
            let _ = open(None, bounds, cx);
        });
    }

    /// 활성 탭을 떼어 새 창으로 옮긴다.
    ///
    /// PTY가 살아남는 이유: `Entity<Terminal>`은 앱 스코프
    /// `SessionRegistry`가 소유하고, `detach_active_tab`은 뷰만 버릴 뿐
    /// `registry.close`(=kill)를 부르지 않는다. 새 창은 같은 session id로
    /// 시드되고 `start_leaf`가 살아 있는 세션을 재사용한다.
    fn on_move_tab_to_new_window(
        &mut self,
        _: &MoveTabToNewWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .terminal
            .update(cx, |terminal, cx| terminal.detach_active_tab(window, cx))
        else {
            return;
        };
        let seed = serde_json::json!({ "tabs": [tab], "activeIndex": 0 });
        let bounds = cascaded(Some(&saved_bounds(window)));
        cx.defer(move |cx| {
            let _ = open(Some(seed), bounds, cx);
        });
    }
}

/// 어떤 창에도 속하지 않게 된 세션의 PTY를 정리한다.
///
/// 탭을 다른 창으로 **옮기는** 경로에서는 이 함수가 도는 시점에 이미 목적
/// 창의 레코드에 그 세션이 들어 있으므로 살아남는다 — 창을 닫아 정말로
/// 버려진 세션만 죽는다.
fn close_orphaned_sessions(live: &[String], cx: &mut App) {
    let Some(registry) = session_registry::try_registry(cx) else {
        return;
    };
    let orphans: Vec<String> = registry
        .read(cx)
        .live_ids()
        .into_iter()
        .filter(|id| !live.contains(id))
        .collect();
    registry.update(cx, |registry, cx| {
        for id in orphans {
            registry.close(&SessionId::new(id), cx);
        }
    });
}

/// gpui 창 지오메트리 → 저장 포맷.
pub fn saved_bounds(window: &Window) -> SavedBounds {
    let bounds = window.bounds();
    SavedBounds {
        width: f32::from(bounds.size.width).round().max(1.) as u32,
        height: f32::from(bounds.size.height).round().max(1.) as u32,
        x: Some(f32::from(bounds.origin.x).round() as i32),
        y: Some(f32::from(bounds.origin.y).round() as i32),
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view_is_stale() {
            let page = self.page;
            self.navigate(page, window, cx);
        }
        // 접속 요청은 Window가 필요해 여기까지 미뤄 뒀다.
        if let Some(server_id) = self.pending_connect.take() {
            let lang = current_lang(cx);
            let label = {
                let state = app_state(cx);
                let state = state.read(cx);
                let server = state.servers.iter().find(|s| s.id == server_id);
                pane_label(server, "", tr(lang, TrKey::TermNewConnection))
            };
            self.terminal.update(cx, |terminal, cx| {
                terminal.open_server_tab(server_id, label, window, cx);
            });
        }

        let t = theme(cx).clone();

        let content = match (&self.active, self.page) {
            (_, Page::Terminal) => self.terminal.clone().into_any_element(),
            (Some(ActiveView::Servers(view)), _) => view.clone().into_any_element(),
            (Some(ActiveView::ServerEdit(view)), _) => view.clone().into_any_element(),
            (Some(ActiveView::Keys(view)), _) => view.clone().into_any_element(),
            (Some(ActiveView::Settings(view)), _) => view.clone().into_any_element(),
            (None, _) => div().into_any_element(),
        };

        // 36px 드래그 스트립. `appears_transparent`라 네이티브 타이틀바가
        // 보이지 않으므로 창 이동을 직접 얹는다.
        let titlebar = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLEBAR_HEIGHT))
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, window, _cx| {
                window.start_window_move();
            });

        div()
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .on_action(cx.listener(Self::on_new_window))
            .on_action(cx.listener(Self::on_move_tab_to_new_window))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .size_full()
                    .min_h(px(0.))
                    .child(self.sidebar.clone())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            // 사이드바가 신호등을 피해 자체 상단 패딩을 갖는다.
                            .child(content),
                    ),
            )
            .child(titlebar)
            .child(render_toast_stack(&self.toasts, window, cx))
            // 모달은 항상 마지막 — 아래 레이어를 완전히 가려야 한다.
            .children(
                self.modal
                    .as_ref()
                    .map(|modal| ModalOverlay::new(modal.view.clone())),
            )
    }
}
