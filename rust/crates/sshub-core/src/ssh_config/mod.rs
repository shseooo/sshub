//! ~/.ssh/config 파싱·렌더·백업 프루닝·파일 동기화.

mod backups;
mod file;
mod parse;
mod render;

pub use backups::backups_to_prune;
pub use file::{
    sync_config_to_servers, sync_config_to_servers_in, sync_servers_to_config,
    sync_servers_to_config_in,
};
pub use parse::parse_ssh_config;
pub use render::render_ssh_config;
