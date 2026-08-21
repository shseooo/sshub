//! sshub-core — 순수 로직 + 영속성 (rust/docs/DESIGN-core.md)
//!
//! 기존 Electron 앱의 on-disk 데이터와 바이트/동작 호환을 유지한다.
//! API는 전부 동기 — 비동기 경계(ssh-keygen·lsof·scrypt)는 호출자(GPUI
//! BackgroundExecutor)가 담당한다.

pub mod backup;
pub mod crypto;
pub mod error;
pub mod fsutil;
pub mod key_files;
pub mod key_type;
pub mod keys_io;
pub mod model;
pub mod ops;
pub mod paths;
pub mod scrollback;
pub mod settings;
pub mod ssh_args;
pub mod ssh_config;
pub mod store;
pub mod terminal_cwd;
pub mod time;
pub mod window_state;

pub use error::CoreError;
pub use model::{
    AuthType, CreateKeyDto, CreateServerDto, ExportBundle, ExportFilter, ImportKeyDto,
    ImportSummary, KeyType, LoadedKeyFile, PrivateKeyEntry, SecureBundle, Server, SshKey,
    SshKeyView, StoreData, UpdateKeyDto, UpdateServerDto,
};
pub use paths::AppPaths;
pub use scrollback::{scrollback_file_name, ScrollbackStore, SCROLLBACK_LINES};
pub use settings::Settings;
pub use ssh_args::{build_connect_banner, build_ssh_args, SshPaths};
pub use store::Store;
pub use terminal_cwd::TerminalCwdStore;
pub use window_state::WindowBounds;
