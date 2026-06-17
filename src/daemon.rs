use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::app::effect::Effect;
use crate::app::model::{AppStatus, ConnectionState, Model, Overlay};
use crate::app::msg::{GeoResult, Msg, StateSnapshot};
use crate::app::update::update;
use crate::infra::process_handle::ProcessHandle;
use crate::ipc::{IpcServer, cleanup_socket};

/// Run the daemon main loop.
pub fn run(mut model: Model) -> Result<()> {
    let (tx, rx) = channel::<Msg>();
    let ipc_server = IpcServer::bind(tx.clone())?;

    spawn_ticker(tx.clone());
    spawn_suspend_watcher(tx.clone());

    reconcile_kill_switch_state(&mut model);

    let process_slot = Arc::new(Mutex::new(None));

    let result = run_loop(&mut model, rx, &tx, process_slot.clone(), &ipc_server);

    // Cleanup
    if let Some(mut handle) = process_slot.lock().unwrap().take() {
        if let Err(e) = handle.kill_and_wait() {
            tracing::warn!("Failed to stop sing-box on exit: {}", e);
        }
    }
    cleanup_socket();

    result
}

fn run_loop(
    model: &mut Model,
    rx: std::sync::mpsc::Receiver<Msg>,
    tx: &Sender<Msg>,
    process_slot: Arc<Mutex<Option<ProcessHandle>>>,
    ipc_server: &IpcServer,
) -> Result<()> {
    loop {
        let msg = rx.recv()?;
        let effects = update(model, msg);
        let mut should_broadcast = false;

        for effect in &effects {
            if matches!(
                effect,
                Effect::Connect { .. }
                    | Effect::Disconnect
                    | Effect::DownloadGeo
                    | Effect::WriteState
                    | Effect::SaveConfig
                    | Effect::PasteClipboard
                    | Effect::UpdateSubscription { .. }
                    | Effect::BroadcastState
                    | Effect::ApplyKillSwitch { .. }
            ) {
                should_broadcast = true;
            }
        }

        for effect in effects {
            execute_daemon_effect(effect, tx, model, &process_slot)?;
        }

        if model.should_quit {
            break;
        }

        if should_broadcast {
            ipc_server.broadcast(&build_snapshot(model));
        }
    }
    Ok(())
}

fn execute_daemon_effect(
    effect: Effect,
    tx: &Sender<Msg>,
    model: &mut Model,
    process_slot: &Arc<Mutex<Option<ProcessHandle>>>,
) -> Result<()> {
    match effect {
        Effect::Connect { profile, settings } => {
            if let Some(mut handle) = process_slot.lock().unwrap().take() {
                if let Err(e) = handle.kill_and_wait() {
                    tracing::warn!("Failed to stop sing-box process: {}", e);
                }
            } else if let Some(pid) = model.singbox_pid {
                unsafe {
                    let _ = libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            model.connection = ConnectionState::ConnectPending;
            let tx = tx.clone();
            let slot = process_slot.clone();
            let kill_switch = model.config.settings.kill_switch;
            thread::spawn(move || {
                if kill_switch {
                    if let Err(e) = open_handshake_window(&profile) {
                        let _ = tx.send(Msg::ConnectFailed(format!(
                            "kill switch handshake setup failed: {}",
                            e
                        )));
                        return;
                    }
                }
                match crate::singbox::runner::start(&profile, &settings) {
                    Ok(handle) => {
                        let pid = handle.pid;
                        *slot.lock().unwrap() = Some(handle);
                        let _ = tx.send(Msg::Connected { pid });
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::ConnectFailed(e.to_string()));
                    }
                }
            });
        }
        Effect::Disconnect => {
            if let Some(mut handle) = process_slot.lock().unwrap().take() {
                if let Err(e) = handle.kill_and_wait() {
                    tracing::warn!("Failed to stop sing-box process: {}", e);
                }
            } else if let Some(pid) = model.singbox_pid {
                unsafe {
                    let _ = libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            model.connection = ConnectionState::Idle;
            model.active_profile_id = None;
            model.singbox_pid = None;
            model.set_status(AppStatus::Info("Disconnected".into()));
            model.overlay = Overlay::None;
            crate::services::waybar::write_state(model);
            if model.config.settings.kill_switch {
                if let Err(e) = crate::services::killswitch::revoke() {
                    tracing::warn!("Failed to flush kill switch handshake set: {}", e);
                }
            }
        }
        Effect::DownloadGeo => {
            model.geo_updating = true;
            let tx = tx.clone();
            let region = model
                .config
                .settings
                .geo_routing
                .current_region
                .unwrap_or(crate::config::profile::GeoRegion::Global);
            thread::spawn(move || {
                let result = match crate::infra::geo::GeoManager::new() {
                    Ok(gm) => match gm.update_if_needed(region) {
                        Ok(geo_result) => geo_result,
                        Err(e) => GeoResult::Error(e.to_string()),
                    },
                    Err(e) => GeoResult::Error(e.to_string()),
                };
                let _ = tx.send(Msg::GeoUpdated(result));
            });
        }
        Effect::RefreshGeoLastUpdated => {
            let tx = tx.clone();
            let region = model
                .config
                .settings
                .geo_routing
                .current_region
                .unwrap_or(crate::config::profile::GeoRegion::Global);
            thread::spawn(move || {
                let last_updated = crate::infra::geo::GeoManager::new()
                    .ok()
                    .and_then(|g| g.last_updated(region));
                let _ = tx.send(Msg::GeoLastUpdated(last_updated));
            });
        }
        Effect::DownloadGeoIfMissing => {
            model.geo_updating = true;
            let tx = tx.clone();
            let region = model
                .config
                .settings
                .geo_routing
                .current_region
                .unwrap_or(crate::config::profile::GeoRegion::Global);
            thread::spawn(move || {
                let result = match crate::infra::geo::GeoManager::new() {
                    Ok(gm) => {
                        if gm.has_databases(region) {
                            GeoResult::UpToDate
                        } else {
                            match gm.update_if_needed(region) {
                                Ok(geo_result) => geo_result,
                                Err(e) => GeoResult::Error(e.to_string()),
                            }
                        }
                    }
                    Err(e) => GeoResult::Error(e.to_string()),
                };
                let _ = tx.send(Msg::GeoUpdated(result));
            });
        }
        Effect::WriteState => {
            crate::services::waybar::write_state(model);
        }
        Effect::SaveConfig => {
            if let Err(e) = model.save() {
                model.set_status(AppStatus::Error(format!("Failed to save config: {}", e)));
            }
        }
        Effect::PasteClipboard => {
            // In daemon mode paste is handled via IpcCommand::Paste directly;
            // this variant should not normally reach the daemon.
        }
        Effect::UpdateSubscription { id } => {
            if let Some(sub) = model.config.subscriptions.iter().find(|s| s.id == id) {
                let url = sub.url.clone();
                let tx = tx.clone();
                thread::spawn(move || {
                    let result = crate::infra::subscription::fetch_subscription(&url)
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Msg::SubscriptionFetched { id, result });
                });
            }
        }
        Effect::BroadcastState => {}
        Effect::Quit => {
            model.should_quit = true;
        }
        Effect::OpenEditor(_) => {
            // Editor is a TUI-local operation; daemon ignores it.
        }
        Effect::AppendAppLog { level, message } => {
            crate::services::log_tailer::append_app_log(&level, &message);
        }
        Effect::ReloadConfig => {
            let tx = tx.clone();
            thread::spawn(move || {
                let result = crate::config::load_config()
                    .map_err(|e| e.to_string())
                    .map(|c| c.validate().map(|_| c).map_err(|e| e.to_string()));
                let result = match result {
                    Ok(Ok(c)) => Ok(c),
                    Ok(Err(e)) | Err(e) => Err(e),
                };
                let _ = tx.send(Msg::ConfigReloaded(result));
            });
        }
        Effect::ApplyKillSwitch { enabled } => {
            let tx = tx.clone();
            thread::spawn(move || {
                let error = crate::services::killswitch::apply(enabled)
                    .err()
                    .map(|e| e.to_string());
                let _ = tx.send(Msg::KillSwitchApplied { enabled, error });
            });
        }
    }
    Ok(())
}

/// At daemon startup, align `settings.kill_switch` with the actual systemd
/// unit state. systemd is the source of truth — if a user disabled the unit
/// manually or never installed the helper, the persisted bool would otherwise
/// drift and the TUI would render `[KS]` against an open firewall.
fn reconcile_kill_switch_state(model: &mut Model) {
    let active = match crate::services::killswitch::is_active() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to query kill switch unit state: {}", e);
            return;
        }
    };
    if model.config.settings.kill_switch != active {
        tracing::info!(
            "Reconciling kill switch state: config={}, systemd={}",
            model.config.settings.kill_switch,
            active
        );
        model.config.settings.kill_switch = active;
        if let Err(e) = model.save() {
            tracing::warn!("Failed to persist reconciled kill switch state: {}", e);
        }
    }
}

/// Pre-resolve the VPN endpoint and open a temporary nft exception so the
/// initial handshake can pass through the kill switch. Also allowlists the
/// hard-coded DoH bootstrap (`1.1.1.1:443`) that sing-box uses to resolve the
/// VPN server hostname (see `src/singbox/config.rs`).
///
/// Set elements are deduplicated by nftables and remain until disconnect, so
/// repeated calls are idempotent and safe across reconnects.
fn open_handshake_window(profile: &crate::config::profile::Profile) -> Result<()> {
    let endpoints = crate::services::killswitch::resolve_endpoints(&profile.address, profile.port)?;
    // sing-box VLESS over TLS/REALITY is TCP; we don't have UDP transports today.
    for addr in &endpoints {
        crate::services::killswitch::allow_endpoint(addr, "tcp")?;
    }
    let doh: std::net::SocketAddr = format!(
        "{}:{}",
        crate::services::killswitch::BOOTSTRAP_DOH_HOST,
        crate::services::killswitch::BOOTSTRAP_DOH_PORT
    )
    .parse()
    .expect("bootstrap DoH address is a constant");
    crate::services::killswitch::allow_endpoint(&doh, "tcp")?;
    Ok(())
}

fn build_snapshot(model: &Model) -> StateSnapshot {
    StateSnapshot {
        connection: model.connection,
        status: model.status.text().to_string(),
        status_is_error: matches!(model.status, AppStatus::Error(_)),
        singbox_pid: model.singbox_pid,
        active_profile_id: model.active_profile_id.map(|id| id.to_string()),
        selected: model.selected,
        routing_selected: model.routing_selected,
        geo_region_selected: model.geo_region_selected,
        geo_updating: model.geo_updating,
        geo_last_updated: model.geo_last_updated.clone(),
        overlay: model.overlay,
        profiles: model.config.profiles.clone(),
        subscriptions: model.config.subscriptions.clone(),
        settings: model.config.settings.clone(),
    }
}

fn spawn_ticker(tx: Sender<Msg>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));
            if tx.send(Msg::Tick).is_err() {
                break;
            }
        }
    });
}

fn spawn_suspend_watcher(tx: Sender<Msg>) {
    thread::spawn(move || {
        crate::services::suspend::listen_blocking(tx);
    });
}
