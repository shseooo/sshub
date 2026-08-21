//! 타임스탬프. `Date.toISOString()`과 동일한 밀리초 정밀 UTC ISO 8601
//! (`YYYY-MM-DDTHH:MM:SS.mmmZ`). 파일명 스탬프는 `:`/`.` → `-` 치환.

use chrono::{SecondsFormat, Utc};

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// JS `iso.replace(/[:.]/g, '-')` — corrupt 백업·config 백업 파일명에 쓰이며,
/// 문자열 정렬이 시간순 정렬과 일치한다.
pub fn stamp_from_iso(iso: &str) -> String {
    iso.chars()
        .map(|c| if c == ':' || c == '.' { '-' } else { c })
        .collect()
}

pub fn now_stamp() -> String {
    stamp_from_iso(&now_iso())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_iso_matches_date_to_iso_string_format() {
        let s = now_iso();
        // 2026-08-21T12:34:56.789Z — 밀리초 3자리 + 'Z'
        assert_eq!(s.len(), 24);
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[19..20], ".");
    }

    #[test]
    fn stamp_replaces_colons_and_dots() {
        assert_eq!(
            stamp_from_iso("2026-08-21T12:34:56.789Z"),
            "2026-08-21T12-34-56-789Z"
        );
    }
}
