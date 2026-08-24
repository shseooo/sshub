//! 앱 데이터 경로. Electron과 동일하게 `~/Library/Application Support`
//! **바로 아래**에 파일을 둔다 (sshub/ 하위 디렉터리가 아님 — 기존 사용자
//! 데이터와의 호환을 위해 절대 변경 금지).

use std::path::PathBuf;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub app_data: PathBuf,
    pub store_file: PathBuf,
    /// **키의 원본 디렉터리 = `~/.ssh`** (config와 같은 곳). `discover()`만
    /// 진짜 홈을 가리키고 `in_dir()`는 주어진 디렉터리 안에 머문다 —
    /// `ssh_config_file`과 정확히 같은 규칙이다.
    pub keys_dir: PathBuf,
    /// 옛 키 디렉터리(`<app_data>/ssh_keys`). 두 가지 이유로 남는다:
    /// 1) 서버별 PEM(`pem_server_<id>`)은 아직 여기 있다(이번 범위 밖),
    /// 2) 여기 있던 개인 키를 `~/.ssh`로 복사한다(원본은 지우지 않는다).
    pub app_keys_dir: PathBuf,
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
        let ssh = home.join(".ssh");
        Ok(AppPaths {
            ssh_config_file: ssh.join("config"),
            keys_dir: ssh,
            ..Self::in_dir(data)
        })
    }

    pub fn in_dir(app_data: PathBuf) -> AppPaths {
        AppPaths {
            store_file: app_data.join("sshub.json"),
            keys_dir: app_data.join(".ssh"),
            app_keys_dir: app_data.join("ssh_keys"),
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
        assert_eq!(p.app_keys_dir, PathBuf::from("/tmp/appdata/ssh_keys"));
        assert_eq!(p.scrollback_dir, PathBuf::from("/tmp/appdata/sshub_scrollback"));
        assert_eq!(p.terminal_cwd_file, PathBuf::from("/tmp/appdata/sshub_terminal_cwd.json"));
        assert_eq!(p.window_file, PathBuf::from("/tmp/appdata/sshub_window.json"));
        assert_eq!(p.settings_file, PathBuf::from("/tmp/appdata/sshub_settings.json"));
    }

    #[test]
    fn in_dir_keeps_the_ssh_directory_inside_the_given_directory() {
        // 테스트가 이 경로를 쓰는 한 진짜 ~/.ssh는 절대 열리지 않는다.
        let p = AppPaths::in_dir(PathBuf::from("/tmp/appdata"));
        assert_eq!(p.ssh_config_file, PathBuf::from("/tmp/appdata/.ssh/config"));
        assert_eq!(p.keys_dir, PathBuf::from("/tmp/appdata/.ssh"));
    }

    #[test]
    fn keys_live_next_to_the_config_not_in_the_app_data_directory() {
        let p = AppPaths::in_dir(PathBuf::from("/tmp/appdata"));
        assert_eq!(p.keys_dir, p.ssh_config_file.parent().unwrap());
        assert_ne!(p.keys_dir, p.app_keys_dir);
    }
}
