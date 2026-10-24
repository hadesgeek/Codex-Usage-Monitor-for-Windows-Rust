use crate::*;
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
use windows::{
    core::{w, Interface, HSTRING, PCWSTR},
    Management::Deployment::PackageManager,
    Win32::{
        Foundation::*,
        Security::Cryptography::*,
        Storage::FileSystem::*,
        System::{Com::*, Diagnostics::ToolHelp::*, Threading::*},
        UI::{Controls::Dialogs::*, Shell::*, WindowsAndMessaging::*},
    },
};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn initialize() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    Ok(())
}

pub fn message(text: &str, error: bool) {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(text).as_ptr()),
            w!("Codex Usage Monitor"),
            MB_OK
                | if error {
                    MB_ICONERROR
                } else {
                    MB_ICONINFORMATION
                },
        );
    }
}

pub fn confirm(text: &str) -> bool {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(text).as_ptr()),
            w!("Codex Usage Monitor"),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES
    }
}

pub fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
    unsafe {
        let value = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let result = PathBuf::from(value.to_string()?);
        CoTaskMemFree(Some(value.0.cast()));
        Ok(result)
    }
}

pub fn replace_file(source: &Path, target: &Path) -> Result<()> {
    unsafe {
        MoveFileExW(
            PCWSTR(wide(source).as_ptr()),
            PCWSTR(wide(target).as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

pub fn protect(input: &[u8], entropy: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    let blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if decrypt {
            CryptUnprotectData(
                &blob,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        } else {
            CryptProtectData(
                &blob,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        }
        let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = bytes.to_vec();
        bytes.fill(0);
        LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Codex {
    pub desktop: PathBuf,
    pub aumid: Option<String>,
    pub cli: Option<PathBuf>,
}

fn find_cli(desktop: &Path) -> Option<PathBuf> {
    let dir = desktop.parent()?;
    let parent = dir.parent().unwrap_or(dir);
    let candidates = [
        env::var_os("CODEX_USAGE_CODEX_PATH").map(PathBuf::from),
        Some(dir.join("resources/codex.exe")),
        Some(parent.join("resources/codex.exe")),
        Some(dir.join("codex.exe")),
        Some(parent.join("codex.exe")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .or_else(|| {
            env::split_paths(&env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join("codex.exe"))
                .find(|p| p.is_file())
        })
}

fn desktop_result(path: PathBuf) -> Result<Codex> {
    anyhow::ensure!(path.is_file(), "选择的 Codex 文件不存在");
    anyhow::ensure!(
        path.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("ChatGPT.exe")),
        "请选择真实的 ChatGPT.exe"
    );
    let mut cli = find_cli(&path);
    if let Ok(saved) = read_json(&state_root()?.join("native-paths.json"), 8192) {
        if saved["desktop"]
            .as_str()
            .is_some_and(|desktop| same_path(Path::new(desktop), &path))
        {
            if let Some(selected) = saved["cli"].as_str().map(PathBuf::from) {
                if selected.is_file()
                    && selected
                        .file_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe"))
                {
                    cli = Some(selected);
                }
            }
        }
    }
    Ok(Codex {
        cli,
        desktop: path,
        aumid: None,
    })
}

pub fn prepare_install(interactive: bool) -> Result<()> {
    let mut codex = discover_codex(interactive)?;
    if codex.cli.is_none() && interactive {
        codex.cli = select_exe("未发现官方 CLI，请选择真实的 codex.exe（不是 ChatGPT.exe）");
    }
    let cli = codex
        .cli
        .as_ref()
        .context("未找到官方 codex.exe。请安装完整的 Codex Desktop，或重新安装并选择真实文件。")?;
    anyhow::ensure!(
        cli.is_file()
            && cli
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe")),
        "请选择真实的 codex.exe"
    );
    if codex.aumid.is_none() {
        atomic_write(
            &state_root()?.join("native-paths.json"),
            &serde_json::to_vec(&codex)?,
        )?;
    }
    Ok(())
}

pub fn discover_codex(interactive: bool) -> Result<Codex> {
    if let Some(path) = env::var_os("CODEX_USAGE_DESKTOP_PATH") {
        return desktop_result(path.into());
    }
    // Query the current user's package registrations; never scan or write WindowsApps.
    if let Ok(packages) = PackageManager::new()?.FindPackagesByUserSecurityId(&HSTRING::new()) {
        let mut matches = Vec::new();
        for package in packages {
            let id = package.Id()?;
            let name = id.Name()?.to_string();
            if name != "OpenAI.Codex"
                && env::var("CODEX_USAGE_APP_PACKAGE_NAME").as_deref() != Ok(name.as_str())
            {
                continue;
            }
            let directory = PathBuf::from(package.InstalledLocation()?.Path()?.to_string());
            let desktop = directory.join("app/ChatGPT.exe");
            if !desktop.is_file() {
                continue;
            }
            let entries = package.GetAppListEntriesAsync()?.join()?;
            if entries.Size()? == 0 {
                continue;
            }
            let version = id.Version()?;
            matches.push((
                (
                    version.Major,
                    version.Minor,
                    version.Build,
                    version.Revision,
                ),
                Codex {
                    cli: find_cli(&desktop),
                    desktop,
                    aumid: Some(entries.GetAt(0)?.AppUserModelId()?.to_string()),
                },
            ));
        }
        matches.sort_by_key(|entry| entry.0);
        if let Some((_, codex)) = matches.pop() {
            return Ok(codex);
        }
    }
    let saved = state_root()?.join("native-paths.json");
    if let Ok(value) = read_json(&saved, 8192) {
        if let Some(path) = value["desktop"].as_str() {
            if Path::new(path).is_file() {
                return desktop_result(PathBuf::from(path));
            }
        }
    }
    let mut roots = Vec::new();
    if let Ok(local) = known_folder(&FOLDERID_LocalAppData) {
        roots.push(local.join("Programs"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = env::var_os(var) {
            roots.push(p.into());
        }
    }
    for root in &roots {
        for relative in [
            "ChatGPT/ChatGPT.exe",
            "OpenAI Codex/ChatGPT.exe",
            "OpenAI/Codex/ChatGPT.exe",
            "Codex/app/ChatGPT.exe",
        ] {
            let path = root.join(relative);
            if path.is_file() {
                return desktop_result(path);
            }
        }
    }
    for root in roots {
        if let Some(path) = scan_desktop(&root, 4) {
            return desktop_result(path);
        }
    }
    if interactive {
        if let Some(path) = select_exe("未发现 Codex，请选择真实的 ChatGPT.exe") {
            let result = desktop_result(path)?;
            atomic_write(&saved, &serde_json::to_vec(&result)?)?;
            return Ok(result);
        }
    }
    bail!("未找到 Codex Desktop。请运行程序并选择真实的 ChatGPT.exe。")
}

fn scan_desktop(root: &Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let kind = entry.file_type().ok()?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_file() && entry.file_name().eq_ignore_ascii_case("ChatGPT.exe") {
            return Some(entry.path());
        }
        if kind.is_dir()
            && !["WindowsApps", "node_modules", "Temp"]
                .iter()
                .any(|n| entry.file_name().eq_ignore_ascii_case(n))
        {
            if let Some(result) = scan_desktop(&entry.path(), depth - 1) {
                return Some(result);
            }
        }
    }
    None
}

pub fn select_exe(title: &str) -> Option<PathBuf> {
    let mut buffer = [0u16; 32768];
    let title = wide(title);
    let filter = wide("Windows 应用程序 (*.exe)\0*.exe\0\0");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        Some(PathBuf::from(String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|c| *c == 0)?],
        )))
    } else {
        None
    }
}

pub fn launch(codex: &Codex, port: u16) -> Result<u32> {
    let args = format!("--remote-debugging-port={port} --remote-debugging-address=127.0.0.1 --remote-allow-origins=http://127.0.0.1:{port}");
    if let Some(aumid) = &codex.aumid {
        unsafe {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)?;
            return Ok(manager.ActivateApplication(
                PCWSTR(wide(aumid).as_ptr()),
                PCWSTR(wide(args).as_ptr()),
                AO_NOERRORUI,
            )?);
        }
    }
    Ok(hidden_command(&codex.desktop)
        .args(args.split_whitespace())
        .spawn()?
        .id())
}

pub fn running(codex: &Codex) -> Result<bool> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len())],
            );
            if name.eq_ignore_ascii_case("ChatGPT.exe") {
                if let Ok(process) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    entry.th32ProcessID,
                ) {
                    let mut buffer = [0u16; 32768];
                    let mut size = buffer.len() as u32;
                    if QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok()
                    {
                        found |= same_path(
                            Path::new(&String::from_utf16_lossy(&buffer[..size as usize])),
                            &codex.desktop,
                        );
                    }
                    let _ = CloseHandle(process);
                }
            }
            if found {
                break;
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        CloseHandle(snapshot)?;
        Ok(found)
    }
}

pub fn same_path(first: &Path, second: &Path) -> bool {
    first
        .to_string_lossy()
        .replace('/', "\\")
        .eq_ignore_ascii_case(&second.to_string_lossy().replace('/', "\\"))
}

pub fn shortcut(target: &Path, destination: &Path, args: &str) -> Result<()> {
    fs::create_dir_all(destination.parent().context("Missing shortcut parent")?)?;
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(wide(target).as_ptr()))?;
        link.SetArguments(PCWSTR(wide(args).as_ptr()))?;
        link.SetIconLocation(PCWSTR(wide(target).as_ptr()), 0)?;
        link.SetWorkingDirectory(PCWSTR(
            wide(target.parent().context("Missing target parent")?).as_ptr(),
        ))?;
        link.SetDescription(w!("Codex Usage Monitor"))?;
        let persist: IPersistFile = link.cast()?;
        persist.Save(PCWSTR(wide(destination).as_ptr()), true)?;
    }
    Ok(())
}

pub fn shortcut_target(path: &Path) -> Result<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist: IPersistFile = link.cast()?;
        persist.Load(PCWSTR(wide(path).as_ptr()), STGM_READ)?;
        let mut buffer = [0u16; 32768];
        link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)?;
        Ok(PathBuf::from(String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len())],
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn fake_desktop_child() {
        std::thread::sleep(std::time::Duration::from_secs(15));
    }

    #[test]
    fn detects_only_the_discovered_desktop_process() {
        let root =
            env::temp_dir().join(format!("codex-native-process-test-{}", std::process::id()));
        assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
        fs::create_dir(&root).unwrap();
        let exe = root.join("ChatGPT.exe");
        fs::copy(env::current_exe().unwrap(), &exe).unwrap();
        let mut child = hidden_command(&exe)
            .args([
                "--ignored",
                "--exact",
                "platform::tests::fake_desktop_child",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let desktop = PathBuf::from(exe.to_string_lossy().replace('\\', "/"));
        let result = running(&Codex {
            desktop,
            aumid: None,
            cli: None,
        });
        let not_ours = running(&Codex {
            desktop: root.join("different/ChatGPT.exe"),
            aumid: None,
            cli: None,
        });
        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.unwrap());
        assert!(!not_ours.unwrap());
        assert!(same_path(
            Path::new(r"C:\Apps\Codex\ChatGPT.exe"),
            Path::new("c:/apps/codex/ChatGPT.exe")
        ));
    }
}
