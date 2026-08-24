//! ssh config 문서 모델 — 원문 왕복 보존이 유일한 핵심 불변식이다.
//!
//! `Document::parse(t).to_string() == t` (바이트 단위, 임의 입력). 이 불변식이
//! 깨지면 동기화 한 번에 사용자가 손으로 쓴 `IdentityFile`·`ProxyJump`·
//! `Include`·`Match`·`ControlMaster`·주석이 사라진다 — 전체 파일 렌더러를
//! 걷어내고 이 모델을 도입한 이유가 그것이다.
//!
//! 구현 전략: 모든 노드/엔트리는 **개행 문자를 포함한 원문 한 줄**을 그대로
//! 들고 있고, 직렬화는 그 문자열들을 이어붙이기만 한다. 편집은 외과적이다 —
//! 건드린 줄의 `raw`만 바뀌고 나머지 바이트는 원본 그대로 남는다.

use std::fmt;

// ssh_config는 라인 지향이라 필드 값에 개행(또는 다른 제어 문자)이 섞이면
// 조작된 서버 이름/호스트/유저가 임의 지시어를 주입할 수 있다 — 예: 다음
// `ssh`에서 실행되는 `ProxyCommand`. 파일에 쓰는 모든 값에서 제어 문자를
// 제거한다. 정상적인 이름/호스트/유저에는 제어 문자가 없다. 신뢰 불가 값은
// config import 왕복과 공유/편집된 서버 항목으로 유입된다.
pub(crate) fn sanitize_config_value(value: &str) -> String {
    // C0 제어 문자(<0x20, CR/LF/TAB 포함)와 DEL(0x7f)만 제거하고 나머지는
    // 유지 — 비ASCII 이름 같은 인쇄 가능한 유니코드는 보존된다.
    value
        .chars()
        .filter(|&c| {
            let code = c as u32;
            code >= 0x20 && code != 0x7f
        })
        .collect()
}

/// Host 블록 안의 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// 주석·빈 줄·파싱 불가 라인 — 원문 그대로.
    Raw(String),
    /// `key`는 소문자 정규화, `value`는 따옴표/후행 주석을 제거한 값,
    /// `raw`는 개행까지 포함한 원문.
    Directive { key: String, value: String, raw: String },
}

impl Entry {
    pub fn raw(&self) -> &str {
        match self {
            Entry::Raw(s) => s,
            Entry::Directive { raw, .. } => raw,
        }
    }

    fn raw_mut(&mut self) -> &mut String {
        match self {
            Entry::Raw(s) => s,
            Entry::Directive { raw, .. } => raw,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostBlock {
    /// `Host a b c` → `["a", "b", "c"]` (따옴표 제거, 쉼표도 구분자).
    pub patterns: Vec<String>,
    /// 헤더 원문 한 줄 (개행 포함).
    pub header: String,
    pub entries: Vec<Entry>,
}

impl HostBlock {
    /// 블록의 특정 지시어 값 (첫 등장) — 읽기 전용 조회용.
    pub fn get(&self, key: &str) -> Option<&str> {
        let key = key.to_ascii_lowercase();
        self.entries.iter().find_map(|e| match e {
            Entry::Directive { key: k, value, .. } if *k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// 앱이 소유할 수 있는 블록이면 그 별칭. 단일 패턴 + 와일드카드 없음이
    /// 조건이다 — Phase 2에서 "이 블록이 서버 목록에 뜨는가"의 유일한 기준.
    pub fn writable_alias(&self) -> Option<&str> {
        match self.patterns.as_slice() {
            [only] if is_writable_alias(only) => Some(only.as_str()),
            _ => None,
        }
    }

    /// 여러 패턴이거나 와일드카드가 있으면 앱이 편집하지 않는다 —
    /// `Host a b c`의 한 패턴만 고치는 건 나머지 패턴의 의미까지 바꾼다.
    fn writable_as(&self, alias: &str) -> bool {
        self.patterns.len() == 1 && self.patterns[0] == alias && !has_wildcard(alias)
    }
}

/// `Match` 블록은 Phase 1에서 읽기 전용이다 (조건 평가를 하지 않으므로
/// 앱이 안전하게 병합할 수 없다). 원문 보존만 책임진다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchBlock {
    pub patterns: Vec<String>,
    pub header: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// 주석·빈 줄·Host 밖 전역 지시어(`Include` 등) — 원문 그대로.
    Raw(String),
    Host(HostBlock),
    Match(MatchBlock),
}

/// 앱이 소유하는 지시어만 담는다. `None`은 "이 블록에서 그 줄을 지운다"는
/// 뜻이다 (앱 상태가 이 다섯 키에 대해서는 권위를 가진다).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostSpec {
    pub host_name: Option<String>,
    /// `None` 또는 22면 줄을 쓰지 않는다 (22는 ssh 기본값이라 노이즈).
    pub port: Option<u16>,
    pub user: Option<String>,
    pub proxy_jump: Option<String>,
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    nodes: Vec<Node>,
    /// 원본이 CRLF면 새로 쓰는 줄도 CRLF로 — 편집이 개행 스타일을 섞지 않게.
    crlf: bool,
}

// -- 라인 스캐너 ------------------------------------------------------------

/// 개행을 포함한 줄 단위로 자른다 (마지막 줄은 개행이 없을 수 있다).
fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            out.push(&text[start..=i]);
            start = i + 1;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// 줄에서 개행을 떼어 (본문, 개행) 으로 분리 — 재조립이 바이트 동일하도록.
fn strip_eol(line: &str) -> (&str, &str) {
    if let Some(rest) = line.strip_suffix("\r\n") {
        (rest, "\r\n")
    } else if let Some(rest) = line.strip_suffix('\n') {
        (rest, "\n")
    } else {
        (line, "")
    }
}

struct Scanned<'a> {
    indent: &'a str,
    key: String,
    value: String,
    /// 본문 기준 값 영역 바이트 범위 (따옴표 포함, 후행 주석 제외).
    /// 값이 없으면 start == end == 삽입 지점.
    span: std::ops::Range<usize>,
}

/// 값 영역의 끝 = 후행 주석 앞(따옴표 밖의 ` #`) + 후행 공백 제거.
fn value_end(content: &str, start: usize) -> usize {
    let b = content.as_bytes();
    let mut in_quote = false;
    let mut cut = content.len();
    let mut i = start;
    while i < b.len() {
        match b[i] {
            b'"' => in_quote = !in_quote,
            b'#' if !in_quote && (i == start || b[i - 1] == b' ' || b[i - 1] == b'\t') => {
                cut = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let mut e = cut;
    while e > start && (b[e - 1] == b' ' || b[e - 1] == b'\t') {
        e -= 1;
    }
    e
}

fn unquote(region: &str) -> String {
    if region.len() >= 2 && region.starts_with('"') && region.ends_with('"') {
        region[1..region.len() - 1].to_string()
    } else {
        region.to_string()
    }
}

/// `Key Value` / `Key=Value` / `Key = Value` 를 모두 인식한다. 주석·빈 줄·
/// 키가 없는 줄은 `None` (= 원문 보존 대상).
fn scan_line(content: &str) -> Option<Scanned<'_>> {
    let b = content.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let indent = &content[..i];
    if i >= b.len() || b[i] == b'#' {
        return None;
    }
    let key_start = i;
    while i < b.len() && b[i] != b' ' && b[i] != b'\t' && b[i] != b'=' {
        i += 1;
    }
    let key = content[key_start..i].to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    if i < b.len() && b[i] == b'=' {
        i += 1;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
    }
    let start = i;
    let end = value_end(content, start);
    Some(Scanned { indent, key, value: unquote(&content[start..end]), span: start..end })
}

/// `Host` 패턴 목록 토큰화 — 공백과 쉼표가 구분자이고 `"..."`는 한 토큰이다
/// (OpenSSH `strdelim` + `match_pattern_list`와 같은 취급).
fn tokenize_patterns(region: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut started = false;
    for c in region.chars() {
        match c {
            '"' => {
                in_quote = !in_quote;
                started = true;
            }
            _ if !in_quote && (c.is_whitespace() || c == ',') => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            _ => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

pub(crate) fn has_wildcard(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('!')
}

/// 앱이 써도 되는 별칭인가. 와일드카드는 다른 호스트를 삼키고, 쉼표는
/// 패턴 목록으로 쪼개지며, `"`는 ssh_config 따옴표 안에서 이스케이프할
/// 방법이 없다 — 셋 다 쓰는 순간 다시 읽어들일 수 없는 줄이 된다.
pub(crate) fn is_writable_alias(alias: &str) -> bool {
    !alias.is_empty() && !has_wildcard(alias) && !alias.contains(',') && !alias.contains('"')
}

/// 공백·`#`·빈 값은 따옴표로 감싼다 (ssh_config 따옴표 규칙).
fn quote_if_needed(value: &str) -> String {
    let v: String = sanitize_config_value(value).replace('"', "");
    if v.is_empty() || v.chars().any(|c| c.is_whitespace()) || v.contains('#') {
        format!("\"{v}\"")
    } else {
        v
    }
}

/// 값만 갈아끼운 원문 한 줄 — 들여쓰기·키 대소문자·구분자 스타일·후행 주석은
/// 그대로 유지된다.
fn rewrite_value(raw: &str, new_value: &str) -> String {
    let (content, eol) = strip_eol(raw);
    let Some(s) = scan_line(content) else { return raw.to_string() };
    format!(
        "{}{}{}{}",
        &content[..s.span.start],
        quote_if_needed(new_value),
        &content[s.span.end..],
        eol
    )
}

/// 앱이 소유하는 다섯 지시어 — (조회 키, 기록 시 표기, 값).
fn owned_directives(spec: &HostSpec) -> Vec<(&'static str, &'static str, Option<String>)> {
    vec![
        ("hostname", "HostName", spec.host_name.clone()),
        ("port", "Port", spec.port.filter(|p| *p != 22).map(|p| p.to_string())),
        ("user", "User", spec.user.clone()),
        ("proxyjump", "ProxyJump", spec.proxy_jump.clone()),
        ("identityfile", "IdentityFile", spec.identity_file.clone()),
    ]
}

// -- Document ---------------------------------------------------------------

impl Document {
    pub fn parse(text: &str) -> Document {
        let mut doc = Document { nodes: Vec::new(), crlf: text.contains("\r\n") };
        // 현재 열려 있는 블록 — Host/Match 헤더를 만날 때까지 뒤따르는 모든
        // 줄이 여기에 속한다.
        let mut host: Option<HostBlock> = None;
        let mut mblock: Option<MatchBlock> = None;

        for line in split_lines(text) {
            let (content, _) = strip_eol(line);
            let scanned = scan_line(content);
            match scanned {
                Some(s) if s.key == "host" || s.key == "match" => {
                    doc.close(&mut host, &mut mblock);
                    let patterns = tokenize_patterns(&content[s.span.clone()]);
                    if s.key == "host" {
                        host = Some(HostBlock {
                            patterns,
                            header: line.to_string(),
                            entries: Vec::new(),
                        });
                    } else {
                        mblock = Some(MatchBlock {
                            patterns,
                            header: line.to_string(),
                            entries: Vec::new(),
                        });
                    }
                }
                Some(s) => {
                    let entry = Entry::Directive {
                        key: s.key,
                        value: s.value,
                        raw: line.to_string(),
                    };
                    if let Some(h) = host.as_mut() {
                        h.entries.push(entry);
                    } else if let Some(m) = mblock.as_mut() {
                        m.entries.push(entry);
                    } else {
                        // 블록 밖 전역 지시어(`Include`·`Host`가 없는 상단
                        // 설정)는 절대 편집하지 않으므로 원문으로 둔다.
                        doc.nodes.push(Node::Raw(line.to_string()));
                    }
                }
                None => {
                    let raw = Entry::Raw(line.to_string());
                    if let Some(h) = host.as_mut() {
                        h.entries.push(raw);
                    } else if let Some(m) = mblock.as_mut() {
                        m.entries.push(raw);
                    } else {
                        doc.nodes.push(Node::Raw(line.to_string()));
                    }
                }
            }
        }
        doc.close(&mut host, &mut mblock);
        doc
    }

    fn close(&mut self, host: &mut Option<HostBlock>, mblock: &mut Option<MatchBlock>) {
        if let Some(h) = host.take() {
            self.nodes.push(Node::Host(h));
        }
        if let Some(m) = mblock.take() {
            self.nodes.push(Node::Match(m));
        }
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// 이 파일이 끌어오는 `Include` 값들 (Phase 1은 따라가지 않고 노출만
    /// 한다 — UI가 "이 항목은 다른 파일에 있다"를 보여줄 수 있게).
    pub fn includes(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut push_raw = |raw: &str| {
            let (content, _) = strip_eol(raw);
            if let Some(s) = scan_line(content) {
                if s.key == "include" {
                    out.push(s.value);
                }
            }
        };
        for node in &self.nodes {
            match node {
                Node::Raw(raw) => push_raw(raw),
                Node::Host(h) => h.entries.iter().for_each(|e| push_raw(e.raw())),
                Node::Match(m) => m.entries.iter().for_each(|e| push_raw(e.raw())),
            }
        }
        out
    }

    pub fn hosts(&self) -> Vec<&HostBlock> {
        self.nodes
            .iter()
            .filter_map(|n| match n {
                Node::Host(h) => Some(h),
                _ => None,
            })
            .collect()
    }

    pub fn host(&self, alias: &str) -> Option<&HostBlock> {
        self.hosts().into_iter().find(|h| h.patterns.iter().any(|p| p == alias))
    }

    /// 와일드카드가 아닌 모든 Host 패턴 (다중 패턴 블록의 개별 패턴 포함 —
    /// "이 별칭이 이미 있는가" 판정용).
    pub fn aliases(&self) -> Vec<String> {
        self.hosts()
            .into_iter()
            .flat_map(|h| h.patterns.iter().filter(|p| !has_wildcard(p)).cloned())
            .collect()
    }

    fn eol(&self) -> &'static str {
        if self.crlf {
            "\r\n"
        } else {
            "\n"
        }
    }

    fn writable_index(&self, alias: &str) -> Option<usize> {
        self.nodes.iter().position(|n| matches!(n, Node::Host(h) if h.writable_as(alias)))
    }

    /// 앱이 소유한 다섯 지시어를 병합한다. 그 외 지시어·주석·빈 줄은 절대
    /// 건드리지 않는다. 다중 패턴/와일드카드 블록은 읽기 전용이라 거절한다.
    pub fn upsert_host(&mut self, alias: &str, spec: &HostSpec) {
        let alias = sanitize_config_value(alias);
        if !is_writable_alias(&alias) {
            return;
        }
        match self.writable_index(&alias) {
            Some(i) => self.merge_into(i, spec),
            None => {
                // 이 별칭을 읽기 전용 블록(`Host a b c`, `Host *.dev`)이
                // 이미 소유하고 있으면 새 블록을 넣지 않는다 — 넣어봐야
                // 어느 쪽이 이기는지 사용자가 알 수 없는 중복이 된다.
                if self.host(&alias).is_some() {
                    return;
                }
                self.insert_host(&alias, spec);
            }
        }
    }

    fn merge_into(&mut self, index: usize, spec: &HostSpec) {
        let eol = self.eol().to_string();
        let Node::Host(block) = &mut self.nodes[index] else { return };
        let indent = dominant_indent(&block.entries);

        for (key, label, wanted) in owned_directives(spec) {
            let hits: Vec<usize> = block
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| match e {
                    Entry::Directive { key: k, .. } if k == key => Some(i),
                    _ => None,
                })
                .collect();

            match wanted {
                Some(value) => {
                    match hits.first() {
                        Some(&i) => {
                            // 값이 같으면 한 바이트도 건드리지 않는다.
                            if let Entry::Directive { value: cur, raw, .. } = &mut block.entries[i]
                            {
                                if *cur != value {
                                    *raw = rewrite_value(raw, &value);
                                    *cur = value;
                                }
                            }
                        }
                        None => {
                            let raw = format!("{indent}{label} {}{eol}", quote_if_needed(&value));
                            let at = insert_position(&block.entries);
                            block.entries.insert(
                                at,
                                Entry::Directive { key: key.to_string(), value, raw },
                            );
                        }
                    }
                    // 나머지 같은 키 줄은 건드리지 않는다. `IdentityFile`을
                    // 여러 줄 두는 것은 OpenSSH가 순서대로 시도하는 정상 설정이고,
                    // 우리가 지우면 접속이 깨진다.
                }
                // 앱에 값이 없다고 해서 사용자가 써 둔 줄을 지우지 않는다.
                // "앱이 이 키를 안 쓴다"와 "이 호스트에 이 키가 없어야 한다"는
                // 다른 말이다 — 실제로 비밀번호 인증 서버 때문에 사용자의
                // `IdentityFile ~/.ssh/id_rsa` 3줄이 지워지는 것을 확인했다.
                // 삭제는 config가 진실이 되는 단계에서 명시적 조작으로 다룬다.
                None => {}
            }
        }
    }

    fn insert_host(&mut self, alias: &str, spec: &HostSpec) {
        let eol = self.eol().to_string();
        let mut block = HostBlock {
            patterns: vec![alias.to_string()],
            header: format!("Host {}{eol}", quote_if_needed(alias)),
            entries: Vec::new(),
        };
        for (key, label, value) in owned_directives(spec) {
            if let Some(v) = value {
                let raw = format!("    {label} {}{eol}", quote_if_needed(&v));
                block.entries.push(Entry::Directive { key: key.to_string(), value: v, raw });
            }
        }

        // ssh는 키마다 **처음 만난 값**을 쓴다. `Host *` 뒤에 놓인 구체
        // 블록은 통째로 가려지므로, 첫 와일드카드 블록 바로 앞에 넣는다.
        let barrier = self
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Host(h) if h.patterns.iter().any(|p| has_wildcard(p))));

        match barrier {
            Some(at) => {
                self.nodes.insert(at, Node::Host(block));
                self.nodes.insert(at + 1, Node::Raw(eol));
            }
            None => {
                // 마지막 줄에 개행이 없으면 새 헤더가 그 줄에 붙어버린다.
                self.ensure_trailing_newline(&eol);
                if !self.nodes.is_empty() && !self.ends_with_blank_line() {
                    self.nodes.push(Node::Raw(eol));
                }
                self.nodes.push(Node::Host(block));
            }
        }
    }

    /// 이 별칭을 앱이 편집할 수 있는가 (쓰기 가능한 단일 패턴 블록이 이미
    /// 있거나, 아무도 소유하지 않아 새로 만들 수 있다).
    pub fn can_write(&self, alias: &str) -> bool {
        if !is_writable_alias(alias) {
            return false;
        }
        self.writable_index(alias).is_some() || self.host(alias).is_none()
    }

    /// 앱이 소유한 지시어 한 줄을 지운다. `upsert_host`의 "절대 지우지 않는다"
    /// 규칙에 대한 **명시적 예외**다 — 사용자가 UI에서 ProxyJump를 비우거나
    /// 포트를 기본값(22)으로 되돌린 경우, 그 줄이 남아 있으면 다음 load에서
    /// 값이 되살아나 편집이 먹지 않는다. 그래서 호출자는 "사용자가 이 필드를
    /// 직접 비웠다"를 아는 단일 서버 편집 경로뿐이고, 일괄 동기화는 쓰지 않는다.
    pub fn remove_directive(&mut self, alias: &str, key: &str) -> bool {
        let alias = sanitize_config_value(alias);
        let key = key.to_ascii_lowercase();
        let Some(i) = self.writable_index(&alias) else { return false };
        let Node::Host(block) = &mut self.nodes[i] else { return false };
        let before = block.entries.len();
        block
            .entries
            .retain(|e| !matches!(e, Entry::Directive { key: k, .. } if *k == key));
        before != block.entries.len()
    }

    /// `from` 블록의 별칭만 바꾼다 (단일 패턴 블록에서만).
    pub fn rename_host(&mut self, from: &str, to: &str) -> bool {
        let from = sanitize_config_value(from);
        let to = sanitize_config_value(to);
        if !is_writable_alias(&from) || !is_writable_alias(&to) {
            return false;
        }
        if self.host(&to).is_some() {
            return false; // 중복 별칭을 만들지 않는다
        }
        let Some(i) = self.writable_index(&from) else { return false };
        let Node::Host(block) = &mut self.nodes[i] else { return false };
        block.header = rewrite_value(&block.header, &to);
        block.patterns = vec![to];
        true
    }

    /// 블록을 통째로 지운다. 마지막 지시어 뒤에 주석이 남아 있으면 그 꼬리는
    /// 살려둔다 — 다음 블록을 설명하는 주석일 수 있기 때문.
    pub fn remove_host(&mut self, alias: &str) -> bool {
        let alias = sanitize_config_value(alias);
        if !is_writable_alias(&alias) {
            return false;
        }
        let Some(i) = self.writable_index(&alias) else { return false };
        let Node::Host(block) = self.nodes.remove(i) else { return false };
        let tail_at = insert_position(&block.entries);
        let tail: Vec<&Entry> = block.entries[tail_at..].iter().collect();
        if tail.iter().any(|e| !e.raw().trim().is_empty()) {
            let keep: Vec<Node> =
                tail.into_iter().map(|e| Node::Raw(e.raw().to_string())).collect();
            for (k, node) in keep.into_iter().enumerate() {
                self.nodes.insert(i + k, node);
            }
        }
        true
    }

    fn ends_with_blank_line(&self) -> bool {
        let raw = match self.nodes.last() {
            Some(Node::Raw(s)) => s.as_str(),
            Some(Node::Host(h)) => h.entries.last().map(|e| e.raw()).unwrap_or(&h.header),
            Some(Node::Match(m)) => m.entries.last().map(|e| e.raw()).unwrap_or(&m.header),
            None => return true,
        };
        raw.trim().is_empty()
    }

    fn ensure_trailing_newline(&mut self, eol: &str) {
        let last = match self.nodes.last_mut() {
            Some(Node::Raw(s)) => s,
            Some(Node::Host(h)) => {
                if h.entries.is_empty() {
                    &mut h.header
                } else {
                    h.entries.last_mut().unwrap().raw_mut()
                }
            }
            Some(Node::Match(m)) => {
                if m.entries.is_empty() {
                    &mut m.header
                } else {
                    m.entries.last_mut().unwrap().raw_mut()
                }
            }
            None => return,
        };
        if !last.ends_with('\n') {
            last.push_str(eol);
        }
    }
}

/// 새 지시어를 넣을 위치 = 마지막 지시어 바로 뒤 (블록 끝의 빈 줄·주석
/// 앞). 지시어가 하나도 없으면 헤더 바로 뒤.
fn insert_position(entries: &[Entry]) -> usize {
    entries
        .iter()
        .rposition(|e| matches!(e, Entry::Directive { .. }))
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// 블록에서 가장 많이 쓰인 들여쓰기 (동률이면 먼저 나온 것). 지시어가 없으면
/// 4칸 — 기존 렌더러가 쓰던 스타일.
fn dominant_indent(entries: &[Entry]) -> String {
    let indents: Vec<String> = entries
        .iter()
        .filter(|e| matches!(e, Entry::Directive { .. }))
        .filter_map(|e| {
            let (content, _) = strip_eol(e.raw());
            scan_line(content).map(|s| s.indent.to_string())
        })
        .collect();
    let mut best: Option<(String, usize)> = None;
    for candidate in &indents {
        let count = indents.iter().filter(|i| *i == candidate).count();
        match &best {
            Some((_, c)) if *c >= count => {}
            _ => best = Some((candidate.clone(), count)),
        }
    }
    best.map(|(i, _)| i).unwrap_or_else(|| "    ".to_string())
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for node in &self.nodes {
            match node {
                Node::Raw(s) => f.write_str(s)?,
                Node::Host(h) => {
                    f.write_str(&h.header)?;
                    for e in &h.entries {
                        f.write_str(e.raw())?;
                    }
                }
                Node::Match(m) => {
                    f.write_str(&m.header)?;
                    for e in &m.entries {
                        f.write_str(e.raw())?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALISTIC: &str = "\
# ~/.ssh/config — 손으로 관리하는 파일
Include ~/.ssh/conf.d/*.conf

Host web prod-web
\tHostName 10.0.0.1
\tUser deploy
\tIdentityFile \"~/my keys/id_ed25519\"   # 주 키

Host db
  HostName=10.0.0.2
  Port = 2222
  ControlMaster auto
  ControlPath ~/.ssh/cm-%r@%h:%p

Match host *.internal exec \"true\"
  ProxyJump bastion
  # match 안 주석

Host *
  ServerAliveInterval 60
";

    fn corpus() -> Vec<String> {
        let mut v: Vec<String> = vec![
            REALISTIC.to_string(),
            String::new(),
            "\n".to_string(),
            "\n\n\n".to_string(),
            "# 주석만 있는 파일\n# 두 번째 줄\n".to_string(),
            "Host solo\n  HostName h".to_string(), // 마지막 개행 없음
            "   \t  \nHost pad\n\t\tUser u\t\n".to_string(),
            "Host eq\nHostName=1.2.3.4\nPort=2200\n".to_string(),
            "Host quoted\n  IdentityFile \"~/my keys/id\"\n".to_string(),
            "Include ~/other\nHost after\n  User u\n".to_string(),
            "Match final all\n  User m\n".to_string(),
            "garbage-line-without-value\nHost x\n".to_string(),
        ];
        // CRLF 변형도 같은 불변식을 지켜야 한다.
        v.push(REALISTIC.replace('\n', "\r\n"));
        v.push("Host crlf\r\n\tUser u\r\n".to_string());
        v.push("Host crlf-no-eol\r\n\tUser u".to_string());
        v
    }

    #[test]
    fn round_trips_every_corpus_file_byte_for_byte() {
        for text in corpus() {
            assert_eq!(Document::parse(&text).to_string(), text, "왕복 실패: {text:?}");
        }
    }

    /// 왕복 불변식은 이 모듈 전체가 서 있는 바닥이라, 손으로 고른 코퍼스만으로는
    /// 부족하다. 결정적 LCG로 만든 잡음 문서 수천 개로도 고정한다.
    #[test]
    fn round_trips_pseudo_random_documents() {
        const TOKENS: [&str; 22] = [
            "Host a", "Host a b", "Host *", "host=x", "Match host *.dev", "  HostName 1.2.3.4",
            "\tUser=deploy", "  Port = 22", "IdentityFile \"~/my keys/id\"", "# 주석",
            "", "   ", "\t", "Include ~/other/*.conf", "  ProxyJump a,b", "garbage",
            "  Key value # 꼬리 주석", "  Key \"quoted # not comment\"", "  =leading-eq",
            "\u{ac00}\u{b098}\u{b2e4} \u{b77c}\u{b9c8}", "  ControlPath ~/.ssh/cm-%r@%h:%p", "Host \"sp ace\"",
        ];
        let mut seed: u64 = 0x5eed_1234;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..2000 {
            let lines = next() % 12;
            let crlf = next() % 2 == 0;
            let eol = if crlf { "\r\n" } else { "\n" };
            let mut text = String::new();
            for _ in 0..lines {
                text.push_str(TOKENS[next() % TOKENS.len()]);
                text.push_str(eol);
            }
            if next() % 3 == 0 {
                text.push_str(TOKENS[next() % TOKENS.len()]); // 개행 없는 마지막 줄
            }
            assert_eq!(Document::parse(&text).to_string(), text, "왕복 실패: {text:?}");
        }
    }

    #[test]
    fn parses_patterns_and_directives() {
        let doc = Document::parse(REALISTIC);
        let aliases = doc.aliases();
        assert!(aliases.contains(&"web".to_string()));
        assert!(aliases.contains(&"prod-web".to_string()));
        assert!(aliases.contains(&"db".to_string()));
        assert!(!aliases.contains(&"*".to_string()));
        let db = doc.host("db").unwrap();
        assert_eq!(db.get("hostname"), Some("10.0.0.2"));
        assert_eq!(db.get("port"), Some("2222")); // `Port = 2222`
        assert_eq!(db.get("controlmaster"), Some("auto"));
        let web = doc.host("web").unwrap();
        assert_eq!(web.get("identityfile"), Some("~/my keys/id_ed25519")); // 따옴표+후행주석
        assert_eq!(doc.includes(), vec!["~/.ssh/conf.d/*.conf".to_string()]);
        assert_eq!(doc.hosts().len(), 3);
    }

    #[test]
    fn editing_one_directive_changes_exactly_one_line() {
        let input = REALISTIC;
        let mut doc = Document::parse(input);
        let db = doc.host("db").unwrap();
        let spec = HostSpec {
            host_name: Some("10.0.0.99".into()),
            port: Some(2222),
            user: None,
            proxy_jump: None,
            identity_file: None,
        };
        assert_eq!(db.get("user"), None);
        doc.upsert_host("db", &spec);
        let out = doc.to_string();
        let before: Vec<&str> = input.lines().collect();
        let after: Vec<&str> = out.lines().collect();
        assert_eq!(before.len(), after.len());
        let diff: Vec<(usize, &str, &str)> = before
            .iter()
            .zip(after.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i, *a, *b))
            .collect();
        assert_eq!(diff.len(), 1, "정확히 한 줄만 달라야 한다: {diff:?}");
        assert_eq!(diff[0].1, "  HostName=10.0.0.2");
        assert_eq!(diff[0].2, "  HostName=10.0.0.99"); // `=` 스타일·들여쓰기 유지
    }

    #[test]
    fn keeps_unknown_directives_and_comments_when_upserting() {
        let mut doc = Document::parse(REALISTIC);
        doc.upsert_host(
            "db",
            &HostSpec {
                host_name: Some("10.0.0.2".into()),
                port: Some(2222),
                user: Some("dba".into()),
                proxy_jump: Some("bastion".into()),
                identity_file: Some("/keys/id_db".into()),
            },
        );
        let out = doc.to_string();
        assert!(out.contains("  ControlMaster auto"));
        assert!(out.contains("  ControlPath ~/.ssh/cm-%r@%h:%p"));
        assert!(out.contains("# ~/.ssh/config — 손으로 관리하는 파일"));
        assert!(out.contains("Include ~/.ssh/conf.d/*.conf"));
        assert!(out.contains("\tIdentityFile \"~/my keys/id_ed25519\"   # 주 키"));
        assert!(out.contains("Match host *.internal exec \"true\""));
        assert!(out.contains("  # match 안 주석"));
        // 새 줄은 블록의 지배적 들여쓰기(2칸)를 따르고 마지막 지시어 뒤에 온다.
        assert!(out.contains("  User dba\n"));
        assert!(out.contains("  ProxyJump bastion\n"));
        assert!(out.contains("  IdentityFile /keys/id_db\n"));
    }

    #[test]
    fn inserts_new_blocks_before_the_first_wildcard_block() {
        let mut doc = Document::parse("Host a\n  User u\n\nHost *\n  ServerAliveInterval 60\n");
        doc.upsert_host("fresh", &HostSpec { host_name: Some("h".into()), ..Default::default() });
        let out = doc.to_string();
        let fresh = out.find("Host fresh").unwrap();
        let wild = out.find("Host *").unwrap();
        assert!(fresh < wild, "구체 블록이 Host * 뒤에 오면 통째로 가려진다:\n{out}");
        assert!(out.contains("Host fresh\n    HostName h\n"));
    }

    #[test]
    fn appends_at_end_of_file_when_there_is_no_wildcard_block() {
        let mut doc = Document::parse("Host a\n  User u\n");
        doc.upsert_host("z", &HostSpec { host_name: Some("h".into()), ..Default::default() });
        assert_eq!(doc.to_string(), "Host a\n  User u\n\nHost z\n    HostName h\n");
    }

    #[test]
    fn appends_a_newline_first_when_the_file_has_no_trailing_newline() {
        let mut doc = Document::parse("Host a\n  User u");
        doc.upsert_host("z", &HostSpec { user: Some("v".into()), ..Default::default() });
        assert_eq!(doc.to_string(), "Host a\n  User u\n\nHost z\n    User v\n");
    }

    #[test]
    fn writes_crlf_lines_into_a_crlf_file() {
        let mut doc = Document::parse("Host a\r\n\tUser u\r\n");
        doc.upsert_host("z", &HostSpec { user: Some("v".into()), ..Default::default() });
        let out = doc.to_string();
        assert!(out.ends_with("Host z\r\n    User v\r\n"), "{out:?}");
        assert!(out.split_inclusive('\n').all(|l| l.ends_with("\r\n")), "개행 스타일이 섞였다: {out:?}");
    }

    #[test]
    fn never_removes_lines_the_app_has_no_value_for() {
        // "앱이 이 키를 안 쓴다"는 "이 호스트에 이 키가 없어야 한다"가 아니다.
        // 비밀번호 인증 서버 때문에 사용자의 IdentityFile이 지워지던 실제 버그.
        let input = "Host a\n  HostName h\n  ProxyJump keep\n  IdentityFile ~/.ssh/id_rsa\n  Compression yes\n";
        let mut doc = Document::parse(input);
        doc.upsert_host("a", &HostSpec { host_name: Some("h".into()), ..Default::default() });
        assert_eq!(doc.to_string(), input, "값이 없다고 사용자 줄을 지우면 안 된다");
    }

    #[test]
    fn keeps_extra_lines_of_an_owned_key() {
        // IdentityFile을 여러 줄 두면 OpenSSH가 순서대로 시도한다 — 정상 설정이다.
        let mut doc = Document::parse(
            "Host a\n  IdentityFile ~/.ssh/one\n  IdentityFile ~/.ssh/two\n  X keep\n",
        );
        doc.upsert_host(
            "a",
            &HostSpec { identity_file: Some("~/.ssh/app".into()), ..Default::default() },
        );
        assert_eq!(
            doc.to_string(),
            "Host a\n  IdentityFile ~/.ssh/app\n  IdentityFile ~/.ssh/two\n  X keep\n",
            "첫 줄만 갱신하고 나머지는 그대로 둔다"
        );
    }

    #[test]
    fn updates_the_first_line_of_an_owned_key() {
        let mut doc = Document::parse("Host a\n  User one\n  User two\n  X keep\n");
        doc.upsert_host("a", &HostSpec { user: Some("three".into()), ..Default::default() });
        assert_eq!(doc.to_string(), "Host a\n  User three\n  User two\n  X keep\n");
    }

    #[test]
    fn never_writes_a_port_22_line() {
        let mut doc = Document::parse("");
        doc.upsert_host("a", &HostSpec { port: Some(22), user: Some("u".into()), ..Default::default() });
        assert_eq!(doc.to_string(), "Host a\n    User u\n");
    }

    #[test]
    fn refuses_to_touch_multi_pattern_and_wildcard_blocks() {
        let input = "Host a b c\n  User u\n\nHost *.dev\n  User d\n";
        for alias in ["a", "b", "c", "*.dev"] {
            let mut doc = Document::parse(input);
            doc.upsert_host(alias, &HostSpec { user: Some("hacked".into()), ..Default::default() });
            assert_eq!(doc.to_string(), input, "{alias} 블록은 읽기 전용이어야 한다");
            assert!(!doc.rename_host(alias, "renamed"));
            assert!(!doc.remove_host(alias));
            assert_eq!(doc.to_string(), input);
        }
    }

    #[test]
    fn refuses_aliases_that_cannot_be_written_back() {
        let mut doc = Document::parse("");
        for alias in ["", "a*", "a,b", "we\"ird", "\n"] {
            doc.upsert_host(alias, &HostSpec { user: Some("u".into()), ..Default::default() });
        }
        assert_eq!(doc.to_string(), "");
    }

    #[test]
    fn quotes_aliases_and_values_containing_spaces() {
        let mut doc = Document::parse("");
        doc.upsert_host(
            "my server",
            &HostSpec { identity_file: Some("/keys/my key".into()), ..Default::default() },
        );
        assert_eq!(doc.to_string(), "Host \"my server\"\n    IdentityFile \"/keys/my key\"\n");
        // 왕복: 따옴표를 벗겨 같은 별칭으로 다시 찾을 수 있어야 한다.
        let again = Document::parse(&doc.to_string());
        assert!(again.host("my server").is_some());
    }

    #[test]
    fn strips_control_characters_so_a_crafted_value_cannot_inject_a_directive() {
        let mut doc = Document::parse("");
        doc.upsert_host(
            "evil",
            &HostSpec {
                host_name: Some("h\n    ProxyCommand touch /tmp/pwned".into()),
                ..Default::default()
            },
        );
        let out = doc.to_string();
        assert!(!out.lines().any(|l| l.trim_start().starts_with("ProxyCommand")));
        assert_eq!(out.lines().filter(|l| l.starts_with("Host ")).count(), 1);
        assert_eq!(Document::parse(&out).to_string(), out);
    }

    #[test]
    fn renames_only_the_header_of_a_single_pattern_block() {
        let mut doc = Document::parse("Host old\n  HostName h\n  # 주석\n");
        assert!(doc.rename_host("old", "new"));
        assert_eq!(doc.to_string(), "Host new\n  HostName h\n  # 주석\n");
        assert!(!doc.rename_host("missing", "x"));
        // 이미 있는 별칭으로는 바꾸지 않는다.
        doc.upsert_host("other", &HostSpec { user: Some("u".into()), ..Default::default() });
        assert!(!doc.rename_host("new", "other"));
    }

    #[test]
    fn remove_host_keeps_trailing_comments_that_may_belong_to_the_next_block() {
        let mut doc = Document::parse("Host a\n  User u\n\n# b를 설명하는 주석\nHost b\n  User v\n");
        assert!(doc.remove_host("a"));
        assert_eq!(doc.to_string(), "\n# b를 설명하는 주석\nHost b\n  User v\n");
        assert!(!doc.remove_host("a"));
    }

    #[test]
    fn remove_host_drops_a_block_whose_tail_is_only_blank_lines() {
        let mut doc = Document::parse("Host a\n  User u\n\nHost b\n  User v\n");
        assert!(doc.remove_host("a"));
        assert_eq!(doc.to_string(), "Host b\n  User v\n");
    }

    #[test]
    fn sanitize_config_value_drops_control_chars_but_keeps_unicode() {
        assert_eq!(sanitize_config_value("a\r\nb\tc\u{7f}"), "abc");
        assert_eq!(sanitize_config_value("한글-ok"), "한글-ok");
    }
}
