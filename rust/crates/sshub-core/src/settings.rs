//! 앱 설정 (`sshub_settings.json`) — Electron 시절 renderer localStorage에 있던
//! 값들의 대체 (DESIGN-core.md §5).
//!
//! 마이그레이션은 하지 않는다: 구 값들은 Chromium LevelDB 안에 있고, 되찾을
//! 것은 UI 취향 6개뿐이다. 사용자가 잃으면 곤란한 데이터(서버·키·스크롤백·
//! 창 크기)는 애초에 localStorage에 없었으므로 그대로 살아남는다.
//!
//! 터미널 레이아웃은 의도적으로 `serde_json::Value`로 둔다 — 트리 타입은
//! sshub-splits 소유이고, 코어가 그 크레이트를 알 필요는 없다. 앱이 읽어서
//! 파싱한다.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fsutil;

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    /// "ko" | "en" | "ja" — 부재면 앱이 시스템 로케일로 감지한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default = "default_start_page")]
    pub start_page: String,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    /// `{ "tabs": [...], "activeIndex": n }` — 단일 창 레이아웃(구버전 호환).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_layout: Option<serde_json::Value>,
    /// 다중 창(신규): 창별 bounds + 레이아웃. 비어 있으면 단일 창으로 시작한다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub accent: String,
    /// 0..=40 (%). 그 이상은 텍스트 가독성이 무너져 상한을 둔다.
    pub translucency: u8,
    pub terminal: TerminalAppearance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAppearance {
    pub font_size: f32,
    /// 비어 있으면 앱 내장 고정폭 한글 폰트(D2Coding)를 쓴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

fn default_start_page() -> String {
    "dashboard".to_string()
}

pub const START_PAGES: [&str; 5] = ["dashboard", "servers", "terminal", "keys", "settings"];

/// 기존 앱의 단축키 기본값을 gpui Keystroke 표기로 옮긴 것
/// (meta→cmd, KeyT→t, Equal→=, ArrowLeft→left …).
///
/// 수식어 순서는 gpui `Keystroke::unparse` 표기 `fn-ctrl-alt-cmd-shift-key`를
/// 따른다 — 저장값끼리 문자열로 비교(충돌 검사)하려면 표기가 하나여야 한다.
pub fn default_shortcuts() -> BTreeMap<String, String> {
    [
        ("newTab", "cmd-t"),
        ("closePane", "cmd-w"),
        ("splitRight", "cmd-d"),
        ("splitDown", "cmd-shift-d"),
        ("broadcast", "cmd-shift-i"),
        ("fontIncrease", "cmd-shift-="),
        ("fontDecrease", "cmd-shift--"),
        ("focusLeft", "alt-cmd-left"),
        ("focusRight", "alt-cmd-right"),
        ("focusUp", "alt-cmd-up"),
        ("focusDown", "alt-cmd-down"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            accent: "#74ade8".to_string(),
            translucency: 0,
            terminal: TerminalAppearance::default(),
        }
    }
}

impl Default for TerminalAppearance {
    fn default() -> Self {
        Self { font_size: 14.0, font_family: None, foreground: None, background: None }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            language: None,
            start_page: default_start_page(),
            sidebar_collapsed: false,
            appearance: Appearance::default(),
            shortcuts: default_shortcuts(),
            terminal_layout: None,
            windows: Vec::new(),
        }
    }
}

impl Settings {
    /// 없거나 손상된 파일은 기본값 — 설정 파일 하나 때문에 앱이 못 뜨면 안 된다.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else { return Settings::default() };
        let mut settings: Settings = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => return Settings::default(),
        };
        settings.normalize();
        settings
    }

    /// best-effort — 설정 저장 실패로 사용자 작업을 막지 않는다.
    pub fn save(&self, path: &Path) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fsutil::atomic_write_0600(path, text.as_bytes());
        }
    }

    /// 부분 파일·잘못된 값 보정. 누락 단축키는 기본값으로 채운다 — 새 액션이
    /// 추가돼도 구버전 설정 파일이 그 단축키를 영원히 잃지 않도록.
    pub fn normalize(&mut self) {
        self.version = SETTINGS_VERSION;
        if !START_PAGES.contains(&self.start_page.as_str()) {
            self.start_page = default_start_page();
        }
        if !matches!(self.language.as_deref(), Some("ko" | "en" | "ja")) {
            self.language = None;
        }
        self.appearance.translucency = self.appearance.translucency.min(40);
        self.appearance.terminal.font_size = self.appearance.terminal.font_size.clamp(10.0, 24.0);
        for (action, combo) in default_shortcuts() {
            self.shortcuts.entry(action).or_insert(combo);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load(&dir.path().join("nope.json"));
        assert_eq!(s, Settings::default());
        assert_eq!(s.shortcuts.get("newTab").unwrap(), "cmd-t");
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub_settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn partial_file_merges_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        std::fs::write(&path, br#"{"version":1,"sidebarCollapsed":true}"#).unwrap();
        let s = Settings::load(&path);
        assert!(s.sidebar_collapsed);
        assert_eq!(s.start_page, "dashboard");
        assert_eq!(s.appearance.accent, "#74ade8");
        assert_eq!(s.shortcuts.len(), default_shortcuts().len());
    }

    #[test]
    fn normalize_clamps_and_rejects_bad_values() {
        let mut s = Settings {
            start_page: "/nope".into(),
            language: Some("fr".into()),
            ..Settings::default()
        };
        s.appearance.translucency = 90;
        s.appearance.terminal.font_size = 99.0;
        s.normalize();
        assert_eq!(s.start_page, "dashboard");
        assert_eq!(s.language, None);
        assert_eq!(s.appearance.translucency, 40);
        assert_eq!(s.appearance.terminal.font_size, 24.0);
    }

    #[test]
    fn round_trip_preserves_layout_blob() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut s = Settings::default();
        s.terminal_layout = Some(serde_json::json!({
            "tabs": [{ "root": { "type": "leaf", "sessionId": "abc", "serverId": null, "label": "local" } }],
            "activeIndex": 0
        }));
        s.save(&path);
        let back = Settings::load(&path);
        assert_eq!(back.terminal_layout, s.terminal_layout);
    }

    #[test]
    fn saved_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        Settings::default().save(&path);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
