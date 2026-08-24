//! 서버 CRUD 순수 로직 (serverOps.ts 직역).
//!
//! 불변식:
//!  - id는 단조 증가 next_server_id 카운터에서 나온다
//!  - port 기본값 22
//!  - pem_data는 절대 데이터에 남지 않는다 (비밀은 0600 파일)
//!  - update의 proxy_jump는 authoritative (부재 → 클리어)
//!  - list는 즐겨찾기 우선, 이름 소문자 오름차순 (stable)

use crate::error::CoreError;
use crate::model::{CreateServerDto, Server, UpdateServerDto};

#[derive(Debug, Clone, Default)]
pub struct ServerStore {
    pub servers: Vec<Server>,
    pub next_server_id: i64,
}

pub fn list_servers(servers: &[Server]) -> Vec<Server> {
    let mut out = servers.to_vec();
    out.sort_by(|a, b| {
        if a.is_favorite != b.is_favorite {
            return if a.is_favorite {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
    out
}

pub fn find_server(servers: &[Server], id: i64) -> Option<&Server> {
    servers.iter().find(|s| s.id == id)
}

pub fn insert_server(
    store: &ServerStore,
    dto: &CreateServerDto,
    now: &str,
) -> (ServerStore, Server) {
    let server = Server {
        id: store.next_server_id,
        name: dto.name.clone(),
        host: dto.host.clone(),
        port: dto.port.unwrap_or(22),
        username: dto.username.clone(),
        auth_type: dto.auth_type,
        key_id: dto.key_id,
        pem_data: None, // 비밀은 절대 데이터에 넣지 않는다
        proxy_jump: dto.proxy_jump.clone(),
        group_name: dto.group_name.clone(),
        tags: dto.tags.clone(),
        is_favorite: false,
        notes: dto.notes.clone(),
        last_connected_at: None,
        created_at: Some(now.to_string()),
        updated_at: Some(now.to_string()),
        // 이 경로로 만들어지는 서버는 항상 앱 소유 블록이다.
        read_only: false,
    };
    let mut servers = store.servers.clone();
    servers.push(server.clone());
    (
        ServerStore { servers, next_server_id: store.next_server_id + 1 },
        server,
    )
}

pub fn update_server(
    store: &ServerStore,
    dto: &UpdateServerDto,
    now: &str,
) -> Result<(ServerStore, Server), CoreError> {
    let idx = store
        .servers
        .iter()
        .position(|s| s.id == dto.id)
        .ok_or(CoreError::ServerNotFound)?;
    let prev = &store.servers[idx];
    let server = Server {
        id: prev.id,
        name: dto.name.clone().unwrap_or_else(|| prev.name.clone()),
        host: dto.host.clone().unwrap_or_else(|| prev.host.clone()),
        port: dto.port.unwrap_or(prev.port),
        username: dto.username.clone().unwrap_or_else(|| prev.username.clone()),
        auth_type: dto.auth_type.unwrap_or(prev.auth_type),
        // `!== undefined` 규칙: 바깥 Some이면 그 값(내부 None=클리어), 없으면 유지
        key_id: match dto.key_id {
            Some(v) => v,
            None => prev.key_id,
        },
        pem_data: None, // 여기서는 절대 영속화하지 않는다
        // authoritative — 부재가 곧 클리어
        proxy_jump: dto.proxy_jump.clone(),
        group_name: match &dto.group_name {
            Some(v) => v.clone(),
            None => prev.group_name.clone(),
        },
        tags: match &dto.tags {
            Some(v) => v.clone(),
            None => prev.tags.clone(),
        },
        is_favorite: prev.is_favorite,
        notes: match &dto.notes {
            Some(v) => v.clone(),
            None => prev.notes.clone(),
        },
        last_connected_at: prev.last_connected_at.clone(),
        created_at: prev.created_at.clone(),
        updated_at: Some(now.to_string()),
        read_only: prev.read_only,
    };
    let mut servers = store.servers.clone();
    servers[idx] = server.clone();
    Ok((
        ServerStore { servers, next_server_id: store.next_server_id },
        server,
    ))
}

pub fn delete_server(store: &ServerStore, id: i64) -> ServerStore {
    ServerStore {
        servers: store.servers.iter().filter(|s| s.id != id).cloned().collect(),
        next_server_id: store.next_server_id,
    }
}

pub fn toggle_favorite(store: &ServerStore, id: i64) -> Result<(ServerStore, Server), CoreError> {
    let idx = store
        .servers
        .iter()
        .position(|s| s.id == id)
        .ok_or(CoreError::ServerNotFound)?;
    let mut server = store.servers[idx].clone();
    server.is_favorite = !server.is_favorite;
    let mut servers = store.servers.clone();
    servers[idx] = server.clone();
    Ok((
        ServerStore { servers, next_server_id: store.next_server_id },
        server,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AuthType;

    const NOW: &str = "2026-06-19T00:00:00.000Z";

    fn srv(id: i64) -> Server {
        Server {
            id,
            name: "srv".into(),
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth_type: AuthType::Key,
            created_at: Some(NOW.into()),
            updated_at: Some(NOW.into()),
            ..Default::default()
        }
    }

    fn base_dto() -> CreateServerDto {
        CreateServerDto {
            name: "web".into(),
            host: "1.2.3.4".into(),
            username: "root".into(),
            auth_type: AuthType::Key,
            ..Default::default()
        }
    }

    #[test]
    fn insert_assigns_next_server_id_and_increments_the_counter() {
        let store = ServerStore { servers: vec![], next_server_id: 7 };
        let (next, server) = insert_server(&store, &base_dto(), NOW);
        assert_eq!(server.id, 7);
        assert_eq!(next.next_server_id, 8);
        assert_eq!(next.servers.len(), 1);
    }

    #[test]
    fn insert_defaults_port_to_22_when_omitted_respects_it_when_given() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        assert_eq!(insert_server(&store, &base_dto(), NOW).1.port, 22);
        let dto = CreateServerDto { port: Some(2222), ..base_dto() };
        assert_eq!(insert_server(&store, &dto, NOW).1.port, 2222);
    }

    #[test]
    fn insert_never_stores_a_pem_in_the_data() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        let dto = CreateServerDto {
            auth_type: AuthType::Pem,
            pem_data: Some("PRIVATE KEY".into()),
            ..base_dto()
        };
        assert_eq!(insert_server(&store, &dto, NOW).1.pem_data, None);
    }

    #[test]
    fn insert_starts_non_favorite_with_timestamps_set_and_last_connected_null() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        let (_, server) = insert_server(&store, &base_dto(), NOW);
        assert!(!server.is_favorite);
        assert_eq!(server.created_at.as_deref(), Some(NOW));
        assert_eq!(server.updated_at.as_deref(), Some(NOW));
        assert_eq!(server.last_connected_at, None);
    }

    #[test]
    fn insert_carries_optional_fields_through() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        let dto = CreateServerDto {
            key_id: Some(3),
            proxy_jump: Some("user@bastion".into()),
            group_name: Some("prod".into()),
            tags: Some("[\"a\"]".into()),
            notes: Some("n".into()),
            ..base_dto()
        };
        let (_, server) = insert_server(&store, &dto, NOW);
        assert_eq!(server.key_id, Some(3));
        assert_eq!(server.proxy_jump.as_deref(), Some("user@bastion"));
        assert_eq!(server.group_name.as_deref(), Some("prod"));
        assert_eq!(server.tags.as_deref(), Some("[\"a\"]"));
        assert_eq!(server.notes.as_deref(), Some("n"));
    }

    #[test]
    fn insert_does_not_mutate_the_input_store() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        let _ = insert_server(&store, &base_dto(), NOW);
        assert!(store.servers.is_empty());
        assert_eq!(store.next_server_id, 1);
    }

    fn update_fixture() -> ServerStore {
        let mut s = srv(5);
        s.name = "old".into();
        s.proxy_jump = Some("keep@me".into());
        s.group_name = Some("g".into());
        s.notes = Some("n".into());
        ServerStore { servers: vec![s], next_server_id: 6 }
    }

    #[test]
    fn update_updates_only_provided_fields_and_bumps_updated_at() {
        let store = update_fixture();
        let dto = UpdateServerDto {
            id: 5,
            name: Some("new".into()),
            port: Some(2200),
            ..Default::default()
        };
        let (_, server) = update_server(&store, &dto, "2026-07-01T00:00:00.000Z").unwrap();
        assert_eq!(server.name, "new");
        assert_eq!(server.port, 2200);
        assert_eq!(server.username, "u"); // untouched
        assert_eq!(server.updated_at.as_deref(), Some("2026-07-01T00:00:00.000Z"));
    }

    #[test]
    fn update_treats_proxy_jump_as_authoritative_clears_it_when_absent() {
        let store = update_fixture();
        let dto = UpdateServerDto { id: 5, name: Some("x".into()), ..Default::default() };
        let (_, server) = update_server(&store, &dto, NOW).unwrap();
        assert_eq!(server.proxy_jump, None);
    }

    #[test]
    fn update_never_persists_a_pem() {
        let store = update_fixture();
        let dto = UpdateServerDto {
            id: 5,
            auth_type: Some(AuthType::Pem),
            ..Default::default()
        };
        let (_, server) = update_server(&store, &dto, NOW).unwrap();
        assert_eq!(server.pem_data, None);
    }

    #[test]
    fn update_some_none_clears_key_id_group_tags_notes_while_none_keeps_them() {
        let mut store = update_fixture();
        store.servers[0].key_id = Some(9);
        store.servers[0].tags = Some("[\"t\"]".into());

        // 바깥 None → 유지
        let keep = UpdateServerDto { id: 5, ..Default::default() };
        let (_, kept) = update_server(&store, &keep, NOW).unwrap();
        assert_eq!(kept.key_id, Some(9));
        assert_eq!(kept.group_name.as_deref(), Some("g"));
        assert_eq!(kept.tags.as_deref(), Some("[\"t\"]"));
        assert_eq!(kept.notes.as_deref(), Some("n"));

        // Some(None) → 클리어 (JS의 명시적 null 전달)
        let clear = UpdateServerDto {
            id: 5,
            key_id: Some(None),
            group_name: Some(None),
            tags: Some(None),
            notes: Some(None),
            ..Default::default()
        };
        let (_, cleared) = update_server(&store, &clear, NOW).unwrap();
        assert_eq!(cleared.key_id, None);
        assert_eq!(cleared.group_name, None);
        assert_eq!(cleared.tags, None);
        assert_eq!(cleared.notes, None);
    }

    #[test]
    fn update_throws_when_the_server_is_missing() {
        let store = update_fixture();
        let dto = UpdateServerDto { id: 999, name: Some("x".into()), ..Default::default() };
        let err = update_server(&store, &dto, NOW).unwrap_err();
        assert_eq!(err.to_string(), "Server not found");
    }

    #[test]
    fn update_does_not_mutate_the_input_store() {
        let store = update_fixture();
        let dto = UpdateServerDto { id: 5, name: Some("mutated?".into()), ..Default::default() };
        let _ = update_server(&store, &dto, NOW).unwrap();
        assert_eq!(store.servers[0].name, "old");
    }

    #[test]
    fn delete_removes_the_matching_server_keeps_the_rest() {
        let store = ServerStore { servers: vec![srv(1), srv(2)], next_server_id: 3 };
        let next = delete_server(&store, 1);
        assert_eq!(next.servers.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn toggle_favorite_flips_the_flag() {
        let store = ServerStore { servers: vec![srv(1)], next_server_id: 2 };
        let (_, server) = toggle_favorite(&store, 1).unwrap();
        assert!(server.is_favorite);
    }

    #[test]
    fn toggle_favorite_throws_when_missing() {
        let store = ServerStore { servers: vec![], next_server_id: 1 };
        let err = toggle_favorite(&store, 1).unwrap_err();
        assert_eq!(err.to_string(), "Server not found");
    }

    #[test]
    fn list_sorts_favorites_first_then_case_insensitive_by_name() {
        let mut a = srv(1);
        a.name = "Zebra".into();
        let mut b = srv(2);
        b.name = "alpha".into();
        let mut c = srv(3);
        c.name = "beta".into();
        c.is_favorite = true;
        let names: Vec<String> =
            list_servers(&[a, b, c]).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["beta", "alpha", "Zebra"]);
    }

    #[test]
    fn list_does_not_mutate_the_input() {
        let mut a = srv(1);
        a.name = "b".into();
        let mut b = srv(2);
        b.name = "a".into();
        let servers = vec![a, b];
        let _ = list_servers(&servers);
        assert_eq!(servers.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn find_returns_the_server_or_none() {
        let servers = vec![srv(1)];
        assert_eq!(find_server(&servers, 1).map(|s| s.id), Some(1));
        assert!(find_server(&servers, 2).is_none());
    }
}
