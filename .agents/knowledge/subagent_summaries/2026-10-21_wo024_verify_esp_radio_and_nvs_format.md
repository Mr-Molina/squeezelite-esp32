# Subagent Summary: WO-024 — Verification of NVS Wi-Fi Format, HTTP Recovery Endpoints, and `esp-radio 1.0.0-beta.1` + `smoltcp` Bridge

- **Date:** 2026-10-21
- **Role:** Discovery Scout / Research Specialist
- **Work Order Reference:** `WO-024` (`/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-024-verify-esp-radio-and-nvs-format.md`)
- **Status:** `[EXECUTED & VERIFIED]`

---

## 1. Files Modified / Created

1. **Modified File:**
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-024-verify-esp-radio-and-nvs-format.md`
   - *Rationale:* Fulfilled Work Order WO-024, recorded empirical analysis of NVS partition layout, cryptographic derivation, HTTP recovery endpoints, and pure-Rust `esp-radio` + `smoltcp` bridge implementation.
2. **Created Backup:**
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-024-verify-esp-radio-and-nvs-format.md.bak`
   - *Rationale:* Pre-modification backup per Rule 35.
3. **Created Scratch Test Project:**
   - `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/radio_probe/Cargo.toml`
   - `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/radio_probe/src/main.rs`
   - *Rationale:* Test crate to empirically compile and verify `esp-radio 1.0.0-beta.1`, `esp-hal 1.2.2`, `esp-alloc 0.11.0`, and `smoltcp 0.12.0` on `xtensa-esp32-none-elf`.

---

## 2. Key Findings & Empirical Ground Truth

### A. NVS Partition & Credentials
- **Target Partition:** Partition `nvs` at flash offset `0x9000` (size `0x4000` = 16 KB). Partition `settings` at `0x3F0000` is used only by `platform_config.c` for DAC/display/audio settings.
- **RAM Storage Flag:** Squeezelite-esp32 executes `esp_wifi_set_storage(WIFI_STORAGE_RAM)`, preventing ESP-IDF from populating `nvs.net80211`.
- **Namespaces & Keys:**
  - Active STA credentials: Namespace `"config"`, key `"ssid"` (`NVS_TYPE_BLOB`), key `"password"` (`NVS_TYPE_BLOB`).
  - Saved AP list: Namespace `"aplist"`, key = SSID, value = JSON string.
- **Encryption Scheme:**
  - Salt: 16-byte domain salt: `0x57, 0x69, 0x46, 0x69, 0x4e, 0x56, 0x53, 0x4b, 0x65, 0x79, 0x24, 0x53, 0x65, 0x63, 0x75, 0x72` (`"WiFiNVSKey$Secur"`).
  - MAC: 6-byte Wi-Fi STA hardware MAC.
  - Keystream: `SHA-256(salt || mac)[0..16]`.
  - Prefix: `"enc:"` (length 4).
  - Ciphertext: Hex-encoded XOR of plaintext with keystream.

### B. HTTP Recovery & OTA Endpoints
- `POST /recovery.json` -> calls `guided_factory()`, executes `guided_boot(ESP_PARTITION_SUBTYPE_APP_FACTORY)` to boot into `recovery` (`0x10000`).
- `POST /reboot_ota.json` -> calls `guided_restart_ota()`, executes `guided_boot(ESP_PARTITION_SUBTYPE_APP_OTA_0)` to boot into `ota_0` (`0x160000`).
- `POST /reboot.json` -> calls `simple_restart()`, executes standard `esp_restart()`.
- `POST /flash.json` -> Active only in recovery mode (`is_recovery_running == true`). Streams firmware into target flash partition.
- `GET /config.json` -> Strips out `"password"`, `"ap_pwd"`, etc., before sending response. Plaintext credentials cannot be obtained via HTTP.

### C. `esp-radio 1.0.0-beta.1` + `smoltcp` Integration
- Controller initialized via `let mut controller = esp_radio::wifi::WifiController::new(peripherals.WIFI, ControllerConfig::default())?`.
- Station interface acquired via `let mut sta_iface = esp_radio::wifi::Interface::station()`.
- Synchronous packet access: `Interface::receive(&mut self) -> Option<(WifiRxToken, WifiTxToken)>` and `Interface::transmit(&mut self) -> Option<WifiTxToken>`.
- Constructed 38-line pure-Rust `smoltcp::phy::Device` adapter `SmolDevice`.
- Empirical verification: compiled cleanly for `xtensa-esp32-none-elf` with `smoltcp::iface::Interface` and TCP port 80 socket (`Exit Code: 0`).

---

## 3. Untested Assumptions & Risks
- When connecting to Wi-Fi at runtime, `esp-radio` relies on preemptive task scheduling provided by `esp-rtos` (`esp-radio-rtos-driver = "0.4.2"`). In bare-metal applications without async runtimes, `esp_rtos::start(timer)` must be invoked prior to radio operations.
