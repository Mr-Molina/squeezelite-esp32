# Subagent Summary: Runtime Stability, Post-Boot `ota_0` Confirmation, Zero-Alloc Cached mDNS & Live Syslog (`2026-11-09`)

## 1. Created / Modified / Deleted Files (Absolute Paths)
- **Modified**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
- **Created**:
  - `/tmp/louder-esp32-app-webui-v4.bin` (`1,984,864` bytes, SHA-256 `8932890ed2c36b563e1a54818c6fe75a541317ddba9249ff70bcc7ddb2593505`)
  - `/tmp/main.rs.bak_v3`, `/tmp/tasks.rs.bak_v3`, `/tmp/http_control.rs.bak_v3`

## 2. Rationale
1. **Post-Boot `ota_0` Confirmation (`persist_ota_0_boot_partition`)**:
   - Preserves Gate 1 early-boot `otadata` (`0xD000..0xF000`) erase so any crash prior to Wi-Fi + DHCP rolls back to C Factory Recovery (`0x10000`).
   - Once `wifi_connected && s.assigned_ip.is_some()` is achieved, parks Core 1, writes `OtaSelectEntry::new_ota_0().to_bytes()` to `0xD000`, unparks Core 1, sets `s.boot_ota0_confirmed = true`, and logs `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"` to `WEB_SYSLOG_RING`.
2. **Explicit `otadata` Erase on `/recovery.json` & Safe `ota_0` Reboot on `/reboot.json`**:
   - `HttpControlAction::RebootToRecovery` (`POST /recovery.json`) erases `0xD000..0xF000` with Core 1 parked before calling `software_reset()`.
   - `HttpControlAction::PersistOta0AndReboot` (`POST /reboot.json` and `POST /reboot_ota.json`) flushes NVS settings, writes `OtaSelectEntry::new_ota_0()` without hazardous `read_nor`, and calls `software_reset()`.
3. **Zero-Allocation Cached mDNS Announcements**:
   - Caches the built AirPlay RAOP (`cached_raop_mdns: [u8; 512]`) and Spotify Connect (`cached_spot_mdns: [u8; 384]`) mDNS announcement packets in `DeviceRuntimeState`, rebuilding only when IP or device name changes. Eliminates ~9 PSRAM heap alloc/dealloc operations per 30s mDNS timer tick and per UDP 5353 query.
4. **Live Runtime Syslog (`/syslog.json` & `/messages.json`) and Telemetry Counters**:
   - Populates `WEB_SYSLOG_RING` on DHCPv4 lease acquired/deconfigured, post-boot `ota_0` confirmation, Wi-Fi station disconnect, Slimproto server endpoint switch, and TAS5805M fault recovery.
   - Wires live `otadata_prearmed_factory`, `disconnect_count`, and `is_i2c_locked` into `HttpStatusSnapshot`.

## 3. Untested Assumptions & Risks
- None; all 452 host unit/integration tests, Clippy, `google_rust_audit.py --strict`, and live hardware Checks 1–4 on `"BASEMENT"` (`172.22.1.15`) are `[EXECUTED & VERIFIED]`.
