//! DNS configuration: strategy, servers, rules, and validation.
//!
//! `DnsConfig` is the source of truth used both by sing-box config generation
//! (`singbox::config`) and by the TUI's settings overlay. The legacy
//! top-level `settings.dns_strategy` field is mirrored from `dns.strategy`
//! by `Config::migrate_legacy_dns_strategy` for backward compatibility.

use serde::{Deserialize, Serialize};

// (body appended by sed)
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
