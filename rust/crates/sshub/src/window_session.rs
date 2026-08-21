//! 창 세션 영속화 (DESIGN-terminal.md §8) — **다중 창**(신규 기능)의 상태 계층.
//!
//! Electron 판은 창이 하나뿐이라 `sshub_window.json`에 지오메트리만 넣고,
//! 탭 레이아웃은 renderer localStorage에 따로 뒀다. 창이 여러 개가 되면 둘을
//! 함께 묶어야 "어느 창에 어떤 탭이 있었는지"가 복원된다. 그래서 창 레코드는
//! 설정 파일(`sshub_settings.json`)의 `windows` 배열에 저장하고, 구버전
//! `sshub_window.json`은 **첫 창의 지오메트리 폴백**으로만 읽는다.
//!
//! 세션 id는 그대로 보존한다 — 스크롤백/cwd 파일이 그 id로 저장돼 있어서,
//! id가 바뀌면 복원할 히스토리를 잃는다.

use serde::{Deserialize, Serialize};
use sshub_core::settings::Settings;
use sshub_core::window_state::{sanitize_bounds, WindowBounds};
use sshub_splits::{leaves, TerminalTab};

/// 창 하나의 복원 정보.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRecord {
    pub bounds: WindowBounds,
    /// TS `SavedTab`과 동일한 형태 (`{root, name?}`).
    #[serde(default)]
    pub tabs: Vec<TerminalTab>,
    #[serde(default)]
    pub active_index: usize,
}

pub const DEFAULT_BOUNDS: WindowBounds =
    WindowBounds { width: 1000, height: 700, x: None, y: None };

impl WindowRecord {
    pub fn empty() -> WindowRecord {
        WindowRecord { bounds: DEFAULT_BOUNDS, tabs: Vec::new(), active_index: 0 }
    }

    /// 저장된 인덱스가 범위를 벗어나면 0으로 — 탭 수가 줄어든 채 저장됐을 수 있다.
    pub fn active_index(&self) -> usize {
        if self.active_index < self.tabs.len() {
            self.active_index
        } else {
            0
        }
    }

    /// 이 창이 참조하는 모든 세션 id (스크롤백 prune 대상 계산에 쓴다).
    pub fn session_ids(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(|t| leaves(&t.root))
            .map(|l| l.session_id.as_str().to_string())
            .collect()
    }
}

/// 시작 시 복원할 창 목록.
///
/// 우선순위: ① 설정의 `windows` 배열(다중 창) → ② 구버전 단일 레이아웃
/// (`terminal_layout`) + `sshub_window.json` 지오메트리 → ③ 기본 창 하나.
pub fn restore_windows(settings: &Settings, legacy_bounds: Option<&WindowBounds>) -> Vec<WindowRecord> {
    let from_settings: Vec<WindowRecord> = settings
        .windows
        .iter()
        .filter_map(|v| serde_json::from_value::<WindowRecord>(v.clone()).ok())
        .map(sanitize_record)
        .collect();
    if !from_settings.is_empty() {
        return from_settings;
    }

    let mut record = WindowRecord {
        bounds: legacy_bounds.cloned().unwrap_or(DEFAULT_BOUNDS),
        ..WindowRecord::empty()
    };
    if let Some(layout) = &settings.terminal_layout {
        if let Some(tabs) = layout.get("tabs") {
            record.tabs = serde_json::from_value(tabs.clone()).unwrap_or_default();
        }
        record.active_index =
            layout.get("activeIndex").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    }
    vec![sanitize_record(record)]
}

/// 창 목록을 설정에 반영한다. 구버전 단일 레이아웃 키는 첫 창 기준으로 함께
/// 갱신해 둔다 — 구버전 앱으로 되돌아가도 탭이 살아 있도록.
pub fn persist_windows(settings: &mut Settings, windows: &[WindowRecord]) {
    settings.windows = windows
        .iter()
        .filter_map(|w| serde_json::to_value(w).ok())
        .collect();
    settings.terminal_layout = windows.first().and_then(|w| {
        serde_json::to_value(serde_json::json!({
            "tabs": w.tabs,
            "activeIndex": w.active_index(),
        }))
        .ok()
    });
}

/// 살아 있어야 할 세션 id 전체 — 나머지 스크롤백/cwd 항목은 정리 대상이다.
pub fn live_session_ids(windows: &[WindowRecord]) -> Vec<String> {
    let mut ids: Vec<String> = windows.iter().flat_map(|w| w.session_ids()).collect();
    ids.sort();
    ids.dedup();
    ids
}

fn sanitize_record(mut record: WindowRecord) -> WindowRecord {
    let value = serde_json::to_value(&record.bounds).unwrap_or(serde_json::Value::Null);
    record.bounds = sanitize_bounds(&value, &DEFAULT_BOUNDS);
    // 빈 탭만 남은 창은 탭 하나 없이 열리게 두지 않는다 — 호출자가 기본 탭을
    // 만들어야 함을 빈 목록으로 알린다.
    record.tabs.retain(|t| !leaves(&t.root).is_empty());
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshub_splits::{PaneNode, SplitDirection, TerminalLeaf, TerminalSplit};

    fn tab(id: &str, session: &str) -> TerminalTab {
        TerminalTab {
            id: id.into(),
            root: PaneNode::Leaf(TerminalLeaf::new(session, None, "local")),
            name: None,
        }
    }

    fn split_tab(id: &str, a: &str, b: &str) -> TerminalTab {
        TerminalTab {
            id: id.into(),
            root: PaneNode::Split(TerminalSplit {
                id: "s0".into(),
                direction: SplitDirection::Row,
                sizes: vec![50.0, 50.0],
                children: vec![
                    PaneNode::Leaf(TerminalLeaf::new(a, None, "a")),
                    PaneNode::Leaf(TerminalLeaf::new(b, Some(3), "b")),
                ],
            }),
            name: Some("작업".into()),
        }
    }

    #[test]
    fn falls_back_to_a_single_default_window() {
        let windows = restore_windows(&Settings::default(), None);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].bounds, DEFAULT_BOUNDS);
        assert!(windows[0].tabs.is_empty());
    }

    #[test]
    fn legacy_single_layout_is_restored_into_one_window() {
        let mut settings = Settings::default();
        settings.terminal_layout = Some(serde_json::json!({
            "tabs": [
                { "root": { "type": "leaf", "sessionId": "s1", "serverId": null, "label": "local" } },
                { "root": { "type": "leaf", "sessionId": "s2", "serverId": 7, "label": "prod" }, "name": "서버" }
            ],
            "activeIndex": 1
        }));
        let legacy = WindowBounds { width: 1280, height: 800, x: Some(10), y: Some(20) };

        let windows = restore_windows(&settings, Some(&legacy));
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].bounds, legacy, "구버전 지오메트리를 이어받는다");
        assert_eq!(windows[0].tabs.len(), 2);
        assert_eq!(windows[0].active_index(), 1);
        assert_eq!(windows[0].session_ids(), vec!["s1", "s2"], "세션 id 보존");
    }

    #[test]
    fn multi_window_records_round_trip() {
        let mut settings = Settings::default();
        let windows = vec![
            WindowRecord {
                bounds: WindowBounds { width: 1200, height: 900, x: Some(0), y: Some(0) },
                tabs: vec![tab("t1", "s1"), split_tab("t2", "s2", "s3")],
                active_index: 1,
            },
            WindowRecord {
                bounds: WindowBounds { width: 800, height: 600, x: None, y: None },
                tabs: vec![tab("t3", "s4")],
                active_index: 0,
            },
        ];
        persist_windows(&mut settings, &windows);

        let restored = restore_windows(&settings, None);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].bounds.width, 1200);
        assert_eq!(restored[0].tabs.len(), 2);
        assert_eq!(restored[0].tabs[1].name.as_deref(), Some("작업"));
        assert_eq!(restored[1].session_ids(), vec!["s4"]);
        assert_eq!(live_session_ids(&restored), vec!["s1", "s2", "s3", "s4"]);
    }

    #[test]
    fn persisting_also_updates_the_legacy_single_layout_key() {
        let mut settings = Settings::default();
        persist_windows(
            &mut settings,
            &[WindowRecord { bounds: DEFAULT_BOUNDS, tabs: vec![tab("t1", "s1")], active_index: 0 }],
        );
        let layout = settings.terminal_layout.unwrap();
        assert_eq!(layout["activeIndex"], 0);
        assert_eq!(layout["tabs"][0]["root"]["sessionId"], "s1");
    }

    #[test]
    fn bad_geometry_is_sanitized_not_trusted() {
        let mut settings = Settings::default();
        settings.windows = vec![serde_json::json!({
            "bounds": { "width": 10, "height": 5, "x": 3 },   // 최소치 미만 + 반쪽 위치
            "tabs": [],
            "activeIndex": 0
        })];
        let restored = restore_windows(&settings, None);
        assert_eq!(restored[0].bounds.width, DEFAULT_BOUNDS.width);
        assert_eq!(restored[0].bounds.height, DEFAULT_BOUNDS.height);
        assert_eq!(restored[0].bounds.x, None, "x/y는 둘 다 있을 때만 유지");
    }

    #[test]
    fn out_of_range_active_index_falls_back_to_the_first_tab() {
        let record = WindowRecord {
            bounds: DEFAULT_BOUNDS,
            tabs: vec![tab("t1", "s1")],
            active_index: 5,
        };
        assert_eq!(record.active_index(), 0);
    }

    #[test]
    fn corrupt_window_entries_are_skipped_without_losing_the_rest() {
        let mut settings = Settings::default();
        settings.windows = vec![
            serde_json::json!({ "nonsense": true }),
            serde_json::to_value(WindowRecord {
                bounds: DEFAULT_BOUNDS,
                tabs: vec![tab("t9", "s9")],
                active_index: 0,
            })
            .unwrap(),
        ];
        let restored = restore_windows(&settings, None);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].session_ids(), vec!["s9"]);
    }
}
