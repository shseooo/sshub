//! 사이드바 — 4개 내비 항목 + 접기 토글 (Electron `src/components/Sidebar.tsx`).
//!
//! 접힘 상태는 `settings.sidebar_collapsed`로 영속화한다.
use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Window};

use crate::i18n::{tr, Lang, TrKey};
use crate::state::{app_state, AppState};
use crate::theme::theme;
use crate::ui::icon::{icon, Icon};
use crate::views::{current_lang, Page, ViewEvent};

pub const SIDEBAR_WIDTH: f32 = 208.;
pub const SIDEBAR_WIDTH_COLLAPSED: f32 = 56.;
const NAV_ITEMS: [(Page, Icon, TrKey); 4] = [
    (Page::Servers, Icon::Server, TrKey::NavServers),
    (Page::Terminal, Icon::Terminal, TrKey::NavTerminal),
    (Page::Keys, Icon::Key, TrKey::NavKeys),
    (Page::Settings, Icon::Settings, TrKey::NavSettings),
];

pub struct Sidebar {
    active: Page,
    state: Entity<AppState>,
    _subscription: gpui::Subscription,
}

impl EventEmitter<ViewEvent> for Sidebar {}

impl Sidebar {
    pub fn new(active: Page, cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        // 언어·접힘 설정이 다른 화면에서 바뀌어도 사이드바가 따라가야 한다.
        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
        Self { active, state, _subscription: subscription }
    }

    pub fn set_active(&mut self, page: Page, cx: &mut Context<Self>) {
        self.active = page;
        cx.notify();
    }

    pub fn is_collapsed(&self, cx: &Context<Self>) -> bool {
        self.state.read(cx).settings.sidebar_collapsed
    }

    fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        let collapsed = self.is_collapsed(cx);
        self.state.update(cx, |state, cx| {
            state.update_settings(|s| s.sidebar_collapsed = !collapsed, cx);
        });
        cx.notify();
    }

    fn nav_item(
        &self,
        page: Page,
        glyph: Icon,
        key: TrKey,
        lang: Lang,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        // 서버 편집 화면에서도 "서버" 항목이 활성으로 보여야 한다.
        let active = matches!(
            (self.active, page),
            (Page::ServerEdit { .. }, Page::Servers)
        ) || self.active == page;
        let label = tr(lang, key);
        let element_id = gpui::SharedString::new_static(match page {
            Page::Servers | Page::ServerEdit { .. } => "nav-servers",
            Page::Terminal => "nav-terminal",
            Page::Keys => "nav-keys",
            Page::Settings => "nav-settings",
        });

        div()
            .id(element_id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .w_full()
            .h(px(32.))
            .px(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(active, |el| el.bg(t.accent_wash))
            .when(!active, |el| el.hover(move |s| s.bg(t.hover)))
            .when(collapsed, |el| el.justify_center())
            .child(icon(glyph).color(if active { t.accent } else { t.text_muted }))
            .when(!collapsed, |el| {
                el.child(
                    div()
                        .flex_1()
                        .text_size(px(13.))
                        .text_color(if active { t.text } else { t.text_muted })
                        .child(label),
                )
            })
            .on_click(cx.listener(move |_, _, _window, cx| {
                cx.emit(ViewEvent::Navigate(page));
            }))
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let collapsed = self.is_collapsed(cx);
        let width = if collapsed { SIDEBAR_WIDTH_COLLAPSED } else { SIDEBAR_WIDTH };

        let items: Vec<_> = NAV_ITEMS
            .iter()
            .map(|(page, glyph, key)| self.nav_item(*page, *glyph, *key, lang, collapsed, cx))
            .collect();

        let toggle_label = if collapsed {
            tr(lang, TrKey::SidebarExpand)
        } else {
            tr(lang, TrKey::SidebarCollapse)
        };

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .pb(px(8.))
            .px(px(8.))
            .gap(px(2.))
            .bg(t.surface)
            .border_r_1()
            .border_color(t.border_subtle)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(28.))
                    .px(px(8.))
                    .when(!collapsed, |el| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_disabled)
                                .child("sshub"),
                        )
                    }),
            )
            .children(items)
            // 접기 토글은 바닥에 붙인다.
            .child(div().flex_1())
            .child(
                div()
                    .id("sidebar-toggle")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .w_full()
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(t.hover))
                    .when(collapsed, |el| el.justify_center())
                    .child(icon(if collapsed {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    }))
                    .when(!collapsed, |el| {
                        el.child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child(toggle_label),
                        )
                    })
                    .on_click(cx.listener(|this, _, _window, cx| this.toggle_collapsed(cx))),
            )
    }
}
