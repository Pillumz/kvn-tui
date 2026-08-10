# kvn-tui

[![CI](https://github.com/yarikov/kvn-tui/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/yarikov/kvn-tui/actions/workflows/ci.yml)
[![AUR version](https://img.shields.io/aur/version/kvn-tui-bin?logo=arch-linux&label=AUR)](https://aur.archlinux.org/packages/kvn-tui-bin)
[![GitHub Release](https://img.shields.io/github/v/release/yarikov/kvn-tui?logo=github&label=release)](https://github.com/yarikov/kvn-tui/releases/latest)
[![Rust Version](https://img.shields.io/badge/rust-1.87%2B-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/github/license/yarikov/kvn-tui)](LICENSE)

> Terminal VPN client for Arch Linux with vim navigation.

`kvn-tui` is a keyboard-driven TUI application for managing VPN connections. It provides a fast, minimal interface for configuring profiles, connecting via the [sing-box](https://sing-box.sagernet.org/) backend, and routing traffic — all without leaving the terminal.

![kvn-tui screenshot](assets/screenshot.png)

---

## Contents

- [Features](#features)
- [Supported Protocols](#supported-protocols)
- [Technology Stack](#technology-stack)
- [Installation (Arch Linux)](#installation-arch-linux)
- [Quick Start](#quick-start)
- [Configuration](#configuration)

---

## Features

- **Vim-style navigation** — `j`/`k` to move, `g`/`G` to jump, `?` for help
- **Profile management** — edit via `$EDITOR`, delete, and organize server profiles
- **One-click paste** — import share links for any supported protocol or subscription URLs directly from the system clipboard
- **Yank to clipboard** — export the selected profile back to a share link with `y`, ready to paste into another client (or copy a subscription's source URL)
- **Subscription support** — subscribe to remote profile feeds (HTTP/HTTPS, Base64 or plain-text, mixed-protocol); configurable auto-update interval (off / 1h / 12h / 1d / 7d)
- **Geo region & routing** — pick Russia, China, Iran, or Global on first launch; only relevant routing modes (Global / Bypass / Only) and geoip/geosite rule-sets are shown and downloaded; refresh rule-sets with `u` or cycle background auto-update (`off` / `12h` / `1d` / `3d` / `7d`) with `Shift+I`
- **Kill switch** — block all outbound traffic if the VPN connection drops; toggled with `K`; powered by nftables + a systemd unit
- **DNS configuration** — built-in presets (Cloudflare DoH, Google DoT, Quad9 DoH, system resolver), strategy cycle, fake-IP toggle (sing-box 1.12 API), plus custom servers and per-domain routing rules via `profiles.json`; opened with `D`
- **Auto-connect** — automatically reconnect to the last used profile on startup
- **Suspend/resume awareness** — automatically detects system resume via D-Bus and reconnects
- **Live logs** — split-pane view interleaves sing-box output with app events; both streams are also persisted to `sing-box.log` and `app.log` on disk
- **Live traffic statistics** — full-width header above the main panes shows instantaneous ↑/↓ rate, cumulative totals, and active connection count while connected; data is scraped from sing-box's Clash API once per second
- **Themable** — 19 bundled [Omarchy](https://omarchy.org/) palettes (gruvbox, tokyo-night, catppuccin, nord, kanagawa, rose-pine, …) compiled into the binary; pick interactively via `C` and persist the choice in `profiles.json`. On Omarchy systems the `omarchy` slug auto-follows `~/.config/omarchy/current/theme.name` and updates live on `omarchy theme set …`

---

## Supported Protocols

All sing-box 1.12+ outbound protocols are supported. Share links can be pasted directly from the clipboard or fetched via subscription URLs.

| Protocol | Share-link scheme(s) | Notes |
|----------|----------------------|-------|
| **VLESS** | `vless://` | REALITY, XTLS Vision, TLS; gRPC / WebSocket / HTTPUpgrade transport |
| **VMess** | `vmess://` | TLS; gRPC / WebSocket / HTTPUpgrade transport; base64-JSON and URI forms |
| **Trojan** | `trojan://` | TLS; gRPC / WebSocket / HTTPUpgrade transport |
| **Shadowsocks** | `ss://` | AEAD-2022 + AEAD ciphers; SIP002 and legacy base64 share-link forms |
| **Hysteria 2** | `hysteria2://`, `hy2://` | QUIC; Salamander obfuscation |
| **TUIC** | `tuic://` | QUIC; BBR / Cubic / NewReno congestion control |
| **ShadowTLS** | `shadowtls://` | v1 / v2 / v3; wraps Shadowsocks as the inner transport |
| **AnyTLS** | `anytls://` | TLS-based multiplexing |
| **SOCKS** | `socks://`, `socks5://` | v5 (default) / v4a; optional username/password |
| **HTTP proxy** | `http://`, `https://` | Plain or TLS CONNECT proxy |
| **SSH** | `ssh://` | Password or private-key auth |

---

## Technology Stack

Under the hood, `kvn-tui` is built entirely in **Rust** and leverages the following ecosystem:

| Component | Library / Tool | Purpose |
|-----------|--------------|---------|
| TUI framework | [ratatui](https://ratatui.rs/) + [crossterm](https://github.com/crossterm-rs/crossterm) | Terminal UI rendering and input handling |
| VPN backend | [sing-box](https://sing-box.sagernet.org/) (external binary) | Actual VPN engine (TUN, routing, protocols) |
| Serialization | [serde](https://serde.rs/) + `serde_json` | Configuration and profile storage |
| HTTP client | [ureq](https://github.com/algesten/ureq) | Geo database downloads |
| D-Bus integration | [zbus](https://docs.rs/zbus/latest/zbus/) | Suspend/resume detection via `systemd-logind` |
| Logging | [tracing](https://github.com/tokio-rs/tracing) | Structured application logs |
| Error handling | [anyhow](https://github.com/dtolnay/anyhow) | Ergonomic error propagation |
| Utilities | `uuid`, `chrono`, `url`, `urlencoding`, `dirs` | IDs, timestamps, URI parsing, XDG directories |

### Architecture Highlights

- **Daemon + TUI client** — headless daemon owns sing-box and state, TUI is just a thin renderer; they talk NDJSON over a Unix socket, so re-running `kvn-tui` re-attaches without restarting sing-box.
- **TEA (The Elm Architecture)** — pure `Model` / `update` / `Effect` layers; `update.rs` is fully synchronous and side-effect-free, making business logic trivial to unit-test.
- **sing-box runner** — generates valid sing-box 1.12+ JSON from profile data, validates with `sing-box check`, and spawns the process with crash detection.
- **Atomic config writes** — `profiles.json` is written to a temp file and renamed to prevent corruption.
- **State I/O** — connection status and active profile are persisted to `state.json` for waybar integration and crash recovery.

---

## Installation (Arch Linux)

### Install from AUR (recommended)

`sing-box` installs automatically as a dependency.

```bash
yay -S kvn-tui-bin
```

### Omarchy Setup

If you use [Omarchy](https://omarchy.org/), run this after installation to enable Waybar integration:

```bash
kvn-tui --install-omarchy
```

This automatically:

- Installs the `omarchy-launch-kvn-tui` launcher script to `~/.local/bin/`
- Adds a `custom/kvn-tui` module to Waybar (shows connected/disconnected status, clicks open the TUI)
- Optionally adds the kvn-tui **daemon** to Hyprland autostart (`~/.config/hypr/autostart.conf`) — runs headlessly on login
- Optionally adds a Hyprland keybinding to open the TUI (default: `Super + Ctrl + K`)
- Configures the TUI window to open centered and floating
- Backs up your Waybar and Hyprland configs before modifying them
- Restarts Waybar to apply changes

> The installer is idempotent — running it again will skip already-applied changes.

After installation, the daemon starts automatically on login. Open the TUI on demand via the Waybar module, the keybinding, or by running `omarchy-launch-kvn-tui`.

### Polkit Setup (recommended)

If your system uses **systemd-resolved** or **NetworkManager**, sing-box may trigger authentication dialogs when it changes DNS settings or routes on connect. To avoid these prompts, install the bundled polkit rule:

```bash
sudo kvn-tui --install-polkit
```

This command will:

1. Add your user to the `network` group (if not already a member).
2. Create `/etc/polkit-1/rules.d/49-kvn-tui.rules`, allowing the `network` group to manage DNS and network settings without a password.
3. Restart the polkit service.

> If you were just added to the `network` group, run `newgrp network` in your current shell, or log out and back in before testing.

If you prefer not to use polkit, you can simply authenticate when the dialog appears — the application works either way.

### Kill Switch Setup (optional)

The kill switch blocks all outbound traffic when the VPN connection is not active. To use it, install the bundled helper:

```bash
sudo kvn-tui --install-killswitch
```

This command will:

1. Add your user to the `network` group (if not already a member).
2. Install `/etc/kvn-tui/killswitch.nft` — the nftables ruleset.
3. Install `/usr/lib/kvn-tui/killswitch-helper.sh` — a root-owned helper script.
4. Install `/etc/systemd/system/kvn-tui-killswitch.service` — loads the ruleset at boot.
5. Create `/etc/sudoers.d/kvn-tui-killswitch` — allows the `network` group to run the helper without a password prompt.

> **Requires** the `nftables` package. Install it with `sudo pacman -S nftables` if not already present.

> If you were just added to the `network` group, run `newgrp network` in your current shell, or log out and back in before toggling the kill switch.

Once installed, press `K` in the TUI to enable or disable the kill switch. The status bar shows `[KS]` when it is active.

> The kill switch is independent from the polkit rule above, but both use the `network` group. Running `--install-polkit` first means `--install-killswitch` will skip the group-add step.

### Build & Install from Source

#### Prerequisites

- **Rust** >= 1.87
- **sing-box** >= 1.12 (external VPN backend, must be available on `$PATH`)
- `base-devel`, `dbus`

Install the dependencies:

```bash
yay -S base-devel rust dbus sing-box
```

> `makepkg -si` will pull these automatically from the PKGBUILD `depends`/`makedepends`, but installing them beforehand avoids surprises.

#### Steps

1. Clone the repository:

```bash
git clone https://github.com/yarikov/kvn-tui.git
cd kvn-tui
```

2. Build and install using the local PKGBUILD:

```bash
cd pkg/arch
makepkg -si
```

This compiles the release binary with `--release --locked` and installs it to `/usr/bin/kvn-tui`.

3. Verify that sing-box is reachable:

```bash
sing-box version
```

### Manual Build (without package manager)

```bash
cargo build --release
sudo install -Dm755 target/release/kvn-tui /usr/local/bin/kvn-tui
```

---

## Quick Start

Launch the application:

```bash
kvn-tui
```

> No root required. TUN mode works via Linux capabilities set on the `sing-box`
> binary (`cap_net_admin`, `cap_net_raw`). The AUR package configures this
> automatically. For a manual install, run once:
>
> ```bash
> sudo setcap cap_net_admin,cap_net_raw+ep /usr/bin/sing-box
> ```

On first launch a modal overlay prompts you to pick a geo region (Russia, China, Iran, or Global). After that, the main UI opens with an empty profile list.

Typical first connection:

1. Copy a share link (`vless://`, `ss://`, `hysteria2://`, …) or a subscription URL to the clipboard.
2. Press `p` to import it. Subscriptions appear as a header with profiles nested underneath; single share links become standalone profiles.
3. Select a profile with `j` / `k` and press `Enter` to connect. The status bar shows `[CONNECTED]` and the traffic header starts ticking.

Press `?` at any time to see the full key map.

### Default Key Bindings

**Navigation**

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` | Go to first profile |
| `G` | Go to last profile |

**Profiles & subscriptions**

| Key | Action |
|-----|--------|
| `Enter` | Connect to selected profile |
| `p` | Paste share link or subscription URL from clipboard |
| `y` | Yank selected profile as a share link (or subscription source URL) to clipboard |
| `d` | Delete selected profile |
| `e` | Open `profiles.json` in `$EDITOR` |
| `u` | Update selected subscription or geoip/geosite databases |
| `i` | Cycle subscription auto-update interval |

**Connection & routing**

| Key | Action |
|-----|--------|
| `r` | Reconnect |
| `s` | Stop / disconnect |
| `m` | Change routing mode |
| `o` | Select geo region |
| `K` | Toggle kill switch |
| `D` | DNS settings (presets, strategy, fake-IP) |
| `C` | Theme picker (live preview, Enter to persist) |
| `a` | Toggle auto-connect |
| `t` | Test latency of selected profile |
| `T` | Test latency of all profiles (up to 4 in parallel) |

**Application**

| Key | Action |
|-----|--------|
| `?` | Show help |
| `q` / `Esc` | Detach TUI — daemon and sing-box keep running; closes the active overlay first if one is open |
| `Ctrl+C` | Quit — stop daemon and sing-box, then exit |

---

## Configuration

Configuration is stored in:

```
~/.config/kvn-tui/profiles.json
```

The file contains your profile list and application settings (default profile, TUN interface name, DNS configuration, routing mode, auto-connect, geo region). You can edit it manually with the `e` keybinding or any text editor.

`settings.dns` maps directly onto the sing-box 1.12 DNS schema (`servers`, `rules`, `final_server`, `strategy`, `fakeip_enabled`). The `D` overlay covers the common cases — built-in presets (Cloudflare DoH, Google DoT, Quad9 DoH, system resolver), strategy cycle (`h` / `l` to preview, `Enter` to apply), and the fake-IP toggle. For custom upstreams or per-domain routing rules, edit `profiles.json` via `e`.

When `auto_connect` is enabled, the application stores `last_connected_profile` and automatically connects to that profile on the next startup.

`settings.geo_region` controls which country rule-sets are downloaded and which routing modes are available. Valid values: `ru`, `cn`, `ir`, or `global`.
- `ru` — download RU geoip/geosite, enable Global / Bypass RU / Only RU
- `cn` — download CN geoip/geosite, enable Global / Bypass CN / Only CN
- `ir` — download IR geoip/geosite, enable Global / Bypass IR / Only IR
- `global` — skip geo downloads, enable Global only

On the very first launch (or after upgrading from an older version without `geo_region`), a modal overlay forces you to pick a region before the main UI becomes usable.

`settings.theme` is a string slug naming the active color palette. Default: `tokyo-night`. The reserved slug `omarchy` is a sentinel that auto-follows `~/.config/omarchy/current/theme.name` (and only appears as the `Auto` entry in the picker when Omarchy is installed). Any of the 19 bundled palettes can be selected by name — see `themes/*.toml` in the repository for the canonical list.

Geo rule-set databases are cached in:

```
~/.config/kvn-tui/geo/
```

Logs are split across two files in:

```
~/.config/kvn-tui/logs/
```

- `sing-box.log` — raw sing-box stdout/stderr (the live log pane tails this file)
- `app.log` — daemon and TUI client status messages, errors, and lifecycle events

---

## Author

Created and maintained by [Dmitry Yarikov](https://github.com/yarikov) — <dmitry@yarikov.com>.

## License

MIT
