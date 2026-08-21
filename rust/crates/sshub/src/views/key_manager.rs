//! SSH 키 관리 (Electron `src/pages/KeyManager.tsx`).
//!
//! ssh-keygen을 부르는 작업(생성·가져오기·수정·패스프레이즈 변경·공개키 유도)은
//! 초 단위가 걸릴 수 있어 **전부** `AppState::spawn_core`로 워커에 보낸다.
use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, IntoElement, PathPromptOptions, Subscription, Window,
};
use sshub_core::keys_io;
use sshub_core::model::{CreateKeyDto, ImportKeyDto, KeyType, LoadedKeyFile, SshKey, UpdateKeyDto};

use crate::i18n::{tr, tr_with, Lang, TrKey};
use crate::state::{app_state, AppState};
use crate::theme::theme;
use crate::ui::icon::{icon, Icon};
use crate::ui::modal::ModalOverlay;
use crate::ui::select::SelectOption;
use crate::ui::text_area::TextArea;
use crate::ui::text_input::TextInput;
use crate::ui::{Button, ConfirmDialog, FormField, Select, SelectEvent};
use crate::views::current_lang;

/// 공개 키를 가릴 때 쓰는 문자열 (원본과 같은 24개 불릿).
const MASKED_PUBLIC_KEY: &str = "••••••••••••••••••••••••";

/// 공개 키 접두어로 타입 판별 (원본 `detectKeyType`).
pub fn detect_key_type(public_key: &str) -> Option<KeyType> {
    if public_key.starts_with("ssh-ed25519") {
        Some(KeyType::Ed25519)
    } else if public_key.starts_with("ssh-rsa") {
        Some(KeyType::Rsa)
    } else if public_key.starts_with("ecdsa-") {
        Some(KeyType::Ecdsa)
    } else if public_key.starts_with("ssh-dss") {
        Some(KeyType::Dsa)
    } else {
        None
    }
}

/// 화면 표기용 키 타입 라벨 (원본 `keyTypeLabels` — 고유명사라 번역 대상이 아니다).
pub fn key_type_label(key_type: KeyType) -> &'static str {
    match key_type {
        KeyType::Ed25519 => "Ed25519",
        KeyType::Rsa => "RSA",
        KeyType::Ecdsa => "ECDSA",
        KeyType::Dsa => "DSA",
    }
}

fn key_type_from_value(value: &str) -> KeyType {
    match value {
        "rsa" => KeyType::Rsa,
        "ecdsa" => KeyType::Ecdsa,
        "dsa" => KeyType::Dsa,
        _ => KeyType::Ed25519,
    }
}

// ---------------------------------------------------------------------------
// 다이얼로그 (생성 / 가져오기 / 편집)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum DialogMode {
    Create,
    Import,
    Edit(SshKey),
}

pub struct KeyDialog {
    mode: DialogMode,
    focus_handle: FocusHandle,
    state: Entity<AppState>,
    name: Entity<TextInput>,
    key_type: Entity<Select>,
    key_size: Entity<Select>,
    public_key: Entity<TextArea>,
    private_key: Entity<TextArea>,
    passphrase: Entity<TextInput>,
    current_passphrase: Entity<TextInput>,
    new_passphrase: Entity<TextInput>,
    selected_type: KeyType,
    busy: bool,
    deriving: bool,
    passphrase_busy: bool,
    error: Option<String>,
    load_error: Option<String>,
    derive_error: Option<String>,
    /// (성공 여부, 메시지) — 패스프레이즈 변경 결과.
    passphrase_message: Option<(bool, String)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for KeyDialog {}

impl Focusable for KeyDialog {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl KeyDialog {
    pub fn new(mode: DialogMode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let lang = current_lang(cx);
        let existing = match &mode {
            DialogMode::Edit(key) => Some(key.clone()),
            _ => None,
        };

        let name = cx.new(|cx| {
            let input = TextInput::new(window, cx).with_text(
                existing.as_ref().map(|k| k.name.clone()).unwrap_or_default(),
            );
            match mode {
                DialogMode::Create => input.with_placeholder(tr(lang, TrKey::KeysNamePlaceholderCreate)),
                DialogMode::Import => input.with_placeholder(tr(lang, TrKey::KeysNamePlaceholderImport)),
                // 편집 모드에는 플레이스홀더가 없다 (원본과 동일).
                DialogMode::Edit(_) => input,
            }
        });

        // 생성 시에는 dsa를 만들 수 없다 (원본도 3종만 노출).
        let type_options = match mode {
            DialogMode::Create => vec![
                SelectOption::new("ed25519", tr(lang, TrKey::KeysEd25519Rec)),
                SelectOption::new("rsa", "RSA"),
                SelectOption::new("ecdsa", "ECDSA"),
            ],
            _ => vec![
                SelectOption::new("rsa", "RSA"),
                SelectOption::new("ed25519", "Ed25519"),
                SelectOption::new("ecdsa", "ECDSA"),
                SelectOption::new("dsa", "DSA"),
            ],
        };
        let selected_type = existing
            .as_ref()
            .map(|k| k.key_type)
            .unwrap_or(match mode {
                DialogMode::Create => KeyType::Ed25519,
                // 가져오기 기본값은 rsa (원본과 동일).
                _ => KeyType::Rsa,
            });
        let selected_ix = type_options
            .iter()
            .position(|o| o.value.as_ref() == selected_type.as_str())
            .unwrap_or(0);
        let key_type = cx.new(|cx| {
            Select::new("key-type", type_options, cx).with_selected_ix(Some(selected_ix))
        });

        let key_size = cx.new(|cx| {
            Select::new(
                "key-size",
                vec![
                    SelectOption::new("2048", "2048"),
                    SelectOption::new("3072", "3072"),
                    SelectOption::new("4096", "4096"),
                ],
                cx,
            )
            // 기본 3072.
            .with_selected_ix(Some(1))
        });

        let public_key = cx.new(|cx| {
            TextArea::new(window, cx)
                .with_text(
                    existing
                        .as_ref()
                        .map(|k| k.public_key.clone())
                        .unwrap_or_default(),
                )
                .with_placeholder("ssh-ed25519 AAAA... user@host")
        });
        let private_key = cx.new(|cx| {
            let area = TextArea::new(window, cx);
            match mode {
                DialogMode::Edit(_) => {
                    area.with_placeholder(tr(lang, TrKey::KeysReplacePrivatePlaceholder))
                }
                _ => area.with_placeholder("-----BEGIN OPENSSH PRIVATE KEY-----\n..."),
            }
        });

        let masked = |window: &mut Window, cx: &mut Context<Self>, ph: &'static str| {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .with_masked(true)
                    .with_placeholder(ph)
            })
        };
        let passphrase = masked(
            window,
            cx,
            match mode {
                DialogMode::Create => tr(lang, TrKey::KeysPassphraseCreatePlaceholder),
                _ => tr(lang, TrKey::KeysPassphraseImportPlaceholder),
            },
        );
        let current_passphrase = masked(window, cx, tr(lang, TrKey::KeysCurrentPassphrase));
        let new_passphrase = masked(window, cx, tr(lang, TrKey::KeysNewPassphrase));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(
            &key_type,
            |this: &mut Self, select, event: &SelectEvent, cx| {
                let SelectEvent::Changed(ix) = event;
                if let Some(option) = select.read(cx).options().get(*ix) {
                    this.selected_type = key_type_from_value(option.value.as_ref());
                }
                cx.notify();
            },
        ));

        Self {
            mode,
            focus_handle: cx.focus_handle(),
            state,
            name,
            key_type,
            key_size,
            public_key,
            private_key,
            passphrase,
            current_passphrase,
            new_passphrase,
            selected_type,
            busy: false,
            deriving: false,
            passphrase_busy: false,
            error: None,
            load_error: None,
            derive_error: None,
            passphrase_message: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    fn title(&self, lang: Lang) -> &'static str {
        match self.mode {
            DialogMode::Create => tr(lang, TrKey::KeysCreateTitle),
            DialogMode::Import => tr(lang, TrKey::KeysImportTitle),
            DialogMode::Edit(_) => tr(lang, TrKey::KeysEditTitle),
        }
    }

    // -- 파일에서 불러오기 ---------------------------------------------------

    fn pick_file(&mut self, cx: &mut Context<Self>) {
        let lang = current_lang(cx);
        self.load_error = None;
        // NOTE(gpui 0.2.2): PathPromptOptions에는 기본 디렉터리·확장자 필터가
        // 없다 — 원본의 `~/.ssh` 기본 경로는 재현할 수 없다.
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(lang, TrKey::KeysDialogPickTitle).into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update(cx, |dialog, cx| dialog.load_from_path(path, cx)).ok();
        })
        .detach();
    }

    fn load_from_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let task = self
            .state
            .update(cx, |state, cx| {
                state.spawn_core(cx, move |_core| keys_io::load_key_file(&path))
            });
        cx.spawn(async move |this, cx| {
            let loaded = task.await;
            this.update(cx, |dialog, cx| match loaded {
                Ok(file) => dialog.apply_loaded(file, cx),
                Err(err) => {
                    let lang = current_lang(cx);
                    dialog.load_error = Some(tr_with(
                        lang,
                        TrKey::KeysLoadError,
                        &[("err", &err.to_string())],
                    ));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_loaded(&mut self, loaded: LoadedKeyFile, cx: &mut Context<Self>) {
        if let Some(private) = loaded.private_key.clone() {
            self.private_key.update(cx, |area, cx| area.set_text(private, cx));
        }
        // 이름 자동 채움은 비어 있을 때만, 그리고 편집 모드에서는 하지 않는다.
        if !matches!(self.mode, DialogMode::Edit(_)) && !loaded.file_name.is_empty() {
            let blank = self.name.read(cx).text().trim().is_empty();
            if blank {
                self.name
                    .update(cx, |input, cx| input.set_text(loaded.file_name.clone(), cx));
            }
        }
        match loaded.public_key {
            Some(public) => self.set_public_key(public, cx),
            None => {
                if loaded.private_key.is_some() {
                    // 공개 키가 없으면 개인 키에서 유도한다. 암호로 보호된
                    // 키라면 실패하는데, 원본과 같이 조용히 넘어간다.
                    self.derive_public_key(true, cx);
                }
            }
        }
        cx.notify();
    }

    fn set_public_key(&mut self, public: String, cx: &mut Context<Self>) {
        if let Some(detected) = detect_key_type(&public) {
            self.selected_type = detected;
            let ix = self
                .key_type
                .read(cx)
                .options()
                .iter()
                .position(|o| o.value.as_ref() == detected.as_str());
            if ix.is_some() {
                self.key_type.update(cx, |select, cx| select.set_selected_ix(ix, cx));
            }
        }
        self.public_key.update(cx, |area, cx| area.set_text(public, cx));
        cx.notify();
    }

    fn derive_public_key(&mut self, silent: bool, cx: &mut Context<Self>) {
        let pem = self.private_key.read(cx).text().trim().to_string();
        if pem.is_empty() {
            return;
        }
        let passphrase = non_empty(self.passphrase.read(cx).text().as_ref());
        if !silent {
            self.derive_error = None;
            self.deriving = true;
        }
        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| {
                keys_io::derive_public_key_from_pem(&core.keys_dir, &pem, passphrase.as_deref())
            })
        });
        cx.spawn(async move |this, cx| {
            let derived = task.await;
            this.update(cx, |dialog, cx| {
                dialog.deriving = false;
                match derived {
                    Ok(public) => dialog.set_public_key(public, cx),
                    Err(err) => {
                        if !silent {
                            dialog.derive_error = Some(err.to_string());
                        }
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    // -- 제출 ---------------------------------------------------------------

    fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let key_type = self.selected_type;
        let passphrase = non_empty(self.passphrase.read(cx).text().as_ref());
        self.error = None;
        self.busy = true;
        cx.notify();

        let task = match &self.mode {
            DialogMode::Create => {
                let key_size = if key_type == KeyType::Rsa {
                    self.key_size
                        .read(cx)
                        .selected_value()
                        .and_then(|v| v.parse::<i64>().ok())
                } else {
                    None
                };
                self.state.update(cx, |state, cx| {
                    state.spawn_core(cx, move |core| {
                        keys_io::create_ssh_key(
                            &mut core.store,
                            &core.keys_dir,
                            &CreateKeyDto {
                                name,
                                key_type: key_type.as_str().to_string(),
                                key_size,
                                passphrase,
                            },
                        )
                        .map(|_| ())
                    })
                })
            }
            DialogMode::Import => {
                let public_key = self.public_key.read(cx).text().trim().to_string();
                let pem_data = non_empty(self.private_key.read(cx).text().trim());
                if public_key.is_empty() && pem_data.is_none() {
                    self.busy = false;
                    return;
                }
                self.state.update(cx, |state, cx| {
                    state.spawn_core(cx, move |core| {
                        keys_io::import_ssh_key(
                            &mut core.store,
                            &core.keys_dir,
                            &ImportKeyDto {
                                name,
                                public_key,
                                pem_data,
                                key_type,
                                passphrase,
                            },
                        )
                        .map(|_| ())
                    })
                })
            }
            DialogMode::Edit(key) => {
                let id = key.id;
                let public_key = self.public_key.read(cx).text().trim().to_string();
                // 비워두면 저장된 개인 키를 그대로 둔다.
                let pem_data = non_empty(self.private_key.read(cx).text().trim());
                self.state.update(cx, |state, cx| {
                    state.spawn_core(cx, move |core| {
                        keys_io::update_ssh_key(
                            &mut core.store,
                            &core.keys_dir,
                            &UpdateKeyDto {
                                id,
                                name,
                                public_key,
                                key_type,
                                pem_data,
                                passphrase,
                            },
                        )
                        .map(|_| ())
                    })
                })
            }
        };

        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |dialog, cx| {
                dialog.busy = false;
                match result {
                    Ok(()) => cx.emit(DismissEvent),
                    Err(err) => dialog.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn change_passphrase(&mut self, cx: &mut Context<Self>) {
        let DialogMode::Edit(key) = &self.mode else { return };
        let id = key.id;
        let current = non_empty(self.current_passphrase.read(cx).text().as_ref());
        let next = non_empty(self.new_passphrase.read(cx).text().as_ref());
        self.passphrase_message = None;
        self.passphrase_busy = true;
        cx.notify();

        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| {
                keys_io::change_key_passphrase(
                    &mut core.store,
                    &core.keys_dir,
                    id,
                    current.as_deref(),
                    next.as_deref(),
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |dialog, cx| {
                dialog.passphrase_busy = false;
                let lang = current_lang(cx);
                match result {
                    Ok(()) => {
                        dialog
                            .current_passphrase
                            .update(cx, |input, cx| input.set_text("", cx));
                        dialog
                            .new_passphrase
                            .update(cx, |input, cx| input.set_text("", cx));
                        dialog.passphrase_message =
                            Some((true, tr(lang, TrKey::KeysPassphraseChanged).to_string()));
                    }
                    Err(err) => dialog.passphrase_message = Some((false, err.to_string())),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

impl Render for KeyDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let is_edit = matches!(self.mode, DialogMode::Edit(_));
        let is_create = matches!(self.mode, DialogMode::Create);
        let name_blank = self.name.read(cx).text().trim().is_empty();

        let hint = |text: &'static str| {
            div()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(text)
        };
        let danger_line = |text: String| {
            div()
                .text_size(px(11.))
                .text_color(t.danger)
                .child(text)
        };

        let mut body = div().flex().flex_col().gap(px(10.));

        // 가져오기/편집: 파일에서 불러오기 버튼이 맨 위.
        if !is_create {
            body = body
                .child(
                    Button::new("key-load-file", tr(lang, TrKey::KeysLoadFromFile)).on_click(
                        cx.listener(|this, _, _window, cx| this.pick_file(cx)),
                    ),
                )
                .when(!is_edit, |el| {
                    el.child(hint(tr(lang, TrKey::KeysLoadFromFileHint)))
                })
                .when_some(self.load_error.clone(), |el, message| {
                    el.child(danger_line(message))
                });
        }

        body = body.child(
            FormField::new(tr(lang, TrKey::KeysName), self.name.clone())
                .required(true)
                .when(is_create, |field| {
                    field.hint(tr(lang, TrKey::KeysCreateNameHint))
                }),
        );

        body = body.child(
            FormField::new(tr(lang, TrKey::KeysKeyType), self.key_type.clone()).when(
                matches!(self.mode, DialogMode::Import),
                |field| field.hint(tr(lang, TrKey::KeysImportTypeNote)),
            ),
        );

        // 키 길이는 RSA 생성일 때만.
        if is_create && self.selected_type == KeyType::Rsa {
            body = body.child(FormField::new(
                tr(lang, TrKey::KeysKeySize),
                self.key_size.clone(),
            ));
        }

        if !is_create {
            body = body.child(
                FormField::new(
                    tr(lang, TrKey::KeysPublicKeyLabel),
                    div().h(px(80.)).w_full().child(self.public_key.clone()),
                )
                .when(!is_edit, |field| {
                    field.hint(tr(lang, TrKey::KeysPublicKeyOptHint))
                }),
            );

            let private_label = if is_edit {
                tr(lang, TrKey::KeysReplacePrivateOpt)
            } else {
                tr(lang, TrKey::KeysPrivateKeyOpt)
            };
            let private_hint = if is_edit {
                tr(lang, TrKey::KeysReplacePrivateHint)
            } else {
                tr(lang, TrKey::KeysPrivateKeyHint)
            };
            body = body.child(
                FormField::new(
                    private_label,
                    div().h(px(96.)).w_full().child(self.private_key.clone()),
                )
                .hint(private_hint),
            );

            body = body
                .child(
                    Button::new(
                        "key-derive",
                        if self.deriving {
                            tr(lang, TrKey::KeysDeriving)
                        } else {
                            tr(lang, TrKey::KeysDerivePub)
                        },
                    )
                    .disabled(self.deriving || self.private_key.read(cx).text().trim().is_empty())
                    .on_click(cx.listener(|this, _, _window, cx| this.derive_public_key(false, cx))),
                )
                .when_some(self.derive_error.clone(), |el, message| {
                    el.child(danger_line(message))
                });
        }

        body = body.child(FormField::new(
            tr(lang, TrKey::KeysPassphraseOpt),
            self.passphrase.clone(),
        ));

        // 편집 모드에만 있는 패스프레이즈 변경 패널 (폼 제출과 별개 동작).
        if let DialogMode::Edit(key) = &self.mode {
            let protected = key.passphrase_protected;
            let panel = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(10.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.border)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text)
                        .child(tr(lang, TrKey::KeysChangePassphrase)),
                )
                // 현재 패스프레이즈는 보호된 키일 때만 묻는다.
                .when(protected, |el| {
                    el.child(self.current_passphrase.clone())
                })
                .child(self.new_passphrase.clone())
                .child(
                    Button::new(
                        "key-change-passphrase",
                        if self.passphrase_busy {
                            tr(lang, TrKey::KeysChanging)
                        } else {
                            tr(lang, TrKey::KeysChangePassphraseBtn)
                        },
                    )
                    .disabled(self.passphrase_busy)
                    .on_click(cx.listener(|this, _, _window, cx| this.change_passphrase(cx))),
                )
                .when_some(self.passphrase_message.clone(), |el, (ok, message)| {
                    el.child(
                        div()
                            .text_size(px(11.))
                            .text_color(if ok { t.success } else { t.danger })
                            .child(message),
                    )
                });
            body = body.child(panel);
        }

        let submit_label = match self.mode {
            DialogMode::Create if self.busy => tr(lang, TrKey::KeysCreating),
            DialogMode::Create => tr(lang, TrKey::KeysCreate),
            DialogMode::Import if self.busy => tr(lang, TrKey::KeysImporting),
            DialogMode::Import => tr(lang, TrKey::KeysImport),
            DialogMode::Edit(_) if self.busy => tr(lang, TrKey::CommonSaving),
            DialogMode::Edit(_) => tr(lang, TrKey::CommonSave),
        };
        let submit_disabled = self.busy
            || name_blank
            || (matches!(self.mode, DialogMode::Import)
                && self.public_key.read(cx).text().trim().is_empty()
                && self.private_key.read(cx).text().trim().is_empty());

        div()
            .key_context("KeyDialog")
            .track_focus(&self.focus_handle)
            .occlude()
            .flex()
            .flex_col()
            .gap(px(12.))
            .w(px(460.))
            .max_h(px(620.))
            .p(px(16.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .text_color(t.text)
                            .child(self.title(lang)),
                    )
                    .child(
                        div()
                            .id("key-dialog-close")
                            .cursor_pointer()
                            .child(icon(Icon::Close))
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                    ),
            )
            .child(
                div()
                    .id("key-dialog-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(body),
            )
            .when_some(self.error.clone(), |el, message| {
                el.child(danger_line(message))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("key-dialog-cancel", tr(lang, TrKey::CommonCancel)).on_click(
                            cx.listener(|_, _, _window, cx| cx.emit(DismissEvent)),
                        ),
                    )
                    .child(
                        Button::new("key-dialog-submit", submit_label)
                            .primary()
                            .disabled(submit_disabled)
                            .on_click(cx.listener(|this, _, _window, cx| this.submit(cx))),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// 키 목록 화면
// ---------------------------------------------------------------------------

pub struct KeyManagerView {
    state: Entity<AppState>,
    dialog: Option<Entity<KeyDialog>>,
    confirm: Option<Entity<ConfirmDialog>>,
    pending_delete: Option<i64>,
    revealed: HashSet<i64>,
    _subscription: Subscription,
}

impl KeyManagerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            dialog: None,
            confirm: None,
            pending_delete: None,
            revealed: HashSet::new(),
            _subscription: subscription,
        }
    }

    fn open_dialog(&mut self, mode: DialogMode, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| KeyDialog::new(mode, window, cx));
        dialog.read(cx).focus(window);
        cx.subscribe(&dialog, |this: &mut Self, _, _: &DismissEvent, cx| {
            this.dialog = None;
            cx.notify();
        })
        .detach();
        self.dialog = Some(dialog);
        cx.notify();
    }

    fn ask_delete(&mut self, key: &SshKey, window: &mut Window, cx: &mut Context<Self>) {
        let lang = current_lang(cx);
        let message = tr_with(lang, TrKey::KeysConfirmDelete, &[("name", &key.name)]);
        let this = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                tr(lang, TrKey::KeysConfirmDeleteTitle),
                message,
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
        self.pending_delete = Some(key.id);
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
            self.state
                .update(cx, |state, cx| {
                    state.spawn_core(cx, move |core| {
                        keys_io::delete_ssh_key(&mut core.store, &core.keys_dir, id)
                    })
                })
                .detach();
        }
        cx.notify();
    }

    fn card(
        &self,
        view: &sshub_core::model::SshKeyView,
        lang: Lang,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        let key = view.key.clone();
        let id = key.id;
        let revealed = self.revealed.contains(&id);
        let public_key = key.public_key.clone();

        let mut meta = key_type_label(key.key_type).to_string();
        // key_size가 0이면 표시하지 않는다 (원본의 truthy 검사와 동일).
        if key.key_size != 0 {
            meta.push_str(&format!(" ({})", key.key_size));
        }
        if key.passphrase_protected {
            meta.push_str(" 🔒");
        }

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(icon(Icon::Key).color(t.accent))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).text_color(t.text).child(key.name.clone()))
                    .child(div().text_size(px(11.)).text_color(t.text_muted).child(meta)),
            )
            // 개인 키 파일이 없는 키는 -i로 쓰이지 못한다 — 눈에 띄게 표시.
            .when(!view.has_private_file, |el| {
                el.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(t.danger)
                        .text_size(px(10.))
                        .text_color(t.danger)
                        .child(tr(lang, TrKey::KeysMissingBadge)),
                )
            })
            .child({
                let key_for_edit = key.clone();
                div()
                    .id(("key-edit", id as usize))
                    .cursor_pointer()
                    .child(icon(Icon::Pencil))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_dialog(DialogMode::Edit(key_for_edit.clone()), window, cx);
                    }))
            })
            .child({
                let key_for_delete = key.clone();
                div()
                    .id(("key-delete", id as usize))
                    .cursor_pointer()
                    .child(icon(Icon::Trash))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.ask_delete(&key_for_delete, window, cx);
                    }))
            });

        let public_panel = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(8.))
            .rounded(px(6.))
            .bg(t.bg)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(10.))
                            .text_color(t.text_muted)
                            .child(tr(lang, TrKey::KeysPublicKey)),
                    )
                    .child(
                        div()
                            .id(("key-reveal", id as usize))
                            .cursor_pointer()
                            .child(icon(if revealed { Icon::EyeOff } else { Icon::Eye }))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                if !this.revealed.remove(&id) {
                                    this.revealed.insert(id);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id(("key-copy", id as usize))
                            .cursor_pointer()
                            .child(icon(Icon::Copy))
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    public_key.clone(),
                                ));
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(if revealed {
                        key.public_key.clone()
                    } else {
                        MASKED_PUBLIC_KEY.into()
                    }),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border_subtle)
            .bg(t.surface)
            .child(header)
            .child(public_panel)
    }
}

impl Render for KeyManagerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let keys = self.state.read(cx).keys.clone();

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
                            .child(tr(lang, TrKey::NavKeys)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(tr_with(
                                lang,
                                TrKey::KeysSubtitle,
                                &[("n", &keys.len().to_string())],
                            )),
                    ),
            )
            .child(
                Button::new("keys-create", tr(lang, TrKey::KeysCreate))
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_dialog(DialogMode::Create, window, cx);
                    })),
            )
            .child(
                Button::new("keys-import", tr(lang, TrKey::KeysImport)).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.open_dialog(DialogMode::Import, window, cx);
                    },
                )),
            );

        let body = if keys.is_empty() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .py(px(48.))
                .child(icon(Icon::Key).size(px(28.)))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(t.text_muted)
                        .child(tr(lang, TrKey::KeysEmpty)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.))
                        .child(
                            Button::new("keys-empty-create", tr(lang, TrKey::KeysCreate))
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_dialog(DialogMode::Create, window, cx);
                                })),
                        )
                        .child(
                            Button::new("keys-empty-import", tr(lang, TrKey::KeysImport)).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_dialog(DialogMode::Import, window, cx);
                                }),
                            ),
                        ),
                )
                .into_any_element()
        } else {
            let cards: Vec<_> = keys.iter().map(|k| self.card(k, lang, cx)).collect();
            div()
                .id("key-cards")
                .flex()
                .flex_col()
                .gap(px(8.))
                .overflow_y_scroll()
                .children(cards)
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
            .child(div().flex_1().min_h(px(0.)).child(body))
            .when_some(self.dialog.clone(), |el, dialog| {
                el.child(ModalOverlay::new(dialog))
            })
            .when_some(self.confirm.clone(), |el, dialog| {
                el.child(ModalOverlay::new(dialog))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_key_type_from_public_key_prefix() {
        assert_eq!(detect_key_type("ssh-ed25519 AAAA"), Some(KeyType::Ed25519));
        assert_eq!(detect_key_type("ssh-rsa AAAA"), Some(KeyType::Rsa));
        assert_eq!(detect_key_type("ecdsa-sha2-nistp256 AAAA"), Some(KeyType::Ecdsa));
        assert_eq!(detect_key_type("ssh-dss AAAA"), Some(KeyType::Dsa));
        assert_eq!(detect_key_type("garbage"), None);
        assert_eq!(detect_key_type(""), None);
    }

    #[test]
    fn key_type_labels_match_original() {
        assert_eq!(key_type_label(KeyType::Ed25519), "Ed25519");
        assert_eq!(key_type_label(KeyType::Rsa), "RSA");
        assert_eq!(key_type_label(KeyType::Ecdsa), "ECDSA");
        assert_eq!(key_type_label(KeyType::Dsa), "DSA");
    }

    #[test]
    fn non_empty_maps_blank_to_none() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("x"), Some("x".to_string()));
        // 원본은 공백만 있는 패스프레이즈도 그대로 보낸다 (`||` 검사).
        assert_eq!(non_empty(" "), Some(" ".to_string()));
    }
}
