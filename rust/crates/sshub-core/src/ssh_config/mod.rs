//! ~/.ssh/config 파싱·문서 모델·백업 프루닝·파일 동기화.
//!
//! 파일 전체를 다시 써내는 렌더러는 없다 — 사용자가 손으로 쓴 지시어와
//! 주석을 지우기 때문. 모든 쓰기는 `Document`의 외과적 병합을 거친다.

mod backups;
mod document;
mod file;
mod parse;

pub use backups::backups_to_prune;
pub use document::{Document, Entry, HostBlock, HostSpec, MatchBlock, Node};
pub use file::{
    sync_config_to_servers, sync_config_to_servers_in, sync_servers_to_config,
    sync_servers_to_config_in,
};
pub use parse::parse_ssh_config;
