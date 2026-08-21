//! Node(Electron) 구현이 만든 실제 바이트와의 호환 검증.
//! 픽스처 재생성: `bun rust/crates/sshub-core/tests/fixtures/gen_fixtures.mjs`
//!
//! 이 테스트가 깨지면 기존 사용자의 sshub.json/백업 파일을 Rust 앱이 잘못
//! 읽거나 쓰는 것이다 — 절대 "기대값을 고쳐서" 통과시키지 말 것.

use std::collections::BTreeMap;

use sshub_core::crypto::{decrypt_bundle, is_encrypted_envelope};
use sshub_core::model::{ExportBundle, ExportFilter, SecureBundle, StoreData};
use sshub_core::ops::bundle_ops::build_export_bundle;
use sshub_core::store::normalize_data;

const PASSPHRASE: &str = "test-pass";

fn fixture(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    std::fs::read_to_string(format!("{path}{name}"))
        .unwrap_or_else(|e| panic!("fixture {name} missing ({e}) — run gen_fixtures.mjs"))
}

#[test]
fn store_json_round_trips_byte_for_byte() {
    let original = fixture("node_sshub.json");
    let data: StoreData = serde_json::from_str(&original).expect("deserialize node sshub.json");
    let rendered = serde_json::to_string_pretty(&data).expect("serialize");
    assert_eq!(rendered, original.trim_end_matches('\n'), "byte-identical round trip");
}

#[test]
fn store_fields_are_parsed_with_node_semantics() {
    let data: StoreData = serde_json::from_str(&fixture("node_sshub.json")).unwrap();
    assert_eq!(data.next_server_id, 4);
    assert_eq!(data.next_key_id, 3);

    let prod = &data.servers[0];
    assert_eq!(prod.name, "prod-web");
    assert_eq!(prod.key_id, Some(1));
    assert_eq!(prod.proxy_jump.as_deref(), Some("bastion.example.com"));
    assert_eq!(prod.tags.as_deref(), Some(r#"["web","nginx"]"#));
    assert!(prod.is_favorite);
    assert!(prod.notes.as_deref().unwrap().contains("한글"));

    let db = &data.servers[1];
    assert_eq!(db.port, 2222);
    assert_eq!(db.key_id, None);
    assert_eq!(db.last_connected_at, None);

    assert_eq!(data.keys[1].key_size, 3072);
    assert!(data.keys[0].passphrase_protected);
}

#[test]
fn normalize_keeps_node_data_unchanged() {
    // 정상 파일은 normalize를 통과해도 그대로여야 한다 (불필요한 재저장 방지).
    let data: StoreData = serde_json::from_str(&fixture("node_sshub.json")).unwrap();
    let normalized = normalize_data(Some(data.clone()));
    assert_eq!(
        serde_json::to_string(&normalized).unwrap(),
        serde_json::to_string(&data).unwrap()
    );
}

#[test]
fn plain_export_matches_node_bytes() {
    let data: StoreData = serde_json::from_str(&fixture("node_sshub.json")).unwrap();
    let shortcuts: BTreeMap<String, String> = [
        ("newTab".to_string(), "meta+KeyT".to_string()),
        ("splitRight".to_string(), "meta+KeyD".to_string()),
    ]
    .into_iter()
    .collect();
    let filter =
        ExportFilter { server_ids: None, key_ids: None, shortcuts: Some(shortcuts) };
    let bundle = build_export_bundle(&data, &filter);
    let rendered = serde_json::to_string_pretty(&bundle).unwrap();
    let expected = fixture("node_plain_export.json");
    assert_eq!(rendered, expected.trim_end_matches('\n'));
}

#[test]
fn decrypts_envelope_produced_by_node() {
    let envelope = fixture("node_envelope.enc");
    assert!(is_encrypted_envelope(&envelope));

    let plaintext = decrypt_bundle(&envelope, PASSPHRASE).expect("decrypt node envelope");
    assert_eq!(plaintext, fixture("node_envelope_plaintext.json"));

    // 내용도 우리 타입으로 그대로 읽혀야 한다.
    let secure: SecureBundle = serde_json::from_str(&plaintext).unwrap();
    assert_eq!(secure.bundle.version, 1);
    assert_eq!(secure.bundle.servers.len(), 3);
    assert_eq!(secure.private_keys[0].name, "work-ed25519");
    assert!(secure.private_keys[0].pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
}

#[test]
fn wrong_passphrase_yields_the_exact_korean_error() {
    let err = decrypt_bundle(&fixture("node_envelope.enc"), "nope").unwrap_err();
    assert_eq!(err.to_string(), "복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.");
}

#[test]
fn plain_json_is_rejected_as_envelope() {
    let plain = fixture("node_plain_export.json");
    assert!(!is_encrypted_envelope(&plain));
    let err = decrypt_bundle(&plain, PASSPHRASE).unwrap_err();
    assert_eq!(err.to_string(), "암호화된 sshub 백업 파일이 아닙니다.");
}

#[test]
fn our_envelope_is_shaped_like_nodes() {
    // 역방향(Rust→Node) 호환: 필드 구성/base64 길이가 Node 산출물과 같아야 한다.
    let ours = sshub_core::crypto::encrypt_bundle("hello 안녕", PASSPHRASE).unwrap();
    let theirs: serde_json::Value = serde_json::from_str(&fixture("node_envelope.enc")).unwrap();
    let ours_v: serde_json::Value = serde_json::from_str(&ours).unwrap();

    let keys = |v: &serde_json::Value| -> Vec<String> {
        v.as_object().unwrap().keys().cloned().collect()
    };
    assert_eq!(keys(&ours_v), keys(&theirs), "envelope key order/set");
    assert_eq!(ours_v["magic"], theirs["magic"]);
    let b64_len = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap().len();
    assert_eq!(b64_len(&ours_v, "salt"), b64_len(&theirs, "salt"), "16B salt");
    assert_eq!(b64_len(&ours_v, "iv"), b64_len(&theirs, "iv"), "12B iv");
    assert_eq!(b64_len(&ours_v, "tag"), b64_len(&theirs, "tag"), "16B tag");

    assert_eq!(decrypt_bundle(&ours, PASSPHRASE).unwrap(), "hello 안녕");
}

#[test]
fn export_bundle_scrubs_secrets_and_import_drops_key_links() {
    let data: StoreData = serde_json::from_str(&fixture("node_sshub.json")).unwrap();
    let bundle = build_export_bundle(&data, &ExportFilter::default());
    assert!(bundle.servers.iter().all(|s| s.pem_data.is_none()));
    assert!(bundle.keys.iter().all(|k| k.pem_data.is_none()));

    // 빈 스토어로 병합하면 keyId 링크는 끊기고 새 id가 부여된다.
    let (merged, summary) =
        sshub_core::ops::bundle_ops::merge_bundle(&StoreData::default(), &bundle);
    assert_eq!(summary.servers_added, 3);
    assert_eq!(summary.keys_added, 2);
    assert!(merged.servers.iter().all(|s| s.key_id.is_none()));
}

#[test]
fn export_bundle_type_survives_json_round_trip() {
    let text = fixture("node_plain_export.json");
    let bundle: ExportBundle = serde_json::from_str(&text).unwrap();
    assert_eq!(bundle.shortcuts.as_ref().unwrap().len(), 2);
    assert_eq!(serde_json::to_string_pretty(&bundle).unwrap(), text.trim_end_matches('\n'));
}
