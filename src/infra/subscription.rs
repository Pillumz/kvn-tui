use anyhow::{Context, Result};
use base64::Engine;

use crate::config::profile::{Profile, parse_share_link};

/// Fetch a subscription URL, decode its body (Base64 or plain text), and parse
/// every VLESS share link it contains.
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

/// Parse a subscription body that is either Base64-encoded or plain text.
/// Each non-empty line is interpreted as a VLESS share link.
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
        if let Ok(profile) = parse_share_link(line) {
            profiles.push(profile);
        }
    }

    if profiles.is_empty() {
        anyhow::bail!("No valid VLESS links found in subscription");
    }

    Ok(profiles)
}

/// Attempt to Base64-decode `text`. Returns `Some(decoded)` only when decoding
/// succeeds and the result looks like a subscription (contains at least one
/// `vless://` line). This prevents treating plain text as binary garbage.
fn try_decode_base64(text: &str) -> Option<String> {
    let decoded_bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .ok()?;
    let decoded = String::from_utf8(decoded_bytes).ok()?;
    if decoded
        .lines()
        .any(|line| line.trim().starts_with("vless://"))
    {
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
}
