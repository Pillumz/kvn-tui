use crate::app::effect::Effect;
use crate::app::model::{AppStatus, ConnectionState, Model, Overlay, TrafficStats};
use crate::app::msg::{GeoResult, Msg};
use crate::config::profile::{GeoRegion, Profile, Subscription, SubscriptionAutoUpdate};
use chrono::Local;
use crossterm::event::KeyCode;
#[cfg(test)]
use crossterm::event::KeyEvent;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod key;

use key::{derive_subscription_name, handle_ipc_command, handle_key};
#[cfg(test)]
use key::{
    handle_confirm_delete, handle_geo_region, handle_routing_mode, handle_sources,
    rebuild_key_event,
};

/// Minimum interval between Clash-API scrapes. The daemon ticker fires every
/// 250 ms; we only emit `Effect::FetchTrafficStats` once per second.
const TRAFFIC_POLL_INTERVAL: Duration = Duration::from_secs(1);

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
            // Fresh sing-box → fresh counters. Drop the previous sample so the
            // first delta is computed against zero rather than a stale value.
            model.traffic = TrafficStats::default();
            model.last_traffic_sample_at_ms = 0;
            model.last_traffic_fetch_at = None;
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
            model.traffic = TrafficStats::default();
            model.last_traffic_sample_at_ms = 0;
            model.last_traffic_fetch_at = None;
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
        Msg::TrafficStatsUpdated {
            up_total,
            down_total,
            conn_count,
            sampled_at_ms,
        } => handle_traffic_stats_updated(model, up_total, down_total, conn_count, sampled_at_ms),
    }
}

/// Compute a per-second byte rate from two cumulative samples. Returns 0 when
/// no time has passed, when the counter went backwards (e.g. sing-box restart
/// reset its totals), or when there's no prior sample yet (`prev_at_ms == 0`).
pub(crate) fn compute_rate(prev_total: u64, curr_total: u64, elapsed_ms: u64) -> u64 {
    if elapsed_ms == 0 {
        return 0;
    }
    let delta = curr_total.saturating_sub(prev_total);
    // (delta bytes / elapsed ms) * 1000 — done in u128 to avoid overflow.
    ((delta as u128 * 1000) / elapsed_ms as u128) as u64
}

fn handle_traffic_stats_updated(
    model: &mut Model,
    up_total: u64,
    down_total: u64,
    conn_count: usize,
    sampled_at_ms: u64,
) -> Vec<Effect> {
    let prev_at_ms = model.last_traffic_sample_at_ms;
    // No prior sample yet — record totals but leave rates at zero. The next
    // tick produces the first instantaneous reading against this baseline.
    let (up_rate, down_rate) = if prev_at_ms == 0 {
        (0, 0)
    } else {
        let elapsed = sampled_at_ms.saturating_sub(prev_at_ms);
        (
            compute_rate(model.traffic.up_total, up_total, elapsed),
            compute_rate(model.traffic.down_total, down_total, elapsed),
        )
    };
    model.traffic = TrafficStats {
        up_rate_bps: up_rate,
        down_rate_bps: down_rate,
        up_total,
        down_total,
        conn_count,
    };
    model.last_traffic_sample_at_ms = sampled_at_ms;
    vec![Effect::BroadcastState]
}

fn handle_kill_switch_applied(
    model: &mut Model,
    enabled: bool,
    error: Option<crate::app::msg::IpcError>,
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
    result: Result<crate::config::profile::Config, crate::app::msg::IpcError>,
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

    // Throttled Clash-API poll for live traffic stats.
    if model.connection == ConnectionState::Connected {
        let now = Instant::now();
        let due = match model.last_traffic_fetch_at {
            None => true,
            Some(prev) => now.duration_since(prev) >= TRAFFIC_POLL_INTERVAL,
        };
        if due {
            model.last_traffic_fetch_at = Some(now);
            effects.push(Effect::FetchTrafficStats {
                prev_up_total: model.traffic.up_total,
                prev_down_total: model.traffic.down_total,
                prev_sampled_at_ms: model.last_traffic_sample_at_ms,
            });
        }
    }

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

fn handle_copied_status(model: &mut Model, name: String, count: usize) -> Vec<Effect> {
    let msg = if count <= 1 {
        format!("Copied: {name}")
    } else {
        format!("Copied {count} links from {name}")
    };
    let mut effects = Vec::new();
    push_status(&mut effects, model, crate::app::model::AppStatus::Info(msg));
    effects
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
    result: Result<Vec<Profile>, crate::app::msg::IpcError>,
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
                let key = profile.dedup_key();
                if let Some(idx) = model
                    .config
                    .profiles
                    .iter()
                    .position(|p| p.dedup_key() == key)
                {
                    let existing = &model.config.profiles[idx];
                    if existing.subscription_id.is_none() {
                        // Update the standalone profile in place and link it to
                        // the subscription, preserving its identity.
                        profile.id = existing.id;
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
            Profile::new_vless(
                "A".to_string(),
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new_vless(
                "B".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut profile = Profile::new_vless(
            "SubProfile".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
            Msg::ConfigReloaded(Err(crate::app::msg::IpcError::new("parse error"))),
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
            Profile::new_vless(
                "A".to_string(),
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new_vless(
                "B".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "Auto".to_string(),
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
            Profile::new_vless(
                "A".to_string(),
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new_vless(
                "B".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let effects = update(
            &mut model,
            Msg::ConnectFailed(crate::app::msg::IpcError::new("timeout")),
        );
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".to_string(),
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
                error: Some(crate::app::msg::IpcError::new("helper missing")),
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
            Profile::new_vless(
                "Sub1".to_string(),
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new_vless(
                "Sub2".to_string(),
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
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "Existing".to_string(),
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        let profiles = vec![
            Profile::new_vless(
                "Existing".to_string(),
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new_vless(
                "New".to_string(),
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
        let mut existing = Profile::new_vless(
            "Existing".to_string(),
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

        let fetched = Profile::new_vless(
            "Existing".to_string(),
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
        let standalone = Profile::new_vless(
            "OldName".to_string(),
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

        let mut fetched = Profile::new_vless(
            "NewName".to_string(),
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

        let effects = handle_subscription_result(
            &mut model,
            Uuid::nil(),
            Err(crate::app::msg::IpcError::new("network down")),
        );

        assert!(!model.subscription_fetching);
        assert_eq!(
            effects,
            vec![app_log_error("Subscription failed: network down")]
        );
    }

    #[test]
    fn subscription_fetched_managed_replaces_profiles_and_updates_last_updated() {
        let sub_id = Uuid::new_v4();
        let mut existing = Profile::new_vless(
            "Old".to_string(),
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

        let new_profiles = vec![Profile::new_vless(
            "New".to_string(),
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
        let mut existing = Profile::new_vless(
            "Old".to_string(),
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

    #[test]
    fn compute_rate_zero_elapsed_returns_zero() {
        assert_eq!(compute_rate(0, 1_000_000, 0), 0);
        assert_eq!(compute_rate(100, 1_000, 0), 0);
    }

    #[test]
    fn compute_rate_counter_rollback_saturates_to_zero() {
        // sing-box restarted mid-session — totals reset; saturating_sub.
        assert_eq!(compute_rate(10_000, 500, 1_000), 0);
    }

    #[test]
    fn compute_rate_basic() {
        // 2000 bytes delta over 1000 ms = 2000 B/s
        assert_eq!(compute_rate(1_000, 3_000, 1_000), 2_000);
        // 1500 bytes delta over 500 ms = 3000 B/s
        assert_eq!(compute_rate(0, 1_500, 500), 3_000);
    }

    #[test]
    fn compute_rate_fractional_second() {
        // 100 bytes over 250 ms = 400 B/s
        assert_eq!(compute_rate(0, 100, 250), 400);
    }

    #[test]
    fn traffic_stats_updated_first_sample_records_zero_rate() {
        let mut model = model_with_profiles(vec![]);
        let effects = handle_traffic_stats_updated(&mut model, 10_000, 50_000, 3, 1_000);
        assert_eq!(model.traffic.up_total, 10_000);
        assert_eq!(model.traffic.down_total, 50_000);
        assert_eq!(model.traffic.conn_count, 3);
        // First sample → no prior baseline → rates remain zero.
        assert_eq!(model.traffic.up_rate_bps, 0);
        assert_eq!(model.traffic.down_rate_bps, 0);
        assert_eq!(model.last_traffic_sample_at_ms, 1_000);
        assert_eq!(effects, vec![Effect::BroadcastState]);
    }

    #[test]
    fn traffic_stats_updated_second_sample_computes_rate() {
        let mut model = model_with_profiles(vec![]);
        // Prime with a first sample.
        handle_traffic_stats_updated(&mut model, 0, 0, 0, 1_000);
        // 5000 ↑ + 12000 ↓ over 1 second.
        let effects = handle_traffic_stats_updated(&mut model, 5_000, 12_000, 7, 2_000);
        assert_eq!(model.traffic.up_rate_bps, 5_000);
        assert_eq!(model.traffic.down_rate_bps, 12_000);
        assert_eq!(model.traffic.conn_count, 7);
        assert_eq!(effects, vec![Effect::BroadcastState]);
    }

    #[test]
    fn traffic_stats_updated_handles_singbox_restart() {
        let mut model = model_with_profiles(vec![]);
        handle_traffic_stats_updated(&mut model, 10_000, 50_000, 5, 1_000);
        // sing-box restarts → counters reset, but we still get a sample.
        handle_traffic_stats_updated(&mut model, 100, 200, 1, 2_000);
        // Rate saturates to 0 for this transition sample.
        assert_eq!(model.traffic.up_rate_bps, 0);
        assert_eq!(model.traffic.down_rate_bps, 0);
        // Totals reflect the new (lower) baseline.
        assert_eq!(model.traffic.up_total, 100);
        assert_eq!(model.traffic.down_total, 200);
    }

    #[test]
    fn handle_tick_emits_fetch_when_connected() {
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::Connected;
        model.last_traffic_fetch_at = None;
        let effects = handle_tick(&mut model);
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::FetchTrafficStats {
                    prev_up_total: 0,
                    prev_down_total: 0,
                    prev_sampled_at_ms: 0,
                }
            )),
            "expected Effect::FetchTrafficStats, got {:?}",
            effects
        );
        assert!(model.last_traffic_fetch_at.is_some());
    }

    #[test]
    fn handle_tick_skips_fetch_when_idle() {
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::Idle;
        let effects = handle_tick(&mut model);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::FetchTrafficStats { .. })),
            "Idle connection must not poll Clash API"
        );
    }

    #[test]
    fn handle_tick_throttles_within_one_second() {
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::Connected;
        // First tick → fetch emitted.
        let _ = handle_tick(&mut model);
        let first_at = model.last_traffic_fetch_at;
        // Second tick almost immediately → no additional fetch.
        let effects = handle_tick(&mut model);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::FetchTrafficStats { .. })),
            "second tick within 1s must not emit FetchTrafficStats"
        );
        assert_eq!(model.last_traffic_fetch_at, first_at);
    }

    #[test]
    fn connected_resets_traffic_state() {
        let mut model = model_with_profiles(vec![Profile::new_vless(
            "A".into(),
            "1.1.1.1".into(),
            443,
            "u".into(),
        )]);
        model.traffic.up_total = 9999;
        model.traffic.down_total = 8888;
        model.traffic.up_rate_bps = 100;
        model.last_traffic_sample_at_ms = 1_000;
        model.last_traffic_fetch_at = Some(Instant::now());
        let _ = update(&mut model, Msg::Connected { pid: 1234 });
        assert_eq!(model.traffic, TrafficStats::default());
        assert_eq!(model.last_traffic_sample_at_ms, 0);
        assert!(model.last_traffic_fetch_at.is_none());
    }
}
