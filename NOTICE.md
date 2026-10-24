# Notices

Codex Usage Monitor for Windows is derivative software based on runtime-injection ideas from:

- JiaYang-BUAA/Codex-Desktop-Usage-Monitor-Windows: https://github.com/JiaYang-BUAA/Codex-Desktop-Usage-Monitor-Windows

- Fei-Away/Codex-Dream-Skin: https://github.com/Fei-Away/Codex-Dream-Skin
- tree0519/Codex-Dream-Skin-Forge: https://github.com/tree0519/Codex-Dream-Skin-Forge

The Rust native edition adds a Windows executable host and installer while retaining the upstream JavaScript usage and panel implementation. Native runtime packages bundle Node.js; its license and third-party notices are included in runtime/NODE-LICENSE.txt. The legacy script package does not bundle Node.js.

Neither package bundles Codex binaries, user credentials, personal provider configuration, user logs, or token ledgers. Node.js is redistributed under the terms in its bundled license. Rust dependencies and their exact versions are recorded in native/Cargo.lock; their license notices are included in runtime/RUST-LICENSES.txt for native distributions.

Reset probability data is retrieved at runtime from the public API of the MIT-licensed open-source project Codex Reset Observatory:

- gussuri/codex-reset-observatory: https://github.com/gussuri/codex-reset-observatory

No Codex Reset Observatory source code or prediction dataset is bundled in this package.

Codex and OpenAI are trademarks of their respective owners. This project is unofficial and is not affiliated with, sponsored by, or endorsed by OpenAI.
