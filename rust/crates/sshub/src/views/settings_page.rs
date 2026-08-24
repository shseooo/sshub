//! 설정 (Electron `src/pages/Settings.tsx`).
//!
//! export/import/config sync는 전부 `AppState::spawn_core` 경유 — scrypt와
//! 파일 I/O가 초 단위로 걸릴 수 있다.
//!
//! 폐기된 것(DESIGN-ui.md §4): CRT 배경 톤 프리셋. 어센트는 새 테마의
//! 프리셋 4종 + 커스텀 hex로 유지한다.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, IntoElement, PathPromptOptions,
    Subscription, Window,
};
use sshub_core::backup::{self, ExportOptions};
use sshub_core::settings::Settings;
use sshub_core::{ssh_config, CoreError};

use crate::i18n::{tr, tr_with, Lang, TrKey};
use crate::keymap;
use crate::state::{app_state, AppState};
use crate::theme::{theme, Theme, ACCENT_PRESETS};
use crate::ui::icon::{icon, Icon};
use crate::ui::select::SelectOption;
use crate::ui::text_input::{InputEvent, TextInput};
use crate::ui::{Button, Checkbox, FormField, Select, SelectEvent};
use crate::views::{current_lang, ViewEvent};

const FONT_SIZE_MIN: f32 = 10.0;
const FONT_SIZE_MAX: f32 = 24.0;
const TRANSLUCENCY_MAX: u8 = 40;

/// `#rrggbb` / `rrggbb` → 0xRRGGBB.
pub fn parse_hex_color(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// 액션 이름 → 라벨 키.
fn shortcut_label(action: &str) -> TrKey {
    match action {
        "closePane" => TrKey::ShortcutClosePane,
        "splitRight" => TrKey::ShortcutSplitRight,
        "splitDown" => TrKey::ShortcutSplitDown,
        "broadcast" => TrKey::ShortcutBroadcast,
        "fontIncrease" => TrKey::ShortcutFontIncrease,
        "fontDecrease" => TrKey::ShortcutFontDecrease,
        "focusLeft" => TrKey::ShortcutFocusLeft,
        "focusRight" => TrKey::ShortcutFocusRight,
        "focusUp" => TrKey::ShortcutFocusUp,
        "focusDown" => TrKey::ShortcutFocusDown,
        _ => TrKey::ShortcutNewTab,
    }
}

/// 수식어만 눌린 키스트로크는 리바인딩 대상이 아니다 (원본 `isModifierOnly`).
fn is_modifier_only(key: &str) -> bool {
    matches!(key, "cmd" | "ctrl" | "alt" | "shift" | "fn" | "function" | "super" | "win")
}

/// 설정값을 전역 테마에 반영한다 — 어센트·반투명·터미널 색을 즉시 적용.
fn apply_theme(settings: &Settings, cx: &mut App) {
    let accent = parse_hex_color(&settings.appearance.accent).unwrap_or(0x74ade8);
    let term_fg = settings
        .appearance
        .terminal
        .foreground
        .as_deref()
        .and_then(parse_hex_color);
    let term_bg = settings
        .appearance
        .terminal
        .background
        .as_deref()
        .and_then(parse_hex_color);
    cx.set_global(Theme::with_overrides(
        accent,
        settings.appearance.translucency,
        term_fg,
        term_bg,
        settings.appearance.terminal.font_size,
        crate::fonts::resolve_family(
            settings.appearance.terminal.font_family.as_deref(),
            true,
        ),
    ));
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PassphraseMode {
    Export,
    Import,
}

pub struct SettingsView {
    state: Entity<AppState>,
    start_page: Entity<Select>,
    language: Entity<Select>,
    accent_input: Entity<TextInput>,
    passphrase: Entity<TextInput>,
    /// 카드 하단 인라인 메시지 (원본과 동일하게 단일 슬롯).
    message: Option<String>,
    syncing_to: bool,
    syncing_from: bool,
    /// 내보내기 선택 모달 — `Some(encrypted)`.
    export_select: Option<bool>,
    selected_servers: HashSet<i64>,
    selected_keys: HashSet<i64>,
    include_shortcuts: bool,
    passphrase_modal: Option<(PassphraseMode, PathBuf)>,
    /// 리바인딩 대기 중인 액션 이름.
    capturing: Option<String>,
    /// 충돌한 상대 액션 (캡처 중 표시).
    conflict: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ViewEvent> for SettingsView {}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = app_state(cx);
        let lang = current_lang(cx);
        let settings = state.read(cx).settings.clone();

        let start_options: Vec<SelectOption> = sshub_core::settings::START_PAGES
            .iter()
            .map(|page| {
                let label = match *page {
                    "servers" => TrKey::NavServers,
                    "terminal" => TrKey::NavTerminal,
                    "keys" => TrKey::NavKeys,
                    "settings" => TrKey::NavSettings,
                    _ => TrKey::NavDashboard,
                };
                SelectOption::new(*page, tr(lang, label))
            })
            .collect();
        let start_ix = start_options
            .iter()
            .position(|o| o.value.as_ref() == settings.start_page)
            .unwrap_or(0);
        let start_page = cx.new(|cx| {
            Select::new("start-page", start_options, cx).with_selected_ix(Some(start_ix))
        });

        // 언어 라벨은 번역하지 않는다 — 각 언어의 고유 표기.
        let language_options = vec![
            SelectOption::new("ko", "한국어"),
            SelectOption::new("en", "English"),
            SelectOption::new("ja", "日本語"),
        ];
        let language_ix = settings
            .language
            .as_deref()
            .and_then(|code| language_options.iter().position(|o| o.value.as_ref() == code))
            .unwrap_or_else(|| match Lang::detect() {
                Lang::Ko => 0,
                Lang::En => 1,
                Lang::Ja => 2,
            });
        let language = cx.new(|cx| {
            Select::new("language", language_options, cx).with_selected_ix(Some(language_ix))
        });

        let accent_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_text(settings.appearance.accent.clone())
                .with_placeholder(tr(lang, TrKey::SettingsCustom))
        });
        let passphrase = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_masked(true)
                .with_placeholder(tr(lang, TrKey::SettingsPassphrase))
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(
            &start_page,
            |this: &mut Self, select, event: &SelectEvent, cx| {
                let SelectEvent::Changed(ix) = event;
                let Some(value) = select.read(cx).options().get(*ix).map(|o| o.value.to_string())
                else {
                    return;
                };
                this.state.update(cx, |state, cx| {
                    state.update_settings(|s| s.start_page = value.clone(), cx);
                });
            },
        ));
        subscriptions.push(cx.subscribe(
            &language,
            |this: &mut Self, select, event: &SelectEvent, cx| {
                let SelectEvent::Changed(ix) = event;
                let Some(value) = select.read(cx).options().get(*ix).map(|o| o.value.to_string())
                else {
                    return;
                };
                this.state.update(cx, |state, cx| {
                    state.update_settings(|s| s.language = Some(value.clone()), cx);
                });
                cx.notify();
            },
        ));
        subscriptions.push(cx.subscribe(
            &accent_input,
            |this: &mut Self, input, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Changed) {
                    return;
                }
                // 유효한 hex일 때만 적용 — 타이핑 중간값으로 테마가 튀지 않게.
                let text = input.read(cx).text().to_string();
                if parse_hex_color(&text).is_some() {
                    this.set_accent(text, cx);
                }
            },
        ));

        Self {
            state,
            start_page,
            language,
            accent_input,
            passphrase,
            message: None,
            syncing_to: false,
            syncing_from: false,
            export_select: None,
            selected_servers: HashSet::new(),
            selected_keys: HashSet::new(),
            include_shortcuts: true,
            passphrase_modal: None,
            capturing: None,
            conflict: None,
            _subscriptions: subscriptions,
        }
    }

    fn set_accent(&mut self, accent: String, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.update_settings(|s| s.appearance.accent = accent.clone(), cx);
        });
        let settings = self.state.read(cx).settings.clone();
        apply_theme(&settings, cx);
        cx.notify();
    }

    fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.update_settings(
                |s| {
                    s.appearance.terminal.font_size =
                        (s.appearance.terminal.font_size + delta).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
                },
                cx,
            );
        });
        let settings = self.state.read(cx).settings.clone();
        apply_theme(&settings, cx);
        cx.notify();
    }

    fn adjust_translucency(&mut self, delta: i16, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.update_settings(
                |s| {
                    let next = i16::from(s.appearance.translucency) + delta;
                    s.appearance.translucency = next.clamp(0, i16::from(TRANSLUCENCY_MAX)) as u8;
                },
                cx,
            );
        });
        let settings = self.state.read(cx).settings.clone();
        apply_theme(&settings, cx);
        cx.notify();
    }

    // -- SSH config 동기화 ---------------------------------------------------

    fn sync_to_config(&mut self, cx: &mut Context<Self>) {
        self.syncing_to = true;
        cx.notify();
        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| ssh_config::sync_servers_to_config(&core.store))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                view.syncing_to = false;
                let lang = current_lang(cx);
                view.message = Some(match result {
                    Ok(()) => tr(lang, TrKey::SettingsSyncToDone).to_string(),
                    Err(err) => tr_with(
                        lang,
                        TrKey::SettingsSyncToFail,
                        &[("err", &err.to_string())],
                    ),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn sync_from_config(&mut self, cx: &mut Context<Self>) {
        self.syncing_from = true;
        cx.notify();
        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| {
                ssh_config::sync_config_to_servers(&mut core.store)
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                view.syncing_from = false;
                let lang = current_lang(cx);
                view.message = Some(match result {
                    Ok(imported) if imported.is_empty() => {
                        tr(lang, TrKey::SettingsImportFromConfigNone).to_string()
                    }
                    Ok(imported) => {
                        let names: Vec<&str> =
                            imported.iter().map(|s| s.name.as_str()).collect();
                        tr_with(
                            lang,
                            TrKey::SettingsImportFromConfigDone,
                            &[
                                ("n", &imported.len().to_string()),
                                ("names", &names.join(", ")),
                            ],
                        )
                    }
                    Err(err) => tr_with(
                        lang,
                        TrKey::SettingsImportFail,
                        &[("err", &err.to_string())],
                    ),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -- 내보내기 -----------------------------------------------------------

    fn open_export_select(&mut self, encrypted: bool, cx: &mut Context<Self>) {
        self.message = None;
        let state = self.state.read(cx);
        // 원본과 동일하게 열 때마다 전부 선택된 상태로 초기화한다.
        self.selected_servers = state.servers.iter().map(|s| s.id).collect();
        self.selected_keys = state.keys.iter().map(|k| k.key.id).collect();
        self.include_shortcuts = true;
        self.export_select = Some(encrypted);
        cx.notify();
    }

    fn confirm_export_select(&mut self, cx: &mut Context<Self>) {
        let Some(encrypted) = self.export_select.take() else { return };
        cx.notify();
        let directory = home_dir();
        let suggested = if encrypted { "sshub-export.enc" } else { "sshub-export.json" };
        // NOTE(gpui 0.2.2): 저장 다이얼로그에 확장자 필터 옵션이 없다.
        let receiver = cx.prompt_for_new_path(&directory, Some(suggested));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else { return };
            this.update(cx, |view, cx| {
                if encrypted {
                    view.passphrase
                        .update(cx, |input, cx| input.set_text("", cx));
                    view.passphrase_modal = Some((PassphraseMode::Export, path));
                    cx.notify();
                } else {
                    view.run_export(path, None, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn run_export(&mut self, path: PathBuf, passphrase: Option<String>, cx: &mut Context<Self>) {
        let encrypted = passphrase.is_some();
        let shortcuts: Option<BTreeMap<String, String>> = if self.include_shortcuts {
            Some(self.state.read(cx).settings.shortcuts.clone())
        } else {
            None
        };
        let server_ids: Vec<i64> = self.selected_servers.iter().copied().collect();
        let key_ids: Vec<i64> = self.selected_keys.iter().copied().collect();
        let display = path.display().to_string();
        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| {
                backup::export_data(
                    &core.store,
                    &core.keys_dir,
                    &path,
                    &ExportOptions {
                        passphrase,
                        shortcuts,
                        server_ids: Some(server_ids),
                        key_ids: Some(key_ids),
                    },
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                let lang = current_lang(cx);
                view.message = Some(match result {
                    Ok(()) if encrypted => tr_with(
                        lang,
                        TrKey::SettingsExportEncryptedDone,
                        &[("path", &display)],
                    ),
                    Ok(()) => tr_with(lang, TrKey::SettingsExportDone, &[("path", &display)]),
                    Err(err) => tr_with(
                        lang,
                        TrKey::SettingsExportFail,
                        &[("err", &err.to_string())],
                    ),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -- 가져오기 -----------------------------------------------------------

    fn start_import(&mut self, cx: &mut Context<Self>) {
        self.message = None;
        let lang = current_lang(cx);
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(lang, TrKey::SettingsImportDialogTitle).into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update(cx, |view, cx| view.run_import(path, None, cx)).ok();
        })
        .detach();
    }

    fn run_import(&mut self, path: PathBuf, passphrase: Option<String>, cx: &mut Context<Self>) {
        let retry_path = path.clone();
        let had_passphrase = passphrase.is_some();
        let task = self.state.update(cx, |state, cx| {
            state.spawn_core(cx, move |core| {
                backup::import_data(
                    &mut core.store,
                    &core.keys_dir,
                    &path,
                    passphrase.as_deref(),
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                let lang = current_lang(cx);
                match result {
                    Ok(summary) => {
                        view.message = Some(tr_with(
                            lang,
                            TrKey::SettingsImportDone,
                            &[
                                ("sa", &summary.servers_added.to_string()),
                                ("ss", &summary.servers_skipped.to_string()),
                                ("ka", &summary.keys_added.to_string()),
                                ("ks", &summary.keys_skipped.to_string()),
                            ],
                        ));
                        if let Some(shortcuts) = summary.shortcuts {
                            view.replace_shortcuts(shortcuts, cx);
                        }
                    }
                    // "ENCRYPTED" 센티널 — 암호가 필요하다는 뜻이니 물어보고 1회 재시도.
                    // 여기서는 문자열이 아니라 타입으로 판별한다.
                    Err(CoreError::NeedsPassphrase) if !had_passphrase => {
                        view.passphrase
                            .update(cx, |input, cx| input.set_text("", cx));
                        view.passphrase_modal = Some((PassphraseMode::Import, retry_path));
                    }
                    Err(err) => {
                        view.message = Some(tr_with(
                            lang,
                            TrKey::SettingsImportFail,
                            &[("err", &err.to_string())],
                        ));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn submit_passphrase(&mut self, cx: &mut Context<Self>) {
        let passphrase = self.passphrase.read(cx).text().to_string();
        if passphrase.is_empty() {
            return;
        }
        let Some((mode, path)) = self.passphrase_modal.take() else { return };
        match mode {
            PassphraseMode::Export => self.run_export(path, Some(passphrase), cx),
            PassphraseMode::Import => self.run_import(path, Some(passphrase), cx),
        }
        self.passphrase.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    // -- 단축키 -------------------------------------------------------------

    fn replace_shortcuts(&mut self, shortcuts: BTreeMap<String, String>, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.update_settings(
                |s| {
                    for (action, combo) in &shortcuts {
                        // 백업 번들에는 Electron 시절 표기가 들어 있을 수 있다.
                        let normalized = keymap::canonicalize_combo(combo)
                            .or_else(|| keymap::combo_from_legacy(combo));
                        if let Some(combo) = normalized {
                            if keymap::ACTION_NAMES.contains(&action.as_str()) {
                                s.shortcuts.insert(action.clone(), combo);
                            }
                        }
                    }
                },
                cx,
            );
        });
        self.rebind(cx);
    }

    /// gpui의 `clear_key_bindings`는 위젯 바인딩까지 지우므로 반드시
    /// clear → `ui::init` → `keymap::register_all` 순서로 재등록한다.
    fn rebind(&mut self, cx: &mut Context<Self>) {
        let shortcuts = self.state.read(cx).settings.shortcuts.clone();
        cx.clear_key_bindings();
        crate::ui::init(cx);
        keymap::register_all(cx, &shortcuts);
    }

    fn capture_keystroke(&mut self, key: &str, combo: String, cx: &mut Context<Self>) {
        let Some(action) = self.capturing.clone() else { return };
        if key == "escape" {
            self.capturing = None;
            self.conflict = None;
            cx.notify();
            return;
        }
        if is_modifier_only(key) {
            return;
        }
        let Some(combo) = keymap::canonicalize_combo(&combo) else { return };
        // 같은 조합이 두 액션에 걸리면 하나가 영영 안 먹는다 — 저장 전에 막는다.
        let conflict = self
            .state
            .read(cx)
            .settings
            .shortcuts
            .iter()
            .find(|(other, existing)| other.as_str() != action && *existing == &combo)
            .map(|(other, _)| other.clone());
        if let Some(other) = conflict {
            self.conflict = Some(other);
            cx.notify();
            return;
        }
        self.state.update(cx, |state, cx| {
            state.update_settings(|s| {
                s.shortcuts.insert(action.clone(), combo.clone());
            }, cx);
        });
        self.capturing = None;
        self.conflict = None;
        self.rebind(cx);
        cx.notify();
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

// ---------------------------------------------------------------------------
// 렌더
// ---------------------------------------------------------------------------

impl SettingsView {
    fn section(&self, title: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(t.text_muted)
                    .child(title),
            )
            .child(div().flex_1().h(px(1.)).bg(t.border_subtle))
    }

    fn card(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = theme(cx).clone();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border_subtle)
            .bg(t.surface)
    }

    fn labeled_row(
        &self,
        title: &'static str,
        description: &'static str,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        div()
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
                    .child(div().text_size(px(12.)).text_color(t.text).child(title))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(description),
                    ),
            )
            .child(control)
    }

    fn stepper(
        &self,
        id: &'static str,
        value: String,
        on_minus: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        on_plus: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        // 위젯 킷에 슬라이더가 없어 스테퍼로 대체한다 (범위는 동일).
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(
                Button::new((id, 0u64), "−")
                    .on_click(cx.listener(move |this, _, _window, cx| on_minus(this, cx))),
            )
            .child(
                div()
                    .w(px(44.))
                    .text_size(px(12.))
                    .text_color(t.text)
                    .child(value),
            )
            .child(
                Button::new((id, 1u64), "+")
                    .on_click(cx.listener(move |this, _, _window, cx| on_plus(this, cx))),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let lang = current_lang(cx);
        let settings = self.state.read(cx).settings.clone();
        let paths = self.state.read(cx).paths.clone();

        // --- SSH Config Sync ---
        let sync_card = self
            .card(cx)
            .child(self.section("SSH Config Sync", cx))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(tr(lang, TrKey::SettingsSyncDesc)),
            )
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsToConfigTitle),
                tr(lang, TrKey::SettingsToConfigDesc),
                Button::new(
                    "sync-to-config",
                    if self.syncing_to {
                        tr(lang, TrKey::SettingsSyncing)
                    } else {
                        tr(lang, TrKey::SettingsSync)
                    },
                )
                .disabled(self.syncing_to)
                .on_click(cx.listener(|this, _, _window, cx| this.sync_to_config(cx))),
                cx,
            ))
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsFromConfigTitle),
                tr(lang, TrKey::SettingsFromConfigDesc),
                Button::new(
                    "sync-from-config",
                    if self.syncing_from {
                        tr(lang, TrKey::CommonImporting)
                    } else {
                        tr(lang, TrKey::CommonImport)
                    },
                )
                .disabled(self.syncing_from)
                .on_click(cx.listener(|this, _, _window, cx| this.sync_from_config(cx))),
                cx,
            ))
            .when_some(self.message.clone(), |el, message| {
                el.child(
                    div()
                        .p(px(8.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .text_size(px(11.))
                        .text_color(t.text)
                        .child(message),
                )
            });

        // --- Backup ---
        let backup_card = self
            .card(cx)
            .child(self.section("Backup", cx))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(tr(lang, TrKey::SettingsBackupDesc)),
            )
            .child(self.labeled_row(
                tr(lang, TrKey::CommonExport),
                tr(lang, TrKey::SettingsExportDesc),
                Button::new("export-plain", tr(lang, TrKey::CommonExport)).on_click(cx.listener(
                    |this, _, _window, cx| this.open_export_select(false, cx),
                )),
                cx,
            ))
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsExportWithKeys),
                tr(lang, TrKey::SettingsExportWithKeysDesc),
                Button::new("export-keys", tr(lang, TrKey::SettingsExportWithKeys)).on_click(
                    cx.listener(|this, _, _window, cx| this.open_export_select(true, cx)),
                ),
                cx,
            ))
            .child(self.labeled_row(
                tr(lang, TrKey::CommonImport),
                tr(lang, TrKey::SettingsImportDesc),
                Button::new("import-backup", tr(lang, TrKey::CommonImport))
                    .on_click(cx.listener(|this, _, _window, cx| this.start_import(cx))),
                cx,
            ));

        // --- General ---
        let general_card = self
            .card(cx)
            .child(self.section("General", cx))
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsStartMenu),
                tr(lang, TrKey::SettingsStartMenuDesc),
                div().w(px(176.)).child(self.start_page.clone()),
                cx,
            ))
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsLanguage),
                tr(lang, TrKey::SettingsLanguageDesc),
                div().w(px(176.)).child(self.language.clone()),
                cx,
            ));

        // --- Appearance ---
        let current_accent = settings.appearance.accent.to_lowercase();
        // 지연 이터레이터로 두면 cx 빌림이 렌더 끝까지 살아 있어 컴파일되지 않는다.
        let swatches: Vec<_> = ACCENT_PRESETS
            .iter()
            .map(|(name, value)| {
                let hex = format!("#{value:06x}");
                let selected = current_accent == hex;
                div()
                    .id(gpui::SharedString::new_static(name))
                    .size(px(20.))
                    .rounded(px(10.))
                    .border_2()
                    .border_color(if selected { t.text } else { t.border })
                    .bg(gpui::Hsla::from(gpui::rgb(*value)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        let hex = hex.clone();
                        this.accent_input
                            .update(cx, |input, cx| input.set_text(hex.clone(), cx));
                        this.set_accent(hex, cx);
                    }))
            })
            .collect();

        let appearance_card = self
            .card(cx)
            .child(self.section("Appearance", cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(t.text)
                            .child(tr(lang, TrKey::SettingsAccent)),
                    )
                    .children(swatches)
                    .child(div().w(px(96.)).child(self.accent_input.clone())),
            )
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsTermFontSize),
                tr(lang, TrKey::SettingsTermColors),
                self.stepper(
                    "font-size",
                    format!("{}px", settings.appearance.terminal.font_size as i32),
                    |this, cx| this.adjust_font_size(-1.0, cx),
                    |this, cx| this.adjust_font_size(1.0, cx),
                    cx,
                ),
                cx,
            ))
            .child(self.labeled_row(
                tr(lang, TrKey::SettingsUiOpacity),
                tr(lang, TrKey::SettingsPhosphorDesc),
                self.stepper(
                    "translucency",
                    format!("{}%", settings.appearance.translucency),
                    |this, cx| this.adjust_translucency(-5, cx),
                    |this, cx| this.adjust_translucency(5, cx),
                    cx,
                ),
                cx,
            ));

        // --- Shortcuts ---
        let capturing = self.capturing.clone();
        let conflict = self.conflict.clone();
        let shortcut_rows: Vec<_> = keymap::ACTION_NAMES
            .iter()
            .map(|action| {
                let action = *action;
                let combo = settings.shortcuts.get(action).cloned().unwrap_or_default();
                let is_capturing = capturing.as_deref() == Some(action);
                let label = tr(lang, shortcut_label(action));
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(t.text)
                            .child(label),
                    )
                    .when(is_capturing, |el| {
                        el.when_some(conflict.clone(), |el, other| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.danger)
                                    .child(tr(lang, shortcut_label(&other))),
                            )
                        })
                    })
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("shortcut-{action}")))
                            .key_context("ShortcutCapture")
                            .track_focus(&cx.focus_handle())
                            .min_w(px(84.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if is_capturing { t.accent } else { t.border })
                            .bg(if is_capturing { t.accent_wash } else { t.elevated })
                            .text_size(px(12.))
                            .text_color(t.text)
                            .cursor_pointer()
                            .child(if is_capturing {
                                tr(lang, TrKey::SettingsPressKeys).to_string()
                            } else {
                                keymap::display_combo(&combo)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.capturing = Some(action.to_string());
                                this.conflict = None;
                                window.focus(&cx.focus_handle());
                                cx.notify();
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, _window, cx| {
                                    if this.capturing.is_none() {
                                        return;
                                    }
                                    cx.stop_propagation();
                                    let key = event.keystroke.key.clone();
                                    let combo = event.keystroke.unparse();
                                    this.capture_keystroke(&key, combo, cx);
                                },
                            )),
                    )
            })
            .collect();

        let shortcuts_card = self
            .card(cx)
            .child(self.section("Shortcuts", cx))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(tr(lang, TrKey::SettingsShortcutsDesc)),
            )
            .children(shortcut_rows);

        // --- System Info (원본과 동일하게 하드코딩 라벨) ---
        let info_line = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .text_size(px(11.))
                .child(div().w(px(36.)).text_color(t.accent).child(label))
                .child(div().flex_1().text_color(t.text_muted).child(value))
        };
        let info_card = self
            .card(cx)
            .child(self.section("System Info", cx))
            .child(info_line("data", paths.store_file.display().to_string()))
            .child(info_line("keys", paths.keys_dir.display().to_string()));

        let content = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .max_w(px(720.))
            .child(sync_card)
            .child(backup_card)
            .child(general_card)
            .child(appearance_card)
            .child(shortcuts_card)
            .child(info_card);

        let mut root = div()
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
                    .child(tr(lang, TrKey::NavSettings)),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(content),
            );

        if let Some(encrypted) = self.export_select {
            root = root.child(self.render_export_select(encrypted, lang, cx));
        }
        if let Some((mode, _)) = self.passphrase_modal.clone() {
            root = root.child(self.render_passphrase_modal(mode, lang, cx));
        }
        root
    }
}

impl SettingsView {
    fn overlay(&self, panel: impl IntoElement, cx: &mut Context<Self>) -> impl IntoElement {
        let backdrop = crate::theme::with_alpha(gpui::black(), 0.45);
        let _ = theme(cx);
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop)
            .child(panel)
    }

    fn render_export_select(
        &self,
        _encrypted: bool,
        lang: Lang,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        let state = self.state.read(cx);
        let servers = state.servers.clone();
        let keys = state.keys.clone();
        let server_total = servers.len();
        let key_total = keys.len();

        let server_rows: Vec<_> = servers
            .iter()
            .map(|s| {
                let id = s.id;
                let checked = self.selected_servers.contains(&id);
                Checkbox::new(("export-server", id as usize), checked)
                    .label(format!("{}  {}@{}", s.name, s.username, s.host))
                    .on_toggle(cx.listener(move |this, _: &bool, _window, cx| {
                        if !this.selected_servers.remove(&id) {
                            this.selected_servers.insert(id);
                        }
                        cx.notify();
                    }))
            })
            .collect();
        let key_rows: Vec<_> = keys
            .iter()
            .map(|k| {
                let id = k.key.id;
                let checked = self.selected_keys.contains(&id);
                Checkbox::new(("export-key", id as usize), checked)
                    .label(format!("{} ({})", k.key.name, k.key.key_type.as_str()))
                    .on_toggle(cx.listener(move |this, _: &bool, _window, cx| {
                        if !this.selected_keys.remove(&id) {
                            this.selected_keys.insert(id);
                        }
                        cx.notify();
                    }))
            })
            .collect();

        let group_header = |text: String| {
            div()
                .text_size(px(10.))
                .text_color(t.text_muted)
                .child(text)
        };

        let export_disabled = self.selected_servers.is_empty()
            && self.selected_keys.is_empty()
            && !self.include_shortcuts;

        let panel = div()
            .occlude()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(440.))
            .max_h(px(560.))
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
                            .child(tr(lang, TrKey::SettingsExportSelectTitle)),
                    )
                    .child(
                        div()
                            .id("export-close")
                            .cursor_pointer()
                            .child(icon(Icon::Close))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.export_select = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex().flex_row().justify_end().child(
                    Button::new("export-select-all", tr(lang, TrKey::SettingsSelectAll)).on_click(
                        cx.listener(move |this, _, _window, cx| {
                            // 둘 다 전부 선택된 상태면 해제, 아니면 전부 선택.
                            let all = this.selected_servers.len() == server_total
                                && this.selected_keys.len() == key_total;
                            if all {
                                this.selected_servers.clear();
                                this.selected_keys.clear();
                            } else {
                                let state = this.state.read(cx);
                                this.selected_servers =
                                    state.servers.iter().map(|s| s.id).collect();
                                this.selected_keys =
                                    state.keys.iter().map(|k| k.key.id).collect();
                            }
                            cx.notify();
                        }),
                    ),
                ),
            )
            .child(
                div()
                    .id("export-selection")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(group_header(format!(
                        "{} ({}/{})",
                        tr(lang, TrKey::NavServers),
                        self.selected_servers.len(),
                        server_total
                    )))
                    .children(server_rows)
                    .child(group_header(format!(
                        "SSH Keys ({}/{})",
                        self.selected_keys.len(),
                        key_total
                    )))
                    .children(key_rows),
            )
            .child(
                Checkbox::new("include-shortcuts", self.include_shortcuts)
                    .label(tr(lang, TrKey::SettingsIncludeShortcuts))
                    .on_toggle(cx.listener(|this, checked: &bool, _window, cx| {
                        this.include_shortcuts = *checked;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("export-cancel", tr(lang, TrKey::CommonCancel)).on_click(
                            cx.listener(|this, _, _window, cx| {
                                this.export_select = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        Button::new("export-confirm", tr(lang, TrKey::CommonExport))
                            .primary()
                            .disabled(export_disabled)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.confirm_export_select(cx)
                            })),
                    ),
            );

        self.overlay(panel, cx)
    }

    fn render_passphrase_modal(
        &self,
        mode: PassphraseMode,
        lang: Lang,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx).clone();
        let hint = match mode {
            PassphraseMode::Export => tr(lang, TrKey::SettingsPassphraseExportHint),
            PassphraseMode::Import => tr(lang, TrKey::SettingsPassphraseImportHint),
        };
        let confirm_label = match mode {
            PassphraseMode::Export => tr(lang, TrKey::CommonExport),
            PassphraseMode::Import => tr(lang, TrKey::SettingsDecrypt),
        };

        let panel = div()
            .occlude()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(360.))
            .p(px(16.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(t.text)
                    .child(tr(lang, TrKey::SettingsPassphrase)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(hint),
            )
            .child(FormField::bare(self.passphrase.clone()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("pph-cancel", tr(lang, TrKey::CommonCancel)).on_click(
                            cx.listener(|this, _, _window, cx| {
                                this.passphrase_modal = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        Button::new("pph-confirm", confirm_label)
                            .primary()
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.submit_passphrase(cx)
                            })),
                    ),
            );

        self.overlay(panel, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_hash() {
        assert_eq!(parse_hex_color("#74ade8"), Some(0x74ade8));
        assert_eq!(parse_hex_color("74ade8"), Some(0x74ade8));
        assert_eq!(parse_hex_color("  #A1C181 "), Some(0xa1c181));
    }

    #[test]
    fn rejects_malformed_hex() {
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color("#74ade80"), None);
    }

    #[test]
    fn every_action_has_a_distinct_label() {
        let labels: Vec<TrKey> = keymap::ACTION_NAMES.iter().map(|a| shortcut_label(a)).collect();
        assert_eq!(labels.len(), 11);
        for (i, a) in labels.iter().enumerate() {
            for b in labels.iter().skip(i + 1) {
                assert_ne!(a, b, "단축키 라벨이 중복되면 어떤 행인지 알 수 없다");
            }
        }
    }

    #[test]
    fn modifier_only_keys_are_ignored_during_capture() {
        assert!(is_modifier_only("cmd"));
        assert!(is_modifier_only("shift"));
        assert!(!is_modifier_only("t"));
        assert!(!is_modifier_only("escape"));
    }
}
