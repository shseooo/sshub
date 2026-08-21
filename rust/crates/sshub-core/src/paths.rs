//! 앱 데이터 경로. Electron과 동일하게 `~/Library/Application Support`
//! **바로 아래**에 파일을 둔다 (sshub/ 하위 디렉터리가 아님 — 기존 사용자
//! 데이터와의 호환을 위해 절대 변경 금지).

use std::path::PathBuf;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub app_data: PathBuf,
    pub store_file: PathBuf,
    pub keys_dir: PathBuf,
    pub scrollback_dir: PathBuf,
    pub terminal_cwd_file: PathBuf,
    pub window_file: PathBuf,
    pub settings_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<AppPaths, CoreError> {
        let data = dirs::data_dir().ok_or(CoreError::NoDataDir)?;
        Ok(Self::in_dir(data))
    }

    pub fn in_dir(app_data: PathBuf) -> AppPaths {
        AppPaths {
            store_file: app_data.join("sshub.json"),
            keys_dir: app_data.join("ssh_keys"),
            scrollback_dir: app_data.join("sshub_scrollback"),
            terminal_cwd_file: app_data.join("sshub_terminal_cwd.json"),
            window_file: app_data.join("sshub_window.json"),
            settings_file: app_data.join("sshub_settings.json"),
            app_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_match_the_electron_layout() {
        let p = AppPaths::in_dir(PathBuf::from("/tmp/appdata"));
        assert_eq!(p.store_file, PathBuf::from("/tmp/appdata/sshub.json"));
        assert_eq!(p.keys_dir, PathBuf::from("/tmp/appdata/ssh_keys"));
        assert_eq!(p.scrollback_dir, PathBuf::from("/tmp/appdata/sshub_scrollback"));
        assert_eq!(p.terminal_cwd_file, PathBuf::from("/tmp/appdata/sshub_terminal_cwd.json"));
        assert_eq!(p.window_file, PathBuf::from("/tmp/appdata/sshub_window.json"));
        assert_eq!(p.settings_file, PathBuf::from("/tmp/appdata/sshub_settings.json"));
    }
}
