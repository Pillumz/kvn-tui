use serde_json::{Map, Value, json};
use std::path::PathBuf;

use crate::config::profile::{
    AnytlsConfig, DnsConfig, DnsServer, HttpConfig, Hysteria2Config, Profile, ProtocolConfig,
    RoutingMode, Settings, ShadowsocksConfig, ShadowtlsConfig, ShadowtlsVersion, SocksConfig,
    SshConfig, TlsCommon, TransportConfig, TransportType, TrojanConfig, TuicConfig, VlessConfig,
    VmessConfig,
};

/// Availability of local geoip/geosite rule-sets used when building routes.
/// Each region is present only when both files exist.
#[derive(Debug, Clone, Default)]
pub struct GeoAvailability {
    pub ru: Option<(PathBuf, PathBuf)>,
    pub cn: Option<(PathBuf, PathBuf)>,
    pub ir: Option<(PathBuf, PathBuf)>,
}

impl GeoAvailability {
    /// All rule-sets are available with dummy paths for tests.
    #[cfg(test)]
    pub fn all() -> Self {
        Self {
            ru: Some((
                PathBuf::from("/geo/geoip-ru.srs"),
                PathBuf::from("/geo/geosite-category-ru.srs"),
            )),
            cn: Some((
                PathBuf::from("/geo/geoip-cn.srs"),
                PathBuf::from("/geo/geosite-cn.srs"),
            )),
            ir: Some((
                PathBuf::from("/geo/geoip-ir.srs"),
                PathBuf::from("/geo/geosite-category-ir.srs"),
            )),
        }
    }
}

/// Generate a complete sing-box JSON configuration from a profile.
/// Uses the modern sing-box 1.12+ format.
pub fn generate_config(
    profile: &Profile,
    settings: &Settings,
    geo: &GeoAvailability,
) -> anyhow::Result<Value> {
    let mut proxy_outbounds = build_outbound(profile)?;
    proxy_outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    let (route, rule_sets) = build_route(&settings.geo_routing.mode(), &settings.dns, geo);
    let dns = build_dns(&settings.dns);

    let mut cache_file = json!({ "enabled": true });
    if settings.dns.fakeip_enabled && settings.dns.fakeip_server().is_some() {
        cache_file["store_fakeip"] = json!(true);
    }

    let mut config = json!({
        "log": {
            "level": "debug",
            "output": crate::infra::paths::singbox_log_path().to_string_lossy(),
            "timestamp": true
        },
        "dns": dns,
        "inbounds": [
            {
                "type": "tun",
                "tag": "tun-in",
                "interface_name": settings.tun_interface.clone(),
                "address": ["172.19.0.1/30"],
                "mtu": 1420,
                "auto_route": true,
                "strict_route": true,
                "endpoint_independent_nat": true,
                "stack": "gvisor"
            }
        ],
        "outbounds": proxy_outbounds,
        "route": route,
        "experimental": {
            "cache_file": cache_file
        }
    });

    // Merge rule_sets into route if any exist.
    if !rule_sets.is_empty() {
        config["route"]["rule_set"] = json!(rule_sets);
    }

    Ok(config)
}

/// Build the `dns` section from user configuration. Maps onto sing-box 1.12's
/// `dns` schema: server entries carry their own fake-IP ranges (no legacy
/// top-level `dns.fakeip` block), and when the user toggles fake-IP on we
/// auto-prepend an `A`/`AAAA`-routing rule and flip `independent_cache` so the
/// fake-IP server actually receives queries and its mappings stay separate
/// from upstream caches.
fn build_dns(dns: &DnsConfig) -> Value {
    let servers: Vec<Value> = dns.servers.iter().map(build_dns_server).collect();
    let mut block = Map::new();
    block.insert("servers".to_string(), Value::Array(servers));

    let mut rules: Vec<Value> = dns.rules.iter().map(build_dns_rule).collect();
    if dns.fakeip_enabled {
        if let Some(server) = dns.fakeip_server() {
            let tag = server.tag().to_string();
            let already_routed = dns.rules.iter().any(|r| r.server == tag);
            if !already_routed {
                rules.insert(
                    0,
                    json!({
                        "query_type": ["A", "AAAA"],
                        "server": tag,
                    }),
                );
            }
        }
    }
    if !rules.is_empty() {
        block.insert("rules".to_string(), Value::Array(rules));
    }

    block.insert("final".to_string(), json!(dns.final_server));
    block.insert("strategy".to_string(), json!(dns.strategy.as_str()));
    if dns.fakeip_enabled && dns.fakeip_server().is_some() {
        block.insert("independent_cache".to_string(), json!(true));
    }
    Value::Object(block)
}

fn build_dns_server(server: &DnsServer) -> Value {
    match server {
        DnsServer::Local { tag } => json!({ "tag": tag, "type": "local" }),
        DnsServer::Udp {
            tag,
            server,
            server_port,
        } => server_with_port("udp", tag, server, *server_port, None),
        DnsServer::Tcp {
            tag,
            server,
            server_port,
        } => server_with_port("tcp", tag, server, *server_port, None),
        DnsServer::Tls {
            tag,
            server,
            server_port,
        } => server_with_port("tls", tag, server, *server_port, None),
        DnsServer::Https {
            tag,
            server,
            server_port,
            path,
        } => server_with_port("https", tag, server, *server_port, Some(path.as_str())),
        DnsServer::Quic {
            tag,
            server,
            server_port,
        } => server_with_port("quic", tag, server, *server_port, None),
        DnsServer::FakeIp {
            tag,
            inet4_range,
            inet6_range,
        } => json!({
            "tag": tag,
            "type": "fakeip",
            "inet4_range": inet4_range,
            "inet6_range": inet6_range,
        }),
    }
}

fn server_with_port(
    ty: &str,
    tag: &str,
    server: &str,
    port: Option<u16>,
    doh_path: Option<&str>,
) -> Value {
    let mut obj = Map::new();
    obj.insert("tag".to_string(), json!(tag));
    obj.insert("type".to_string(), json!(ty));
    obj.insert("server".to_string(), json!(server));
    if let Some(p) = port {
        obj.insert("server_port".to_string(), json!(p));
    }
    if let Some(path) = doh_path {
        obj.insert("path".to_string(), json!(path));
    }
    Value::Object(obj)
}

fn build_dns_rule(rule: &crate::config::profile::DnsRule) -> Value {
    let mut obj = Map::new();
    if !rule.domain.is_empty() {
        obj.insert("domain".to_string(), json!(rule.domain));
    }
    if !rule.domain_suffix.is_empty() {
        obj.insert("domain_suffix".to_string(), json!(rule.domain_suffix));
    }
    if !rule.domain_keyword.is_empty() {
        obj.insert("domain_keyword".to_string(), json!(rule.domain_keyword));
    }
    if !rule.domain_regex.is_empty() {
        obj.insert("domain_regex".to_string(), json!(rule.domain_regex));
    }
    if !rule.rule_set.is_empty() {
        obj.insert("rule_set".to_string(), json!(rule.rule_set));
    }
    obj.insert("server".to_string(), json!(rule.server));
    if rule.disable_cache {
        obj.insert("disable_cache".to_string(), json!(true));
    }
    Value::Object(obj)
}

/// Build route object and local rule-sets based on routing mode.
/// Returns (route_value, rule_sets_vec).
fn build_route(
    routing_mode: &RoutingMode,
    dns: &DnsConfig,
    geo: &GeoAvailability,
) -> (Value, Vec<Value>) {
    let mut rules = vec![
        json!({
            "ip_version": 6,
            "action": "reject"
        }),
        json!({
            "inbound": ["tun-in"],
            "port": 53,
            "action": "hijack-dns"
        }),
        json!({
            "ip_cidr": ["172.19.0.0/30"],
            "outbound": "direct"
        }),
    ];

    let mut rule_sets: Vec<Value> = Vec::new();

    match routing_mode {
        RoutingMode::Global => {}
        RoutingMode::BypassRu => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_ru, geosite_ru)) = &geo.ru {
                rules.push(json!({
                    "rule_set": ["geosite-category-ru"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-category-ru",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_ru
                }));
                rules.push(json!({
                    "rule_set": ["geoip-ru"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-ru",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_ru
                }));
            }
        }
        RoutingMode::OnlyRu => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_ru, geosite_ru)) = &geo.ru {
                rules.push(json!({
                    "rule_set": ["geosite-category-ru"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-category-ru",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_ru
                }));
                rules.push(json!({
                    "rule_set": ["geoip-ru"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-ru",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_ru
                }));
            }
        }
        RoutingMode::BypassCn => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_cn, geosite_cn)) = &geo.cn {
                rules.push(json!({
                    "rule_set": ["geosite-cn"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-cn",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_cn
                }));
                rules.push(json!({
                    "rule_set": ["geoip-cn"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-cn",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_cn
                }));
            }
        }
        RoutingMode::OnlyCn => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_cn, geosite_cn)) = &geo.cn {
                rules.push(json!({
                    "rule_set": ["geosite-cn"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-cn",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_cn
                }));
                rules.push(json!({
                    "rule_set": ["geoip-cn"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-cn",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_cn
                }));
            }
        }
        RoutingMode::BypassIr => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_ir, geosite_ir)) = &geo.ir {
                rules.push(json!({
                    "rule_set": ["geosite-category-ir"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-category-ir",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_ir
                }));
                rules.push(json!({
                    "rule_set": ["geoip-ir"],
                    "outbound": "direct"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-ir",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_ir
                }));
            }
        }
        RoutingMode::OnlyIr => {
            rules.push(json!({
                "ip_is_private": true,
                "outbound": "direct"
            }));
            if let Some((geoip_ir, geosite_ir)) = &geo.ir {
                rules.push(json!({
                    "rule_set": ["geosite-category-ir"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geosite-category-ir",
                    "type": "local",
                    "format": "binary",
                    "path": geosite_ir
                }));
                rules.push(json!({
                    "rule_set": ["geoip-ir"],
                    "outbound": "proxy"
                }));
                rule_sets.push(json!({
                    "tag": "geoip-ir",
                    "type": "local",
                    "format": "binary",
                    "path": geoip_ir
                }));
            }
        }
    }

    let final_outbound = match routing_mode {
        RoutingMode::OnlyRu | RoutingMode::OnlyCn | RoutingMode::OnlyIr => "direct",
        _ => "proxy",
    };

    // `default_mark` tags every packet sing-box sends to the network with a
    // Linux fwmark. The kvn-tui kill switch's nft ruleset allowlists this mark,
    // so traffic from sing-box's `direct` outbound (used by Bypass/Only routing
    // modes) can reach the physical interface while everything else is dropped.
    let route = json!({
        "default_domain_resolver": {
            "server": dns.final_server,
            "strategy": dns.strategy.as_str(),
        },
        "rules": rules,
        "auto_detect_interface": true,
        "default_mark": 666,
        "final": final_outbound
    });

    (route, rule_sets)
}

/// Build the proxy outbound list for a profile. Most protocols return a
/// single outbound tagged `proxy`; ShadowTLS returns two (the wrapper plus
/// an inner Shadowsocks detour, with the SS half tagged `proxy`).
fn build_outbound(profile: &Profile) -> anyhow::Result<Vec<Value>> {
    let outbounds = match &profile.config {
        ProtocolConfig::Vless(cfg) => vec![build_vless_outbound(profile, cfg)?],
        ProtocolConfig::Vmess(cfg) => vec![build_vmess_outbound(profile, cfg)?],
        ProtocolConfig::Trojan(cfg) => vec![build_trojan_outbound(profile, cfg)?],
        ProtocolConfig::Shadowsocks(cfg) => vec![build_shadowsocks_outbound(profile, cfg)?],
        ProtocolConfig::Hysteria2(cfg) => vec![build_hysteria2_outbound(profile, cfg)?],
        ProtocolConfig::Tuic(cfg) => vec![build_tuic_outbound(profile, cfg)?],
        ProtocolConfig::Shadowtls(cfg) => build_shadowtls_outbounds(profile, cfg)?,
        ProtocolConfig::Anytls(cfg) => vec![build_anytls_outbound(profile, cfg)?],
        ProtocolConfig::Socks(cfg) => vec![build_socks_outbound(profile, cfg)?],
        ProtocolConfig::Http(cfg) => vec![build_http_outbound(profile, cfg)?],
        ProtocolConfig::Ssh(cfg) => vec![build_ssh_outbound(profile, cfg)?],
    };
    Ok(outbounds)
}

/// Render the sing-box 1.12 `tls` block from [`TlsCommon`].
/// `default_sni` is used when `tls.server_name` is unset (typically the
/// profile address). `default_alpn` is used when `tls.alpn` is empty
/// (e.g. `["h3"]` for QUIC-based protocols).
fn build_tls_block(default_sni: &str, default_alpn: &[&str], tls: &TlsCommon) -> Value {
    let mut block = Map::new();
    block.insert("enabled".to_string(), json!(true));
    let sni = tls.server_name.as_deref().unwrap_or(default_sni);
    block.insert("server_name".to_string(), json!(sni));
    if tls.insecure {
        block.insert("insecure".to_string(), json!(true));
    }
    let alpn: Vec<&str> = if tls.alpn.is_empty() {
        default_alpn.to_vec()
    } else {
        tls.alpn.iter().map(String::as_str).collect()
    };
    if !alpn.is_empty() {
        block.insert("alpn".to_string(), json!(alpn));
    }
    if let Some(fp) = tls.utls_fingerprint.as_deref() {
        block.insert(
            "utls".to_string(),
            json!({ "enabled": true, "fingerprint": fp }),
        );
    }
    if let Some(reality) = &tls.reality {
        block.insert("server_name".to_string(), json!(reality.server_name));
        block.insert(
            "reality".to_string(),
            json!({
                "enabled": true,
                "public_key": reality.public_key,
                "short_id": reality.short_id,
            }),
        );
    }
    if let Some(ech) = tls.ech.as_ref().filter(|e| e.enabled) {
        let mut ech_block = json!({ "enabled": true });
        if !ech.config.is_empty() {
            ech_block["config"] = json!(ech.config);
        }
        block.insert("ech".to_string(), ech_block);
    }
    Value::Object(block)
}

/// Render the sing-box 1.12 `transport` block from [`TransportConfig`].
fn build_transport_block(t: &TransportConfig) -> Value {
    let mut obj = Map::new();
    obj.insert("type".to_string(), json!(transport_type_str(&t.kind)));
    if let Some(path) = t.path.as_deref() {
        obj.insert("path".to_string(), json!(path));
    }
    if let Some(host) = t.host.as_deref() {
        obj.insert("host".to_string(), json!(host));
    }
    if let Some(service_name) = t.service_name.as_deref() {
        obj.insert("service_name".to_string(), json!(service_name));
    }
    if t.kind == TransportType::Grpc {
        obj.entry("idle_timeout").or_insert(json!("15s"));
        obj.entry("ping_timeout").or_insert(json!("15s"));
    }
    if !t.headers.is_empty() {
        obj.insert("headers".to_string(), json!(t.headers));
    }
    Value::Object(obj)
}

fn transport_type_str(kind: &TransportType) -> &'static str {
    match kind {
        TransportType::Grpc => "grpc",
        TransportType::Ws => "ws",
        TransportType::Http => "http",
    }
}

/// Build VLESS outbound with optional REALITY / XTLS Vision / ECH.
fn build_vless_outbound(profile: &Profile, cfg: &VlessConfig) -> anyhow::Result<Value> {
    let tls = if let Some(ref reality) = cfg.reality {
        let fingerprint = cfg.fingerprint.as_deref().unwrap_or("chrome");
        json!({
            "enabled": true,
            "server_name": reality.server_name,
            "utls": { "enabled": true, "fingerprint": fingerprint },
            "reality": {
                "enabled": true,
                "public_key": reality.public_key,
                "short_id": reality.short_id,
            }
        })
    } else {
        let mut tls = json!({
            "enabled": true,
            "server_name": profile.address,
            "insecure": false
        });
        if let Some(fp) = cfg.fingerprint.as_deref() {
            tls["utls"] = json!({ "enabled": true, "fingerprint": fp });
        }
        if let Some(ech) = cfg.ech.as_ref().filter(|e| e.enabled) {
            let mut ech_block = json!({ "enabled": true });
            if !ech.config.is_empty() {
                ech_block["config"] = json!(ech.config);
            }
            tls["ech"] = ech_block;
        }
        tls
    };

    let mut outbound = json!({
        "type": "vless",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "uuid": cfg.uuid,
        "packet_encoding": "xudp",
        "tls": tls
    });

    if let Some(ref flow) = cfg.flow {
        outbound["flow"] = json!(flow);
    }

    if let Some(ref transport_type) = cfg.transport_type {
        let mut transport = json!({ "type": transport_type_str(transport_type) });
        if *transport_type == TransportType::Grpc {
            if let Some(ref service_name) = cfg.transport_service_name {
                transport["service_name"] = json!(service_name);
            }
            transport["idle_timeout"] = json!("15s");
            transport["ping_timeout"] = json!("15s");
        }
        outbound["transport"] = transport;
    }

    Ok(outbound)
}

/// Build VMess outbound (sing-box 1.12: explicit `security` cipher, no
/// deprecated `aes-128-cfb`; `alter_id: 0` for AEAD-only mode).
fn build_vmess_outbound(profile: &Profile, cfg: &VmessConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "vmess",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "uuid": cfg.uuid,
        "security": cfg.security.as_str(),
        "alter_id": cfg.alter_id,
        "packet_encoding": "xudp",
        "tls": build_tls_block(&profile.address, &[], &cfg.tls),
    });
    if let Some(padding) = cfg.global_padding {
        outbound["global_padding"] = json!(padding);
    }
    if let Some(transport) = cfg.transport.as_ref() {
        outbound["transport"] = build_transport_block(transport);
    }
    Ok(outbound)
}

/// Build Trojan outbound.
fn build_trojan_outbound(profile: &Profile, cfg: &TrojanConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "trojan",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "password": cfg.password,
        "tls": build_tls_block(&profile.address, &[], &cfg.tls),
    });
    if let Some(transport) = cfg.transport.as_ref() {
        outbound["transport"] = build_transport_block(transport);
    }
    Ok(outbound)
}

/// Build Shadowsocks outbound (AEAD/AEAD-2022 ciphers only).
fn build_shadowsocks_outbound(profile: &Profile, cfg: &ShadowsocksConfig) -> anyhow::Result<Value> {
    Ok(json!({
        "type": "shadowsocks",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "method": cfg.method.as_str(),
        "password": cfg.password,
    }))
}

/// Build Hysteria2 outbound.
///
/// QUIC-based; ALPN defaults to `["h3"]` when not explicitly set.
/// Uses the sing-box 1.12 nested `obfs: { type, password }` form (the
/// legacy top-level `obfs_password` field is not emitted).
fn build_hysteria2_outbound(profile: &Profile, cfg: &Hysteria2Config) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "hysteria2",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "password": cfg.password,
        "tls": build_tls_block(&profile.address, &["h3"], &cfg.tls),
    });
    if let Some(up) = cfg.up_mbps {
        outbound["up_mbps"] = json!(up);
    }
    if let Some(down) = cfg.down_mbps {
        outbound["down_mbps"] = json!(down);
    }
    if let Some(obfs) = cfg.obfs.as_ref() {
        outbound["obfs"] = json!({
            "type": "salamander",
            "password": obfs.password,
        });
        let _ = &obfs.kind; // single supported type today; kept for forward compat
    }
    Ok(outbound)
}

/// Build TUIC v5 outbound.
fn build_tuic_outbound(profile: &Profile, cfg: &TuicConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "tuic",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "uuid": cfg.uuid,
        "password": cfg.password,
        "congestion_control": cfg.congestion_control.as_str(),
        "udp_relay_mode": cfg.udp_relay_mode.as_str(),
        "tls": build_tls_block(&profile.address, &["h3"], &cfg.tls),
    });
    if cfg.zero_rtt_handshake {
        outbound["zero_rtt_handshake"] = json!(true);
    }
    Ok(outbound)
}

/// Build the ShadowTLS wrapper + inner Shadowsocks detour pair.
/// The Shadowsocks outbound is tagged `proxy` (referenced by routing rules);
/// the ShadowTLS outbound is tagged internally and chained via `detour`.
fn build_shadowtls_outbounds(
    profile: &Profile,
    cfg: &ShadowtlsConfig,
) -> anyhow::Result<Vec<Value>> {
    const SHADOWTLS_TAG: &str = "shadowtls-wrap";

    let mut shadowtls = json!({
        "type": "shadowtls",
        "tag": SHADOWTLS_TAG,
        "server": profile.address,
        "server_port": profile.port,
        "version": cfg.version.as_u8(),
        "tls": build_tls_block(&profile.address, &[], &cfg.tls),
    });
    if cfg.version == ShadowtlsVersion::V3 {
        shadowtls["password"] = json!(cfg.password);
    }

    let shadowsocks = json!({
        "type": "shadowsocks",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "method": cfg.method.as_str(),
        "password": cfg.ss_password,
        "detour": SHADOWTLS_TAG,
    });

    Ok(vec![shadowtls, shadowsocks])
}

/// Build AnyTLS outbound.
fn build_anytls_outbound(profile: &Profile, cfg: &AnytlsConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "anytls",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "password": cfg.password,
        "tls": build_tls_block(&profile.address, &[], &cfg.tls),
    });
    if let Some(v) = cfg.idle_session_check_interval.as_deref() {
        outbound["idle_session_check_interval"] = json!(v);
    }
    if let Some(v) = cfg.idle_session_timeout.as_deref() {
        outbound["idle_session_timeout"] = json!(v);
    }
    Ok(outbound)
}

/// Build SOCKS outbound (no TLS layer in sing-box; use ShadowTLS for that).
fn build_socks_outbound(profile: &Profile, cfg: &SocksConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "socks",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "version": socks_version_str(&cfg.version),
    });
    if let Some(user) = cfg.username.as_deref() {
        outbound["username"] = json!(user);
    }
    if let Some(pass) = cfg.password.as_deref() {
        outbound["password"] = json!(pass);
    }
    Ok(outbound)
}

fn socks_version_str(v: &crate::config::profile::SocksVersion) -> &'static str {
    use crate::config::profile::SocksVersion::*;
    match v {
        V4 => "4",
        V4a => "4a",
        V5 => "5",
    }
}

/// Build HTTP CONNECT outbound (TLS optional via [`TlsCommon`]).
fn build_http_outbound(profile: &Profile, cfg: &HttpConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "http",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
    });
    if let Some(user) = cfg.username.as_deref() {
        outbound["username"] = json!(user);
    }
    if let Some(pass) = cfg.password.as_deref() {
        outbound["password"] = json!(pass);
    }
    if tls_is_enabled(&cfg.tls) {
        outbound["tls"] = build_tls_block(&profile.address, &[], &cfg.tls);
    }
    Ok(outbound)
}

/// `TlsCommon::default()` represents "no TLS" — we treat a TLS block as
/// enabled when the user supplied at least one TLS-related field.
fn tls_is_enabled(tls: &TlsCommon) -> bool {
    tls.server_name.is_some()
        || tls.insecure
        || !tls.alpn.is_empty()
        || tls.utls_fingerprint.is_some()
        || tls.reality.is_some()
        || tls.ech.as_ref().is_some_and(|e| e.enabled)
}

/// Build SSH outbound.
fn build_ssh_outbound(profile: &Profile, cfg: &SshConfig) -> anyhow::Result<Value> {
    let mut outbound = json!({
        "type": "ssh",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "user": cfg.user,
    });
    if let Some(pass) = cfg.password.as_deref() {
        outbound["password"] = json!(pass);
    }
    if let Some(pk) = cfg.private_key.as_deref() {
        outbound["private_key"] = json!(pk);
    }
    if let Some(pkp) = cfg.private_key_path.as_deref() {
        outbound["private_key_path"] = json!(pkp);
    }
    if let Some(passphrase) = cfg.private_key_passphrase.as_deref() {
        outbound["private_key_passphrase"] = json!(passphrase);
    }
    if !cfg.host_key.is_empty() {
        outbound["host_key"] = json!(cfg.host_key);
    }
    if !cfg.host_key_algorithms.is_empty() {
        outbound["host_key_algorithms"] = json!(cfg.host_key_algorithms);
    }
    Ok(outbound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{
        DnsRule, DnsStrategy, GeoRegion, GeoRouting, Profile, ProtocolConfig, RealitySettings,
    };

    fn test_profile() -> Profile {
        let mut p = Profile::new_vless(
            "Example".to_string(),
            "203.0.113.42".to_string(),
            59431,
            "671c62c7-6768-4b98-ac6b-572c9c707be0".to_string(),
        );
        if let ProtocolConfig::Vless(ref mut cfg) = p.config {
            cfg.security = Some(crate::config::profile::Security::Reality);
            cfg.reality = Some(RealitySettings {
                public_key: "0IO3LodsrMnhOWh4ogwgdVqYg30CS5-snhFMwldOuAQ".to_string(),
                short_id: "f04debc34cbc48a4".to_string(),
                server_name: "google.com".to_string(),
                spider_x: "/".to_string(),
            });
            cfg.transport_type = Some(TransportType::Grpc);
            cfg.fingerprint = Some("chrome".to_string());
        }
        p
    }

    fn vless_cfg_mut(profile: &mut Profile) -> &mut VlessConfig {
        match &mut profile.config {
            ProtocolConfig::Vless(c) => c,
            _ => panic!("expected VLESS"),
        }
    }

    fn vless_cfg(profile: &Profile) -> &VlessConfig {
        match &profile.config {
            ProtocolConfig::Vless(c) => c,
            _ => panic!("expected VLESS"),
        }
    }

    #[test]
    fn generated_config_has_required_keys() {
        let profile = test_profile();
        let settings = Settings::default();
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();

        assert!(config.get("log").is_some());
        assert!(config.get("dns").is_some());
        assert!(config.get("inbounds").is_some());
        assert!(config.get("outbounds").is_some());
        assert!(config.get("route").is_some());
        assert!(config.get("experimental").is_some());
    }

    #[test]
    fn generated_config_global_final_is_proxy() {
        let profile = test_profile();
        let settings = Settings::default();
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        let route = config.get("route").unwrap();
        assert_eq!(route["final"].as_str().unwrap(), "proxy");
    }

    #[test]
    fn generated_config_only_ru_final_is_direct() {
        let profile = test_profile();
        let mut geo_routing = GeoRouting::default();
        geo_routing.set_region(GeoRegion::Ru);
        geo_routing.set_mode(RoutingMode::OnlyRu);
        let settings = Settings {
            geo_routing,
            ..Default::default()
        };
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        let route = config.get("route").unwrap();
        assert_eq!(route["final"].as_str().unwrap(), "direct");
    }

    #[test]
    fn vless_outbound_with_reality() {
        let profile = test_profile();
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();

        assert_eq!(outbound["type"], "vless");
        assert_eq!(outbound["tag"], "proxy");
        assert_eq!(outbound["server"], "203.0.113.42");
        assert_eq!(outbound["server_port"], 59431);
        assert_eq!(outbound["uuid"], "671c62c7-6768-4b98-ac6b-572c9c707be0");

        let tls = &outbound["tls"];
        assert_eq!(tls["enabled"], true);
        assert_eq!(tls["server_name"], "google.com");
        assert!(tls.get("reality").is_some());
        assert_eq!(
            tls["reality"]["public_key"],
            "0IO3LodsrMnhOWh4ogwgdVqYg30CS5-snhFMwldOuAQ"
        );
        assert_eq!(tls["reality"]["short_id"], "f04debc34cbc48a4");
        assert_eq!(tls["utls"]["enabled"], true);
        assert_eq!(tls["utls"]["fingerprint"], "chrome");
    }

    #[test]
    fn vless_outbound_without_reality() {
        let profile = Profile::new_vless(
            "Simple".to_string(),
            "1.2.3.4".to_string(),
            443,
            "uuid".to_string(),
        );
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();

        let tls = &outbound["tls"];
        assert_eq!(tls["enabled"], true);
        assert_eq!(tls["server_name"], "1.2.3.4");
        assert_eq!(tls["insecure"], false);
        assert!(tls.get("reality").is_none());
    }

    #[test]
    fn vless_outbound_with_flow() {
        let mut profile = test_profile();
        vless_cfg_mut(&mut profile).flow = Some(crate::config::profile::Flow::XtlsRprxVision);
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();
        assert_eq!(outbound["flow"], "xtls-rprx-vision");
    }

    #[test]
    fn vless_outbound_with_grpc_transport() {
        let profile = test_profile();
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();

        assert!(outbound.get("transport").is_some());
        let transport = &outbound["transport"];
        assert_eq!(transport["type"], "grpc");
        assert_eq!(transport["idle_timeout"], "15s");
        assert_eq!(transport["ping_timeout"], "15s");
    }

    #[test]
    fn vless_outbound_with_grpc_service_name() {
        let mut profile = test_profile();
        vless_cfg_mut(&mut profile).transport_service_name = Some("my-service".to_string());
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();
        assert_eq!(outbound["transport"]["service_name"], "my-service");
    }

    #[test]
    fn vless_outbound_without_transport() {
        let mut profile = test_profile();
        vless_cfg_mut(&mut profile).transport_type = None;
        let outbound = build_vless_outbound(&profile, vless_cfg(&profile)).unwrap();
        assert!(outbound.get("transport").is_none());
    }

    fn profile_with(config: ProtocolConfig, address: &str, port: u16) -> Profile {
        Profile {
            id: uuid::Uuid::nil(),
            name: "T".into(),
            address: address.into(),
            port,
            config,
            tags: Vec::new(),
            subscription_id: None,
        }
    }

    fn build_one(profile: &Profile) -> Value {
        let mut outs = build_outbound(profile).unwrap();
        assert_eq!(outs.len(), 1, "expected a single outbound");
        outs.remove(0)
    }

    #[test]
    fn vmess_outbound_basic_shape() {
        use crate::config::profile::{VmessConfig, VmessSecurity};
        let profile = profile_with(
            ProtocolConfig::Vmess(VmessConfig {
                uuid: "vm-uuid".into(),
                alter_id: 0,
                security: VmessSecurity::Aes128Gcm,
                ..Default::default()
            }),
            "1.1.1.1",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "vmess");
        assert_eq!(outbound["uuid"], "vm-uuid");
        assert_eq!(outbound["security"], "aes-128-gcm");
        assert_eq!(outbound["alter_id"], 0);
        assert_eq!(outbound["packet_encoding"], "xudp");
        assert!(outbound.get("tls").is_some());
        // Sing-box 1.12 forbids the legacy stream cipher.
        assert_ne!(outbound["security"], "aes-128-cfb");
    }

    #[test]
    fn vmess_outbound_with_ws_transport() {
        use crate::config::profile::{TransportConfig, TransportType, VmessConfig};
        let profile = profile_with(
            ProtocolConfig::Vmess(VmessConfig {
                uuid: "u".into(),
                transport: Some(TransportConfig {
                    kind: TransportType::Ws,
                    path: Some("/ws".into()),
                    host: Some("example.com".into()),
                    service_name: None,
                    headers: Default::default(),
                }),
                ..Default::default()
            }),
            "1.1.1.1",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["transport"]["type"], "ws");
        assert_eq!(outbound["transport"]["path"], "/ws");
        assert_eq!(outbound["transport"]["host"], "example.com");
    }

    #[test]
    fn vmess_tls_emits_ech_when_enabled() {
        use crate::config::profile::{EchSettings, TlsCommon, VmessConfig};
        let profile = profile_with(
            ProtocolConfig::Vmess(VmessConfig {
                uuid: "u".into(),
                tls: TlsCommon {
                    ech: Some(EchSettings {
                        enabled: true,
                        config: vec!["base64-blob".into()],
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            "ech.example.com",
            443,
        );
        let outbound = build_one(&profile);
        let ech = &outbound["tls"]["ech"];
        assert_eq!(ech["enabled"], true);
        assert_eq!(ech["config"][0], "base64-blob");
    }

    #[test]
    fn trojan_outbound_shape() {
        use crate::config::profile::TrojanConfig;
        let profile = profile_with(
            ProtocolConfig::Trojan(TrojanConfig {
                password: "secret".into(),
                ..Default::default()
            }),
            "trojan.example",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "trojan");
        assert_eq!(outbound["password"], "secret");
        assert_eq!(outbound["tls"]["enabled"], true);
        assert_eq!(outbound["tls"]["server_name"], "trojan.example");
    }

    #[test]
    fn shadowsocks_outbound_uses_aead_cipher() {
        use crate::config::profile::{ShadowsocksCipher, ShadowsocksConfig};
        let profile = profile_with(
            ProtocolConfig::Shadowsocks(ShadowsocksConfig {
                method: ShadowsocksCipher::Blake3Aes256Gcm,
                password: "ss-pass".into(),
            }),
            "ss.example",
            8388,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "shadowsocks");
        assert_eq!(outbound["method"], "2022-blake3-aes-256-gcm");
        assert_eq!(outbound["password"], "ss-pass");
        assert!(
            outbound.get("tls").is_none(),
            "Shadowsocks must not carry a tls block"
        );
    }

    #[test]
    fn hysteria2_outbound_shape() {
        use crate::config::profile::{Hysteria2Config, Hysteria2Obfs, Hysteria2ObfsType};
        let profile = profile_with(
            ProtocolConfig::Hysteria2(Hysteria2Config {
                password: "hy2-pass".into(),
                up_mbps: Some(100),
                down_mbps: Some(200),
                obfs: Some(Hysteria2Obfs {
                    kind: Hysteria2ObfsType::Salamander,
                    password: "obfs-pass".into(),
                }),
                ..Default::default()
            }),
            "hy2.example",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "hysteria2");
        assert_eq!(outbound["password"], "hy2-pass");
        assert_eq!(outbound["up_mbps"], 100);
        assert_eq!(outbound["down_mbps"], 200);
        assert_eq!(outbound["obfs"]["type"], "salamander");
        assert_eq!(outbound["obfs"]["password"], "obfs-pass");
        // The legacy top-level obfs_password key must not appear.
        assert!(outbound.get("obfs_password").is_none());
        // Hysteria2 is QUIC-based — ALPN defaults to h3 when not set.
        assert_eq!(outbound["tls"]["alpn"][0], "h3");
    }

    #[test]
    fn tuic_outbound_shape() {
        use crate::config::profile::{TuicConfig, TuicCongestion, TuicUdpRelayMode};
        let profile = profile_with(
            ProtocolConfig::Tuic(TuicConfig {
                uuid: "tuic-uuid".into(),
                password: "tuic-pass".into(),
                congestion_control: TuicCongestion::Bbr,
                udp_relay_mode: TuicUdpRelayMode::Native,
                zero_rtt_handshake: true,
                ..Default::default()
            }),
            "tuic.example",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "tuic");
        assert_eq!(outbound["uuid"], "tuic-uuid");
        assert_eq!(outbound["password"], "tuic-pass");
        assert_eq!(outbound["congestion_control"], "bbr");
        assert_eq!(outbound["udp_relay_mode"], "native");
        assert_eq!(outbound["zero_rtt_handshake"], true);
        assert_eq!(outbound["tls"]["alpn"][0], "h3");
    }

    #[test]
    fn shadowtls_emits_wrapper_and_detour_pair() {
        use crate::config::profile::{ShadowsocksCipher, ShadowtlsConfig};
        let profile = profile_with(
            ProtocolConfig::Shadowtls(ShadowtlsConfig {
                version: ShadowtlsVersion::V3,
                password: "st-pass".into(),
                method: ShadowsocksCipher::Chacha20IetfPoly1305,
                ss_password: "inner-ss".into(),
                ..Default::default()
            }),
            "st.example",
            443,
        );
        let outs = build_outbound(&profile).unwrap();
        assert_eq!(outs.len(), 2, "ShadowTLS emits wrapper + detour");
        let wrap = outs.iter().find(|o| o["type"] == "shadowtls").unwrap();
        let inner = outs.iter().find(|o| o["type"] == "shadowsocks").unwrap();
        assert_eq!(wrap["version"], 3);
        assert_eq!(wrap["password"], "st-pass");
        assert_eq!(inner["tag"], "proxy");
        assert_eq!(inner["server"], "st.example");
        assert_eq!(inner["server_port"], 443);
        assert_eq!(inner["password"], "inner-ss");
        assert_eq!(inner["detour"], wrap["tag"]);
    }

    #[test]
    fn shadowtls_v1_omits_password() {
        use crate::config::profile::{ShadowsocksCipher, ShadowtlsConfig};
        let profile = profile_with(
            ProtocolConfig::Shadowtls(ShadowtlsConfig {
                version: ShadowtlsVersion::V1,
                password: "ignored".into(),
                method: ShadowsocksCipher::Chacha20IetfPoly1305,
                ss_password: "inner-ss".into(),
                ..Default::default()
            }),
            "st.example",
            443,
        );
        let outs = build_outbound(&profile).unwrap();
        let wrap = outs.iter().find(|o| o["type"] == "shadowtls").unwrap();
        assert_eq!(wrap["version"], 1);
        assert!(wrap.get("password").is_none(), "v1 must not emit password");
    }

    #[test]
    fn anytls_outbound_shape() {
        use crate::config::profile::AnytlsConfig;
        let profile = profile_with(
            ProtocolConfig::Anytls(AnytlsConfig {
                password: "anytls-pass".into(),
                idle_session_timeout: Some("30s".into()),
                ..Default::default()
            }),
            "anytls.example",
            443,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "anytls");
        assert_eq!(outbound["password"], "anytls-pass");
        assert_eq!(outbound["idle_session_timeout"], "30s");
        assert_eq!(outbound["tls"]["enabled"], true);
    }

    #[test]
    fn socks_outbound_with_auth() {
        use crate::config::profile::{SocksConfig, SocksVersion};
        let profile = profile_with(
            ProtocolConfig::Socks(SocksConfig {
                version: SocksVersion::V5,
                username: Some("u".into()),
                password: Some("p".into()),
            }),
            "socks.example",
            1080,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "socks");
        assert_eq!(outbound["version"], "5");
        assert_eq!(outbound["username"], "u");
        assert_eq!(outbound["password"], "p");
        assert!(outbound.get("tls").is_none());
    }

    #[test]
    fn http_outbound_emits_tls_only_when_user_opts_in() {
        use crate::config::profile::{HttpConfig, TlsCommon};
        let plain = profile_with(
            ProtocolConfig::Http(HttpConfig::default()),
            "http.example",
            8080,
        );
        let plain_out = build_one(&plain);
        assert!(plain_out.get("tls").is_none(), "plain HTTP omits TLS");

        let secure = profile_with(
            ProtocolConfig::Http(HttpConfig {
                tls: TlsCommon {
                    server_name: Some("proxy.example".into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            "http.example",
            8443,
        );
        let secure_out = build_one(&secure);
        assert_eq!(secure_out["tls"]["enabled"], true);
        assert_eq!(secure_out["tls"]["server_name"], "proxy.example");
    }

    #[test]
    fn ssh_outbound_password_and_key() {
        use crate::config::profile::SshConfig;
        let profile = profile_with(
            ProtocolConfig::Ssh(SshConfig {
                user: "alice".into(),
                password: Some("p".into()),
                private_key_path: Some("/keys/id_ed25519".into()),
                host_key_algorithms: vec!["ssh-ed25519".into()],
                ..Default::default()
            }),
            "ssh.example",
            22,
        );
        let outbound = build_one(&profile);
        assert_eq!(outbound["type"], "ssh");
        assert_eq!(outbound["user"], "alice");
        assert_eq!(outbound["password"], "p");
        assert_eq!(outbound["private_key_path"], "/keys/id_ed25519");
        assert_eq!(outbound["host_key_algorithms"][0], "ssh-ed25519");
    }

    #[test]
    fn generated_config_includes_direct_outbound() {
        let profile = test_profile();
        let settings = Settings::default();
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        let outbounds = config["outbounds"].as_array().unwrap();
        assert!(outbounds.iter().any(|o| o["type"] == "direct"));
        assert!(outbounds.iter().any(|o| o["tag"] == "proxy"));
    }

    fn dns_default() -> DnsConfig {
        DnsConfig::default()
    }

    #[test]
    fn route_has_default_mark_for_killswitch() {
        let (route, _) = build_route(
            &RoutingMode::Global,
            &dns_default(),
            &GeoAvailability::all(),
        );
        assert_eq!(
            route["default_mark"].as_u64(),
            Some(666),
            "default_mark must match the kill-switch nft rule (0x29a)"
        );
    }

    #[test]
    fn build_route_global_has_basic_rules() {
        let mut dns = dns_default();
        dns.strategy = DnsStrategy::OnlyIpv4;
        let (route, rule_sets) = build_route(&RoutingMode::Global, &dns, &GeoAvailability::all());
        assert!(rule_sets.is_empty());
        let rules = route["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 3); // ipv6 reject, dns hijack, direct cidr
        assert_eq!(route["final"], "proxy");
        assert_eq!(route["default_domain_resolver"]["strategy"], "ipv4_only");
        assert_eq!(route["default_domain_resolver"]["server"], "remote");
    }

    #[test]
    fn build_route_only_ru_has_private_rule_and_final_direct() {
        let (route, _rule_sets) = build_route(
            &RoutingMode::OnlyRu,
            &dns_default(),
            &GeoAvailability::all(),
        );
        let rules = route["rules"].as_array().unwrap();
        assert!(rules.len() >= 4); // basic 3 + ip_is_private
        assert_eq!(route["final"], "direct");
    }

    #[test]
    fn generated_config_only_cn_final_is_direct() {
        let profile = test_profile();
        let mut geo_routing = GeoRouting::default();
        geo_routing.set_region(GeoRegion::Cn);
        geo_routing.set_mode(RoutingMode::OnlyCn);
        let settings = Settings {
            geo_routing,
            ..Default::default()
        };
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        let route = config.get("route").unwrap();
        assert_eq!(route["final"].as_str().unwrap(), "direct");
    }

    #[test]
    fn build_route_bypass_cn_has_private_rule() {
        let (route, _rule_sets) = build_route(
            &RoutingMode::BypassCn,
            &dns_default(),
            &GeoAvailability::all(),
        );
        let rules = route["rules"].as_array().unwrap();
        assert!(rules.len() >= 4); // basic 3 + ip_is_private
        assert_eq!(route["final"], "proxy");
    }

    #[test]
    fn build_route_only_cn_has_private_rule_and_final_direct() {
        let (route, _rule_sets) = build_route(
            &RoutingMode::OnlyCn,
            &dns_default(),
            &GeoAvailability::all(),
        );
        let rules = route["rules"].as_array().unwrap();
        assert!(rules.len() >= 4); // basic 3 + ip_is_private
        assert_eq!(route["final"], "direct");
    }

    #[test]
    fn default_dns_block_matches_legacy_layout() {
        let dns = build_dns(&dns_default());
        let servers = dns["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0]["tag"], "local");
        assert_eq!(servers[0]["type"], "local");
        assert_eq!(servers[1]["tag"], "remote");
        assert_eq!(servers[1]["type"], "https");
        assert_eq!(servers[1]["server"], "1.1.1.1");
        assert_eq!(servers[1]["path"], "/dns-query");
        assert!(servers[1].get("server_port").is_none());
        assert_eq!(dns["final"], "remote");
        assert_eq!(dns["strategy"], "prefer_ipv4");
        assert!(dns.get("rules").is_none());
        assert!(dns.get("fakeip").is_none());
    }

    #[test]
    fn build_dns_emits_dot_server_with_port() {
        let dns = DnsConfig {
            servers: vec![
                DnsServer::Local {
                    tag: "local".to_string(),
                },
                DnsServer::Tls {
                    tag: "google-dot".to_string(),
                    server: "8.8.8.8".to_string(),
                    server_port: Some(853),
                },
            ],
            rules: Vec::new(),
            final_server: "google-dot".to_string(),
            strategy: DnsStrategy::PreferIpv4,
            fakeip_enabled: false,
        };
        let block = build_dns(&dns);
        let s = &block["servers"].as_array().unwrap()[1];
        assert_eq!(s["type"], "tls");
        assert_eq!(s["server"], "8.8.8.8");
        assert_eq!(s["server_port"], 853);
        assert_eq!(block["final"], "google-dot");
    }

    #[test]
    fn build_dns_emits_fakeip_server_and_auto_rule_when_enabled() {
        let dns = DnsConfig {
            servers: vec![
                DnsServer::Local {
                    tag: "local".to_string(),
                },
                DnsServer::FakeIp {
                    tag: "fake".to_string(),
                    inet4_range: "198.18.0.0/15".to_string(),
                    inet6_range: "fc00::/18".to_string(),
                },
            ],
            rules: Vec::new(),
            final_server: "local".to_string(),
            strategy: DnsStrategy::PreferIpv4,
            fakeip_enabled: true,
        };
        let block = build_dns(&dns);
        // Sing-box 1.12 no longer accepts the top-level `dns.fakeip` block;
        // the ranges live inside the server entry instead.
        assert!(block.get("fakeip").is_none());
        let fake_server = block["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["type"] == "fakeip")
            .unwrap();
        assert_eq!(fake_server["tag"], "fake");
        assert_eq!(fake_server["inet4_range"], "198.18.0.0/15");
        assert_eq!(fake_server["inet6_range"], "fc00::/18");

        let rules = block["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            1,
            "auto-rule must be injected when fakeip is on"
        );
        assert_eq!(rules[0]["server"], "fake");
        assert_eq!(rules[0]["query_type"][0], "A");
        assert_eq!(rules[0]["query_type"][1], "AAAA");

        assert_eq!(block["independent_cache"], true);
    }

    #[test]
    fn build_dns_does_not_duplicate_fakeip_rule_when_user_added_one() {
        let dns = DnsConfig {
            servers: vec![
                DnsServer::Local {
                    tag: "local".to_string(),
                },
                DnsServer::FakeIp {
                    tag: "fake".to_string(),
                    inet4_range: "198.18.0.0/15".to_string(),
                    inet6_range: "fc00::/18".to_string(),
                },
            ],
            rules: vec![DnsRule {
                domain_suffix: vec!["example.com".to_string()],
                server: "fake".to_string(),
                ..Default::default()
            }],
            final_server: "local".to_string(),
            strategy: DnsStrategy::PreferIpv4,
            fakeip_enabled: true,
        };
        let block = build_dns(&dns);
        let rules = block["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["domain_suffix"][0], "example.com");
        assert_eq!(rules[0]["server"], "fake");
    }

    #[test]
    fn generated_config_sets_store_fakeip_when_enabled() {
        let profile = test_profile();
        let settings = Settings {
            dns: DnsConfig {
                servers: vec![
                    DnsServer::Local {
                        tag: "local".to_string(),
                    },
                    DnsServer::FakeIp {
                        tag: "fake".to_string(),
                        inet4_range: "198.18.0.0/15".to_string(),
                        inet6_range: "fc00::/18".to_string(),
                    },
                ],
                rules: Vec::new(),
                final_server: "local".to_string(),
                strategy: DnsStrategy::PreferIpv4,
                fakeip_enabled: true,
            },
            ..Settings::default()
        };
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        assert_eq!(
            config["experimental"]["cache_file"]["store_fakeip"], true,
            "store_fakeip must persist the v4/v6→domain map across restarts"
        );
    }

    #[test]
    fn generated_config_omits_store_fakeip_when_disabled() {
        let profile = test_profile();
        let settings = Settings::default();
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        assert!(
            config["experimental"]["cache_file"]
                .get("store_fakeip")
                .is_none()
        );
    }

    #[test]
    fn build_dns_emits_rules_with_domain_suffix() {
        let dns = DnsConfig {
            servers: vec![
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
            rules: vec![DnsRule {
                domain_suffix: vec!["example.com".to_string()],
                server: "local".to_string(),
                ..Default::default()
            }],
            final_server: "remote".to_string(),
            strategy: DnsStrategy::PreferIpv4,
            fakeip_enabled: false,
        };
        let block = build_dns(&dns);
        let rules = block["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["domain_suffix"][0], "example.com");
        assert_eq!(rules[0]["server"], "local");
        assert!(rules[0].get("disable_cache").is_none());
    }

    #[test]
    fn generated_config_uses_custom_final_server() {
        let profile = test_profile();
        let settings = Settings {
            dns: DnsConfig {
                servers: vec![
                    DnsServer::Local {
                        tag: "local".to_string(),
                    },
                    DnsServer::Https {
                        tag: "quad9".to_string(),
                        server: "9.9.9.9".to_string(),
                        server_port: None,
                        path: "/dns-query".to_string(),
                    },
                ],
                rules: Vec::new(),
                final_server: "quad9".to_string(),
                strategy: DnsStrategy::PreferIpv4,
                fakeip_enabled: false,
            },
            ..Settings::default()
        };
        let config = generate_config(&profile, &settings, &GeoAvailability::all()).unwrap();
        assert_eq!(config["dns"]["final"], "quad9");
        assert_eq!(
            config["route"]["default_domain_resolver"]["server"],
            "quad9"
        );
    }
}
