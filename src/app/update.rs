use crate::app::effect::Effect;
use crate::app::model::{AppStatus, ConnectionState, Model, Overlay};
use crate::app::msg::{GeoResult, Msg};
#[cfg(test)]
use crate::config::profile::Protocol;
use crate::config::profile::{GeoRegion, Profile, Subscription, SubscriptionAutoUpdate};
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent};
use uuid::Uuid;

/// Pure function: Model + Msg → updated Model + list of Effects.
/// No I/O, no threads, no system calls.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(key) => handle_key(model, key),
        Msg::Tick => handle_tick(model),
        Msg::GeoUpdated(result) => handle_geo_result(model, result),
        Msg::GeoLastUpdated(last_updated) => {
            model.geo_last_updated = last_updated;
            vec![Effect::BroadcastState]
        }
        Msg::SystemResumed => {
            if model.connection == ConnectionState::Connected {
                let profile = model.selected_profile().cloned();
                let settings = model.config.settings.clone();
                let mut effects = profile
                    .map(|p| {
                        vec![Effect::Connect {
                            profile: p,
                            settings,
                        }]
                    })
                    .unwrap_or_default();
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info("Resumed — reconnecting…".into()),
                );
                effects
            } else {
                vec![]
            }
        }
        Msg::Connected { pid } => {
            model.singbox_pid = Some(pid);
            model.connection = ConnectionState::Connected;
            model.overlay = Overlay::None;
            let mut effects = vec![Effect::WriteState];
            if let Some(profile) = model.selected_profile() {
                let profile_id = profile.id;
                let profile_name = profile.name.clone();
                model.active_profile_id = Some(profile_id);
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!("Connected to {}", profile_name)),
                );
                // Persist last connected profile for auto-connect on next startup.
                if model.config.settings.last_connected_profile != Some(profile_id) {
                    model.config.settings.last_connected_profile = Some(profile_id);
                    effects.push(Effect::SaveConfig);
                }
            }
            effects
        }
        Msg::SubscriptionFetched { id, result } => handle_subscription_result(model, id, result),
        Msg::ConnectFailed(err) => {
            model.connection = ConnectionState::Idle;
            model.overlay = Overlay::Error;
            let mut effects = vec![Effect::BroadcastState];
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Error(format!("Connection failed: {}", err)),
            );
            effects
        }

        Msg::Resize => {
            model.needs_redraw = true;
            vec![]
        }
        Msg::IpcCommand(cmd) => handle_ipc_command(model, cmd),
        Msg::StateUpdate(_) => vec![],
        Msg::ConfigReloaded(result) => handle_config_reloaded(model, result),
        Msg::KillSwitchApplied { enabled, error } => {
            handle_kill_switch_applied(model, enabled, error)
        }
    }
}

fn handle_kill_switch_applied(
    model: &mut Model,
    enabled: bool,
    error: Option<String>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    match error {
        None => {
            model.config.settings.kill_switch = enabled;
            push_status(
                &mut effects,
                model,
                AppStatus::Info(format!(
                    "Kill switch {}",
                    if enabled { "enabled" } else { "disabled" }
                )),
            );
            effects.push(Effect::SaveConfig);
            effects.push(Effect::BroadcastState);
        }
        Some(err) => {
            push_status(
                &mut effects,
                model,
                AppStatus::Error(format!("Kill switch: {}", err)),
            );
            effects.push(Effect::BroadcastState);
        }
    }
    effects
}

/// Set the application status (pure, in-memory) and return an effect that
/// appends the same message to the on-disk log file.
fn set_status(model: &mut Model, status: AppStatus) -> Option<Effect> {
    let text = status.text();
    let effect = if text.is_empty() {
        None
    } else {
        let level = match &status {
            AppStatus::Info(_) => "INFO",
            AppStatus::Error(_) => "ERROR",
        };
        Some(Effect::AppendAppLog {
            level: level.to_string(),
            message: text.to_string(),
        })
    };
    model.set_status(status);
    effect
}

fn push_status(effects: &mut Vec<Effect>, model: &mut Model, status: AppStatus) {
    if let Some(e) = set_status(model, status) {
        effects.push(e);
    }
}

fn handle_config_reloaded(
    model: &mut Model,
    result: Result<crate::config::profile::Config, String>,
) -> Vec<Effect> {
    match result {
        Ok(config) => {
            model.selected = crate::app::model::row_for_profile(&config, config.resolve_selected());
            model.config = config;
            let mut effects = vec![Effect::BroadcastState];
            push_status(
                &mut effects,
                model,
                AppStatus::Info("Profiles reloaded".into()),
            );
            effects
        }
        Err(e) => {
            let mut effects = vec![Effect::BroadcastState];
            push_status(
                &mut effects,
                model,
                AppStatus::Error(format!("Failed to reload: {}", e)),
            );
            effects
        }
    }
}

fn handle_tick(model: &mut Model) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Check geo updates — in the new architecture geo runs in its own thread
    // and sends GeoUpdated messages, so nothing to do here directly.

    // Connection handling
    if model.connection == ConnectionState::Connecting {
        if let Some(profile) = model.selected_profile().cloned() {
            let settings = model.config.settings.clone();
            effects.push(Effect::Connect { profile, settings });
        } else {
            model.connection = ConnectionState::Idle;
            model.overlay = Overlay::None;
            effects.push(Effect::BroadcastState);
        }
    }

    // Auto-update subscriptions that are due.
    effects.extend(check_due_subscriptions(model));

    effects
}

fn check_due_subscriptions(model: &mut Model) -> Vec<Effect> {
    let mut effects = Vec::new();
    let now = Local::now();
    for sub in &model.config.subscriptions {
        let interval = sub.auto_update.interval_minutes();
        if interval == 0 {
            continue;
        }
        if model.subscription_updates.contains(&sub.id) {
            continue;
        }
        let due = match sub.last_updated {
            None => true,
            Some(last) => {
                let elapsed = now.signed_duration_since(last);
                elapsed.num_minutes() >= interval as i64
            }
        };
        if due {
            model.subscription_updates.insert(sub.id);
            effects.push(Effect::UpdateSubscription { id: sub.id });
        }
    }
    effects
}

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match model.overlay {
        Overlay::None => handle_sources(model, key),
        Overlay::Help => {
            model.overlay = Overlay::None;
            vec![]
        }
        Overlay::ConfirmDelete => handle_confirm_delete(model, key),
        Overlay::RoutingMode => handle_routing_mode(model, key),
        Overlay::GeoRegions => handle_geo_region(model, key),
        Overlay::Error => {
            model.overlay = Overlay::None;
            vec![]
        }
    }
}

fn handle_sources(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
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

        // Help
        KeyCode::Char('?') => model.overlay = Overlay::Help,

        _ => {}
    }
    effects
}

fn handle_enter_on_sources(model: &mut Model) -> Vec<Effect> {
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

fn handle_update_key(model: &mut Model) -> Vec<Effect> {
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

fn handle_confirm_delete(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
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

fn derive_subscription_name(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "Subscription".to_string())
}

fn handle_ipc_command(model: &mut Model, cmd: crate::app::msg::IpcCommand) -> Vec<Effect> {
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
        IpcCommand::ReloadConfig => {
            vec![Effect::ReloadConfig]
        }
        IpcCommand::Quit => vec![Effect::Quit],
    };
    effects.push(Effect::BroadcastState);
    effects
}

fn rebuild_key_event(
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

fn handle_routing_mode(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
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

fn handle_geo_region(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
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

fn handle_clipboard_text(model: &mut Model, text: &str) -> Vec<Effect> {
    let trimmed = text.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return add_and_fetch_subscription(model, trimmed);
    }

    match crate::config::profile::parse_share_link(trimmed) {
        Ok(profile) => {
            if model.has_duplicate(&profile) {
                let mut effects = Vec::new();
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Error("Profile already exists".into()),
                );
                return effects;
            }
            let name = profile.name.clone();
            model.add_profile(profile);
            let mut effects = vec![Effect::SaveConfig];
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Info(format!("Pasted profile: {}", name)),
            );
            effects
        }
        Err(e) => {
            let mut effects = Vec::new();
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Error(format!("Invalid URI: {}", e)),
            );
            effects
        }
    }
}

fn add_and_fetch_subscription(model: &mut Model, url: &str) -> Vec<Effect> {
    let name = derive_subscription_name(url);
    let id = Uuid::new_v4();
    let sub = Subscription {
        id,
        name: name.clone(),
        url: url.to_string(),
        auto_update: SubscriptionAutoUpdate::default(),
        last_updated: None,
    };
    model.config.subscriptions.push(sub);
    model.selected = crate::app::model::row_for_subscription_header(
        &model.config,
        model.config.subscriptions.len().saturating_sub(1),
    );
    model.subscription_fetching = true;
    model.subscription_updates.insert(id);

    let mut effects = vec![Effect::SaveConfig, Effect::UpdateSubscription { id }];
    push_status(
        &mut effects,
        model,
        crate::app::model::AppStatus::Info(format!(
            "Added subscription '{}' and fetching profiles…",
            name
        )),
    );
    effects
}

fn handle_subscription_result(
    model: &mut Model,
    id: Uuid,
    result: Result<Vec<Profile>, String>,
) -> Vec<Effect> {
    let managed = !id.is_nil();
    model.subscription_updates.remove(&id);
    if model.subscription_updates.is_empty() {
        model.subscription_fetching = false;
    }

    if managed {
        if let Some(sub) = model.config.subscriptions.iter_mut().find(|s| s.id == id) {
            sub.last_updated = Some(Local::now());
        }
        // Replace previously imported profiles from this subscription.
        model
            .config
            .profiles
            .retain(|p| p.subscription_id != Some(id));
    }

    match result {
        Ok(profiles) => {
            let mut imported = 0;
            for mut profile in profiles {
                if let Some(idx) = model
                    .config
                    .profiles
                    .iter()
                    .position(|p| p.uuid == profile.uuid)
                {
                    let existing = &model.config.profiles[idx];
                    if existing.subscription_id.is_none() {
                        // Update the standalone profile in place and link it to
                        // the subscription, preserving its identity.
                        profile.id = existing.id;
                        profile.uuid.clone_from(&existing.uuid);
                        if managed {
                            profile.subscription_id = Some(id);
                        }
                        model.config.profiles[idx] = profile;
                        imported += 1;
                    }
                    // Profiles belonging to other subscriptions are skipped.
                } else {
                    if managed {
                        profile.subscription_id = Some(id);
                    }
                    model.add_profile(profile);
                    imported += 1;
                }
            }

            let mut effects = Vec::new();
            if imported > 0 || managed {
                effects.push(Effect::SaveConfig);
            }
            if imported > 0 {
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info(format!(
                        "Imported {} profile(s) from subscription",
                        imported
                    )),
                );
            } else {
                push_status(
                    &mut effects,
                    model,
                    crate::app::model::AppStatus::Info("No new profiles in subscription".into()),
                );
            }
            effects
        }
        Err(err) => {
            let mut effects = Vec::new();
            if managed {
                effects.push(Effect::SaveConfig);
            }
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Error(format!("Subscription failed: {}", err)),
            );
            effects
        }
    }
}

fn handle_geo_result(model: &mut Model, result: GeoResult) -> Vec<Effect> {
    model.geo_updating = false;
    let mut effects = match result {
        GeoResult::Updated {
            parts,
            last_updated,
        } => {
            model.geo_last_updated = last_updated;
            let mut log_effects = Vec::new();
            for part in &parts {
                let text = format!("Updated: {}", part);
                log_effects.push(Effect::AppendAppLog {
                    level: "INFO".to_string(),
                    message: text.clone(),
                });
                model.logs.push_back(text);
            }
            push_status(
                &mut log_effects,
                model,
                crate::app::model::AppStatus::Info("Geo databases updated".into()),
            );
            if model.connection == ConnectionState::Connected {
                model
                    .logs
                    .push_back("Reconnecting to apply new geo databases".into());
                model.connection = ConnectionState::Connecting;
            }
            log_effects
        }
        GeoResult::UpToDate => {
            let mut effects = Vec::new();
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Info("Geo databases are up to date".into()),
            );
            effects
        }
        GeoResult::Error(err) => {
            let mut effects = Vec::new();
            push_status(
                &mut effects,
                model,
                crate::app::model::AppStatus::Error(err),
            );
            effects
        }
    };
    effects.push(Effect::BroadcastState);
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{RoutingMode, SubscriptionAutoUpdate};
    use crate::test_helpers::*;
    use crossterm::event::KeyCode;

    fn app_log_info(message: &str) -> Effect {
        Effect::AppendAppLog {
            level: "INFO".to_string(),
            message: message.to_string(),
        }
    }

    fn app_log_error(message: &str) -> Effect {
        Effect::AppendAppLog {
            level: "ERROR".to_string(),
            message: message.to_string(),
        }
    }

    #[test]
    fn handle_event_non_key_is_noop() {
        let mut model = model_with_profiles(vec![]);
        let effects = update(&mut model, Msg::Resize);
        assert!(effects.is_empty());
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn normal_mode_navigates() {
        let mut model = model_with_profiles(vec![
            Profile::new(
                "A".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "B".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ]);
        assert_eq!(model.selected, 0);
        let _ = handle_sources(&mut model, key('j'));
        assert_eq!(model.selected, 1);
        let _ = handle_sources(&mut model, key('k'));
        assert_eq!(model.selected, 0);
        let _ = handle_sources(&mut model, key('G'));
        assert_eq!(model.selected, 1);
        let _ = handle_sources(&mut model, key('g'));
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn normal_mode_enter_connects() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        let effects = handle_sources(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(model.connection, ConnectionState::Connecting);
        assert_eq!(effects, vec![app_log_info("Connecting to A…")]);
    }

    #[test]
    fn normal_mode_enter_no_profile() {
        let mut model = model_with_profiles(vec![]);
        let effects = handle_sources(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(
            effects,
            vec![app_log_info("No sources. Press p to paste or e to edit.")]
        );
    }

    #[test]
    fn normal_mode_enter_on_subscription_header_does_nothing() {
        use crate::config::profile::Subscription;
        use uuid::Uuid;

        let sub_id = Uuid::new_v4();
        let mut profile = Profile::new(
            "SubProfile".to_string(),
            Protocol::Vless,
            "2.2.2.2".to_string(),
            443,
            "u2".to_string(),
        );
        profile.subscription_id = Some(sub_id);
        let mut model = model_with_profiles(vec![profile]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        model.selected = 0; // subscription header
        let effects = handle_sources(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(model.connection, ConnectionState::Idle);
        assert!(effects.is_empty());
    }

    #[test]
    fn normal_mode_enter_on_empty_subscription_updates_it() {
        use crate::config::profile::Subscription;
        use uuid::Uuid;

        let sub_id = Uuid::new_v4();
        let mut model = model_with_profiles(vec![]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        model.selected = 0; // subscription header
        let effects = handle_sources(&mut model, KeyEvent::from(KeyCode::Enter));
        assert!(model.subscription_fetching);
        assert!(effects.contains(&Effect::UpdateSubscription { id: sub_id }));
        assert!(effects.contains(&Effect::SaveConfig));
    }

    #[test]
    fn normal_mode_d_confirms_delete() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        let effects = handle_sources(&mut model, key('d'));
        assert_eq!(model.overlay, Overlay::ConfirmDelete);
        assert!(effects.is_empty());
    }

    #[test]
    fn normal_mode_m_opens_routing() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model
            .config
            .settings
            .geo_routing
            .set_mode(RoutingMode::BypassRu);
        let effects = handle_sources(&mut model, key('m'));
        assert_eq!(model.overlay, Overlay::RoutingMode);
        assert_eq!(model.routing_selected, 1);
        assert!(effects.is_empty());
    }

    #[test]
    fn ipc_command_attach_broadcasts_state() {
        let mut model = model_with_profiles(vec![]);
        let effects = handle_ipc_command(&mut model, crate::app::msg::IpcCommand::Attach);
        assert_eq!(effects, vec![Effect::BroadcastState]);
    }

    #[test]
    fn ipc_command_reload_config_returns_effect() {
        let mut model = model_with_profiles(vec![]);
        let effects = handle_ipc_command(&mut model, crate::app::msg::IpcCommand::ReloadConfig);
        assert_eq!(effects, vec![Effect::ReloadConfig, Effect::BroadcastState]);
    }

    #[test]
    fn config_reloaded_updates_model() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        let config = model.config.clone();
        let effects = update(&mut model, Msg::ConfigReloaded(Ok(config)));
        assert_eq!(
            effects,
            vec![Effect::BroadcastState, app_log_info("Profiles reloaded")]
        );
    }

    #[test]
    fn config_reloaded_error_updates_status() {
        let mut model = model_with_profiles(vec![]);
        let effects = update(
            &mut model,
            Msg::ConfigReloaded(Err("parse error".to_string())),
        );
        assert_eq!(
            effects,
            vec![
                Effect::BroadcastState,
                app_log_error("Failed to reload: parse error")
            ]
        );
    }

    #[test]
    fn ipc_command_key_navigates() {
        let mut model = model_with_profiles(vec![
            Profile::new(
                "A".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "B".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ]);
        let effects = handle_ipc_command(
            &mut model,
            crate::app::msg::IpcCommand::Key {
                code: "Char".into(),
                char: Some('j'),
                ctrl: false,
            },
        );
        assert_eq!(effects, vec![Effect::BroadcastState]);
        assert_eq!(model.selected, 1);
    }

    #[test]
    fn rebuild_key_event_handles_tab_and_backtab() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let tab = rebuild_key_event("Tab", None, false).unwrap();
        assert_eq!(tab.code, KeyCode::Tab);
        assert_eq!(tab.modifiers, KeyModifiers::empty());

        let backtab = rebuild_key_event("BackTab", None, false).unwrap();
        assert_eq!(backtab.code, KeyCode::BackTab);
        assert_eq!(backtab.modifiers, KeyModifiers::empty());
    }

    #[test]
    fn help_mode_any_key_returns_to_normal() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::Help;
        let effects = handle_key(&mut model, key('x'));
        assert_eq!(model.overlay, Overlay::None);
        assert!(effects.is_empty());
    }

    #[test]
    fn error_mode_any_key_returns_to_normal() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::Error;
        let effects = handle_key(&mut model, key('x'));
        assert_eq!(model.overlay, Overlay::None);
        assert!(effects.is_empty());
    }

    #[test]
    fn confirm_delete_yes() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.overlay = Overlay::ConfirmDelete;
        let effects = handle_confirm_delete(&mut model, key('y'));
        assert!(model.config.profiles.is_empty());
        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(
            effects,
            vec![Effect::SaveConfig, app_log_info("Profile 'A' deleted")]
        );
    }

    #[test]
    fn confirm_delete_no() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.overlay = Overlay::ConfirmDelete;
        let effects = handle_confirm_delete(&mut model, key('n'));
        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(model.overlay, Overlay::None);
        assert!(effects.is_empty());
    }

    #[test]
    fn routing_mode_navigates() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model.overlay = Overlay::RoutingMode;
        model.routing_selected = 0;

        let _ = handle_routing_mode(&mut model, key('j'));
        assert_eq!(model.routing_selected, 1);
        let _ = handle_routing_mode(&mut model, key('j'));
        assert_eq!(model.routing_selected, 2);
        let _ = handle_routing_mode(&mut model, key('j'));
        assert_eq!(model.routing_selected, 2); // clamp

        let _ = handle_routing_mode(&mut model, key('k'));
        assert_eq!(model.routing_selected, 1);
        let _ = handle_routing_mode(&mut model, key('g'));
        assert_eq!(model.routing_selected, 0);
        let _ = handle_routing_mode(&mut model, key('G'));
        assert_eq!(model.routing_selected, 2);
    }

    #[test]
    fn routing_mode_enter_changes_mode() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model.overlay = Overlay::RoutingMode;
        model.routing_selected = 2; // OnlyRu

        let effects = handle_routing_mode(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.mode(),
            RoutingMode::OnlyRu
        );
        assert_eq!(model.overlay, Overlay::None);
        assert!(model.status.text().contains("Only RU"));
        assert_eq!(
            effects,
            vec![Effect::SaveConfig, app_log_info("Routing mode: Only RU")]
        );
    }

    #[test]
    fn routing_mode_esc_cancels() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model.overlay = Overlay::RoutingMode;
        model.routing_selected = 2;
        let effects = handle_routing_mode(&mut model, KeyEvent::from(KeyCode::Esc));
        assert_eq!(model.overlay, Overlay::None);
        assert!(effects.is_empty());
    }

    #[test]
    fn geo_region_navigates() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 0;

        let _ = handle_geo_region(&mut model, key('j'));
        assert_eq!(model.geo_region_selected, 1);
        let _ = handle_geo_region(&mut model, key('j'));
        assert_eq!(model.geo_region_selected, 2);
        let _ = handle_geo_region(&mut model, key('j'));
        assert_eq!(model.geo_region_selected, 3);
        let _ = handle_geo_region(&mut model, key('j'));
        assert_eq!(model.geo_region_selected, 3); // clamp

        let _ = handle_geo_region(&mut model, key('k'));
        assert_eq!(model.geo_region_selected, 2);
        let _ = handle_geo_region(&mut model, key('g'));
        assert_eq!(model.geo_region_selected, 0);
        let _ = handle_geo_region(&mut model, key('G'));
        assert_eq!(model.geo_region_selected, 3);
    }

    #[test]
    fn geo_region_enter_changes_region() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 1; // Cn

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Cn)
        );
        assert_eq!(model.overlay, Overlay::None);
        assert!(model.logs.iter().any(|l| l.contains("Geo region: cn")));
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                Effect::RefreshGeoLastUpdated,
                app_log_info("Geo region: cn"),
                app_log_info("Checking geo databases..."),
                Effect::DownloadGeoIfMissing,
            ]
        );
    }

    #[test]
    fn geo_region_esc_blocked_when_none() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::GeoRegions;

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Esc));
        assert_eq!(model.overlay, Overlay::GeoRegions);
        assert!(effects.is_empty());
    }

    #[test]
    fn geo_region_esc_allowed_when_some() {
        let mut model = model_with_profiles(vec![]);
        model.overlay = Overlay::GeoRegions;
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Esc));
        assert_eq!(model.overlay, Overlay::None);
        assert!(effects.is_empty());
    }

    #[test]
    fn geo_region_change_resets_incompatible_routing_mode() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model
            .config
            .settings
            .geo_routing
            .set_mode(RoutingMode::OnlyRu);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 3; // Global

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Global)
        );
        assert_eq!(
            model.config.settings.geo_routing.mode(),
            RoutingMode::Global
        );
        assert_eq!(
            model
                .config
                .settings
                .geo_routing
                .selected_region_modes
                .get(&GeoRegion::Ru)
                .copied()
                .unwrap_or(RoutingMode::Global),
            RoutingMode::OnlyRu,
            "previous region's routing mode should be preserved"
        );
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                Effect::RefreshGeoLastUpdated,
                app_log_info("Geo region: global"),
                app_log_info("Routing mode: Global")
            ]
        );
    }

    #[test]
    fn routing_mode_persists_per_region() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model
            .config
            .settings
            .geo_routing
            .set_mode(RoutingMode::BypassRu);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 1; // Cn

        // Switch to Cn: routing mode falls back to Global.
        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Cn)
        );
        assert_eq!(
            model.config.settings.geo_routing.mode(),
            RoutingMode::Global
        );
        assert!(effects.contains(&Effect::DownloadGeoIfMissing));

        // Switch back to Ru: routing mode is restored to BypassRu.
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 0; // Ru
        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Ru)
        );
        assert_eq!(
            model.config.settings.geo_routing.mode(),
            RoutingMode::BypassRu
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::AppendAppLog { message, .. } if message.contains("Routing mode: Bypass RU"))));
    }

    #[test]
    fn routing_mode_change_is_stored_per_region() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model.overlay = Overlay::RoutingMode;
        model.routing_selected = 1; // BypassRu

        handle_routing_mode(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.mode(),
            RoutingMode::BypassRu
        );
        assert_eq!(
            model
                .config
                .settings
                .geo_routing
                .selected_region_modes
                .get(&GeoRegion::Ru)
                .copied()
                .unwrap_or(RoutingMode::Global),
            RoutingMode::BypassRu
        );
    }

    #[test]
    fn geo_region_triggers_auto_connect_after_selection() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Auto".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        let id = model.config.profiles[0].id;
        model.config.settings.auto_connect = true;
        model.config.settings.last_connected_profile = Some(id);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 0; // Ru

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Ru)
        );
        assert_eq!(model.connection, ConnectionState::Connecting);
        assert_eq!(model.selected, 0);
        assert!(model.status.text().contains("Auto-connecting"));
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                Effect::RefreshGeoLastUpdated,
                app_log_info("Geo region: ru"),
                app_log_info("Checking geo databases..."),
                Effect::DownloadGeoIfMissing,
                app_log_info("Auto-connecting to Auto…")
            ]
        );
    }

    #[test]
    fn geo_region_same_region_does_not_refresh_last_updated() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Cn);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 1; // Cn

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Cn)
        );
        assert!(!effects.contains(&Effect::RefreshGeoLastUpdated));
    }

    #[test]
    fn geo_region_global_does_not_trigger_geo_download() {
        let mut model = model_with_profiles(vec![]);
        model.config.settings.geo_routing.set_region(GeoRegion::Ru);
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 3; // Global

        let effects = handle_geo_region(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            model.config.settings.geo_routing.current_region,
            Some(GeoRegion::Global)
        );
        assert!(!effects.contains(&Effect::DownloadGeoIfMissing));
        assert!(!model.status.text().contains("Checking geo databases"));
    }

    #[test]
    fn geo_last_updated_message_updates_model() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = None;
        let effects = update(
            &mut model,
            Msg::GeoLastUpdated(Some("2026-06-15 08:00".to_string())),
        );
        assert_eq!(model.geo_last_updated, Some("2026-06-15 08:00".to_string()));
        assert_eq!(effects, vec![Effect::BroadcastState]);
    }

    #[test]
    fn normal_mode_u_blocked_for_global_region() {
        let mut model = model_with_profiles(vec![]);
        model
            .config
            .settings
            .geo_routing
            .set_region(GeoRegion::Global);

        let effects = handle_sources(&mut model, key('u'));
        assert!(!model.geo_updating);
        assert!(!effects.contains(&Effect::DownloadGeo));
        assert!(model.status.text().contains("not available"));
    }

    #[test]
    fn geo_result_updated_broadcasts_state() {
        let mut model = model_with_profiles(vec![]);
        model.geo_updating = true;
        let effects = update(
            &mut model,
            Msg::GeoUpdated(GeoResult::Updated {
                parts: vec!["geoip".into()],
                last_updated: Some("2026-05-31 13:41".to_string()),
            }),
        );
        assert!(!model.geo_updating);
        assert_eq!(
            effects,
            vec![
                app_log_info("Updated: geoip"),
                app_log_info("Geo databases updated"),
                Effect::BroadcastState
            ]
        );
    }

    #[test]
    fn geo_result_up_to_date_broadcasts_state() {
        let mut model = model_with_profiles(vec![]);
        model.geo_updating = true;
        let effects = update(&mut model, Msg::GeoUpdated(GeoResult::UpToDate));
        assert!(!model.geo_updating);
        assert_eq!(
            effects,
            vec![
                app_log_info("Geo databases are up to date"),
                Effect::BroadcastState
            ]
        );
    }

    #[test]
    fn geo_result_error_broadcasts_state() {
        let mut model = model_with_profiles(vec![]);
        model.geo_updating = true;
        let effects = update(
            &mut model,
            Msg::GeoUpdated(GeoResult::Error("net fail".into())),
        );
        assert!(!model.geo_updating);
        assert_eq!(
            effects,
            vec![app_log_error("net fail"), Effect::BroadcastState]
        );
    }

    #[test]
    fn tick_idle_fallback_broadcasts_state() {
        let mut model = Model::test_new(crate::config::profile::Config::default());
        model.connection = ConnectionState::Connecting;
        let effects = handle_tick(&mut model);
        assert_eq!(model.connection, ConnectionState::Idle);
        assert_eq!(effects, vec![Effect::BroadcastState]);
    }

    #[test]
    fn connected_mode_s_disconnects() {
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::Connected;
        model.overlay = Overlay::None;
        let effects = handle_key(&mut model, key('s'));
        assert_eq!(effects, vec![Effect::Disconnect]);
    }

    #[test]
    fn connected_mode_navigates() {
        let mut model = model_with_profiles(vec![
            Profile::new(
                "A".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "B".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ]);
        model.connection = ConnectionState::Connected;
        model.overlay = Overlay::None;
        assert_eq!(model.selected, 0);
        let _ = handle_key(&mut model, key('j'));
        assert_eq!(model.selected, 1);
        let _ = handle_key(&mut model, key('k'));
        assert_eq!(model.selected, 0);
        let _ = handle_key(&mut model, key('G'));
        assert_eq!(model.selected, 1);
        let _ = handle_key(&mut model, key('g'));
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn connected_mode_enter_connects() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::Connected;
        model.overlay = Overlay::None;
        let effects = handle_key(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(effects, vec![app_log_info("Connecting to A…")]);
        assert_eq!(model.connection, ConnectionState::Connecting);
    }

    #[test]
    fn connected_mode_r_reconnects() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::Connected;
        model.overlay = Overlay::None;
        let effects = handle_key(&mut model, key('r'));
        assert_eq!(effects, vec![app_log_info("Reconnecting to A…")]);
        assert_eq!(model.connection, ConnectionState::Connecting);
    }

    #[test]
    fn connected_mode_help() {
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::Connected;
        model.overlay = Overlay::None;
        let effects = handle_key(&mut model, key('?'));
        assert!(effects.is_empty());
        assert_eq!(model.overlay, Overlay::Help);
    }

    #[test]
    fn connect_failed_sets_error_mode() {
        let mut model = Model::test_new(crate::config::profile::Config::default());
        let effects = update(&mut model, Msg::ConnectFailed("timeout".into()));
        assert_eq!(model.overlay, Overlay::Error);
        assert_eq!(model.connection, ConnectionState::Idle);
        assert_eq!(
            effects,
            vec![
                Effect::BroadcastState,
                app_log_error("Connection failed: timeout")
            ]
        );
    }

    #[test]
    fn handle_tick_skips_connect_when_pending() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::ConnectPending;
        let effects = handle_tick(&mut model);
        assert!(effects.iter().all(|e| !matches!(e, Effect::Connect { .. })));
    }

    #[test]
    fn connected_clears_pending() {
        let mut model = Model::test_new(crate::config::profile::Config::default());
        model.connection = ConnectionState::ConnectPending;
        let effects = update(&mut model, Msg::Connected { pid: 12345 });
        assert_eq!(model.connection, ConnectionState::Connected);
        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(effects, vec![Effect::WriteState]);
    }

    #[test]
    fn connected_saves_last_profile() {
        let mut model = model_with_profiles(vec![Profile::new(
            "A".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::ConnectPending;
        let effects = update(&mut model, Msg::Connected { pid: 12345 });
        assert_eq!(model.connection, ConnectionState::Connected);
        assert_eq!(
            model.config.settings.last_connected_profile,
            Some(model.config.profiles[0].id)
        );
        assert_eq!(
            effects,
            vec![
                Effect::WriteState,
                app_log_info("Connected to A"),
                Effect::SaveConfig
            ]
        );
    }

    #[test]
    fn toggle_auto_connect() {
        let mut model = model_with_profiles(vec![]);
        assert!(!model.config.settings.auto_connect);
        let effects = handle_sources(&mut model, key('a'));
        assert!(model.config.settings.auto_connect);
        assert!(model.status.text().contains("enabled"));
        assert_eq!(
            effects,
            vec![app_log_info("Auto-connect enabled"), Effect::SaveConfig]
        );

        let effects = handle_sources(&mut model, key('a'));
        assert!(!model.config.settings.auto_connect);
        assert!(model.status.text().contains("disabled"));
        assert_eq!(
            effects,
            vec![app_log_info("Auto-connect disabled"), Effect::SaveConfig]
        );
    }

    #[test]
    fn toggle_kill_switch_emits_apply_effect_and_does_not_flip_bool() {
        let mut model = model_with_profiles(vec![]);
        assert!(!model.config.settings.kill_switch);

        let effects = handle_sources(&mut model, key('K'));
        // Bool is NOT flipped synchronously — it waits for KillSwitchApplied.
        assert!(!model.config.settings.kill_switch);
        assert!(model.status.text().contains("enabling"));
        assert_eq!(
            effects,
            vec![
                app_log_info("Kill switch enabling…"),
                Effect::ApplyKillSwitch { enabled: true },
            ]
        );
    }

    #[test]
    fn kill_switch_applied_success_flips_and_saves() {
        let mut model = model_with_profiles(vec![]);
        let effects = update(
            &mut model,
            Msg::KillSwitchApplied {
                enabled: true,
                error: None,
            },
        );
        assert!(model.config.settings.kill_switch);
        assert!(model.status.text().contains("enabled"));
        assert_eq!(
            effects,
            vec![
                app_log_info("Kill switch enabled"),
                Effect::SaveConfig,
                Effect::BroadcastState,
            ]
        );
    }

    #[test]
    fn kill_switch_applied_error_keeps_bool_unchanged() {
        let mut model = model_with_profiles(vec![]);
        let effects = update(
            &mut model,
            Msg::KillSwitchApplied {
                enabled: true,
                error: Some("helper missing".into()),
            },
        );
        assert!(!model.config.settings.kill_switch);
        assert!(model.status.text().contains("helper missing"));
        assert!(!effects.iter().any(|e| matches!(e, Effect::SaveConfig)));
    }

    #[test]
    fn paste_duplicate_profile_shows_error() {
        let mut model = model_with_profiles(vec![]);
        let uri = "vless://671c62c7-6768-4b98-ac6b-572c9c707be0@203.0.113.42:443#Test";

        // First paste succeeds
        let effects = handle_clipboard_text(&mut model, uri);
        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(
            effects,
            vec![Effect::SaveConfig, app_log_info("Pasted profile: Test")]
        );
        assert!(model.status.text().contains("Pasted profile"));

        // Second paste with same UUID fails
        let effects = handle_clipboard_text(&mut model, uri);
        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(effects, vec![app_log_error("Profile already exists")]);
        assert!(model.status.is_error());
        assert!(model.status.text().contains("already exists"));
    }

    #[test]
    fn paste_subscription_url_creates_subscription_and_fetches() {
        let mut model = model_with_profiles(vec![]);
        let url = "http://31.58.134.29:2096/sub/xrkjeq2mhwes0i8f";

        let effects = handle_clipboard_text(&mut model, url);

        assert_eq!(model.config.subscriptions.len(), 1);
        assert_eq!(model.config.subscriptions[0].url, url);
        assert!(matches!(
            model.selected_row(),
            Some(crate::app::model::SourceRow::SubscriptionHeader(0))
        ));
        assert!(model.subscription_fetching);
        assert!(
            model
                .subscription_updates
                .contains(&model.config.subscriptions[0].id)
        );
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                Effect::UpdateSubscription {
                    id: model.config.subscriptions[0].id
                },
                app_log_info("Added subscription '31.58.134.29' and fetching profiles…")
            ]
        );
    }

    #[test]
    fn paste_vless_adds_standalone_profile() {
        let mut model = model_with_profiles(vec![]);
        let uri = "vless://671c62c7-6768-4b98-ac6b-572c9c707be0@203.0.113.42:443#Test";

        let effects = handle_clipboard_text(&mut model, uri);

        assert_eq!(model.config.profiles.len(), 1);
        assert!(matches!(
            model.selected_row(),
            Some(crate::app::model::SourceRow::StandaloneProfile(0))
        ));
        assert_eq!(
            effects,
            vec![Effect::SaveConfig, app_log_info("Pasted profile: Test")]
        );
    }

    #[test]
    fn subscription_fetched_adds_profiles_and_saves() {
        let mut model = model_with_profiles(vec![]);
        let profiles = vec![
            Profile::new(
                "Sub1".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "Sub2".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ];

        let effects = handle_subscription_result(&mut model, Uuid::nil(), Ok(profiles));

        assert!(!model.subscription_fetching);
        assert_eq!(model.config.profiles.len(), 2);
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("Imported 2 profile(s) from subscription")
            ]
        );
    }

    #[test]
    fn subscription_fetched_updates_standalone_duplicate() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Existing".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        let profiles = vec![
            Profile::new(
                "Existing".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "New".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ];

        let effects = handle_subscription_result(&mut model, Uuid::nil(), Ok(profiles));

        assert_eq!(model.config.profiles.len(), 2);
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("Imported 2 profile(s) from subscription")
            ]
        );
    }

    #[test]
    fn subscription_fetched_skips_duplicate_from_other_subscription() {
        let other_sub_id = Uuid::new_v4();
        let mut existing = Profile::new(
            "Existing".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        );
        existing.subscription_id = Some(other_sub_id);
        let mut model = model_with_profiles(vec![existing]);
        model.config.subscriptions.push(Subscription {
            id: other_sub_id,
            name: "Other".to_string(),
            url: "http://example.com/other".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });

        let new_sub_id = Uuid::new_v4();
        model.config.subscriptions.push(Subscription {
            id: new_sub_id,
            name: "New".to_string(),
            url: "http://example.com/new".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });

        let fetched = Profile::new(
            "Existing".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        );

        let effects = handle_subscription_result(&mut model, new_sub_id, Ok(vec![fetched]));

        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(model.config.profiles[0].subscription_id, Some(other_sub_id));
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("No new profiles in subscription")
            ]
        );
    }

    #[test]
    fn subscription_fetched_attaches_standalone_duplicate() {
        let sub_id = Uuid::new_v4();
        let standalone = Profile::new(
            "OldName".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        );
        let standalone_id = standalone.id;
        let mut model = model_with_profiles(vec![standalone]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });

        let mut fetched = Profile::new(
            "NewName".to_string(),
            Protocol::Vless,
            "2.2.2.2".to_string(),
            443,
            "u1".to_string(),
        );
        // Different id from parse, same uuid as the standalone profile.
        fetched.id = Uuid::new_v4();

        let effects = handle_subscription_result(&mut model, sub_id, Ok(vec![fetched]));

        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(model.config.profiles[0].id, standalone_id);
        assert_eq!(model.config.profiles[0].name, "NewName");
        assert_eq!(model.config.profiles[0].address, "2.2.2.2");
        assert_eq!(model.config.profiles[0].subscription_id, Some(sub_id));
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("Imported 1 profile(s) from subscription")
            ]
        );
    }

    #[test]
    fn subscription_fetched_empty_logs_no_new_profiles() {
        let mut model = model_with_profiles(vec![]);

        let effects = handle_subscription_result(&mut model, Uuid::nil(), Ok(vec![]));

        assert_eq!(model.config.profiles.len(), 0);
        assert_eq!(
            effects,
            vec![app_log_info("No new profiles in subscription")]
        );
    }

    #[test]
    fn subscription_fetched_error_logs_failure() {
        let mut model = model_with_profiles(vec![]);

        let effects =
            handle_subscription_result(&mut model, Uuid::nil(), Err("network down".to_string()));

        assert!(!model.subscription_fetching);
        assert_eq!(
            effects,
            vec![app_log_error("Subscription failed: network down")]
        );
    }

    #[test]
    fn subscription_fetched_managed_replaces_profiles_and_updates_last_updated() {
        let sub_id = Uuid::new_v4();
        let mut existing = Profile::new(
            "Old".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        );
        existing.subscription_id = Some(sub_id);
        let mut model = model_with_profiles(vec![existing]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });

        let new_profiles = vec![Profile::new(
            "New".to_string(),
            Protocol::Vless,
            "2.2.2.2".to_string(),
            443,
            "u2".to_string(),
        )];

        let effects = handle_subscription_result(&mut model, sub_id, Ok(new_profiles));

        assert_eq!(model.config.profiles.len(), 1);
        assert_eq!(model.config.profiles[0].name, "New");
        assert_eq!(model.config.profiles[0].subscription_id, Some(sub_id));
        assert!(
            model
                .config
                .subscriptions
                .iter()
                .find(|s| s.id == sub_id)
                .unwrap()
                .last_updated
                .is_some()
        );
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("Imported 1 profile(s) from subscription")
            ]
        );
    }

    #[test]
    fn subscriptions_update_triggers_fetch() {
        let mut model = model_with_profiles(vec![]);
        let sub_id = Uuid::new_v4();
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        model.selected = 0;

        let effects = handle_sources(&mut model, key('u'));

        assert!(model.subscription_fetching);
        assert!(model.subscription_updates.contains(&sub_id));
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                Effect::UpdateSubscription { id: sub_id },
                app_log_info("Updating subscription 'Sub'…"),
            ]
        );
    }

    #[test]
    fn subscription_interval_shows_status() {
        use crate::config::profile::Subscription;
        use uuid::Uuid;

        let sub_id = Uuid::new_v4();
        let mut model = model_with_profiles(vec![]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        model.selected = 0;
        let effects = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Every12h
        );
        assert!(effects.contains(&Effect::SaveConfig));
        assert!(effects.contains(&app_log_info("Subscription 'Sub' [🗘 12h]")));
    }

    #[test]
    fn subscriptions_interval_cycles() {
        let mut model = model_with_profiles(vec![]);
        model.config.subscriptions.push(Subscription {
            id: Uuid::new_v4(),
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Off,
            last_updated: None,
        });
        model.selected = 0;

        let _ = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Every1h
        );
        let _ = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Every12h
        );
        let _ = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Every1d
        );
        let _ = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Every7d
        );
        let _ = handle_sources(&mut model, KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(
            model.config.subscriptions[0].auto_update,
            SubscriptionAutoUpdate::Off
        );
    }

    #[test]
    fn confirm_delete_subscription_removes_subscription_and_profiles() {
        let sub_id = Uuid::new_v4();
        let mut existing = Profile::new(
            "Old".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        );
        existing.subscription_id = Some(sub_id);
        let mut model = model_with_profiles(vec![existing]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        // source_rows: [SubscriptionHeader(0), SubscriptionProfile { sub_idx: 0, profile_idx: 0 }]
        model.selected = 0;
        model.overlay = Overlay::ConfirmDelete;

        let effects = handle_confirm_delete(&mut model, KeyEvent::from(KeyCode::Enter));

        assert!(model.config.subscriptions.is_empty());
        assert!(model.config.profiles.is_empty());
        assert_eq!(model.selected, 0);
        assert_eq!(
            effects,
            vec![
                Effect::SaveConfig,
                app_log_info("Subscription 'Sub' deleted")
            ]
        );
    }

    #[test]
    fn due_subscriptions_are_queued_for_update() {
        let sub_id = Uuid::new_v4();
        let mut model = model_with_profiles(vec![]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: Some(chrono::Local::now() - chrono::Duration::try_hours(2).unwrap()),
        });

        let effects = check_due_subscriptions(&mut model);

        assert_eq!(effects, vec![Effect::UpdateSubscription { id: sub_id }]);
        assert!(model.subscription_updates.contains(&sub_id));
    }

    #[test]
    fn non_due_subscriptions_are_skipped() {
        let sub_id = Uuid::new_v4();
        let mut model = model_with_profiles(vec![]);
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Sub".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: Some(chrono::Local::now()),
        });

        let effects = check_due_subscriptions(&mut model);

        assert!(effects.is_empty());
        assert!(!model.subscription_updates.contains(&sub_id));
    }
}
