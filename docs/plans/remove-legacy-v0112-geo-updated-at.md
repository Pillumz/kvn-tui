# Plan: Remove legacy v0.11.2 geo `updated_at` support

## Goal

Drop the temporary migration layer in `src/infra/geo.rs` that reads the v0.11.2 `metadata.json` format with a single global `updated_at` timestamp. After this change only the per-region `updated_at` map will be recognized.

## When to execute

Schedule for a future release once enough time has passed for users to migrate from v0.11.2 — for example, the first minor or major release after at least one migration-focused release (e.g. v0.13.0).

## Current state

`src/infra/geo.rs` contains:

- A `GeoMetadata` struct with `updated_at: HashMap<GeoRegion, DateTime<Local>>` (current format).
- A `GeoMetadataLegacy` struct mirroring the v0.11.2 schema with `updated_at: Option<DateTime<Local>>`.
- A fallback branch in `GeoManager::load_metadata` that:
  1. Tries to parse `metadata.json` as current format.
  2. On failure tries to parse it as legacy format.
  3. Migrates the legacy timestamp into per-region entries for regions that have local `.srs` files.
  4. Persists the migrated metadata back to disk.
- Three tests verifying the legacy migration behavior.

## Steps

1. **Remove the legacy struct** in `src/infra/geo.rs`:
   - Delete `GeoMetadataLegacy`.

2. **Simplify `GeoManager::load_metadata`**:
   - Remove the `serde_json::from_str::<GeoMetadataLegacy>` fallback branch.
   - Remove the migration/save logic.
   - Keep only the current-format parse path.

   The simplified `load_metadata` should look like the pre-migration version:
   ```rust
   fn load_metadata(&self) -> Result<GeoMetadata> {
       if !self.metadata_path.exists() {
           return Ok(GeoMetadata::default());
       }
       let text = fs::read_to_string(&self.metadata_path)
           .with_context(|| format!("Failed to read {:?}", self.metadata_path))?;
       let meta: GeoMetadata = serde_json::from_str(&text)
           .with_context(|| format!("Failed to parse {:?}", self.metadata_path))?;
       Ok(meta)
   }
   ```

3. **Clean up migration tests** in `src/infra/geo.rs` `mod tests`:
   - Delete `legacy_metadata_migrates_to_per_region`.
   - Delete `legacy_metadata_without_updated_at_migrates_empty`.
   - Delete `legacy_metadata_missing_files_migrates_empty`.

4. **Review remaining tests**:
   - Ensure `metadata_roundtrip` still exercises the current per-region format.
   - Ensure `load_metadata_missing_returns_default` still passes.

5. **Decide on direct-upgrade behavior** (optional, document the choice):
   - If a user upgrades directly from v0.11.2 after this removal, `metadata.json` will fail to parse and `load_metadata` will return an error.
   - This is acceptable if the removal is scheduled after a sufficient migration window.
   - Alternatively, keep a minimal fallback that treats unparseable `metadata.json` as default (losing the timestamp but avoiding a hard error). If this path is chosen, add a warning log and a test.

6. **Update project documentation**:
   - Save this plan to `docs/plans/remove-legacy-v0112-geo-updated-at.md`.
   - Update `AGENTS.md` only if it mentions the v0.11.2 geo metadata migration (it currently does not).

7. **Run checks**:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo test`

## Result

`src/infra/geo.rs` will contain only the current per-region geo metadata format. Any `metadata.json` still using the v0.11.2 single-string `updated_at` will fail to parse (or be treated as default, depending on the decision in step 5), forcing a one-time re-download or manual migration.
