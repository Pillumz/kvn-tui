use crate::app::effect::Effect;
use crate::app::model::{AppStatus, ConnectionState, Model, Overlay};
use crate::app::msg::{GeoResult, Msg};
use crate::config::profile::GeoRegion;
#[cfg(test)]
use crate::config::profile::{Profile, Protocol};
use crossterm::event::{KeyCode, KeyEvent};

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
                let log_effect = set_status(
                    model,
                    crate::app::model::AppStatus::Info("Resumed — reconnecting…".into()),
                );
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
                if let Some(e) = log_effect {
                    effects.push(e);
                }
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
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!("Connected to {}", profile_name)),
                ) {
                    effects.push(e);
                }
                // Persist last connected profile for auto-connect on next startup.
                if model.config.settings.last_connected_profile != Some(profile_id) {
                    model.config.settings.last_connected_profile = Some(profile_id);
                    effects.push(Effect::SaveConfig);
                }
            }
            effects
        }
        Msg::ConnectFailed(err) => {
            model.connection = ConnectionState::Idle;
            model.overlay = Overlay::Error;
            let log_effect = set_status(
                model,
                crate::app::model::AppStatus::Error(format!("Connection failed: {}", err)),
            );
            let mut effects = vec![Effect::BroadcastState];
            if let Some(e) = log_effect {
                effects.push(e);
            }
            effects
        }

        Msg::Resize => {
            model.needs_redraw = true;
            vec![]
        }
        Msg::IpcCommand(cmd) => handle_ipc_command(model, cmd),
        Msg::StateUpdate(_) => vec![],
        Msg::ConfigReloaded(result) => handle_config_reloaded(model, result),
    }
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

fn handle_config_reloaded(
    model: &mut Model,
    result: Result<crate::config::profile::Config, String>,
) -> Vec<Effect> {
    match result {
        Ok(config) => {
            model.selected = config.resolve_selected();
            model.config = config;
            let mut effects = vec![Effect::BroadcastState];
            if let Some(e) = set_status(model, AppStatus::Info("Profiles reloaded".into())) {
                effects.push(e);
            }
            effects
        }
        Err(e) => {
            let mut effects = vec![Effect::BroadcastState];
            if let Some(e) = set_status(model, AppStatus::Error(format!("Failed to reload: {}", e)))
            {
                effects.push(e);
            }
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

    effects
}

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match model.overlay {
        Overlay::None => handle_main(model, key),
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

fn handle_main(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let mut effects = Vec::new();
    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            model.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            model.select_prev();
        }
        KeyCode::Char('g') => {
            model.select_first();
        }
        KeyCode::Char('G') => {
            model.select_last();
        }

        // Actions
        KeyCode::Enter => {
            if let Some(profile) = model.selected_profile() {
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!("Connecting to {}…", profile.name)),
                ) {
                    effects.push(e);
                }
                model.connection = ConnectionState::Connecting;
            } else if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info(
                    "No profiles. Press p to paste or e to edit.".into(),
                ),
            ) {
                effects.push(e);
            }
        }
        KeyCode::Char('p') => {
            return vec![Effect::PasteClipboard];
        }
        KeyCode::Char('d') if model.selected_profile().is_some() => {
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
        KeyCode::Char('u') if !model.geo_updating => {
            if model.config.settings.geo_routing.current_region == Some(GeoRegion::Global) {
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(
                        "Geo updates are not available in Global region".to_string(),
                    ),
                ) {
                    effects.push(e);
                }
                return effects;
            }
            model.geo_updating = true;
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info("Checking for geo updates...".to_string()),
            ) {
                effects.push(e);
            }
            effects.push(Effect::DownloadGeo);
            return effects;
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
            return vec![Effect::OpenEditor(model.selected)];
        }
        KeyCode::Char('r') if model.connection == ConnectionState::Connected => {
            if let Some(profile) = model.selected_profile() {
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!(
                        "Reconnecting to {}…",
                        profile.name
                    )),
                ) {
                    effects.push(e);
                }
            }
            model.connection = ConnectionState::Connecting;
        }
        KeyCode::Char('s') if model.connection == ConnectionState::Connected => {
            return vec![Effect::Disconnect];
        }
        KeyCode::Char('a') => {
            let new_val = !model.config.settings.auto_connect;
            model.config.settings.auto_connect = new_val;
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info(format!(
                    "Auto-connect {}",
                    if new_val { "enabled" } else { "disabled" }
                )),
            ) {
                effects.push(e);
            }
            effects.push(Effect::SaveConfig);
            return effects;
        }

        // Help
        KeyCode::Char('?') => model.overlay = Overlay::Help,

        _ => {}
    }
    effects
}

fn handle_confirm_delete(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            let name = model.selected_profile().map(|p| p.name.clone());
            model.delete_selected();
            let mut effects = vec![Effect::SaveConfig];
            if let Some(name) = name {
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!("Profile '{}' deleted", name)),
                ) {
                    effects.push(e);
                }
            }
            return effects;
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            model.overlay = Overlay::None;
        }
        _ => {}
    }
    vec![]
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
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!("Routing mode: {}", mode.as_str())),
                ) {
                    effects.push(e);
                }

                if changed && model.connection == ConnectionState::Connected {
                    model.connection = ConnectionState::Connecting;
                    let text = format!("Mode changed to {} — reconnecting", mode.as_str());
                    if let Some(e) = set_status(model, crate::app::model::AppStatus::Info(text)) {
                        effects.push(e);
                    }
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
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Info(format!("Geo region: {}", region.as_str())),
                ) {
                    effects.push(e);
                }

                // If the region changed and is not Global, check whether geo databases
                // are present and download them automatically if they are missing.
                if changed && region != GeoRegion::Global {
                    model.geo_updating = true;
                    if let Some(e) = set_status(
                        model,
                        crate::app::model::AppStatus::Info("Checking geo databases...".to_string()),
                    ) {
                        effects.push(e);
                    }
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
                        if let Some(e) = set_status(
                            model,
                            crate::app::model::AppStatus::Info(format!(
                                "Routing mode: {}",
                                new_mode.as_str()
                            )),
                        ) {
                            effects.push(e);
                        }
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
                        model.selected = idx;
                        model.connection = ConnectionState::Connecting;
                        if let Some(profile) = model.config.profiles.get(idx) {
                            if let Some(e) = set_status(
                                model,
                                crate::app::model::AppStatus::Info(format!(
                                    "Auto-connecting to {}…",
                                    profile.name
                                )),
                            ) {
                                effects.push(e);
                            }
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
    match crate::config::profile::parse_share_link(text) {
        Ok(profile) => {
            if model.has_duplicate(&profile) {
                let mut effects = Vec::new();
                if let Some(e) = set_status(
                    model,
                    crate::app::model::AppStatus::Error("Profile already exists".into()),
                ) {
                    effects.push(e);
                }
                return effects;
            }
            let name = profile.name.clone();
            model.add_profile(profile);
            let mut effects = vec![Effect::SaveConfig];
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info(format!("Pasted profile: {}", name)),
            ) {
                effects.push(e);
            }
            effects
        }
        Err(e) => {
            let mut effects = Vec::new();
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Error(format!("Invalid URI: {}", e)),
            ) {
                effects.push(e);
            }
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
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info("Geo databases updated".into()),
            ) {
                log_effects.push(e);
            }
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
            if let Some(e) = set_status(
                model,
                crate::app::model::AppStatus::Info("Geo databases are up to date".into()),
            ) {
                effects.push(e);
            }
            effects
        }
        GeoResult::Error(err) => {
            let mut effects = Vec::new();
            if let Some(e) = set_status(model, crate::app::model::AppStatus::Error(err)) {
                effects.push(e);
            }
            effects
        }
    };
    effects.push(Effect::BroadcastState);
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::RoutingMode;
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
        let _ = handle_main(&mut model, key('j'));
        assert_eq!(model.selected, 1);
        let _ = handle_main(&mut model, key('k'));
        assert_eq!(model.selected, 0);
        let _ = handle_main(&mut model, key('G'));
        assert_eq!(model.selected, 1);
        let _ = handle_main(&mut model, key('g'));
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
        let effects = handle_main(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(model.connection, ConnectionState::Connecting);
        assert_eq!(effects, vec![app_log_info("Connecting to A…")]);
    }

    #[test]
    fn normal_mode_enter_no_profile() {
        let mut model = model_with_profiles(vec![]);
        let effects = handle_main(&mut model, KeyEvent::from(KeyCode::Enter));
        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(
            effects,
            vec![app_log_info("No profiles. Press p to paste or e to edit.")]
        );
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
        let effects = handle_main(&mut model, key('d'));
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
        let effects = handle_main(&mut model, key('m'));
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

        let effects = handle_main(&mut model, key('u'));
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
        let effects = handle_main(&mut model, key('a'));
        assert!(model.config.settings.auto_connect);
        assert!(model.status.text().contains("enabled"));
        assert_eq!(
            effects,
            vec![app_log_info("Auto-connect enabled"), Effect::SaveConfig]
        );

        let effects = handle_main(&mut model, key('a'));
        assert!(!model.config.settings.auto_connect);
        assert!(model.status.text().contains("disabled"));
        assert_eq!(
            effects,
            vec![app_log_info("Auto-connect disabled"), Effect::SaveConfig]
        );
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
}
