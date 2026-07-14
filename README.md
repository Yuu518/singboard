# Singboard

`Singboard` is a desktop dashboard for sing-box built with `Tauri 2 + Vue 3`, focused on Clash API visualization and Windows service management.

## Features

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
  - Theme switching (`light`/`dark`/`dracula`/`nord`)
- First-run Setup Wizard
  - Guided working directory and Clash API setup
  - Auto-scan for `sing-box` executable and `config.json` under the selected directory

## Tech Stack

- Frontend: `Vue 3`, `TypeScript`, `Vite`, `TailwindCSS`, `DaisyUI`, `ECharts`, `CodeMirror 6`
- Backend: `Rust`, `Tauri 2`
- Platform: currently focused on `Windows` (with Windows Service integration)

## Requirements

- Node.js 18+
- `pnpm`
- Rust stable (recommended via `rustup`)
- Windows C++ Build Tools (MSVC)
- WebView2 Runtime

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

## Build

Build frontend:

```bash
pnpm build
```

Build desktop app:

```bash
pnpm tauri build
```

This produces the distributable panel executable:

- `src-tauri/target/release/singboard.exe` — the panel (GUI), with the service host embedded

The service host remains a standalone crate (`src-tauri/service-host/`), but its executable bytes are embedded into `singboard.exe` at build time. When installing or repairing the service, the panel extracts it to `%APPDATA%\singboard\singboard-service.exe` and registers the Windows service against that copy, so the panel's own file is never locked while the service runs. Only `singboard.exe` needs to be distributed.

The deployed copy is refreshed automatically (with a brief service restart) only when its SHA-256 differs from the embedded payload. The helper is built with `/Brepro` for reproducible bytes, so panel-only releases do not normally touch the running service. Both development and release panels embed the release helper, preventing a development session from replacing an installed service with a debug build. The Tauri hooks build the helper before compiling the panel. If building Rust directly, first run `cargo build --release -p singboard-service` from `src-tauri/`. Avoid `--workspace`: it can build the panel before the helper payload exists and can unify dependency features differently.

## Project Structure

```text
.
├─ src/                 # Vue frontend (views, stores, API layer)
├─ src-tauri/           # Rust + Tauri commands and service integration
├─ public/
├─ package.json
└─ src-tauri/tauri.conf.json
```

## Configuration and Data

- Default landing route: `/proxies`
- Runtime settings are persisted via in-app Settings (localStorage)
- Typical data directory: `%LOCALAPPDATA%\singboard\EBWebView\Default\`

## Notes

- This project depends on Clash API; make sure sing-box exposes a reachable API endpoint.
- The default Windows service name is `sing-box` and can be changed in Settings.

---

> 🤖 **AI-Assisted Development**
>
> This project is an exploration of AI's project-level coding capabilities. Code generated using **Claude Code(Claude Opus 4.6)** and **GPT-5.3-Codex** models via **Visual Studio Code**.
>
