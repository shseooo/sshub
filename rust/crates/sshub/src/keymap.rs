//! 앱 액션 + 키맵 (DESIGN-ui.md §7).
//!
//! gpui의 `clear_key_bindings()`는 위젯 바인딩까지 전부 지운다. 따라서
//! **모든** 바인딩 등록은 이 파일의 `register_all`을 거쳐야 한다 — 사용자가
//! 단축키를 다시 지정할 때 clear 후 여기만 다시 부르면 되도록.

use std::collections::BTreeMap;

use gpui::{actions, App, KeyBinding, Keystroke};

actions!(
    sshub,
    [
        NewTab,
        NewWindow,
        ClosePane,
        SplitRight,
        SplitDown,
        ToggleBroadcast,
        FontIncrease,
        FontDecrease,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
    ]
);

/// 설정 파일의 액션 이름 ↔ 액션 인스턴스. 설정 키는 Electron 시절 이름을
/// 유지한다(백업 번들 호환).
pub const ACTION_NAMES: [&str; 11] = [
    "newTab",
    "closePane",
    "splitRight",
    "splitDown",
    "broadcast",
    "fontIncrease",
    "fontDecrease",
    "focusLeft",
    "focusRight",
    "focusUp",
    "focusDown",
];

fn binding_for(action: &str, combo: &str) -> Option<KeyBinding> {
    // context "Workspace": 터미널 pane도 이 안에 있으므로 앱 단축키가
    // 어디서든 먹는다. 일반 키는 매칭되지 않아 그대로 PTY로 흘러간다.
    const CTX: Option<&str> = Some("Workspace");
    Some(match action {
        "newTab" => KeyBinding::new(combo, NewTab, CTX),
        "closePane" => KeyBinding::new(combo, ClosePane, CTX),
        "splitRight" => KeyBinding::new(combo, SplitRight, CTX),
        "splitDown" => KeyBinding::new(combo, SplitDown, CTX),
        "broadcast" => KeyBinding::new(combo, ToggleBroadcast, CTX),
        "fontIncrease" => KeyBinding::new(combo, FontIncrease, CTX),
        "fontDecrease" => KeyBinding::new(combo, FontDecrease, CTX),
        "focusLeft" => KeyBinding::new(combo, FocusLeft, CTX),
        "focusRight" => KeyBinding::new(combo, FocusRight, CTX),
        "focusUp" => KeyBinding::new(combo, FocusUp, CTX),
        "focusDown" => KeyBinding::new(combo, FocusDown, CTX),
        _ => return None,
    })
}

/// 사용자 지정 콤보가 gpui 문법으로 파싱되는지 — `KeyBinding::new`는 파싱
/// 실패 시 panic이므로 등록 전에 반드시 통과시킨다.
///
/// `Keystroke::parse`는 `"cmd-"`처럼 키가 비어 있어도 성공하므로(수식어만
/// 남은 미완성 입력) 키가 실제로 있는지 따로 확인한다.
pub fn is_valid_combo(combo: &str) -> bool {
    !combo.is_empty()
        && combo
            .split(' ')
            .all(|k| Keystroke::parse(k).is_ok_and(|ks| !ks.key.is_empty()))
}

/// gpui 정식 표기(`fn-ctrl-alt-cmd-shift-key`)로 정규화. 저장·비교는 항상 이
/// 형태로 한다 — 같은 조합이 두 가지 문자열로 남으면 충돌 검사가 새어나간다.
pub fn canonicalize_combo(combo: &str) -> Option<String> {
    if !is_valid_combo(combo) {
        return None;
    }
    let strokes: Vec<String> = combo
        .split(' ')
        .map(|k| Keystroke::parse(k).expect("검증됨").unparse())
        .collect();
    Some(strokes.join(" "))
}

/// 전체 키맵 재구축. 리바인딩 시 호출자는 `cx.clear_key_bindings()` 후 이
/// 함수를 다시 부른다.
/// 위젯 바인딩(TextInput/Select/…)은 `ui::init`이 등록한다 — 부트스트랩과
/// 리바인딩 경로 모두 `clear_key_bindings()` → `ui::init` → `register_all`
/// 순서를 지켜야 한다.
pub fn register_all(cx: &mut App, shortcuts: &BTreeMap<String, String>) {
    let defaults = sshub_core::settings::default_shortcuts();
    let mut bindings = vec![KeyBinding::new("cmd-n", NewWindow, Some("Workspace"))];
    for action in ACTION_NAMES {
        let combo = shortcuts
            .get(action)
            .filter(|c| is_valid_combo(c))
            .or_else(|| defaults.get(action))
            .cloned()
            .unwrap_or_default();
        if let Some(binding) = binding_for(action, &combo) {
            bindings.push(binding);
        }
    }
    cx.bind_keys(bindings);
}

/// Electron 시절 조합(`meta+KeyT`)을 gpui 표기(`cmd-t`)로 옮긴다.
/// 백업 번들에는 구 포맷이 들어 있어 import 시 변환이 필요하다.
pub fn combo_from_legacy(legacy: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut key: Option<String> = None;

    for token in legacy.split('+') {
        match token {
            "meta" => parts.push("cmd".into()),
            "ctrl" => parts.push("ctrl".into()),
            "alt" => parts.push("alt".into()),
            "shift" => parts.push("shift".into()),
            "" => return None,
            code => key = Some(legacy_key_name(code)?),
        }
    }

    let key = key?;
    parts.push(key);
    // 순서 정규화는 gpui에 맡긴다 — 우리가 흉내 내면 gpui 표기가 바뀔 때 어긋난다.
    canonicalize_combo(&parts.join("-"))
}

/// `KeyboardEvent.code` → gpui 키 이름.
fn legacy_key_name(code: &str) -> Option<String> {
    let name = match code {
        "Equal" => "=".to_string(),
        "Minus" => "-".to_string(),
        "NumpadAdd" => "+".to_string(),
        "NumpadSubtract" => "-".to_string(),
        "Space" => "space".to_string(),
        "Enter" => "enter".to_string(),
        "Escape" => "escape".to_string(),
        "Tab" => "tab".to_string(),
        "Backspace" => "backspace".to_string(),
        "Delete" => "delete".to_string(),
        "Home" => "home".to_string(),
        "End" => "end".to_string(),
        "PageUp" => "pageup".to_string(),
        "PageDown" => "pagedown".to_string(),
        "Backslash" => "\\".to_string(),
        "Slash" => "/".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Semicolon" => ";".to_string(),
        "Quote" => "'".to_string(),
        "BracketLeft" => "[".to_string(),
        "BracketRight" => "]".to_string(),
        "Backquote" => "`".to_string(),
        other => {
            if let Some(rest) = other.strip_prefix("Arrow") {
                rest.to_lowercase()
            } else if let Some(rest) = other.strip_prefix("Key") {
                rest.to_lowercase()
            } else if let Some(rest) = other.strip_prefix("Digit") {
                rest.to_string()
            } else if let Some(rest) = other.strip_prefix('F') {
                // F1..F20 — 숫자가 아니면 알 수 없는 코드다.
                if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
                    format!("f{rest}")
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    };
    Some(name)
}

/// 표시용 라벨 (⌘⌥⇧⌃ + 대문자 키) — 설정 화면의 단축키 목록에 쓴다.
pub fn display_combo(combo: &str) -> String {
    combo
        .split(' ')
        .map(|stroke| {
            let mut out = String::new();
            let mut key = stroke;
            loop {
                let Some((modifier, rest)) = key.split_once('-') else { break };
                // "cmd--" (cmd + 하이픈)처럼 키 자체가 '-'인 경우를 위해
                // 알려진 수식어일 때만 잘라낸다.
                match modifier {
                    "ctrl" => out.push('⌃'),
                    "alt" => out.push('⌥'),
                    "shift" => out.push('⇧'),
                    "cmd" | "super" | "win" => out.push('⌘'),
                    _ => break,
                }
                key = rest;
            }
            let label = match key {
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                "space" => "␣".to_string(),
                "" => "-".to_string(), // "cmd--" 의 뒤쪽 하이픈
                other if other.chars().count() == 1 => other.to_uppercase(),
                other => {
                    let mut c = other.chars();
                    match c.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                }
            };
            out.push_str(&label);
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_every_legacy_default() {
        // 구버전 기본값 11종이 전부 현재 기본값과 같은 콤보로 변환돼야 한다.
        let legacy = [
            ("newTab", "meta+KeyT", "cmd-t"),
            ("closePane", "meta+KeyW", "cmd-w"),
            ("splitRight", "meta+KeyD", "cmd-d"),
            ("splitDown", "shift+meta+KeyD", "cmd-shift-d"),
            ("broadcast", "shift+meta+KeyI", "cmd-shift-i"),
            ("fontIncrease", "shift+meta+Equal", "cmd-shift-="),
            ("fontDecrease", "shift+meta+Minus", "cmd-shift--"),
            ("focusLeft", "alt+meta+ArrowLeft", "alt-cmd-left"),
            ("focusRight", "alt+meta+ArrowRight", "alt-cmd-right"),
            ("focusUp", "alt+meta+ArrowUp", "alt-cmd-up"),
            ("focusDown", "alt+meta+ArrowDown", "alt-cmd-down"),
        ];
        let defaults = sshub_core::settings::default_shortcuts();
        for (action, old, expected) in legacy {
            let converted = combo_from_legacy(old).unwrap_or_else(|| panic!("{old} 변환 실패"));
            assert_eq!(converted, expected, "{action}");
            assert_eq!(&converted, defaults.get(action).unwrap(), "{action} 기본값과 일치");
        }
    }

    #[test]
    fn legacy_conversion_normalizes_modifier_order() {
        // 입력 순서가 달라도 gpui 정식 표기 하나로 모인다.
        assert_eq!(combo_from_legacy("meta+shift+KeyD").unwrap(), "cmd-shift-d");
        assert_eq!(combo_from_legacy("shift+meta+KeyD").unwrap(), "cmd-shift-d");
        assert_eq!(canonicalize_combo("shift-cmd-d").unwrap(), "cmd-shift-d");
    }

    #[test]
    fn legacy_conversion_handles_digits_and_function_keys() {
        assert_eq!(combo_from_legacy("ctrl+Digit1").unwrap(), "ctrl-1");
        assert_eq!(combo_from_legacy("F5").unwrap(), "f5");
    }

    #[test]
    fn legacy_conversion_rejects_unknown_codes() {
        assert_eq!(combo_from_legacy("meta+Unknown"), None);
        assert_eq!(combo_from_legacy(""), None);
        assert_eq!(combo_from_legacy("meta+"), None);
    }

    #[test]
    fn every_default_combo_parses() {
        for (action, combo) in sshub_core::settings::default_shortcuts() {
            assert!(is_valid_combo(&combo), "{action} = {combo} 파싱 실패");
        }
    }

    #[test]
    fn invalid_combos_are_rejected_before_binding() {
        // KeyBinding::new는 파싱 실패 시 panic — 등록 전 걸러내야 한다.
        assert!(!is_valid_combo("cmd-"));
        assert!(!is_valid_combo("nonsense-key"));
        assert!(!is_valid_combo(""));
    }

    #[test]
    fn display_labels_use_mac_symbols() {
        assert_eq!(display_combo("cmd-t"), "⌘T");
        assert_eq!(display_combo("cmd-shift-d"), "⌘⇧D");
        assert_eq!(display_combo("alt-cmd-left"), "⌥⌘←");
        assert_eq!(display_combo("cmd-shift-="), "⌘⇧=");
        assert_eq!(display_combo("cmd-shift--"), "⌘⇧-");
    }

    #[test]
    fn action_names_match_settings_defaults() {
        let defaults = sshub_core::settings::default_shortcuts();
        assert_eq!(ACTION_NAMES.len(), defaults.len());
        for name in ACTION_NAMES {
            assert!(defaults.contains_key(name), "{name} 기본값 누락");
        }
    }
}
