//! Share-link URI parsing and encoding.
//!
//! Each supported VPN protocol has a canonical URI form (`vless://...`,
//! `vmess://...`, etc.) that users paste into the TUI or that subscription
//! servers return. This module owns both directions of the translation —
//! `parse_share_link` builds a [`Profile`] from a URI; `encode_share_link`
//! produces a URI from a [`Profile`].

use std::collections::HashMap;

use anyhow::{Context, Result};
use url::Url;

use super::*;

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
