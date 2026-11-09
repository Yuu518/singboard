# Singboard

`Singboard` is a sing-box dashboard built with `Tauri 2 + Vue 3`. One codebase ships two apps:

- **Windows panel**: Clash API visualization, config profile management, Windows service control, and core/panel updates.
- **Android app**: a remote panel for any reachable Clash API, which can also run a local core on rooted devices.

The Proxies, Connections, Logs and Rules pages are shared between both platforms, so improvements reach both. Android keeps its own glass cards, touch sizes and bottom-nav layout.

## Windows features

- **Window**
  - Frameless glass window with a Mica backdrop on Windows 11
  - Remembers window size and position; only one instance can run
  - Optional close to tray. The tray menu can open the panel, start, stop or restart the service, and quit
  - Optional launch at startup; the panel starts hidden in the tray
- **First-run setup wizard**
  - Asks for the working directory and the Clash API URL and secret
  - Finds the `sing-box` core and `config.json` under that directory, and imports the config as the default profile
- **Overview**
  - Status band shows:
    - Core state, uptime and memory
    - Proxy mode
    - The current exit node, resolved through selector chains, with its latency
    - The active config profile
  - Upload speed, download speed and connection count (TCP/UDP split), each with a sparkline and session totals
  - Network info:
    - Direct and proxied exit IPs, which can be hidden
    - Latency to common sites
  - Connection topology:
    - Sankey chart, or a list view
    - Grouped by outbound or by rule
    - Updates can be paused
- **Proxies**
  - Switch nodes in a group, with an 8-second undo
  - Latency tests for a single node or a whole group. Each group can have its own test URL. Optional IPv6 reachability check
  - Sort nodes by latency; search nodes and groups (`Ctrl+F`)
  - Proxy providers:
    - Node counts, subscription expiry and last update time
    - Update one provider or all
    - Health check
- **Connections**
  - Active and closed tabs. The closed tab keeps the last 500 connections
  - Search by host, IP, port, process, rule or node. Seven sort orders; the chosen order is remembered
  - Disconnect one connection (`Delete`), all connections, or only the filtered ones; bulk disconnects ask for confirmation
  - Pause updates
  - Detail dialog:
    - Traffic, IP location and ISP
    - Rule and its payload
    - Outbound chain
    - Process and process path, sniffed host
- **Logs**
  - Realtime virtualized stream of up to 5,000 lines
  - Level filter, with an option to follow the core's log level
  - Keyword search (`Ctrl+F`) and auto-follow
  - Pause, with a count of lines that arrived while paused
  - Clear, with undo
- **Rules**
  - The rule list reads top to bottom as a pipeline:
    - Readable conditions
    - Resolved outbound chains
    - The final fallback
  - Search rules or outbounds. Typing a domain or IP also lists the rule providers that match it
  - Rule providers:
    - Update one or all
    - A detail dialog searches the provider's content and shows which rules use it
- **Config**
  - Profiles can be:
    - Local files
    - Remote subscriptions, downloaded as sing-box JSON with optional auto-update every N hours
  - Every remote update and every switch of the active profile is validated with `sing-box check` first. A failed update keeps the existing config
  - Every write keeps a `.bak`, so the previous version is one click away
  - Editor:
    - CodeMirror, editing the whole file or one top-level module at a time
    - Keeps JSONC comments, with batch comment and uncomment
    - Undo and redo, format, validate, save (`Ctrl+S`)
    - Warns about unsaved changes
  - Applying a profile writes the running config and offers to restart the core
- **Settings**
  - Core and service:
    - Start, restart and stop the core, with a reachability check after start
    - Proxy mode, from the modes the core reports
    - Install or uninstall the Windows service
    - Core path, working directory, and the startup delay after login
  - Clash API: multiple endpoints (add, edit, switch, remove) with live reachability
  - Network test: default and per-group latency test URLs, IPv6 test toggle
  - DNS query tool:
    - A, AAAA and CNAME records
    - Resolvers: system DNS, presets, or a custom server
    - Optional IP location lookup
  - Appearance:
    - Theme: follow system, light or dark
    - Glass tint slider, from clear to tinted
    - Custom wallpaper with an image editor (crop, rotate, mirror)
  - Core updates:
    - Sources: official SagerNet, reF1nd, or any `owner/repo`
    - Stable or testing channel, optional download mirror
    - SHA-256 verified, rolled back if the new core fails to start
  - Panel updates: checked on startup, SHA-256 verified; the panel replaces itself without interrupting the proxy service
    - Downloads from the release link first and falls back to the GitHub API asset endpoint
    - With an optional GitHub token, checks and downloads go through the GitHub API first, avoiding the anonymous rate limit

## Android features

- **Navigation**
  - Floating glass bottom bar: Home, Proxies, Connections, Logs, Rules, Settings
  - Back returns to Home
  - Swiping between pages is an opt-in setting (off by default)
- **Setup wizard**
  - Clash API URL and secret
  - Optionally, a sing-box working directory for root mode
  - Can be skipped and finished later in Settings
- **Home** (opens first)
  - Root mode only:
    - Core state, uptime and version
    - Power and restart buttons
    - Run mode, control endpoint and working directory
  - Realtime upload and download speed with sparklines and totals
  - Memory usage; in root mode, also the core process's CPU usage and resident memory
  - Network info: exit IPs and connectivity checks
- **Proxies, Connections, Logs, Rules**: the shared pages described above. Each page shows a clear state when the core is stopped, the Clash API is unreachable, or the secret is wrong
- **Settings**
  - Core and service (root mode):
    - Start, restart and stop
    - Working directory detection
    - Start on boot
  - Clash API endpoints, network test URLs, DNS query tool
  - Theme, glass tint slider, custom wallpaper, page swiping
  - Core updates (root mode) and app updates
- **Remote mode** (no root required)
  - Connects to sing-box running on a router, a PC or another app
  - Connection state and core version come from the Clash API
- **Root mode** (Magisk / KernelSU / APatch)
  - Point the app at a sing-box working directory. It searches the directory and its subdirectories (4 levels) for:
    - The core: ELF executables in a `bin` folder first, otherwise files named `sing-box*` / `singbox*`
    - The configuration: JSON files containing `"outbounds"`, preferring `config.json`
    - Rule sets: a `rules` / `ruleset` / `rule-set` style folder, otherwise the folder holding the most `.srs` files
  - Start, stop and restart the core; it keeps running after the panel is closed
  - Stopping sends SIGTERM and waits for the core to exit on its own (up to 60 s), so TUN `auto_redirect` rules are cleaned up. The core is never force-killed
  - Core logs follow `log.output` in the configuration. Singboard keeps its own log in `/data/adb/singboard/logs`
  - Start on boot through `/data/adb/service.d/singboard.sh`; Magisk, KernelSU and APatch run it after boot completes
  - Core updates:
    - Downloaded from GitHub releases (`android-*.tar.gz`)
    - Verified by SHA-256, and rolled back if the new core fails to start
    - Without an existing core, it installs to `bin/sing-box` (or `sing-box` in the working directory)
  - Every root operation goes through one long-lived `su` shell, so the root manager prompts once
- **In-app updates**
  - Checks `Yuu518/singboard` releases on startup (can be disabled)
  - Downloads the APK for the device ABI, verifies its SHA-256 digest, then opens the system installer
  - Same download order as Windows: release link first, GitHub API as fallback, or the API first when a GitHub token is set

### Root layout

The working directory stays yours. Singboard keeps its service files in a fixed place so the boot hook can find them:

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

Each time the app starts the core, it rewrites the service script with the detected paths. You can also run the script from a root shell, for example `sh /data/adb/singboard/singboard.sh restart`.

## Tech Stack

- Frontend: `Vue 3`, `TypeScript`, `Vite`, `TailwindCSS`, `DaisyUI`, `ECharts`, `CodeMirror 6`
- Backend: `Rust`, `Tauri 2`
- Platforms: `Windows` (with Windows Service integration) and `Android` (remote panel, plus root mode)

## Requirements

- Node.js 18+ (CI uses Node.js 24)
- `pnpm`
- Rust stable (recommended via `rustup`)
- Windows C++ Build Tools (MSVC)
- WebView2 Runtime
- For Android:
  - JDK 17+
  - Android SDK with platform 36 and build-tools 36
  - NDK r30
  - `ANDROID_HOME` and `NDK_HOME` set
  - Rust targets `aarch64-linux-android` and `armv7-linux-androideabi`

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

The platform is fixed at build time. The Tauri CLI sets `TAURI_ENV_PLATFORM`, and the frontend only bundles the code for that platform.

- Shared logic (API, stores, bridges, utilities) and the shared pages live in `src/`.
- The Android shell, Home and Settings pages, and Android-only components live in `src/android/`.

To preview the Android interface in a desktop browser, run `TAURI_ENV_PLATFORM=android pnpm dev`.

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

The Windows build produces one distributable file, `src-tauri/target/release/singboard.exe`. It is the panel (GUI) with the service host embedded.

- **Service host**
  - It is a standalone crate (`src-tauri/service-host/`), but its executable bytes are embedded into `singboard.exe` at build time.
  - An elevated install creates `%ProgramFiles%\singboard-service\` and places there:
    - `singboard.service`
    - Approved copies of the core, its adjacent DLLs and the configuration
  - Only SYSTEM and Administrators can replace these files or their parent directories. Only those accounts can read the configuration's contents.
- **Approved snapshots**
  - The panel keeps editing the original configuration and uses the selected working directory for core data.
  - Installing, starting or restarting through the panel approves a new protected snapshot of the core and configuration. The login task runs the last approved snapshot.
  - Core updates replace the selected source files and the protected runtime together, and restore both on failure.
- **Migration**
  - Existing services are migrated through the startup elevation request.
  - A change in the service-host version also requests a component refresh.
- **Building the helper**
  - Both development and release panels embed the release helper. The Tauri hooks build the helper before compiling the panel.
  - If you build Rust directly, first run `cargo build --release -p singboard-service` from `src-tauri/`.
  - Avoid `--workspace`: it can build the panel before the helper payload exists, and it can unify dependency features differently.

## Releases

The `Build` workflow runs on pushes to `master`; pushes that only touch Markdown files or `LICENSE` are skipped. It can also be started manually.

- It reads the latest tag as the version.
- It publishes `singboard.exe`, `singboard-arm64-v8a.apk` and `singboard-armeabi-v7a.apk` to that tag's release.

Repository secrets:

| Secret | Value |
| --- | --- |
| `WINDOWS_CERT_PFX_B64` | Optional. Base64 of a code-signing certificate (PFX) used to sign the Windows binaries |
| `WINDOWS_CERT_PASSWORD` | Optional. Password of that certificate |
| `ANDROID_KEYSTORE_B64` | Base64 of the release keystore |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `ANDROID_KEY_ALIAS` | Key alias |
| `ANDROID_KEY_PASSWORD` | Key password |

Android APKs are built only when all four `ANDROID_*` secrets are set. Without them, the release ships only the Windows panel. Every Android release must be signed with the same key, otherwise Android refuses to install the update over the existing app.

## Project Structure

```text
.
├─ src/                 # Vue frontend: shared logic and pages, Windows shell
│  └─ android/          # Android shell, Home and Settings pages, components
├─ src-tauri/           # Rust + Tauri commands
│  ├─ src/service/      # Windows service integration
│  ├─ src/root/         # Android root shell
│  ├─ service-host/     # Windows service host, embedded into singboard.exe
│  ├─ scripts/          # Android service and boot scripts
│  └─ gen/android/      # Android Gradle project
├─ public/
├─ package.json
└─ src-tauri/tauri.conf.json
```

## Configuration and Data

- **Default landing page**: `/proxies` on Windows, `/home` on Android
- **App settings**: saved in localStorage. Custom wallpapers are saved in IndexedDB
- **Windows WebView data**: `%LOCALAPPDATA%\singboard\EBWebView\`
- **Windows config files**: `%APPDATA%\singboard\` holds:
  - `running-config.json`, the configuration the core actually reads
  - `Remote Config\`, the downloaded remote profiles
- **Windows protected service runtime**: `%ProgramFiles%\singboard-service\`
- **Android package name**: `io.github.Yuu518.singboard` (Android 8.0+)

## Notes

- This project depends on Clash API; make sure sing-box exposes a reachable API endpoint.
- The Windows service name is `singboard.service`. Start at login is handled by a scheduled task.
- The Android app talks to the Clash API over HTTP, so cleartext traffic is allowed.
- Rule provider content search reads `cache.db`, `.srs` and source `.json` rule sets from the working directory; on Android it needs root mode.
- Remote config profiles must be sing-box JSON. Clash YAML and base64 subscriptions are not converted.

---

> 🤖 **AI-Assisted Development**
>
> This project is an exploration of AI's project-level coding capabilities. Code generated using **Claude Code(Claude Opus 4.6)** and **GPT-5.3-Codex** models via **Visual Studio Code**.
>
