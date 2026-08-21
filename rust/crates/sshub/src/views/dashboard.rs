//! 대시보드 (Electron `src/pages/Dashboard.tsx`).
//!
//! 읽기 전용 화면 — 뮤테이션이 없고, 카드 클릭은 접속 요청만 올려보낸다.
use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Window};
use sshub_core::model::Server;

use crate::i18n::{tr, Lang, TrKey};
use crate::state::{app_state, AppState};
use crate::theme::theme;
use crate::ui::icon::{icon, Icon};
use crate::ui::Button;
use crate::views::{current_lang, Page, ViewEvent};

/// 최근 접속 카드 최대 개수 (원본 `.slice(0, 6)`).
const RECENT_LIMIT: usize = 6;

pub struct DashboardView {
    state: Entity<AppState>,
    _subscription: gpui::Subscription,
}

impl EventEmitter<ViewEvent> for DashboardView {}

impl DashboardView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
        Self { state, _subscription: subscription }
    }

    fn card(&self, server: &Server, lang: Lang, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let id = server.id;
        // 원본과 동일하게 22번 포트는 접속 줄에서 감춘다.
        let mut ssh_line = format!("$ ssh {}@{}", server.username, server.host);
        if server.port != 22 {
            ssh_line.push_str(&format!(" -p {}", server.port));
        }
        let mut meta = match server.group_name.as_deref() {
            Some(g) if !g.trim().is_empty() => format!("group/{g}"),
            _ => "ungrouped".to_string(),
        };
        if let Some(at) = server.last_connected_at.as_deref() {
            // ISO 타임스탬프의 날짜 부분만 — 로케일 포매터를 끌어오지 않는다.
            meta.push_str(&format!(" · last {}", at.split('T').next().unwrap_or(at)));
        }
        let connected = server.last_connected_at.is_some();

        div()
            .id(("dash-card", id as usize))
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border_subtle)
            .bg(t.surface)
            .cursor_pointer()
            .hover(move |s| s.bg(t.hover))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        // 접속 이력이 있으면 accent 점 — 원본의 led/led-off 등가.
                        div().size(px(6.)).rounded(px(3.)).bg(if connected {
                            t.accent
                        } else {
                            t.text_disabled
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(t.text)
                            .child(server.name.clone()),
                    )
                    .when(server.is_favorite, |el| {
                        el.child(icon(Icon::Check).color(t.accent))
                    }),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(ssh_line),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_disabled)
                    .child(meta),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(
                        Button::new(("dash-connect", id as usize), tr(lang, TrKey::CommonConnect))
                            .primary()
                            .on_click(cx.listener(move |_, _, _window, cx| {
                                cx.stop_propagation();
                                cx.emit(ViewEvent::Connect(id));
                            })),
                    )
                    .child(
                        Button::new(("dash-edit", id as usize), tr(lang, TrKey::CommonEdit))
                            .on_click(cx.listener(move |_, _, _window, cx| {
                                cx.stop_propagation();
                                cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: Some(id) }));
                            })),
                    ),
            )
            .on_click(cx.listener(move |_, _, _window, cx| {
                cx.emit(ViewEvent::Connect(id));
            }))
    }

    fn section(&self, title: &'static str, glyph: Icon, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(icon(glyph).color(t.text_muted))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(title),
            )
    }
}

/// 최근 접속 서버 — ISO 문자열 역순, 최대 6개.
///
/// 원본 비교자는 `b > a ? 1 : -1`로 **동점에서 0을 돌려주지 않는다**.
/// 같은 타임스탬프의 순서를 흉내 내려면 `Equal`을 쓰지 않아야 한다.
pub fn recent_servers(servers: &[Server]) -> Vec<&Server> {
    let mut recent: Vec<&Server> =
        servers.iter().filter(|s| s.last_connected_at.is_some()).collect();
    recent.sort_by(|a, b| {
        let (a, b) = (
            a.last_connected_at.as_deref().unwrap_or(""),
            b.last_connected_at.as_deref().unwrap_or(""),
        );
        if b > a {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Less
        }
    });
    recent.truncate(RECENT_LIMIT);
    recent
}

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let servers = self.state.read(cx).servers.clone();
        let favorites: Vec<Server> =
            servers.iter().filter(|s| s.is_favorite).cloned().collect();
        let recent: Vec<Server> = recent_servers(&servers).into_iter().cloned().collect();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(t.text)
                    .child(tr(lang, TrKey::NavDashboard)),
            )
            .child(
                Button::new("dash-new-server", tr(lang, TrKey::DashboardNewServer))
                    .primary()
                    .on_click(cx.listener(|_, _, _window, cx| {
                        cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: None }));
                    })),
            );

        let mut body = div().flex().flex_col().gap(px(18.));

        if servers.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .py(px(48.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.text_muted)
                            .child(tr(lang, TrKey::DashboardEmptyHint)),
                    )
                    .child(
                        Button::new("dash-empty-cta", tr(lang, TrKey::DashboardAddServerCta))
                            .primary()
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: None }));
                            })),
                    ),
            );
        } else {
            if !favorites.is_empty() {
                let cards: Vec<_> = favorites.iter().map(|s| self.card(s, lang, cx)).collect();
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        // 원본에도 i18n 키가 없는 하드코딩 라벨 — 그대로 옮긴다.
                        .child(self.section("Favorites", Icon::Check, cx))
                        .child(div().flex().flex_col().gap(px(6.)).children(cards)),
                );
            }
            if !recent.is_empty() {
                let cards: Vec<_> = recent.iter().map(|s| self.card(s, lang, cx)).collect();
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(self.section("Recent Connections", Icon::Terminal, cx))
                        .child(div().flex().flex_col().gap(px(6.)).children(cards)),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(14.))
            .p(px(20.))
            .bg(t.bg)
            .child(header)
            .child(
                div()
                    .id("dashboard-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: i64, at: Option<&str>) -> Server {
        Server {
            id,
            last_connected_at: at.map(Into::into),
            ..Server::default()
        }
    }

    #[test]
    fn recent_excludes_never_connected_and_sorts_newest_first() {
        let servers = vec![
            server(1, Some("2024-01-01T00:00:00Z")),
            server(2, None),
            server(3, Some("2024-03-01T00:00:00Z")),
            server(4, Some("2024-02-01T00:00:00Z")),
        ];
        let ids: Vec<i64> = recent_servers(&servers).iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![3, 4, 1]);
    }

    #[test]
    fn recent_is_capped_at_six() {
        let servers: Vec<Server> =
            (0..10).map(|i| server(i, Some("2024-01-01T00:00:00Z"))).collect();
        assert_eq!(recent_servers(&servers).len(), RECENT_LIMIT);
    }

    #[test]
    fn recent_is_empty_when_nothing_connected() {
        let servers = vec![server(1, None), server(2, None)];
        assert!(recent_servers(&servers).is_empty());
    }
}
