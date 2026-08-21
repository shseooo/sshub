//! 코어 에러. Display 문자열은 Electron 구현의 사용자 노출 메시지와
//! 바이트 단위로 동일해야 한다 (DESIGN-core.md §6) — UI가 이 문자열로
//! 분기하거나 그대로 표시하기 때문.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    /// 암호화 백업을 passphrase 없이 import — UI가 이 센티널을 보고 재시도한다.
    #[error("ENCRYPTED")]
    NeedsPassphrase,

    #[error("암호화된 sshub 백업 파일이 아닙니다.")]
    NotEncryptedEnvelope,

    #[error("복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.")]
    DecryptFailed,

    #[error("Server not found")]
    ServerNotFound,

    #[error("SSH key not found")]
    KeyNotFound,

    #[error("SSH key not found: {0}")]
    KeyNotFoundId(i64),

    #[error("Unsupported key type: {0}")]
    UnsupportedKeyType(String),

    #[error("Key file already exists: {0}")]
    KeyFileExists(String),

    #[error("공개 키 또는 개인 키(PEM) 중 하나는 필요합니다.")]
    PublicOrPemRequired,

    #[error("같은 이름의 키 파일이 이미 있습니다.")]
    KeyFileNameTaken,

    #[error("이 기기에 개인 키 파일이 없습니다.")]
    PrivateFileMissing,

    #[error("개인 키(PEM)가 비어 있습니다.")]
    EmptyPem,

    #[error("개인 키에서 공개 키를 추출하지 못했습니다. 암호로 보호된 키라면 passphrase를 입력하세요. ({0})")]
    DerivePublicKey(String),

    #[error("패스프레이즈 변경 실패 — 현재 패스프레이즈가 맞는지 확인하세요. ({0})")]
    ChangePassphrase(String),

    #[error("등록된 서버가 없어 ~/.ssh/config를 덮어쓰지 않았습니다.")]
    NoServersForConfig,

    /// ssh-keygen 실패 — stderr trim 그대로 노출 (JS keygen() 헬퍼와 동일).
    #[error("{0}")]
    Keygen(String),

    #[error("could not determine the application data directory")]
    NoDataDir,

    #[error("could not determine the home directory")]
    NoHomeDir,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
