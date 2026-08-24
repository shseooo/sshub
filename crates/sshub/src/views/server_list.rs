//! 서버 목록 (Electron `src/pages/ServerList.tsx`).
//!
//! 검색 + 그룹 필터는 `AppState::filter_servers`가 단독으로 판단한다 —
//! 필터 규칙이 뷰와 상태 양쪽에 흩어지면 테스트가 무의미해진다.
use gpui::{
    div, prelude::*, px, Context, DismissEvent, Entity, EventEmitter, IntoElement, SharedString,
    Subscription, Window,
};
use sshub_core::model::Server;

use crate::i18n::{tr, tr_with, Lang, TrKey};
use crate::state::{app_state, AppState, StateEvent};
use crate::theme::theme;
use crate::ui::icon::{icon, Icon};
use crate::ui::modal::ModalOverlay;
use crate::ui::select::SelectOption;
use crate::ui::text_input::{InputEvent, TextInput};
use crate::ui::{Button, ConfirmDialog, Select, SelectEvent};
use crate::views::{current_lang, Page, ViewEvent};

/// "모든 그룹" 옵션의 값 — 빈 문자열은 그룹 이름으로 쓰일 수 없다.
const ALL_GROUPS: &str = "";

pub struct ServerListView {
    state: Entity<AppState>,
    search: Entity<TextInput>,
    group: Entity<Select>,
    query: String,
    group_filter: Option<String>,
    confirm: Option<Entity<ConfirmDialog>>,
    pending_delete: Option<i64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ViewEvent> for ServerListView {}

impl ServerListView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let lang = current_lang(cx);
        let search = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(tr(lang, TrKey::CommonLoading))
        });
        let groups = AppState::server_groups(&state.read(cx).servers);
        let group = cx.new(|cx| {
            Select::new("server-group-filter", group_options(&groups, lang), cx)
                .with_selected_ix(Some(0))
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&search, |this: &mut Self, input, event, cx| {
            if matches!(event, InputEvent::Changed) {
                this.query = input.read(cx).text().to_string();
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(&group, |this: &mut Self, select, event, cx| {
            let SelectEvent::Changed(ix) = event;
            let value = select.read(cx).options().get(*ix).map(|o| o.value.to_string());
            this.group_filter = match value.as_deref() {
                None | Some(ALL_GROUPS) => None,
                Some(g) => Some(g.to_string()),
            };
            cx.notify();
        }));
        // 서버가 추가/삭제되면 그룹 목록도 달라진다.
        subscriptions.push(cx.subscribe(&state, |this: &mut Self, _, event, cx| {
            if matches!(event, StateEvent::ServersChanged | StateEvent::SettingsChanged) {
                this.sync_group_options(cx);
                cx.notify();
            }
        }));

        // 플레이스홀더는 검색어 안내가 없으므로 비워 둔다 (기존 키에 검색 전용
        // 문자열이 없다 — 새 문자열을 지어내지 않는다).
        search.update(cx, |input, cx| input.set_placeholder("", cx));

        Self {
            state,
            search,
            group,
            query: String::new(),
            group_filter: None,
            confirm: None,
            pending_delete: None,
            _subscriptions: subscriptions,
        }
    }

    fn sync_group_options(&mut self, cx: &mut Context<Self>) {
        let lang = current_lang(cx);
        let groups = AppState::server_groups(&self.state.read(cx).servers);
        let selected = self.group_filter.clone();
        self.group.update(cx, |select, cx| {
            select.set_options(group_options(&groups, lang), cx);
            let ix = match &selected {
                None => Some(0),
                Some(g) => select
                    .options()
                    .iter()
                    .position(|o| o.value.as_ref() == g.as_str())
                    // 선택했던 그룹이 사라졌으면 "모든 그룹"으로 되돌린다.
                    .or(Some(0)),
            };
            select.set_selected_ix(ix, cx);
        });
        if let Some(g) = selected {
            let still_there = self
                .group
                .read(cx)
                .selected_value()
                .map(|v| v.as_ref() == g.as_str())
                .unwrap_or(false);
            if !still_there {
                self.group_filter = None;
            }
        }
    }

    fn ask_delete(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let lang = current_lang(cx);
        let this = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                tr(lang, TrKey::ListConfirmDeleteTitle),
                tr(lang, TrKey::ListConfirmDelete),
                tr(lang, TrKey::CommonDelete),
                tr(lang, TrKey::CommonCancel),
                cx,
            )
            .danger(true)
            .on_result(move |confirmed, _window, cx| {
                if let Some(view) = this.upgrade() {
                    view.update(cx, |view, cx| view.finish_delete(confirmed, cx));
                }
            })
        });
        dialog.read(cx).focus(window);
        self.pending_delete = Some(id);
        self.confirm = Some(dialog.clone());
        cx.subscribe(&dialog, |this: &mut Self, _, _: &DismissEvent, cx| {
            this.confirm = None;
            cx.notify();
        })
        .detach();
        cx.notify();
    }

    fn finish_delete(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        let Some(id) = self.pending_delete.take() else { return };
        self.confirm = None;
        if confirmed {
            self.state.update(cx, |state, cx| {
                let _ = state.delete_server(id, cx);
            });
        }
        cx.notify();
    }

    fn row(&self, server: &Server, lang: Lang, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let id = server.id;
        let group_name: SharedString = format!("server-row-{id}").into();
        // 목록은 원본과 동일하게 포트를 항상 노출한다 (대시보드만 22를 감춘다).
        let subtitle = format!("{}@{}:{}", server.username, server.host, server.port);
        let favorite = server.is_favorite;
        // `Host a b c` 같은 블록에서 온 항목. 접속은 되지만 config를 고치는
        // 조작(편집·삭제)은 막는다 — 패턴 하나를 손대면 나머지 패턴의 의미가
        // 같이 바뀐다. 즐겨찾기는 사이드카(별칭 키)에만 쓰므로 그대로 둔다.
        let read_only = server.read_only;

        let meta = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .when_some(server.group_name.clone(), |el, g| {
                el.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(t.elevated)
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .child(g),
                )
            })
            .when(read_only, |el| {
                el.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(t.border)
                        .text_size(px(11.))
                        .text_color(t.text_disabled)
                        .child(tr(lang, TrKey::ListReadOnly)),
                )
            });

        // 행 전체가 아니라 이름 영역만 접속 트리거 — 액션 버튼 클릭이
        // 접속으로 새어나가지 않게 한다.
        let body = div()
            .id(("server-open", id as usize))
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .gap(px(2.))
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.text)
                            .child(server.name.clone()),
                    )
                    .child(meta),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(subtitle),
            )
            .on_click(cx.listener(move |_, _, _window, cx| {
                cx.emit(ViewEvent::Connect(id));
            }));

        let actions = div()
            .flex()
            .flex_none()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .child(
                Button::new(("connect", id as usize), tr(lang, TrKey::CommonConnect))
                    .primary()
                    .on_click(cx.listener(move |_, _, _window, cx| {
                        cx.stop_propagation();
                        cx.emit(ViewEvent::Connect(id));
                    })),
            )
            .child(
                Button::new(("edit", id as usize), tr(lang, TrKey::CommonEdit))
                    .disabled(read_only)
                    .on_click(cx.listener(move |_, _, _window, cx| {
                        cx.stop_propagation();
                        if read_only {
                            return;
                        }
                        cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: Some(id) }));
                    })),
            )
            .child(
                div()
                    .id(("favorite", id as usize))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(t.hover))
                    .child(icon(Icon::Check).color(if favorite {
                        t.accent
                    } else {
                        t.text_disabled
                    }))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, cx| {
                            let _ = state.toggle_favorite(id, cx);
                        });
                    })),
            )
            // 삭제 버튼은 아예 그리지 않는다 — 눌러도 아무 일이 없는 버튼보다
            // 없는 편이 "이 항목은 앱 소유가 아니다"를 정확히 전달한다.
            .when(!read_only, |el| {
                el.child(
                    div()
                        .id(("delete", id as usize))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(26.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.hover))
                        .child(icon(Icon::Trash).color(t.text_disabled))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.ask_delete(id, window, cx);
                        })),
                )
            });

        div()
            .id(("server-row", id as usize))
            .group(group_name)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_subtle)
            .bg(t.surface)
            .hover(move |s| s.bg(t.hover))
            .child(icon(Icon::Server).color(t.text_muted))
            .child(body)
            .child(actions)
    }
}

fn group_options(groups: &[String], lang: Lang) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new(ALL_GROUPS, tr(lang, TrKey::ListAllGroups))];
    options.extend(groups.iter().map(|g| SelectOption::new(g.clone(), g.clone())));
    options
}

impl Render for ServerListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let all = self.state.read(cx).servers.clone();
        let total = all.len();
        let filtered: Vec<Server> =
            AppState::filter_servers(&all, &self.query, self.group_filter.as_deref())
                .into_iter()
                .cloned()
                .collect();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .text_color(t.text)
                            .child(tr(lang, TrKey::NavServers)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(tr_with(
                                lang,
                                TrKey::ListSubtitle,
                                &[("n", &total.to_string())],
                            )),
                    ),
            )
            .child(
                Button::new("add-server", tr(lang, TrKey::CommonAddServer))
                    .primary()
                    .on_click(cx.listener(|_, _, _window, cx| {
                        cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: None }));
                    })),
            );

        let filters = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .h(px(32.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.surface)
                    .child(icon(Icon::Search))
                    .child(div().flex_1().child(self.search.clone())),
            )
            // 원본과 동일하게 그룹이 하나도 없으면 필터 자체를 감춘다.
            .when(!AppState::server_groups(&all).is_empty(), |el| {
                el.child(div().w(px(180.)).child(self.group.clone()))
            });

        let body = if filtered.is_empty() {
            let hint = if total == 0 {
                tr(lang, TrKey::ListEmptyHintNoServers)
            } else {
                tr(lang, TrKey::ListEmptyHintNoMatch)
            };
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
                        .child(hint),
                )
                .when(total == 0, |el| {
                    el.child(
                        Button::new("empty-add-server", tr(lang, TrKey::CommonAddServer))
                            .primary()
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(ViewEvent::Navigate(Page::ServerEdit { id: None }));
                            })),
                    )
                })
                .into_any_element()
        } else {
            let rows: Vec<_> = filtered.iter().map(|s| self.row(s, lang, cx)).collect();
            div()
                .id("server-rows")
                .flex()
                .flex_col()
                .gap(px(6.))
                .overflow_y_scroll()
                .children(rows)
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(14.))
            .p(px(20.))
            .bg(t.bg)
            .child(header)
            .child(filters)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .when_some(self.confirm.clone(), |el, dialog| {
                el.child(ModalOverlay::new(dialog))
            })
    }
}
