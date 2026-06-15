use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;
use uuid::Uuid;

/// Supported VPN protocols.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Vless => write!(f, "vless"),
        }
    }
}

/// Selected geo region for rule-set downloads and routing mode availability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GeoRegion {
    #[serde(alias = "other")]
    Global,
    Ru,
    Cn,
    Ir,
}

impl GeoRegion {
    pub fn as_str(&self) -> &'static str {
        match self {
            GeoRegion::Global => "global",
            GeoRegion::Ru => "ru",
            GeoRegion::Cn => "cn",
            GeoRegion::Ir => "ir",
        }
    }
}

/// Routing mode for geoip/geosite rules.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    #[default]
    Global,
    BypassRu,
    OnlyRu,
    BypassCn,
    OnlyCn,
    BypassIr,
    OnlyIr,
}

impl RoutingMode {
    /// Return the list of routing modes available for the given geo region.
    pub fn available(region: Option<GeoRegion>) -> Vec<RoutingMode> {
        match region {
            Some(GeoRegion::Ru) => vec![
                RoutingMode::Global,
                RoutingMode::BypassRu,
                RoutingMode::OnlyRu,
            ],
            Some(GeoRegion::Cn) => vec![
                RoutingMode::Global,
                RoutingMode::BypassCn,
                RoutingMode::OnlyCn,
            ],
            Some(GeoRegion::Ir) => vec![
                RoutingMode::Global,
                RoutingMode::BypassIr,
                RoutingMode::OnlyIr,
            ],
            Some(GeoRegion::Global) | None => vec![RoutingMode::Global],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RoutingMode::Global => "Global",
            RoutingMode::BypassRu => "Bypass RU",
            RoutingMode::OnlyRu => "Only RU",
            RoutingMode::BypassCn => "Bypass CN",
            RoutingMode::OnlyCn => "Only CN",
            RoutingMode::BypassIr => "Bypass IR",
            RoutingMode::OnlyIr => "Only IR",
        }
    }
}

/// REALITY security settings for XTLS Vision.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealitySettings {
    #[serde(rename = "public_key")]
    pub public_key: String,
    #[serde(rename = "short_id")]
    pub short_id: String,
    #[serde(rename = "server_name")]
    pub server_name: String,
    #[serde(rename = "spider_x")]
    pub spider_x: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    #[default]
    None,
    Reality,
    Tls,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Grpc,
    Ws,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Flow {
    #[default]
    None,
    #[serde(rename = "xtls-rprx-vision")]
    XtlsRprxVision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DnsStrategy {
    #[default]
    #[serde(rename = "prefer_ipv4")]
    PreferIpv4,
    #[serde(rename = "prefer_ipv6")]
    PreferIpv6,
    #[serde(rename = "ipv4_only")]
    OnlyIpv4,
    #[serde(rename = "ipv6_only")]
    OnlyIpv6,
}

/// Single VPN profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    pub protocol: Protocol,
    pub address: String,
    pub port: u16,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<Flow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Security>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reality: Option<RealitySettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_type: Option<TransportType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Profile {
    /// Create a new profile with a generated UUID.
    pub fn new(name: String, protocol: Protocol, address: String, port: u16, uuid: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            protocol,
            address,
            port,
            uuid,
            flow: None,
            security: None,
            reality: None,
            transport_type: None,
            transport_service_name: None,
            fingerprint: None,
            tags: Vec::new(),
        }
    }
}

/// Geo-region and routing-mode preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GeoRouting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_region: Option<GeoRegion>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub selected_region_modes: HashMap<GeoRegion, RoutingMode>,
}

impl GeoRouting {
    /// Return the active routing mode for the current region.
    /// Falls back to `Global` when no region is selected or no mode is stored.
    pub fn mode(&self) -> RoutingMode {
        self.current_region
            .and_then(|r| self.selected_region_modes.get(&r).copied())
            .unwrap_or(RoutingMode::Global)
    }

    /// Change the active geo region.
    pub fn set_region(&mut self, region: GeoRegion) {
        self.current_region = Some(region);
    }

    /// Store the routing mode for the current region.
    pub fn set_mode(&mut self, mode: RoutingMode) {
        if let Some(region) = self.current_region {
            self.selected_region_modes.insert(region, mode);
        }
    }

    /// Return routing modes available for the current region.
    pub fn available_modes(&self) -> Vec<RoutingMode> {
        RoutingMode::available(self.current_region)
    }
}

/// Helper struct used only to deserialize `Settings` while accepting the
/// v0.11.2 legacy fields `geo_region` and `routing_mode`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsRaw {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_profile: Option<Uuid>,
    #[serde(default = "default_tun_interface")]
    tun_interface: String,
    #[serde(default = "default_dns_strategy")]
    dns_strategy: DnsStrategy,
    #[serde(default)]
    geo_routing: GeoRouting,
    #[serde(default)]
    auto_connect: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_connected_profile: Option<Uuid>,

    // Legacy v0.11.2 fields.
    #[serde(default)]
    geo_region: Option<GeoRegion>,
    #[serde(default)]
    routing_mode: RoutingMode,
}

/// Application settings stored alongside profiles.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<Uuid>,
    #[serde(default = "default_tun_interface")]
    pub tun_interface: String,
    #[serde(default = "default_dns_strategy")]
    pub dns_strategy: DnsStrategy,
    #[serde(default)]
    pub geo_routing: GeoRouting,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected_profile: Option<Uuid>,

    // Legacy v0.11.2 fields kept only for deserializing old profiles.json.
    #[serde(skip_serializing)]
    pub(crate) geo_region: Option<GeoRegion>,
    #[serde(skip_serializing)]
    pub(crate) routing_mode: RoutingMode,
}

fn default_tun_interface() -> String {
    "tun0".to_string()
}

fn default_dns_strategy() -> DnsStrategy {
    DnsStrategy::PreferIpv4
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = SettingsRaw::deserialize(deserializer)?;
        let mut settings = Self {
            default_profile: raw.default_profile,
            tun_interface: raw.tun_interface,
            dns_strategy: raw.dns_strategy,
            geo_routing: raw.geo_routing,
            auto_connect: raw.auto_connect,
            last_connected_profile: raw.last_connected_profile,
            geo_region: raw.geo_region,
            routing_mode: raw.routing_mode,
        };
        settings.migrate_legacy_geo_routing();
        Ok(settings)
    }
}

impl Settings {
    /// Migrate legacy v0.11.2 geo/routing fields into `geo_routing`.
    fn migrate_legacy_geo_routing(&mut self) {
        if let Some(region) = self.geo_region {
            self.geo_routing.current_region = Some(region);
            self.geo_routing
                .selected_region_modes
                .insert(region, self.routing_mode);
        }
        self.geo_region = None;
        self.routing_mode = RoutingMode::default();
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_profile: None,
            tun_interface: default_tun_interface(),
            dns_strategy: default_dns_strategy(),
            geo_routing: GeoRouting::default(),
            auto_connect: false,
            last_connected_profile: None,
            geo_region: None,
            routing_mode: RoutingMode::default(),
        }
    }
}

/// Root configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub settings: Settings,
}

impl Config {
    /// Resolve the selected profile index from `settings.default_profile`.
    /// Returns the index of the default profile if it exists, otherwise `0`.
    pub fn resolve_selected(&self) -> usize {
        self.settings
            .default_profile
            .and_then(|id| self.profiles.iter().position(|p| p.id == id))
            .unwrap_or(0)
    }

    /// Validate semantic constraints that serde cannot enforce.
    ///
    /// Checks:
    /// - Each profile has non-empty `name`, `address`, and `uuid`.
    /// - `settings.default_profile` references an existing profile if set.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (idx, profile) in self.profiles.iter().enumerate() {
            let num = idx + 1;
            if profile.name.trim().is_empty() {
                anyhow::bail!("Profile {num}: name must not be empty");
            }
            if profile.address.trim().is_empty() {
                anyhow::bail!("Profile {num}: address must not be empty");
            }
            if profile.uuid.trim().is_empty() {
                anyhow::bail!("Profile {num}: uuid must not be empty");
            }
        }

        if let Some(id) = self.settings.default_profile {
            if !self.profiles.iter().any(|p| p.id == id) {
                anyhow::bail!("settings.default_profile ({id}) references a non-existent profile");
            }
        }

        Ok(())
    }
}

/// Parse a share link text into a Profile.
pub fn parse_share_link(text: &str) -> Result<Profile> {
    let trimmed = text.trim();

    if let Some(rest) = trimmed.strip_prefix("vless://") {
        parse_vless(rest)
    } else {
        anyhow::bail!("Unsupported share link format: only vless:// is supported")
    }
}

/// Parse a VLESS URI fragment.
fn parse_vless(rest: &str) -> Result<Profile> {
    let url = Url::parse(&format!("vless://{}", rest)).context("Invalid VLESS URL")?;

    let uuid = url.username().to_string();
    let host = url
        .host_str()
        .context("Missing host in VLESS URL")?
        .to_string();
    let port = url.port().unwrap_or(443);

    let mut profile = Profile::new(host.clone(), Protocol::Vless, host, port, uuid);

    // Extract fragment as profile name
    if let Some(fragment) = url.fragment() {
        profile.name = urlencoding::decode(fragment)?.to_string();
    }

    let query: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    if let Some(flow) = query.get("flow") {
        profile.flow = match flow.as_str() {
            "xtls-rprx-vision" => Some(Flow::XtlsRprxVision),
            _ => None,
        };
    }
    if let Some(security) = query.get("security") {
        profile.security = match security.as_str() {
            "reality" => Some(Security::Reality),
            "tls" => Some(Security::Tls),
            _ => None,
        };
    }
    if let Some(fp) = query.get("fp") {
        profile.fingerprint = Some(fp.clone());
    }
    if let Some(transport) = query.get("type") {
        profile.transport_type = match transport.as_str() {
            "grpc" => Some(TransportType::Grpc),
            "ws" => Some(TransportType::Ws),
            "http" => Some(TransportType::Http),
            _ => None,
        };
    }
    if let Some(service_name) = query.get("serviceName") {
        profile.transport_service_name = Some(service_name.clone());
    }
    if let Some(pbk) = query.get("pbk") {
        let reality = RealitySettings {
            public_key: pbk.clone(),
            short_id: query.get("sid").cloned().unwrap_or_default(),
            server_name: query.get("sni").cloned().unwrap_or_default(),
            spider_x: query.get("spx").cloned().unwrap_or_default(),
        };
        profile.reality = Some(reality);
    }

    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_display() {
        assert_eq!(format!("{}", Protocol::Vless), "vless");
    }

    #[test]
    fn routing_mode_as_str() {
        assert_eq!(RoutingMode::Global.as_str(), "Global");
        assert_eq!(RoutingMode::BypassRu.as_str(), "Bypass RU");
        assert_eq!(RoutingMode::OnlyRu.as_str(), "Only RU");
        assert_eq!(RoutingMode::BypassCn.as_str(), "Bypass CN");
        assert_eq!(RoutingMode::OnlyCn.as_str(), "Only CN");
        assert_eq!(RoutingMode::BypassIr.as_str(), "Bypass IR");
        assert_eq!(RoutingMode::OnlyIr.as_str(), "Only IR");
    }

    #[test]
    fn geo_region_deserializes_old_other_alias() {
        let json = r#""other""#;
        let region: GeoRegion = serde_json::from_str(json).unwrap();
        assert_eq!(region, GeoRegion::Global);
    }

    #[test]
    fn geo_region_serializes_to_global() {
        let json = serde_json::to_string(&GeoRegion::Global).unwrap();
        assert_eq!(json, r#""global""#);
    }

    #[test]
    fn routing_mode_available() {
        assert_eq!(RoutingMode::available(None), vec![RoutingMode::Global]);
        assert_eq!(
            RoutingMode::available(Some(GeoRegion::Ru)),
            vec![
                RoutingMode::Global,
                RoutingMode::BypassRu,
                RoutingMode::OnlyRu
            ]
        );
        assert_eq!(
            RoutingMode::available(Some(GeoRegion::Cn)),
            vec![
                RoutingMode::Global,
                RoutingMode::BypassCn,
                RoutingMode::OnlyCn
            ]
        );
        assert_eq!(
            RoutingMode::available(Some(GeoRegion::Ir)),
            vec![
                RoutingMode::Global,
                RoutingMode::BypassIr,
                RoutingMode::OnlyIr
            ]
        );
        assert_eq!(
            RoutingMode::available(Some(GeoRegion::Global)),
            vec![RoutingMode::Global]
        );
    }

    #[test]
    fn profile_new_defaults() {
        let p = Profile::new(
            "test".to_string(),
            Protocol::Vless,
            "1.2.3.4".to_string(),
            443,
            "uuid-here".to_string(),
        );
        assert_eq!(p.name, "test");
        assert_eq!(p.protocol, Protocol::Vless);
        assert_eq!(p.address, "1.2.3.4");
        assert_eq!(p.port, 443);
        assert_eq!(p.uuid, "uuid-here");
        assert!(p.flow.is_none());
        assert!(p.security.is_none());
        assert!(p.reality.is_none());
        assert!(p.transport_type.is_none());
        assert!(p.transport_service_name.is_none());
        assert!(p.fingerprint.is_none());
        assert!(p.tags.is_empty());
        // UUID should be non-nil
        assert_ne!(p.id, Uuid::nil());
    }

    #[test]
    fn settings_default() {
        let s = Settings::default();
        assert_eq!(s.tun_interface, "tun0");
        assert_eq!(s.dns_strategy, DnsStrategy::PreferIpv4);
        assert!(s.default_profile.is_none());
        assert!(!s.auto_connect);
        assert!(s.last_connected_profile.is_none());
        assert!(s.geo_routing.current_region.is_none());
        assert!(s.geo_routing.selected_region_modes.is_empty());
        assert_eq!(s.geo_routing.mode(), RoutingMode::Global);
    }

    #[test]
    fn geo_routing_mode_falls_back_to_global() {
        let g = GeoRouting::default();
        assert_eq!(g.mode(), RoutingMode::Global);
    }

    #[test]
    fn geo_routing_set_mode_persists_per_region() {
        let mut g = GeoRouting::default();
        g.set_region(GeoRegion::Ru);
        g.set_mode(RoutingMode::BypassRu);
        assert_eq!(g.mode(), RoutingMode::BypassRu);
        assert_eq!(
            g.selected_region_modes.get(&GeoRegion::Ru),
            Some(&RoutingMode::BypassRu)
        );

        g.set_region(GeoRegion::Cn);
        g.set_mode(RoutingMode::OnlyCn);
        assert_eq!(g.mode(), RoutingMode::OnlyCn);
        g.set_region(GeoRegion::Ru);
        assert_eq!(g.mode(), RoutingMode::BypassRu);
    }

    #[test]
    fn geo_routing_available_modes_uses_current_region() {
        let mut g = GeoRouting::default();
        assert_eq!(g.available_modes(), vec![RoutingMode::Global]);
        g.set_region(GeoRegion::Ru);
        assert_eq!(
            g.available_modes(),
            vec![
                RoutingMode::Global,
                RoutingMode::BypassRu,
                RoutingMode::OnlyRu
            ]
        );
    }

    #[test]
    fn settings_migrate_legacy_geo_routing() {
        let mut s = Settings {
            geo_region: Some(GeoRegion::Ru),
            routing_mode: RoutingMode::BypassRu,
            ..Default::default()
        };
        s.migrate_legacy_geo_routing();
        assert_eq!(s.geo_routing.current_region, Some(GeoRegion::Ru));
        assert_eq!(s.geo_routing.mode(), RoutingMode::BypassRu);
    }

    #[test]
    fn config_default() {
        let c = Config::default();
        assert!(c.profiles.is_empty());
        assert_eq!(c.settings.tun_interface, "tun0");
    }

    #[test]
    fn config_serde_roundtrip() {
        let mut config = Config::default();
        let mut profile = Profile::new(
            "Example".to_string(),
            Protocol::Vless,
            "203.0.113.1".to_string(),
            443,
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
        );
        profile.security = Some(Security::Reality);
        profile.reality = Some(RealitySettings {
            public_key: "pk".to_string(),
            short_id: "sid".to_string(),
            server_name: "sni".to_string(),
            spider_x: "/".to_string(),
        });
        profile.tags = vec!["tag1".to_string()];
        config.profiles.push(profile);
        config.settings.geo_routing.set_region(GeoRegion::Ru);
        config.settings.geo_routing.set_mode(RoutingMode::BypassRu);

        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, restored);
    }

    #[test]
    fn config_serde_roundtrip_with_geo_routing() {
        let mut config = Config::default();
        config
            .settings
            .geo_routing
            .selected_region_modes
            .insert(GeoRegion::Ru, RoutingMode::BypassRu);
        config
            .settings
            .geo_routing
            .selected_region_modes
            .insert(GeoRegion::Cn, RoutingMode::OnlyCn);
        config.settings.geo_routing.current_region = Some(GeoRegion::Ru);

        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, restored);
        assert_eq!(
            restored
                .settings
                .geo_routing
                .selected_region_modes
                .get(&GeoRegion::Ru)
                .copied()
                .unwrap_or(RoutingMode::Global),
            RoutingMode::BypassRu
        );
        assert_eq!(
            restored
                .settings
                .geo_routing
                .selected_region_modes
                .get(&GeoRegion::Cn)
                .copied()
                .unwrap_or(RoutingMode::Global),
            RoutingMode::OnlyCn
        );
    }

    #[test]
    fn config_deserializes_v0_11_2_legacy_fields() {
        // v0.11.2 only had geo_region and routing_mode; there was no
        // routing_modes map and no geo_routing object.
        let json = r#"{
            "profiles": [],
            "settings": {
                "geo_region": "ru",
                "routing_mode": "bypass_ru"
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.settings.geo_routing.current_region,
            Some(GeoRegion::Ru)
        );
        assert_eq!(config.settings.geo_routing.mode(), RoutingMode::BypassRu);

        // Reserializing must drop the legacy fields and emit the new shape.
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("\"geo_region\""));
        assert!(!json.contains("\"routing_mode\""));
        assert!(json.contains("\"geo_routing\""));
    }

    #[test]
    fn profile_deserialize_missing_optionals() {
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "name": "Minimal",
            "protocol": "vless",
            "address": "1.1.1.1",
            "port": 443,
            "uuid": "uuid"
        }"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.name, "Minimal");
        assert!(p.flow.is_none());
        assert!(p.reality.is_none());
        assert!(p.tags.is_empty());
    }

    #[test]
    fn config_deserialize_missing_fields() {
        let json = r#"{}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert!(c.profiles.is_empty());
        assert_eq!(c.settings.tun_interface, "tun0");
    }

    #[test]
    fn config_rejects_unknown_top_level_field() {
        let json = r#"{"unknown_field": 42}"#;
        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Should reject unknown top-level field");
    }

    #[test]
    fn profile_rejects_unknown_field() {
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "name": "Test",
            "protocol": "vless",
            "address": "1.1.1.1",
            "port": 443,
            "uuid": "uuid",
            "unknown_field": true
        }"#;
        let result: Result<Profile, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Should reject unknown profile field");
    }

    #[test]
    fn config_validate_accepts_valid_config() {
        let mut config = Config::default();
        config.profiles.push(Profile::new(
            "Valid".to_string(),
            Protocol::Vless,
            "1.2.3.4".to_string(),
            443,
            "uuid".to_string(),
        ));
        config.settings.default_profile = Some(config.profiles[0].id);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validate_rejects_empty_profile_name() {
        let mut config = Config::default();
        config.profiles.push(Profile::new(
            "   ".to_string(),
            Protocol::Vless,
            "1.2.3.4".to_string(),
            443,
            "uuid".to_string(),
        ));
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("name must not be empty"), "Error was: {}", err);
    }

    #[test]
    fn config_validate_rejects_empty_profile_address() {
        let mut config = Config::default();
        config.profiles.push(Profile::new(
            "Name".to_string(),
            Protocol::Vless,
            "".to_string(),
            443,
            "uuid".to_string(),
        ));
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("address must not be empty"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn config_validate_rejects_empty_profile_uuid() {
        let mut config = Config::default();
        config.profiles.push(Profile::new(
            "Name".to_string(),
            Protocol::Vless,
            "1.2.3.4".to_string(),
            443,
            "  ".to_string(),
        ));
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("uuid must not be empty"), "Error was: {}", err);
    }

    #[test]
    fn config_validate_rejects_dangling_default_profile() {
        let mut config = Config::default();
        config.settings.default_profile = Some(Uuid::new_v4());
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("references a non-existent profile"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn parse_long_vless_uri() {
        let uri = r#"vless://671c62c7-6768-4b98-ac6b-572c9c707be0@203.0.113.42:59431?type=grpc&encryption=none&serviceName=&authority=&security=reality&pbk=0IO3LodsrMnhOWh4ogwgdVqYg30CS5-snhFMwldOuAQ&fp=chrome&sni=google.com&sid=f04debc34cbc48a4&spx=%2F#Example-2873vb06"#;
        let profile = parse_share_link(uri).unwrap();
        assert_eq!(profile.protocol, Protocol::Vless);
        assert_eq!(profile.address, "203.0.113.42");
        assert_eq!(profile.port, 59431);
        assert_eq!(profile.uuid, "671c62c7-6768-4b98-ac6b-572c9c707be0");
        assert_eq!(profile.name, "Example-2873vb06");
        assert!(profile.security.is_some());
        let reality = profile.reality.unwrap();
        assert_eq!(
            reality.public_key,
            "0IO3LodsrMnhOWh4ogwgdVqYg30CS5-snhFMwldOuAQ"
        );
        assert_eq!(reality.server_name, "google.com");
        assert_eq!(reality.short_id, "f04debc34cbc48a4");
        assert_eq!(reality.spider_x, "/");
    }

    #[test]
    fn parse_vless_minimal() {
        let uri = "vless://uuid@1.2.3.4:443#Name";
        let profile = parse_share_link(uri).unwrap();
        assert_eq!(profile.uuid, "uuid");
        assert_eq!(profile.address, "1.2.3.4");
        assert_eq!(profile.port, 443);
        assert_eq!(profile.name, "Name");
        assert!(profile.reality.is_none());
        assert!(profile.flow.is_none());
        assert!(profile.fingerprint.is_none());
        assert!(profile.transport_type.is_none());
    }

    #[test]
    fn parse_vless_default_port() {
        let uri = "vless://uuid@example.com#Test";
        let profile = parse_share_link(uri).unwrap();
        assert_eq!(profile.port, 443);
        assert_eq!(profile.address, "example.com");
    }

    #[test]
    fn parse_vless_partial_reality() {
        let uri = "vless://uuid@1.2.3.4:8443?security=reality&pbk=pk123&sni=sni.test#Partial";
        let profile = parse_share_link(uri).unwrap();
        assert_eq!(profile.security, Some(Security::Reality));
        let reality = profile.reality.unwrap();
        assert_eq!(reality.public_key, "pk123");
        assert_eq!(reality.server_name, "sni.test");
        assert!(reality.short_id.is_empty());
        assert!(reality.spider_x.is_empty());
    }

    #[test]
    fn parse_vless_url_encoded_spx() {
        let uri = "vless://uuid@1.2.3.4?pbk=k&spx=%2Fpath%2Fhere#N";
        let profile = parse_share_link(uri).unwrap();
        assert_eq!(profile.reality.as_ref().unwrap().spider_x, "/path/here");
    }

    #[test]
    fn parse_unsupported_format_fails() {
        let result = parse_share_link("ss://encrypted");
        assert!(result.is_err());
    }

    #[test]
    fn parse_vless_missing_host_fails() {
        let result = parse_share_link("vless://");
        assert!(result.is_err());
    }
}
