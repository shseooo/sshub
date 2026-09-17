//! JSONL 세션 파일을 **필요한 줄만** 파싱하는 도우미.
//!
//! 세션 파일은 수 MB까지 크고 도구 결과 한 줄이 수십 KB일 수 있다. 줄마다
//! JSON 파싱을 하면 메뉴가 늦어지므로, 호출자가 준 부분 문자열이 들어 있는
//! 줄만 파싱한다 (`"type":"user"` 같은 것). 형식이 깨진 줄은 건너뛴다.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// `needles` 중 하나를 포함하는 줄만 JSON으로 파싱해 `visit`에 준다.
/// `visit`이 `false`를 돌려주면 멈춘다.
pub fn scan(path: &Path, needles: &[&str], mut visit: impl FnMut(&Value) -> bool) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let reader = BufReader::new(file);
    for line in reader.split(b'\n') {
        let Ok(line) = line else {
            break;
        };
        let Ok(line) = std::str::from_utf8(&line) else {
            continue;
        };
        if !needles.iter().any(|n| line.contains(n)) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !visit(&value) {
            break;
        }
    }
}

/// 첫 줄만 파싱한다 (헤더 레코드용).
pub fn first_line(path: &Path) -> Option<Value> {
    let file = File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    serde_json::from_str(line.trim_end()).ok()
}

/// 메시지 본문에서 사용자가 친 텍스트를 뽑는다. `content`가 문자열이면 그대로,
/// 블록 배열이면 `text` 블록들을 이어 붙인다.
pub fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}
