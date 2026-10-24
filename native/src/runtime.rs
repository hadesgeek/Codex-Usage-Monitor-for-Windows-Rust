use crate::*;
use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        System::{LibraryLoader::*, Threading::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub fn node_path(root: &Path) -> Result<PathBuf> {
    let node = root.join("runtime/node.exe");
    anyhow::ensure!(
        node.is_file(),
        "缺少随包 Node 运行时，请使用完整原生安装包。"
    );
    let output = hidden_command(&node).arg("--version").output()?;
    let version = String::from_utf8(output.stdout)?;
    let major: u32 = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .context("Invalid Node version")?
        .parse()?;
    anyhow::ensure!(output.status.success() && major >= 22, "Node 22+ required");
    Ok(node)
}

fn cdp_ready(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(30)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    if write!(
        stream,
        "GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return false;
    }
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut expected = None;
    let (body_start, body_length) = loop {
        if Instant::now() >= deadline {
            return false;
        }
        let mut chunk = [0u8; 1024];
        let Ok(count) = stream.read(&mut chunk) else {
            return false;
        };
        if count == 0 || bytes.len() + count > 65536 {
            return false;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if expected.is_none() {
            if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let Ok(headers) = std::str::from_utf8(&bytes[..offset]) else {
                    return false;
                };
                if headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    != Some("200")
                {
                    return false;
                }
                let mut length = None;
                for line in headers.lines().skip(1) {
                    let Some((name, value)) = line.split_once(':') else {
                        return false;
                    };
                    if name.eq_ignore_ascii_case("transfer-encoding") {
                        return false;
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        if length.is_some() {
                            return false;
                        }
                        length = value.trim().parse::<usize>().ok();
                        if length.is_none() {
                            return false;
                        }
                    }
                }
                let Some(length) = length else {
                    return false;
                };
                if length == 0 || length > 65536 - (offset + 4) {
                    return false;
                }
                expected = Some((offset + 4, length));
            }
        }
        if let Some((start, length)) = expected {
            // CDP keeps HTTP/1.1 connections open even when Connection: close was requested.
            if bytes.len() >= start + length {
                break (start, length);
            }
        }
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes[body_start..body_start + body_length])
    else {
        return false;
    };
    value["webSocketDebuggerUrl"].as_str().is_some_and(|s| {
        s.starts_with(&format!("ws://127.0.0.1:{port}/"))
            || s.starts_with(&format!("ws://localhost:{port}/"))
    })
}

pub fn find_cdp_port() -> Option<u16> {
    let mut candidates = Vec::new();
    if let Ok(root) = state_root() {
        if let Ok(state) = read_json(&root.join("state.json"), 65536) {
            if let Some(port) = state["port"].as_u64().and_then(|v| u16::try_from(v).ok()) {
                candidates.push(port);
            }
        }
    }
    if let Ok(roaming) = platform::known_folder(&FOLDERID_RoamingAppData) {
        for relative in ["Codex/DevToolsActivePort", "ChatGPT/DevToolsActivePort"] {
            if let Ok(value) = fs::read_to_string(roaming.join(relative)) {
                if let Some(port) = value.lines().next().and_then(|v| v.parse::<u16>().ok()) {
                    candidates.push(port);
                }
            }
        }
    }
    candidates.extend(9335..9367);
    candidates.push(9229);
    candidates
        .into_iter()
        .filter(|p| *p >= 1024)
        .find(|p| cdp_ready(*p))
}

pub fn available_port(preferred: u16) -> Result<u16> {
    for port in preferred..=preferred.saturating_add(31) {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    bail!("没有可用的本机 CDP 端口")
}

enum Action {
    Open,
    Reload,
    Exit,
}
struct Tray {
    sender: mpsc::Sender<Action>,
    status: Arc<Mutex<String>>,
}

pub fn run(root: PathBuf, interactive: bool) -> Result<()> {
    unsafe {
        let handle = CreateMutexW(None, false, w!("Local\\CodexUsageMonitorNative"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle)?;
            atomic_write(&state_root()?.join("native-open"), b"open")?;
            return Ok(());
        }
        clear_stale_commands(&state_root()?)?;
        let (sender, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new("正在连接 Codex".to_string()));
        let worker_status = status.clone();
        let worker = thread::spawn(move || {
            if let Err(error) = supervise(root, receiver, worker_status.clone(), interactive) {
                *worker_status.lock().unwrap() = format!("连接失败：{error}");
            }
        });
        let tray = Box::new(Tray { sender, status });
        let tray_pointer = Box::into_raw(tray);
        let result = tray_loop(tray_pointer);
        let tray = Box::from_raw(tray_pointer);
        let _ = tray.sender.send(Action::Exit);
        let _ = worker.join();
        CloseHandle(handle)?;
        if state_root()?.join("native-updated").exists() {
            let _ = fs::remove_file(state_root()?.join("native-updated"));
            hidden_command(install::install_base()?.join("CodexUsageMonitor.exe")).spawn()?;
        }
        result
    }
}

fn supervise(
    root: PathBuf,
    receiver: mpsc::Receiver<Action>,
    status: Arc<Mutex<String>>,
    mut interactive: bool,
) -> Result<()> {
    platform::initialize()?;
    let node = node_path(&root)?;
    let state = state_root()?;
    fs::create_dir_all(&state)?;
    let codex = platform::discover_codex(interactive)?;
    let mut daemon: Option<std::process::Child> = None;
    let mut launch_requested = true;
    let mut started_at = None;
    let mut retry_after = Instant::now();
    let mut remove_panel = false;
    let result = (|| -> Result<()> {
        loop {
            let action = receiver.recv_timeout(Duration::from_secs(1));
            let file_action = if state.join("native-exit").exists() {
                let _ = fs::remove_file(state.join("native-exit"));
                Some(Action::Exit)
            } else if state.join("native-open").exists() {
                let _ = fs::remove_file(state.join("native-open"));
                Some(Action::Open)
            } else if state.join("native-reload").exists() {
                let _ = fs::remove_file(state.join("native-reload"));
                Some(Action::Reload)
            } else {
                None
            };
            if state.join("native-updated").exists() {
                break;
            }
            match file_action.or(action.ok()) {
                Some(Action::Exit) => {
                    remove_panel = true;
                    break;
                }
                Some(Action::Open) => {
                    launch_requested = true;
                    interactive = true;
                    retry_after = Instant::now();
                }
                Some(Action::Reload) => {
                    if let Some(mut child) = daemon.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    retry_after = Instant::now();
                }
                None => (),
            }
            if launch_requested && daemon.is_some() {
                if !platform::running(&codex)? {
                    if let Some(mut child) = daemon.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                } else if let Some(active) = find_cdp_port() {
                    platform::launch(&codex, active)?;
                    launch_requested = false;
                }
            }
            if let Some(child) = &mut daemon {
                if child.try_wait()?.is_some() {
                    daemon = None;
                    retry_after = Instant::now() + Duration::from_secs(5);
                } else {
                    let phase = read_json(&state.join("state.json"), 65536)
                        .ok()
                        .and_then(|v| v["startupPhase"].as_str().map(str::to_owned));
                    *status.lock().unwrap() = if !platform::running(&codex)? {
                        "等待 Codex 启动"
                    } else if phase.as_deref() == Some("ready") {
                        "已连接 · 点击监视栏配置"
                    } else {
                        "正在等待 Codex 界面"
                    }
                    .to_string();
                    continue;
                }
            }
            if Instant::now() < retry_after {
                continue;
            }
            let running = platform::running(&codex)?;
            if !running && !launch_requested {
                *status.lock().unwrap() = "等待 Codex 启动".into();
                continue;
            }
            let selected = if let Some(active) = find_cdp_port() {
                started_at = None;
                active
            } else if !running && launch_requested {
                let selected = available_port(9335)?;
                platform::launch(&codex, selected)?;
                started_at = Some(Instant::now());
                launch_requested = false;
                *status.lock().unwrap() = "Codex 启动中".into();
                continue;
            } else if started_at.is_some_and(|time| time.elapsed() < Duration::from_secs(180)) {
                *status.lock().unwrap() = "正在等待 Codex 调试端口".into();
                continue;
            } else {
                *status.lock().unwrap() = "Codex 未开放 CDP，请正常退出后从本工具打开".into();
                if launch_requested && interactive {
                    thread::spawn(|| {
                        platform::message("当前 Codex 没有可用的本机 CDP 连接。\n\n请正常退出 Codex，再从托盘菜单选择“打开 Codex”。本工具不会强制结束会话。", false)
                    });
                }
                launch_requested = false;
                retry_after = Instant::now() + Duration::from_secs(5);
                continue;
            };
            let mut command = hidden_command(&node);
            command.arg(root.join("scripts/injector.mjs")).args([
                "--watch",
                "--monitor-only",
                "--port",
                &selected.to_string(),
            ]);
            command.current_dir(&root).stdin(Stdio::null());
            let launch_id = format!("native-{}", std::process::id());
            let out = state.join(format!("injector-{launch_id}.log"));
            let err = state.join(format!("injector-{launch_id}-error.log"));
            command.stdout(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&out)?,
            );
            command.stderr(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&err)?,
            );
            if load_credentials(&mut command).is_err() {
                // Optional credential failure must not block the official subscription source.
                command
                    .env_remove("CODEX_USAGE_API_KEY")
                    .env_remove("CODEX_USAGE_PROVIDER_CONFIG_PATH")
                    .env_remove("CODEX_USAGE_ACCOUNT_TOKEN")
                    .env_remove("CODEX_USAGE_ACCOUNT_USER_ID")
                    .env_remove("CODEX_USAGE_ACCOUNT_BASE_URL");
                let _ = atomic_write(&state.join("credential-error.log"), b"Stored API credentials could not be loaded. Reconfigure them in the monitor panel.");
            }
            if let Some(cli) = &codex.cli {
                command.env(
                    "CODEX_USAGE_CODEX_PATH",
                    mirror_cli(cli, &state.join("runtime/codex-cli"))?,
                );
            }
            let mut child = command.spawn()?;
            let value = json!({"schemaVersion":2,"runtimeVersion":fs::read_to_string(root.join("VERSION"))?.trim(),"port":selected,
            "injectorPid":child.id(),"injectorPath":root.join("scripts/injector.mjs"),"stdoutPath":out,"stderrPath":err,
            "startupPhase":"waiting-ui","nativeHostPid":std::process::id()});
            if let Err(error) =
                atomic_write(&state.join("state.json"), &serde_json::to_vec(&value)?)
            {
                let _ = child.kill();
                return Err(error);
            }
            daemon = Some(child);
            launch_requested = false;
        }
        Ok(())
    })();
    if let Some(mut child) = daemon {
        let owned_pid = child.id();
        let _ = child.kill();
        let _ = child.wait();
        if remove_panel {
            if let Ok(value) = read_json(&state.join("state.json"), 65536) {
                if value["injectorPid"].as_u64() == Some(owned_pid as u64) {
                    if let Some(port) = value["port"]
                        .as_u64()
                        .filter(|port| (1024..=65535).contains(port))
                    {
                        let _ = hidden_command(&node)
                            .arg(root.join("scripts/injector.mjs"))
                            .args([
                                "--remove",
                                "--monitor-only",
                                "--port",
                                &port.to_string(),
                                "--timeout-ms",
                                "5000",
                            ])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                }
            }
        }
    }
    if result.is_ok() {
        *status.lock().unwrap() = "__exit".into();
    }
    result
}

fn mirror_cli(source: &Path, mirror_root: &Path) -> Result<PathBuf> {
    if !source
        .components()
        .any(|part| part.as_os_str().eq_ignore_ascii_case("WindowsApps"))
    {
        return Ok(source.to_owned());
    }
    use sha2::{Digest, Sha256};
    let data = fs::read(source)?;
    let hash = format!("{:x}", Sha256::digest(&data));
    let target = mirror_root.join(format!("codex-{}.exe", &hash[..20]));
    if !target.exists() {
        atomic_write(&target, &data)?;
    }
    Ok(target)
}

fn clear_stale_commands(root: &Path) -> Result<()> {
    for name in [
        "native-exit",
        "native-updated",
        "native-reload",
        "native-open",
    ] {
        match fs::remove_file(root.join(name)) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Tray;
    if !pointer.is_null() {
        let tray = &*pointer;
        match message {
            WM_TIMER => {
                let text = tray.status.lock().unwrap().clone();
                if text == "__exit" {
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                let mut icon = icon_data(hwnd);
                for (slot, character) in icon.szTip.iter_mut().zip(text.encode_utf16().take(127)) {
                    *slot = character;
                }
                let _ = Shell_NotifyIconW(NIM_MODIFY, &icon);
                if state_root().is_ok_and(|root| {
                    root.join("native-updated").exists() || root.join("native-exit").exists()
                }) {
                    PostQuitMessage(0);
                }
                return LRESULT(0);
            }
            value if value == WM_APP + 1 => {
                if lp.0 as u32 == WM_LBUTTONDBLCLK {
                    let _ = tray.sender.send(Action::Open);
                }
                if lp.0 as u32 == WM_RBUTTONUP {
                    if let Ok(menu) = CreatePopupMenu() {
                        let status = tray.status.lock().unwrap().clone();
                        let _ = AppendMenuW(
                            menu,
                            MF_STRING | MF_DISABLED,
                            0,
                            PCWSTR(platform::wide(&status).as_ptr()),
                        );
                        for (id, label) in [
                            (1, "打开 Codex"),
                            (2, "重新连接监视器"),
                            (
                                3,
                                if install::autostart_enabled() {
                                    "关闭登录自启动"
                                } else {
                                    "开启登录自启动"
                                },
                            ),
                            (4, "打开数据目录"),
                            (5, "恢复原 Codex 自启动"),
                            (6, "退出监视器（保留 Codex）"),
                        ] {
                            let _ = AppendMenuW(
                                menu,
                                MF_STRING,
                                id,
                                PCWSTR(platform::wide(label).as_ptr()),
                            );
                        }
                        let mut point = POINT::default();
                        let _ = GetCursorPos(&mut point);
                        let _ = SetForegroundWindow(hwnd);
                        let _ = TrackPopupMenu(
                            menu,
                            TPM_RIGHTBUTTON,
                            point.x,
                            point.y,
                            None,
                            hwnd,
                            None,
                        );
                        let _ = DestroyMenu(menu);
                    }
                }
                return LRESULT(0);
            }
            WM_COMMAND => {
                let result = match wp.0 & 0xffff {
                    1 => tray.sender.send(Action::Open).map_err(anyhow::Error::from),
                    2 => tray
                        .sender
                        .send(Action::Reload)
                        .map_err(anyhow::Error::from),
                    3 => install::set_autostart(!install::autostart_enabled()),
                    4 => state_root().map(|root| {
                        ShellExecuteW(
                            None,
                            w!("open"),
                            PCWSTR(platform::wide(root).as_ptr()),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    }),
                    5 => install::restore_startup(),
                    6 => {
                        PostQuitMessage(0);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    platform::message(&error.to_string(), true);
                }
                return LRESULT(0);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => (),
        }
    }
    DefWindowProcW(hwnd, message, wp, lp)
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        ..Default::default()
    }
}

unsafe fn tray_loop(tray: *mut Tray) -> Result<()> {
    let instance = GetModuleHandleW(None)?;
    let class = w!("CodexUsageMonitorNative");
    let icon = LoadIconW(
        Some(HINSTANCE(instance.0)),
        PCWSTR(std::ptr::without_provenance(1)),
    )?;
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: class,
        hIcon: icon,
        ..Default::default()
    };
    anyhow::ensure!(RegisterClassW(&wc) != 0, "Unable to register tray window");
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        w!("Codex Usage Monitor"),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        None,
        None,
        Some(HINSTANCE(instance.0)),
        None,
    )?;
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, tray as isize);
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_APP + 1;
    data.hIcon = icon;
    Shell_NotifyIconW(NIM_ADD, &data).ok()?;
    SetTimer(Some(hwnd), 1, 1000, None);
    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, None, 0, 0).0;
        if result == 0 {
            break;
        }
        anyhow::ensure!(result != -1, "Windows message loop failed");
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    let _ = DestroyWindow(hwnd);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cdp_probe_uses_http11_and_finishes_before_keepalive_closes() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if stream.read(&mut byte).unwrap_or(0) == 0 {
                    return false;
                }
                request.push(byte[0]);
            }
            let http11 = request.starts_with(b"GET /json/version HTTP/1.1\r\n");
            if !http11 {
                return false;
            }
            let body = format!(
                "{{\"webSocketDebuggerUrl\":\"ws://127.0.0.1:{port}/devtools/browser/fixture\"}}"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length:{}\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(2));
            http11
        });
        let ready = cdp_ready(port);
        release.send(()).unwrap_or(());
        assert!(server.join().unwrap());
        assert!(ready, "A complete CDP response must not wait for EOF");
    }

    #[test]
    fn cdp_probe_rejects_oversized_truncated_and_remote_responses() {
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length:99999999\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length:100\r\n\r\n{}",
            "HTTP/1.1 403 Forbidden\r\nContent-Length:2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length:2\r\nContent-Length:2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length:60\r\n\r\n{\"webSocketDebuggerUrl\":\"ws://example.com/devtools/browser\"}",
        ] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            });
            assert!(!cdp_ready(port));
            server.join().unwrap();
        }
    }
    #[test]
    fn fresh_host_discards_old_commands_and_preserves_user_state() {
        let root =
            env::temp_dir().join(format!("codex-native-command-test-{}", std::process::id()));
        assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("native-exit"), "stale").unwrap();
        fs::write(root.join("account-token-counter.json"), "ledger fixture").unwrap();
        clear_stale_commands(&root).unwrap();
        assert!(!root.join("native-exit").exists());
        assert_eq!(
            fs::read_to_string(root.join("account-token-counter.json")).unwrap(),
            "ledger fixture"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn store_cli_is_copied_and_refreshed_without_execution() {
        let root = env::temp_dir().join(format!("codex-native-cli-test-{}", std::process::id()));
        assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
        let source = root.join("WindowsApps/fixture/app/resources/codex.exe");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"fixture-v1").unwrap();
        let mirror = root.join("mirror");
        let first = mirror_cli(&source, &mirror).unwrap();
        assert_ne!(first, source);
        assert_eq!(fs::read(&first).unwrap(), b"fixture-v1");
        assert_eq!(mirror_cli(&source, &mirror).unwrap(), first);
        fs::write(&source, b"fixture-v2").unwrap();
        let second = mirror_cli(&source, &mirror).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&second).unwrap(), b"fixture-v2");
        let non_store = root.join("codex.exe");
        assert_eq!(mirror_cli(&non_store, &mirror).unwrap(), non_store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn port_fallback_skips_an_occupied_port() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        let selected = available_port(port).unwrap();
        assert_ne!(selected, port);
        assert!(TcpListener::bind(("127.0.0.1", selected)).is_ok());
    }
}
