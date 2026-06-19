# kvn-tui

[![CI](https://github.com/yarikov/kvn-tui/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/yarikov/kvn-tui/actions/workflows/ci.yml)
[![AUR version](https://img.shields.io/aur/version/kvn-tui-bin?logo=arch-linux&label=AUR)](https://aur.archlinux.org/packages/kvn-tui-bin)
[![GitHub Release](https://img.shields.io/github/v/release/yarikov/kvn-tui?logo=github&label=release)](https://github.com/yarikov/kvn-tui/releases/latest)
[![Rust Version](https://img.shields.io/badge/rust-1.87%2B-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/github/license/yarikov/kvn-tui)](LICENSE)

> Terminal VPN client for Arch Linux + Wayland with vim navigation.

`kvn-tui` is a keyboard-driven TUI application for managing VPN connections. It provides a fast, minimal interface for configuring profiles, connecting via the [sing-box](https://sing-box.sagernet.org/) backend, and routing traffic — all without leaving the terminal.

![kvn-tui screenshot](assets/screenshot.png)

---

## Features

- **Vim-style navigation** — `j`/`k` to move, `g`/`G` to jump, `?` for help
- **Profile management** — edit via `$EDITOR`, delete, and organize server profiles
- **One-click paste** — import share links for any supported protocol or subscription URLs directly from the Wayland clipboard
- **Yank to clipboard** — export the selected profile back to a share link with `y`, ready to paste into another client (or copy a subscription's source URL)
- **Subscription support** — subscribe to remote profile feeds (HTTP/HTTPS, Base64 or plain-text, mixed-protocol); configurable auto-update interval (off / 1h / 12h / 1d / 7d)
- **Geo region selection** — choose Russia, China, Iran, or Global on first launch; only relevant routing modes and geo databases are shown/downloaded
- **Routing modes** — Global, Bypass RU, Only RU, Bypass CN, Only CN, Bypass IR, Only IR (powered by geoip/geosite rule-sets)
- **Geo database updates** — download and update rule-sets from within the app
- **Kill switch** — block all outbound traffic if the VPN connection drops; toggled with `K`; powered by nftables + a systemd unit
- **DNS configuration** — built-in presets (Cloudflare DoH, Google DoT, Quad9 DoH, system resolver), strategy cycle, fake-IP toggle (sing-box 1.12 API), plus custom servers and per-domain routing rules via `profiles.json`; opened with `D`
- **Auto-connect** — automatically reconnect to the last used profile on startup
- **Suspend/resume awareness** — automatically detects system resume via D-Bus and reconnects
- **Live logs** — tail sing-box output in a split-pane view
- **Live traffic statistics** — full-width header above the main panes shows instantaneous ↑/↓ rate, cumulative totals, and active connection count while connected; data is scraped from sing-box's Clash API once per second

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
| Error handling | [anyhow](https://github.com/dtolnay/anyhow) + [thiserror](https://github.com/dtolnay/thiserror) | Ergonomic error propagation |
| Utilities | `uuid`, `chrono`, `url`, `urlencoding`, `dirs` | IDs, timestamps, URI parsing, XDG directories |

### Architecture Highlights

- **Daemon + TUI client** — the application splits into a headless daemon (owns sing-box, config, and state) and a TUI client (renders UI and forwards input). They communicate over a Unix domain socket via NDJSON. Running `kvn-tui` auto-starts the daemon in the background if it is not already running.
- **TEA (The Elm Architecture)** — the daemon's business logic is split into pure `Model` / `update` / `Effect` layers. `update.rs` is fully synchronous and side-effect-free, making it easy to unit-test.
- **sing-box runner** — dynamically generates valid sing-box 1.12+ JSON configurations from profile data, validates them with `sing-box check`, and spawns the process with automatic crash detection.
- **Background services** — event reader, ticker, suspend watcher, IPC server, and effect workers run in dedicated threads inside the daemon communicating through an `mpsc` channel. Log tailing and geo updates are driven by messages, not shared mutable state.
- **Atomic config writes** — `profiles.json` is written to a temporary file and renamed to prevent corruption.
- **State I/O** — connection status and active profile are persisted to `state.json` for waybar integration and crash recovery.

---

## Platform Support

> ⚠️ **Current version supports Arch Linux on Wayland only.**

The application relies on Wayland-specific clipboard integration (`wl-paste`) and D-Bus/systemd-logind for power events. X11 support is not available at this time.

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

### Default Key Bindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` | Go to first profile |
| `G` | Go to last profile |
| `Enter` | Connect to selected profile |
| `p` | Paste share link or subscription URL from clipboard |
| `y` | Yank selected profile as a share link (or subscription URL) to clipboard |
| `d` | Delete selected profile |
| `m` | Change routing mode |
| `o` | Select geo region |
| `u` | Update geoip/geosite databases |
| `e` | Open `profiles.json` in `$EDITOR` |
| `K` | Toggle kill switch |
| `D` | DNS settings (presets, strategy, fake-IP) |
| `a` | Toggle auto-connect |
| `r` | Reconnect |
| `s` | Stop / disconnect |
| `q` / `Esc` | Detach TUI — daemon and sing-box keep running. If an overlay is open, closes the overlay first |
| `Ctrl+C` | Quit — stop daemon and sing-box, then exit |
| `?` | Show help |

---

## Configuration

Configuration is stored in:

```
~/.config/kvn-tui/profiles.json
```

The file contains your profile list and application settings (default profile, TUN interface name, DNS configuration, routing mode, auto-connect, geo region). You can edit it manually with the `e` keybinding or any text editor.

`settings.dns` controls how sing-box resolves names. It maps directly onto sing-box 1.12 DNS schema:

- `dns.servers` — list of upstream servers (`local`, `udp`, `tcp`, `tls` / DoT, `https` / DoH, `quic` / DoQ, `fakeip`)
- `dns.rules` — per-domain routing (`domain`, `domain_suffix`, `domain_keyword`, `domain_regex`, `rule_set`) targeting a specific server tag
- `dns.final_server` — fallback server tag when no rule matches
- `dns.strategy` — `prefer_ipv4` / `prefer_ipv6` / `ipv4_only` / `ipv6_only`
- `dns.fakeip_enabled` — when true, an `A`/`AAAA` rule is auto-routed to the `fakeip` server, `dns.independent_cache` is enabled, and `experimental.cache_file.store_fakeip` persists the IP→domain map across restarts

The `D` overlay in the TUI exposes built-in presets (Cloudflare DoH, Google DoT, Quad9 DoH, system resolver), a strategy cycle (`h` / `l` preview, Enter to apply), and the fake-IP toggle. Custom servers and per-domain rules are edited by hand in `profiles.json` via the `e` keybinding.

When `auto_connect` is enabled, the application stores `last_connected_profile` and automatically connects to that profile on the next startup.

`settings.geo_region` controls which country rule-sets are downloaded and which routing modes are available. Valid values: `ru`, `cn`, `ir`, or `global`.
- `ru` — download RU geoip/geosite, enable Global / Bypass RU / Only RU
- `cn` — download CN geoip/geosite, enable Global / Bypass CN / Only CN
- `ir` — download IR geoip/geosite, enable Global / Bypass IR / Only IR
- `global` — skip geo downloads, enable Global only

On the very first launch (or after upgrading from an older version without `geo_region`), a modal overlay forces you to pick a region before the main UI becomes usable.

Geo rule-set databases are cached in:

```
~/.config/kvn-tui/geo/
```

Logs (both sing-box output and app status messages) are written to:

```
~/.config/kvn-tui/logs/sing-box.log
```

---

## Roadmap to v1.0.0

- ~~**Kill switch support** — block all outbound traffic if the VPN connection drops unexpectedly~~ ✅ Done
- ~~**DNS configuration** — custom DNS servers, routing rules, and strategy settings (e.g., DoH, DoT, fake-ip)~~ ✅ Done
- ~~**All sing-box protocols** — extend beyond VLESS to support Shadowsocks, Trojan, VMess, Hysteria 2, and any other protocol sing-box supports~~ ✅ Done
- ~~**Traffic statistics** — live bandwidth and connection stats in the TUI~~ ✅ Done
- ~~**Export profiles** — export profiles to shareable links~~ ✅ Done

---

## Author

Created and maintained by [Dmitry Yarikov](https://github.com/yarikov) — <dmitry@yarikov.com>.

## License

MIT
