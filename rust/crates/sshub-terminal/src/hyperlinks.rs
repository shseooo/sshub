//! 터미널 한 줄에서 클릭 가능한 대상(URL / 절대 파일 경로)을 찾는다.
//! `src/lib/filePaths.ts` 포팅 + URL 정규식 (DESIGN-terminal.md §4 "마우스/스크롤/링크/검색").
//!
//! 오프셋은 **char(유니코드 스칼라) 인덱스**다. 호출부(`Terminal`)가 hover 줄을
//! "셀 1개 = char 1개"(WIDE_CHAR_SPACER 제외)로 만들고 char 인덱스 → `Column`
//! 매핑을 따로 들고 있기 때문에, 바이트 오프셋보다 이쪽이 그리드에 바로 꽂힌다.

use std::sync::OnceLock;

use regex::Regex;

/// 줄 안에서 찾아낸 링크 후보.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkMatch {
    pub text: String,
    pub kind: LinkKind,
    /// 0-based char 오프셋 (시작).
    pub start: usize,
    /// 0-based char 오프셋 (끝 다음).
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    /// 절대 경로(`/` 또는 `~/`). 존재 여부는 클릭 시점에 확인한다.
    Path,
}

// URL은 스킴이 있는 것만 — 스킴 없는 "www." 휴리스틱은 오탐이 많아 제외한다.
const URL_PATTERN: &str = r#"(?i)\b(?:https?|ftp|file)://[^\s<>"'`{}|\\^\[\]]+"#;

// 경로는 줄 시작이거나 구분자 뒤에서만 시작한다. `:`를 구분자에서 뺀 이유는
// `https://…`의 `//host`가 경로로 잡히지 않게 하기 위함이고, 그래서 단어 중간의
// 슬래시(`a/b`)도 자연히 건너뛴다. (원본: /(^|[\s([{=<'"])((?:~\/|\/)[A-Za-z0-9._+\-@\/]+)/g)
const PATH_PATTERN: &str = r#"(^|[\s(\[{=<'"])((?:~/|/)[A-Za-z0-9._+\-@/]+)"#;

// 문장 부호가 경로/URL 끝에 붙어오는 경우를 떼어낸다.
const TRAILING_PATTERN: &str = r#"[.,;:)\]}>'"]+$"#;

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(URL_PATTERN).expect("URL_PATTERN"))
}

fn path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(PATH_PATTERN).expect("PATH_PATTERN"))
}

fn trailing_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TRAILING_PATTERN).expect("TRAILING_PATTERN"))
}

fn strip_trailing(text: &str) -> &str {
    match trailing_re().find(text) {
        Some(m) => &text[..m.start()],
        None => text,
    }
}

/// 바이트 오프셋 → char 오프셋.
fn char_offset(line: &str, byte: usize) -> usize {
    line[..byte].chars().count()
}

/// 줄에서 URL만 찾는다.
pub fn find_urls(line: &str) -> Vec<LinkMatch> {
    let mut out = Vec::new();
    for m in url_re().find_iter(line) {
        let text = strip_trailing(m.as_str());
        // 스킴만 남은 것(`http://`)은 링크로 치지 않는다.
        if text.len() < 2 || text.ends_with("//") {
            continue;
        }
        let start = char_offset(line, m.start());
        out.push(LinkMatch {
            start,
            end: start + text.chars().count(),
            text: text.to_string(),
            kind: LinkKind::Url,
        });
    }
    out
}

/// 줄에서 절대 파일 경로만 찾는다 (`filePaths.ts::findFilePaths`).
pub fn find_file_paths(line: &str) -> Vec<LinkMatch> {
    let mut out = Vec::new();
    for caps in path_re().captures_iter(line) {
        let lead = caps.get(1).map(|m| m.as_str().len()).unwrap_or(0);
        let whole = caps.get(0).expect("group 0");
        let raw = caps.get(2).expect("group 2").as_str();
        let text = strip_trailing(raw);
        if text.chars().count() < 2 || !text.contains('/') {
            continue;
        }
        let start = char_offset(line, whole.start() + lead);
        out.push(LinkMatch {
            start,
            end: start + text.chars().count(),
            text: text.to_string(),
            kind: LinkKind::Path,
        });
    }
    out
}

/// URL + 경로를 한 번에. URL이 우선이며, URL과 겹치는 경로 후보는 버린다.
pub fn find_links(line: &str) -> Vec<LinkMatch> {
    let mut out = find_urls(line);
    for p in find_file_paths(line) {
        let overlaps = out.iter().any(|u| p.start < u.end && u.start < p.end);
        if !overlaps {
            out.push(p);
        }
    }
    out.sort_by_key(|m| m.start);
    out
}

/// char 오프셋이 가리키는 링크 (cmd-hover 판정).
pub fn link_at(line: &str, char_index: usize) -> Option<LinkMatch> {
    find_links(line)
        .into_iter()
        .find(|m| char_index >= m.start && char_index < m.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(line: &str) -> Vec<(String, usize, usize)> {
        find_file_paths(line)
            .into_iter()
            .map(|m| (m.text, m.start, m.end))
            .collect()
    }

    fn urls(line: &str) -> Vec<(String, usize, usize)> {
        find_urls(line)
            .into_iter()
            .map(|m| (m.text, m.start, m.end))
            .collect()
    }

    #[test]
    fn finds_absolute_path_at_line_start() {
        assert_eq!(paths("/usr/local/bin"), vec![("/usr/local/bin".into(), 0, 14)]);
    }

    #[test]
    fn finds_tilde_path_after_space() {
        assert_eq!(paths("open ~/.ssh/config"), vec![("~/.ssh/config".into(), 5, 18)]);
    }

    #[test]
    fn skips_slash_inside_a_word() {
        // 단어 중간 슬래시는 구분자 뒤가 아니라서 매치되지 않는다
        assert!(paths("foo/bar baz").is_empty());
    }

    #[test]
    fn does_not_match_url_authority_as_path() {
        // `:`가 구분자 목록에서 빠져 있어 `//example.com`이 경로로 잡히지 않는다
        assert!(paths("https://example.com/x").is_empty());
    }

    #[test]
    fn strips_trailing_punctuation() {
        assert_eq!(paths("see /etc/hosts."), vec![("/etc/hosts".into(), 4, 14)]);
        assert_eq!(paths("(/tmp/a)"), vec![("/tmp/a".into(), 1, 7)]);
    }

    #[test]
    fn rejects_bare_slash() {
        assert!(paths("cd / now").is_empty());
    }

    #[test]
    fn multiple_paths_in_one_line() {
        assert_eq!(
            paths("cp /a/b /c/d"),
            vec![("/a/b".into(), 3, 7), ("/c/d".into(), 8, 12)]
        );
    }

    #[test]
    fn path_after_bracket_and_quote_delimiters() {
        assert_eq!(paths("[\"/x/y\"]").len(), 1);
        assert_eq!(paths("={/x/y}").len(), 1);
    }

    #[test]
    fn offsets_are_char_based_not_byte_based() {
        // 한글 3글자 + 공백 = char 4개. 바이트였다면 10이 나온다.
        let line = "가나다 /tmp/x";
        assert_eq!(paths(line), vec![("/tmp/x".into(), 4, 10)]);
    }

    #[test]
    fn finds_http_and_https_urls() {
        assert_eq!(
            urls("go to https://zed.dev/docs now"),
            vec![("https://zed.dev/docs".into(), 6, 26)]
        );
        assert_eq!(urls("http://a.b").len(), 1);
    }

    #[test]
    fn url_trailing_punctuation_and_brackets_removed() {
        assert_eq!(urls("(see https://x.dev/a)"), vec![("https://x.dev/a".into(), 5, 20)]);
    }

    #[test]
    fn bare_scheme_is_not_a_url() {
        assert!(urls("http://").is_empty());
    }

    #[test]
    fn find_links_prefers_url_over_overlapping_path() {
        let all = find_links("open https://example.com/a/b here");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, LinkKind::Url);
    }

    #[test]
    fn find_links_returns_both_kinds_sorted() {
        let all = find_links("/tmp/a and https://x.dev/b");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, LinkKind::Path);
        assert_eq!(all[1].kind, LinkKind::Url);
        assert!(all[0].start < all[1].start);
    }

    #[test]
    fn link_at_hit_and_miss() {
        let line = "run /usr/bin/env";
        assert!(link_at(line, 4).is_some());
        assert!(link_at(line, 15).is_some());
        assert!(link_at(line, 16).is_none());
        assert!(link_at(line, 0).is_none());
    }
}
