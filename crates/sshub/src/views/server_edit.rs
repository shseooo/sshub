//! 서버 생성/편집 폼 (Electron `src/pages/ServerEdit.tsx`).
//!
//! 검증은 원본 zod 스키마를 그대로 옮긴 순수 함수(`validate`)로 두고, 뷰는
//! 결과를 필드 아래에 그리기만 한다. 원본은 첫 번째 오류 하나만 폼 상단에
//! 보여줬지만, 여기서는 필드별로 표시한다(DESIGN-ui.md §2 폼 검증).
use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, IntoElement, Subscription, Window};
use sshub_core::model::{AuthType, CreateServerDto, Server, UpdateServerDto};

use crate::i18n::{tr, Lang, TrKey};
use crate::state::{app_state, AppState, StateEvent};
use crate::theme::theme;
use crate::ui::select::SelectOption;
use crate::ui::text_area::TextArea;
use crate::ui::text_input::TextInput;
use crate::ui::{Button, FormField, Select, SelectEvent};
use crate::views::{blank_to_none, current_lang, Page, ViewEvent};

/// 검증 대상 필드.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Host,
    Port,
    Username,
}

/// zod 스키마 재현:
/// ```text
/// name:     z.string().trim().min(1, 'edit.errName')
/// host:     z.string().trim().min(1, 'edit.errHost')
/// port:     z.number().int(..).min(1, ..).max(65535, ..)
/// username: z.string().trim().min(1, 'edit.errUser')
/// ```
/// 반환 순서는 스키마 선언 순서와 같다.
pub fn validate(name: &str, host: &str, port: &str, username: &str) -> Vec<(Field, TrKey)> {
    let mut errors = Vec::new();
    if name.trim().is_empty() {
        errors.push((Field::Name, TrKey::EditErrName));
    }
    if host.trim().is_empty() {
        errors.push((Field::Host, TrKey::EditErrHost));
    }
    if let Some(key) = validate_port(port) {
        errors.push((Field::Port, key));
    }
    if username.trim().is_empty() {
        errors.push((Field::Username, TrKey::EditErrUser));
    }
    errors
}

/// 포트 문자열 검증. 빈 값은 원본의 `Number('') === 0`과 같게 취급해
/// "1 이상" 오류로 떨어뜨린다(숫자 입력을 비웠을 때의 동작).
fn validate_port(port: &str) -> Option<TrKey> {
    let trimmed = port.trim();
    if trimmed.is_empty() {
        return Some(TrKey::EditErrPortMin);
    }
    let Ok(value) = trimmed.parse::<f64>() else {
        return Some(TrKey::EditErrPortInt);
    };
    if !value.is_finite() || value.fract() != 0.0 {
        return Some(TrKey::EditErrPortInt);
    }
    if value < 1.0 {
        return Some(TrKey::EditErrPortMin);
    }
    if value > 65535.0 {
        return Some(TrKey::EditErrPortMax);
    }
    None
}

/// 저장된 tags(JSON 문자열 배열) → 쉼표 구분 입력값.
/// 배열이 아니거나 파싱에 실패하면 원문 그대로 (원본 `tagsToInput`).
pub fn tags_to_input(tags: Option<&str>) -> String {
    let Some(raw) = tags else { return String::new() };
    if raw.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                // JS `Array.join`은 null/undefined를 빈 문자열로 만든다.
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => raw.to_string(),
    }
}

/// 쉼표 구분 입력값 → JSON 문자열 배열. 항목이 없으면 `None`
/// (필드를 아예 보내지 않아 기존 값이 유지된다 — 원본 `inputToTags`).
pub fn input_to_tags(input: &str) -> Option<String> {
    let items: Vec<&str> = input
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if items.is_empty() {
        None
    } else {
        serde_json::to_string(&items).ok()
    }
}

fn auth_from_ix(ix: usize) -> AuthType {
    match ix {
        1 => AuthType::Password,
        2 => AuthType::Pem,
        3 => AuthType::Agent,
        _ => AuthType::Key,
    }
}

fn auth_to_ix(auth: AuthType) -> usize {
    match auth {
        AuthType::Key => 0,
        AuthType::Password => 1,
        AuthType::Pem => 2,
        AuthType::Agent => 3,
    }
}

pub struct ServerEditView {
    state: Entity<AppState>,
    server_id: Option<i64>,
    name: Entity<TextInput>,
    group: Entity<TextInput>,
    host: Entity<TextInput>,
    port: Entity<TextInput>,
    username: Entity<TextInput>,
    proxy_jump: Entity<TextInput>,
    tags: Entity<TextInput>,
    pem: Entity<TextArea>,
    notes: Entity<TextArea>,
    auth_select: Entity<Select>,
    key_select: Entity<Select>,
    auth: AuthType,
    errors: Vec<(Field, TrKey)>,
    form_error: Option<String>,
    saving: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ViewEvent> for ServerEditView {}

impl ServerEditView {
    pub fn new(server_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let lang = current_lang(cx);
        let existing: Option<Server> = server_id
            .and_then(|id| state.read(cx).servers.iter().find(|s| s.id == id).cloned());

        let text = |window: &mut Window, cx: &mut Context<Self>, value: String, ph: &'static str| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .with_text(value)
                    .with_placeholder(ph)
            })
        };

        let name = text(
            window,
            cx,
            existing.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            "",
        );
        let group = text(
            window,
            cx,
            existing
                .as_ref()
                .and_then(|s| s.group_name.clone())
                .unwrap_or_default(),
            tr(lang, TrKey::EditGroupPlaceholder),
        );
        let host = text(
            window,
            cx,
            existing.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
            tr(lang, TrKey::EditHostPlaceholder),
        );
        let port = text(
            window,
            cx,
            existing.as_ref().map(|s| s.port.to_string()).unwrap_or_else(|| "22".into()),
            "",
        );
        let username = text(
            window,
            cx,
            existing.as_ref().map(|s| s.username.clone()).unwrap_or_default(),
            tr(lang, TrKey::EditUserPlaceholder),
        );
        let proxy_jump = text(
            window,
            cx,
            existing
                .as_ref()
                .and_then(|s| s.proxy_jump.clone())
                .unwrap_or_default(),
            "user@bastion.example.com",
        );
        let tags = text(
            window,
            cx,
            tags_to_input(existing.as_ref().and_then(|s| s.tags.as_deref())),
            tr(lang, TrKey::EditTagsPlaceholder),
        );

        let pem = cx.new(|cx| {
            TextArea::new(window, cx)
                .with_placeholder("-----BEGIN RSA PRIVATE KEY-----\n...")
        });
        let notes = cx.new(|cx| {
            TextArea::new(window, cx).with_text(
                existing
                    .as_ref()
                    .and_then(|s| s.notes.clone())
                    .unwrap_or_default(),
            )
        });

        let auth = existing.as_ref().map(|s| s.auth_type).unwrap_or(AuthType::Key);
        let auth_select = cx.new(|cx| {
            Select::new(
                "server-auth-type",
                vec![
                    SelectOption::new("key", tr(lang, TrKey::EditAuthKey)),
                    SelectOption::new("password", tr(lang, TrKey::EditAuthPassword)),
                    SelectOption::new("pem", tr(lang, TrKey::EditAuthPem)),
                    SelectOption::new("agent", tr(lang, TrKey::EditAuthAgent)),
                ],
                cx,
            )
            .with_selected_ix(Some(auth_to_ix(auth)))
        });

        let key_options = key_options(&state.read(cx).keys, lang);
        let selected_key_ix = existing
            .as_ref()
            .and_then(|s| s.key_id)
            .and_then(|id| {
                key_options
                    .iter()
                    .position(|o| o.value.as_ref() == id.to_string())
            })
            .unwrap_or(0);
        let key_select = cx.new(|cx| {
            Select::new("server-key", key_options, cx).with_selected_ix(Some(selected_key_ix))
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(
            &auth_select,
            |this: &mut Self, _, event: &SelectEvent, cx| {
                let SelectEvent::Changed(ix) = event;
                this.auth = auth_from_ix(*ix);
                cx.notify();
            },
        ));
        // 키가 추가/삭제되면 선택지도 갱신돼야 한다.
        subscriptions.push(cx.subscribe(&state, |this: &mut Self, _, event, cx| {
            if matches!(event, StateEvent::KeysChanged) {
                this.sync_key_options(cx);
            }
        }));

        Self {
            state,
            server_id,
            name,
            group,
            host,
            port,
            username,
            proxy_jump,
            tags,
            pem,
            notes,
            auth_select,
            key_select,
            auth,
            errors: Vec::new(),
            form_error: None,
            saving: false,
            _subscriptions: subscriptions,
        }
    }

    fn sync_key_options(&mut self, cx: &mut Context<Self>) {
        let lang = current_lang(cx);
        let options = key_options(&self.state.read(cx).keys, lang);
        self.key_select.update(cx, |select, cx| select.set_options(options, cx));
        cx.notify();
    }

    fn selected_key_id(&self, cx: &Context<Self>) -> Option<i64> {
        self.key_select
            .read(cx)
            .selected_value()
            .and_then(|v| v.parse::<i64>().ok())
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        self.form_error = None;
        let name = self.name.read(cx).text().to_string();
        let host = self.host.read(cx).text().to_string();
        let port_raw = self.port.read(cx).text().to_string();
        let username = self.username.read(cx).text().to_string();

        self.errors = validate(&name, &host, &port_raw, &username);
        if !self.errors.is_empty() {
            cx.notify();
            return;
        }

        let port: i64 = port_raw.trim().parse().unwrap_or(22);
        let group = self.group.read(cx).text().to_string();
        let tags = self.tags.read(cx).text().to_string();
        let notes = self.notes.read(cx).text().to_string();
        let proxy_jump = self.proxy_jump.read(cx).text().to_string();
        let pem = self.pem.read(cx).text().to_string();
        let auth = self.auth;
        // 원본과 동일: 인증 방식이 key일 때, 선택된 키가 있을 때만 보낸다.
        let key_id = if auth == AuthType::Key {
            self.selected_key_id(cx)
        } else {
            None
        };

        self.saving = true;
        let result = self.state.update(cx, |state, cx| {
            if let Some(id) = self.server_id {
                state
                    .update_server(
                        UpdateServerDto {
                            id,
                            name: Some(name.trim().to_string()),
                            host: Some(host.trim().to_string()),
                            port: Some(port),
                            username: Some(username.trim().to_string()),
                            auth_type: Some(auth),
                            // `None` = 건드리지 않음 (원본의 `undefined`와 동일).
                            key_id: key_id.map(Some),
                            group_name: blank_to_none(&group).map(Some),
                            tags: input_to_tags(&tags).map(Some),
                            notes: blank_to_none(&notes).map(Some),
                            // proxy_jump는 authoritative — 비우면 삭제된다.
                            proxy_jump: blank_to_none(&proxy_jump),
                        },
                        cx,
                    )
                    .map(|s| s.id)
            } else {
                state
                    .create_server(
                        CreateServerDto {
                            name: name.trim().to_string(),
                            host: host.trim().to_string(),
                            port: Some(port),
                            username: username.trim().to_string(),
                            auth_type: auth,
                            key_id,
                            pem_data: None,
                            proxy_jump: blank_to_none(&proxy_jump),
                            group_name: blank_to_none(&group),
                            tags: input_to_tags(&tags),
                            notes: blank_to_none(&notes),
                        },
                        cx,
                    )
                    .map(|s| s.id)
            }
        });
        self.saving = false;

        match result {
            Ok(id) => {
                // PEM은 스토어가 아니라 0600 파일로만 존재한다 — 저장 후 별도 기록.
                if auth == AuthType::Pem && !pem.trim().is_empty() {
                    let pem = pem.trim().to_string();
                    self.state
                        .update(cx, |state, cx| {
                            state.spawn_core(cx, move |core| {
                                sshub_core::keys_io::write_server_pem(&core.keys_dir, id, &pem)
                            })
                        })
                        .detach();
                }
                cx.emit(ViewEvent::Navigate(Page::Servers));
            }
            Err(err) => {
                self.form_error = Some(err.to_string());
                cx.notify();
            }
        }
    }
}

fn key_options(keys: &[sshub_core::model::SshKeyView], lang: Lang) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new("", tr(lang, TrKey::EditKeyDefault))];
    options.extend(keys.iter().map(|k| {
        SelectOption::new(
            k.key.id.to_string(),
            format!("{} ({})", k.key.name, k.key.key_type.as_str()),
        )
    }));
    options
}

impl Render for ServerEditView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let is_edit = self.server_id.is_some();
        let pem_blank = self.pem.read(cx).text().trim().is_empty();

        let err = |view: &Self, field: Field| -> Option<&'static str> {
            view.errors
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, k)| tr(lang, *k))
        };

        let row = |a: gpui::AnyElement, b: gpui::AnyElement, ratio: f32| {
            div()
                .flex()
                .flex_row()
                .gap(px(10.))
                .child(div().flex_1().child(a))
                .child(div().w(px(ratio)).child(b))
        };

        let auth_block = match self.auth {
            AuthType::Key => div()
                .child(
                    FormField::new(tr(lang, TrKey::EditKeySelect), self.key_select.clone())
                        .hint(tr(lang, TrKey::EditKeyHint)),
                )
                .into_any_element(),
            AuthType::Password => div()
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(tr(lang, TrKey::EditPasswordHint))
                .into_any_element(),
            AuthType::Pem => div()
                .child(
                    FormField::new(
                        tr(lang, TrKey::EditPemLabel),
                        div().h(px(128.)).w_full().child(self.pem.clone()),
                    )
                    // 편집 중 PEM을 비워두면 "기존 키 유지" 안내로 바뀐다.
                    .hint(if is_edit && pem_blank {
                        tr(lang, TrKey::EditPemKeptHint)
                    } else {
                        tr(lang, TrKey::EditPemHint)
                    }),
                )
                .into_any_element(),
            AuthType::Agent => div()
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(tr(lang, TrKey::EditAgentHint))
                .into_any_element(),
        };

        let form = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(row(
                FormField::new(tr(lang, TrKey::EditName), self.name.clone())
                    .required(true)
                    .error(err(self, Field::Name))
                    .into_any_element(),
                FormField::new(tr(lang, TrKey::EditGroup), self.group.clone()).into_any_element(),
                220.,
            ))
            .child(row(
                FormField::new(tr(lang, TrKey::EditHost), self.host.clone())
                    .required(true)
                    .error(err(self, Field::Host))
                    .into_any_element(),
                FormField::new(tr(lang, TrKey::EditPort), self.port.clone())
                    .error(err(self, Field::Port))
                    .into_any_element(),
                110.,
            ))
            .child(
                FormField::new(tr(lang, TrKey::EditUser), self.username.clone())
                    .required(true)
                    .error(err(self, Field::Username)),
            )
            .child(FormField::new(
                tr(lang, TrKey::EditAuthType),
                self.auth_select.clone(),
            ))
            .child(auth_block)
            .child(
                FormField::new(tr(lang, TrKey::EditProxyJump), self.proxy_jump.clone())
                    .hint(tr(lang, TrKey::EditProxyJumpHint)),
            )
            .child(FormField::new(tr(lang, TrKey::EditTags), self.tags.clone()))
            .child(FormField::new(
                tr(lang, TrKey::EditNotes),
                div().h(px(80.)).w_full().child(self.notes.clone()),
            ));

        let buttons = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .child(
                Button::new(
                    "server-save",
                    if self.saving {
                        tr(lang, TrKey::CommonSaving)
                    } else {
                        tr(lang, TrKey::CommonSave)
                    },
                )
                .primary()
                .disabled(self.saving)
                .on_click(cx.listener(|this, _, _window, cx| this.submit(cx))),
            )
            .child(
                Button::new("server-cancel", tr(lang, TrKey::CommonCancel)).on_click(cx.listener(
                    |_, _, _window, cx| {
                        cx.emit(ViewEvent::Navigate(Page::Servers));
                    },
                )),
            );

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(14.))
            .p(px(20.))
            .bg(t.bg)
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(t.text)
                    .child(if is_edit {
                        tr(lang, TrKey::CommonEdit)
                    } else {
                        tr(lang, TrKey::CommonAddServer)
                    }),
            )
            .child(
                div()
                    .id("server-edit-body")
                    .flex_1()
                    .min_h(px(0.))
                    .max_w(px(640.))
                    .overflow_y_scroll()
                    .child(form),
            )
            .when_some(self.form_error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.danger)
                        .child(message),
                )
            })
            .child(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_form_has_no_errors() {
        assert!(validate("web", "10.0.0.1", "22", "deploy").is_empty());
    }

    #[test]
    fn blank_fields_are_rejected_after_trim() {
        let errors = validate("  ", "  ", "22", "\t");
        assert_eq!(
            errors,
            vec![
                (Field::Name, TrKey::EditErrName),
                (Field::Host, TrKey::EditErrHost),
                (Field::Username, TrKey::EditErrUser),
            ]
        );
    }

    #[test]
    fn port_bounds_map_to_distinct_messages() {
        let port_err = |p: &str| validate("n", "h", p, "u").first().map(|(_, k)| *k);
        assert_eq!(port_err("0"), Some(TrKey::EditErrPortMin));
        assert_eq!(port_err("65536"), Some(TrKey::EditErrPortMax));
        assert_eq!(port_err("22.5"), Some(TrKey::EditErrPortInt));
        assert_eq!(port_err("abc"), Some(TrKey::EditErrPortInt));
        // 빈 값은 원본의 Number('') === 0 과 같게 "1 이상" 오류.
        assert_eq!(port_err(""), Some(TrKey::EditErrPortMin));
        assert_eq!(port_err("1"), None);
        assert_eq!(port_err("65535"), None);
    }

    #[test]
    fn errors_follow_schema_declaration_order() {
        let errors = validate("", "", "0", "");
        let fields: Vec<Field> = errors.iter().map(|(f, _)| *f).collect();
        assert_eq!(fields, vec![Field::Name, Field::Host, Field::Port, Field::Username]);
    }

    #[test]
    fn tags_json_array_becomes_comma_space_string() {
        assert_eq!(tags_to_input(Some(r#"["web","aws"]"#)), "web, aws");
    }

    #[test]
    fn tags_passthrough_when_not_a_json_array() {
        assert_eq!(tags_to_input(Some("web,aws")), "web,aws");
        assert_eq!(tags_to_input(Some(r#"{"a":1}"#)), r#"{"a":1}"#);
        assert_eq!(tags_to_input(None), "");
        assert_eq!(tags_to_input(Some("")), "");
    }

    #[test]
    fn input_to_tags_trims_and_drops_empties() {
        assert_eq!(input_to_tags(" web , aws "), Some(r#"["web","aws"]"#.to_string()));
        assert_eq!(input_to_tags("web,,aws,"), Some(r#"["web","aws"]"#.to_string()));
    }

    #[test]
    fn input_to_tags_is_none_when_nothing_remains() {
        assert_eq!(input_to_tags(""), None);
        assert_eq!(input_to_tags("   "), None);
        assert_eq!(input_to_tags(",,,"), None);
    }

    #[test]
    fn tags_roundtrip_preserves_items() {
        let json = input_to_tags("web, aws, db").unwrap();
        assert_eq!(tags_to_input(Some(&json)), "web, aws, db");
    }

    #[test]
    fn auth_ix_roundtrips() {
        for auth in [AuthType::Key, AuthType::Password, AuthType::Pem, AuthType::Agent] {
            assert_eq!(auth_from_ix(auth_to_ix(auth)), auth);
        }
    }
}
