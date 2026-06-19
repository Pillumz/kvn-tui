use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;
use uuid::Uuid;

/// Supported VPN protocols.
///
/// This enum is the discriminant for [`ProtocolConfig`] and is also used as
/// a lightweight label for UI rendering. Per-protocol fields live on the
/// corresponding [`ProtocolConfig`] variant, not on this enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
    Hysteria2,
    Tuic,
    Shadowtls,
    Anytls,
    Socks,
    Http,
    Ssh,
}

impl Protocol {
    /// Lowercase identifier used in JSON serialization and internal dispatch.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Vless => "vless",
            Protocol::Vmess => "vmess",
            Protocol::Trojan => "trojan",
            Protocol::Shadowsocks => "shadowsocks",
            Protocol::Hysteria2 => "hysteria2",
            Protocol::Tuic => "tuic",
            Protocol::Shadowtls => "shadowtls",
            Protocol::Anytls => "anytls",
            Protocol::Socks => "socks",
            Protocol::Http => "http",
            Protocol::Ssh => "ssh",
        }
    }

    /// Short label for the UI protocol column (fits within 6 characters).
    pub fn ui_label(self) -> &'static str {
        match self {
            Protocol::Shadowsocks => "ss",
            Protocol::Hysteria2 => "hy2",
            Protocol::Shadowtls => "stls",
            other => other.as_str(),
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Selected geo region for rule-set downloads and routing mode availability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GeoRegion {
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

/// TLS Encrypted Client Hello (ECH) configuration.
///
/// Maps onto sing-box's `tls.ech` block. When `config` is empty, sing-box
/// fetches the `ECHConfigList` from DNS HTTPS RR for the target server.
/// Mutually exclusive with REALITY (validated by [`Config::validate`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct EchSettings {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<String>,
}

/// Shared TLS configuration for protocols that carry a TLS layer
/// (VMess, Trojan, ShadowTLS, AnyTLS, Hysteria2, TUIC).
///
/// VLESS keeps its TLS-related fields flat on [`VlessConfig`] for
/// backward compatibility with existing `profiles.json` files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TlsCommon {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub insecure: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alpn: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utls_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality: Option<RealitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ech: Option<EchSettings>,
}

impl TlsCommon {
    /// REALITY and ECH cannot be enabled simultaneously — REALITY uses its
    /// own SNI-cloaking mechanism that conflicts with ECH's `ECHConfigList`.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.reality.is_some() && self.ech.as_ref().is_some_and(|e| e.enabled) {
            anyhow::bail!("tls.reality and tls.ech are mutually exclusive");
        }
        Ok(())
    }
}

/// Transport layer configuration (ws / grpc / http / httpupgrade).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportConfig {
    #[serde(rename = "type")]
    pub kind: TransportType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

/// VMess encryption cipher. Sing-box 1.12 still accepts `auto`; we forbid
/// the legacy stream cipher `aes-128-cfb`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum VmessSecurity {
    #[default]
    Auto,
    None,
    Zero,
    #[serde(rename = "aes-128-gcm")]
    Aes128Gcm,
    #[serde(rename = "chacha20-poly1305")]
    Chacha20Poly1305,
}

impl VmessSecurity {
    #[allow(dead_code)] // consumed by per-protocol outbound builders landing in PR2
    pub fn as_str(self) -> &'static str {
        match self {
            VmessSecurity::Auto => "auto",
            VmessSecurity::None => "none",
            VmessSecurity::Zero => "zero",
            VmessSecurity::Aes128Gcm => "aes-128-gcm",
            VmessSecurity::Chacha20Poly1305 => "chacha20-poly1305",
        }
    }
}

/// Shadowsocks AEAD-2022 + AEAD ciphers supported by sing-box 1.12.
/// Legacy stream ciphers (e.g. `aes-128-cfb`) are intentionally excluded.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ShadowsocksCipher {
    #[default]
    #[serde(rename = "chacha20-ietf-poly1305")]
    Chacha20IetfPoly1305,
    #[serde(rename = "aes-128-gcm")]
    Aes128Gcm,
    #[serde(rename = "aes-256-gcm")]
    Aes256Gcm,
    #[serde(rename = "2022-blake3-aes-128-gcm")]
    Blake3Aes128Gcm,
    #[serde(rename = "2022-blake3-aes-256-gcm")]
    Blake3Aes256Gcm,
    #[serde(rename = "2022-blake3-chacha20-poly1305")]
    Blake3Chacha20Poly1305,
    None,
}

impl ShadowsocksCipher {
    #[allow(dead_code)] // consumed by per-protocol outbound builders landing in PR2
    pub fn as_str(self) -> &'static str {
        match self {
            ShadowsocksCipher::Chacha20IetfPoly1305 => "chacha20-ietf-poly1305",
            ShadowsocksCipher::Aes128Gcm => "aes-128-gcm",
            ShadowsocksCipher::Aes256Gcm => "aes-256-gcm",
            ShadowsocksCipher::Blake3Aes128Gcm => "2022-blake3-aes-128-gcm",
            ShadowsocksCipher::Blake3Aes256Gcm => "2022-blake3-aes-256-gcm",
            ShadowsocksCipher::Blake3Chacha20Poly1305 => "2022-blake3-chacha20-poly1305",
            ShadowsocksCipher::None => "none",
        }
    }
}

/// Hysteria2 obfuscation. Sing-box 1.12+ supports the `salamander` type
/// (legacy top-level `obfs_password` is rejected).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub kind: Hysteria2ObfsType,
    pub password: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Hysteria2ObfsType {
    #[default]
    Salamander,
}

/// TUIC v5 congestion control algorithm.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TuicCongestion {
    #[default]
    Bbr,
    Cubic,
    NewReno,
}

impl TuicCongestion {
    #[allow(dead_code)] // consumed by per-protocol outbound builders landing in PR2
    pub fn as_str(self) -> &'static str {
        match self {
            TuicCongestion::Bbr => "bbr",
            TuicCongestion::Cubic => "cubic",
            TuicCongestion::NewReno => "new_reno",
        }
    }
}

/// TUIC v5 UDP relay mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TuicUdpRelayMode {
    #[default]
    Native,
    Quic,
}

impl TuicUdpRelayMode {
    #[allow(dead_code)] // consumed by per-protocol outbound builders landing in PR2
    pub fn as_str(self) -> &'static str {
        match self {
            TuicUdpRelayMode::Native => "native",
            TuicUdpRelayMode::Quic => "quic",
        }
    }
}

/// ShadowTLS protocol version. v1/v2 are deprecated; v3 is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowtlsVersion {
    V1,
    V2,
    #[default]
    V3,
}

impl ShadowtlsVersion {
    pub fn as_u8(self) -> u8 {
        match self {
            ShadowtlsVersion::V1 => 1,
            ShadowtlsVersion::V2 => 2,
            ShadowtlsVersion::V3 => 3,
        }
    }
}

impl Serialize for ShadowtlsVersion {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u8(self.as_u8())
    }
}

impl<'de> Deserialize<'de> for ShadowtlsVersion {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(de)?;
        match v {
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            other => Err(serde::de::Error::custom(format!(
                "unknown ShadowTLS version {}",
                other
            ))),
        }
    }
}

/// SOCKS proxy version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SocksVersion {
    #[serde(rename = "4")]
    V4,
    #[serde(rename = "4a")]
    V4a,
    #[default]
    #[serde(rename = "5")]
    V5,
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

impl DnsStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            DnsStrategy::PreferIpv4 => "prefer_ipv4",
            DnsStrategy::PreferIpv6 => "prefer_ipv6",
            DnsStrategy::OnlyIpv4 => "ipv4_only",
            DnsStrategy::OnlyIpv6 => "ipv6_only",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            DnsStrategy::PreferIpv4 => DnsStrategy::PreferIpv6,
            DnsStrategy::PreferIpv6 => DnsStrategy::OnlyIpv4,
            DnsStrategy::OnlyIpv4 => DnsStrategy::OnlyIpv6,
            DnsStrategy::OnlyIpv6 => DnsStrategy::PreferIpv4,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            DnsStrategy::PreferIpv4 => DnsStrategy::OnlyIpv6,
            DnsStrategy::PreferIpv6 => DnsStrategy::PreferIpv4,
            DnsStrategy::OnlyIpv4 => DnsStrategy::PreferIpv6,
            DnsStrategy::OnlyIpv6 => DnsStrategy::OnlyIpv4,
        }
    }
}

/// A single sing-box DNS server. Variants map 1:1 onto sing-box 1.12 server types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DnsServer {
    Local {
        tag: String,
    },
    Udp {
        tag: String,
        server: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_port: Option<u16>,
    },
    Tcp {
        tag: String,
        server: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_port: Option<u16>,
    },
    Tls {
        tag: String,
        server: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_port: Option<u16>,
    },
    Https {
        tag: String,
        server: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_port: Option<u16>,
        #[serde(default = "default_doh_path")]
        path: String,
    },
    Quic {
        tag: String,
        server: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_port: Option<u16>,
    },
    FakeIp {
        tag: String,
        #[serde(default = "default_fakeip_v4")]
        inet4_range: String,
        #[serde(default = "default_fakeip_v6")]
        inet6_range: String,
    },
}

impl DnsServer {
    pub fn tag(&self) -> &str {
        match self {
            DnsServer::Local { tag }
            | DnsServer::Udp { tag, .. }
            | DnsServer::Tcp { tag, .. }
            | DnsServer::Tls { tag, .. }
            | DnsServer::Https { tag, .. }
            | DnsServer::Quic { tag, .. }
            | DnsServer::FakeIp { tag, .. } => tag,
        }
    }

    /// Short label used in the status bar, e.g. "DoH", "DoT", "fakeip".
    pub fn kind_label(&self) -> &'static str {
        match self {
            DnsServer::Local { .. } => "local",
            DnsServer::Udp { .. } => "UDP",
            DnsServer::Tcp { .. } => "TCP",
            DnsServer::Tls { .. } => "DoT",
            DnsServer::Https { .. } => "DoH",
            DnsServer::Quic { .. } => "DoQ",
            DnsServer::FakeIp { .. } => "fakeip",
        }
    }
}

/// A per-domain DNS routing rule. Maps onto sing-box `dns.rules[*]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct DnsRule {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_suffix: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_keyword: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_regex: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_set: Vec<String>,
    /// Must match a server tag in [`DnsConfig::servers`].
    pub server: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disable_cache: bool,
}

/// User-controlled DNS configuration. Replaces the hard-coded DNS section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DnsConfig {
    #[serde(default = "default_dns_servers")]
    pub servers: Vec<DnsServer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<DnsRule>,
    /// Tag of the fallback DNS server used when no rule matches.
    #[serde(default = "default_final_server")]
    pub final_server: String,
    #[serde(default)]
    pub strategy: DnsStrategy,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fakeip_enabled: bool,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            servers: default_dns_servers(),
            rules: Vec::new(),
            final_server: default_final_server(),
            strategy: DnsStrategy::default(),
            fakeip_enabled: false,
        }
    }
}

fn default_doh_path() -> String {
    "/dns-query".to_string()
}

fn default_fakeip_v4() -> String {
    "198.18.0.0/15".to_string()
}

fn default_fakeip_v6() -> String {
    "fc00::/18".to_string()
}

fn default_final_server() -> String {
    "remote".to_string()
}

fn default_dns_servers() -> Vec<DnsServer> {
    vec![
        DnsServer::Local {
            tag: "local".to_string(),
        },
        DnsServer::Https {
            tag: "remote".to_string(),
            server: "1.1.1.1".to_string(),
            server_port: None,
            path: default_doh_path(),
        },
    ]
}

/// VLESS-specific profile configuration.
///
/// TLS-related fields (`security`, `reality`, `fingerprint`) and transport
/// fields are kept flat at the same JSON level as the outer [`Profile`] for
/// backward compatibility with existing `profiles.json` files predating the
/// `ProtocolConfig` refactor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VlessConfig {
    pub uuid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<Flow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Security>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reality: Option<RealitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_type: Option<TransportType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ech: Option<EchSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VmessConfig {
    pub uuid: String,
    #[serde(default)]
    pub alter_id: u32,
    #[serde(default)]
    pub security: VmessSecurity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_padding: Option<bool>,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrojanConfig {
    pub password: String,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShadowsocksConfig {
    pub method: ShadowsocksCipher,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Hysteria2Config {
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_mbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_mbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TuicConfig {
    pub uuid: String,
    pub password: String,
    #[serde(default)]
    pub congestion_control: TuicCongestion,
    #[serde(default)]
    pub udp_relay_mode: TuicUdpRelayMode,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub zero_rtt_handshake: bool,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
}

/// ShadowTLS-wrapped Shadowsocks. The sing-box `shadowtls` outbound is
/// a TLS-camouflage wrapper that does not perform any traffic ciphering on
/// its own; an inner Shadowsocks outbound chained via `detour` carries the
/// actual data. We model both halves in one profile so the user supplies
/// the ShadowTLS password (v3) plus the inner SS method/password once.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShadowtlsConfig {
    #[serde(default)]
    pub version: ShadowtlsVersion,
    /// ShadowTLS v3 client password. Unused for v1/v2.
    pub password: String,
    /// Inner Shadowsocks cipher used by the detour outbound.
    #[serde(default)]
    pub method: ShadowsocksCipher,
    /// Inner Shadowsocks password used by the detour outbound.
    #[serde(default)]
    pub ss_password: String,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnytlsConfig {
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_session_check_interval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_session_timeout: Option<String>,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SocksConfig {
    #[serde(default)]
    pub version: SocksVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HttpConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, flatten)]
    pub tls: TlsCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SshConfig {
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_passphrase: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_key_algorithms: Vec<String>,
}

/// Protocol-specific profile configuration.
///
/// The `protocol` discriminant is serialized at the same JSON level as the
/// other [`Profile`] fields via `#[serde(flatten)]` (internally-tagged enum).
/// For VLESS this preserves the historic `profiles.json` shape exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum ProtocolConfig {
    Vless(VlessConfig),
    Vmess(VmessConfig),
    Trojan(TrojanConfig),
    Shadowsocks(ShadowsocksConfig),
    Hysteria2(Hysteria2Config),
    Tuic(TuicConfig),
    Shadowtls(ShadowtlsConfig),
    Anytls(AnytlsConfig),
    Socks(SocksConfig),
    Http(HttpConfig),
    Ssh(SshConfig),
}

impl ProtocolConfig {
    pub fn protocol(&self) -> Protocol {
        match self {
            ProtocolConfig::Vless(_) => Protocol::Vless,
            ProtocolConfig::Vmess(_) => Protocol::Vmess,
            ProtocolConfig::Trojan(_) => Protocol::Trojan,
            ProtocolConfig::Shadowsocks(_) => Protocol::Shadowsocks,
            ProtocolConfig::Hysteria2(_) => Protocol::Hysteria2,
            ProtocolConfig::Tuic(_) => Protocol::Tuic,
            ProtocolConfig::Shadowtls(_) => Protocol::Shadowtls,
            ProtocolConfig::Anytls(_) => Protocol::Anytls,
            ProtocolConfig::Socks(_) => Protocol::Socks,
            ProtocolConfig::Http(_) => Protocol::Http,
            ProtocolConfig::Ssh(_) => Protocol::Ssh,
        }
    }

    fn tls_common(&self) -> Option<&TlsCommon> {
        match self {
            ProtocolConfig::Vmess(c) => Some(&c.tls),
            ProtocolConfig::Trojan(c) => Some(&c.tls),
            ProtocolConfig::Hysteria2(c) => Some(&c.tls),
            ProtocolConfig::Tuic(c) => Some(&c.tls),
            ProtocolConfig::Shadowtls(c) => Some(&c.tls),
            ProtocolConfig::Anytls(c) => Some(&c.tls),
            ProtocolConfig::Http(c) => Some(&c.tls),
            // VLESS keeps reality/ech flat on VlessConfig; no shared block.
            ProtocolConfig::Vless(_)
            | ProtocolConfig::Shadowsocks(_)
            | ProtocolConfig::Socks(_)
            | ProtocolConfig::Ssh(_) => None,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        match self {
            ProtocolConfig::Vless(c) => {
                if c.uuid.trim().is_empty() {
                    anyhow::bail!("vless.uuid must not be empty");
                }
                if c.reality.is_some() && c.ech.as_ref().is_some_and(|e| e.enabled) {
                    anyhow::bail!("vless: reality and ech are mutually exclusive");
                }
            }
            ProtocolConfig::Vmess(c) => {
                if c.uuid.trim().is_empty() {
                    anyhow::bail!("vmess.uuid must not be empty");
                }
            }
            ProtocolConfig::Trojan(c) => {
                if c.password.is_empty() {
                    anyhow::bail!("trojan.password must not be empty");
                }
            }
            ProtocolConfig::Shadowsocks(c) => {
                if c.password.is_empty() {
                    anyhow::bail!("shadowsocks.password must not be empty");
                }
            }
            ProtocolConfig::Hysteria2(c) => {
                if c.password.is_empty() {
                    anyhow::bail!("hysteria2.password must not be empty");
                }
            }
            ProtocolConfig::Tuic(c) => {
                if c.uuid.trim().is_empty() {
                    anyhow::bail!("tuic.uuid must not be empty");
                }
                if c.password.is_empty() {
                    anyhow::bail!("tuic.password must not be empty");
                }
            }
            ProtocolConfig::Shadowtls(c) => {
                if c.version == ShadowtlsVersion::V3 && c.password.is_empty() {
                    anyhow::bail!("shadowtls.password must not be empty for v3");
                }
                if c.ss_password.is_empty() {
                    anyhow::bail!(
                        "shadowtls.ss_password must not be empty (inner Shadowsocks detour)"
                    );
                }
            }
            ProtocolConfig::Anytls(c) => {
                if c.password.is_empty() {
                    anyhow::bail!("anytls.password must not be empty");
                }
            }
            ProtocolConfig::Socks(_) | ProtocolConfig::Http(_) => {}
            ProtocolConfig::Ssh(c) => {
                if c.user.trim().is_empty() {
                    anyhow::bail!("ssh.user must not be empty");
                }
            }
        }
        if let Some(tls) = self.tls_common() {
            tls.validate()?;
        }
        Ok(())
    }
}

/// Single VPN profile. The `protocol` discriminant and protocol-specific
/// fields are flattened into [`ProtocolConfig`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub port: u16,
    #[serde(flatten)]
    pub config: ProtocolConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<Uuid>,
}

impl Profile {
    /// Create a new VLESS profile with a generated UUID. Other protocols
    /// gain dedicated constructors as their share-link parsers land.
    pub fn new_vless(name: String, address: String, port: u16, uuid: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            address,
            port,
            config: ProtocolConfig::Vless(VlessConfig {
                uuid,
                ..VlessConfig::default()
            }),
            tags: Vec::new(),
            subscription_id: None,
        }
    }

    /// Protocol discriminant.
    pub fn protocol(&self) -> Protocol {
        self.config.protocol()
    }

    /// Short label for the UI protocol column (≤6 chars).
    pub fn protocol_label(&self) -> &'static str {
        self.protocol().ui_label()
    }

    /// Stable key identifying the credentials behind this profile,
    /// used by the subscription importer to detect duplicates.
    pub fn dedup_key(&self) -> String {
        match &self.config {
            ProtocolConfig::Vless(c) => format!("vless:{}", c.uuid),
            ProtocolConfig::Vmess(c) => format!("vmess:{}", c.uuid),
            ProtocolConfig::Trojan(c) => {
                format!("trojan:{}@{}:{}", c.password, self.address, self.port)
            }
            ProtocolConfig::Shadowsocks(c) => {
                format!("ss:{}@{}:{}", c.password, self.address, self.port)
            }
            ProtocolConfig::Hysteria2(c) => {
                format!("hy2:{}@{}:{}", c.password, self.address, self.port)
            }
            ProtocolConfig::Tuic(c) => format!("tuic:{}", c.uuid),
            ProtocolConfig::Shadowtls(c) => {
                format!("shadowtls:{}@{}:{}", c.password, self.address, self.port)
            }
            ProtocolConfig::Anytls(c) => {
                format!("anytls:{}@{}:{}", c.password, self.address, self.port)
            }
            ProtocolConfig::Socks(c) => format!(
                "socks:{}@{}:{}",
                c.username.as_deref().unwrap_or(""),
                self.address,
                self.port
            ),
            ProtocolConfig::Http(c) => format!(
                "http:{}@{}:{}",
                c.username.as_deref().unwrap_or(""),
                self.address,
                self.port
            ),
            ProtocolConfig::Ssh(c) => format!("ssh:{}@{}:{}", c.user, self.address, self.port),
        }
    }
}

/// Auto-update schedule for a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionAutoUpdate {
    #[default]
    Off,
    Every1h,
    Every12h,
    Every1d,
    Every7d,
}

impl SubscriptionAutoUpdate {
    /// Return the interval in minutes.
    pub fn interval_minutes(self) -> u64 {
        match self {
            Self::Off => 0,
            Self::Every1h => 60,
            Self::Every12h => 720,
            Self::Every1d => 1440,
            Self::Every7d => 10080,
        }
    }

    /// Cycle to the next schedule.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Every1h,
            Self::Every1h => Self::Every12h,
            Self::Every12h => Self::Every1d,
            Self::Every1d => Self::Every7d,
            Self::Every7d => Self::Off,
        }
    }

    /// Short label for the schedule, e.g. "✕" or "🗘 1h".
    pub fn label(self) -> String {
        match self {
            Self::Off => "✕".to_string(),
            _ => format!("🗘 {}", self.interval_label()),
        }
    }

    /// Short interval label without icon.
    pub fn interval_label(self) -> String {
        match self {
            Self::Off => "off".to_string(),
            Self::Every1h => "1h".to_string(),
            Self::Every12h => "12h".to_string(),
            Self::Every1d => "1d".to_string(),
            Self::Every7d => "7d".to_string(),
        }
    }
}

/// A subscription URL that can be refreshed to import a set of profiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub auto_update: SubscriptionAutoUpdate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<DateTime<Local>>,
}

#[test]
fn subscription_auto_update_cycles_and_labels() {
    assert_eq!(SubscriptionAutoUpdate::Off.interval_minutes(), 0);
    assert_eq!(SubscriptionAutoUpdate::Every1h.interval_minutes(), 60);
    assert_eq!(SubscriptionAutoUpdate::Every12h.interval_minutes(), 720);
    assert_eq!(SubscriptionAutoUpdate::Every1d.interval_minutes(), 1440);
    assert_eq!(SubscriptionAutoUpdate::Every7d.interval_minutes(), 10080);

    assert_eq!(
        SubscriptionAutoUpdate::Off.next(),
        SubscriptionAutoUpdate::Every1h
    );
    assert_eq!(
        SubscriptionAutoUpdate::Every7d.next(),
        SubscriptionAutoUpdate::Off
    );

    assert_eq!(SubscriptionAutoUpdate::Off.label(), "✕");
    assert_eq!(SubscriptionAutoUpdate::Every1h.label(), "🗘 1h");
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

/// Application settings stored alongside profiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<Uuid>,
    #[serde(default = "default_tun_interface")]
    pub tun_interface: String,
    /// Legacy field, superseded by `dns.strategy`. Kept for one release so
    /// existing config files still load; on save we re-emit it from `dns.strategy`
    /// to avoid splitting the source of truth.
    #[serde(default = "default_dns_strategy")]
    pub dns_strategy: DnsStrategy,
    #[serde(default)]
    pub dns: DnsConfig,
    #[serde(default)]
    pub geo_routing: GeoRouting,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default)]
    pub kill_switch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected_profile: Option<Uuid>,
}

fn default_tun_interface() -> String {
    "tun0".to_string()
}

fn default_dns_strategy() -> DnsStrategy {
    DnsStrategy::PreferIpv4
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_profile: None,
            tun_interface: default_tun_interface(),
            dns_strategy: default_dns_strategy(),
            dns: DnsConfig::default(),
            geo_routing: GeoRouting::default(),
            auto_connect: false,
            kill_switch: false,
            last_connected_profile: None,
        }
    }
}

/// Root configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<Subscription>,
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
    /// - DNS server tags are non-empty and unique; `dns.final_server` and every
    ///   `dns.rules[*].server` reference an existing tag; when `fakeip_enabled`
    ///   at least one server is of type `fakeip`.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (idx, profile) in self.profiles.iter().enumerate() {
            let num = idx + 1;
            if profile.name.trim().is_empty() {
                anyhow::bail!("Profile {num}: name must not be empty");
            }
            if profile.address.trim().is_empty() {
                anyhow::bail!("Profile {num}: address must not be empty");
            }
            if let Err(e) = profile.config.validate() {
                anyhow::bail!("Profile {num}: {e}");
            }
        }

        if let Some(id) = self.settings.default_profile {
            if !self.profiles.iter().any(|p| p.id == id) {
                anyhow::bail!("settings.default_profile ({id}) references a non-existent profile");
            }
        }

        self.settings.dns.validate()?;

        Ok(())
    }

    /// Promote the legacy `Settings.dns_strategy` field into `Settings.dns.strategy`
    /// when the latter is at its default and the former is not. Idempotent.
    pub fn migrate_legacy_dns_strategy(&mut self) {
        if self.settings.dns.strategy == DnsStrategy::default()
            && self.settings.dns_strategy != DnsStrategy::default()
        {
            self.settings.dns.strategy = self.settings.dns_strategy.clone();
        }
        // Keep both fields in sync going forward; `dns.strategy` is the source.
        self.settings.dns_strategy = self.settings.dns.strategy.clone();
    }
}

impl DnsConfig {
    /// Validate that server tags are unique and rule/final references point at
    /// existing tags. Called from [`Config::validate`].
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for server in &self.servers {
            let tag = server.tag();
            if tag.trim().is_empty() {
                anyhow::bail!("dns.servers: server tag must not be empty");
            }
            if !tags.insert(tag) {
                anyhow::bail!("dns.servers: duplicate server tag {:?}", tag);
            }
        }
        if !tags.contains(self.final_server.as_str()) {
            anyhow::bail!(
                "dns.final_server {:?} does not match any server tag",
                self.final_server
            );
        }
        for (idx, rule) in self.rules.iter().enumerate() {
            if !tags.contains(rule.server.as_str()) {
                anyhow::bail!(
                    "dns.rules[{idx}].server {:?} does not match any server tag",
                    rule.server
                );
            }
        }
        if self.fakeip_enabled
            && !self
                .servers
                .iter()
                .any(|s| matches!(s, DnsServer::FakeIp { .. }))
        {
            anyhow::bail!("dns.fakeip_enabled = true but no fakeip server is defined");
        }
        Ok(())
    }

    /// Return the first `fakeip` server, if any.
    pub fn fakeip_server(&self) -> Option<&DnsServer> {
        self.servers
            .iter()
            .find(|s| matches!(s, DnsServer::FakeIp { .. }))
    }

    /// Return the final server entry (used by the status bar to label the
    /// current upstream by its kind).
    pub fn final_server_entry(&self) -> Option<&DnsServer> {
        self.servers
            .iter()
            .find(|s| s.tag() == self.final_server.as_str())
    }
}

/// All share-link URI schemes recognised by [`parse_share_link`].
/// Used both for prefix dispatch and by `infra::subscription` to detect
/// subscription bodies after base64 decoding.
pub const SUPPORTED_SHARE_SCHEMES: &[&str] = &[
    "vless://",
    "vmess://",
    "trojan://",
    "ss://",
    "hysteria2://",
    "hy2://",
    "tuic://",
    "socks://",
    "socks5://",
    "http://",
    "https://",
    "ssh://",
    "anytls://",
    "shadowtls://",
];

/// Parse a share link text into a Profile. Dispatches on URI scheme.
pub fn parse_share_link(text: &str) -> Result<Profile> {
    let trimmed = text.trim();
    let scheme_end = trimmed.find("://").context("Missing URI scheme")?;
    let scheme = &trimmed[..scheme_end];
    let rest = &trimmed[scheme_end + 3..];

    match scheme {
        "vless" => parse_vless(rest),
        "vmess" => parse_vmess(rest),
        "trojan" => parse_trojan(rest),
        "ss" => parse_shadowsocks(rest),
        "hysteria2" | "hy2" => parse_hysteria2(rest),
        "tuic" => parse_tuic(rest),
        "socks" | "socks5" => parse_socks(rest),
        "http" | "https" => parse_http(rest, scheme == "https"),
        "ssh" => parse_ssh(rest),
        "anytls" => parse_anytls(rest),
        "shadowtls" => parse_shadowtls(rest),
        other => anyhow::bail!("Unsupported share link scheme: {other}://"),
    }
}

fn parse_uri(scheme: &str, rest: &str) -> Result<Url> {
    Url::parse(&format!("{scheme}://{rest}")).with_context(|| format!("Invalid {scheme} URL"))
}

fn query_map(url: &Url) -> std::collections::HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn fragment_name(url: &Url, fallback: &str) -> Result<String> {
    Ok(match url.fragment() {
        Some(f) => urlencoding::decode(f)?.to_string(),
        None => fallback.to_string(),
    })
}

fn decode_b64_lenient(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    // Try URL-safe-no-pad first (ss://, vmess JSON often), then standard.
    if let Ok(b) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&cleaned) {
        return Ok(b);
    }
    if let Ok(b) = base64::engine::general_purpose::URL_SAFE.decode(&cleaned) {
        return Ok(b);
    }
    if let Ok(b) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned) {
        return Ok(b);
    }
    base64::engine::general_purpose::STANDARD
        .decode(&cleaned)
        .context("base64 decode failed")
}

fn parse_transport_type(s: &str) -> Option<TransportType> {
    match s {
        "grpc" => Some(TransportType::Grpc),
        "ws" => Some(TransportType::Ws),
        "http" => Some(TransportType::Http),
        _ => None,
    }
}

fn parse_alpn(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_bool_param(s: &str) -> bool {
    matches!(s, "1" | "true" | "yes")
}

/// Apply transport/SNI/utls/reality/ech parameters that VLESS/VMess/Trojan
/// share when their share-link is in plain URI form (not VMess base64-JSON).
fn extract_tls_common_from_query(q: &std::collections::HashMap<String, String>) -> TlsCommon {
    let mut tls = TlsCommon::default();
    if let Some(sni) = q.get("sni") {
        tls.server_name = Some(sni.clone());
    } else if let Some(host) = q.get("host") {
        tls.server_name = Some(host.clone());
    }
    if let Some(v) = q.get("alpn") {
        tls.alpn = parse_alpn(v);
    }
    if let Some(fp) = q.get("fp") {
        tls.utls_fingerprint = Some(fp.clone());
    }
    if q.get("allowInsecure")
        .map(|s| parse_bool_param(s))
        .unwrap_or(false)
        || q.get("insecure")
            .map(|s| parse_bool_param(s))
            .unwrap_or(false)
    {
        tls.insecure = true;
    }
    if let Some(pbk) = q.get("pbk") {
        tls.reality = Some(RealitySettings {
            public_key: pbk.clone(),
            short_id: q.get("sid").cloned().unwrap_or_default(),
            server_name: q.get("sni").cloned().unwrap_or_default(),
            spider_x: q.get("spx").cloned().unwrap_or_default(),
        });
    }
    tls
}

fn extract_transport_from_query(
    q: &std::collections::HashMap<String, String>,
) -> Option<TransportConfig> {
    let kind = parse_transport_type(q.get("type")?)?;
    Some(TransportConfig {
        kind,
        path: q.get("path").cloned(),
        host: q.get("host").cloned(),
        service_name: q.get("serviceName").cloned(),
        headers: HashMap::new(),
    })
}

/// Parse a VLESS URI fragment.
fn parse_vless(rest: &str) -> Result<Profile> {
    let url = parse_uri("vless", rest)?;
    let uuid = url.username().to_string();
    let host = url
        .host_str()
        .context("Missing host in VLESS URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let mut profile = Profile::new_vless(name, host, port, uuid);

    let query = query_map(&url);

    let ProtocolConfig::Vless(ref mut cfg) = profile.config else {
        unreachable!("Profile::new_vless constructs a Vless variant");
    };

    if let Some(flow) = query.get("flow") {
        cfg.flow = match flow.as_str() {
            "xtls-rprx-vision" => Some(Flow::XtlsRprxVision),
            _ => None,
        };
    }
    if let Some(security) = query.get("security") {
        cfg.security = match security.as_str() {
            "reality" => Some(Security::Reality),
            "tls" => Some(Security::Tls),
            _ => None,
        };
    }
    if let Some(fp) = query.get("fp") {
        cfg.fingerprint = Some(fp.clone());
    }
    if let Some(transport) = query.get("type") {
        cfg.transport_type = parse_transport_type(transport);
    }
    if let Some(service_name) = query.get("serviceName") {
        cfg.transport_service_name = Some(service_name.clone());
    }
    if let Some(pbk) = query.get("pbk") {
        cfg.reality = Some(RealitySettings {
            public_key: pbk.clone(),
            short_id: query.get("sid").cloned().unwrap_or_default(),
            server_name: query.get("sni").cloned().unwrap_or_default(),
            spider_x: query.get("spx").cloned().unwrap_or_default(),
        });
    }

    Ok(profile)
}

/// Parse a VMess share link. Supports both formats:
/// (a) v2rayN/Shadowrocket: `vmess://<base64 JSON>` with v/ps/add/port/id/aid/scy/net/type/host/path/tls/sni/alpn/fp.
/// (b) Plain URI: `vmess://uuid@host:port?security=tls&type=ws&path=&host=#name`.
fn parse_vmess(rest: &str) -> Result<Profile> {
    if !rest.contains('@') {
        return parse_vmess_b64(rest);
    }
    let url = parse_uri("vmess", rest)?;
    let uuid = url.username().to_string();
    let host = url
        .host_str()
        .context("Missing host in VMess URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let query = query_map(&url);

    let security = query
        .get("scy")
        .or_else(|| query.get("security"))
        .map(String::as_str);
    let cfg = VmessConfig {
        uuid,
        alter_id: query.get("aid").and_then(|v| v.parse().ok()).unwrap_or(0),
        security: parse_vmess_security(security),
        tls: extract_tls_common_from_query(&query),
        transport: extract_transport_from_query(&query),
        ..VmessConfig::default()
    };
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Vmess(cfg),
        tags: Vec::new(),
        subscription_id: None,
    })
}

fn parse_vmess_b64(b64: &str) -> Result<Profile> {
    let bytes = decode_b64_lenient(b64).context("VMess base64 payload")?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).context("VMess JSON payload")?;

    let host = v["add"]
        .as_str()
        .context("VMess: missing 'add'")?
        .to_string();
    let port = v["port"]
        .as_u64()
        .or_else(|| v["port"].as_str().and_then(|s| s.parse().ok()))
        .context("VMess: missing 'port'")? as u16;
    let uuid = v["id"].as_str().context("VMess: missing 'id'")?.to_string();
    let name = v["ps"].as_str().unwrap_or(&host).to_string();
    let aid = v["aid"]
        .as_u64()
        .or_else(|| v["aid"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0) as u32;
    let security = v["scy"].as_str();

    let mut tls = TlsCommon::default();
    if v["tls"].as_str() == Some("tls") {
        tls.server_name = v["sni"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| v["host"].as_str().filter(|s| !s.is_empty()))
            .map(|s| s.to_string());
        if let Some(alpn) = v["alpn"].as_str() {
            tls.alpn = parse_alpn(alpn);
        }
        if let Some(fp) = v["fp"].as_str().filter(|s| !s.is_empty()) {
            tls.utls_fingerprint = Some(fp.to_string());
        }
    }

    let net = v["net"].as_str().unwrap_or("tcp");
    let transport = match net {
        "ws" => Some(TransportConfig {
            kind: TransportType::Ws,
            path: v["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            host: v["host"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            service_name: None,
            headers: HashMap::new(),
        }),
        "grpc" => Some(TransportConfig {
            kind: TransportType::Grpc,
            path: None,
            host: None,
            service_name: v["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            headers: HashMap::new(),
        }),
        "h2" | "http" => Some(TransportConfig {
            kind: TransportType::Http,
            path: v["path"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            host: v["host"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            service_name: None,
            headers: HashMap::new(),
        }),
        _ => None,
    };

    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Vmess(VmessConfig {
            uuid,
            alter_id: aid,
            security: parse_vmess_security(security),
            tls,
            transport,
            ..VmessConfig::default()
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

fn parse_vmess_security(s: Option<&str>) -> VmessSecurity {
    match s.unwrap_or("auto") {
        "auto" | "" => VmessSecurity::Auto,
        "none" => VmessSecurity::None,
        "zero" => VmessSecurity::Zero,
        "aes-128-gcm" => VmessSecurity::Aes128Gcm,
        "chacha20-poly1305" => VmessSecurity::Chacha20Poly1305,
        _ => VmessSecurity::Auto,
    }
}

/// Parse `trojan://password@host:port?sni=&type=&path=&host=#name`. TLS is implicit.
fn parse_trojan(rest: &str) -> Result<Profile> {
    let url = parse_uri("trojan", rest)?;
    let password = urlencoding::decode(url.username())?.to_string();
    if password.is_empty() {
        anyhow::bail!("Trojan share link missing password");
    }
    let host = url
        .host_str()
        .context("Missing host in Trojan URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let query = query_map(&url);
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Trojan(TrojanConfig {
            password,
            tls: extract_tls_common_from_query(&query),
            transport: extract_transport_from_query(&query),
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse Shadowsocks share link. Supports SIP002 (`ss://b64(method:pw)@host:port#name`)
/// and the legacy fully-base64 form (`ss://b64(method:pw@host:port)#name`).
fn parse_shadowsocks(rest: &str) -> Result<Profile> {
    // Split fragment off so we don't accidentally base64-decode the name.
    let (body, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    // Strip query (and plugin) — we don't model SS plugins yet.
    let body = match body.find('?') {
        Some(i) => &body[..i],
        None => body,
    };

    let (method, password, host, port) = if let Some(at) = body.rfind('@') {
        let userinfo = &body[..at];
        let hostport = &body[at + 1..];
        let creds = decode_b64_lenient(userinfo)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| userinfo.to_string());
        let (m, p) = creds
            .split_once(':')
            .context("Shadowsocks: expected method:password")?;
        let (h, port_s) = hostport
            .rsplit_once(':')
            .context("Shadowsocks: expected host:port")?;
        let port: u16 = port_s.parse().context("Shadowsocks: invalid port")?;
        (m.to_string(), p.to_string(), h.to_string(), port)
    } else {
        // Legacy: entire body is base64.
        let bytes = decode_b64_lenient(body).context("Shadowsocks base64")?;
        let s = String::from_utf8(bytes).context("Shadowsocks base64 utf8")?;
        let (creds, hostport) = s
            .rsplit_once('@')
            .context("Shadowsocks legacy form: expected method:pw@host:port")?;
        let (m, p) = creds
            .split_once(':')
            .context("Shadowsocks: expected method:password")?;
        let (h, port_s) = hostport
            .rsplit_once(':')
            .context("Shadowsocks: expected host:port")?;
        let port: u16 = port_s.parse().context("Shadowsocks: invalid port")?;
        (m.to_string(), p.to_string(), h.to_string(), port)
    };

    let cipher = parse_shadowsocks_cipher(&method)
        .with_context(|| format!("Unsupported Shadowsocks cipher: {method}"))?;
    let name = match fragment {
        Some(f) => urlencoding::decode(f)?.to_string(),
        None => host.clone(),
    };
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Shadowsocks(ShadowsocksConfig {
            method: cipher,
            password,
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

fn parse_shadowsocks_cipher(s: &str) -> Option<ShadowsocksCipher> {
    Some(match s {
        "chacha20-ietf-poly1305" => ShadowsocksCipher::Chacha20IetfPoly1305,
        "aes-128-gcm" => ShadowsocksCipher::Aes128Gcm,
        "aes-256-gcm" => ShadowsocksCipher::Aes256Gcm,
        "2022-blake3-aes-128-gcm" => ShadowsocksCipher::Blake3Aes128Gcm,
        "2022-blake3-aes-256-gcm" => ShadowsocksCipher::Blake3Aes256Gcm,
        "2022-blake3-chacha20-poly1305" => ShadowsocksCipher::Blake3Chacha20Poly1305,
        "none" | "plain" => ShadowsocksCipher::None,
        _ => return None,
    })
}

/// Parse `hysteria2://password@host:port?obfs=&obfs-password=&sni=&insecure=&alpn=#name`.
fn parse_hysteria2(rest: &str) -> Result<Profile> {
    let url = parse_uri("hysteria2", rest)?;
    let password = urlencoding::decode(url.username())?.to_string();
    if password.is_empty() {
        anyhow::bail!("Hysteria2 share link missing password");
    }
    let host = url
        .host_str()
        .context("Missing host in Hysteria2 URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let query = query_map(&url);

    let obfs = match (query.get("obfs"), query.get("obfs-password")) {
        (Some(kind), Some(p)) if kind == "salamander" => Some(Hysteria2Obfs {
            kind: Hysteria2ObfsType::Salamander,
            password: p.clone(),
        }),
        _ => None,
    };

    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Hysteria2(Hysteria2Config {
            password,
            up_mbps: query.get("up").and_then(|s| s.parse().ok()),
            down_mbps: query.get("down").and_then(|s| s.parse().ok()),
            obfs,
            tls: extract_tls_common_from_query(&query),
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `tuic://uuid:password@host:port?congestion_control=&udp_relay_mode=&alpn=&sni=#name`.
fn parse_tuic(rest: &str) -> Result<Profile> {
    let url = parse_uri("tuic", rest)?;
    let uuid = urlencoding::decode(url.username())?.to_string();
    let password = urlencoding::decode(url.password().unwrap_or(""))?.to_string();
    if uuid.is_empty() || password.is_empty() {
        anyhow::bail!("TUIC share link missing uuid or password");
    }
    let host = url
        .host_str()
        .context("Missing host in TUIC URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let query = query_map(&url);

    let cc = match query.get("congestion_control").map(String::as_str) {
        Some("cubic") => TuicCongestion::Cubic,
        Some("new_reno") | Some("new-reno") | Some("newreno") => TuicCongestion::NewReno,
        _ => TuicCongestion::Bbr,
    };
    let udp = match query.get("udp_relay_mode").map(String::as_str) {
        Some("quic") => TuicUdpRelayMode::Quic,
        _ => TuicUdpRelayMode::Native,
    };
    let zero_rtt = query
        .get("zero_rtt_handshake")
        .map(|s| parse_bool_param(s))
        .unwrap_or(false);

    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Tuic(TuicConfig {
            uuid,
            password,
            congestion_control: cc,
            udp_relay_mode: udp,
            zero_rtt_handshake: zero_rtt,
            tls: extract_tls_common_from_query(&query),
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `socks5://user:pass@host:port#name` (also `socks://`).
fn parse_socks(rest: &str) -> Result<Profile> {
    let url = parse_uri("socks5", rest)?;
    let host = url
        .host_str()
        .context("Missing host in SOCKS URL")?
        .to_string();
    let port = url.port().context("Missing port in SOCKS URL")?;
    let name = fragment_name(&url, &host)?;
    let user = url.username();
    let pass = url.password();
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Socks(SocksConfig {
            version: SocksVersion::V5,
            username: (!user.is_empty())
                .then(|| urlencoding::decode(user).unwrap_or_default().to_string()),
            password: pass.map(|p| urlencoding::decode(p).unwrap_or_default().to_string()),
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `http://user:pass@host:port#name` / `https://...#name`. The `tls` flag
/// is set when the scheme is `https`.
fn parse_http(rest: &str, tls_enabled: bool) -> Result<Profile> {
    let url = parse_uri("http", rest)?;
    let host = url
        .host_str()
        .context("Missing host in HTTP URL")?
        .to_string();
    let port = url.port().unwrap_or(if tls_enabled { 443 } else { 80 });
    let name = fragment_name(&url, &host)?;
    let user = url.username();
    let pass = url.password();
    let tls = if tls_enabled {
        TlsCommon {
            server_name: Some(host.clone()),
            ..TlsCommon::default()
        }
    } else {
        TlsCommon::default()
    };
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Http(HttpConfig {
            username: (!user.is_empty())
                .then(|| urlencoding::decode(user).unwrap_or_default().to_string()),
            password: pass.map(|p| urlencoding::decode(p).unwrap_or_default().to_string()),
            tls,
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `ssh://user@host:port#name` (optional `?password=&private_key_path=`).
fn parse_ssh(rest: &str) -> Result<Profile> {
    let url = parse_uri("ssh", rest)?;
    let host = url
        .host_str()
        .context("Missing host in SSH URL")?
        .to_string();
    let port = url.port().unwrap_or(22);
    let name = fragment_name(&url, &host)?;
    let user = url.username().to_string();
    if user.is_empty() {
        anyhow::bail!("SSH share link missing user");
    }
    let url_pass = url
        .password()
        .map(|p| urlencoding::decode(p).unwrap_or_default().to_string());
    let q = query_map(&url);
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Ssh(SshConfig {
            user,
            password: url_pass.or_else(|| q.get("password").cloned()),
            private_key_path: q.get("private_key_path").cloned(),
            private_key_passphrase: q.get("private_key_passphrase").cloned(),
            ..SshConfig::default()
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `anytls://password@host:port?sni=#name`.
fn parse_anytls(rest: &str) -> Result<Profile> {
    let url = parse_uri("anytls", rest)?;
    let password = urlencoding::decode(url.username())?.to_string();
    if password.is_empty() {
        anyhow::bail!("AnyTLS share link missing password");
    }
    let host = url
        .host_str()
        .context("Missing host in AnyTLS URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let q = query_map(&url);
    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Anytls(AnytlsConfig {
            password,
            tls: extract_tls_common_from_query(&q),
            ..AnytlsConfig::default()
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Parse `shadowtls://stpassword@host:port?version=3&ss-method=&ss-password=&sni=#name`.
/// There is no widely-deployed standard URI for ShadowTLS; this form follows the
/// closest community convention. The inner Shadowsocks cipher and password are
/// required (they configure the detour outbound that carries actual traffic).
fn parse_shadowtls(rest: &str) -> Result<Profile> {
    let url = parse_uri("shadowtls", rest)?;
    let password = urlencoding::decode(url.username())?.to_string();
    let host = url
        .host_str()
        .context("Missing host in ShadowTLS URL")?
        .to_string();
    let port = url.port().unwrap_or(443);
    let name = fragment_name(&url, &host)?;
    let q = query_map(&url);

    let version = match q.get("version").and_then(|s| s.parse::<u8>().ok()) {
        Some(1) => ShadowtlsVersion::V1,
        Some(2) => ShadowtlsVersion::V2,
        _ => ShadowtlsVersion::V3,
    };
    let method = q
        .get("ss-method")
        .or_else(|| q.get("method"))
        .map(String::as_str)
        .and_then(parse_shadowsocks_cipher)
        .unwrap_or(ShadowsocksCipher::Chacha20IetfPoly1305);
    let ss_password = q
        .get("ss-password")
        .or_else(|| q.get("ss_password"))
        .cloned()
        .unwrap_or_default();
    if ss_password.is_empty() {
        anyhow::bail!("ShadowTLS share link missing ss-password (inner Shadowsocks detour)");
    }

    Ok(Profile {
        id: Uuid::new_v4(),
        name,
        address: host,
        port,
        config: ProtocolConfig::Shadowtls(ShadowtlsConfig {
            version,
            password,
            method,
            ss_password,
            tls: extract_tls_common_from_query(&q),
        }),
        tags: Vec::new(),
        subscription_id: None,
    })
}

/// Encode a Profile to a share link URI. Inverse of [`parse_share_link`]:
/// `parse_share_link(encode_share_link(&p)?)` reproduces `p` modulo `id`
/// (which is regenerated on parse) and fields the parser does not extract.
pub fn encode_share_link(profile: &Profile) -> Result<String> {
    match &profile.config {
        ProtocolConfig::Vless(cfg) => Ok(encode_vless(profile, cfg)),
        ProtocolConfig::Vmess(cfg) => Ok(encode_vmess(profile, cfg)),
        ProtocolConfig::Trojan(cfg) => Ok(encode_trojan(profile, cfg)),
        ProtocolConfig::Shadowsocks(cfg) => Ok(encode_shadowsocks(profile, cfg)),
        ProtocolConfig::Hysteria2(cfg) => Ok(encode_hysteria2(profile, cfg)),
        ProtocolConfig::Tuic(cfg) => Ok(encode_tuic(profile, cfg)),
        ProtocolConfig::Socks(cfg) => Ok(encode_socks(profile, cfg)),
        ProtocolConfig::Http(cfg) => Ok(encode_http(profile, cfg)),
        ProtocolConfig::Ssh(cfg) => Ok(encode_ssh(profile, cfg)),
        ProtocolConfig::Anytls(cfg) => Ok(encode_anytls(profile, cfg)),
        ProtocolConfig::Shadowtls(cfg) => Ok(encode_shadowtls(profile, cfg)),
    }
}

fn build_query(pairs: &[(&str, String)]) -> String {
    let parts: Vec<String> = pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

fn fragment_for(name: &str) -> String {
    if name.is_empty() {
        String::new()
    } else {
        format!("#{}", urlencoding::encode(name))
    }
}

fn host_for_uri(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn append_tls_common_query(pairs: &mut Vec<(&'static str, String)>, tls: &TlsCommon) {
    if let Some(reality) = &tls.reality {
        pairs.push(("security", "reality".to_string()));
        if !reality.server_name.is_empty() {
            pairs.push(("sni", reality.server_name.clone()));
        } else if let Some(sni) = &tls.server_name {
            pairs.push(("sni", sni.clone()));
        }
        pairs.push(("pbk", reality.public_key.clone()));
        if !reality.short_id.is_empty() {
            pairs.push(("sid", reality.short_id.clone()));
        }
        if !reality.spider_x.is_empty() {
            pairs.push(("spx", reality.spider_x.clone()));
        }
    } else if let Some(sni) = &tls.server_name {
        pairs.push(("sni", sni.clone()));
    }
    if !tls.alpn.is_empty() {
        pairs.push(("alpn", tls.alpn.join(",")));
    }
    if let Some(fp) = &tls.utls_fingerprint {
        pairs.push(("fp", fp.clone()));
    }
    if tls.insecure {
        pairs.push(("insecure", "1".to_string()));
    }
}

fn append_transport_query(pairs: &mut Vec<(&'static str, String)>, t: &TransportConfig) {
    let kind = match t.kind {
        TransportType::Grpc => "grpc",
        TransportType::Ws => "ws",
        TransportType::Http => "http",
    };
    pairs.push(("type", kind.to_string()));
    if let Some(p) = &t.path {
        pairs.push(("path", p.clone()));
    }
    if let Some(h) = &t.host {
        pairs.push(("host", h.clone()));
    }
    if let Some(s) = &t.service_name {
        pairs.push(("serviceName", s.clone()));
    }
}

fn encode_vless(profile: &Profile, cfg: &VlessConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if cfg.flow == Some(Flow::XtlsRprxVision) {
        pairs.push(("flow", "xtls-rprx-vision".to_string()));
    }
    if cfg.reality.is_some() {
        pairs.push(("security", "reality".to_string()));
    } else if cfg.security == Some(Security::Tls) {
        pairs.push(("security", "tls".to_string()));
    }
    if let Some(fp) = &cfg.fingerprint {
        pairs.push(("fp", fp.clone()));
    }
    if let Some(tt) = &cfg.transport_type {
        let kind = match tt {
            TransportType::Grpc => "grpc",
            TransportType::Ws => "ws",
            TransportType::Http => "http",
        };
        pairs.push(("type", kind.to_string()));
    }
    if let Some(svc) = &cfg.transport_service_name {
        pairs.push(("serviceName", svc.clone()));
    }
    if let Some(reality) = &cfg.reality {
        if !reality.server_name.is_empty() {
            pairs.push(("sni", reality.server_name.clone()));
        }
        pairs.push(("pbk", reality.public_key.clone()));
        if !reality.short_id.is_empty() {
            pairs.push(("sid", reality.short_id.clone()));
        }
        if !reality.spider_x.is_empty() {
            pairs.push(("spx", reality.spider_x.clone()));
        }
    }
    format!(
        "vless://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.uuid),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_vmess(profile: &Profile, cfg: &VmessConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if cfg.security != VmessSecurity::Auto {
        pairs.push(("scy", cfg.security.as_str().to_string()));
    }
    if cfg.alter_id != 0 {
        pairs.push(("aid", cfg.alter_id.to_string()));
    }
    append_tls_common_query(&mut pairs, &cfg.tls);
    if let Some(t) = &cfg.transport {
        append_transport_query(&mut pairs, t);
    }
    format!(
        "vmess://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.uuid),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_trojan(profile: &Profile, cfg: &TrojanConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    append_tls_common_query(&mut pairs, &cfg.tls);
    if let Some(t) = &cfg.transport {
        append_transport_query(&mut pairs, t);
    }
    format!(
        "trojan://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.password),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_shadowsocks(profile: &Profile, cfg: &ShadowsocksConfig) -> String {
    use base64::Engine;
    let creds = format!("{}:{}", cfg.method.as_str(), cfg.password);
    let userinfo = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(creds.as_bytes());
    format!(
        "ss://{}@{}:{}{}",
        userinfo,
        host_for_uri(&profile.address),
        profile.port,
        fragment_for(&profile.name),
    )
}

fn encode_hysteria2(profile: &Profile, cfg: &Hysteria2Config) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if let Some(obfs) = &cfg.obfs {
        let kind = match obfs.kind {
            Hysteria2ObfsType::Salamander => "salamander",
        };
        pairs.push(("obfs", kind.to_string()));
        pairs.push(("obfs-password", obfs.password.clone()));
    }
    if let Some(up) = cfg.up_mbps {
        pairs.push(("up", up.to_string()));
    }
    if let Some(down) = cfg.down_mbps {
        pairs.push(("down", down.to_string()));
    }
    append_tls_common_query(&mut pairs, &cfg.tls);
    format!(
        "hysteria2://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.password),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_tuic(profile: &Profile, cfg: &TuicConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if cfg.congestion_control != TuicCongestion::default() {
        pairs.push((
            "congestion_control",
            cfg.congestion_control.as_str().to_string(),
        ));
    }
    if cfg.udp_relay_mode != TuicUdpRelayMode::default() {
        pairs.push(("udp_relay_mode", cfg.udp_relay_mode.as_str().to_string()));
    }
    if cfg.zero_rtt_handshake {
        pairs.push(("zero_rtt_handshake", "1".to_string()));
    }
    append_tls_common_query(&mut pairs, &cfg.tls);
    format!(
        "tuic://{}:{}@{}:{}{}{}",
        urlencoding::encode(&cfg.uuid),
        urlencoding::encode(&cfg.password),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_socks(profile: &Profile, cfg: &SocksConfig) -> String {
    let mut userinfo = String::new();
    if let Some(u) = &cfg.username {
        userinfo.push_str(&urlencoding::encode(u));
        if let Some(p) = &cfg.password {
            userinfo.push(':');
            userinfo.push_str(&urlencoding::encode(p));
        }
        userinfo.push('@');
    }
    format!(
        "socks5://{}{}:{}{}",
        userinfo,
        host_for_uri(&profile.address),
        profile.port,
        fragment_for(&profile.name),
    )
}

fn encode_http(profile: &Profile, cfg: &HttpConfig) -> String {
    let tls_enabled = cfg.tls.server_name.is_some()
        || !cfg.tls.alpn.is_empty()
        || cfg.tls.utls_fingerprint.is_some()
        || cfg.tls.reality.is_some();
    let scheme = if tls_enabled { "https" } else { "http" };
    let mut userinfo = String::new();
    if let Some(u) = &cfg.username {
        userinfo.push_str(&urlencoding::encode(u));
        if let Some(p) = &cfg.password {
            userinfo.push(':');
            userinfo.push_str(&urlencoding::encode(p));
        }
        userinfo.push('@');
    }
    format!(
        "{}://{}{}:{}{}",
        scheme,
        userinfo,
        host_for_uri(&profile.address),
        profile.port,
        fragment_for(&profile.name),
    )
}

fn encode_ssh(profile: &Profile, cfg: &SshConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if let Some(p) = &cfg.password {
        pairs.push(("password", p.clone()));
    }
    if let Some(p) = &cfg.private_key_path {
        pairs.push(("private_key_path", p.clone()));
    }
    if let Some(p) = &cfg.private_key_passphrase {
        pairs.push(("private_key_passphrase", p.clone()));
    }
    format!(
        "ssh://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.user),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_anytls(profile: &Profile, cfg: &AnytlsConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    append_tls_common_query(&mut pairs, &cfg.tls);
    format!(
        "anytls://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.password),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
}

fn encode_shadowtls(profile: &Profile, cfg: &ShadowtlsConfig) -> String {
    let mut pairs: Vec<(&'static str, String)> = vec![
        ("version", cfg.version.as_u8().to_string()),
        ("ss-method", cfg.method.as_str().to_string()),
        ("ss-password", cfg.ss_password.clone()),
    ];
    append_tls_common_query(&mut pairs, &cfg.tls);
    format!(
        "shadowtls://{}@{}:{}{}{}",
        urlencoding::encode(&cfg.password),
        host_for_uri(&profile.address),
        profile.port,
        build_query(&pairs),
        fragment_for(&profile.name),
    )
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

    fn vless_cfg(profile: &Profile) -> &VlessConfig {
        match &profile.config {
            ProtocolConfig::Vless(c) => c,
            other => panic!("expected VLESS, got {:?}", other.protocol()),
        }
    }

    #[test]
    fn profile_new_defaults() {
        let p = Profile::new_vless(
            "test".to_string(),
            "1.2.3.4".to_string(),
            443,
            "uuid-here".to_string(),
        );
        assert_eq!(p.name, "test");
        assert_eq!(p.protocol(), Protocol::Vless);
        assert_eq!(p.address, "1.2.3.4");
        assert_eq!(p.port, 443);
        let cfg = vless_cfg(&p);
        assert_eq!(cfg.uuid, "uuid-here");
        assert!(cfg.flow.is_none());
        assert!(cfg.security.is_none());
        assert!(cfg.reality.is_none());
        assert!(cfg.transport_type.is_none());
        assert!(cfg.transport_service_name.is_none());
        assert!(cfg.fingerprint.is_none());
        assert!(cfg.ech.is_none());
        assert!(p.tags.is_empty());
        assert_ne!(p.id, Uuid::nil());
    }

    #[test]
    fn settings_default() {
        let s = Settings::default();
        assert_eq!(s.tun_interface, "tun0");
        assert_eq!(s.dns_strategy, DnsStrategy::PreferIpv4);
        assert!(s.default_profile.is_none());
        assert!(!s.auto_connect);
        assert!(!s.kill_switch);
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
    fn config_default() {
        let c = Config::default();
        assert!(c.profiles.is_empty());
        assert_eq!(c.settings.tun_interface, "tun0");
    }

    #[test]
    fn settings_serde_roundtrip_with_kill_switch() {
        let s = Settings {
            kill_switch: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kill_switch\":true"));
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, restored);
        assert!(restored.kill_switch);
    }

    #[test]
    fn settings_serde_kill_switch_defaults_when_absent() {
        // Older configs without the field should deserialize with kill_switch=false.
        let json = r#"{
            "tun_interface": "tun0",
            "dns_strategy": "prefer_ipv4",
            "geo_routing": {},
            "auto_connect": false
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.kill_switch);
    }

    #[test]
    fn config_serde_roundtrip() {
        let mut config = Config::default();
        let mut profile = Profile::new_vless(
            "Example".to_string(),
            "203.0.113.1".to_string(),
            443,
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
        );
        if let ProtocolConfig::Vless(ref mut cfg) = profile.config {
            cfg.security = Some(Security::Reality);
            cfg.reality = Some(RealitySettings {
                public_key: "pk".to_string(),
                short_id: "sid".to_string(),
                server_name: "sni".to_string(),
                spider_x: "/".to_string(),
            });
        }
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
        let cfg = vless_cfg(&p);
        assert!(cfg.flow.is_none());
        assert!(cfg.reality.is_none());
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
    fn config_validate_accepts_valid_config() {
        let mut config = Config::default();
        config.profiles.push(Profile::new_vless(
            "Valid".to_string(),
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
        config.profiles.push(Profile::new_vless(
            "   ".to_string(),
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
        config.profiles.push(Profile::new_vless(
            "Name".to_string(),
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
        config.profiles.push(Profile::new_vless(
            "Name".to_string(),
            "1.2.3.4".to_string(),
            443,
            "  ".to_string(),
        ));
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("vless.uuid must not be empty"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn config_validate_rejects_reality_plus_ech() {
        let mut config = Config::default();
        let mut profile = Profile::new_vless(
            "RealityEch".to_string(),
            "1.2.3.4".to_string(),
            443,
            "uuid".to_string(),
        );
        if let ProtocolConfig::Vless(ref mut cfg) = profile.config {
            cfg.reality = Some(RealitySettings::default());
            cfg.ech = Some(EchSettings {
                enabled: true,
                config: Vec::new(),
            });
        }
        config.profiles.push(profile);
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("reality and ech are mutually exclusive"),
            "Error was: {}",
            err
        );
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
        assert_eq!(profile.protocol(), Protocol::Vless);
        assert_eq!(profile.address, "203.0.113.42");
        assert_eq!(profile.port, 59431);
        assert_eq!(profile.name, "Example-2873vb06");
        let cfg = vless_cfg(&profile);
        assert_eq!(cfg.uuid, "671c62c7-6768-4b98-ac6b-572c9c707be0");
        assert!(cfg.security.is_some());
        let reality = cfg.reality.as_ref().unwrap();
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
        assert_eq!(profile.address, "1.2.3.4");
        assert_eq!(profile.port, 443);
        assert_eq!(profile.name, "Name");
        let cfg = vless_cfg(&profile);
        assert_eq!(cfg.uuid, "uuid");
        assert!(cfg.reality.is_none());
        assert!(cfg.flow.is_none());
        assert!(cfg.fingerprint.is_none());
        assert!(cfg.transport_type.is_none());
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
        let cfg = vless_cfg(&profile);
        assert_eq!(cfg.security, Some(Security::Reality));
        let reality = cfg.reality.as_ref().unwrap();
        assert_eq!(reality.public_key, "pk123");
        assert_eq!(reality.server_name, "sni.test");
        assert!(reality.short_id.is_empty());
        assert!(reality.spider_x.is_empty());
    }

    #[test]
    fn parse_vless_url_encoded_spx() {
        let uri = "vless://uuid@1.2.3.4?pbk=k&spx=%2Fpath%2Fhere#N";
        let profile = parse_share_link(uri).unwrap();
        let cfg = vless_cfg(&profile);
        assert_eq!(cfg.reality.as_ref().unwrap().spider_x, "/path/here");
    }

    #[test]
    fn legacy_vless_json_deserializes_into_new_shape() {
        // Old config files (pre-ProtocolConfig refactor) used the same flat
        // layout we now keep on Vless variant via #[serde(flatten)]; ensure
        // a snapshot of the historic shape parses cleanly.
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "name": "Legacy",
            "protocol": "vless",
            "address": "1.1.1.1",
            "port": 443,
            "uuid": "legacy-uuid",
            "flow": "xtls-rprx-vision",
            "security": "reality",
            "reality": {
                "public_key": "pk",
                "short_id": "sid",
                "server_name": "sni",
                "spider_x": "/"
            },
            "transport_type": "grpc",
            "transport_service_name": "svc",
            "fingerprint": "chrome",
            "tags": ["legacy"]
        }"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.protocol(), Protocol::Vless);
        let cfg = vless_cfg(&p);
        assert_eq!(cfg.uuid, "legacy-uuid");
        assert_eq!(cfg.flow, Some(Flow::XtlsRprxVision));
        assert_eq!(cfg.security, Some(Security::Reality));
        assert!(cfg.reality.is_some());
        assert_eq!(cfg.transport_type, Some(TransportType::Grpc));
        assert_eq!(cfg.transport_service_name.as_deref(), Some("svc"));
        assert_eq!(cfg.fingerprint.as_deref(), Some("chrome"));
        assert_eq!(p.tags, vec!["legacy".to_string()]);
    }

    #[test]
    fn vmess_profile_roundtrip() {
        let profile = Profile {
            id: Uuid::nil(),
            name: "VMess".to_string(),
            address: "1.1.1.1".to_string(),
            port: 443,
            config: ProtocolConfig::Vmess(VmessConfig {
                uuid: "vm-uuid".to_string(),
                alter_id: 0,
                security: VmessSecurity::Aes128Gcm,
                global_padding: None,
                tls: TlsCommon {
                    server_name: Some("sni".to_string()),
                    ech: Some(EchSettings {
                        enabled: true,
                        config: Vec::new(),
                    }),
                    ..TlsCommon::default()
                },
                transport: None,
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("\"protocol\":\"vmess\""));
        assert!(json.contains("\"ech\""));
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile, restored);
    }

    #[test]
    fn dedup_key_distinguishes_protocols() {
        let v = Profile::new_vless("V".into(), "1.1.1.1".into(), 443, "shared-uuid".to_string());
        let m = Profile {
            id: Uuid::nil(),
            name: "M".into(),
            address: "1.1.1.1".into(),
            port: 443,
            config: ProtocolConfig::Vmess(VmessConfig {
                uuid: "shared-uuid".to_string(),
                ..VmessConfig::default()
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_ne!(
            v.dedup_key(),
            m.dedup_key(),
            "same UUID on different protocols must dedup separately"
        );
    }

    #[test]
    fn parse_rejects_unknown_scheme() {
        let result = parse_share_link("snake-oil://whatever");
        assert!(result.is_err());
    }

    #[test]
    fn parse_vless_missing_host_fails() {
        let result = parse_share_link("vless://");
        assert!(result.is_err());
    }

    // ---- VMess ----

    #[test]
    fn parse_vmess_uri_form() {
        let uri = "vmess://vm-uuid@1.1.1.1:443?security=tls&type=ws&path=/ws&host=host.example&sni=sni.example#VMess-1";
        let p = parse_share_link(uri).unwrap();
        assert_eq!(p.protocol(), Protocol::Vmess);
        assert_eq!(p.address, "1.1.1.1");
        assert_eq!(p.port, 443);
        assert_eq!(p.name, "VMess-1");
        let ProtocolConfig::Vmess(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.uuid, "vm-uuid");
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
        let t = cfg.transport.as_ref().unwrap();
        assert_eq!(t.kind, TransportType::Ws);
        assert_eq!(t.path.as_deref(), Some("/ws"));
        assert_eq!(t.host.as_deref(), Some("host.example"));
    }

    #[test]
    fn parse_vmess_b64_json_form() {
        use base64::Engine;
        let body = serde_json::json!({
            "v": "2", "ps": "VMessB64", "add": "1.2.3.4", "port": "10086",
            "id": "vm-id", "aid": "0", "scy": "aes-128-gcm",
            "net": "ws", "type": "none", "host": "h.example", "path": "/wp",
            "tls": "tls", "sni": "sni.example", "alpn": "h2,http/1.1", "fp": "chrome",
        });
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&body).unwrap());
        let p = parse_share_link(&format!("vmess://{}", encoded)).unwrap();
        assert_eq!(p.name, "VMessB64");
        assert_eq!(p.address, "1.2.3.4");
        assert_eq!(p.port, 10086);
        let ProtocolConfig::Vmess(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.uuid, "vm-id");
        assert_eq!(cfg.security, VmessSecurity::Aes128Gcm);
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
        assert_eq!(cfg.tls.alpn, vec!["h2".to_string(), "http/1.1".to_string()]);
        assert_eq!(cfg.tls.utls_fingerprint.as_deref(), Some("chrome"));
        let t = cfg.transport.as_ref().unwrap();
        assert_eq!(t.kind, TransportType::Ws);
        assert_eq!(t.path.as_deref(), Some("/wp"));
    }

    // ---- Trojan ----

    #[test]
    fn parse_trojan_basic() {
        let uri = "trojan://secret@trojan.example:443?sni=sni.example&type=ws&path=/p#Trojan-1";
        let p = parse_share_link(uri).unwrap();
        assert_eq!(p.protocol(), Protocol::Trojan);
        let ProtocolConfig::Trojan(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.password, "secret");
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
        assert_eq!(cfg.transport.as_ref().unwrap().kind, TransportType::Ws);
    }

    #[test]
    fn parse_trojan_url_decodes_password() {
        let uri = "trojan://hello%20world@trojan.example:443#T";
        let p = parse_share_link(uri).unwrap();
        let ProtocolConfig::Trojan(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.password, "hello world");
    }

    // ---- Shadowsocks ----

    #[test]
    fn parse_shadowsocks_sip002() {
        use base64::Engine;
        let creds = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("aes-256-gcm:ssecret");
        let uri = format!("ss://{}@ss.example:8388#SS-1", creds);
        let p = parse_share_link(&uri).unwrap();
        assert_eq!(p.protocol(), Protocol::Shadowsocks);
        let ProtocolConfig::Shadowsocks(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.method, ShadowsocksCipher::Aes256Gcm);
        assert_eq!(cfg.password, "ssecret");
        assert_eq!(p.address, "ss.example");
        assert_eq!(p.port, 8388);
        assert_eq!(p.name, "SS-1");
    }

    #[test]
    fn parse_shadowsocks_legacy_form() {
        use base64::Engine;
        let blob = base64::engine::general_purpose::STANDARD
            .encode("chacha20-ietf-poly1305:pw@1.1.1.1:8388");
        let uri = format!("ss://{}#Legacy", blob);
        let p = parse_share_link(&uri).unwrap();
        let ProtocolConfig::Shadowsocks(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.method, ShadowsocksCipher::Chacha20IetfPoly1305);
        assert_eq!(cfg.password, "pw");
        assert_eq!(p.address, "1.1.1.1");
        assert_eq!(p.port, 8388);
        assert_eq!(p.name, "Legacy");
    }

    #[test]
    fn parse_shadowsocks_unsupported_cipher_fails() {
        use base64::Engine;
        let creds = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("aes-128-cfb:pw");
        let uri = format!("ss://{}@1.2.3.4:8388#X", creds);
        assert!(parse_share_link(&uri).is_err());
    }

    // ---- Hysteria2 ----

    #[test]
    fn parse_hysteria2_with_obfs_and_alias() {
        let uri = "hy2://hp@hy.example:443?obfs=salamander&obfs-password=ob&sni=sni.example&insecure=1&alpn=h3#H2";
        let p = parse_share_link(uri).unwrap();
        assert_eq!(p.protocol(), Protocol::Hysteria2);
        let ProtocolConfig::Hysteria2(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.password, "hp");
        let obfs = cfg.obfs.as_ref().unwrap();
        assert_eq!(obfs.kind, Hysteria2ObfsType::Salamander);
        assert_eq!(obfs.password, "ob");
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
        assert!(cfg.tls.insecure);
        assert_eq!(cfg.tls.alpn, vec!["h3".to_string()]);
    }

    // ---- TUIC ----

    #[test]
    fn parse_tuic_basic() {
        let uri = "tuic://tu-uuid:tp@tuic.example:443?congestion_control=cubic&udp_relay_mode=quic&zero_rtt_handshake=1&alpn=h3&sni=sni.example#TUIC";
        let p = parse_share_link(uri).unwrap();
        assert_eq!(p.protocol(), Protocol::Tuic);
        let ProtocolConfig::Tuic(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.uuid, "tu-uuid");
        assert_eq!(cfg.password, "tp");
        assert_eq!(cfg.congestion_control, TuicCongestion::Cubic);
        assert_eq!(cfg.udp_relay_mode, TuicUdpRelayMode::Quic);
        assert!(cfg.zero_rtt_handshake);
        assert_eq!(cfg.tls.alpn, vec!["h3".to_string()]);
    }

    // ---- SOCKS / HTTP / SSH / AnyTLS / ShadowTLS ----

    #[test]
    fn parse_socks5_with_auth() {
        let p = parse_share_link("socks5://u:p@s.example:1080#S5").unwrap();
        assert_eq!(p.protocol(), Protocol::Socks);
        let ProtocolConfig::Socks(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.version, SocksVersion::V5);
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
    }

    #[test]
    fn parse_https_enables_tls() {
        let plain = parse_share_link("http://h.example:8080#HTTP").unwrap();
        let ProtocolConfig::Http(cfg) = &plain.config else {
            panic!()
        };
        assert!(!tls_has_anything(&cfg.tls));

        let secure = parse_share_link("https://u:p@h.example#HTTPS").unwrap();
        assert_eq!(secure.port, 443);
        let ProtocolConfig::Http(cfg) = &secure.config else {
            panic!()
        };
        assert!(cfg.tls.server_name.is_some());
        assert_eq!(cfg.username.as_deref(), Some("u"));
        assert_eq!(cfg.password.as_deref(), Some("p"));
    }

    fn tls_has_anything(tls: &TlsCommon) -> bool {
        tls.server_name.is_some()
            || tls.insecure
            || !tls.alpn.is_empty()
            || tls.utls_fingerprint.is_some()
            || tls.reality.is_some()
            || tls.ech.is_some()
    }

    #[test]
    fn parse_ssh_with_password_in_query() {
        let p = parse_share_link("ssh://alice@ssh.example:2222?password=p#SSH").unwrap();
        let ProtocolConfig::Ssh(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.user, "alice");
        assert_eq!(cfg.password.as_deref(), Some("p"));
        assert_eq!(p.port, 2222);
    }

    #[test]
    fn parse_anytls_basic() {
        let p = parse_share_link("anytls://pp@a.example:443?sni=sni.example#A").unwrap();
        let ProtocolConfig::Anytls(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.password, "pp");
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
    }

    #[test]
    fn parse_shadowtls_basic() {
        let uri = "shadowtls://stp@st.example:443?version=3&ss-method=2022-blake3-aes-256-gcm&ss-password=isp&sni=sni.example#ST";
        let p = parse_share_link(uri).unwrap();
        let ProtocolConfig::Shadowtls(cfg) = &p.config else {
            panic!()
        };
        assert_eq!(cfg.version, ShadowtlsVersion::V3);
        assert_eq!(cfg.password, "stp");
        assert_eq!(cfg.method, ShadowsocksCipher::Blake3Aes256Gcm);
        assert_eq!(cfg.ss_password, "isp");
        assert_eq!(cfg.tls.server_name.as_deref(), Some("sni.example"));
    }

    #[test]
    fn parse_shadowtls_requires_inner_ss_password() {
        // No ss-password means we can't build the SS detour.
        let uri = "shadowtls://stp@st.example:443?version=3&ss-method=aes-128-gcm#X";
        assert!(parse_share_link(uri).is_err());
    }

    // ---- Round-trip: encode → parse must reproduce the input profile
    // (modulo `id`, which is regenerated on every parse).

    fn assert_roundtrip(mut profile: Profile) {
        let link = encode_share_link(&profile).expect("encode");
        let mut parsed =
            parse_share_link(&link).unwrap_or_else(|e| panic!("parse failed for `{link}`: {e}"));
        parsed.id = profile.id;
        // `tags` and `subscription_id` are not transported via share links;
        // strip them from both sides for the comparison.
        profile.tags.clear();
        profile.subscription_id = None;
        assert_eq!(parsed, profile, "round-trip mismatch for `{link}`");
    }

    #[test]
    fn encode_vless_roundtrip_plain() {
        let p = Profile::new_vless(
            "VLESS plain".to_string(),
            "1.1.1.1".to_string(),
            443,
            "vless-uuid".to_string(),
        );
        assert_roundtrip(p);
    }

    #[test]
    fn encode_vless_roundtrip_reality() {
        let mut p = Profile::new_vless(
            "VLESS reality".to_string(),
            "rt.example".to_string(),
            443,
            "vless-uuid".to_string(),
        );
        let ProtocolConfig::Vless(ref mut cfg) = p.config else {
            unreachable!()
        };
        cfg.flow = Some(Flow::XtlsRprxVision);
        cfg.security = Some(Security::Reality);
        cfg.fingerprint = Some("chrome".to_string());
        cfg.reality = Some(RealitySettings {
            public_key: "pbk-value".to_string(),
            short_id: "sid-value".to_string(),
            server_name: "rt.example".to_string(),
            spider_x: "/spx".to_string(),
        });
        cfg.transport_type = Some(TransportType::Grpc);
        cfg.transport_service_name = Some("svc".to_string());
        assert_roundtrip(p);
    }

    #[test]
    fn encode_vmess_roundtrip_with_tls_and_ws() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "VMess WS".to_string(),
            address: "vm.example".to_string(),
            port: 8443,
            config: ProtocolConfig::Vmess(VmessConfig {
                uuid: "vm-uuid".to_string(),
                alter_id: 0,
                security: VmessSecurity::Aes128Gcm,
                tls: TlsCommon {
                    server_name: Some("sni.example".to_string()),
                    alpn: vec!["h2".to_string(), "http/1.1".to_string()],
                    utls_fingerprint: Some("chrome".to_string()),
                    ..TlsCommon::default()
                },
                transport: Some(TransportConfig {
                    kind: TransportType::Ws,
                    path: Some("/ws".to_string()),
                    host: Some("host.example".to_string()),
                    service_name: None,
                    headers: HashMap::new(),
                }),
                ..VmessConfig::default()
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_trojan_roundtrip() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "Trojan-1".to_string(),
            address: "tr.example".to_string(),
            port: 443,
            config: ProtocolConfig::Trojan(TrojanConfig {
                password: "hello world".to_string(),
                tls: TlsCommon {
                    server_name: Some("sni.example".to_string()),
                    ..TlsCommon::default()
                },
                transport: None,
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_shadowsocks_roundtrip_sip002() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "SS AEAD-2022".to_string(),
            address: "ss.example".to_string(),
            port: 8388,
            config: ProtocolConfig::Shadowsocks(ShadowsocksConfig {
                method: ShadowsocksCipher::Blake3Aes128Gcm,
                password: "p4ssw0rd".to_string(),
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_hysteria2_roundtrip_with_obfs() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "Hy2".to_string(),
            address: "hy.example".to_string(),
            port: 443,
            config: ProtocolConfig::Hysteria2(Hysteria2Config {
                password: "secret".to_string(),
                up_mbps: Some(100),
                down_mbps: Some(500),
                obfs: Some(Hysteria2Obfs {
                    kind: Hysteria2ObfsType::Salamander,
                    password: "obfs-pw".to_string(),
                }),
                tls: TlsCommon {
                    server_name: Some("hy.example".to_string()),
                    insecure: true,
                    ..TlsCommon::default()
                },
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_tuic_roundtrip() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "TUIC".to_string(),
            address: "tuic.example".to_string(),
            port: 443,
            config: ProtocolConfig::Tuic(TuicConfig {
                uuid: "tuic-uuid".to_string(),
                password: "tuic-pass".to_string(),
                congestion_control: TuicCongestion::Cubic,
                udp_relay_mode: TuicUdpRelayMode::Quic,
                zero_rtt_handshake: true,
                tls: TlsCommon {
                    server_name: Some("tuic.example".to_string()),
                    alpn: vec!["h3".to_string()],
                    ..TlsCommon::default()
                },
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_socks_roundtrip_with_auth() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "Socks".to_string(),
            address: "socks.example".to_string(),
            port: 1080,
            config: ProtocolConfig::Socks(SocksConfig {
                version: SocksVersion::V5,
                username: Some("alice".to_string()),
                password: Some("pa ss".to_string()),
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_http_roundtrip_https_tls() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "HTTPS proxy".to_string(),
            address: "proxy.example".to_string(),
            port: 443,
            config: ProtocolConfig::Http(HttpConfig {
                username: Some("u".to_string()),
                password: Some("p".to_string()),
                tls: TlsCommon {
                    server_name: Some("proxy.example".to_string()),
                    ..TlsCommon::default()
                },
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_ssh_roundtrip_key_path() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "SSH".to_string(),
            address: "ssh.example".to_string(),
            port: 22,
            config: ProtocolConfig::Ssh(SshConfig {
                user: "root".to_string(),
                password: None,
                private_key_path: Some("/home/me/.ssh/id_ed25519".to_string()),
                private_key_passphrase: Some("kp".to_string()),
                ..SshConfig::default()
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_anytls_roundtrip() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "AnyTLS".to_string(),
            address: "at.example".to_string(),
            port: 443,
            config: ProtocolConfig::Anytls(AnytlsConfig {
                password: "anytls-pw".to_string(),
                tls: TlsCommon {
                    server_name: Some("at.example".to_string()),
                    ..TlsCommon::default()
                },
                ..AnytlsConfig::default()
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }

    #[test]
    fn encode_shadowtls_roundtrip_v3() {
        let p = Profile {
            id: Uuid::new_v4(),
            name: "ShadowTLS v3".to_string(),
            address: "st.example".to_string(),
            port: 443,
            config: ProtocolConfig::Shadowtls(ShadowtlsConfig {
                version: ShadowtlsVersion::V3,
                password: "stls-pw".to_string(),
                method: ShadowsocksCipher::Aes128Gcm,
                ss_password: "inner-ss-pw".to_string(),
                tls: TlsCommon {
                    server_name: Some("st.example".to_string()),
                    ..TlsCommon::default()
                },
            }),
            tags: Vec::new(),
            subscription_id: None,
        };
        assert_roundtrip(p);
    }
}
