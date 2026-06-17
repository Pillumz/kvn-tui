# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build --release          # release build
cargo test                     # run all tests
cargo test ui::layout          # run a single module's tests
INSTA_UPDATE=always cargo test # run tests and auto-accept snapshot changes
cargo fmt                      # format (required before committing)
cargo clippy --all-targets --all-features  # lint (fix warnings before committing)
```

Snapshot tests use [insta](https://insta.rs/). When layout changes affect terminal output, run with `INSTA_UPDATE=always` to regenerate `.snap` files, then review the diffs.

## Architecture

See `AGENTS.md` for the full module map, design decisions, and coding conventions. Key points:

**Daemon + TUI client split** — `kvn-tui` auto-starts a headless daemon (`--daemon`) that owns the sing-box process and all state, then connects a TUI client over a Unix socket (NDJSON protocol in `ipc.rs`). Re-running `kvn-tui` re-attaches to the existing daemon without restarting sing-box. `q`/`Esc` detaches the TUI; `Ctrl+C` kills the daemon.

**TEA architecture** — business logic lives entirely in the pure function `app::update::update(model, msg) -> Vec<Effect>`. It must not perform I/O. Side effects are declared as `Effect` variants and executed exclusively by `daemon::execute_daemon_effect`. To add a new side effect: add an `Effect` variant, handle it there.

**UI rendering** — `src/ui/layout.rs` builds ratatui `Line`/`Span` trees from `Model`. `src/ui/styles.rs` is the single source of truth for colors. `src/ui/nav.rs` handles cursor movement. All UI code is used only by the TUI client, never by the daemon.

**Config** — `~/.config/kvn-tui/profiles.json`. All writes must be atomic (write to `.tmp`, then `fs::rename`). See `config::save_config_at` for the pattern.

**Platform** — Arch Linux + Wayland only. Clipboard uses `wl-paste`; suspend detection uses zbus/systemd-logind. Do not add X11 fallbacks.
