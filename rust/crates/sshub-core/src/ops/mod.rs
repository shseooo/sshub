//! 순수 CRUD/병합 로직 — I/O 없음. Store가 슬라이스를 넘겨 호출하고 결과를
//! 영속화한다. TS electron/lib/{serverOps,keyOps,bundleOps}.ts의 직역.

pub mod bundle_ops;
pub mod key_ops;
pub mod server_ops;
