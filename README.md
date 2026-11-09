# Singboard

`Singboard` is a sing-box dashboard built with `Tauri 2 + Vue 3`. One codebase ships the Windows panel, focused on Clash API visualization and Windows service management, and the Android app, which works as a remote panel for any reachable Clash API and can run a local core on rooted devices.

## Windows features

- Overview
  - Realtime upload/download speed, active connection count, and memory usage
  - Network information (IP info + connectivity/latency checks for common sites)
  - Connection topology (Sankey chart)
- Proxies
  - Proxy group switching and per-node latency testing
  - Per-group latency test URL support
  - Optional IPv6 reachability indicator
  - Proxy provider list, single/all update, and health check
- Rules
  - Rule list filtering
  - Rule provider list and update actions
  - Rule provider content search (SRS/cache.db based matching)
- Connections
  - Active/closed connection tabs
  - Connection detail modal
  - Disconnect single/all and pause updates
- Logs
  - Realtime log stream
  - Level filter, keyword filter, pause, clear, and auto-scroll
- Config Editor
  - Read/write `config.json`
  - Whole-file and module-based editing modes
  - JSON formatting
  - `sing-box check` validation
  - JSON validation before save, plus automatic `.bak` backup on write
- Settings
  - Windows service install/uninstall/start/stop/restart
  - Service status polling and error log reading
  - Multi Clash API profiles (add/edit/switch/remove)
  - Clash mode switching from core-reported mode list
  - `sing-box` path, config path, working directory, and service name settings
  - Theme switching (`light`/`dark`)
- First-run Setup Wizard
  - Guided working directory and Clash API setup
  - Auto-scan for `sing-box` executable and `config.json` under the selected directory

## Android features

- Floating bottom bar: Home, Proxies, Connections, Logs, Rules, Settings; swiping between pages is an opt-in setting (off by default)
- Home (opens first)
  - Running state, core version and uptime
  - Core on/off switch and restart (root mode only)
  - Realtime upload/download speed with totals
  - Memory usage, plus CPU usage of the core process (root mode only)
  - Network info: IP lookups and connectivity checks
- Proxies: group switching, latency tests, per-group test URLs, providers
- Rules: rule list, rule providers, provider content search (root mode)
- Connections: active/closed tabs, details, disconnect
- Logs: realtime stream with level/keyword filters
- Settings: Clash API endpoints, DNS query tool, themes, glass styles, custom wallpapers, updates
- Remote mode (no root required)
  - Connects to sing-box running on a router, a PC or another app
  - Connection state and core version come from the Clash API
- Root mode (Magisk / KernelSU / APatch)
  - Point the app at a sing-box working directory; it searches the directory and its subdirectories (4 levels) for:
    - the core: ELF executables in a `bin` folder first, otherwise files named `sing-box*` / `singbox*`
    - the configuration: JSON files containing `"outbounds"`, preferring `config.json`
    - rule sets: a `rules` / `ruleset` / `rule-set` style folder, otherwise the folder holding the most `.srs` files
  - Start, stop and restart the core; it keeps running after the panel is closed
  - Stopping sends SIGTERM and waits for the core to exit on its own (up to 60 s) so TUN `auto_redirect` rules are cleaned up; the core is never force-killed
  - Core logs follow `log.output` in the configuration; singboard keeps its own log in `/data/adb/singboard/logs`
  - Start on boot through `/data/adb/service.d/singboard.sh` (Magisk / KernelSU / APatch run it after boot completes)
  - Core updates from GitHub releases (`android-*.tar.gz`), verified by SHA-256 and rolled back if the new core fails to start; without a core, it installs to `bin/sing-box` (or `sing-box` in the working directory)
  - Every root operation goes through one long-lived `su` shell, so the root manager prompts once
- In-app updates
  - Checks `Yuu518/singboard` releases on startup (can be disabled)
  - Downloads the APK for the device ABI, verifies its SHA-256 digest, then opens the system installer

### Root layout

The working directory stays yours; singboard only keeps its service files in a fixed place so the boot hook can find them:

```text
/data/adb/singboard/
├─ singboard.sh      # service script: start | stop | restart | status | log
├─ service.env       # detected core, config and working directory
├─ logs/
│  └─ singboard.log  # service events plus core stdout/stderr, trimmed at 1 MB
└─ run/
   └─ sing-box.pid
/data/adb/service.d/singboard.sh   # boot hook
```

The service script is refreshed with the detected paths every time the core is started from the app. It can also be used from a root shell, for example `sh /data/adb/singboard/singboard.sh restart`.

## Tech Stack

- Frontend: `Vue 3`, `TypeScript`, `Vite`, `TailwindCSS`, `DaisyUI`, `ECharts`, `CodeMirror 6`
- Backend: `Rust`, `Tauri 2`
- Platforms: `Windows` (with Windows Service integration) and `Android` (remote panel, plus root mode)

## Requirements

- Node.js 18+
- `pnpm`
- Rust stable (recommended via `rustup`)
- Windows C++ Build Tools (MSVC)
- WebView2 Runtime
- For Android: JDK 17+, Android SDK with platform 36, build-tools 36 and NDK r30 (`ANDROID_HOME`, `NDK_HOME`), and the Rust targets `aarch64-linux-android` and `armv7-linux-androideabi`

## Development

Install dependencies:

```bash
pnpm install
```

Run frontend only:

```bash
pnpm dev
```

Run desktop app in development mode:

```bash
pnpm tauri dev
```

Run the Android app on a connected device or emulator:

```bash
pnpm tauri android dev
```

The platform is fixed at build time: the Tauri CLI sets `TAURI_ENV_PLATFORM`, and the frontend only bundles the code for that platform. Shared logic (API, stores, bridges, utilities) lives in `src/`, while the Android shell, pages and Android-only components live in `src/android/`. To preview the Android interface in a desktop browser, run `TAURI_ENV_PLATFORM=android pnpm dev`.

Run tests for both platforms:

```bash
pnpm test
```

## Build

Build frontend:

```bash
pnpm build
```

Build desktop app:

```bash
pnpm tauri build
```

Build Android APKs:

```bash
pnpm tauri android build --apk --split-per-abi --target aarch64 armv7
```

APKs are written to `src-tauri/gen/android/app/build/outputs/apk/<abi>/release/`. Release builds are signed when `src-tauri/gen/android/keystore.properties` exists:

```properties
storeFile=release.jks
storePassword=...
keyAlias=...
keyPassword=...
```

This produces the distributable panel executable:

- `src-tauri/target/release/singboard.exe` — the panel (GUI), with the service host embedded

The service host remains a standalone crate (`src-tauri/service-host/`), but its executable bytes are embedded into `singboard.exe` at build time. An elevated installation places `singboard.service` and approved copies of the core, its adjacent DLLs, and the configuration under the Windows Program Files known folder in `singboard-service`. Only SYSTEM and Administrators can replace these files or their parent directories; configuration contents are readable only by those accounts. Only `singboard.exe` needs to be distributed.

Existing services are migrated through the startup elevation request. The panel continues editing the original configuration and using the selected working directory for core data. Installing, starting, or restarting through the panel approves a new protected core/configuration snapshot; the login task runs the last approved snapshot. Core updates replace the selected source files and the protected runtime together, restoring both on failure. A service-host version change also requests a component refresh. Both development and release panels embed the release helper. The Tauri hooks build the helper before compiling the panel. If building Rust directly, first run `cargo build --release -p singboard-service` from `src-tauri/`. Avoid `--workspace`: it can build the panel before the helper payload exists and can unify dependency features differently.

## Releases

The `Build` workflow runs on pushes to `master`, reads the latest tag as the version, and publishes `singboard.exe`, `singboard-arm64-v8a.apk` and `singboard-armeabi-v7a.apk` to that release. Android APKs are only built when these repository secrets are set; without them the release ships the Windows panel alone:

| Secret | Value |
| --- | --- |
| `ANDROID_KEYSTORE_B64` | Base64 of the release keystore |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `ANDROID_KEY_ALIAS` | Key alias |
| `ANDROID_KEY_PASSWORD` | Key password |

Every Android release must be signed with the same key, otherwise Android refuses to install the update over the existing app.

## Project Structure

```text
.
├─ src/                 # Vue frontend (shared logic, Windows views)
│  └─ android/          # Android shell, pages and components
├─ src-tauri/           # Rust + Tauri commands
│  ├─ src/service/      # Windows service integration
│  ├─ src/root/         # Android root shell
│  ├─ scripts/          # Android service and boot scripts
│  └─ gen/android/      # Android Gradle project
├─ public/
├─ package.json
└─ src-tauri/tauri.conf.json
```

## Configuration and Data

- Default landing route: `/proxies`
- Runtime settings are persisted via in-app Settings (localStorage)
- Typical data directory: `%LOCALAPPDATA%\singboard\EBWebView\Default\`
- Android package name: `io.github.Yuu518.singboard`

## Notes

- This project depends on Clash API; make sure sing-box exposes a reachable API endpoint.
- The Windows service name is `singboard.service`.
- The Android app talks to the Clash API over HTTP, so cleartext traffic is allowed.
- Rule provider content search reads `cache.db`, `.srs` and source `.json` rule sets from the working directory; on Android it needs root mode.

---

> 🤖 **AI-Assisted Development**
>
> This project is an exploration of AI's project-level coding capabilities. Code generated using **Claude Code(Claude Opus 4.6)** and **GPT-5.3-Codex** models via **Visual Studio Code**.
>
