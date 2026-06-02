# MIT(MI-Terminal)

<p align="center">
  <strong>A lightweight Xiaomi Home CLI &amp; full-screen TUI for device control</strong>
  <br/><br/>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"/></a>
  <a href="https://github.com/ttys026/mit/releases/latest"><img src="https://img.shields.io/github/v/release/ttys026/mit" alt="Latest Release"/></a>
  <a href="https://github.com/ttys026/mit/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/ttys026/mit/release.yml?label=build" alt="Build"/></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey" alt="Platforms"/>
  <br/><br/>
  <a href="README.zh-CN.md">中文文档</a>
</p>

---

`mit` is a fast, zero-dependency Xiaomi Home CLI written in Rust. It lets you log in to your Xiaomi account, browse and control smart home devices from the terminal, and send push notifications — all without opening the app.

> **Disclaimer:** This project is a community-maintained tool and is **not** affiliated with, endorsed by, or part of Xiaomi.

## Features

- **Full-screen TUI** — navigate accounts, rooms, and devices; read/write MIoT properties; invoke actions; all keyboard-driven
- **Local-first LAN control** — automatically discovers local IPs and tokens, probes reachability, and falls back to cloud seamlessly
- **CLI props access** — read/write device properties, trigger actions, and subscribe to live property changes without opening the TUI
- **Push notifications** — send messages to any logged-in Xiaomi account
- **Multi-account** — log in with multiple accounts across different regions
- **JSON output** — commands have a `--json` mode for scripting and automation

---

## Demo

| Sending a push notification                           | Reading & writing device properties                              | TUI(mobile view)                            |
| ----------------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------- |
| ![Sending a push notification](docs/assets/push.webp) | ![Reading and writing device properties](docs/assets/props.webp) | ![TUI mobile view](docs/assets/mobile.webp) |

---

## Install

**One-liner (macOS / Linux):**

```bash
curl -sSfL https://raw.githubusercontent.com/ttys026/mit/main/install.sh | sh
```

The script detects your platform, downloads the latest pre-built binary, verifies the SHA-256 checksum, and installs to `/usr/local/bin` (or `~/.local/bin` if that's not writable).

**Build from source (requires Rust ≥ 1.75):**

```bash
cargo install --git https://github.com/ttys026/mit.git --bin mit
```

**Custom install directory:**

```bash
INSTALL_DIR=~/.local/bin curl -sSfL https://raw.githubusercontent.com/ttys026/mit/main/install.sh | sh
```

### Supported platforms

| Platform      | x86_64 | aarch64 |
| ------------- | ------ | ------- |
| Linux (musl)  | ✅     | ✅      |
| Linux (glibc) | ✅     | ✅      |
| macOS         | ✅     | ✅      |
| Windows       | ✅     | ✅      |

---

## Quick Start

```bash
# 1. Log in to your Xiaomi account
mit auth login

# 2. List your devices
mit devices list

# 3. Launch the full-screen TUI
mit tui
```

---

## Commands

```bash
# Root help
mit
mit --help

# ── Auth ───────────────────────────────────────────────────
mit auth login                                   # Log in (opens browser)
mit auth login --region cn                       # Specify region
mit auth list                                    # List saved accounts

# ── TUI ────────────────────────────────────────────────────
mit tui                                          # Launch full-screen dashboard
mit tui --uid 1234567                            # Start with a specific account

# ── Devices ────────────────────────────────────────────────
mit devices list                                 # List devices for all accounts

# ── Push notifications ─────────────────────────────────────
mit push "Hello World"                           # Push to all accounts
mit push --uid 1001 "Hello World"                # Push to a specific account

# ── MIoT properties & actions (no TUI required) ────────────
mit props get did-1 2 1                          # Read property (siid=2, piid=1)
mit props set did-1 2 1 true                     # Write property
mit props act did-1 5 1 1 2                      # Invoke action with parameters
mit props sub                                    # Subscribe to all device property changes
mit props sub did-1                              # Subscribe to all changes for one device
mit props sub did-1 2 1                          # Subscribe to one property
```

### Notes

- After browser login completes, `mit auth login` finalises auth automatically and shows a success page in the browser.
- `mit props get/set/act/sub` work directly without opening the TUI.
- `mit props sub` streams subscription logs and property updates to stdout until interrupted.
- `mit tui` syncs devices and caches MIoT specs under `~/.mit/cache/specs/`.
- bare `auth` keeps compatibility with `mit auth --help`.
- bare `devices` keeps compatibility with `mit devices --help`.

### TUI tabs

1. `1:Account` — account list, login/logout, push message
2. `2:Devices` — device list and property/action dialog
3. `3:Logs` — runtime logs and status information
4. `4:Settings` — cache cleanup (keep auth) and full reset (delete `~/.mit`, double-confirmed)

---

## Local-First LAN Control

`mit` prefers talking to your devices over LAN (UDP/MIIO protocol) instead of going through Xiaomi's cloud:

1. On first sync, `mit` fetches each device's `localip` and `token` from Xiaomi cloud.
2. Credentials are persisted per account to `~/.mit/accounts/{account_id}/local_credentials.json`.
3. `mit` probes reachability for each device in the background.

---

## JSON Output

Commands support machine-readable JSON output via the global `--json` flag:

```bash
mit --json                  # Help as JSON
mit --json auth list        # Account list as JSON
mit --json devices list     # Device list as JSON
mit --json push "hello"     # Push result as JSON
```

- `--json` applies only to successful stdout. Parse errors and runtime errors are always printed as human-readable text on stderr.
- Streaming commands such as `mit props sub` print live text lines to stdout and do not support `--json`.
- This makes `mit` easy to use in scripts, CI pipelines, or any automation that needs structured output.

---

## Configuration

| Environment variable                          | Description                                                               |
| --------------------------------------------- | ------------------------------------------------------------------------- |
| `MIT_PROFILE_DIR` / `MIT_HOME` / `XMCLI_HOME` | Override the directory where `~/.mit` data is stored                      |
| `MIT_MIOT_SPEC_URL_BASE`                      | Override the MIoT spec API base URL (useful for testing)                  |
| `MIT_MICO_BASE_URL`                           | Override the Mico API base URL (debug builds only)                        |
| `MIT_LOG_DEVICE_LIST_PAGE_RAW`                | Set to any non-empty value to log raw device list API responses to stderr |

Profile data is stored under `~/.mit/`:

```
~/.mit/
├── auth.json                          # Saved account tokens
├── accounts/{uid}/
│   ├── devices.json                   # Cached device list for this account
│   └── local_credentials.json         # Per-account LAN credentials
└── cache/
    └── specs/
        ├── index.json                 # Model → URN mapping
        ├── models/                    # Per-model MIoT spec JSON
        └── sources/                   # Cached MIoT API index files
```

---

## License

[MIT](LICENSE)
