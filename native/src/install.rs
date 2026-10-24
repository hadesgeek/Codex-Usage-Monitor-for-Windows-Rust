use crate::*;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Seek};
use windows::Win32::UI::Shell::*;
use winreg::{enums::*, RegKey};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexUsageMonitorNative";

pub fn install_base() -> Result<PathBuf> {
    Ok(platform::known_folder(&FOLDERID_LocalAppData)?.join("Programs/CodexUsageMonitor"))
}

pub fn safe_relative(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty() && !value.contains(['\\', ':', '\0']) && !value.starts_with('/'),
        "Unsafe package path"
    );
    for segment in value.split('/') {
        anyhow::ensure!(
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.ends_with([' ', '.']),
            "Unsafe package path"
        );
        let stem = segment.split('.').next().unwrap().to_ascii_uppercase();
        anyhow::ensure!(
            ![
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9"
            ]
            .contains(&stem.as_str()),
            "Reserved Windows path"
        );
    }
    Ok(())
}

fn package_files(root: &Path) -> Result<Vec<String>> {
    let manifest = read_json(&root.join("config/native-package-files.json"), 65536)?;
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for item in manifest
        .as_array()
        .context("Invalid native package manifest")?
    {
        let item = item.as_str().context("Invalid package filename")?;
        safe_relative(item)?;
        anyhow::ensure!(
            seen.insert(item.to_ascii_lowercase()),
            "Duplicate native package file"
        );
        anyhow::ensure!(
            root.join(item).is_file(),
            "Missing native package file: {item}"
        );
        anyhow::ensure!(
            !fs::symlink_metadata(root.join(item))?
                .file_type()
                .is_symlink(),
            "Package symlinks are not allowed"
        );
        files.push(item.to_string());
    }
    for required in [
        "VERSION",
        "CodexUsageMonitor.exe",
        "runtime/node.exe",
        "scripts/injector.mjs",
        "scripts/native-config.mjs",
        "config/native-package-files.json",
    ] {
        anyhow::ensure!(
            files.iter().any(|f| f == required),
            "Native package missing {required}"
        );
    }
    anyhow::ensure!(files.len() <= 512, "Native package has too many files");
    Ok(files)
}

pub fn validate_package(root: &Path) -> Result<()> {
    let files = package_files(root)?;
    runtime::node_path(root)?;
    writeln!(io::stdout(), "{}", json!({"ok":true,"files":files.len()}))?;
    Ok(())
}

fn digest_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    Ok(hash.finalize().to_vec())
}

pub fn install(source: &Path, base: &Path, autostart: bool, interactive: bool) -> Result<()> {
    let files = package_files(source)?;
    // Validate before staging: executing Node in staging can temporarily lock its directory on Windows.
    runtime::node_path(source).context("Cannot validate bundled Node runtime")?;
    let version = fs::read_to_string(source.join("VERSION"))?
        .trim()
        .to_string();
    anyhow::ensure!(valid_version(&version), "Invalid package VERSION");
    let mut hash = Sha256::new();
    for file in &files {
        hash.update(file.as_bytes());
        hash.update(digest_file(&source.join(file))?);
    }
    let fingerprint = format!("{:x}", hash.finalize());
    let directory = format!("{version}-rust-{}", &fingerprint[..12]);
    let target = base.join(&directory);
    fs::create_dir_all(base).context("Cannot create native installation root")?;
    // Immutable version directories keep a failed install away from the running copy.
    if !target.exists() {
        let staging = base.join(format!(".install-{}", std::process::id()));
        fs::create_dir(&staging).context("Cannot create native staging directory")?;
        let result = (|| -> Result<()> {
            for file in &files {
                let destination = staging.join(file);
                fs::create_dir_all(destination.parent().unwrap())?;
                fs::copy(source.join(file), &destination)
                    .with_context(|| format!("Cannot copy native package file: {file}"))?;
                anyhow::ensure!(
                    digest_file(&destination)? == digest_file(&source.join(file))?,
                    "Installed file verification failed"
                );
            }
            fs::rename(&staging, &target).context("Cannot activate native version directory")?;
            Ok(())
        })();
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        result?;
    }
    for file in &files {
        anyhow::ensure!(
            digest_file(&target.join(file))? == digest_file(&source.join(file))?,
            "Installed version is damaged; refusing to activate it"
        );
    }
    let stable = base.join("CodexUsageMonitor.exe");
    if !stable.exists() || !platform::same_path(&env::current_exe()?, &stable) {
        let temporary = base.join(format!("CodexUsageMonitor.{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            fs::copy(target.join("CodexUsageMonitor.exe"), &temporary)?;
            platform::replace_file(&temporary, &stable)
        })();
        let _ = fs::remove_file(temporary);
        result.context("Cannot update stable native launcher")?;
    }
    let current = json!({"schemaVersion":1,"directory":directory,"version":version});
    let changed =
        read_json(&base.join("current.json"), 65536).map_or(true, |previous| previous != current);
    // Only switch the pointer after the entire new runtime has passed verification.
    atomic_write(&base.join("current.json"), &serde_json::to_vec(&current)?)?;
    if !platform::same_path(base, &install_base()?) {
        return Ok(());
    }
    platform::shortcut(
        &stable,
        &platform::known_folder(&FOLDERID_Desktop)?.join("Codex Usage Monitor.lnk"),
        "",
    )?;
    platform::shortcut(
        &stable,
        &platform::known_folder(&FOLDERID_Programs)?.join("Codex Usage Monitor.lnk"),
        "",
    )?;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY)?;
    key.set_value("DisplayName", &"Codex Usage Monitor (Rust)")?;
    key.set_value("DisplayVersion", &NATIVE_VERSION)?;
    key.set_value("DisplayIcon", &stable.to_string_lossy().as_ref())?;
    key.set_value("InstallLocation", &base.to_string_lossy().as_ref())?;
    key.set_value(
        "UninstallString",
        &format!("\"{}\" --uninstall", stable.display()),
    )?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    if autostart {
        set_autostart(true)?;
    }
    if changed {
        // Ask only our own supervisor to switch runtimes; never restart Codex.
        atomic_write(&state_root()?.join("native-updated"), b"updated")?;
    }
    if interactive {
        platform::message(&format!("安装完成：{}\n\n已创建原生 EXE 快捷方式。{}\n请正常退出 Codex，再使用新入口启动。当前会话未被中断。", target.display(), if autostart { "已启用登录自启动。" } else { "已保留原来的自启动选择。" }), false);
    }
    Ok(())
}

pub fn autostart_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|key| key.get_value::<String, _>("CodexUsageMonitor"))
        .is_ok()
}

fn command_executable(command: &str) -> Option<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    let mut count = 0;
    unsafe {
        let args = CommandLineToArgvW(PCWSTR(platform::wide(command).as_ptr()), &mut count);
        if args.is_null() {
            return None;
        }
        let result = if count > 0 {
            (*args).to_string().ok().map(PathBuf::from)
        } else {
            None
        };
        LocalFree(Some(HLOCAL(args.cast())));
        result
    }
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if !enabled {
        match key.delete_value("CodexUsageMonitor") {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        return Ok(());
    }
    let stable = install_base()?.join("CodexUsageMonitor.exe");
    anyhow::ensure!(stable.is_file(), "请先安装，再启用自启动");
    let codex = platform::discover_codex(false)?;
    let backup_path = state_root()?.join("codex-startup-backup.json");
    let mut backup = if backup_path.exists() {
        read_json(&backup_path, 65536)?
    } else {
        json!({"run":{},"shortcuts":[]})
    };
    let mut matching = Vec::new();
    for entry in key.enum_values() {
        let (name, _) = entry?;
        if name == "CodexUsageMonitor" {
            continue;
        }
        if let Ok(command) = key.get_value::<String, _>(&name) {
            if command_executable(&command).is_some_and(|p| platform::same_path(&p, &codex.desktop))
            {
                backup["run"][&name] = Value::String(command);
                matching.push(name);
            }
        }
    }
    let startup = platform::known_folder(&FOLDERID_Startup)?;
    let backup_dir = state_root()?.join("startup-backup");
    let mut links = Vec::new();
    for entry in fs::read_dir(&startup)?.flatten() {
        if entry
            .path()
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("lnk"))
            && platform::shortcut_target(&entry.path())
                .ok()
                .is_some_and(|p| platform::same_path(&p, &codex.desktop))
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if !backup["shortcuts"]
                .as_array()
                .context("Invalid startup backup")?
                .iter()
                .any(|s| s == &name)
            {
                backup["shortcuts"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(name));
            }
            links.push((entry.path(), backup_dir.join(name)));
        }
    }
    fs::create_dir_all(&backup_dir)?;
    for (source, destination) in &links {
        if !destination.exists() {
            fs::copy(source, destination)?;
        }
    }
    atomic_write(&backup_path, &serde_json::to_vec(&backup)?)?;
    // Enable our entry before removing any matching Codex entry; backups survive every retry.
    key.set_value(
        "CodexUsageMonitor",
        &format!("\"{}\" --autostart", stable.display()),
    )?;
    for name in matching {
        key.delete_value(name)?;
    }
    for (source, _) in links {
        fs::remove_file(source)?;
    }
    Ok(())
}

pub fn restore_startup() -> Result<()> {
    let root = state_root()?;
    let file = root.join("codex-startup-backup.json");
    if !file.exists() {
        return Ok(());
    }
    let backup = read_json(&file, 65536)?;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    for (name, value) in backup["run"]
        .as_object()
        .context("Invalid startup backup")?
    {
        // Never overwrite a user setting changed after installation.
        if key.get_raw_value(name).is_err() {
            key.set_value(name, &value.as_str().context("Invalid startup command")?)?;
        }
    }
    for value in backup["shortcuts"]
        .as_array()
        .context("Invalid startup backup")?
    {
        let name = value.as_str().context("Invalid shortcut backup")?;
        safe_relative(name)?;
        anyhow::ensure!(!name.contains('/'), "Invalid startup shortcut name");
        let destination = platform::known_folder(&FOLDERID_Startup)?.join(name);
        if !destination.exists() {
            fs::copy(root.join("startup-backup").join(name), destination)?;
        }
    }
    set_autostart(false)
}

pub fn uninstall(quiet: bool) -> Result<()> {
    if !quiet && !platform::confirm("卸载 Codex Usage Monitor 原生版？\n\n会恢复备份的 Codex 自启动。用量记录和加密凭据会保留，不会退出 Codex。") { return Ok(()); }
    restore_startup()?;
    set_autostart(false)?;
    atomic_write(&state_root()?.join("native-exit"), b"exit")?;
    let stable = install_base()?.join("CodexUsageMonitor.exe");
    for folder in [&FOLDERID_Desktop, &FOLDERID_Programs] {
        let link = platform::known_folder(folder)?.join("Codex Usage Monitor.lnk");
        if link.exists() && platform::shortcut_target(&link).ok().as_ref() == Some(&stable) {
            fs::remove_file(link)?;
        }
    }
    RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY)?;
    let temporary = env::temp_dir().join(format!(
        "CodexUsageMonitor-uninstall-{}.exe",
        std::process::id()
    ));
    fs::copy(env::current_exe()?, &temporary)?;
    let mut command = hidden_command(&temporary);
    command.args(["--cleanup-install", &std::process::id().to_string()]);
    if quiet {
        command.arg("--quiet");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn cleanup(parent_pid: u32, quiet: bool) -> Result<()> {
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    unsafe {
        if let Ok(parent) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            let waited = WaitForSingleObject(parent, 15000);
            CloseHandle(parent)?;
            anyhow::ensure!(
                waited == WAIT_OBJECT_0,
                "Uninstaller is still running; no program files were removed"
            );
        }
    }
    let base = install_base()?;
    cleanup_files(&base)?;
    if !quiet {
        platform::message(
            "原生版已卸载。用量记录、加密凭据和用户添加的文件已保留，Codex 未被关闭。",
            false,
        );
    }
    Ok(())
}

pub fn uninstall_fixture(base: &Path) -> Result<()> {
    let base = fs::canonicalize(base)?;
    let temporary = fs::canonicalize(env::temp_dir())?;
    anyhow::ensure!(
        base.starts_with(&temporary)
            && base != temporary
            && !platform::same_path(&base, &install_base()?),
        "Isolated uninstall is limited to an explicit temporary installation"
    );
    let pointer = read_json(&base.join("current.json"), 4096)?;
    let directory = pointer["directory"]
        .as_str()
        .context("Not a native installation")?;
    safe_relative(directory)?;
    anyhow::ensure!(directory.contains("-rust-"), "Not a native installation");
    package_files(&base.join(directory))?;
    cleanup_files(&base)
}

fn cleanup_files(base: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    anyhow::ensure!(base.is_absolute(), "Invalid installation directory");
    let mut directories = Vec::new();
    for entry in fs::read_dir(base)?.flatten() {
        if entry.file_name().to_string_lossy().contains("-rust-")
            && entry.file_type()?.is_dir()
            && entry.metadata()?.file_attributes() & 0x400 == 0
        {
            if let Ok(files) = package_files(&entry.path()) {
                directories.push((entry.path(), files));
            }
        }
    }
    // Delete only allowlisted native files; leave legacy versions and user-added files intact.
    for (directory, files) in directories {
        for relative in files {
            let mut target = directory.clone();
            let mut reparse = false;
            for component in Path::new(&relative).components() {
                target.push(component);
                if fs::symlink_metadata(&target)
                    .is_ok_and(|meta| meta.file_attributes() & 0x400 != 0)
                {
                    reparse = true;
                    break;
                }
            }
            if !reparse {
                remove_with_retry(&target)?;
            }
        }
        remove_empty_directories(&directory)?;
    }
    remove_with_retry(&base.join("CodexUsageMonitor.exe"))?;
    remove_with_retry(&base.join("current.json"))?;
    let _ = fs::remove_dir(base);
    Ok(())
}

fn remove_with_retry(path: &Path) -> Result<()> {
    for attempt in 0..50 {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) if attempt < 49 => std::thread::sleep(std::time::Duration::from_millis(200)),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn remove_empty_directories(root: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    for entry in fs::read_dir(root)?.flatten() {
        if entry.file_type()?.is_dir() && entry.metadata()?.file_attributes() & 0x400 == 0 {
            remove_empty_directories(&entry.path())?;
        }
    }
    let _ = fs::remove_dir(root);
    Ok(())
}

pub fn valid_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && (p.len() == 1 || !p.starts_with('0'))
                && p.len() <= 6
                && p.bytes().all(|c| c.is_ascii_digit())
        })
}

pub fn package(source: &Path, output: &Path) -> Result<()> {
    let files = package_files(source)?;
    let version = fs::read_to_string(source.join("VERSION"))?
        .trim()
        .to_owned();
    anyhow::ensure!(valid_version(&version), "Invalid package version");
    let temporary = output.with_extension("zip.tmp");
    let result = (|| -> Result<()> {
        let file = fs::File::create(&temporary)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for relative in files {
            zip.start_file(
                format!("codex-usage-monitor-windows-{version}/{relative}"),
                options,
            )?;
            io::copy(&mut fs::File::open(source.join(relative))?, &mut zip)?;
        }
        zip.finish()?;
        platform::replace_file(&temporary, output)?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn extract_update(archive: &Path, sha256: &str, version: &str, destination: &Path) -> Result<()> {
    anyhow::ensure!(valid_version(version), "Invalid release version");
    anyhow::ensure!(
        sha256.len() == 64 && sha256.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid SHA-256"
    );
    let mut file = fs::File::open(archive)?;
    anyhow::ensure!(
        file.metadata()?.len() <= 64 * 1024 * 1024,
        "Archive exceeds size limit"
    );
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    anyhow::ensure!(
        format!("{:x}", hash.finalize()) == sha256.to_ascii_lowercase(),
        "Update SHA-256 changed before installation"
    );
    file.rewind()?;
    let mut zip = zip::ZipArchive::new(file)?;
    anyhow::ensure!(
        !zip.is_empty() && zip.len() <= 512,
        "Invalid ZIP entry count"
    );
    let prefix = format!("codex-usage-monitor-windows-{version}/");
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        let relative = entry
            .name()
            .strip_prefix(&prefix)
            .context("Unexpected ZIP root")?
            .trim_end_matches('/');
        if relative.is_empty() {
            continue;
        }
        safe_relative(relative)?;
        anyhow::ensure!(
            seen.insert(relative.to_ascii_lowercase()),
            "Duplicate ZIP entry"
        );
        anyhow::ensure!(
            entry.unix_mode().unwrap_or(0) & 0xf000 != 0xa000,
            "ZIP symlinks are not allowed"
        );
        // Only the bundled Node binary may exceed the legacy 64 MiB per-file ceiling.
        let maximum = if relative == "runtime/node.exe" {
            100 * 1024 * 1024
        } else {
            64 * 1024 * 1024
        };
        anyhow::ensure!(entry.size() <= maximum, "ZIP entry exceeds size limit");
        total = total
            .checked_add(entry.size())
            .context("ZIP size overflow")?;
        anyhow::ensure!(total <= 128 * 1024 * 1024, "ZIP expands beyond size limit");
    }
    anyhow::ensure!(
        seen.contains("codexusagemonitor.exe") && seen.contains("config/native-package-files.json"),
        "Release has no native runtime; keeping current installation"
    );
    fs::create_dir(destination)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let relative = entry
            .name()
            .strip_prefix(&prefix)
            .unwrap()
            .trim_end_matches('/');
        if relative.is_empty() || entry.is_dir() {
            continue;
        }
        let target = destination.join(relative);
        fs::create_dir_all(target.parent().unwrap())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let expected = entry.size();
        let bytes = io::copy(&mut entry.by_ref().take(expected + 1), &mut output)?;
        anyhow::ensure!(bytes == expected, "ZIP entry size mismatch");
    }
    anyhow::ensure!(
        fs::read_to_string(destination.join("VERSION"))?.trim() == version,
        "Release VERSION mismatch"
    );
    package_files(destination)?;
    Ok(())
}

pub fn apply_update(args: &[String]) -> Result<()> {
    anyhow::ensure!(args.len() == 4, "Expected archive, digest, version, port");
    let version = &args[2];
    let _: u16 = args[3].parse()?;
    let root = state_root()?;
    let archive = fs::canonicalize(&args[0])?;
    let updates = fs::canonicalize(root.join("updates"))?;
    anyhow::ensure!(
        archive.parent() == Some(updates.as_path())
            && archive.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(&format!("codex-usage-monitor-windows-{version}.zip."))
                    && name.ends_with(".download")
            }),
        "Update archive is outside the updater's owned download directory"
    );
    let staging = root
        .join("updates")
        .join(format!("native-extract-{}", std::process::id()));
    let result = (|| -> Result<()> {
        extract_update(&archive, &args[1], version, &staging)?;
        install(&staging, &install_base()?, false, false)?;
        atomic_write(
            &root.join("update-state.json"),
            &serde_json::to_vec(
                &json!({"schemaVersion":1,"checkedAt":read_json(&root.join("update-state.json"),65536)?["checkedAt"],"status":"installed","currentVersion":version}),
            )?,
        )?;
        Ok(())
    })();
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::remove_file(&archive)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_keeps_user_added_files_and_removes_only_empty_directories() {
        let root =
            env::temp_dir().join(format!("codex-native-cleanup-test-{}", std::process::id()));
        assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("assets/owned.js"), "fixture").unwrap();
        fs::write(root.join("assets/private.local.json"), "user fixture").unwrap();
        remove_with_retry(&root.join("assets/owned.js")).unwrap();
        remove_with_retry(&root.join("already-missing")).unwrap();
        remove_empty_directories(&root).unwrap();
        assert!(!root.join("assets/owned.js").exists());
        assert!(!root.join("empty").exists());
        assert_eq!(
            fs::read_to_string(root.join("assets/private.local.json")).unwrap(),
            "user fixture"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn zip_validation_rejects_unsafe_paths_and_legacy_packages_before_extraction() {
        let root = env::temp_dir().join(format!("codex-native-zip-test-{}", std::process::id()));
        assert_eq!(root.parent(), Some(env::temp_dir().as_path()));
        fs::create_dir(&root).unwrap();
        for (index, (name, expected)) in [
            (
                "codex-usage-monitor-windows-3.2.1/../escape",
                "Unsafe package path",
            ),
            (
                "codex-usage-monitor-windows-3.2.1/CON.txt",
                "Reserved Windows path",
            ),
            (
                "codex-usage-monitor-windows-3.2.1/a:stream",
                "Unsafe package path",
            ),
            ("wrong-root/VERSION", "Unexpected ZIP root"),
            (
                "codex-usage-monitor-windows-3.2.1/VERSION",
                "no native runtime",
            ),
        ]
        .iter()
        .enumerate()
        {
            let archive = root.join(format!("{index}.zip"));
            let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fixture").unwrap();
            zip.finish().unwrap();
            let digest = format!("{:x}", Sha256::digest(fs::read(&archive).unwrap()));
            let destination = root.join(format!("extract-{index}"));
            let error = extract_update(&archive, &digest, "3.2.1", &destination).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert!(!destination.exists());
            assert!(
                extract_update(&archive, &"0".repeat(64), "3.2.1", &destination)
                    .unwrap_err()
                    .to_string()
                    .contains("SHA-256")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_windows_path_attacks() {
        for path in [
            "../escape",
            "/absolute",
            "C:/escape",
            "a\\b",
            "a/../b",
            "a/./b",
            "CON.txt",
            "a/LPT1",
            "a/trailing.",
            "a//b",
            "a/ads:stream",
        ] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        assert!(safe_relative("scripts/usage/history.mjs").is_ok());
    }
    #[test]
    fn version_validation() {
        assert!(valid_version("3.2.1"));
        for v in ["../3.2.1", "03.2.1", "3.2", "3.2.1-beta", "3.2.9999999"] {
            assert!(!valid_version(v));
        }
    }
    #[test]
    fn startup_command_matches_executable_only() {
        assert_eq!(
            command_executable(r#""C:\Apps\Codex\ChatGPT.exe" --startup"#),
            Some(PathBuf::from(r"C:\Apps\Codex\ChatGPT.exe"))
        );
        assert_ne!(
            command_executable(r#""C:\Other\app.exe" --label Codex"#),
            Some(PathBuf::from(r"C:\Apps\Codex\ChatGPT.exe"))
        );
    }
}
