//! SSH 키 메타데이터 CRUD 순수 로직 (keyOps.ts 직역). I/O 없음 — 0600 키
//! 파일 처리는 keys_io가 담당한다. pem_data는 절대 여기 남지 않는다.

use crate::error::CoreError;
use crate::model::{KeyType, SshKey};

#[derive(Debug, Clone, Default)]
pub struct KeyStore {
    pub keys: Vec<SshKey>,
    pub next_key_id: i64,
}

#[derive(Debug, Clone)]
pub struct NewKey {
    pub name: String,
    pub public_key: String,
    pub key_type: KeyType,
    pub key_size: i64,
    pub passphrase_protected: bool,
}

#[derive(Debug, Clone)]
pub struct KeyMetaUpdate {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    pub key_type: KeyType,
    pub passphrase_protected: bool,
}

pub fn list_keys(keys: &[SshKey]) -> Vec<SshKey> {
    let mut out = keys.to_vec();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn find_key(keys: &[SshKey], id: i64) -> Option<&SshKey> {
    keys.iter().find(|k| k.id == id)
}

pub fn insert_key(store: &KeyStore, nk: &NewKey, now: &str) -> (KeyStore, SshKey) {
    let key = SshKey {
        id: store.next_key_id,
        name: nk.name.clone(),
        public_key: nk.public_key.clone(),
        pem_data: None, // 비밀은 절대 데이터에 넣지 않는다
        key_type: nk.key_type,
        key_size: nk.key_size,
        passphrase_protected: nk.passphrase_protected,
        created_at: Some(now.to_string()),
    };
    let mut keys = store.keys.clone();
    keys.push(key.clone());
    (KeyStore { keys, next_key_id: store.next_key_id + 1 }, key)
}

pub fn update_key_meta(store: &KeyStore, u: &KeyMetaUpdate) -> Result<(KeyStore, SshKey), CoreError> {
    let idx = store
        .keys
        .iter()
        .position(|k| k.id == u.id)
        .ok_or(CoreError::KeyNotFound)?;
    let mut key = store.keys[idx].clone();
    key.name = u.name.clone();
    key.public_key = u.public_key.clone();
    key.key_type = u.key_type;
    key.passphrase_protected = u.passphrase_protected;
    key.pem_data = None;
    let mut keys = store.keys.clone();
    keys[idx] = key.clone();
    Ok((KeyStore { keys, next_key_id: store.next_key_id }, key))
}

pub fn set_passphrase_protected(store: &KeyStore, id: i64, protected: bool) -> KeyStore {
    let keys = store
        .keys
        .iter()
        .map(|k| {
            if k.id == id {
                let mut k = k.clone();
                k.passphrase_protected = protected;
                k
            } else {
                k.clone()
            }
        })
        .collect();
    KeyStore { keys, next_key_id: store.next_key_id }
}

pub fn delete_key(store: &KeyStore, id: i64) -> KeyStore {
    KeyStore {
        keys: store.keys.iter().filter(|k| k.id != id).cloned().collect(),
        next_key_id: store.next_key_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-06-19T00:00:00.000Z";

    fn key(id: i64) -> SshKey {
        SshKey {
            id,
            name: "k".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            key_type: KeyType::Ed25519,
            key_size: 256,
            created_at: Some(NOW.into()),
            ..Default::default()
        }
    }

    fn new_key() -> NewKey {
        NewKey {
            name: "mykey".into(),
            public_key: "ssh-rsa AAAA".into(),
            key_type: KeyType::Rsa,
            key_size: 3072,
            passphrase_protected: true,
        }
    }

    #[test]
    fn insert_assigns_next_key_id_increments_never_stores_pem() {
        let store = KeyStore { keys: vec![], next_key_id: 4 };
        let (next, k) = insert_key(&store, &new_key(), NOW);
        assert_eq!(k.id, 4);
        assert_eq!(next.next_key_id, 5);
        assert_eq!(k.pem_data, None);
        assert_eq!(k.name, "mykey");
        assert_eq!(k.key_type, KeyType::Rsa);
        assert_eq!(k.key_size, 3072);
        assert!(k.passphrase_protected);
        assert_eq!(k.created_at.as_deref(), Some(NOW));
    }

    #[test]
    fn insert_does_not_mutate_input() {
        let store = KeyStore { keys: vec![], next_key_id: 1 };
        let _ = insert_key(&store, &new_key(), NOW);
        assert!(store.keys.is_empty());
    }

    #[test]
    fn update_meta_updates_name_public_key_type_and_protection() {
        let mut old = key(2);
        old.name = "old".into();
        let store = KeyStore { keys: vec![old], next_key_id: 3 };
        let u = KeyMetaUpdate {
            id: 2,
            name: "new".into(),
            public_key: "ssh-rsa BBBB".into(),
            key_type: KeyType::Rsa,
            passphrase_protected: true,
        };
        let (_, k) = update_key_meta(&store, &u).unwrap();
        assert_eq!(k.name, "new");
        assert_eq!(k.public_key, "ssh-rsa BBBB");
        assert_eq!(k.key_type, KeyType::Rsa);
        assert!(k.passphrase_protected);
    }

    #[test]
    fn update_meta_throws_when_missing() {
        let store = KeyStore { keys: vec![key(2)], next_key_id: 3 };
        let u = KeyMetaUpdate {
            id: 99,
            name: "x".into(),
            public_key: String::new(),
            key_type: KeyType::Rsa,
            passphrase_protected: false,
        };
        let err = update_key_meta(&store, &u).unwrap_err();
        assert_eq!(err.to_string(), "SSH key not found");
    }

    #[test]
    fn set_passphrase_protected_flips_the_flag() {
        let store = KeyStore { keys: vec![key(1)], next_key_id: 2 };
        let next = set_passphrase_protected(&store, 1, true);
        assert!(next.keys[0].passphrase_protected);
    }

    #[test]
    fn delete_removes_the_matching_key() {
        let store = KeyStore { keys: vec![key(1), key(2)], next_key_id: 3 };
        let ids: Vec<i64> = delete_key(&store, 1).keys.iter().map(|k| k.id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn list_sorts_case_insensitively_without_mutating_input() {
        let mut a = key(1);
        a.name = "Zed".into();
        let mut b = key(2);
        b.name = "alpha".into();
        let keys = vec![a, b];
        let names: Vec<String> = list_keys(&keys).into_iter().map(|k| k.name).collect();
        assert_eq!(names, vec!["alpha", "Zed"]);
        assert_eq!(keys.iter().map(|k| k.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn find_returns_the_key_or_none() {
        let keys = vec![key(5)];
        assert_eq!(find_key(&keys, 5).map(|k| k.id), Some(5));
        assert!(find_key(&keys, 6).is_none());
    }
}
