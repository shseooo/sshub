//! PTY 프로세스 정보 — 전경(foreground) pid와 그 프로세스의 cwd.
//!
//! 용도 두 가지:
//!  1. 분할 시 "포커스된 터미널의 cwd 상속" (Electron 판 `terminalCwd.ts` 동작 유지)
//!  2. 종료 시 세션별 cwd 스냅샷 → 다음 실행에서 같은 위치로 셸 복원
//!
//! cwd는 **로그인 셸의 pid**가 아니라 PTY의 **전경 프로세스 그룹**에서 읽는다.
//! `cd` 는 셸 자신이 하므로 대개 둘이 같지만, `vim`/`less` 가 떠 있는 동안에도
//! 그 자식의 cwd(= 대개 셸과 동일)를 얻어 실패하지 않는다.
//!
//! 1차 경로는 `proc_pidinfo(PROC_PIDVNODEPATHINFO)` — 프로세스 스폰이 없어
//! 수 µs다. 실패 시(권한/샌드박스) `/usr/sbin/lsof` 폴백: 수십~수백 ms가 걸리니
//! **반드시 백그라운드 executor에서** 호출한다.

use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::Command;

/// 한 터미널의 PTY 프로세스 정보. `Terminal`이 소유한다.
#[derive(Debug)]
pub struct PtyProcessInfo {
    /// PTY master fd. `Pty`를 EventLoop에 넘기기 **전에** 복사해 둔 값이라
    /// 우리는 소유권이 없다 — 절대 close 하지 않는다.
    pty_fd: RawFd,
    /// PTY에 직접 붙은 셸의 pid (전경 그룹을 못 읽을 때의 폴백).
    shell_pid: u32,
    /// 마지막으로 성공한 cwd — 프로세스가 죽는 순간에도 값이 남도록 캐시한다.
    last_cwd: Option<String>,
}

impl PtyProcessInfo {
    pub fn new(pty_fd: RawFd, shell_pid: u32) -> PtyProcessInfo {
        PtyProcessInfo { pty_fd, shell_pid, last_cwd: None }
    }

    pub fn shell_pid(&self) -> u32 {
        self.shell_pid
    }

    /// PTY의 전경 프로세스 그룹 id. 실패하면 셸 pid로 폴백.
    pub fn foreground_pid(&self) -> u32 {
        foreground_pid(self.pty_fd).unwrap_or(self.shell_pid)
    }

    /// 전경 프로세스의 cwd를 새로 읽어 캐시에 반영한다. **블로킹 가능**.
    pub fn refresh_cwd(&mut self) -> Option<String> {
        let pid = self.foreground_pid();
        // 전경 프로세스가 cwd를 못 주면(권한 등) 셸 pid로 한 번 더 시도한다.
        let cwd = read_pid_cwd(pid).or_else(|| {
            if pid == self.shell_pid {
                None
            } else {
                read_pid_cwd(self.shell_pid)
            }
        });
        if let Some(cwd) = cwd {
            if Path::new(&cwd).is_dir() {
                self.last_cwd = Some(cwd);
            }
        }
        self.last_cwd.clone()
    }

    /// 마지막으로 성공한 cwd (I/O 없음).
    pub fn cached_cwd(&self) -> Option<&str> {
        self.last_cwd.as_deref()
    }
}

/// `tcgetpgrp(2)` — PTY master fd의 전경 프로세스 그룹.
pub fn foreground_pid(pty_fd: RawFd) -> Option<u32> {
    if pty_fd < 0 {
        return None;
    }
    // SAFETY: fd는 우리가 만든 PTY master이고 tcgetpgrp는 fd만 읽는다.
    let pgrp = unsafe { libc::tcgetpgrp(pty_fd) };
    if pgrp <= 0 {
        None
    } else {
        Some(pgrp as u32)
    }
}

/// pid로 실행 중 프로세스의 cwd. 실패(프로세스 종료·권한·미지원)는 None.
pub fn read_pid_cwd(pid: u32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(cwd) = proc_cwd(pid) {
            return Some(cwd);
        }
        return lsof_cwd(pid);
    }
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_link(format!("/proc/{pid}/cwd"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

/// `proc_pidinfo(PROC_PIDVNODEPATHINFO)` — 스폰 없는 빠른 경로 (macOS).
#[cfg(target_os = "macos")]
fn proc_cwd(pid: u32) -> Option<String> {
    use std::mem::MaybeUninit;

    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: 커널이 정확히 `size` 바이트를 채우며, 성공 시 반환값이 그 크기다.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written < size {
        return None;
    }
    // SAFETY: 위에서 전체 구조체가 채워졌음을 확인했다.
    let info = unsafe { info.assume_init() };
    // libc가 rustc 하위호환 때문에 `[c_char; MAXPATHLEN]`을 `[[c_char; 32]; 32]`로
    // 쪼개 놨다 — 평평한 바이트 배열로 다시 본다.
    let raw = &info.pvi_cdir.vip_path;
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(raw.as_ptr().cast::<u8>(), std::mem::size_of_val(raw))
    };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if end == 0 {
        return None;
    }
    String::from_utf8(bytes[..end].to_vec()).ok()
}

/// `lsof` 폴백. `-Fn`은 cwd 경로를 'n' 접두 라인으로 낸다. 절대 경로로 실행하는
/// 이유: Finder/Dock에서 뜬 GUI 앱의 PATH에는 /usr/sbin이 없을 수 있다.
#[cfg(target_os = "macos")]
fn lsof_cwd(pid: u32) -> Option<String> {
    let out = Command::new("/usr/sbin/lsof")
        .args(["-a", "-d", "cwd", "-Fn", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find(|l| l.starts_with('n'))
        .map(|l| l[1..].to_string())
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn lsof_cwd(_pid: u32) -> Option<String> {
    let _ = Command::new("true");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_our_own_process_cwd() {
        let cwd = read_pid_cwd(std::process::id()).expect("own cwd readable");
        assert!(Path::new(&cwd).is_dir(), "not a directory: {cwd}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fast_path_and_lsof_agree_for_our_own_process() {
        let pid = std::process::id();
        let fast = proc_cwd(pid).expect("proc_pidinfo path");
        let slow = lsof_cwd(pid).expect("lsof path");
        assert_eq!(fast, slow);
    }

    #[test]
    fn unknown_pid_yields_none() {
        // 예약 범위 밖의 존재할 수 없는 pid
        assert_eq!(read_pid_cwd(0x7FFF_FFFF), None);
    }

    #[test]
    fn foreground_pid_on_a_non_tty_fd_is_none() {
        // stdout이 파이프인 테스트 러너에서는 tcgetpgrp가 실패해야 한다.
        assert_eq!(foreground_pid(-1), None);
    }

    #[test]
    fn cached_cwd_is_empty_before_refresh() {
        let info = PtyProcessInfo::new(-1, std::process::id());
        assert_eq!(info.cached_cwd(), None);
        assert_eq!(info.foreground_pid(), std::process::id());
    }

    #[test]
    fn refresh_populates_cache_from_shell_pid_fallback() {
        // fd가 없으니 전경 그룹 조회는 실패 → shell_pid(= 우리 자신)로 폴백
        let mut info = PtyProcessInfo::new(-1, std::process::id());
        let cwd = info.refresh_cwd().expect("cwd via fallback");
        assert!(Path::new(&cwd).is_dir());
        assert_eq!(info.cached_cwd(), Some(cwd.as_str()));
    }
}
