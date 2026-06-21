use anyhow::{Context, Result};
use base64::Engine;

use crate::config::profile::{Profile, SUPPORTED_SHARE_SCHEMES, parse_share_link};

/// Fetch a subscription URL, decode its body (Base64 or plain text), and parse
/// every share link it contains. Mixed-protocol subscriptions are supported;
/// individual malformed lines are logged and skipped.
pub fn fetch_subscription(url: &str) -> Result<Vec<Profile>> {
    let agent = ureq::Agent::new_with_defaults();
    let resp = agent
        .get(url)
        .call()
        .with_context(|| format!("GET {} failed", url))?;

    if resp.status() != 200 {
        anyhow::bail!("HTTP {} for {}", resp.status(), url);
    }

    let body = resp
        .into_body()
        .read_to_string()
        .context("Failed to read subscription body")?;

    parse_subscription_body(&body)
}

/// True when `line` (already trimmed) starts with any supported share-link scheme.
fn line_has_supported_scheme(line: &str) -> bool {
    SUPPORTED_SHARE_SCHEMES
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

/// Parse a subscription body that is either Base64-encoded or plain text.
/// Each non-empty line is interpreted as a share link in any supported scheme.
pub fn parse_subscription_body(body: &str) -> Result<Vec<Profile>> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Subscription body is empty");
    }

    let decoded = try_decode_base64(trimmed);
    let text = decoded.as_deref().unwrap_or(trimmed);

    let mut profiles = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_share_link(line) {
            Ok(profile) => profiles.push(profile),
            Err(e) => tracing::warn!("subscription: skipped malformed line: {e}"),
        }
    }

    if profiles.is_empty() {
        anyhow::bail!("No supported share links found in subscription");
    }

    Ok(profiles)
}

/// Attempt to Base64-decode `text`. Returns `Some(decoded)` only when decoding
/// succeeds and the result looks like a subscription (contains at least one
/// supported scheme prefix). This prevents treating plain text as binary garbage.
fn try_decode_base64(text: &str) -> Option<String> {
    let decoded_bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .ok()?;
    let decoded = String::from_utf8(decoded_bytes).ok()?;
    if decoded.lines().any(|line| {
        let l = line.trim();
        line_has_supported_scheme(l)
    }) {
        Some(decoded)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_vless() -> &'static str {
        "vless://671c62c7-6768-4b98-ac6b-572c9c707be0@203.0.113.42:443?security=tls#Sub-1"
    }

    #[test]
    fn parse_plain_body_with_two_links() {
        let body = format!("{}\n{}\n", sample_vless(), sample_vless());
        let profiles = parse_subscription_body(&body).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].address, "203.0.113.42");
        assert_eq!(profiles[0].name, "Sub-1");
    }

    #[test]
    fn parse_base64_body() {
        let plain = format!("{}\n{}\n", sample_vless(), sample_vless());
        let encoded = base64::engine::general_purpose::STANDARD.encode(&plain);
        let profiles = parse_subscription_body(&encoded).unwrap();
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn parse_body_with_invalid_lines_is_tolerant() {
        let body = format!("not-a-link\n{}\n\nalso-not-a-link\n", sample_vless());
        let profiles = parse_subscription_body(&body).unwrap();
        assert_eq!(profiles.len(), 1);
    }

    #[test]
    fn parse_empty_body_fails() {
        assert!(parse_subscription_body("   ").is_err());
    }

    #[test]
    fn parse_body_without_vless_links_fails() {
        assert!(parse_subscription_body("just some text").is_err());
    }

    #[test]
    fn parse_mixed_protocol_subscription() {
        let body = "vless://uuid@1.1.1.1:443#V\n\
                    trojan://pw@2.2.2.2:443#T\n\
                    ss://YWVzLTI1Ni1nY206cHc@3.3.3.3:8388#S\n\
                    hysteria2://hp@4.4.4.4:443#H2\n";
        let profiles = parse_subscription_body(body).unwrap();
        assert_eq!(profiles.len(), 4);
        let names: Vec<_> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["V", "T", "S", "H2"]);
    }

    #[test]
    fn parse_base64_body_with_multiple_schemes() {
        let body = "vless://uuid@1.1.1.1:443#V\ntrojan://pw@2.2.2.2:443#T\n";
        let encoded = base64::engine::general_purpose::STANDARD.encode(body);
        let profiles = parse_subscription_body(&encoded).unwrap();
        assert_eq!(profiles.len(), 2);
    }
}
