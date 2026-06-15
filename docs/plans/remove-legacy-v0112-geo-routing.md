# Plan: Remove legacy v0.11.2 geo/routing field support

## Goal
Drop the temporary migration layer that accepts the v0.11.2 config fields `settings.geo_region` and `settings.routing_mode`. After this change only the `settings.geo_routing` object will be recognized.

## When to execute
Schedule for a future release once enough time has passed for users to migrate from v0.11.2 — for example, the first minor or major release after at least one migration-focused release.

## Current state
`Settings` contains:
- A manual `Deserialize` implementation that deserializes into `SettingsRaw` to accept both `geo_routing` and the legacy fields `geo_region` / `routing_mode`.
- Two `pub(crate)` legacy fields: `geo_region` and `routing_mode`.
- A `migrate_legacy_geo_routing()` method that copies legacy values into `geo_routing`.
- Tests that verify the v0.11.2 JSON shape deserializes correctly.

## Steps

1. **Simplify `Settings` deserialization** in `src/config/profile.rs`.
   - Remove the `SettingsRaw` helper struct.
   - Replace the manual `impl<'de> Deserialize<'de> for Settings` with `#[derive(Deserialize)]`.
   - Keep `#[serde(deny_unknown_fields)]` on `Settings`.

2. **Remove legacy fields** from `Settings`:
   - `geo_region: Option<GeoRegion>`
   - `routing_mode: RoutingMode`

3. **Remove migration helper**:
   - Delete `Settings::migrate_legacy_geo_routing()`.

4. **Update `Settings::default()`** so it no longer initializes the removed legacy fields.

5. **Clean up tests** in `src/config/profile.rs`:
   - Delete `settings_migrate_legacy_geo_routing`.
   - Delete `config_deserializes_v0_11_2_legacy_fields`.
   - Remove any remaining references to `geo_region` or `routing_mode` on `Settings`.

6. **Review `src/config.rs`**:
   - Ensure `load_config_at` does not reference migration helpers (it already should not after the previous change).

7. **Update `AGENTS.md`**:
   - Remove any notes about migration from v0.11.2.
   - Update the routing/geo storage section to describe only the `geo_routing` object.

8. **Run checks**:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo test`

## Result
`Settings` will contain only the current fields plus `geo_routing`:

```rust
pub struct Settings {
    pub default_profile: Option<Uuid>,
    pub tun_interface: String,
    pub dns_strategy: DnsStrategy,
    pub geo_routing: GeoRouting,
    pub auto_connect: bool,
    pub last_connected_profile: Option<Uuid>,
}
```

Any config file still containing `settings.geo_region` or `settings.routing_mode` will fail to parse, forcing a one-time manual migration or a reinstall.
