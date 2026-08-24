//! 탭 배열 순수 연산 — TS `src/lib/tabOps.ts` 포팅 + `TerminalContext.tsx`의
//! 워크스페이스 합성 연산(closeTab 폴백/mergeTab/detachPane).

use crate::tree::{insert_at, leaves, remove_leaf};
use crate::types::*;

/// `tab_id` 탭을 삽입 경계 `to_index`(0..=len, **원본 배열 기준**)로 이동.
/// 드래그된 탭 제거 후 뒤쪽 요소가 한 칸 당겨지므로 from < to_index → to_index-1.
/// 범위 밖 인덱스는 clamp. 미존재 id는 그대로 반환.
pub fn reorder_tabs(tabs: Vec<TerminalTab>, tab_id: &TabId, to_index: usize) -> Vec<TerminalTab> {
    let from = match tabs.iter().position(|t| t.id == *tab_id) {
        Some(i) => i,
        None => return tabs,
    };
    let mut arr = tabs;
    let moved = arr.remove(from);
    let idx = if from < to_index { to_index - 1 } else { to_index };
    let idx = idx.min(arr.len());
    arr.insert(idx, moved);
    arr
}

/// `tab_id`만 남긴다 (나머지 전부 닫기).
pub fn tabs_except(tabs: Vec<TerminalTab>, tab_id: &TabId) -> Vec<TerminalTab> {
    tabs.into_iter().filter(|t| t.id == *tab_id).collect()
}

/// `tab_id`까지(포함) 남긴다 (오른쪽 전부 닫기). 미존재 id는 그대로 반환.
pub fn tabs_up_to_inclusive(tabs: Vec<TerminalTab>, tab_id: &TabId) -> Vec<TerminalTab> {
    let idx = match tabs.iter().position(|t| t.id == *tab_id) {
        Some(i) => i,
        None => return tabs,
    };
    let mut arr = tabs;
    arr.truncate(idx + 1);
    arr
}

/// `tab_id`부터(포함) 남긴다 (왼쪽 전부 닫기). 미존재 id는 그대로 반환.
pub fn tabs_from_inclusive(tabs: Vec<TerminalTab>, tab_id: &TabId) -> Vec<TerminalTab> {
    let idx = match tabs.iter().position(|t| t.id == *tab_id) {
        Some(i) => i,
        None => return tabs,
    };
    let mut arr = tabs;
    arr.drain(..idx);
    arr
}

/// `item`을 경계 `at_index`에 삽입(clamp); `None`이면 맨 뒤에 추가.
pub fn insert_at_index<T>(mut arr: Vec<T>, item: T, at_index: Option<usize>) -> Vec<T> {
    let i = at_index.unwrap_or(arr.len()).min(arr.len());
    arr.insert(i, item);
    arr
}

/// 탭 닫기 + active 폴백. 닫힌 탭이 active였으면 **마지막 탭**으로 폴백
/// (남은 탭이 없으면 `None`), 아니면 active 유지. TS `closeTab`.
pub fn close_tab(
    tabs: Vec<TerminalTab>,
    active: Option<&TabId>,
    tab_id: &TabId,
) -> (Vec<TerminalTab>, Option<TabId>) {
    let next: Vec<TerminalTab> = tabs.into_iter().filter(|t| t.id != *tab_id).collect();
    let next_active = if active == Some(tab_id) {
        next.last().map(|t| t.id.clone())
    } else {
        active.cloned()
    };
    (next, next_active)
}

/// 탭 전체를 다른 탭으로 병합: src 탭을 제거하고 그 pane 트리 전체를 dst 탭의
/// `dst_pane_id` 옆에 `side` 방향으로 graft. src==dst 또는 src 미존재 → 그대로.
/// active 전환(→ dst)은 호출자 책임. TS `mergeTab`.
///
/// 주의(원본 TS와 동일): dst 탭/대상 pane이 없으면 src 탭만 제거된 채 트리가
/// 유실된다 — 호출자는 dst 존재를 보장해야 한다.
pub fn merge_tab(
    tabs: Vec<TerminalTab>,
    src_tab_id: &TabId,
    dst_tab_id: &TabId,
    dst_pane_id: &SessionId,
    side: DropSide,
    new_split_id: &SplitId,
) -> Vec<TerminalTab> {
    if src_tab_id == dst_tab_id {
        return tabs;
    }
    let src_pos = match tabs.iter().position(|t| t.id == *src_tab_id) {
        Some(i) => i,
        None => return tabs,
    };
    let mut arr = tabs;
    let src = arr.remove(src_pos);
    if let Some(dst_pos) = arr.iter().position(|t| t.id == *dst_tab_id) {
        let dst = arr.remove(dst_pos);
        let root = insert_at(
            dst.root,
            dst_pane_id,
            src.root,
            side.direction(),
            side.before(),
            new_split_id,
        );
        arr.insert(dst_pos, TerminalTab { root, ..dst });
    }
    arr
}

/// pane을 떼어내 새 탭으로 (탭바로 드래그). leaf가 2개 이상일 때만 동작하며
/// (1개면 이미 단독 탭 — 그대로 반환), 새 탭은 경계 `at_index`에 삽입된다
/// (소스 탭은 pane을 하나 잃고 남는다). 새 탭 id는 외부에서 주입.
/// active 전환(→ 새 탭)은 호출자 책임. TS `detachPane`.
pub fn detach_pane(
    tabs: Vec<TerminalTab>,
    src_tab_id: &TabId,
    session_id: &SessionId,
    at_index: Option<usize>,
    new_tab_id: TabId,
) -> Vec<TerminalTab> {
    let leaf = {
        let src = match tabs.iter().find(|t| t.id == *src_tab_id) {
            Some(t) => t,
            None => return tabs,
        };
        let all = leaves(&src.root);
        if all.len() <= 1 {
            return tabs; // 이미 단독 탭
        }
        match all.into_iter().find(|l| l.session_id == *session_id) {
            Some(l) => l.clone(),
            None => return tabs,
        }
    };
    let collapsed: Vec<TerminalTab> = tabs
        .into_iter()
        .filter_map(|t| {
            if t.id != *src_tab_id {
                return Some(t);
            }
            // detach는 pane ≥2를 요구하므로 소스 탭은 항상 ≥1개 pane으로 남는다.
            remove_leaf(t.root, session_id).map(|root| TerminalTab { root, ..t })
        })
        .collect();
    insert_at_index(
        collapsed,
        TerminalTab {
            id: new_tab_id,
            root: PaneNode::Leaf(leaf),
            name: None,
        },
        at_index,
    )
}
