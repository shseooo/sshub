//! 위젯 킷 쇼케이스. `cargo run -p sshub --example widgets_demo`
//!
//! TextInput(일반/마스킹) · TextArea · Select · Button 4종 · Checkbox ·
//! ListItem · ConfirmDialog · FormField(에러) · Toast 스택을 한 화면에 띄운다.
//! 한글 IME는 여기서 수동 검증한다 (DESIGN-ui.md §8 리스크 1).
use gpui::{
    actions, div, point, prelude::*, px, size, App, Application, Bounds, Context, DismissEvent,
    Entity, FocusHandle, Focusable, KeyBinding, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use sshub::theme::{self, theme};
use sshub::ui::{
    self,
    icon::{icon, Icon},
    modal::ModalOverlay,
    select::SelectOption,
    text_area::TextArea,
    text_input::{InputEvent, TextInput},
    toast::render_toast_stack,
    Button, ButtonVariant, Checkbox, ConfirmDialog, FormField, ListItem, Select, SelectEvent, Toast,
    ToastKind,
};

actions!(widgets_demo, [Quit]);

struct Demo {
    focus_handle: FocusHandle,
    name: Entity<TextInput>,
    passphrase: Entity<TextInput>,
    pem: Entity<TextArea>,
    auth_type: Entity<Select>,
    remember: bool,
    show_name_error: bool,
    dialog: Option<Entity<ConfirmDialog>>,
    last_result: Option<bool>,
    toasts: Vec<Toast>,
    next_toast_id: u64,
    _subscriptions: Vec<gpui::Subscription>,
}

impl Demo {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_placeholder("my-server")
                .with_text("prod-web-01")
        });
        let passphrase =
            cx.new(|cx| TextInput::new(window, cx).with_placeholder("passphrase").with_masked(true));
        let pem = cx.new(|cx| {
            TextArea::new(window, cx)
                .with_placeholder("-----BEGIN OPENSSH PRIVATE KEY-----")
                .with_text(
                    "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                     b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
                     여러 줄 랩 동작을 보려면 이 줄처럼 아주 긴 한글 문장을 넣어 두면 편하다. \
                     wrap 전용이라 가로 스크롤은 생기지 않고 접혀야 한다.\n\
                     -----END OPENSSH PRIVATE KEY-----",
                )
        });
        let auth_type = cx.new(|cx| {
            Select::new(
                "auth-type",
                vec![
                    SelectOption::new("password", "Password"),
                    SelectOption::new("key", "SSH Key"),
                    SelectOption::new("pem", "PEM file"),
                    SelectOption::new("agent", "SSH Agent"),
                ],
                cx,
            )
            .with_placeholder("Select auth type")
            .with_selected_value("key")
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&name, |this: &mut Demo, input, event, cx| {
            if let InputEvent::Changed = event {
                this.show_name_error = input.read(cx).text().trim().is_empty();
                cx.notify();
            }
        }));
        subscriptions.push(cx.subscribe(
            &auth_type,
            |this: &mut Demo, select, event, cx| {
                let SelectEvent::Changed(ix) = event;
                let label = select
                    .read(cx)
                    .options()
                    .get(*ix)
                    .map(|o| o.label.clone())
                    .unwrap_or_default();
                this.push_toast(ToastKind::Info, format!("auth type → {label}"), cx);
            },
        ));

        Self {
            focus_handle: cx.focus_handle(),
            name,
            passphrase,
            pem,
            auth_type,
            remember: true,
            show_name_error: false,
            dialog: None,
            last_result: None,
            toasts: Vec::new(),
            next_toast_id: 1,
            _subscriptions: subscriptions,
        }
    }

    fn push_toast(&mut self, kind: ToastKind, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast::new(id, kind, message));
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        cx.notify();
    }

    fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // on_result는 FnOnce라 엔티티를 약참조로 잡아 결과를 돌려받는다.
        let this = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                "서버 삭제",
                "prod-web-01 을(를) 삭제할까요? 되돌릴 수 없습니다.",
                "삭제",
                "취소",
                cx,
            )
            .danger(true)
            .on_result(move |confirmed, _window, cx| {
                // 실제 앱에서는 여기서 mutation을 건다.
                this.update(cx, |demo, cx| {
                    demo.last_result = Some(confirmed);
                    let kind = if confirmed {
                        ToastKind::Success
                    } else {
                        ToastKind::Info
                    };
                    demo.push_toast(kind, if confirmed { "삭제됨" } else { "취소됨" }, cx);
                })
                .ok();
            })
        });
        dialog.read(cx).focus(window);
        let subscription = cx.subscribe(&dialog, |this: &mut Demo, _dialog, _: &DismissEvent, cx| {
            this.dialog = None;
            cx.notify();
        });
        self._subscriptions.push(subscription);
        self.dialog = Some(dialog);
        cx.notify();
    }
}

impl Focusable for Demo {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();

        let section = |title: &'static str| {
            div()
                .text_size(px(11.))
                .text_color(t.text_disabled)
                .child(title)
        };

        let card = || {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(16.))
                .rounded(px(10.))
                .border_1()
                .border_color(t.border)
                .bg(t.surface)
        };

        let buttons = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(8.))
            .child(Button::new("btn-primary", "Primary").variant(ButtonVariant::Primary).on_click(
                cx.listener(|this, _, _window, cx| {
                    this.push_toast(ToastKind::Success, "저장했습니다", cx);
                }),
            ))
            .child(Button::new("btn-secondary", "Secondary"))
            .child(Button::new("btn-ghost", "Ghost").variant(ButtonVariant::Ghost))
            .child(
                Button::new("btn-danger", "Danger")
                    .variant(ButtonVariant::Danger)
                    .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx))),
            )
            .child(Button::new("btn-disabled", "Disabled").disabled(true))
            .child(Button::new("btn-loading", "Loading").loading(true));

        let list = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                ListItem::new("row-1", "prod-web-01")
                    .leading(icon(Icon::Server).color(t.accent))
                    .subtitle("deploy@10.0.1.20:22 · key")
                    .trailing(icon(Icon::Pencil))
                    .trailing(icon(Icon::Trash))
                    .selected(true),
            )
            .child(
                ListItem::new("row-2", "staging-db")
                    .leading(icon(Icon::Server))
                    .subtitle("root@10.0.2.11:2222 · password")
                    .trailing(icon(Icon::Pencil))
                    .trailing(icon(Icon::Trash)),
            )
            .child(
                ListItem::new("row-3", "legacy-box (비활성)")
                    .leading(icon(Icon::Server))
                    .subtitle("보관됨")
                    .disabled(true),
            );

        let form = card()
            .child(section("FORM"))
            .child(
                FormField::new("Name", self.name.clone())
                    .required(true)
                    .error(self.show_name_error.then_some("이름은 비워둘 수 없습니다")),
            )
            .child(
                FormField::new("Passphrase", self.passphrase.clone())
                    .hint("마스킹 모드 — 화면에는 • 만 그려지고 편집/IME는 실제 텍스트로 동작"),
            )
            .child(FormField::new("Auth type", self.auth_type.clone()))
            .child(FormField::bare(
                Checkbox::new("remember", self.remember)
                    .label("이 서버를 즐겨찾기에 추가")
                    .on_toggle(cx.listener(|this, checked: &bool, _window, cx| {
                        this.remember = *checked;
                        cx.notify();
                    })),
            ))
            .child(
                FormField::new(
                    "Private key (PEM)",
                    div().h(px(120.)).w_full().child(self.pem.clone()),
                )
                .hint("wrap 전용 · 위/아래는 표시 행 단위 이동"),
            );

        let right = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .flex_1()
            .child(card().child(section("BUTTONS")).child(buttons))
            .child(card().child(section("LIST")).child(list))
            .child(
                card().child(section("DIALOG")).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            Button::new("open-dialog", "확인 다이얼로그 열기")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_dialog(window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child(match self.last_result {
                                    Some(true) => "마지막 결과: 확인",
                                    Some(false) => "마지막 결과: 취소",
                                    None => "아직 닫힌 적 없음",
                                }),
                        ),
                ),
            );

        div()
            .track_focus(&self.focus_handle)
            .key_context("WidgetsDemo")
            .relative()
            .size_full()
            .p(px(16.))
            .bg(t.bg)
            .text_color(t.text)
            .font_family("Helvetica")
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(12.))
                    .size_full()
                    .child(div().flex_1().child(form))
                    .child(right),
            )
            .child(render_toast_stack(&self.toasts, window, cx))
            .children(self.dialog.clone().map(|dialog| {
                ModalOverlay::new(dialog).on_backdrop_click(cx.listener(|this, _, _window, cx| {
                    this.dialog = None;
                    cx.notify();
                }))
            }))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        theme::init(cx);
        ui::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

        let bounds = Bounds::centered(None, size(px(980.), px(760.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("sshub widgets".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(12.), px(12.))),
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Demo::new(window, cx)),
            )
            .unwrap();

        window
            .update(cx, |demo, window, cx| {
                window.focus(&demo.name.focus_handle(cx));
                cx.activate(true);
            })
            .unwrap();
    });
}
