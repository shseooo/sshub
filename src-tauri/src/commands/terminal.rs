use crate::AppState;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct PtySession {
    // Writer has its own lock so a blocking write to one terminal never
    // stalls input to the others (the session map lock is released first).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub struct TerminalSessions(pub Mutex<HashMap<String, PtySession>>);

/// Spawn a PTY running ssh (or a local shell when server_id is None) and
/// stream its output to the frontend as `terminal-output-<session_id>` events.
#[tauri::command]
pub fn start_terminal_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    server_id: Option<i64>,
) -> Result<(), String> {
    {
        let sessions = app.state::<TerminalSessions>();
        let map = sessions.0.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&session_id) {
            return Err("Session already exists".to_string());
        }
    }

    // A banner shown before ssh produces any output, so connecting to an
    // unreachable/offline host doesn't look like a frozen blank screen.
    let mut banner: Option<String> = None;

    let mut cmd = match server_id {
        Some(id) => {
            let server = state.store.get_server(id).map_err(|e| e.to_string())?;
            let mut c = CommandBuilder::new("ssh");
            c.arg("-o");
            c.arg("StrictHostKeyChecking=accept-new");
            // Fail fast (and visibly) instead of hanging on a dead host; keep the
            // session from silently dying once connected.
            c.arg("-o");
            c.arg("ConnectTimeout=15");
            c.arg("-o");
            c.arg("ServerAliveInterval=15");
            c.arg("-o");
            c.arg("ServerAliveCountMax=3");
            if server.port != 22 {
                c.arg("-p");
                c.arg(server.port.to_string());
            }

            if server.auth_type == "password" {
                // Go straight to the password prompt. Without this, ssh first
                // offers every agent/default key and a password-only server can
                // exhaust MaxAuthTries ("Too many authentication failures")
                // before it ever asks for a password.
                c.arg("-o");
                c.arg("PreferredAuthentications=keyboard-interactive,password");
                c.arg("-o");
                c.arg("PubkeyAuthentication=no");
            } else if server.auth_type == "pem" {
                // The PEM lives in a 0600 file (written at save time), not in JSON.
                if let Ok(pem_path) = crate::commands::key::server_pem_path(&app, id) {
                    if pem_path.exists() {
                        c.arg("-i");
                        c.arg(&pem_path);
                        c.arg("-o");
                        c.arg("IdentitiesOnly=yes");
                    }
                }
            } else if server.auth_type == "agent" {
                // Use keys loaded in ssh-agent (and default identities); skip
                // password fallback so a missing key fails fast and clearly.
                c.arg("-o");
                c.arg("PreferredAuthentications=publickey");
            } else if let Some(key_id) = server.key_id {
                // Use only the key selected for this server (no agent spraying)
                if let Ok(key) = state.store.get_ssh_key(key_id) {
                    if let Ok(data_dir) = app.path().data_dir() {
                        let key_path = data_dir
                            .join("ssh_keys")
                            .join(crate::commands::key::key_file_name(&key.name));
                        if key_path.exists() {
                            c.arg("-i");
                            c.arg(key_path);
                            c.arg("-o");
                            c.arg("IdentitiesOnly=yes");
                        }
                    }
                }
            }
            // Jump host(s): ssh -J user@bastion[,next-hop...]
            if let Some(pj) = server.proxy_jump.as_ref().filter(|p| !p.trim().is_empty()) {
                c.arg("-J");
                c.arg(pj.trim());
            }
            c.arg(format!("{}@{}", server.username, server.host));
            let jump_note = match server.proxy_jump.as_ref().filter(|p| !p.trim().is_empty()) {
                Some(pj) => format!(" -J {}", pj.trim()),
                None => String::new(),
            };
            banner = Some(format!(
                "\x1b[90m── sshub ──▶ ssh{} {}@{}{} \x1b[0m(연결 중, 15초 내 응답 없으면 시간 초과)\r\n",
                jump_note,
                server.username,
                server.host,
                if server.port != 22 {
                    format!(":{}", server.port)
                } else {
                    String::new()
                }
            ));
            let _ = state.store.touch_last_connected(id);
            c
        }
        None => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
            let mut c = CommandBuilder::new(shell);
            c.arg("-l");
            c
        }
    };
    cmd.env("TERM", "xterm-256color");

    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut child = pty.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let killer = child.clone_killer();
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;

    {
        let sessions = app.state::<TerminalSessions>();
        sessions.0.lock().map_err(|e| e.to_string())?.insert(
            session_id.clone(),
            PtySession {
                writer: Arc::new(Mutex::new(writer)),
                master: pty.master,
                killer,
            },
        );
    }

    if let Some(text) = banner {
        let _ = app.emit(&format!("terminal-output-{}", session_id), text);
    }

    // PTY output -> frontend events; on EOF clean up and notify
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let channel = format!("terminal-output-{}", session_id);
        let mut buf = [0u8; 8192];
        // Holds bytes of an incomplete UTF-8 sequence split across reads,
        // so multibyte chars (한글/이모지) at a chunk boundary aren't corrupted.
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let valid_up_to = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) => e.valid_up_to(),
                    };
                    if valid_up_to > 0 {
                        // Safe: bytes up to valid_up_to are guaranteed valid UTF-8
                        let text =
                            String::from_utf8_lossy(&pending[..valid_up_to]).into_owned();
                        let _ = app_handle.emit(&channel, text);
                        pending.drain(..valid_up_to);
                    }
                    // A genuine incomplete sequence is at most 3 trailing bytes;
                    // anything longer is invalid data — flush it lossily.
                    if pending.len() >= 4 {
                        let text = String::from_utf8_lossy(&pending).into_owned();
                        let _ = app_handle.emit(&channel, text);
                        pending.clear();
                    }
                }
            }
        }
        if !pending.is_empty() {
            let _ = app_handle.emit(&channel, String::from_utf8_lossy(&pending).into_owned());
        }
        let _ = child.wait();
        let sessions = app_handle.state::<TerminalSessions>();
        if let Ok(mut map) = sessions.0.lock() {
            map.remove(&session_id);
        }
        let _ = app_handle.emit(&format!("terminal-closed-{}", session_id), ());
    });

    Ok(())
}

#[tauri::command]
pub fn write_terminal(
    sessions: State<'_, TerminalSessions>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    // Clone the writer handle under a brief map lock, then release it before
    // the (potentially blocking) write so other sessions aren't held up.
    let writer = {
        let map = sessions.0.lock().map_err(|e| e.to_string())?;
        let session = map.get(&session_id).ok_or("Session not found")?;
        session.writer.clone()
    };
    let mut w = writer.lock().map_err(|e| e.to_string())?;
    w.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_terminal(
    sessions: State<'_, TerminalSessions>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let map = sessions.0.lock().map_err(|e| e.to_string())?;
    let session = map.get(&session_id).ok_or("Session not found")?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_terminal(
    sessions: State<'_, TerminalSessions>,
    session_id: String,
) -> Result<(), String> {
    let mut map = sessions.0.lock().map_err(|e| e.to_string())?;
    if let Some(mut session) = map.remove(&session_id) {
        let _ = session.killer.kill();
    }
    Ok(())
}
