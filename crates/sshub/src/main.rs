//! sshub — GPUI 앱 진입점 (rust/docs/DESIGN-ui.md §3, DESIGN-terminal.md §8).
//!
//! 부트스트랩 순서가 곧 의존 순서다:
//! 경로 → 전역 상태 → 테마 → 위젯 키맵 → 앱 키맵 → 세션 레지스트리 →
//! 창 매니저 → 창 복원.
//!
//! 키맵은 `ui::init` → `keymap::register_all` 순서를 반드시 지킨다.
//! `clear_key_bindings()`가 전역 키맵을 통째로 비우므로 위젯 바인딩이 먼저
//! 깔려 있어야 리바인드 후에도 살아남는다 (DESIGN-ui.md §9).

use gpui::{actions, App, Application, KeyBinding, Menu, MenuItem};
use sshub_core::settings::Settings;
use sshub_core::window_state::load_window_bounds;
use sshub_core::AppPaths;

use sshub::state::app_state;
use sshub::theme::Theme;
use sshub::views::settings_page::parse_hex_color;
use sshub::window_session::{restore_windows, WindowRecord, DEFAULT_BOUNDS};
use sshub::workspace::{self, MoveTabToNewWindow};
use sshub::{fonts, keymap, session_registry, state, theme, ui, window_manager};

actions!(sshub_app, [Quit]);

fn main() {
    let paths = match AppPaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            // 데이터 디렉터리가 없으면 무엇도 할 수 없다 — 조용히 죽지 않는다.
            eprintln!("sshub: 데이터 디렉터리를 찾을 수 없습니다: {error}");
            std::process::exit(1);
        }
    };

    let app = Application::new();

    // macOS Dock 재활성화. `on_reopen`은 `Application`에만 있고 `run`이 self를
    // 소비하므로 반드시 run 이전에 등록해야 한다.
    app.on_reopen(|cx: &mut App| {
        if cx.windows().is_empty() {
            let bounds = last_known_bounds(cx);
            if let Err(error) = workspace::open(None, bounds, cx) {
                eprintln!("sshub: 창을 다시 열지 못했습니다: {error}");
            }
        }
    });

    app.run(move |cx: &mut App| {
        let app_state_entity = state::init(cx);
        let settings: Settings = app_state_entity.read(cx).settings.clone();

        // 테마가 폰트 패밀리를 들고 있으므로 등록이 먼저다.
        if fonts::register(cx) {
            cx.set_global(EmbeddedFontOk);
        }
        theme::init(cx);
        apply_appearance(&settings, cx);

        // 위젯 바인딩이 먼저, 그 다음 앱 단축키.
        ui::init(cx);
        keymap::register_all(cx, &settings.shortcuts);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-shift-n", MoveTabToNewWindow, Some("Workspace")),
        ]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

        session_registry::init(&paths, cx);
        {
            let state = app_state_entity.read(cx);
            let (servers, keys) = (state.servers.clone(), state.keys.clone());
            session_registry::registry(cx)
                .update(cx, |registry, _| registry.set_catalog(servers, keys));
        }

        window_manager::init(cx);
        set_menus(cx);
        install_quit_hook(cx);

        // 마지막 창을 닫아도 앱은 살아 있다 (Electron 판과 동일한 macOS 관례).
        // → `on_window_closed`에서 `cx.quit()`을 부르지 않는다.

        restore_all_windows(&paths, &settings, cx);

        // 어느 창에도 속하지 않는 스크롤백/cwd 파일을 정리한다.
        let live = window_manager::manager(cx).read(cx).live_session_ids();
        session_registry::registry(cx)
            .update(cx, |registry, _| registry.prune_scrollback(&live));

        cx.activate(true);
    });
}

/// 저장된 창 레코드대로 창을 연다. 하나도 못 열면 기본 창으로 떨어진다.
fn restore_all_windows(paths: &AppPaths, settings: &Settings, cx: &mut App) {
    // 구버전(단일 창) 지오메트리 — `windows`가 비었을 때만 쓰인다.
    let legacy = load_window_bounds(&paths.window_file, &DEFAULT_BOUNDS);
    let records: Vec<WindowRecord> = restore_windows(settings, Some(&legacy));

    let mut opened = 0usize;
    for record in &records {
        let seed = if record.tabs.is_empty() {
            None
        } else {
            Some(serde_json::json!({
                "tabs": record.tabs,
                "activeIndex": record.active_index(),
            }))
        };
        match workspace::open(seed, record.bounds.clone(), cx) {
            Ok(_) => opened += 1,
            Err(error) => eprintln!("sshub: 창을 열지 못했습니다: {error}"),
        }
    }

    if opened == 0 {
        if let Err(error) = workspace::open(None, DEFAULT_BOUNDS, cx) {
            eprintln!("sshub: 기본 창을 열지 못했습니다: {error}");
        }
    }
}

/// Dock 재활성화 시 쓸 위치 — 마지막으로 알던 창 지오메트리.
fn last_known_bounds(cx: &App) -> sshub_core::window_state::WindowBounds {
    window_manager::try_manager(cx)
        .and_then(|manager| manager.read(cx).records().first().map(|r| r.bounds.clone()))
        .unwrap_or(DEFAULT_BOUNDS)
}

/// 설정의 어센트·반투명·터미널 색을 전역 테마에 굽는다.
/// 내장 폰트가 등록됐는지 — 등록 실패 시 시스템 폰트로 떨어뜨리기 위해 남긴다.
fn embedded_font_ok(cx: &App) -> bool {
    cx.has_global::<EmbeddedFontOk>()
}

struct EmbeddedFontOk;
impl gpui::Global for EmbeddedFontOk {}

fn apply_appearance(settings: &Settings, cx: &mut App) {
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
        fonts::resolve_family(
            settings.appearance.terminal.font_family.as_deref(),
            embedded_font_ok(cx),
        ).into(),
    ));
}

fn set_menus(cx: &mut App) {
    cx.set_menus(vec![Menu {
        name: "sshub".into(),
        items: vec![MenuItem::action("Quit sshub", Quit)],
    }]);
}

/// 종료 순서 (DESIGN-terminal.md §6):
/// ① 살아 있는 로컬 세션의 cwd 스냅샷 — **PTY를 죽이기 전에** 해야 읽을 수 있다.
/// ② 스크롤백 flush (디바운스를 기다리지 않는다).
/// ③ 창 레코드 저장 (디바운스 우회 — 즉시 쓴다).
/// ④ 모든 PTY kill.
fn install_quit_hook(cx: &mut App) {
    cx.on_app_quit(|cx: &mut App| {
        // 이 시점부터 창이 드랍돼도 레코드를 지우지 않는다 (아래 ③을 보호).
        if let Some(manager) = window_manager::try_manager(cx) {
            manager.update(cx, |manager, _| manager.begin_quit());
        }

        if let Some(registry) = session_registry::try_registry(cx) {
            registry.update(cx, |registry, cx| {
                registry.snapshot_cwds(cx); // ①
                registry.flush_scrollback(cx); // ②
            });
        }

        if let Some(manager) = window_manager::try_manager(cx) {
            manager.update(cx, |manager, cx| manager.persist_now(cx)); // ③
        }

        if let Some(registry) = session_registry::try_registry(cx) {
            registry.update(cx, |registry, cx| registry.shutdown_all(cx)); // ④
        }

        // 남은 비동기 작업 없음 — 위 단계는 모두 동기다.
        async {}
    })
    .detach();

    // `app_state`가 초기화돼 있는지 확인해 두면 종료 훅에서 안전하게 쓸 수 있다.
    let _ = app_state(cx);
}
