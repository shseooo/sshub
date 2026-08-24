//! 창 지오메트리 영속화 (windowState.ts 직역). x/y는 옵션 — 새 설치(또는
//! 손상 파일)면 크기만으로 OS가 창을 가운데 놓게 한다. compact JSON,
//! best-effort 쓰기.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

// 이 밑으로는 저장된 크기를 쓰레기로 보고 기본값으로 돌아간다 — 엉뚱한 작은
// 값 때문에 앱이 못 쓸 조각으로 열리는 일을 막는다.
const MIN_W: f64 = 600.0;
const MIN_H: f64 = 400.0;

/// 필드 순서는 JS 직렬화 순서(width,height,x,y)와 일치. x/y 부재 시 키 생략
/// (JS는 프로퍼티 자체를 만들지 않는다 — null이 아님).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
}

/// JS `Math.round` — 정확히 .5일 때 +∞ 방향 (f64::round의 away-from-zero와
/// 다르다: Math.round(-10.5) === -10).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// 저장된 bounds를 기본값 기준으로 검증한다. 잘못된/누락 필드는 기본값으로,
/// x/y는 둘 다 있을 때만 유지(한쪽만 있는 위치는 무의미), 숫자는 반올림 —
/// 소수 장치 픽셀은 일부 윈도 매니저를 혼란시킨다.
pub fn sanitize_bounds(saved: &serde_json::Value, defaults: &WindowBounds) -> WindowBounds {
    let Some(obj) = saved.as_object() else { return defaults.clone() };
    let num = |k: &str| obj.get(k).and_then(|v| v.as_f64());
    let width = match num("width") {
        Some(w) if w >= MIN_W => js_round(w) as u32,
        _ => defaults.width,
    };
    let height = match num("height") {
        Some(h) if h >= MIN_H => js_round(h) as u32,
        _ => defaults.height,
    };
    let mut out = WindowBounds { width, height, x: None, y: None };
    if let (Some(x), Some(y)) = (num("x"), num("y")) {
        out.x = Some(js_round(x) as i32);
        out.y = Some(js_round(y) as i32);
    }
    out
}

/// 저장된 bounds 로드 — 읽기/파싱 실패 시 기본값.
pub fn load_window_bounds(path: &Path, defaults: &WindowBounds) -> WindowBounds {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| sanitize_bounds(&v, defaults))
        .unwrap_or_else(|| defaults.clone())
}

/// bounds 영속화 (best-effort — 지오메트리 쓰기 실패가 종료를 막으면 안 된다).
pub fn save_window_bounds(path: &Path, bounds: &WindowBounds) {
    if let Ok(json) = serde_json::to_string(bounds) {
        let _ = fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DEFAULTS: WindowBounds = WindowBounds { width: 1000, height: 700, x: None, y: None };

    #[test]
    fn falls_back_to_defaults_for_non_object_input() {
        assert_eq!(sanitize_bounds(&json!(null), &DEFAULTS), DEFAULTS);
        assert_eq!(sanitize_bounds(&json!("nope"), &DEFAULTS), DEFAULTS);
    }

    #[test]
    fn keeps_a_valid_saved_size_and_position() {
        let r = sanitize_bounds(&json!({"x": 120, "y": 80, "width": 1280, "height": 800}), &DEFAULTS);
        assert_eq!(r, WindowBounds { width: 1280, height: 800, x: Some(120), y: Some(80) });
    }

    #[test]
    fn rejects_a_too_small_size_and_uses_the_defaults() {
        assert_eq!(sanitize_bounds(&json!({"width": 10, "height": 10}), &DEFAULTS), DEFAULTS);
    }

    #[test]
    fn drops_position_when_only_one_of_x_y_is_present() {
        let r = sanitize_bounds(&json!({"x": 50, "width": 1100, "height": 720}), &DEFAULTS);
        assert_eq!(r, WindowBounds { width: 1100, height: 720, x: None, y: None });
    }

    #[test]
    fn rounds_fractional_device_pixels() {
        let r = sanitize_bounds(
            &json!({"x": 10.6, "y": 20.4, "width": 1000.5, "height": 700.5}),
            &DEFAULTS,
        );
        assert_eq!(r, WindowBounds { width: 1001, height: 701, x: Some(11), y: Some(20) });
    }

    #[test]
    fn returns_defaults_when_the_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub_window.json");
        assert_eq!(load_window_bounds(&path, &DEFAULTS), DEFAULTS);
    }

    #[test]
    fn returns_defaults_when_the_file_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub_window.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_window_bounds(&path, &DEFAULTS), DEFAULTS);
    }

    #[test]
    fn round_trips_saved_bounds_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sshub_window.json");
        let b = WindowBounds { width: 1366, height: 900, x: Some(200), y: Some(150) };
        save_window_bounds(&path, &b);
        assert_eq!(load_window_bounds(&path, &DEFAULTS), b);
    }

    #[test]
    fn serializes_compact_with_js_key_order_and_omits_absent_position() {
        let b = WindowBounds { width: 1100, height: 720, x: None, y: None };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"width":1100,"height":720}"#);
        let with_pos = WindowBounds { width: 1, height: 2, x: Some(3), y: Some(4) };
        // 검증만: 최소 크기 미달 값도 저장 함수는 그대로 쓴다 (검증은 load 몫)
        assert_eq!(
            serde_json::to_string(&with_pos).unwrap(),
            r#"{"width":1,"height":2,"x":3,"y":4}"#
        );
    }
}
