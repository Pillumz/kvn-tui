use serde_json::{Map, Value, json};
use std::path::PathBuf;

use crate::config::profile::{DnsConfig, DnsServer, Profile, RoutingMode, Settings, TransportType};

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
    let outbound = build_outbound(profile)?;
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
        "outbounds": [
            outbound,
            {
                "type": "direct",
                "tag": "direct"
            }
        ],
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

/// Build the outbound object based on profile protocol and settings.
fn build_outbound(profile: &Profile) -> anyhow::Result<Value> {
    build_vless_outbound(profile)
}

/// Build VLESS outbound with optional REALITY / XTLS Vision.
fn build_vless_outbound(profile: &Profile) -> anyhow::Result<Value> {
    let tls = if let Some(ref reality) = profile.reality {
        let fingerprint = profile.fingerprint.as_deref().unwrap_or("chrome");
        let reality_json = json!({
            "enabled": true,
            "public_key": reality.public_key,
            "short_id": reality.short_id
        });
        json!({
            "enabled": true,
            "server_name": reality.server_name,
            "utls": {
                "enabled": true,
                "fingerprint": fingerprint
            },
            "reality": reality_json
        })
    } else {
        json!({
            "enabled": true,
            "server_name": profile.address,
            "insecure": false
        })
    };

    let mut outbound = json!({
        "type": "vless",
        "tag": "proxy",
        "server": profile.address,
        "server_port": profile.port,
        "uuid": profile.uuid,
        "packet_encoding": "xudp",
        "tls": tls
    });

    if let Some(ref flow) = profile.flow {
        outbound["flow"] = json!(flow);
    }

    // Add transport layer if specified (grpc, ws, httpupgrade, etc.)
    if let Some(ref transport_type) = profile.transport_type {
        let mut transport = json!({"type": transport_type});
        if *transport_type == TransportType::Grpc {
            if let Some(ref service_name) = profile.transport_service_name {
                transport["service_name"] = json!(service_name);
            }
            transport["idle_timeout"] = json!("15s");
            transport["ping_timeout"] = json!("15s");
        }
        outbound["transport"] = transport;
    }

    Ok(outbound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{
        DnsRule, DnsStrategy, GeoRegion, GeoRouting, Profile, Protocol, RealitySettings,
    };

    fn test_profile() -> Profile {
        let mut p = Profile::new(
            "Example".to_string(),
            Protocol::Vless,
            "203.0.113.42".to_string(),
            59431,
            "671c62c7-6768-4b98-ac6b-572c9c707be0".to_string(),
        );
        p.security = Some(crate::config::profile::Security::Reality);
        p.reality = Some(RealitySettings {
            public_key: "0IO3LodsrMnhOWh4ogwgdVqYg30CS5-snhFMwldOuAQ".to_string(),
            short_id: "f04debc34cbc48a4".to_string(),
            server_name: "google.com".to_string(),
            spider_x: "/".to_string(),
        });
        p.transport_type = Some(TransportType::Grpc);
        p.fingerprint = Some("chrome".to_string());
        p
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
        let outbound = build_vless_outbound(&profile).unwrap();

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
        let profile = Profile::new(
            "Simple".to_string(),
            Protocol::Vless,
            "1.2.3.4".to_string(),
            443,
            "uuid".to_string(),
        );
        let outbound = build_vless_outbound(&profile).unwrap();

        let tls = &outbound["tls"];
        assert_eq!(tls["enabled"], true);
        assert_eq!(tls["server_name"], "1.2.3.4");
        assert_eq!(tls["insecure"], false);
        assert!(tls.get("reality").is_none());
    }

    #[test]
    fn vless_outbound_with_flow() {
        let mut profile = test_profile();
        profile.flow = Some(crate::config::profile::Flow::XtlsRprxVision);
        let outbound = build_vless_outbound(&profile).unwrap();
        assert_eq!(outbound["flow"], "xtls-rprx-vision");
    }

    #[test]
    fn vless_outbound_with_grpc_transport() {
        let profile = test_profile();
        let outbound = build_vless_outbound(&profile).unwrap();

        assert!(outbound.get("transport").is_some());
        let transport = &outbound["transport"];
        assert_eq!(transport["type"], "grpc");
        assert_eq!(transport["idle_timeout"], "15s");
        assert_eq!(transport["ping_timeout"], "15s");
    }

    #[test]
    fn vless_outbound_with_grpc_service_name() {
        let mut profile = test_profile();
        profile.transport_service_name = Some("my-service".to_string());
        let outbound = build_vless_outbound(&profile).unwrap();
        assert_eq!(outbound["transport"]["service_name"], "my-service");
    }

    #[test]
    fn vless_outbound_without_transport() {
        let mut profile = test_profile();
        profile.transport_type = None;
        let outbound = build_vless_outbound(&profile).unwrap();
        assert!(outbound.get("transport").is_none());
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
