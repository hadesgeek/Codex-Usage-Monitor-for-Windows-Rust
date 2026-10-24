#![cfg_attr(not(test), windows_subsystem = "windows")]

mod install;
mod platform;
mod runtime;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const NATIVE_VERSION: &str = "3.2.0-rust.1";

fn state_root() -> Result<PathBuf> {
    Ok(
        platform::known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)?
            .join("CodexUsageMonitor"),
    )
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("Missing parent directory")?)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        fs::write(&temp, bytes)?;
        platform::replace_file(&temp, path)
    })();
    let _ = fs::remove_file(temp);
    result
}

fn read_json(path: &Path, limit: u64) -> Result<Value> {
    let file = fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.len() <= limit,
        "JSON file exceeds size limit"
    );
    Ok(serde_json::from_reader(file)?)
}

fn read_input(limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    io::stdin().take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= limit, "Input exceeds size limit");
    Ok(bytes)
}

fn hidden_command(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(exe);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn main() {
    if let Err(error) = run() {
        // Credential helpers only return a generic failure through their private pipe.
        if env::args().any(|arg| arg == "--save-secret") {
            std::process::exit(1);
        }
        let message = format!("{error:#}");
        if env::args().any(|arg| {
            matches!(
                arg.as_str(),
                "--diagnose"
                    | "--validate-package"
                    | "--apply-update"
                    | "--package"
                    | "--quiet"
                    | "--install-root"
            )
        }) {
            let _ = writeln!(io::stdout(), "{}", json!({"ok": false, "message": message}));
        } else {
            if let Ok(root) = state_root() {
                let _ = atomic_write(&root.join("launcher-error.log"), message.as_bytes());
            }
            platform::message(&message, true);
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    platform::initialize()?;
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--save-secret") {
        let kind = args.get(1).context("Missing credential kind")?;
        let bytes = read_input(16384)?;
        anyhow::ensure!(
            !bytes.is_empty() && bytes.iter().all(|c| (33..=126).contains(c)),
            "Invalid credential"
        );
        let (name, entropy) = credential_kind(kind)?;
        let protected = platform::protect(&bytes, entropy, false)?;
        atomic_write(&state_root()?.join(name), &protected)?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--apply-update") {
        return install::apply_update(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("--package") {
        anyhow::ensure!(args.len() == 3, "Expected package source and output ZIP");
        return install::package(Path::new(&args[1]), Path::new(&args[2]));
    }
    if args.first().map(String::as_str) == Some("--cleanup-install") {
        return install::cleanup(
            args.get(1)
                .context("Missing uninstaller process ID")?
                .parse()?,
            args.iter().any(|arg| arg == "--quiet"),
        );
    }
    if args.iter().any(|arg| arg == "--prepare-install") {
        return platform::prepare_install(!args.iter().any(|arg| arg == "--quiet"));
    }
    let exe = env::current_exe()?;
    let mut root = if let Some(index) = args.iter().position(|arg| arg == "--root") {
        fs::canonicalize(args.get(index + 1).context("Missing --root value")?)?
    } else {
        exe.parent()
            .context("Missing executable directory")?
            .to_owned()
    };
    if !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--root"
                | "--uninstall"
                | "--enable-autostart"
                | "--disable-autostart"
                | "--restore-codex-startup"
                | "--reload"
        )
    }) && root == install::install_base()?
    {
        let pointer = read_json(&root.join("current.json"), 4096)?;
        let directory = pointer["directory"]
            .as_str()
            .context("Invalid current version")?;
        install::safe_relative(directory)?;
        root = root.join(directory);
    }
    if args.iter().any(|arg| arg == "--validate-package") {
        return install::validate_package(&root);
    }
    if args.iter().any(|arg| arg == "--install") {
        let base = if let Some(index) = args.iter().position(|arg| arg == "--install-root") {
            PathBuf::from(args.get(index + 1).context("Missing install root")?)
        } else {
            install::install_base()?
        };
        return install::install(
            &root,
            &base,
            args.iter().any(|a| a == "--enable-autostart"),
            !args.iter().any(|a| a == "--quiet"),
        );
    }
    if args.iter().any(|arg| arg == "--enable-autostart") {
        return install::set_autostart(true);
    }
    if args.iter().any(|arg| arg == "--disable-autostart") {
        return install::set_autostart(false);
    }
    if args.iter().any(|arg| arg == "--restore-codex-startup") {
        return install::restore_startup();
    }
    if args.iter().any(|arg| arg == "--uninstall") {
        if let Some(index) = args.iter().position(|arg| arg == "--install-root") {
            return install::uninstall_fixture(Path::new(
                args.get(index + 1).context("Missing install root")?,
            ));
        }
        return install::uninstall(args.iter().any(|arg| arg == "--quiet"));
    }
    if args.iter().any(|arg| arg == "--diagnose") {
        let codex = platform::discover_codex(false)?;
        let node = runtime::node_path(&root)?;
        writeln!(
            io::stdout(),
            "{}",
            json!({"ok":true,"version":NATIVE_VERSION,"root":root,"node":node,"codex":codex,"autostart":install::autostart_enabled(),"cdpPort":runtime::find_cdp_port()})
        )?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--reload") {
        return atomic_write(&state_root()?.join("native-reload"), b"reload");
    }
    // The stable entry delegates to the versioned host, so updates never overwrite a running EXE.
    if !args.iter().any(|arg| arg == "--run" || arg == "--portable") {
        let current_file = install::install_base()?.join("current.json");
        if exe.parent() == Some(install::install_base()?.as_path()) && current_file.exists() {
            let current = read_json(&current_file, 4096)?;
            let version = current["directory"]
                .as_str()
                .context("Invalid current version")?;
            install::safe_relative(version)?;
            let target = install::install_base()?
                .join(version)
                .join("CodexUsageMonitor.exe");
            hidden_command(target)
                .arg("--run")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }
        if !args.iter().any(|arg| arg == "--root") {
            if platform::confirm("安装 Codex Usage Monitor？\n\n安装到当前用户目录，创建图标与卸载入口，开启登录自启动，安装完成后立即启动工具。不会强制退出或重启 Codex。") {
                platform::prepare_install(true)?;
                install::install(&root, &install::install_base()?, true, false)?;
                hidden_command(install::install_base()?.join("CodexUsageMonitor.exe")).spawn()?;
            }
            return Ok(());
        }
    }
    runtime::run(root, !args.iter().any(|arg| arg == "--autostart"))
}

fn credential_kind(kind: &str) -> Result<(&'static str, &'static [u8])> {
    match kind {
        "provider" => Ok(("api-key.dpapi", b"CodexUsageMonitor/v1/APIKey")),
        "account" => Ok((
            "account-token.dpapi",
            b"CodexUsageMonitor/v1/APIAccountToken",
        )),
        _ => bail!("Unknown credential kind"),
    }
}

fn load_credentials(command: &mut Command) -> Result<()> {
    let state = state_root()?;
    command.env("CODEX_USAGE_NATIVE_HOST", env::current_exe()?);
    command.env(
        "CODEX_USAGE_ACCOUNT_COUNTER_PATH",
        state.join("account-token-counter.json"),
    );
    for (kind, variable, disabled) in [
        (
            "provider",
            "CODEX_USAGE_API_KEY",
            "CODEX_USAGE_DISABLE_PERSISTED_PROVIDER",
        ),
        (
            "account",
            "CODEX_USAGE_ACCOUNT_TOKEN",
            "CODEX_USAGE_DISABLE_PERSISTED_ACCOUNT",
        ),
    ] {
        if env::var(disabled).as_deref() == Ok("1") || env::var_os(variable).is_some() {
            continue;
        }
        let (name, entropy) = credential_kind(kind)?;
        let secret = state.join(name);
        let config_path = state.join(if kind == "account" {
            "account.json"
        } else {
            "provider.json"
        });
        if !secret.exists() && !config_path.exists() {
            continue;
        }
        let config = read_json(&config_path, 131072)?;
        let encrypted = fs::read(&secret)?;
        anyhow::ensure!(
            encrypted.len() < 65536,
            "Encrypted credential exceeds size limit"
        );
        let mut plain = platform::protect(&encrypted, entropy, true)?;
        let text = std::str::from_utf8(&plain)?;
        anyhow::ensure!(
            !text.is_empty() && text.bytes().all(|c| (33..=126).contains(&c)),
            "Invalid stored credential"
        );
        command.env(variable, text);
        plain.fill(0);
        if kind == "account" {
            command.env(
                "CODEX_USAGE_ACCOUNT_BASE_URL",
                config["baseUrl"].as_str().context("Missing account URL")?,
            );
            command.env(
                "CODEX_USAGE_ACCOUNT_USER_ID",
                config["userId"]
                    .as_str()
                    .context("Missing account user ID")?,
            );
        } else {
            command.env("CODEX_USAGE_PROVIDER_CONFIG_PATH", config_path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpapi_compatible_entropy_and_roundtrip() {
        platform::initialize().unwrap();
        let plain = b"unit-test-native-not-a-real-key";
        let encrypted =
            platform::protect(plain, credential_kind("provider").unwrap().1, false).unwrap();
        assert_ne!(encrypted, plain);
        assert_eq!(
            platform::protect(&encrypted, credential_kind("provider").unwrap().1, true).unwrap(),
            plain
        );
        assert!(
            platform::protect(&encrypted, credential_kind("account").unwrap().1, true).is_err()
        );
    }
}
