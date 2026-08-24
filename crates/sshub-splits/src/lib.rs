//! sshub-splits — 분할 트리·탭 순수 연산 (rust/docs/DESIGN-terminal.md §5)
//!
//! TS 원본 시맨틱의 정확한 포팅:
//! - `src/types/terminal.ts` — PaneNode/TerminalLeaf/TerminalSplit/TerminalTab
//! - `src/contexts/TerminalContext.tsx` — 트리 연산 + SavedNode 영속 포맷
//! - `src/lib/tabOps.ts` — 탭 배열 연산
//!
//! id(세션/스플릿/탭)는 전부 외부에서 생성해 주입한다 — 이 크레이트는 순수 로직만.

mod tabs;
mod tree;
mod types;

pub use tabs::*;
pub use tree::*;
pub use types::*;

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> TerminalLeaf {
        TerminalLeaf::new(id, None, id)
    }

    fn lnode(id: &str) -> PaneNode {
        PaneNode::Leaf(leaf(id))
    }

    fn sid(s: &str) -> SessionId {
        SessionId::from(s)
    }

    fn spid(s: &str) -> SplitId {
        SplitId::from(s)
    }

    fn tid(s: &str) -> TabId {
        TabId::from(s)
    }

    fn ids(node: &PaneNode) -> Vec<&str> {
        leaves(node).into_iter().map(|l| l.session_id.as_str()).collect()
    }

    fn split(id: &str, direction: SplitDirection, sizes: Vec<f32>, children: Vec<PaneNode>) -> PaneNode {
        PaneNode::Split(TerminalSplit {
            id: spid(id),
            direction,
            sizes,
            children,
        })
    }

    fn as_split(node: &PaneNode) -> &TerminalSplit {
        match node {
            PaneNode::Split(s) => s,
            PaneNode::Leaf(_) => panic!("expected split, got leaf"),
        }
    }

    fn as_leaf(node: &PaneNode) -> &TerminalLeaf {
        match node {
            PaneNode::Leaf(l) => l,
            PaneNode::Split(_) => panic!("expected leaf, got split"),
        }
    }

    fn assert_sizes(sizes: &[f32], expected: &[f32]) {
        assert_eq!(sizes.len(), expected.len(), "sizes length");
        for (a, b) in sizes.iter().zip(expected) {
            assert!((a - b).abs() < 1e-4, "size {a} != expected {b}");
        }
    }

    // -------- leaves (TerminalContext.test.ts 포팅) --------

    #[test]
    fn leaves_returns_the_node_itself_for_a_lone_leaf() {
        assert_eq!(ids(&lnode("a")), ["a"]);
    }

    #[test]
    fn leaves_flattens_a_nested_split_tree_left_to_right() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![
                lnode("a"),
                split("s1", SplitDirection::Column, vec![50.0, 50.0], vec![lnode("b"), lnode("c")]),
            ],
        );
        assert_eq!(ids(&tree), ["a", "b", "c"]);
    }

    // -------- tab_title --------

    #[test]
    fn tab_title_prefers_the_custom_name() {
        let tab = TerminalTab {
            id: tid("t1"),
            root: lnode("a"),
            name: Some("custom".into()),
        };
        assert_eq!(tab_title(&tab), "custom");
    }

    #[test]
    fn tab_title_falls_back_to_the_first_leaf_label() {
        let tab = TerminalTab {
            id: tid("t1"),
            root: split(
                "s0",
                SplitDirection::Row,
                vec![50.0, 50.0],
                vec![lnode("first"), lnode("second")],
            ),
            name: None,
        };
        assert_eq!(tab_title(&tab), "first");
    }

    // -------- split_at (TerminalContext.test.ts 포팅) --------

    #[test]
    fn split_at_wraps_a_lone_leaf_in_a_split_with_the_addition_after_it() {
        let out = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("new"));
        let s = as_split(&out);
        assert_eq!(ids(&out), ["a", "b"]);
        assert_eq!(s.direction, SplitDirection::Row);
        assert_eq!(s.id, spid("new"));
        assert_sizes(&s.sizes, &[50.0, 50.0]);
    }

    #[test]
    fn split_at_adds_a_flat_sibling_when_the_parent_split_has_the_same_direction() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = split_at(row, &sid("b"), SplitDirection::Row, leaf("c"), &spid("s1"));
        let s = as_split(&out);
        assert_eq!(s.children.len(), 3);
        assert_eq!(ids(&out), ["a", "b", "c"]);
        // 기존 split id 유지, 새 id는 미사용
        assert_eq!(s.id, spid("s0"));
    }

    #[test]
    fn split_at_nests_when_splitting_in_the_cross_direction() {
        // 회귀: 오른쪽 분할(row) 후 같은 pane 아래 분할(column)은 row로
        // 평평해지지 않고 중첩 column이어야 한다.
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = split_at(row, &sid("b"), SplitDirection::Column, leaf("c"), &spid("s1"));
        let outer = as_split(&out);
        assert_eq!(outer.direction, SplitDirection::Row);
        assert_eq!(outer.children.len(), 2);
        let second = as_split(&outer.children[1]);
        assert_eq!(second.direction, SplitDirection::Column);
        assert_eq!(second.id, spid("s1"));
        assert_eq!(ids(&outer.children[1]), ["b", "c"]);
    }

    #[test]
    fn split_at_flattens_only_at_the_direct_child_level_of_a_same_direction_split() {
        // col[ row[a,b], c ] 에서 'a'를 column 분할 → 'a'는 바깥 column의 직계
        // 자식이 아니므로 평탄화되지 않고 row 안에서 중첩 column이 된다.
        let tree = split(
            "outer",
            SplitDirection::Column,
            vec![50.0, 50.0],
            vec![
                split("inner", SplitDirection::Row, vec![50.0, 50.0], vec![lnode("a"), lnode("b")]),
                lnode("c"),
            ],
        );
        let out = split_at(tree, &sid("a"), SplitDirection::Column, leaf("n"), &spid("new"));
        let outer = as_split(&out);
        assert_eq!(outer.children.len(), 2, "outer column must not flatten");
        let inner = as_split(&outer.children[0]);
        assert_eq!(inner.direction, SplitDirection::Row);
        assert_eq!(inner.children.len(), 2);
        let nested = as_split(&inner.children[0]);
        assert_eq!(nested.direction, SplitDirection::Column);
        assert_eq!(ids(&inner.children[0]), ["a", "n"]);
    }

    #[test]
    fn split_at_flattens_a_direct_child_of_the_same_direction_split() {
        // col[ row[a,b], c ] 에서 'c'를 column 분할 → 바깥 column의 직계 자식이므로
        // 평평하게 3자식 + even(3).
        let tree = split(
            "outer",
            SplitDirection::Column,
            vec![70.0, 30.0],
            vec![
                split("inner", SplitDirection::Row, vec![50.0, 50.0], vec![lnode("a"), lnode("b")]),
                lnode("c"),
            ],
        );
        let out = split_at(tree, &sid("c"), SplitDirection::Column, leaf("n"), &spid("new"));
        let outer = as_split(&out);
        assert_eq!(outer.children.len(), 3);
        assert_eq!(ids(&out), ["a", "b", "c", "n"]);
        assert_sizes(&outer.sizes, &[100.0 / 3.0, 100.0 / 3.0, 100.0 / 3.0]);
    }

    #[test]
    fn split_at_re_evens_custom_sizes_on_flat_insert() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![20.0, 80.0],
            vec![lnode("a"), lnode("b")],
        );
        let out = split_at(tree, &sid("a"), SplitDirection::Row, leaf("c"), &spid("new"));
        assert_sizes(&as_split(&out).sizes, &[100.0 / 3.0, 100.0 / 3.0, 100.0 / 3.0]);
        assert_eq!(ids(&out), ["a", "c", "b"]); // idx+1 삽입
    }

    #[test]
    fn split_at_returns_the_node_unchanged_for_an_unknown_session() {
        let out = split_at(lnode("a"), &sid("zzz"), SplitDirection::Row, leaf("b"), &spid("new"));
        assert_eq!(out, lnode("a"));
    }

    // -------- remove_leaf (TerminalContext.test.ts 포팅) --------

    #[test]
    fn remove_leaf_returns_none_when_the_only_leaf_is_removed() {
        assert_eq!(remove_leaf(lnode("a"), &sid("a")), None);
    }

    #[test]
    fn remove_leaf_collapses_a_single_child_split_into_that_child() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = remove_leaf(row, &sid("a")).expect("tree survives");
        assert_eq!(as_leaf(&out).session_id, sid("b"));
    }

    #[test]
    fn remove_leaf_keeps_siblings_when_one_of_three_is_removed() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let row = split_at(row, &sid("b"), SplitDirection::Row, leaf("c"), &spid("s1"));
        let out = remove_leaf(row, &sid("b")).expect("tree survives");
        assert_eq!(ids(&out), ["a", "c"]);
    }

    #[test]
    fn remove_leaf_re_evens_survivor_sizes() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![20.0, 30.0, 50.0],
            vec![lnode("a"), lnode("b"), lnode("c")],
        );
        let out = remove_leaf(tree, &sid("b")).expect("tree survives");
        assert_sizes(&as_split(&out).sizes, &[50.0, 50.0]);
    }

    #[test]
    fn remove_leaf_collapse_cascades_through_nested_splits() {
        // col[ row[b,c], a ] 에서 b 제거 → row가 c로 collapse → col[c, a]
        let tree = split(
            "outer",
            SplitDirection::Column,
            vec![60.0, 40.0],
            vec![
                split("inner", SplitDirection::Row, vec![50.0, 50.0], vec![lnode("b"), lnode("c")]),
                lnode("a"),
            ],
        );
        let out = remove_leaf(tree, &sid("b")).expect("tree survives");
        let outer = as_split(&out);
        assert_eq!(outer.direction, SplitDirection::Column);
        assert_eq!(ids(&out), ["c", "a"]);
        assert_eq!(as_leaf(&outer.children[0]).session_id, sid("c"));
        assert_sizes(&outer.sizes, &[50.0, 50.0]); // 생존자 재균등
    }

    #[test]
    fn remove_leaf_collapses_the_root_to_a_leaf_and_then_to_none() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = remove_leaf(row, &sid("b")).expect("tree survives");
        assert_eq!(out, lnode("a"));
        assert_eq!(remove_leaf(out, &sid("a")), None);
    }

    #[test]
    fn remove_leaf_ignores_an_unknown_session() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = remove_leaf(row.clone(), &sid("zzz")).expect("tree survives");
        assert_eq!(out, row);
    }

    // -------- insert_at (TerminalContext.test.ts 포팅) --------

    #[test]
    fn insert_at_splits_a_leaf_placing_the_addition_before_when_requested() {
        let out = insert_at(lnode("a"), &sid("a"), lnode("b"), SplitDirection::Column, true, &spid("new"));
        assert_eq!(ids(&out), ["b", "a"]);
        assert_eq!(as_split(&out).direction, SplitDirection::Column);
    }

    #[test]
    fn insert_at_grafts_a_whole_subtree_next_to_the_target() {
        let subtree = split_at(lnode("x"), &sid("x"), SplitDirection::Row, leaf("y"), &spid("sub"));
        let out = insert_at(lnode("a"), &sid("a"), subtree, SplitDirection::Column, false, &spid("new"));
        assert_eq!(as_split(&out).direction, SplitDirection::Column);
        assert_eq!(ids(&out), ["a", "x", "y"]);
    }

    #[test]
    fn insert_at_flat_inserts_before_the_target_in_a_same_direction_split() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("a"), lnode("b")],
        );
        let out = insert_at(tree, &sid("b"), lnode("x"), SplitDirection::Row, true, &spid("new"));
        let s = as_split(&out);
        assert_eq!(ids(&out), ["a", "x", "b"]); // before ⇒ idx 위치
        assert_eq!(s.id, spid("s0"));
        assert_sizes(&s.sizes, &[100.0 / 3.0, 100.0 / 3.0, 100.0 / 3.0]);
    }

    #[test]
    fn insert_at_nests_in_the_cross_direction_of_the_parent_split() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("a"), lnode("b")],
        );
        let out = insert_at(tree, &sid("b"), lnode("x"), SplitDirection::Column, false, &spid("new"));
        let outer = as_split(&out);
        assert_eq!(outer.children.len(), 2);
        let nested = as_split(&outer.children[1]);
        assert_eq!(nested.direction, SplitDirection::Column);
        assert_eq!(ids(&outer.children[1]), ["b", "x"]);
    }

    // -------- reconnect / rename / set_split_sizes --------

    #[test]
    fn reconnect_leaf_replaces_only_the_target_session_id() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = reconnect_leaf(row, &sid("a"), &sid("a2"));
        assert_eq!(ids(&out), ["a2", "b"]);
    }

    #[test]
    fn reconnect_all_reissues_every_session_id_in_order() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![
                lnode("a"),
                split("s1", SplitDirection::Column, vec![50.0, 50.0], vec![lnode("b"), lnode("c")]),
            ],
        );
        let mut n = 0;
        let out = reconnect_all(tree, &mut || {
            n += 1;
            SessionId::from(format!("new-{n}"))
        });
        assert_eq!(ids(&out), ["new-1", "new-2", "new-3"]);
    }

    #[test]
    fn rename_leaf_changes_only_the_target_label() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = rename_leaf(row, &sid("b"), "renamed");
        let labels: Vec<&str> = leaves(&out).into_iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, ["a", "renamed"]);
    }

    #[test]
    fn set_split_sizes_targets_a_nested_split_by_id() {
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![
                lnode("a"),
                split("s1", SplitDirection::Column, vec![50.0, 50.0], vec![lnode("b"), lnode("c")]),
            ],
        );
        let out = set_split_sizes(tree, &spid("s1"), vec![30.0, 70.0]);
        let outer = as_split(&out);
        assert_sizes(&outer.sizes, &[50.0, 50.0]); // 바깥은 그대로
        assert_sizes(&as_split(&outer.children[1]).sizes, &[30.0, 70.0]);
    }

    #[test]
    fn set_split_sizes_ignores_an_unknown_split_id() {
        let tree = split("s0", SplitDirection::Row, vec![50.0, 50.0], vec![lnode("a"), lnode("b")]);
        let out = set_split_sizes(tree.clone(), &spid("zzz"), vec![10.0, 90.0]);
        assert_eq!(out, tree);
    }

    // -------- DropSide --------

    #[test]
    fn drop_side_maps_to_direction_and_before() {
        assert_eq!(DropSide::Left.direction(), SplitDirection::Row);
        assert_eq!(DropSide::Right.direction(), SplitDirection::Row);
        assert_eq!(DropSide::Top.direction(), SplitDirection::Column);
        assert_eq!(DropSide::Bottom.direction(), SplitDirection::Column);
        assert!(DropSide::Left.before());
        assert!(DropSide::Top.before());
        assert!(!DropSide::Right.before());
        assert!(!DropSide::Bottom.before());
    }

    // -------- move_pane --------

    #[test]
    fn move_pane_aborts_when_source_is_the_only_pane() {
        let out = move_pane(lnode("a"), &sid("a"), &sid("zzz"), DropSide::Right, &spid("new"));
        assert_eq!(out, lnode("a"));
    }

    #[test]
    fn move_pane_is_a_noop_when_src_equals_dst() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = move_pane(row.clone(), &sid("a"), &sid("a"), DropSide::Right, &spid("new"));
        assert_eq!(out, row);
    }

    #[test]
    fn move_pane_is_a_noop_for_an_unknown_source() {
        let row = split_at(lnode("a"), &sid("a"), SplitDirection::Row, leaf("b"), &spid("s0"));
        let out = move_pane(row.clone(), &sid("zzz"), &sid("a"), DropSide::Right, &spid("new"));
        assert_eq!(out, row);
    }

    #[test]
    fn move_pane_moves_a_pane_to_the_right_of_the_target() {
        // row[a,b,c] 에서 a를 c 오른쪽으로 → row[b,c,a]
        let tree = split(
            "s0",
            SplitDirection::Row,
            vec![100.0 / 3.0; 3],
            vec![lnode("a"), lnode("b"), lnode("c")],
        );
        let out = move_pane(tree, &sid("a"), &sid("c"), DropSide::Right, &spid("new"));
        assert_eq!(ids(&out), ["b", "c", "a"]);
        assert_eq!(as_split(&out).children.len(), 3); // 같은 방향 ⇒ 평평 유지
    }

    #[test]
    fn move_pane_nests_when_dropped_on_the_top_side() {
        // row[a,b] 에서 a를 b 위로 → column[a,b] (2개 pane이라 root collapse 후 재분할)
        let tree = split("s0", SplitDirection::Row, vec![50.0, 50.0], vec![lnode("a"), lnode("b")]);
        let out = move_pane(tree, &sid("a"), &sid("b"), DropSide::Top, &spid("new"));
        let s = as_split(&out);
        assert_eq!(s.direction, SplitDirection::Column);
        assert_eq!(ids(&out), ["a", "b"]);
        assert_eq!(s.id, spid("new"));
    }

    // -------- reorder_tabs (tabOps.test.ts 포팅) --------

    fn tabs(ids: &[&str]) -> Vec<TerminalTab> {
        ids.iter()
            .map(|id| TerminalTab {
                id: tid(id),
                root: lnode(id),
                name: None,
            })
            .collect()
    }

    fn tab_ids(tabs: &[TerminalTab]) -> Vec<&str> {
        tabs.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn reorder_moves_a_tab_forward_to_an_insertion_boundary() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b", "c"]), &tid("a"), 2)), ["b", "a", "c"]);
    }

    #[test]
    fn reorder_moves_a_tab_to_the_end() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b", "c"]), &tid("a"), 3)), ["b", "c", "a"]);
    }

    #[test]
    fn reorder_moves_a_tab_backward() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b", "c"]), &tid("c"), 0)), ["c", "a", "b"]);
    }

    #[test]
    fn reorder_is_a_noop_when_dropped_on_its_own_boundary() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b", "c"]), &tid("b"), 1)), ["a", "b", "c"]);
    }

    #[test]
    fn reorder_returns_the_array_unchanged_for_an_unknown_id() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b"]), &tid("zzz"), 0)), ["a", "b"]);
    }

    #[test]
    fn reorder_clamps_an_out_of_range_index() {
        assert_eq!(tab_ids(&reorder_tabs(tabs(&["a", "b", "c"]), &tid("a"), 99)), ["b", "c", "a"]);
    }

    // -------- tabs_except / tabs_up_to_inclusive (tabOps.test.ts 포팅) --------

    #[test]
    fn tabs_except_keeps_only_the_given_tab() {
        assert_eq!(tab_ids(&tabs_except(tabs(&["a", "b", "c"]), &tid("b"))), ["b"]);
    }

    #[test]
    fn tabs_up_to_inclusive_keeps_tabs_up_to_and_including_the_given_one() {
        assert_eq!(
            tab_ids(&tabs_up_to_inclusive(tabs(&["a", "b", "c", "d"]), &tid("b"))),
            ["a", "b"]
        );
    }

    #[test]
    fn tabs_up_to_inclusive_keeps_everything_when_the_target_is_last() {
        assert_eq!(tab_ids(&tabs_up_to_inclusive(tabs(&["a", "b"]), &tid("b"))), ["a", "b"]);
    }

    #[test]
    fn tabs_up_to_inclusive_returns_unchanged_for_an_unknown_id() {
        assert_eq!(tab_ids(&tabs_up_to_inclusive(tabs(&["a", "b"]), &tid("zzz"))), ["a", "b"]);
    }

    // -------- tabs_from_inclusive (왼쪽 전부 닫기) --------

    #[test]
    fn tabs_from_inclusive_keeps_the_given_tab_and_everything_after_it() {
        assert_eq!(
            tab_ids(&tabs_from_inclusive(tabs(&["a", "b", "c", "d"]), &tid("c"))),
            ["c", "d"]
        );
    }

    #[test]
    fn tabs_from_inclusive_keeps_everything_when_the_target_is_first() {
        assert_eq!(tab_ids(&tabs_from_inclusive(tabs(&["a", "b"]), &tid("a"))), ["a", "b"]);
    }

    #[test]
    fn tabs_from_inclusive_returns_unchanged_for_an_unknown_id() {
        assert_eq!(tab_ids(&tabs_from_inclusive(tabs(&["a", "b"]), &tid("zzz"))), ["a", "b"]);
    }

    /// 좌/우 닫기는 서로의 여집합 + 자기 자신이다 — 한쪽만 고치면 어긋난다.
    #[test]
    fn left_and_right_close_partition_the_strip_around_the_target() {
        let all = ["a", "b", "c", "d"];
        let kept_left = tabs_up_to_inclusive(tabs(&all), &tid("c"));
        let kept_right = tabs_from_inclusive(tabs(&all), &tid("c"));
        let left = tab_ids(&kept_left);
        let right = tab_ids(&kept_right);
        assert_eq!(left.len() + right.len(), all.len() + 1, "겹치는 건 대상 탭 하나뿐");
        assert_eq!(*left.last().unwrap(), "c");
        assert_eq!(*right.first().unwrap(), "c");
    }

    // -------- insert_at_index (tabOps.test.ts 포팅) --------

    #[test]
    fn insert_at_index_inserts_at_the_requested_boundary() {
        assert_eq!(insert_at_index(vec!["a", "b", "c"], "x", Some(1)), ["a", "x", "b", "c"]);
    }

    #[test]
    fn insert_at_index_appends_when_no_index_is_given() {
        assert_eq!(insert_at_index(vec!["a", "b"], "x", None), ["a", "b", "x"]);
    }

    #[test]
    fn insert_at_index_clamps_a_too_large_index_to_the_end() {
        assert_eq!(insert_at_index(vec!["a", "b"], "x", Some(99)), ["a", "b", "x"]);
    }

    #[test]
    fn insert_at_index_inserts_at_the_front() {
        assert_eq!(insert_at_index(vec!["a", "b"], "x", Some(0)), ["x", "a", "b"]);
    }

    // -------- close_tab (active 폴백) --------

    #[test]
    fn close_tab_falls_back_to_the_last_tab_when_the_active_one_closes() {
        let (next, active) = close_tab(tabs(&["a", "b", "c"]), Some(&tid("b")), &tid("b"));
        assert_eq!(tab_ids(&next), ["a", "c"]);
        assert_eq!(active, Some(tid("c")));
    }

    #[test]
    fn close_tab_keeps_the_active_tab_when_another_closes() {
        let (next, active) = close_tab(tabs(&["a", "b", "c"]), Some(&tid("a")), &tid("c"));
        assert_eq!(tab_ids(&next), ["a", "b"]);
        assert_eq!(active, Some(tid("a")));
    }

    #[test]
    fn close_tab_yields_no_active_when_the_last_tab_closes() {
        let (next, active) = close_tab(tabs(&["a"]), Some(&tid("a")), &tid("a"));
        assert!(next.is_empty());
        assert_eq!(active, None);
    }

    // -------- merge_tab --------

    #[test]
    fn merge_tab_grafts_the_whole_source_tree_next_to_the_target_pane() {
        let mut all = tabs(&["t1", "t2"]);
        all[0].root = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("x"), lnode("y")],
        );
        let out = merge_tab(all, &tid("t1"), &tid("t2"), &sid("t2"), DropSide::Bottom, &spid("new"));
        assert_eq!(tab_ids(&out), ["t2"]);
        let root = as_split(&out[0].root);
        assert_eq!(root.direction, SplitDirection::Column);
        assert_eq!(ids(&out[0].root), ["t2", "x", "y"]); // Bottom ⇒ 대상 뒤
    }

    #[test]
    fn merge_tab_is_a_noop_when_src_equals_dst() {
        let all = tabs(&["t1", "t2"]);
        let out = merge_tab(all.clone(), &tid("t1"), &tid("t1"), &sid("t1"), DropSide::Left, &spid("n"));
        assert_eq!(out, all);
    }

    #[test]
    fn merge_tab_is_a_noop_for_an_unknown_source_tab() {
        let all = tabs(&["t1", "t2"]);
        let out = merge_tab(all.clone(), &tid("zzz"), &tid("t2"), &sid("t2"), DropSide::Left, &spid("n"));
        assert_eq!(out, all);
    }

    // -------- detach_pane --------

    #[test]
    fn detach_pane_moves_the_pane_into_a_new_tab_at_the_boundary() {
        let mut all = tabs(&["t1", "t2"]);
        all[0].root = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("x"), lnode("y")],
        );
        let out = detach_pane(all, &tid("t1"), &sid("y"), Some(1), tid("t3"));
        assert_eq!(tab_ids(&out), ["t1", "t3", "t2"]);
        assert_eq!(ids(&out[0].root), ["x"]); // 소스는 collapse되어 남는다
        assert_eq!(ids(&out[1].root), ["y"]);
        assert_eq!(out[1].name, None);
    }

    #[test]
    fn detach_pane_appends_the_new_tab_when_no_index_is_given() {
        let mut all = tabs(&["t1", "t2"]);
        all[0].root = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("x"), lnode("y")],
        );
        let out = detach_pane(all, &tid("t1"), &sid("x"), None, tid("t3"));
        assert_eq!(tab_ids(&out), ["t1", "t2", "t3"]);
        assert_eq!(ids(&out[2].root), ["x"]);
    }

    #[test]
    fn detach_pane_aborts_when_the_tab_has_a_single_pane() {
        let all = tabs(&["t1", "t2"]);
        let out = detach_pane(all.clone(), &tid("t1"), &sid("t1"), Some(0), tid("t3"));
        assert_eq!(out, all);
    }

    #[test]
    fn detach_pane_aborts_for_an_unknown_session() {
        let mut all = tabs(&["t1"]);
        all[0].root = split(
            "s0",
            SplitDirection::Row,
            vec![50.0, 50.0],
            vec![lnode("x"), lnode("y")],
        );
        let out = detach_pane(all.clone(), &tid("t1"), &sid("zzz"), Some(0), tid("t3"));
        assert_eq!(out, all);
    }

    // -------- serde (SavedNode 영속 포맷) --------

    // TS serializeNode가 출력하는 JSON을 그대로 복사한 픽스처.
    const TS_SAVED_NODE: &str = r#"{"type":"split","direction":"row","sizes":[30,70],"children":[{"type":"leaf","sessionId":"s-1","serverId":null,"label":"local"},{"type":"split","direction":"column","sizes":[50,50],"children":[{"type":"leaf","sessionId":"s-2","serverId":3,"label":"prod"},{"type":"leaf","sessionId":"s-3","serverId":null,"label":"logs"}]}]}"#;

    #[test]
    fn serde_deserializes_the_verbatim_ts_saved_node_format() {
        let node: PaneNode = serde_json::from_str(TS_SAVED_NODE).expect("deserialize");
        let outer = as_split(&node);
        assert_eq!(outer.direction, SplitDirection::Row);
        assert!(outer.id.is_empty(), "split id is transient — not persisted");
        assert_sizes(&outer.sizes, &[30.0, 70.0]);
        assert_eq!(ids(&node), ["s-1", "s-2", "s-3"]);
        let first = as_leaf(&outer.children[0]);
        assert_eq!(first.server_id, None);
        assert_eq!(first.label, "local");
        assert_eq!(first.cwd_from_session, None);
        let inner = as_split(&outer.children[1]);
        assert_eq!(inner.direction, SplitDirection::Column);
        assert_eq!(as_leaf(&inner.children[0]).server_id, Some(3));
    }

    #[test]
    fn serde_round_trips_a_nested_tree() {
        let node: PaneNode = serde_json::from_str(TS_SAVED_NODE).expect("deserialize");
        let json = serde_json::to_string(&node).expect("serialize");
        let back: PaneNode = serde_json::from_str(&json).expect("re-deserialize");
        assert_eq!(back, node);
    }

    #[test]
    fn serde_output_matches_the_ts_key_shape_and_skips_transient_fields() {
        let mut l = leaf("s-1");
        l.cwd_from_session = Some(sid("other")); // transient — 직렬화 제외
        let node = split("split-id", SplitDirection::Column, vec![50.0, 50.0], vec![
            PaneNode::Leaf(l),
            lnode("s-2"),
        ]);
        let json = serde_json::to_string(&node).expect("serialize");
        assert!(json.contains(r#""type":"split""#));
        assert!(json.contains(r#""direction":"column""#));
        assert!(json.contains(r#""type":"leaf""#));
        assert!(json.contains(r#""sessionId":"s-1""#));
        assert!(json.contains(r#""serverId":null"#));
        assert!(json.contains(r#""label":"s-1""#));
        assert!(!json.contains("cwd"), "cwdFromSession must not be persisted");
        assert!(!json.contains("split-id"), "split id must not be persisted");
        assert!(!json.contains(r#""id""#), "no id key in persisted format");
    }

    #[test]
    fn serde_accepts_a_legacy_leaf_without_session_id_and_revive_fills_it() {
        // 구버전 레이아웃: sessionId 없음 (TS reviveNode의 `?? uid()` 경로)
        let json = r#"{"type":"split","direction":"row","sizes":[50,50],"children":[{"type":"leaf","serverId":null,"label":"old"},{"type":"leaf","sessionId":"keep","serverId":null,"label":"new"}]}"#;
        let mut node: PaneNode = serde_json::from_str(json).expect("deserialize");
        assert!(as_leaf(&as_split(&node).children[0]).session_id.is_empty());

        let mut s = 0;
        let mut p = 0;
        revive_ids(
            &mut node,
            &mut || {
                s += 1;
                SessionId::from(format!("gen-s{s}"))
            },
            &mut || {
                p += 1;
                SplitId::from(format!("gen-p{p}"))
            },
        );
        let outer = as_split(&node);
        assert_eq!(outer.id, spid("gen-p1"));
        assert_eq!(ids(&node), ["gen-s1", "keep"]); // 저장된 sessionId는 유지
    }

    #[test]
    fn serde_round_trips_a_terminal_tab_in_saved_tab_shape() {
        let tab = TerminalTab {
            id: tid("transient"),
            root: lnode("a"),
            name: Some("이름".into()),
        };
        let json = serde_json::to_string(&tab).expect("serialize");
        assert!(json.contains(r#""name":"이름""#));
        assert!(!json.contains("transient"), "tab id must not be persisted");
        let back: TerminalTab = serde_json::from_str(&json).expect("deserialize");
        assert!(back.id.is_empty());
        assert_eq!(back.root, tab.root);
        assert_eq!(back.name, tab.name);

        // name 없는 탭은 name 키 자체를 쓰지 않는다 (TS: undefined → 키 생략)
        let unnamed = TerminalTab { id: tid("t"), root: lnode("a"), name: None };
        let json = serde_json::to_string(&unnamed).expect("serialize");
        assert!(!json.contains("name"));
    }
}
