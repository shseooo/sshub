//! 타입 정의 — TS `src/types/terminal.ts` 1:1 대응.
//!
//! 직렬화는 TS `TerminalContext.tsx`의 `SavedNode` 영속 포맷을 그대로 따른다:
//! - leaf: `{"type":"leaf","sessionId":..,"serverId":..,"label":..}`
//!   (`cwd_from_session`은 transient — 직렬화하지 않음)
//! - split: `{"type":"split","direction":"row"|"column","sizes":[..],"children":[..]}`
//!   (split `id`는 transient — 직렬화하지 않고 복원 시 [`crate::revive_ids`]로 재생성)
//!
//! 모든 id는 외부에서 생성해 주입한다(이 크레이트는 uuid 미의존, 순수 로직만).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(SessionId, "PTY 세션 id — 이벤트 채널 접미사로도 쓰인다.");
string_id!(SplitId, "Split 컨테이너 id — transient, 영속화하지 않는다.");
string_id!(TabId, "탭 id — transient, 영속화하지 않는다.");

/// 분할 방향. `row` = 좌우 나란히, `column` = 상하 스택.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Row,
    Column,
}

/// 단일 터미널 세션 pane (트리 leaf). TS `TerminalLeaf`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLeaf {
    /// 구버전 레이아웃에는 sessionId가 없을 수 있어 default("") 허용 —
    /// 복원 후 [`crate::revive_ids`]가 채운다 (TS `reviveNode`의 `?? uid()` 대응).
    #[serde(default)]
    pub session_id: SessionId,
    pub server_id: Option<i64>,
    pub label: String,
    /// Transient (영속화 안 함): 로컬 pane 분할 직후 원본 pane의 세션 id.
    /// 새 로컬 셸이 그 pane의 cwd에서 시작하며, 첫 세션 시작 시 1회 소비된다.
    #[serde(skip)]
    pub cwd_from_session: Option<SessionId>,
}

impl TerminalLeaf {
    pub fn new(
        session_id: impl Into<SessionId>,
        server_id: Option<i64>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            server_id,
            label: label.into(),
            cwd_from_session: None,
        }
    }
}

/// 자식 pane들을 나란히('row')/스택('column')으로 담는 split 컨테이너. TS `TerminalSplit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSplit {
    /// Transient — 영속화하지 않고 복원 시 재생성 (TS `SavedNode`에 id 없음).
    #[serde(skip)]
    pub id: SplitId,
    pub direction: SplitDirection,
    /// 자식별 크기(%) — 합이 ~100.
    pub sizes: Vec<f32>,
    pub children: Vec<PaneNode>,
}

/// pane 트리 노드. 직렬화 태그는 TS와 동일하게 `"type": "leaf" | "split"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PaneNode {
    Leaf(TerminalLeaf),
    Split(TerminalSplit),
}

/// 터미널 탭. 직렬화 형태는 TS `SavedTab` (`{root, name?}` — 탭 id는 transient).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalTab {
    #[serde(skip)]
    pub id: TabId,
    /// pane 트리 루트 — 단독 leaf 또는 중첩 split.
    pub root: PaneNode,
    /// 커스텀 탭 이름; 없으면 첫 leaf의 label로 표시한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// pane drop 위치 → 분할 방향/삽입 위치 매핑. TS `DropSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl DropSide {
    /// left/right ⇒ Row, top/bottom ⇒ Column (TS movePane/mergeTab의 `dir`).
    pub fn direction(self) -> SplitDirection {
        match self {
            DropSide::Left | DropSide::Right => SplitDirection::Row,
            DropSide::Top | DropSide::Bottom => SplitDirection::Column,
        }
    }

    /// left/top ⇒ 대상 앞(왼쪽/위)에 삽입 (TS의 `before`).
    pub fn before(self) -> bool {
        matches!(self, DropSide::Left | DropSide::Top)
    }
}
