# Active Bugs & Monitored Items Ledger (`MK5-Rust`)

## Current Bug Status
- **Active Critical Bugs**: 0
- **Active Blocking Bugs**: 0
- **Remediated Audit Findings**: All 88 code audit findings (`MA-01`..`MA-22`, `REM-01`..`REM-32`, `IPV-01`..`IPV-08`) and 10 performance tuning items (`OPT-01`..`OPT-08`) successfully remediated, counter-verified, and certified.

## Monitored Infrastructure Items
1. **Host `cargo-audit` Tooling Gap**:
   - `cargo-audit` is not currently installed in the host environment on `pc-1`.
   - `.agents/.manifest_hash` remains unmarked until `cargo-audit` is provisioned.
2. **Build-Time Advisory Tracking**:
   - `rust/Cargo.lock` contains 3 informational unmaintained build-time advisories: `paste 1.0.15`, `serde_yaml 0.9.34+deprecated`, and `unsafe-libyaml 0.2.11`.
   - These are build-time only and do not affect embedded runtime security.
3. **LMS Plugin Configuration Protection**:
   - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` on `10.0.0.10` has a custom exception for player `c8:2e:18:be:b8:34` (`"BASEMENT"`) to prevent infinite rewrite loops of `custom-convert.conf`.
   - Monitor if LMS updates or reinstallations revert this configuration.
4. **Upstream Git Parity**:
   - Upstream branch `master-v4.3` tracked through `ee1720c7` (tag `build-number-1780`).
