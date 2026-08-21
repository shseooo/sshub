//! 관리 화면 5종 쇼케이스. `cargo run -p sshub --example views_demo`
//!
//! 사이드바 + Dashboard/Servers/ServerEdit/Keys/Settings를 실제 `AppState`에
//! 물려 띄운다. 워크스페이스가 아직 없으므로 여기서 라우팅(`ViewEvent`)을
//! 최소한으로 흉내 낸다 — 레이아웃 눈으로 확인하는 용도.
//!
//! 주의: 실제 `sshub.json`/`ssh_keys/`를 읽고 쓴다.
use gpui::{
    actions, div, point, prelude::*, px, size, App, Application, Bounds, Context, Entity,
    KeyBinding, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use sshub::state;
use sshub::theme::{self, theme};
use sshub::ui;
use sshub::views::{
    dashboard::DashboardView, key_manager::KeyManagerView, server_edit::ServerEditView,
    server_list::ServerListView, settings_page::SettingsView, sidebar::Sidebar, Page, ViewEvent,
};

actions!(views_demo, [Quit]);

/// 현재 페이지의 뷰 — 페이지를 옮길 때마다 새로 만든다(워크스페이스와 동일한 정책).
enum ActiveView {
    Dashboard(Entity<DashboardView>),
    Servers(Entity<ServerListView>),
    ServerEdit(Entity<ServerEditView>),
    Keys(Entity<KeyManagerView>),
    Settings(Entity<SettingsView>),
}

struct Demo {
    sidebar: Entity<Sidebar>,
    active: ActiveView,
    page: Page,
    _subscriptions: Vec<gpui::Subscription>,
}

impl Demo {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = Page::Dashboard;
        let sidebar = cx.new(|cx| Sidebar::new(page, cx));
        let subscription = cx.subscribe(&sidebar, |this: &mut Demo, _, event: &ViewEvent, cx| {
            this.handle_event(*event, cx);
        });
        let active = ActiveView::Dashboard(cx.new(DashboardView::new));
        let mut demo = Self {
            sidebar,
            active,
            page,
            _subscriptions: vec![subscription],
        };
        // 창이 뜨자마자 대시보드 구독까지 걸어 둔다.
        demo.navigate(page, window, cx);
        demo
    }

    fn handle_event(&mut self, event: ViewEvent, cx: &mut Context<Self>) {
        match event {
            ViewEvent::Navigate(page) => {
                // navigate에는 Window가 필요하므로 다음 프레임에 처리한다.
                self.page = page;
                cx.notify();
            }
            // 터미널 호스트가 없는 데모에서는 접속 요청을 무시한다.
            ViewEvent::Connect(_) => {}
        }
    }

    fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.set_active(page, cx));

        let mut subscriptions = vec![cx.subscribe(
            &self.sidebar,
            |this: &mut Demo, _, event: &ViewEvent, cx| this.handle_event(*event, cx),
        )];

        self.active = match page {
            Page::Dashboard | Page::Terminal => {
                let view = cx.new(DashboardView::new);
                subscriptions.push(cx.subscribe(
                    &view,
                    |this: &mut Demo, _, event: &ViewEvent, cx| this.handle_event(*event, cx),
                ));
                ActiveView::Dashboard(view)
            }
            Page::Servers => {
                let view = cx.new(|cx| ServerListView::new(window, cx));
                subscriptions.push(cx.subscribe(
                    &view,
                    |this: &mut Demo, _, event: &ViewEvent, cx| this.handle_event(*event, cx),
                ));
                ActiveView::Servers(view)
            }
            Page::ServerEdit { id } => {
                let view = cx.new(|cx| ServerEditView::new(id, window, cx));
                subscriptions.push(cx.subscribe(
                    &view,
                    |this: &mut Demo, _, event: &ViewEvent, cx| this.handle_event(*event, cx),
                ));
                ActiveView::ServerEdit(view)
            }
            Page::Keys => ActiveView::Keys(cx.new(KeyManagerView::new)),
            Page::Settings => {
                let view = cx.new(|cx| SettingsView::new(window, cx));
                subscriptions.push(cx.subscribe(
                    &view,
                    |this: &mut Demo, _, event: &ViewEvent, cx| this.handle_event(*event, cx),
                ));
                ActiveView::Settings(view)
            }
        };
        self._subscriptions = subscriptions;
        cx.notify();
    }
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 내비 이벤트로 페이지가 바뀌었는데 뷰가 아직 그대로면 여기서 만든다.
        let stale = match (&self.active, self.page) {
            (ActiveView::Dashboard(_), Page::Dashboard | Page::Terminal) => false,
            (ActiveView::Servers(_), Page::Servers) => false,
            (ActiveView::ServerEdit(_), Page::ServerEdit { .. }) => false,
            (ActiveView::Keys(_), Page::Keys) => false,
            (ActiveView::Settings(_), Page::Settings) => false,
            _ => true,
        };
        if stale {
            let page = self.page;
            self.navigate(page, window, cx);
        }

        let t = theme(cx).clone();
        let content = match &self.active {
            ActiveView::Dashboard(view) => view.clone().into_any_element(),
            ActiveView::Servers(view) => view.clone().into_any_element(),
            ActiveView::ServerEdit(view) => view.clone().into_any_element(),
            ActiveView::Keys(view) => view.clone().into_any_element(),
            ActiveView::Settings(view) => view.clone().into_any_element(),
        };

        div()
            .key_context("Workspace")
            .flex()
            .flex_row()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .child(self.sidebar.clone())
            .child(div().flex_1().min_w(px(0.)).child(content))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        theme::init(cx);
        state::init(cx);
        ui::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("sshub — views demo".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(12.))),
                }),
                window_min_size: Some(size(px(760.), px(480.))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Demo::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
