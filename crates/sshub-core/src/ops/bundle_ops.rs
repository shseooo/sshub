//! export/import 번들 순수 로직 (bundleOps.ts 직역).
//! export는 비밀 제거, import는 이름 정확 일치 스킵 + 새 id 부여.

use std::collections::HashSet;

use crate::model::{ExportBundle, ExportFilter, ImportSummary, StoreData};

/// 비밀 제거 + (옵션) id 필터가 적용된 export 번들.
pub fn build_export_bundle(data: &StoreData, filter: &ExportFilter) -> ExportBundle {
    let mut servers: Vec<_> = data
        .servers
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.pem_data = None;
            s
        })
        .collect();
    let mut keys: Vec<_> = data
        .keys
        .iter()
        .map(|k| {
            let mut k = k.clone();
            k.pem_data = None;
            k
        })
        .collect();
    if let Some(ids) = &filter.server_ids {
        servers.retain(|s| ids.contains(&s.id));
    }
    if let Some(ids) = &filter.key_ids {
        keys.retain(|k| ids.contains(&k.id));
    }
    ExportBundle { version: 1, servers, keys, shortcuts: filter.shortcuts.clone() }
}

/// 번들을 스토어 데이터에 병합. 기존 이름은 스킵(절대 덮어쓰지 않음),
/// 새 항목은 새 id + 비밀 제거 + 서버 key_id 클리어(참조 무결성 보장 불가).
pub fn merge_bundle(data: &StoreData, bundle: &ExportBundle) -> (StoreData, ImportSummary) {
    let mut summary = ImportSummary {
        shortcuts: bundle.shortcuts.clone(),
        ..Default::default()
    };

    let mut servers = data.servers.clone();
    let mut next_server_id = data.next_server_id;
    let mut server_names: HashSet<String> = servers.iter().map(|s| s.name.clone()).collect();
    for s in &bundle.servers {
        if server_names.contains(&s.name) {
            summary.servers_skipped += 1;
            continue;
        }
        server_names.insert(s.name.clone());
        let mut s = s.clone();
        s.id = next_server_id;
        next_server_id += 1;
        s.pem_data = None;
        s.key_id = None;
        servers.push(s);
        summary.servers_added += 1;
    }

    let mut keys = data.keys.clone();
    let mut next_key_id = data.next_key_id;
    let mut key_names: HashSet<String> = keys.iter().map(|k| k.name.clone()).collect();
    for k in &bundle.keys {
        if key_names.contains(&k.name) {
            summary.keys_skipped += 1;
            continue;
        }
        key_names.insert(k.name.clone());
        let mut k = k.clone();
        k.id = next_key_id;
        next_key_id += 1;
        k.pem_data = None;
        keys.push(k);
        summary.keys_added += 1;
    }

    (StoreData { next_server_id, next_key_id, servers, keys }, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthType, KeyType, Server, SshKey};
    use std::collections::BTreeMap;

    fn srv(id: i64, name: &str) -> Server {
        Server {
            id,
            name: name.into(),
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth_type: AuthType::Key,
            key_id: Some(9),
            pem_data: Some("SECRET".into()),
            ..Default::default()
        }
    }

    fn key(id: i64, name: &str) -> SshKey {
        SshKey {
            id,
            name: name.into(),
            public_key: "p".into(),
            pem_data: Some("SECRET".into()),
            key_type: KeyType::Ed25519,
            key_size: 256,
            ..Default::default()
        }
    }

    fn shortcuts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn build_strips_secrets_from_servers_and_keys() {
        let data = StoreData {
            next_server_id: 1,
            next_key_id: 1,
            servers: vec![srv(1, "s")],
            keys: vec![key(1, "k")],
        };
        let b = build_export_bundle(&data, &ExportFilter::default());
        assert_eq!(b.servers[0].pem_data, None);
        assert_eq!(b.keys[0].pem_data, None);
        assert_eq!(b.version, 1);
    }

    #[test]
    fn build_filters_by_server_ids_and_key_ids_when_given() {
        let data = StoreData {
            next_server_id: 3,
            next_key_id: 3,
            servers: vec![srv(1, "a"), srv(2, "b")],
            keys: vec![key(1, "ka"), key(2, "kb")],
        };
        let filter = ExportFilter {
            server_ids: Some(vec![2]),
            key_ids: Some(vec![1]),
            shortcuts: None,
        };
        let b = build_export_bundle(&data, &filter);
        assert_eq!(b.servers.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!(b.keys.iter().map(|k| k.id).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn build_carries_shortcuts() {
        let data = StoreData::default();
        let filter = ExportFilter { shortcuts: Some(shortcuts(&[("a", "b")])), ..Default::default() };
        let b = build_export_bundle(&data, &filter);
        assert_eq!(b.shortcuts, Some(shortcuts(&[("a", "b")])));
    }

    fn merge_fixture() -> (StoreData, ExportBundle) {
        let bundle = ExportBundle {
            version: 1,
            servers: vec![
                {
                    let mut s = srv(99, "new");
                    s.pem_data = Some("X".into());
                    s
                },
                srv(5, "dup"),
            ],
            keys: vec![{
                let mut k = key(99, "newkey");
                k.pem_data = Some("X".into());
                k
            }],
            shortcuts: Some(shortcuts(&[("x", "y")])),
        };
        let base = StoreData {
            next_server_id: 2,
            next_key_id: 1,
            servers: vec![srv(1, "dup")],
            keys: vec![],
        };
        (base, bundle)
    }

    #[test]
    fn merge_adds_new_entries_with_fresh_ids_skips_existing_names() {
        let (base, bundle) = merge_fixture();
        let (d, summary) = merge_bundle(&base, &bundle);
        assert_eq!(summary.servers_added, 1);
        assert_eq!(summary.servers_skipped, 1);
        assert_eq!(summary.keys_added, 1);
        assert_eq!(summary.keys_skipped, 0);
        let added = d.servers.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(added.id, 2); // next_server_id에서 온 새 id
        assert_eq!(d.next_server_id, 3);
    }

    #[test]
    fn merge_strips_secrets_and_clears_dangling_key_id_on_imported_servers() {
        let (base, bundle) = merge_fixture();
        let (d, _) = merge_bundle(&base, &bundle);
        let added = d.servers.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(added.pem_data, None);
        assert_eq!(added.key_id, None);
    }

    #[test]
    fn merge_passes_shortcuts_through_to_the_summary() {
        let (base, bundle) = merge_fixture();
        let (_, summary) = merge_bundle(&base, &bundle);
        assert_eq!(summary.shortcuts, Some(shortcuts(&[("x", "y")])));
    }

    #[test]
    fn merge_does_not_mutate_the_input_data() {
        let (base, bundle) = merge_fixture();
        let _ = merge_bundle(&base, &bundle);
        assert_eq!(base.servers.len(), 1);
    }
}
