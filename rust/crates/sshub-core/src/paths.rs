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
    /// 접속 정보의 원본(source of truth). `discover()`만 실제 `~/.ssh/config`를
    /// 가리키고, `in_dir()`는 주어진 디렉터리 안에 머문다 — 테스트가 실수로
    /// 사용자의 진짜 config를 건드리지 못하게 하는 유일한 장치다.
    pub ssh_config_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<AppPaths, CoreError> {
        let data = dirs::data_dir().ok_or(CoreError::NoDataDir)?;
        let home = dirs::home_dir().ok_or(CoreError::NoHomeDir)?;
        Ok(AppPaths { ssh_config_file: home.join(".ssh").join("config"), ..Self::in_dir(data) })
    }

    pub fn in_dir(app_data: PathBuf) -> AppPaths {
        AppPaths {
            store_file: app_data.join("sshub.json"),
            keys_dir: app_data.join("ssh_keys"),
            scrollback_dir: app_data.join("sshub_scrollback"),
            terminal_cwd_file: app_data.join("sshub_terminal_cwd.json"),
            window_file: app_data.join("sshub_window.json"),
            settings_file: app_data.join("sshub_settings.json"),
            ssh_config_file: app_data.join(".ssh").join("config"),
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

    #[test]
    fn in_dir_keeps_the_ssh_config_inside_the_given_directory() {
        // 테스트가 이 경로를 쓰는 한 진짜 ~/.ssh/config는 절대 열리지 않는다.
        let p = AppPaths::in_dir(PathBuf::from("/tmp/appdata"));
        assert_eq!(p.ssh_config_file, PathBuf::from("/tmp/appdata/.ssh/config"));
    }
}
