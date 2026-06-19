//! Keyboard input dispatchers for every overlay state.
//!
//! Entry point is `handle_key`, which routes by `Model.overlay`. Each
//! overlay has its own handler that returns the same `Vec<Effect>`
//! contract as the top-level `update` function. All handlers stay pure —
//! they mutate the `Model` and return effects; actual I/O happens in
//! the daemon.

use crossterm::event::KeyEvent;

use crate::app::effect::Effect;
use crate::app::model::{AppStatus, Model, Overlay};

use super::*;

pub(super) fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match model.overlay {
        Overlay::None => handle_sources(model, key),
        Overlay::Help => {
            model.overlay = Overlay::None;
            vec![]
        }
        Overlay::ConfirmDelete => handle_confirm_delete(model, key),
        Overlay::RoutingMode => handle_routing_mode(model, key),
        Overlay::GeoRegions => handle_geo_region(model, key),
        Overlay::DnsSettings => handle_dns_settings(model, key),
        Overlay::Error => {
            model.overlay = Overlay::None;
            vec![]
        }
    }
}

pub(super) fn handle_sources(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let mut effects = Vec::new();
    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => model.select_next(),
        KeyCode::Char('k') | KeyCode::Up => model.select_prev(),
        KeyCode::Char('g') => model.select_first(),
        KeyCode::Char('G') => model.select_last(),

        // Actions
        KeyCode::Enter => return handle_enter_on_sources(model),
        KeyCode::Char('p') => return vec![Effect::PasteClipboard],
        KeyCode::Char('d')
            if model.selected_profile().is_some() || model.selected_subscription().is_some() =>
        {
            model.overlay = Overlay::ConfirmDelete;
        }
        KeyCode::Char('m') => {
            model.overlay = Overlay::RoutingMode;
            let available = model.config.settings.geo_routing.available_modes();
            model.routing_selected = available
                .iter()
                .position(|m| *m == model.config.settings.geo_routing.mode())
                .unwrap_or(0);
        }
        KeyCode::Char('u') => return handle_update_key(model),
        KeyCode::Char('i') => {
            if let Some(idx) = model.selected_subscription_index() {
                let (name, label) = if let Some(sub) = model.config.subscriptions.get_mut(idx) {
                    sub.auto_update = sub.auto_update.next();
                    (sub.name.clone(), sub.auto_update.label())
                } else {
                    return effects;
                };
                let mut effects = vec![Effect::SaveConfig];
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!(
                        "Subscription '{}' [{}]",
                        name, label
                    )),
                );
                return effects;
            }
        }
        KeyCode::Char('o') => {
            model.overlay = Overlay::GeoRegions;
            model.geo_region_selected = match model.config.settings.geo_routing.current_region {
                Some(GeoRegion::Ru) => 0,
                Some(GeoRegion::Cn) => 1,
                Some(GeoRegion::Ir) => 2,
                Some(GeoRegion::Global) => 3,
                None => 0,
            };
        }
        KeyCode::Char('e') => {
            return vec![Effect::OpenEditor(
                model.selected_profile_index().unwrap_or(0),
            )];
        }
        KeyCode::Char('r') if model.connection == ConnectionState::Connected => {
            if let Some(profile) = model.selected_profile() {
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!(
                        "Reconnecting to {}…",
                        profile.name
                    )),
                );
            }
            model.connection = ConnectionState::Connecting;
        }
        KeyCode::Char('s') if model.connection == ConnectionState::Connected => {
            return vec![Effect::Disconnect];
        }
        KeyCode::Char('a') => {
            let new_val = !model.config.settings.auto_connect;
            model.config.settings.auto_connect = new_val;
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Info(format!(
                    "Auto-connect {}",
                    if new_val { "enabled" } else { "disabled" }
                )),
            );
            effects.push(Effect::SaveConfig);
            return effects;
        }
        KeyCode::Char('K') => {
            let new_val = !model.config.settings.kill_switch;
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Info(format!(
                    "Kill switch {}…",
                    if new_val { "enabling" } else { "disabling" }
                )),
            );
            effects.push(Effect::ApplyKillSwitch { enabled: new_val });
            return effects;
        }
        KeyCode::Char('D') => {
            model.overlay = Overlay::DnsSettings;
            model.dns_selected = 0;
            model.dns_strategy_draft = None;
        }

        // Help
        KeyCode::Char('?') => model.overlay = Overlay::Help,

        _ => {}
    }
    effects
}

pub(super) fn handle_enter_on_sources(model: &mut Model) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(profile) = model.selected_profile() {
        push_status(
            &mut effects,
            model,
            crate::app::model::AppStatus::Info(format!("Connecting to {}…", profile.name)),
        );
        model.connection = ConnectionState::Connecting;
    } else if let Some(sub) = model.selected_subscription() {
        let id = sub.id;
        let name = sub.name.clone();
        if !model
            .config
            .profiles
            .iter()
            .any(|p| p.subscription_id == Some(id))
        {
            model.subscription_fetching = true;
            model.subscription_updates.insert(id);
            let mut result = vec![Effect::SaveConfig, Effect::UpdateSubscription { id }];
            push_status(
                &mut result,
                model,
                crate::app::model::AppStatus::Info(format!("Updating subscription '{}'…", name)),
            );
            return result;
        }
    } else {
        push_status(
            &mut effects,
            model,
            crate::app::model::AppStatus::Info("No sources. Press p to paste or e to edit.".into()),
        );
    }
    effects
}

pub(super) fn handle_update_key(model: &mut Model) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(idx) = model.selected_subscription_index() {
        if let Some(sub) = model.config.subscriptions.get(idx) {
            let id = sub.id;
            let name = sub.name.clone();
            model.subscription_fetching = true;
            model.subscription_updates.insert(id);
            let mut result = vec![Effect::SaveConfig, Effect::UpdateSubscription { id }];
            push_status(
                &mut result,
                model,
                crate::app::model::AppStatus::Info(format!("Updating subscription '{}'…", name)),
            );
            return result;
        }
    } else if !model.geo_updating {
        if model.config.settings.geo_routing.current_region == Some(GeoRegion::Global) {
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Info(
                    "Geo updates are not available in Global region".to_string(),
                ),
            );
            return effects;
        }
        model.geo_updating = true;
        push_status(
            &mut effects,
            model,
            crate::app::model::AppStatus::Info("Checking for geo updates...".to_string()),
        );
        effects.push(Effect::DownloadGeo);
        return effects;
    }
    effects
}

pub(super) fn handle_confirm_delete(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            model.overlay = Overlay::None;
            let row = model.selected_row();
            match row {
                Some(crate::app::model::SourceRow::StandaloneProfile(_))
                | Some(crate::app::model::SourceRow::SubscriptionProfile { .. }) => {
                    let name = model.selected_profile().map(|p| p.name.clone());
                    model.delete_selected();
                    let mut effects = vec![Effect::SaveConfig];
                    if let Some(name) = name {
                        push_status(
                            &mut effects,
                            model,
                            crate::app::model::AppStatus::Info(format!(
                                "Profile '{}' deleted",
                                name
                            )),
                        );
                    }
                    return effects;
                }
                Some(crate::app::model::SourceRow::SubscriptionHeader(_)) => {
                    let name = model.selected_subscription().map(|s| s.name.clone());
                    model.delete_selected();
                    let mut effects = vec![Effect::SaveConfig];
                    if let Some(name) = name {
                        push_status(
                            &mut effects,
                            model,
                            crate::app::model::AppStatus::Info(format!(
                                "Subscription '{}' deleted",
                                name
                            )),
                        );
                    }
                    return effects;
                }
                _ => {}
            }
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            model.overlay = Overlay::None;
        }
        _ => {}
    }
    vec![]
}

pub(super) fn derive_subscription_name(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "Subscription".to_string())
}

pub(super) fn handle_ipc_command(
    model: &mut Model,
    cmd: crate::app::msg::IpcCommand,
) -> Vec<Effect> {
    use crate::app::msg::IpcCommand;
    let mut effects = match cmd {
        IpcCommand::Attach => vec![],
        IpcCommand::Detach => vec![],
        IpcCommand::Key { code, char, ctrl } => {
            let key_event = rebuild_key_event(&code, char, ctrl);
            if let Some(key) = key_event {
                handle_key(model, key)
            } else {
                vec![]
            }
        }
        IpcCommand::Paste { text } => handle_clipboard_text(model, &text),
        IpcCommand::Copied { name, count } => handle_copied_status(model, name, count),
        IpcCommand::ReloadConfig => {
            vec![Effect::ReloadConfig]
        }
        IpcCommand::Quit => vec![Effect::Quit],
    };
    effects.push(Effect::BroadcastState);
    effects
}

pub(super) fn rebuild_key_event(
    code: &str,
    ch: Option<char>,
    ctrl: bool,
) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key_code = match code {
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Tab" => KeyCode::Tab,
        "BackTab" => KeyCode::BackTab,
        "Char" => KeyCode::Char(ch.unwrap_or(' ')),
        _ => return None,
    };
    let mut modifiers = KeyModifiers::empty();
    if ctrl {
        modifiers |= KeyModifiers::CONTROL;
    }
    Some(KeyEvent::new(key_code, modifiers))
}

pub(super) fn handle_routing_mode(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let available = model.config.settings.geo_routing.available_modes();
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            crate::ui::nav::select_next(&mut model.routing_selected, available.len());
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::ui::nav::select_prev(&mut model.routing_selected);
        }
        KeyCode::Char('g') => {
            crate::ui::nav::select_first(&mut model.routing_selected);
        }
        KeyCode::Char('G') => {
            crate::ui::nav::select_last(&mut model.routing_selected, available.len());
        }
        KeyCode::Enter => {
            if let Some(&mode) = available.get(model.routing_selected) {
                let changed = model.config.settings.geo_routing.mode() != mode;
                model.config.settings.geo_routing.set_mode(mode);
                model.overlay = Overlay::None;
                let mut effects = vec![Effect::SaveConfig];
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!("Routing mode: {}", mode.as_str())),
                );

                if changed && model.connection == ConnectionState::Connected {
                    model.connection = ConnectionState::Connecting;
                    push_status(
                        &mut effects,
                        model,
                        crate::app::model::AppStatus::Info(format!(
                            "Mode changed to {} — reconnecting",
                            mode.as_str()
                        )),
                    );
                }
                return effects;
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            model.overlay = Overlay::None;
        }
        _ => {}
    }
    vec![]
}

pub(super) fn handle_geo_region(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    const REGIONS: &[GeoRegion] = &[
        GeoRegion::Ru,
        GeoRegion::Cn,
        GeoRegion::Ir,
        GeoRegion::Global,
    ];
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            crate::ui::nav::select_next(&mut model.geo_region_selected, REGIONS.len());
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::ui::nav::select_prev(&mut model.geo_region_selected);
        }
        KeyCode::Char('g') => {
            crate::ui::nav::select_first(&mut model.geo_region_selected);
        }
        KeyCode::Char('G') => {
            crate::ui::nav::select_last(&mut model.geo_region_selected, REGIONS.len());
        }
        KeyCode::Enter => {
            if let Some(&region) = REGIONS.get(model.geo_region_selected) {
                let old_region = model.config.settings.geo_routing.current_region;
                let old_mode = model.config.settings.geo_routing.mode();
                let changed = old_region != Some(region);
                model.config.settings.geo_routing.set_region(region);
                model.overlay = Overlay::None;
                let mut effects = vec![Effect::SaveConfig];
                if changed {
                    effects.push(Effect::RefreshGeoLastUpdated);
                }
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!("Geo region: {}", region.as_str())),
                );

                // If the region changed and is not Global, check whether geo databases
                // are present and download them automatically if they are missing.
                if changed && region != GeoRegion::Global {
                    model.geo_updating = true;
                    push_status(
                        &mut effects,
                        model,
                        crate::app::model::AppStatus::Info("Checking geo databases...".to_string()),
                    );
                    effects.push(Effect::DownloadGeoIfMissing);
                }

                // Persist the previously active routing mode under the old region
                // and restore the mode stored for the newly selected region.
                if changed {
                    if let Some(old_region) = old_region {
                        model
                            .config
                            .settings
                            .geo_routing
                            .selected_region_modes
                            .insert(old_region, old_mode);
                    }
                    let new_mode = model.config.settings.geo_routing.mode();
                    if new_mode != old_mode {
                        push_status(
                            &mut effects,
                            model,
                            crate::app::model::AppStatus::Info(format!(
                                "Routing mode: {}",
                                new_mode.as_str()
                            )),
                        );
                    }
                }

                // Trigger auto-connect immediately after picking a region
                // so the user does not have to restart the app.
                if model.config.settings.auto_connect {
                    if let Some(idx) = model
                        .config
                        .settings
                        .last_connected_profile
                        .and_then(|id| model.config.profiles.iter().position(|p| p.id == id))
                    {
                        model.selected = crate::app::model::row_for_profile(&model.config, idx);
                        model.connection = ConnectionState::Connecting;
                        if let Some(profile) = model.config.profiles.get(idx) {
                            push_status(
                                &mut effects,
                                model,
                                crate::app::model::AppStatus::Info(format!(
                                    "Auto-connecting to {}…",
                                    profile.name
                                )),
                            );
                        }
                    }
                }

                if changed && model.connection == ConnectionState::Connected {
                    model.connection = ConnectionState::Connecting;
                    model.logs.push_back("Region changed — reconnecting".into());
                }
                return effects;
            }
        }
        KeyCode::Char('q') | KeyCode::Esc
            if model.config.settings.geo_routing.current_region.is_some() =>
        {
            model.overlay = Overlay::None;
        }
        _ => {}
    }
    vec![]
}

/// Items in the DNS settings overlay, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsSettingsItem {
    PresetCloudflareDoh,
    PresetGoogleDot,
    PresetQuad9Doh,
    PresetSystemLocal,
    CycleStrategy,
    ToggleFakeIp,
}

impl DnsSettingsItem {
    pub const ALL: [DnsSettingsItem; 6] = [
        DnsSettingsItem::PresetCloudflareDoh,
        DnsSettingsItem::PresetGoogleDot,
        DnsSettingsItem::PresetQuad9Doh,
        DnsSettingsItem::PresetSystemLocal,
        DnsSettingsItem::CycleStrategy,
        DnsSettingsItem::ToggleFakeIp,
    ];

    pub fn from_index(idx: usize) -> Option<Self> {
        Self::ALL.get(idx).copied()
    }
}

pub(super) fn handle_dns_settings(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let len = DnsSettingsItem::ALL.len();
    let on_strategy =
        DnsSettingsItem::from_index(model.dns_selected) == Some(DnsSettingsItem::CycleStrategy);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            crate::ui::nav::select_next(&mut model.dns_selected, len);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::ui::nav::select_prev(&mut model.dns_selected);
        }
        KeyCode::Char('g') => crate::ui::nav::select_first(&mut model.dns_selected),
        KeyCode::Char('G') => crate::ui::nav::select_last(&mut model.dns_selected, len),
        KeyCode::Char('l') | KeyCode::Right if on_strategy => {
            let base = model
                .dns_strategy_draft
                .clone()
                .unwrap_or_else(|| model.config.settings.dns.strategy.clone());
            model.dns_strategy_draft = Some(base.next());
        }
        KeyCode::Char('h') | KeyCode::Left if on_strategy => {
            let base = model
                .dns_strategy_draft
                .clone()
                .unwrap_or_else(|| model.config.settings.dns.strategy.clone());
            model.dns_strategy_draft = Some(base.prev());
        }
        KeyCode::Enter => {
            let Some(item) = DnsSettingsItem::from_index(model.dns_selected) else {
                return vec![];
            };
            return apply_dns_item(model, item);
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            model.overlay = Overlay::None;
            model.dns_strategy_draft = None;
        }
        _ => {}
    }
    vec![]
}

fn apply_dns_item(model: &mut Model, item: DnsSettingsItem) -> Vec<Effect> {
    use crate::config::profile::{DnsServer, DnsStrategy};

    let mut effects = Vec::new();
    let mut servers_changed = false;
    let mut strategy_changed = false;
    let mut fakeip_changed = false;

    match item {
        DnsSettingsItem::PresetCloudflareDoh => {
            replace_preset(
                &mut model.config.settings.dns.servers,
                &mut model.config.settings.dns.final_server,
                vec![
                    DnsServer::Local {
                        tag: "local".to_string(),
                    },
                    DnsServer::Https {
                        tag: "remote".to_string(),
                        server: "1.1.1.1".to_string(),
                        server_port: None,
                        path: "/dns-query".to_string(),
                    },
                ],
                "remote",
            );
            servers_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info("DNS preset: Cloudflare DoH (1.1.1.1)".into()),
            );
        }
        DnsSettingsItem::PresetGoogleDot => {
            replace_preset(
                &mut model.config.settings.dns.servers,
                &mut model.config.settings.dns.final_server,
                vec![
                    DnsServer::Local {
                        tag: "local".to_string(),
                    },
                    DnsServer::Tls {
                        tag: "remote".to_string(),
                        server: "8.8.8.8".to_string(),
                        server_port: Some(853),
                    },
                ],
                "remote",
            );
            servers_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info("DNS preset: Google DoT (8.8.8.8)".into()),
            );
        }
        DnsSettingsItem::PresetQuad9Doh => {
            replace_preset(
                &mut model.config.settings.dns.servers,
                &mut model.config.settings.dns.final_server,
                vec![
                    DnsServer::Local {
                        tag: "local".to_string(),
                    },
                    DnsServer::Https {
                        tag: "remote".to_string(),
                        server: "9.9.9.9".to_string(),
                        server_port: None,
                        path: "/dns-query".to_string(),
                    },
                ],
                "remote",
            );
            servers_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info("DNS preset: Quad9 DoH (9.9.9.9)".into()),
            );
        }
        DnsSettingsItem::PresetSystemLocal => {
            replace_preset(
                &mut model.config.settings.dns.servers,
                &mut model.config.settings.dns.final_server,
                vec![DnsServer::Local {
                    tag: "local".to_string(),
                }],
                "local",
            );
            servers_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info("DNS preset: system resolver".into()),
            );
        }
        DnsSettingsItem::CycleStrategy => {
            // Commit the h/l-driven draft if it differs from the current
            // setting; without a draft, Enter is a no-op (h/l is the way to
            // change strategy now).
            let Some(draft) = model.dns_strategy_draft.take() else {
                return effects;
            };
            if draft == model.config.settings.dns.strategy {
                return effects;
            }
            model.config.settings.dns.strategy = draft.clone();
            model.config.settings.dns_strategy = draft.clone();
            strategy_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info(format!("DNS strategy: {}", draft.as_str())),
            );
        }
        DnsSettingsItem::ToggleFakeIp => {
            let enabled = !model.config.settings.dns.fakeip_enabled;
            model.config.settings.dns.fakeip_enabled = enabled;
            if enabled
                && !model
                    .config
                    .settings
                    .dns
                    .servers
                    .iter()
                    .any(|s| matches!(s, DnsServer::FakeIp { .. }))
            {
                model.config.settings.dns.servers.push(DnsServer::FakeIp {
                    tag: "fakeip".to_string(),
                    inet4_range: "198.18.0.0/15".to_string(),
                    inet6_range: "fc00::/18".to_string(),
                });
            }
            // Force a strategy that fake-IP can serve sensibly.
            if enabled && matches!(model.config.settings.dns.strategy, DnsStrategy::OnlyIpv6) {
                model.config.settings.dns.strategy = DnsStrategy::PreferIpv4;
                model.config.settings.dns_strategy = DnsStrategy::PreferIpv4;
            }
            fakeip_changed = true;
            push_status(
                &mut effects,
                model,
                AppStatus::Info(format!(
                    "Fake-IP {}",
                    if enabled { "enabled" } else { "disabled" }
                )),
            );
        }
    }

    if servers_changed || strategy_changed || fakeip_changed {
        effects.push(Effect::SaveConfig);
        effects.push(Effect::BroadcastState);
        if model.connection == ConnectionState::Connected {
            if let Some(profile) = model.selected_profile().cloned() {
                let settings = model.config.settings.clone();
                model.connection = ConnectionState::Connecting;
                push_status(
                    &mut effects,
                    model,
                    AppStatus::Info("DNS changed — reconnecting…".into()),
                );
                effects.push(Effect::Connect { profile, settings });
            }
        }
    }

    effects
}

fn replace_preset(
    servers: &mut Vec<crate::config::profile::DnsServer>,
    final_server: &mut String,
    preset: Vec<crate::config::profile::DnsServer>,
    new_final: &str,
) {
    *servers = preset;
    *final_server = new_final.to_string();
}
