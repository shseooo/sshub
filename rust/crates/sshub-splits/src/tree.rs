//! pane 트리 순수 연산 — TS `src/contexts/TerminalContext.tsx`의
//! leaves/splitAt/removeLeaf/insertAt/reconnectLeaf/reconnectAll/setSizesAt/
//! renameLeaf/movePane 시맨틱을 정확히 포팅.

use crate::types::*;

/// `even(n)` — 자식 n개 균등 분배 (각 100/n %).
pub(crate) fn even(n: usize) -> Vec<f32> {
    vec![100.0 / n as f32; n]
}

/// 트리의 leaf들을 좌→우(depth-first) 순서로 수집.
pub fn leaves(node: &PaneNode) -> Vec<&TerminalLeaf> {
    match node {
        PaneNode::Leaf(l) => vec![l],
        PaneNode::Split(s) => s.children.iter().flat_map(leaves).collect(),
    }
}

/// 탭 표시 이름 규칙: `tab.name ?? 첫 leaf.label`.
pub fn tab_title(tab: &TerminalTab) -> &str {
    if let Some(name) = &tab.name {
        return name;
    }
    leaves(&tab.root)
        .first()
        .map(|l| l.label.as_str())
        .unwrap_or("")
}

fn contains_session(node: &PaneNode, session_id: &SessionId) -> bool {
    match node {
        PaneNode::Leaf(l) => l.session_id == *session_id,
        PaneNode::Split(s) => s.children.iter().any(|c| contains_session(c, session_id)),
    }
}

/// 대상 leaf를 `direction`으로 분할하고 그 뒤에 `addition`을 붙인다.
/// 같은 방향 split의 **직계** 자식이면 idx+1에 평평하게 삽입 + even(n) 재분배;
/// 아니면 중첩 split(50/50)을 만든다. 새 split id는 외부에서 주입한다
/// (중첩이 발생하지 않으면 사용되지 않는다).
pub fn split_at(
    node: PaneNode,
    session_id: &SessionId,
    direction: SplitDirection,
    addition: TerminalLeaf,
    new_split_id: &SplitId,
) -> PaneNode {
    match node {
        PaneNode::Leaf(leaf) => {
            if leaf.session_id != *session_id {
                return PaneNode::Leaf(leaf);
            }
            PaneNode::Split(TerminalSplit {
                id: new_split_id.clone(),
                direction,
                sizes: vec![50.0, 50.0],
                children: vec![PaneNode::Leaf(leaf), PaneNode::Leaf(addition)],
            })
        }
        PaneNode::Split(mut split) => {
            let direct_idx = split
                .children
                .iter()
                .position(|c| matches!(c, PaneNode::Leaf(l) if l.session_id == *session_id));
            if let Some(idx) = direct_idx {
                if split.direction == direction {
                    split.children.insert(idx + 1, PaneNode::Leaf(addition));
                    split.sizes = even(split.children.len());
                    return PaneNode::Split(split);
                }
            }
            // TS는 모든 자식에 재귀하지만 sessionId는 트리 내 유일하므로
            // 대상 서브트리에만 재귀해도 시맨틱이 동일하다.
            if let Some(pos) = split
                .children
                .iter()
                .position(|c| contains_session(c, session_id))
            {
                let child = split.children.remove(pos);
                let replaced = split_at(child, session_id, direction, addition, new_split_id);
                split.children.insert(pos, replaced);
            }
            PaneNode::Split(split)
        }
    }
}

/// leaf 제거. 마지막 leaf였으면 `None`(탭 자체가 닫힘), 자식 1개 남은 split은
/// 그 자식으로 collapse(연쇄), 생존 자식들의 sizes는 even(n)으로 재균등.
pub fn remove_leaf(node: PaneNode, session_id: &SessionId) -> Option<PaneNode> {
    match node {
        PaneNode::Leaf(l) => {
            if l.session_id == *session_id {
                None
            } else {
                Some(PaneNode::Leaf(l))
            }
        }
        PaneNode::Split(split) => {
            let TerminalSplit {
                id,
                direction,
                children,
                ..
            } = split;
            let mut kids: Vec<PaneNode> = children
                .into_iter()
                .filter_map(|c| remove_leaf(c, session_id))
                .collect();
            match kids.len() {
                0 => None,
                1 => kids.pop(), // 자식 1개 split은 그 자식으로 collapse
                n => Some(PaneNode::Split(TerminalSplit {
                    id,
                    direction,
                    sizes: even(n),
                    children: kids,
                })),
            }
        }
    }
}

/// `addition`(leaf 또는 서브트리 전체)을 dst leaf 옆에 `dir` 방향으로 삽입.
/// `before` = addition이 왼쪽/위로 간다. 같은 방향 split의 직계 자식이면
/// 평평하게 삽입(before ? idx : idx+1) + even(n); 아니면 중첩 split(50/50).
pub fn insert_at(
    node: PaneNode,
    dst_id: &SessionId,
    addition: PaneNode,
    dir: SplitDirection,
    before: bool,
    new_split_id: &SplitId,
) -> PaneNode {
    match node {
        PaneNode::Leaf(leaf) => {
            if leaf.session_id != *dst_id {
                return PaneNode::Leaf(leaf);
            }
            let children = if before {
                vec![addition, PaneNode::Leaf(leaf)]
            } else {
                vec![PaneNode::Leaf(leaf), addition]
            };
            PaneNode::Split(TerminalSplit {
                id: new_split_id.clone(),
                direction: dir,
                sizes: vec![50.0, 50.0],
                children,
            })
        }
        PaneNode::Split(mut split) => {
            let direct_idx = split
                .children
                .iter()
                .position(|c| matches!(c, PaneNode::Leaf(l) if l.session_id == *dst_id));
            if let Some(idx) = direct_idx {
                if split.direction == dir {
                    let at = if before { idx } else { idx + 1 };
                    split.children.insert(at, addition);
                    split.sizes = even(split.children.len());
                    return PaneNode::Split(split);
                }
            }
            if let Some(pos) = split
                .children
                .iter()
                .position(|c| contains_session(c, dst_id))
            {
                let child = split.children.remove(pos);
                let replaced = insert_at(child, dst_id, addition, dir, before, new_split_id);
                split.children.insert(pos, replaced);
            }
            PaneNode::Split(split)
        }
    }
}

/// 대상 leaf의 세션 id만 교체 (재접속). 새 id는 외부에서 주입.
pub fn reconnect_leaf(
    node: PaneNode,
    session_id: &SessionId,
    new_session_id: &SessionId,
) -> PaneNode {
    match node {
        PaneNode::Leaf(mut l) => {
            if l.session_id == *session_id {
                l.session_id = new_session_id.clone();
            }
            PaneNode::Leaf(l)
        }
        PaneNode::Split(mut s) => {
            s.children = s
                .children
                .into_iter()
                .map(|c| reconnect_leaf(c, session_id, new_session_id))
                .collect();
            PaneNode::Split(s)
        }
    }
}

/// 모든 leaf의 세션 id를 재발급 (탭 전체 재접속). 좌→우 depth-first 순서로 gen 호출.
pub fn reconnect_all(node: PaneNode, gen: &mut impl FnMut() -> SessionId) -> PaneNode {
    match node {
        PaneNode::Leaf(mut l) => {
            l.session_id = gen();
            PaneNode::Leaf(l)
        }
        PaneNode::Split(mut s) => {
            s.children = s
                .children
                .into_iter()
                .map(|c| reconnect_all(c, &mut *gen))
                .collect();
            PaneNode::Split(s)
        }
    }
}

/// 대상 leaf의 label 교체. (TS `renameLeaf` — trim 등 정규화는 호출자 책임,
/// TS `renamePane` 콜백이 trim/빈 문자열 거부를 수행한다.)
pub fn rename_leaf(node: PaneNode, session_id: &SessionId, label: &str) -> PaneNode {
    match node {
        PaneNode::Leaf(mut l) => {
            if l.session_id == *session_id {
                l.label = label.to_owned();
            }
            PaneNode::Leaf(l)
        }
        PaneNode::Split(mut s) => {
            s.children = s
                .children
                .into_iter()
                .map(|c| rename_leaf(c, session_id, label))
                .collect();
            PaneNode::Split(s)
        }
    }
}

fn find_split_mut<'a>(node: &'a mut PaneNode, split_id: &SplitId) -> Option<&'a mut TerminalSplit> {
    match node {
        PaneNode::Leaf(_) => None,
        PaneNode::Split(s) if s.id == *split_id => Some(s),
        PaneNode::Split(s) => s
            .children
            .iter_mut()
            .find_map(|c| find_split_mut(c, split_id)),
    }
}

/// `split_id`를 가진 split의 sizes 교체 (디바이더 드래그). TS `setSizesAt`.
pub fn set_split_sizes(mut node: PaneNode, split_id: &SplitId, sizes: Vec<f32>) -> PaneNode {
    if let Some(s) = find_split_mut(&mut node, split_id) {
        s.sizes = sizes;
    }
    node
}

/// pane을 다른 pane 옆으로 이동 (드래그 재배치). TS `movePane`:
/// src==dst 또는 src 미존재 → 그대로; src가 유일한 pane → 중단(그대로);
/// 그 외 src 제거 후 dst 옆에 `side` 방향으로 삽입.
///
/// 주의(원본 TS와 동일): dst가 트리에 없으면 삽입이 no-op이 되어 src pane이
/// 트리에서 사라진다 — 호출자는 dst 존재를 보장해야 한다.
pub fn move_pane(
    root: PaneNode,
    src_id: &SessionId,
    dst_id: &SessionId,
    side: DropSide,
    new_split_id: &SplitId,
) -> PaneNode {
    if src_id == dst_id {
        return root;
    }
    let src = match leaves(&root).into_iter().find(|l| l.session_id == *src_id) {
        Some(l) => l.clone(),
        None => return root,
    };
    let removed = match remove_leaf(root, src_id) {
        Some(n) => n,
        // src가 유일한 pane이었다면 여기 오지 않는다(위에서 src를 찾았고 트리가
        // 통째로 사라짐) — TS는 이 경우 원본을 반환하지만 root가 이미 move됐으므로
        // src 단독 leaf 트리를 복원한다. 시맨틱 동일: 변경 없음.
        None => return PaneNode::Leaf(src),
    };
    insert_at(
        removed,
        dst_id,
        PaneNode::Leaf(src),
        side.direction(),
        side.before(),
        new_split_id,
    )
}

/// 역직렬화 직후 transient id 복원: 비어 있는 leaf sessionId(구버전 레이아웃)와
/// 모든 비어 있는 split id를 주입된 생성기로 채운다 (TS `reviveNode` 대응 —
/// TS는 split id를 항상 재생성하지만 영속 포맷에 id가 없으므로 관측 결과 동일).
pub fn revive_ids(
    node: &mut PaneNode,
    gen_session: &mut impl FnMut() -> SessionId,
    gen_split: &mut impl FnMut() -> SplitId,
) {
    match node {
        PaneNode::Leaf(l) => {
            if l.session_id.is_empty() {
                l.session_id = gen_session();
            }
        }
        PaneNode::Split(s) => {
            if s.id.is_empty() {
                s.id = gen_split();
            }
            for c in &mut s.children {
                revive_ids(c, &mut *gen_session, &mut *gen_split);
            }
        }
    }
}
