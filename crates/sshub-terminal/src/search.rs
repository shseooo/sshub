//! `RegexSearch` 래퍼 — 그리드 전체에서 매치를 모은다 (DESIGN-terminal.md §4).
//!
//! alacritty의 `RegexSearch`는 DFA를 미리 굽기 때문에 생성 비용이 있다.
//! 검색바 입력마다 새로 만들지 말고 `SearchQuery`를 들고 재사용하며,
//! 실제 스캔은 background executor에서 돌린다.

use crate::backend::{
    AlacPoint, Column, Dimensions, Direction, EventListener, Line, Match, RegexIter, RegexSearch,
    Term,
};

/// 컴파일된 검색 패턴.
pub struct SearchQuery {
    source: String,
    regex: RegexSearch,
}

impl std::fmt::Debug for SearchQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchQuery").field("source", &self.source).finish()
    }
}

impl SearchQuery {
    /// 잘못된 정규식이면 `None` (검색바가 조용히 무시하게).
    pub fn new(pattern: &str) -> Option<SearchQuery> {
        if pattern.is_empty() {
            return None;
        }
        RegexSearch::new(pattern)
            .ok()
            .map(|regex| SearchQuery { source: pattern.to_string(), regex })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// 히스토리를 포함한 그리드 전체의 매치. 상한을 둬 거대한 스크롤백에서
    /// UI가 굳지 않게 한다.
    pub fn find_matches<T: EventListener>(
        &mut self,
        term: &Term<T>,
        limit: usize,
    ) -> Vec<Match> {
        let grid = term.grid();
        if grid.columns() == 0 {
            return Vec::new();
        }
        let start = AlacPoint::new(grid.topmost_line(), Column(0));
        let end = AlacPoint::new(grid.bottommost_line(), Column(grid.columns() - 1));
        RegexIter::new(start, end, Direction::Right, term, &mut self.regex)
            .take(limit)
            .collect()
    }
}

/// 매치 목록에서 `origin` 다음(또는 이전) 매치의 인덱스. 순환한다.
pub fn step_match(matches: &[Match], origin: Option<usize>, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(match origin {
        None => {
            if forward {
                0
            } else {
                matches.len() - 1
            }
        }
        Some(i) => {
            if forward {
                (i + 1) % matches.len()
            } else {
                (i + matches.len() - 1) % matches.len()
            }
        }
    })
}

/// 매치가 차지하는 라인 범위 (렌더러가 rect를 그릴 때 쓴다).
pub fn match_lines(m: &Match) -> (Line, Line) {
    (m.start().line, m.end().line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AnsiProcessor, TermConfig, TermSize, VoidListener};

    fn make_term(cols: usize, lines: usize, input: &[u8]) -> Term<VoidListener> {
        let config = TermConfig { scrolling_history: 100, ..Default::default() };
        let mut term = Term::new(config, &TermSize::new(cols, lines), VoidListener);
        let mut parser = AnsiProcessor::new();
        parser.advance(&mut term, input);
        term
    }

    fn text_of(term: &Term<VoidListener>, m: &Match) -> String {
        term.bounds_to_string(*m.start(), *m.end())
    }

    #[test]
    fn rejects_empty_and_invalid_patterns() {
        assert!(SearchQuery::new("").is_none());
        assert!(SearchQuery::new("(unclosed").is_none());
        assert!(SearchQuery::new("ok").is_some());
    }

    #[test]
    fn finds_every_occurrence() {
        let term = make_term(40, 6, b"foo bar foo\r\nbaz foo\r\n");
        let mut q = SearchQuery::new("foo").unwrap();
        let matches = q.find_matches(&term, 100);
        assert_eq!(matches.len(), 3);
        for m in &matches {
            assert_eq!(text_of(&term, m), "foo");
        }
    }

    #[test]
    fn matches_are_ordered_top_to_bottom() {
        let term = make_term(40, 6, b"aaa\r\nbbb\r\naaa\r\n");
        let mut q = SearchQuery::new("aaa").unwrap();
        let matches = q.find_matches(&term, 100);
        assert_eq!(matches.len(), 2);
        assert!(matches[0].start().line <= matches[1].start().line);
    }

    #[test]
    fn regex_metacharacters_work() {
        let term = make_term(40, 6, b"id=42 id=7\r\n");
        let mut q = SearchQuery::new("id=[0-9]+").unwrap();
        let matches = q.find_matches(&term, 100);
        assert_eq!(matches.len(), 2);
        assert_eq!(text_of(&term, &matches[0]), "id=42");
        assert_eq!(text_of(&term, &matches[1]), "id=7");
    }

    #[test]
    fn no_matches_yields_empty() {
        let term = make_term(40, 6, b"hello\r\n");
        let mut q = SearchQuery::new("zzz").unwrap();
        assert!(q.find_matches(&term, 100).is_empty());
    }

    #[test]
    fn limit_caps_the_result_count() {
        let term = make_term(40, 8, b"x x x x x x\r\n");
        let mut q = SearchQuery::new("x").unwrap();
        assert_eq!(q.find_matches(&term, 2).len(), 2);
    }

    #[test]
    fn searches_scrollback_history_too() {
        let mut input = Vec::new();
        input.extend_from_slice(b"needle\r\n");
        for i in 0..10 {
            input.extend_from_slice(format!("filler{i}\r\n").as_bytes());
        }
        let term = make_term(40, 4, &input);
        let mut q = SearchQuery::new("needle").unwrap();
        assert_eq!(q.find_matches(&term, 100).len(), 1);
    }

    #[test]
    fn step_match_cycles_in_both_directions() {
        let term = make_term(40, 6, b"a\r\na\r\na\r\n");
        let mut q = SearchQuery::new("a").unwrap();
        let matches = q.find_matches(&term, 100);
        assert_eq!(matches.len(), 3);
        assert_eq!(step_match(&matches, None, true), Some(0));
        assert_eq!(step_match(&matches, None, false), Some(2));
        assert_eq!(step_match(&matches, Some(2), true), Some(0));
        assert_eq!(step_match(&matches, Some(0), false), Some(2));
        assert_eq!(step_match(&[], None, true), None);
    }

    #[test]
    fn match_lines_reports_the_span() {
        let term = make_term(40, 6, b"hello\r\n");
        let mut q = SearchQuery::new("hello").unwrap();
        let matches = q.find_matches(&term, 10);
        let (start, end) = match_lines(&matches[0]);
        assert_eq!(start, Line(0));
        assert_eq!(end, Line(0));
    }
}
