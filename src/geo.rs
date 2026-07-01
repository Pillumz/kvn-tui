use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::app::msg::GeoResult;
use crate::config::profile::GeoRegion;

/// One downloadable rule-set file (geoip *or* geosite) for a region.
pub(crate) struct GeoAsset {
    pub(crate) filename: &'static str,
    pub(crate) url: &'static str,
}

impl GeoAsset {
    /// sing-box rule-set tag — the filename without the `.srs` extension
    /// (`"geoip-ru.srs"` → `"geoip-ru"`). The tag is what's referenced from
    /// route rules in the generated sing-box config.
    pub(crate) fn tag(&self) -> &'static str {
        self.filename.trim_end_matches(".srs")
    }
}

/// Both rule-sets (geoip + geosite) a region downloads.
pub(crate) struct RegionAssets {
    pub(crate) geoip: GeoAsset,
    pub(crate) geosite: GeoAsset,
}

/// Single source of truth for which files / URLs back each region. Adding a
/// new country = add one match arm here plus a `GeoRegion::ALL` entry.
///
/// Returns `None` for `GeoRegion::Global`, which has no rule-sets.
pub(crate) fn region_assets(region: GeoRegion) -> Option<RegionAssets> {
    match region {
        GeoRegion::Global => None,
        GeoRegion::Ru => Some(RegionAssets {
            geoip: GeoAsset {
                filename: "geoip-ru.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
            },
            geosite: GeoAsset {
                filename: "geosite-category-ru.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ru.srs",
            },
        }),
        GeoRegion::Cn => Some(RegionAssets {
            geoip: GeoAsset {
                filename: "geoip-cn.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs",
            },
            geosite: GeoAsset {
                filename: "geosite-cn.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
            },
        }),
        GeoRegion::Ir => Some(RegionAssets {
            geoip: GeoAsset {
                filename: "geoip-ir.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ir.srs",
            },
            geosite: GeoAsset {
                filename: "geosite-category-ir.srs",
                url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ir.srs",
            },
        }),
    }
}

/// Metadata tracking ETags and update time for geo rule-sets.
#[derive(Debug, Clone, Default, Serialize)]
struct GeoMetadata {
    /// Maps filename (e.g. `"geoip-ru.srs"`) → ETag returned by the server on
    /// the last successful download. Used to short-circuit re-downloads.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    etags: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    updated_at: HashMap<GeoRegion, DateTime<Local>>,
}

/// Deserialization shape that also accepts the legacy per-country etag fields
/// written by older builds. Folded into the modern map-shaped `GeoMetadata`.
#[derive(Debug, Default, Deserialize)]
struct GeoMetadataRaw {
    #[serde(default)]
    etags: HashMap<String, String>,
    #[serde(default)]
    updated_at: HashMap<GeoRegion, DateTime<Local>>,
    #[serde(default)]
    geoip_ru_etag: Option<String>,
    #[serde(default)]
    geosite_ru_etag: Option<String>,
    #[serde(default)]
    geoip_cn_etag: Option<String>,
    #[serde(default)]
    geosite_cn_etag: Option<String>,
    #[serde(default)]
    geoip_ir_etag: Option<String>,
    #[serde(default)]
    geosite_ir_etag: Option<String>,
}

impl From<GeoMetadataRaw> for GeoMetadata {
    fn from(raw: GeoMetadataRaw) -> Self {
        let mut etags = raw.etags;
        let mut promote = |filename: &str, value: Option<String>| {
            if let Some(v) = value {
                etags.entry(filename.to_string()).or_insert(v);
            }
        };
        promote("geoip-ru.srs", raw.geoip_ru_etag);
        promote("geosite-category-ru.srs", raw.geosite_ru_etag);
        promote("geoip-cn.srs", raw.geoip_cn_etag);
        promote("geosite-cn.srs", raw.geosite_cn_etag);
        promote("geoip-ir.srs", raw.geoip_ir_etag);
        promote("geosite-category-ir.srs", raw.geosite_ir_etag);
        GeoMetadata {
            etags,
            updated_at: raw.updated_at,
        }
    }
}

/// Manages downloading and updating geoip/geosite rule-sets for sing-box.
pub struct GeoManager {
    geo_dir: PathBuf,
    metadata_path: PathBuf,
    agent: ureq::Agent,
}

impl GeoManager {
    /// Create a new GeoManager, ensuring the geo directory exists.
    pub fn new() -> Result<Self> {
        let geo_dir = crate::paths::geo_dir();

        fs::create_dir_all(&geo_dir)
            .with_context(|| format!("Failed to create geo dir {:?}", geo_dir))?;

        let metadata_path = geo_dir.join("metadata.json");
        let agent = ureq::Agent::new_with_defaults();

        Ok(Self {
            geo_dir,
            metadata_path,
            agent,
        })
    }

    /// Return `(geoip, geosite)` local paths for `region`, or `None` for
    /// regions without rule-sets (`Global`). Paths are computed; the files
    /// may not exist yet.
    pub fn local_paths(&self, region: GeoRegion) -> Option<(PathBuf, PathBuf)> {
        region_assets(region).map(|a| {
            (
                self.geo_dir.join(a.geoip.filename),
                self.geo_dir.join(a.geosite.filename),
            )
        })
    }

    /// Return whether local rule-set files for the given region are present.
    /// `Global` always returns `true` (nothing to download).
    pub fn has_databases(&self, region: GeoRegion) -> bool {
        match region_assets(region) {
            None => true,
            Some(a) => {
                self.geo_dir.join(a.geoip.filename).exists()
                    && self.geo_dir.join(a.geosite.filename).exists()
            }
        }
    }

    /// Return a human-readable string of the last update time for the given region, or None.
    pub fn last_updated(&self, region: GeoRegion) -> Option<String> {
        if matches!(region, GeoRegion::Global) {
            return None;
        }
        let meta = self.load_metadata().ok()?;
        meta.updated_at
            .get(&region)
            .map(|dt| dt.format("%d %b %H:%M").to_string())
    }

    /// Check whether rule-sets have updates available for the given region.
    /// Returns `(geoip_has_update, geosite_has_update)`. For `Global`, both
    /// are `false`.
    pub fn check_update_available(&self, region: GeoRegion) -> Result<(bool, bool)> {
        let Some(assets) = region_assets(region) else {
            return Ok((false, false));
        };
        let meta = self.load_metadata().unwrap_or_default();
        let needs = |asset: &GeoAsset| -> Result<bool> {
            let local = self.geo_dir.join(asset.filename);
            if !local.exists() {
                return Ok(true);
            }
            self.check_single(
                asset.url,
                meta.etags.get(asset.filename).map(String::as_str),
            )
        };
        Ok((needs(&assets.geoip)?, needs(&assets.geosite)?))
    }

    /// Download rule-sets for the given region and update metadata atomically.
    /// `Global` is a no-op returning `Ok(false)`.
    pub fn download_databases(&self, region: GeoRegion) -> Result<bool> {
        let Some(assets) = region_assets(region) else {
            return Ok(false);
        };
        let mut meta = self.load_metadata().unwrap_or_default();
        for asset in [&assets.geoip, &assets.geosite] {
            let dest = self.geo_dir.join(asset.filename);
            let etag = self
                .download_file(asset.url, &dest)
                .with_context(|| format!("Failed to download {}", asset.filename))?;
            if let Some(e) = etag {
                meta.etags.insert(asset.filename.to_string(), e);
            }
        }
        meta.updated_at.insert(region, Local::now());
        self.save_metadata(&meta)?;
        Ok(true)
    }

    /// Full update flow: check then download if needed.
    /// Returns typed result describing what happened.
    pub fn update_if_needed(&self, region: GeoRegion) -> Result<GeoResult> {
        if matches!(region, GeoRegion::Global) {
            return Ok(GeoResult::UpToDate);
        }

        let (geoip_need, geosite_need) = self.check_update_available(region)?;

        if !geoip_need && !geosite_need {
            return Ok(GeoResult::UpToDate);
        }

        let updated = self.download_databases(region)?;
        if updated {
            let mut parts = Vec::new();
            if geoip_need {
                parts.push(format!("geoip-{}", region.as_str()));
            }
            if geosite_need {
                parts.push(format!("geosite-{}", region.as_str()));
            }
            let last_updated = self.last_updated(region);
            Ok(GeoResult::Updated {
                parts,
                last_updated,
            })
        } else {
            Ok(GeoResult::UpToDate)
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn load_metadata(&self) -> Result<GeoMetadata> {
        if !self.metadata_path.exists() {
            return Ok(GeoMetadata::default());
        }
        let text = fs::read_to_string(&self.metadata_path)
            .with_context(|| format!("Failed to read {:?}", self.metadata_path))?;
        let raw: GeoMetadataRaw = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse {:?}", self.metadata_path))?;
        Ok(raw.into())
    }

    fn save_metadata(&self, meta: &GeoMetadata) -> Result<()> {
        let text = serde_json::to_string_pretty(meta)?;
        self.write_atomic(&self.metadata_path, text.as_bytes())?;
        Ok(())
    }

    fn check_single(&self, url: &str, saved_etag: Option<&str>) -> Result<bool> {
        let resp = self
            .agent
            .head(url)
            .call()
            .with_context(|| format!("HEAD request failed for {}", url))?;

        if resp.status() != 200 {
            return Ok(true); // assume update needed if we can't check
        }

        let remote_etag = resp.headers().get("etag").and_then(|v| v.to_str().ok());

        match (saved_etag, remote_etag) {
            (Some(saved), Some(remote)) => Ok(saved != remote),
            (None, _) => Ok(true),
            _ => Ok(true),
        }
    }

    /// Download a file and return its ETag on success.
    fn download_file(&self, url: &str, dest: &Path) -> Result<Option<String>> {
        let resp = self
            .agent
            .get(url)
            .call()
            .with_context(|| format!("GET {}", url))?;

        if resp.status() != 200 {
            anyhow::bail!("HTTP {} for {}", resp.status(), url);
        }

        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let bytes = resp
            .into_body()
            .read_to_vec()
            .context("Failed to read response body")?;
        self.write_atomic(dest, &bytes)?;
        Ok(etag)
    }

    fn write_atomic(&self, dest: &Path, data: &[u8]) -> Result<()> {
        crate::atomic_write::write(dest, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regions that actually have rule-sets (all of `GeoRegion::ALL` except
    /// `Global`). Used to parametrize per-country tests.
    const RULE_REGIONS: [GeoRegion; 3] = [GeoRegion::Ru, GeoRegion::Cn, GeoRegion::Ir];

    #[test]
    fn region_assets_some_for_rule_regions_none_for_global() {
        assert!(region_assets(GeoRegion::Global).is_none());
        for region in RULE_REGIONS {
            let a = region_assets(region).expect("rule region has assets");
            assert!(a.geoip.filename.starts_with("geoip-"));
            assert!(a.geosite.filename.starts_with("geosite-"));
            assert!(a.geoip.url.contains(a.geoip.filename));
            assert!(a.geosite.url.contains(a.geosite.filename));
        }
    }

    #[test]
    fn geo_asset_tag_strips_srs_extension() {
        let a = region_assets(GeoRegion::Ru).unwrap();
        assert_eq!(a.geoip.tag(), "geoip-ru");
        assert_eq!(a.geosite.tag(), "geosite-category-ru");
    }

    #[test]
    fn local_paths_match_region_assets() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        assert!(gm.local_paths(GeoRegion::Global).is_none());
        for region in RULE_REGIONS {
            let (geoip, geosite) = gm.local_paths(region).unwrap();
            let a = region_assets(region).unwrap();
            assert_eq!(geoip.file_name().unwrap(), a.geoip.filename);
            assert_eq!(geosite.file_name().unwrap(), a.geosite.filename);
            assert!(geoip.starts_with(&gm.geo_dir));
            assert!(geosite.starts_with(&gm.geo_dir));
        }
    }

    #[test]
    fn metadata_roundtrip_via_map() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let now = Local::now();
        let mut etags = HashMap::new();
        let mut updated_at = HashMap::new();
        for region in RULE_REGIONS {
            let a = region_assets(region).unwrap();
            etags.insert(
                a.geoip.filename.to_string(),
                format!("etag-{}-ip", region.as_str()),
            );
            etags.insert(
                a.geosite.filename.to_string(),
                format!("etag-{}-site", region.as_str()),
            );
            updated_at.insert(region, now);
        }
        let meta = GeoMetadata { etags, updated_at };
        gm.save_metadata(&meta).unwrap();
        let loaded = gm.load_metadata().unwrap();
        for region in RULE_REGIONS {
            let a = region_assets(region).unwrap();
            assert_eq!(
                loaded.etags.get(a.geoip.filename),
                Some(&format!("etag-{}-ip", region.as_str()))
            );
            assert_eq!(
                loaded.etags.get(a.geosite.filename),
                Some(&format!("etag-{}-site", region.as_str()))
            );
            assert!(loaded.updated_at.contains_key(&region));
        }
    }

    #[test]
    fn metadata_migrates_legacy_per_country_etag_fields() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        // Hand-crafted legacy shape (no `etags` map, just the named fields).
        let legacy = r#"{
            "geoip_ru_etag": "legacy-ru-ip",
            "geosite_ru_etag": "legacy-ru-site",
            "geoip_cn_etag": "legacy-cn-ip",
            "geoip_ir_etag": "legacy-ir-ip"
        }"#;
        fs::write(&gm.metadata_path, legacy).unwrap();
        let loaded = gm.load_metadata().unwrap();
        assert_eq!(
            loaded.etags.get("geoip-ru.srs").map(String::as_str),
            Some("legacy-ru-ip")
        );
        assert_eq!(
            loaded
                .etags
                .get("geosite-category-ru.srs")
                .map(String::as_str),
            Some("legacy-ru-site")
        );
        assert_eq!(
            loaded.etags.get("geoip-cn.srs").map(String::as_str),
            Some("legacy-cn-ip")
        );
        assert_eq!(
            loaded.etags.get("geoip-ir.srs").map(String::as_str),
            Some("legacy-ir-ip")
        );
        // Absent legacy fields stay absent.
        assert!(!loaded.etags.contains_key("geosite-cn.srs"));
        assert!(!loaded.etags.contains_key("geosite-category-ir.srs"));
    }

    #[test]
    fn metadata_map_wins_over_legacy_field_when_both_present() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let mixed = r#"{
            "etags": { "geoip-ru.srs": "modern" },
            "geoip_ru_etag": "legacy"
        }"#;
        fs::write(&gm.metadata_path, mixed).unwrap();
        let loaded = gm.load_metadata().unwrap();
        assert_eq!(
            loaded.etags.get("geoip-ru.srs").map(String::as_str),
            Some("modern")
        );
    }

    #[test]
    fn load_metadata_missing_returns_default() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let _ = fs::remove_file(&gm.metadata_path);
        let meta = gm.load_metadata().unwrap();
        assert!(meta.etags.is_empty());
        assert!(meta.updated_at.is_empty());
    }

    #[test]
    fn has_databases_reflects_file_presence_per_region() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        assert!(gm.has_databases(GeoRegion::Global));
        for region in RULE_REGIONS {
            let (geoip, geosite) = gm.local_paths(region).unwrap();
            let _ = fs::remove_file(&geoip);
            let _ = fs::remove_file(&geosite);

            assert!(!gm.has_databases(region), "missing files: {:?}", region);
            fs::write(&geoip, b"x").unwrap();
            assert!(
                !gm.has_databases(region),
                "only geoip present: {:?}",
                region
            );
            fs::write(&geosite, b"x").unwrap();
            assert!(gm.has_databases(region), "both present: {:?}", region);

            let _ = fs::remove_file(&geoip);
            let _ = fs::remove_file(&geosite);
        }
    }

    #[test]
    fn write_atomic_creates_file() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let dest = gm.geo_dir.join("test_atomic.txt");
        let _ = fs::remove_file(&dest);
        gm.write_atomic(&dest, b"hello world").unwrap();
        assert!(dest.exists());
        let contents = fs::read_to_string(&dest).unwrap();
        assert_eq!(contents, "hello world");
        let _ = fs::remove_file(&dest);
    }

    #[test]
    fn write_atomic_preserves_srs_extension() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let dest = gm.geo_dir.join("geoip-ru.srs");
        let _ = fs::remove_file(&dest);
        gm.write_atomic(&dest, b"data").unwrap();
        assert!(dest.exists());
        let temp = gm.geo_dir.join("geoip-ru.srs.tmp");
        assert!(!temp.exists());
        let _ = fs::remove_file(&dest);
    }

    #[test]
    fn last_updated_returns_none_for_global() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        assert!(gm.last_updated(GeoRegion::Global).is_none());
    }

    #[test]
    fn last_updated_returns_none_when_metadata_missing() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let _ = fs::remove_file(&gm.metadata_path);
        for region in RULE_REGIONS {
            assert!(gm.last_updated(region).is_none());
        }
    }

    #[test]
    fn last_updated_returns_formatted_string_after_save() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let mut updated_at = HashMap::new();
        let dt = Local::now();
        updated_at.insert(GeoRegion::Ru, dt);
        let meta = GeoMetadata {
            updated_at,
            ..GeoMetadata::default()
        };
        gm.save_metadata(&meta).unwrap();
        let formatted = gm.last_updated(GeoRegion::Ru).unwrap();
        assert_eq!(formatted.len(), 12);
        assert_eq!(formatted, dt.format("%d %b %H:%M").to_string());
        assert!(gm.last_updated(GeoRegion::Cn).is_none());
    }

    #[test]
    fn check_update_available_global_returns_no_updates() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let (a, b) = gm.check_update_available(GeoRegion::Global).unwrap();
        assert!(!a);
        assert!(!b);
    }

    #[test]
    fn download_databases_global_is_noop() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        assert!(!gm.download_databases(GeoRegion::Global).unwrap());
        assert!(!gm.metadata_path.exists());
    }

    #[test]
    fn update_if_needed_global_is_up_to_date() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        let result = gm.update_if_needed(GeoRegion::Global).unwrap();
        assert!(matches!(result, GeoResult::UpToDate));
    }

    #[test]
    fn load_metadata_parse_error_is_propagated() {
        let _guard = crate::test_helpers::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
        let gm = GeoManager::new().unwrap();
        fs::write(&gm.metadata_path, b"not valid json {{").unwrap();
        let err = gm.load_metadata().unwrap_err().to_string();
        assert!(err.contains("Failed to parse"));
    }

    /// Integration test that hits the real network. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn test_download_srs_files() {
        let gm = GeoManager::new().unwrap();
        let (geoip_ru, geosite_ru) = gm.local_paths(GeoRegion::Ru).unwrap();
        let _ = fs::remove_file(&geoip_ru);
        let _ = fs::remove_file(&geosite_ru);

        let result = gm.download_databases(GeoRegion::Ru);
        assert!(result.is_ok(), "download failed: {:?}", result);
        assert!(result.unwrap(), "expected updated=true");

        assert!(geoip_ru.exists(), "geoip-ru.srs should exist");
        assert!(geosite_ru.exists(), "geosite-category-ru.srs should exist");

        let updated = gm.last_updated(GeoRegion::Ru);
        assert!(updated.is_some(), "last_updated should be set");

        let result = gm.update_if_needed(GeoRegion::Ru).unwrap();
        assert!(
            matches!(result, GeoResult::UpToDate),
            "unexpected result: {:?}",
            result
        );
    }
}
