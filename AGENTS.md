# Codex Instructions — Rust Native Edition

This repository contains only the native Windows edition. Read `README.md` and `docs/native-windows.md` first.

## Architecture

- Rust owns installation, uninstall, Codex discovery/launch, tray, DPAPI and startup migration.
- Node.js owns the existing CDP integration, usage collection, configuration bridge and embedded UI. Do not add deployment or runtime shell scripts.
- Native shortcuts point directly to `CodexUsageMonitor.exe` and use its embedded icon.
- Inno Setup is the declarative single-file installer build configuration; it delegates installation and uninstall registration to Rust.

## Safety and data

- Never terminate or restart Codex. If an existing instance lacks CDP, explain that the user must exit it normally and use the monitor shortcut.
- Discover current-user Store and common non-Store installations; only ask for real `ChatGPT.exe` or `codex.exe` files when discovery fails. Do not invent paths.
- Never modify WindowsApps, app.asar, Codex login files or model configuration. Mirror the Store CLI into the monitor's user-state runtime before execution.
- Bind CDP to `127.0.0.1`, retain port fallback and verify the actual port in the monitor's `state.json`.
- Startup migration affects only confirmed current-user Codex executable entries, with backup and restore. Do not modify machine startup or undocumented StartupApproved data.
- Preserve user data, Token ledgers, private provider files and DPAPI compatibility when updating or uninstalling. Remove only allowlisted program files.
- Never ask for credentials in chat, source, JSON, command arguments or logs. Guide users to the expanded local panel. Official subscription needs no API key. Unknown provider fields require public documentation or a sanitized example.
- API account access tokens and API keys are not interchangeable. A cumulative Token baseline must be a real non-negative integer; explain the use of zero when the exact value is unknown.
- Keep opt-in updates, the 24-hour schedule, newer formal releases, exact asset names, GitHub SHA-256, safe ZIP boundaries and failure preservation. Never restart Codex for updates.

## Changes and verification

Use `npm test` for the complete Node suite, `cargo test --locked --manifest-path native/Cargo.toml`, and `cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings`.

After installer or package-manifest changes, run `npm run build`: this builds the native runtime and real Setup EXE, then verifies installation, reinstall, hashes, uninstall and user-file preservation in exact temporary directories. Do not substitute file existence for real runtime verification.

Report build/test results separately from live CDP, panel and data-source results. Never claim that the monitor is visible without a verified Codex session. Do not commit generated caches, runtime binaries, personal files or credentials.
