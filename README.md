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
  - Theme switching (`light`/`dark`)
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

The service host remains a standalone crate (`src-tauri/service-host/`), but its executable bytes are embedded into `singboard.exe` at build time. An elevated installation places `singboard.service` and approved copies of the core, its adjacent DLLs, and the configuration under the Windows Program Files known folder in `singboard-service`. Only SYSTEM and Administrators can replace these files or their parent directories; configuration contents are readable only by those accounts. Only `singboard.exe` needs to be distributed.

Existing services are migrated through the startup elevation request. The panel continues editing the original configuration and using the selected working directory for core data. Installing, starting, or restarting through the panel approves a new protected core/configuration snapshot; the login task runs the last approved snapshot. Core updates replace the selected source files and the protected runtime together, restoring both on failure. A service-host version change also requests a component refresh. Both development and release panels embed the release helper. The Tauri hooks build the helper before compiling the panel. If building Rust directly, first run `cargo build --release -p singboard-service` from `src-tauri/`. Avoid `--workspace`: it can build the panel before the helper payload exists and can unify dependency features differently.

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
- The Windows service name is `singboard.service`.

---

> 🤖 **AI-Assisted Development**
>
> This project is an exploration of AI's project-level coding capabilities. Code generated using **Claude Code(Claude Opus 4.6)** and **GPT-5.3-Codex** models via **Visual Studio Code**.
>
