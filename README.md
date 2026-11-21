# Singboard

`Singboard` is a native sing-box dashboard with a shared Rust Clash API core (`singboard-stream`). One repository ships two native apps:

- **Windows panel** (`GPUI-Fast` + `Rust`): Clash API visualization, config profile management, Windows service control, and core/panel updates.
- **Android app** (`Flutter` + `Rust` FFI): a remote panel for any reachable Clash API, which can also run a local core on rooted devices.

## Windows features

- **Window**
  - Frameless translucent glass window with an aurora backdrop (or custom wallpaper)
  - Remembers window size, position and maximized state; only one instance can run
  - Optional close to tray. The tray menu can open the panel, start, stop or restart the service, and quit
  - Optional launch at startup; the panel starts hidden in the tray
- **First-run setup wizard**
  - Asks for the working directory and the Clash API URL and secret
  - Finds the `sing-box` core and `config.json` under that directory, and imports the config as the default profile
- **Overview**
  - One main card shows:
    - Core state, uptime and proxy mode
    - The current exit node, resolved through selector chains, with its latency
    - The active config profile
    - Download and upload speed with totals, over a mirrored traffic wave for the last minute
  - Connection count (TCP/UDP split) and memory, each with a small trend line
  - When sing-box is stopped or unreachable, the main card says why and offers the next step
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
    - Tree-sitter JSON/JSONC editor, editing the whole file or one top-level module at a time
    - Keeps JSONC comments, with batch comment and uncomment
    - Undo and redo, format, validate, save (`Ctrl+S`)
    - Warns about unsaved changes
  - Applying a profile validates it, switches the active config path, and offers to restart the core to activate a new protected snapshot
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
    - The release check sends the GitHub token when one is set, avoiding the anonymous rate limit
  - Panel updates: checked on startup, SHA-256 verified (including same-version digest mismatch detection); the panel replaces itself without interrupting the proxy service
    - Tries the configured download mirror first, then the GitHub release link and GitHub API asset endpoint
    - With an optional GitHub token, checks and downloads prefer the GitHub API to avoid anonymous rate limits

## Android features

- **Navigation**
  - Floating glass bottom bar: Overview, Proxies, Connections, Logs, Rules, Settings
  - Back returns to Overview
  - Swiping between pages is an opt-in setting (off by default)
- **Setup wizard**
  - Clash API URL and secret
  - Optionally, a sing-box working directory for root mode
  - Can be skipped and finished later in Settings
- **Overview** (opens first): traffic wave, connection/memory trends, exit IPs, site latencies, and connection topology
  - Root mode adds the core version, restart and power buttons, the core process's CPU usage and resident memory, and asks for a working directory when none is set
  - Remote mode shows the endpoint name, and points to the Clash API settings when the core cannot be reached
- **Proxies, Connections, Logs, Rules**: full native Flutter pages powered by `singboard-stream`. Each page shows a clear state when the core is stopped, the Clash API is unreachable, or the secret is wrong
- **Config** (root mode, from Settings):
  - Manage remote sing-box JSON subscriptions (with optional auto-update) and edit local or root config files
  - Whole-file or top-level module JSON/JSONC editing, formatting, `sing-box check` validation, and `.bak` restore
- **Settings**
  - Core and service (root mode):
    - Start, restart and stop
    - Working directory detection
    - Start on boot
  - Clash API endpoints, network test URLs, DNS query tool
  - Theme, glass tint slider, custom wallpaper with image editor (crop, rotate, mirror), page swiping
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
  - Checks `Yuu518/singboard` releases on startup (can be disabled), including same-version SHA-256 mismatch detection
  - Downloads the APK for the device ABI, verifies its SHA-256 digest and package signature, then opens the system installer
  - Same download order as Windows: optional mirror first, then release link and GitHub API (or API first when a GitHub token is set)

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

- Shared core: `Rust` (`singboard-stream`)
- Windows UI: `Rust`, `GPUI-Fast`, `gpui-component` (with Windows Service integration)
- Android UI: `Flutter` (`Dart` + Rust FFI), remote panel plus root mode

## Requirements

- Rust stable (recommended via `rustup`)
- Windows C++ Build Tools (MSVC)
- For Android:
  - Flutter SDK 3.47+ (Dart 3.13+)
  - JDK 21
  - Android SDK with `platforms;android-36` and `build-tools;36.0.0`
  - Android NDK r28+ (default path `$ANDROID_HOME/ndk/28.2.13676358`, or set `ANDROID_NDK_HOME`)
  - `ANDROID_HOME` set
  - Rust targets `aarch64-linux-android` and `armv7-linux-androideabi`

## Build

Build the Windows app:

```powershell
pwsh clients/native/build.ps1 -Platform Windows
```

Build the Android APKs:

```powershell
pwsh clients/native/build.ps1 -Platform Android
```

Outputs are written to `dist/native/`:

- Windows writes `singboard.exe` by default, or `singboard-windows-x64.exe` when `-ReleaseAssets` is passed.
- Android writes `singboard-arm64-v8a.apk` and `singboard-armeabi-v7a.apk`. Release signing uses `SINGBOARD_ANDROID_KEYSTORE`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD` (required with `-ReleaseAssets`).
- All build caches (Cargo, Pub, Gradle and Android staging) are kept under `.build-deps/`.

The Windows build produces one self-contained executable with the service host embedded.

- **Service host**
  - It is a standalone crate (`clients/native/service-host/`), embedded into `singboard.exe` at build time.
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
  - `clients/native/build.ps1 -Platform Windows` builds `singboard-service` first and passes `SINGBOARD_SERVICE_HOST` when compiling `singboard-gpui`.
  - If you build Rust directly, first build `singboard-service` in release mode from `clients/native/` and set `SINGBOARD_SERVICE_HOST` to the resulting executable.

## Releases

The `Build` workflow runs on pushes to `master`; pushes that only touch Markdown files or `LICENSE` are skipped. It can also be started manually.

- It reads the latest tag as the version.
- It publishes `singboard-windows-x64.exe`, `singboard-arm64-v8a.apk` and `singboard-armeabi-v7a.apk` to that tag's release.

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
├─ .github/workflows/
│  └─ build.yml           # Windows and Android CI build & release workflow
└─ clients/native/
   ├─ Cargo.toml          # Rust workspace (stream, windows, service-host)
   ├─ build.ps1           # Local Windows and Android build script
   ├─ stream/             # Shared Rust Clash API, commands, root shell, Android scripts
   ├─ windows/            # Windows GPUI-Fast client, icons, manifest and service integration
   ├─ service-host/       # Windows service host, embedded into singboard.exe
   ├─ android/            # Android Flutter app, shaders, icons and Kotlin host
   └─ compat/gpui-pre/    # GPUI compatibility patch crate
```

## Configuration and Data

- **Windows data directory**: `%APPDATA%\singboard\` holds:
  - `native-settings.json`, app preferences, Clash API endpoints and profile metadata
  - `window.json`, saved window size, position and maximized state
  - `wallpaper.png`, custom wallpaper
  - `remote-configs\`, downloaded remote config profiles
  - `updates\`, staged panel updates (cleaned up after apply)
- **Windows protected service runtime**: `%ProgramFiles%\singboard-service\`
- **Android package name**: `io.github.Yuu518.singboard` (Android 8.0+ / API 26+)
  - App-private storage (`filesDir`) stores `native-settings.json`, `wallpaper.png`, remote subscriptions, and staged `updates/` APKs

## Notes

- This project depends on Clash API; make sure sing-box exposes a reachable API endpoint.
- The Windows service name is `singboard.service`. Start at login is handled by a scheduled task.
- The Android app talks to the Clash API over HTTP, so cleartext traffic is allowed.
- Rule provider content search reads `cache.db`, `.srs` and source `.json` rule sets from the working directory; on Android it needs root mode.
- Remote config profiles must be sing-box JSON. Clash YAML and base64 subscriptions are not converted.
