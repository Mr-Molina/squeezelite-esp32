# Complete Web Interface Feature Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Achieve 100% web interface and HTTP REST API feature parity between the original C/C++ `squeezelite-esp32` repository (`components/wifi-manager/webapp` & `http_server_handlers.c`) and the `MK5-Rust` bare-metal firmware (`rust/crates/louder-esp32-app`), combining all 9 original C configuration/diagnostic tabs and REST endpoints with the full MK5-Rust real-time DSP, TAS5805M, and transport control dashboard.

**Architecture:** Extend the zero-allocation, non-blocking smoltcp Port 80 HTTP server (`http_control.rs`) to support `DELETE` requests, C-compatible and Rust-compatible dual-schema JSON payloads (`/status.json`, `/ap.json`, `/scan.json`, `/connect.json`, `/config.json`, `/messages.json`, `/commands.json`, `/flash.json`), and gzip/chunked `.rodata` static asset streaming (zero `.iram0.text` or `.dram2_uninit` impact). Wire the five currently stubbed `HttpControlAction` variants in both `tasks.rs` and `main.rs` (`UpdateConfig`, `ExecuteCliCommand`, `TriggerWifiScan`, `ConnectWifi`/`DisconnectWifi`, `StartOtaFlash`) into `ConfigHandle`, `FlashJournal`, `Console`, and the Wi-Fi/OTA controllers, and upgrade the embedded web UI (`web_ui.rs`) to provide all 9 original C tabs alongside the MK5-Rust Live Player & Hardware Telemetry tab.

**Tech Stack:** Rust (`no_std` compatible `louder-esp32-app` crate), `smoltcp` TCP socket streaming (`HttpStreamResponse`), `squeezelite-config` (`FlashJournal`), Vanilla ES6 HTML5/CSS3/JS single-page application stored in DROM (`.rodata`).

**Spec:** `/home/console-admin/Github/squeezelite-esp32/components/wifi-manager/http_server_handlers.c`, `/home/console-admin/Github/squeezelite-esp32/components/wifi-manager/webapp/src/index.ejs`, `/home/console-admin/Github/squeezelite-esp32/components/wifi-manager/webapp/src/js/custom.js`, and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`.

## Global Constraints

- **Zero Panic / Google Rust Style Compliance**: Zero `.unwrap()`, `.expect()`, `panic!()`, `todo!()`, or `unimplemented!()` in production code; must pass `/usr/bin/python3 "/home/console-admin/.gemini/config/skills/rust-pro/scripts/google_rust_audit.py" . --strict` with `0` errors and `0` warnings.
- **Bare-Metal ESP32 Memory Bounds (`xtensa-esp32-none-elf`)**:
  - `.stack >= 36,000` B (current baseline: `37,288` B)
  - `.iram0.text <= 131,072` B (current baseline: `126,944` B — place no web handlers in `#[ram]`)
  - `.dram2_uninit <= 98,768` B (current baseline: `97,560` B — place all UI templates and static tables in `.rodata` / DROM)
  - `ota_0` binary size `<= 2,686,976` B (`0x290000`, current baseline: `1,904,368` B)
- **Backwards Compatibility with Gate 1–6 Hardware Certification**: All 38 existing `GET /status.json` fields, `GET/POST /cmd.json` actions, `GET /eq.json`, `POST /recovery.json`, `POST /reboot_ota.json`, and Spotify Connect ZeroConf (`/?action=getInfo`, `/?action=addUser`, `/spotify_info`) MUST remain 100% intact.
- **Staging-Before-Restart (Invariant 42)**: Build and verify host unit tests (`cargo test`) and `xtensa-esp32-none-elf` release image (`--stage-only`) before any live hardware deployment on `"BASEMENT"` (`172.22.1.15`). Never touch `/dev/ttyUSB0`.

## Review Focus

- **Secret Redaction on `GET /config.json` and `POST /commands.json`**: Sensitive NVS keys (`password`, `pwd`, `ap_pwd`, `telnet_pwd`, `a2dp_spin`, `web_pwd`) must always be masked or omitted in `GET /config.json` and redacted in `POST /commands.json` responses and `/messages.json` syslog entries.
- **Cross-Origin CSRF Protection on Mutating Methods (`POST` & `DELETE`)**: `DELETE /connect.json` must enforce the exact same `is_permitted_origin` LAN/same-origin check and optional token auth gate as `POST` endpoints.
- **Dual-Schema JSON Parsing in `POST /config.json` and `POST /commands.json`**: `POST /config.json` must accept both flat Rust payloads (`key=val` or `{"key":"...","value":"..."}`) and original C nested payloads (`{"config":{"<key>":{"value":"...","type":33}}}` plus `"fwurl"`). `POST /commands.json` must accept both `cmd=<str>` and `{"command":"<str>"}`.
- **Bounded Ring Buffers for Syslog (`/messages.json`) and Scanned APs (`/ap.json`)**: Log and Wi-Fi scan storage must use fixed-capacity stack/static structures without unbounded heap growth.
- **Non-Blocking Chunked Response Streaming (`HttpStreamResponse`)**: Large `.rodata` HTML/JS/CSS payloads and JSON tables (`/commands.json`, `/config.json`) must stream across multiple `smoltcp` TCP poll cycles without truncating at the single-packet MTU (`1460` B).

---

## Feature Parity Gap Matrix (Original C vs. Current MK5-Rust)

| Subsystem / Feature | Original C (`wifi-manager` + `webapp`) | Current `MK5-Rust` (`http_control.rs`) | Target Parity State |
| :--- | :--- | :--- | :--- |
| **HTTP Methods** | `GET`, `POST`, `DELETE` | `GET`, `POST` only | Add `HttpMethod::Delete` (`DELETE /connect.json`) with CSRF/Auth gates |
| **Wi-Fi Scan List** | `GET /scan.json` (`{}`) + `GET /ap.json` (`[{ssid, chan, rssi, auth, known}]`) | `GET/POST /scan.json` (`{"aps":[...]}`), no `/ap.json` | Support `GET /ap.json` (C array schema) AND `GET/POST /scan.json`; wire `TriggerWifiScan` |
| **Wi-Fi Connect / Disconnect** | `POST /connect.json` (`{ssid, pwd, host_name}`), `DELETE /connect.json` | `POST /connect.json` (stubbed in superloop), no `DELETE` | Wire `ConnectWifi` & `DisconnectWifi` to `ConfigHandle`/`FlashJournal` + Wi-Fi state machine |
| **Status Telemetry (`/status.json`)** | 18 fields (`Jack`, `Voltage`, `disconnect_count`, `avg_conn_time`, `is_i2c_locked`, `urc`, `bt_status`, `bt_sub_status`, etc.) | 38 fields (DSP, TAS5805M, PSRAM, Core 1, I2C ACK/NACK, BT), missing 8 C status fields | Merge all 38 Rust fields + 8 C status fields (46 total fields) |
| **Config & GPIO (`/config.json`)** | `{ "gpio": [...], "config": { [k]: { "value": v, "type": t } } }` + `POST` NVS commit & `fwurl` | Flat 7-key snapshot; `POST` only flushes without updating keys | Serve `settings_seq`, `gpio` pin table, flat + typed `config` map; wire `UpdateConfig` for all NVS keys |
| **Syslog & Task Stats (`/messages.json`)** | Array of `{message, type, class, sent_time, current_time}` including `MESSAGING_CLASS_STATS` task/heap JSON | Single-item `{messages: [{seq, uptime_ms, level, msg}]}` | Bounded syslog ring buffer + `MESSAGING_CLASS_SYSTEM` & `MESSAGING_CLASS_STATS` (Core 0/1 & PSRAM/SRAM stats) |
| **CLI & Config Commands (`/commands.json`)** | `{ "commands": [{name, help, hascb, argtable}], "values": {...} }`; `POST {"command": "..."}` executes via `platform_console` | 8 static descriptors without `argtable`/`values`; `POST` is a no-op stub in `tasks.rs`/`main.rs` | Full command catalog (`cfg-audio-*`, `cfg-syst-*`, `cfg-hw-*`, `free`, `heap`, `version`, `i2cdetect`, `i2cget`, `i2cset`, `i2cdump`, etc.) + live `Console` execution |
| **OTA Firmware Update (`/flash.json`)** | `POST /flash.json` binary upload in recovery; `fwurl` HTTPS OTA | `GET/POST /flash.json` returns static JSON; stubbed in superloop | Track live `OtaProgressState`, wire `StartOtaFlash` & `fwurl` staging/validation |
| **Captive Portal Probes** | `/generate_204`, `/mobile/status.php` (200), `/gen_204`, `/hotspot-detect.html`, `/hotspotdetect.html`, `/library/test/success.html`, `/success.txt` (302) | Handled in `wifi.rs` helper, not all wired in `http_control.rs` router | Route all 7 captive portal probe paths directly in `http_control.rs` |
| **Web UI Tabs & Workflows** | 9 tabs: WiFi, Status/Syslog (Logs + GPIO + Tasks), Audio (`autoexec1` builder), System, Hardware (Presets/Pins), Updates/OTA, NVS Editor, Advanced Commands, Credits | Single-page compact dashboard (Telemetry + Transport/Volume/Presets/EQ/AGAIN/Recovery) | Unified 10-Tab Web UI: Live Player/DSP (Rust) + WiFi + Status/Syslog + Audio + System + Hardware + Firmware/OTA + NVS Editor + Console Commands + Credits |

---

## File Structure

- **Create**: `rust/crates/louder-esp32-app/src/web_ui.rs`
  - Holds the modular `.rodata` HTML/CSS/JS single-page application (`WEB_UI_HTML`) implementing the global status header, footer bar, and all 10 tabs (Live Player/DSP Dashboard + the 9 original C `wifi-manager` tabs), plus static GPIO assignment metadata (`BOARD_GPIO_MAP`) and `cfg-*` command schema definitions.
- **Modify**: `rust/crates/louder-esp32-app/src/http_control.rs`
  - Add `HttpMethod::Delete`, `GET /ap.json`, `DELETE /connect.json`, captive portal probe routes, C-parity fields in `HttpStatusSnapshot`, full `gpio` + typed `config` serialization/parsing in `/config.json`, `argtable` + `values` schema and `{"command": "..."}` parsing in `/commands.json`, syslog ring buffer + `MESSAGING_CLASS_STATS` in `/messages.json`, and `HttpControlAction::DisconnectWifi`.
- **Modify**: `rust/crates/louder-esp32-app/src/config_handle.rs`
  - Expand `RuntimeConfig` / `ConfigHandle` to store and serialize web-configurable NVS parameters (`host_name`, `lms_ip`, `lms_port`, `lms_cport`, `autoexec`, `autoexec1`, `enable_airplay`, `enable_cspot`, `enable_bt_sink`, `enable_telnet`, `bt_name`, `ap_name`, `cspot_name`, `dac_config`, `i2c_config`, `rotary_config`, `fwurl`) with secret redaction and import/export support.
- **Modify**: `rust/crates/louder-esp32-app/src/tasks.rs` and `rust/crates/louder-esp32-app/src/main.rs`
  - Replace the `HttpControlAction` no-op match arms (`UpdateConfig`, `ExecuteCliCommand`, `TriggerWifiScan`, `ConnectWifi`, `DisconnectWifi`, `StartOtaFlash`) with live implementations backed by `ConfigHandle`, `FlashJournal`, `Console`, Wi-Fi state, and the syslog message ring buffer.
- **Modify**: `rust/crates/louder-esp32-app/src/lib.rs`
  - Export `pub mod web_ui;`.

---

### Task 1: HTTP Method `DELETE`, Captive Portal Routes, and Wi-Fi Endpoints (`GET /ap.json`, `DELETE /connect.json`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

**Interfaces:**
- Produces:
  - `HttpMethod::Delete` variant in `pub enum HttpMethod`
  - `HttpControlAction::DisconnectWifi` variant in `pub enum HttpControlAction`
  - `pub struct ScannedApEntry { pub ssid: [u8; 32], pub ssid_len: u8, pub chan: u8, pub rssi: i8, pub auth: u8, pub known: bool }`
  - `pub fn format_ap_list_json(aps: &[ScannedApEntry], current_ssid: &str, current_rssi: i8, out: &mut [u8]) -> usize`

- [ ] **Step 1: Write the failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs`**

```rust
#[test]
fn test_get_ap_json_returns_c_compatible_array() {
    let snap = sample_snapshot();
    let req = b"GET /ap.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, action) = handle_request_bytes(req, &snap, None);
    assert_eq!(action, HttpControlAction::None);
    let body = extract_http_body(&resp);
    assert!(body.starts_with('[') && body.ends_with(']'));
    assert!(body.contains("\"ssid\":\"iot\""));
    assert!(body.contains("\"chan\":"));
    assert!(body.contains("\"rssi\":-50"));
    assert!(body.contains("\"auth\":"));
    assert!(body.contains("\"known\":true"));
}

#[test]
fn test_delete_connect_json_emits_disconnect_wifi_and_enforces_origin() {
    let snap = sample_snapshot();
    let req = b"DELETE /connect.json HTTP/1.1\r\nHost: 172.22.1.15\r\nOrigin: http://172.22.1.15\r\n\r\n";
    let (resp, action) = handle_request_bytes(req, &snap, None);
    assert_eq!(action, HttpControlAction::DisconnectWifi);
    assert!(resp.starts_with("HTTP/1.1 200 OK"));

    let bad_req = b"DELETE /connect.json HTTP/1.1\r\nHost: 172.22.1.15\r\nOrigin: http://evil.example.com\r\n\r\n";
    let (bad_resp, bad_action) = handle_request_bytes(bad_req, &snap, None);
    assert_eq!(bad_action, HttpControlAction::None);
    assert!(bad_resp.starts_with("HTTP/1.1 403 Forbidden"));
}

#[test]
fn test_captive_portal_probes_return_200_or_302() {
    let snap = sample_snapshot();
    for path in ["/generate_204", "/mobile/status.php"] {
        let req = format!("GET {} HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n", path);
        let (resp, _) = handle_request_bytes(req.as_bytes(), &snap, None);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
    }
    for path in ["/gen_204", "/hotspot-detect.html", "/hotspotdetect.html", "/library/test/success.html", "/success.txt"] {
        let req = format!("GET {} HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n", path);
        let (resp, _) = handle_request_bytes(req.as_bytes(), &snap, None);
        assert!(resp.starts_with("HTTP/1.1 302 Found"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app test_get_ap_json`
Expected: FAIL (`404 Not Found` on `/ap.json` and `HttpControlAction::DisconnectWifi` not defined).

- [ ] **Step 3: Implement `HttpMethod::Delete`, `GET /ap.json`, `DELETE /connect.json`, and captive portal routes in `rust/crates/louder-esp32-app/src/http_control.rs`**

Parse `DELETE ` in `parse_http_method`, enforce `is_permitted_origin` on both `HttpMethod::Post` and `HttpMethod::Delete`, add `GET /ap.json` serializing `[{"ssid":...,"chan":...,"rssi":...,"auth":...,"known":...}]`, update `POST /connect.json` to also parse optional `host_name`, and route the 7 captive portal paths (`/generate_204` and `/mobile/status.php` -> `200 OK`; `/gen_204`, `/hotspot-detect.html`, `/hotspotdetect.html`, `/library/test/success.html`, `/success.txt` -> `302 Found` with `Location: http://<ip>/`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /home/console-admin/Github/squeezelite-esp32 add rust/crates/louder-esp32-app/src/http_control.rs rust/crates/louder-esp32-app/src/tasks.rs rust/crates/louder-esp32-app/src/main.rs
git -C /home/console-admin/Github/squeezelite-esp32 commit -m "feat(web): add DELETE /connect.json, GET /ap.json, and captive portal routes"
```

---

### Task 2: Full C + Rust Dual-Schema Parity for `/status.json` and `/config.json` (with GPIO Map & NVS Persistence)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/config_handle.rs`
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

**Interfaces:**
- Consumes: `ConfigHandle` from `crate::config_handle`
- Produces:
  - Extended `HttpStatusSnapshot` containing C-parity fields: `jack_inserted: bool`, `voltage_mv: u16`, `disconnect_count: u32`, `avg_conn_time_ms: u32`, `is_i2c_locked: bool`, `urc: u8`, `bt_status: u8`, `bt_sub_status: u8`
  - Extended `HttpConfigSnapshot` & `format_config_json` emitting `"settings_seq"`, `"gpio": [{"gpio": 21, "name": "SDA", "group": "I2C", "fixed": true}, ...]`, and `"config"` containing both flat compatibility fields and C-typed `{ "value": "...", "type": 33 }` entries (with secrets stripped)
  - `HttpControlAction::UpdateConfig` parsing both flat `key=val` / `{"key":"...","value":"..."}` and nested `{"config":{"<key>":{"value":"...","type":...}}}` + `"fwurl"`

- [ ] **Step 1: Write the failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs`**

```rust
#[test]
fn test_status_json_includes_all_c_and_rust_fields() {
    let snap = sample_snapshot();
    let req = b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, _) = handle_request_bytes(req, &snap, None);
    let body = extract_http_body(&resp);
    for key in [
        "\"project_name\":", "\"version\":", "\"recovery\":", "\"Jack\":",
        "\"Voltage\":", "\"disconnect_count\":", "\"avg_conn_time\":",
        "\"is_i2c_locked\":", "\"urc\":", "\"bt_status\":", "\"bt_sub_status\":",
        "\"frames_played\":", "\"tas_pll_locked\":", "\"psram_free_bytes\":",
    ] {
        assert!(body.contains(key), "missing key {}", key);
    }
}

#[test]
fn test_config_json_get_includes_gpio_and_strips_secrets() {
    let snap = sample_snapshot();
    let req = b"GET /config.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, _) = handle_request_bytes(req, &snap, None);
    let body = extract_http_body(&resp);
    assert!(body.contains("\"gpio\":["));
    assert!(body.contains("\"name\":\"SDA\""));
    assert!(body.contains("\"autoexec1\""));
    assert!(!body.contains("\"password\""));
    assert!(!body.contains("\"web_pwd\""));
}

#[test]
fn test_config_json_post_accepts_nested_c_schema_and_fwurl() {
    let snap = sample_snapshot();
    let req = b"POST /config.json HTTP/1.1\r\nHost: 172.22.1.15\r\nOrigin: http://172.22.1.15\r\nContent-Type: application/json\r\n\r\n{\"config\":{\"host_name\":{\"value\":\"BASEMENT\",\"type\":33}}}";
    let (resp, action) = handle_request_bytes(req, &snap, None);
    assert!(resp.starts_with("HTTP/1.1 200 OK"));
    assert!(body_contains_ok_or_result(&resp));
    assert!(matches!(action, HttpControlAction::UpdateConfig { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app test_config_json`
Expected: FAIL (missing `"gpio":[` and `"Jack":`).

- [ ] **Step 3: Implement C+Rust `/status.json` fields, `BOARD_GPIO_MAP`, and dual-schema `/config.json` in `config_handle.rs` and `http_control.rs`**

Add the 8 C status fields to `HttpStatusSnapshot` (defaulting `Jack` to `"0"`, `Voltage` to `0`, `urc` to `0` when connected, `bt_status` derived from `bt_connected`/`bt_streaming`). Add Louder-ESP32 fixed GPIO pin descriptors (`I2C SDA=21`, `I2C SCL=22`, `I2S BCK=26`, `I2S WS=25`, `I2S DO=22/23`, `TAS PDN=17`) and configurable NVS keys (`host_name`, `autoexec`, `autoexec1`, `lms_ip`, `lms_port`, `lms_cport`, `vol_pct`, `eq_preset`, `again_code`, `enable_airplay`, `enable_cspot`, `enable_bt_sink`, `enable_telnet`, `dac_config`, `i2c_config`, `rotary_config`) to `GET /config.json` and `POST /config.json`. Ensure `POST /config.json` returns `{"ok":true,"result":"OK",...}` so both C `custom.js` and Rust tests succeed.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /home/console-admin/Github/squeezelite-esp32 add rust/crates/louder-esp32-app/src/config_handle.rs rust/crates/louder-esp32-app/src/http_control.rs rust/crates/louder-esp32-app/src/tasks.rs rust/crates/louder-esp32-app/src/main.rs
git -C /home/console-admin/Github/squeezelite-esp32 commit -m "feat(web): add full C/Rust parity for /status.json and /config.json GPIO & NVS schemas"
```

---

### Task 3: Full `/commands.json` Catalog (`argtable` + `values`) and `/messages.json` Syslog & Task Stats Engine

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

**Interfaces:**
- Produces:
  - `pub struct SyslogRingBuffer` (fixed-capacity 16-entry ring buffer storing `seq`, `uptime_ms`, `level`, `msg_class`, `msg`)
  - Updated `format_messages_json` emitting both `"messages": [...]` and C-compatible `type` (`MESSAGING_INFO`/`MESSAGING_WARNING`/`MESSAGING_ERROR`), `class` (`MESSAGING_CLASS_SYSTEM`/`MESSAGING_CLASS_STATS`), `sent_time`, `current_time`, and embedded task/heap telemetry
  - Updated `format_commands_json` including `argtable` and `values` for `cfg-audio-general`, `cfg-audio-tmpl`, `cfg-syst-services`, `cfg-syst-name`, `cfg-hw-preset`, `cfg-hw-dac`, `cfg-hw-spdif`, `cfg-hw-rotary`, `cfg-hw-i2c`, `cfg-hw-spi`, `cfg-hw-display`, and diagnostic CLI commands (`free`, `heap`, `version`, `restart`, `recovery`, `restart_ota`, `i2cdetect`, `i2cget`, `i2cset`, `i2cdump`, `system`, `i2c`, `config`, `join`, `update`, `player`, `sleep`, `help`)
  - Updated `POST /commands.json` accepting both `cmd=<str>` and `{"command":"<str>"}` and returning `{"ok":true,"Result":"Success","cmd":"<redacted>"}`

- [ ] **Step 1: Write the failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs`**

```rust
#[test]
fn test_commands_json_includes_argtable_and_values_and_accepts_c_post_body() {
    let snap = sample_snapshot();
    let req = b"GET /commands.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, _) = handle_request_bytes(req, &snap, None);
    let body = extract_http_body(&resp);
    assert!(body.contains("\"commands\":["));
    assert!(body.contains("\"argtable\":["));
    assert!(body.contains("\"values\":{"));
    assert!(body.contains("\"cfg-syst-services\""));
    assert!(body.contains("\"i2cdetect\""));

    let post_req = b"POST /commands.json HTTP/1.1\r\nHost: 172.22.1.15\r\nOrigin: http://172.22.1.15\r\nContent-Type: application/json\r\n\r\n{\"command\":\"i2cdetect\"}";
    let (post_resp, action) = handle_request_bytes(post_req, &snap, None);
    let post_body = extract_http_body(&post_resp);
    assert!(post_body.contains("\"Result\":\"Success\""));
    assert!(post_body.contains("\"ok\":true"));
    assert!(matches!(action, HttpControlAction::ExecuteCliCommand { .. }));
}

#[test]
fn test_messages_json_includes_system_and_stats_classes() {
    let snap = sample_snapshot();
    let req = b"GET /messages.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, _) = handle_request_bytes(req, &snap, None);
    let body = extract_http_body(&resp);
    assert!(body.contains("\"messages\":["));
    assert!(body.contains("MESSAGING_CLASS_STATS"));
    assert!(body.contains("MESSAGING_INFO"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app test_commands_json`
Expected: FAIL (missing `"argtable":[` and `"Result":"Success"`).

- [ ] **Step 3: Implement `SyslogRingBuffer`, enhanced `/messages.json`, and full `/commands.json` schema in `http_control.rs`**

Add `SyslogRingBuffer` with fixed-size entries, synthesize the `MESSAGING_CLASS_STATS` task/heap diagnostic payload from `HttpStatusSnapshot` (`psram_free_bytes`, `psram_used_bytes`, `core1_active`, `core1_loops`, `decode_us_max`), and extend `/commands.json` `GET` and `POST` handlers.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /home/console-admin/Github/squeezelite-esp32 add rust/crates/louder-esp32-app/src/http_control.rs
git -C /home/console-admin/Github/squeezelite-esp32 commit -m "feat(web): add C-compatible /commands.json argtables and /messages.json syslog & task stats"
```

---

### Task 4: Wire All Stubbed `HttpControlAction` Handlers in `tasks.rs` and `main.rs`

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/tasks.rs`
- Modify: `rust/crates/louder-esp32-app/src/main.rs`
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

**Interfaces:**
- Consumes: `HttpControlAction`, `ConfigHandle`, `FlashJournal`, `Console`, `SyslogRingBuffer`
- Produces:
  - `pub fn execute_web_cli_command(cmd: &str, snap: &HttpStatusSnapshot, config: &mut ConfigHandle, syslog: &mut SyslogRingBuffer) -> bool`
  - Live execution of `UpdateConfig`, `ExecuteCliCommand`, `TriggerWifiScan`, `ConnectWifi`, `DisconnectWifi`, and `StartOtaFlash` in both `tasks.rs` and `main.rs`

- [ ] **Step 1: Write the failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs`**

```rust
#[test]
fn test_execute_web_cli_command_populates_syslog_for_diagnostics_and_cfg() {
    let snap = sample_snapshot();
    let mut cfg = crate::config_handle::ConfigHandle::new();
    let mut syslog = SyslogRingBuffer::new();

    assert!(execute_web_cli_command("free", &snap, &mut cfg, &mut syslog));
    assert!(syslog.latest_message().contains("PSRAM free:"));

    assert!(execute_web_cli_command("i2cdetect", &snap, &mut cfg, &mut syslog));
    assert!(syslog.latest_message().contains("0x2D"));

    assert!(execute_web_cli_command("cfg-syst-name --host_name STUDIO", &snap, &mut cfg, &mut syslog));
    assert_eq!(cfg.host_name_str(), "STUDIO");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app test_execute_web_cli_command`
Expected: FAIL (`execute_web_cli_command` not defined).

- [ ] **Step 3: Implement `execute_web_cli_command` and wire all 6 `HttpControlAction` variants in `tasks.rs` and `main.rs`**

Implement `execute_web_cli_command` handling `free`, `heap`, `version`, `i2cdetect`, `i2cget`, `i2cset`, `i2cdump`, `system`, `i2c`, `config`, `join`, `update`, `player`, `sleep`, `help`, and `cfg-*` commands, logging structured output into `SyslogRingBuffer`. In `tasks.rs` and `main.rs`, wire `UpdateConfig` (updates `ConfigHandle` key, persists to `FlashJournal`, logs to `SyslogRingBuffer`), `ExecuteCliCommand`, `TriggerWifiScan`, `ConnectWifi`, `DisconnectWifi`, and `StartOtaFlash`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /home/console-admin/Github/squeezelite-esp32 add rust/crates/louder-esp32-app/src/http_control.rs rust/crates/louder-esp32-app/src/config_handle.rs rust/crates/louder-esp32-app/src/tasks.rs rust/crates/louder-esp32-app/src/main.rs
git -C /home/console-admin/Github/squeezelite-esp32 commit -m "feat(web): wire UpdateConfig, ExecuteCliCommand, WiFi scan/connect, and OTA actions"
```

---

### Task 5: Full 10-Tab Embedded Web UI (`web_ui.rs`) Combining Original C Webapp & MK5-Rust Dashboard

**Files:**
- Create: `rust/crates/louder-esp32-app/src/web_ui.rs`
- Modify: `rust/crates/louder-esp32-app/src/lib.rs`
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

**Interfaces:**
- Produces: `pub const WEB_UI_HTML: &str` in `rust/crates/louder-esp32-app/src/web_ui.rs`, served by `GET /` and `GET /index.html` in `http_control.rs` via chunked `.rodata` streaming (`HttpStreamResponse`).

- [ ] **Step 1: Write the failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs`**

```rust
#[test]
fn test_web_ui_contains_all_c_tabs_and_rust_dashboard_controls() {
    let snap = sample_snapshot();
    let req = b"GET / HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
    let (resp, _) = handle_request_bytes(req, &snap, None);
    let body = extract_http_body(&resp);
    for marker in [
        "id=\"tab-player\"",
        "id=\"tab-wifi\"",
        "id=\"tab-syslog\"",
        "id=\"tab-cfg-audio\"",
        "id=\"tab-cfg-syst\"",
        "id=\"tab-cfg-hw\"",
        "id=\"tab-cfg-fw\"",
        "id=\"tab-nvs\"",
        "id=\"tab-commands\"",
        "id=\"tab-credits\"",
        "id=\"navtitle\"",
        "id=\"wifiTable\"",
        "id=\"syslogTable\"",
        "id=\"gpiotable\"",
        "id=\"tasks\"",
        "id=\"save-autoexec1\"",
        "id=\"save-nvs\"",
        "id=\"fwUpload\"",
        "eq_loudness",
        "again4",
        "/recovery.json",
        "/reboot_ota.json",
    ] {
        assert!(body.contains(marker), "missing UI element {}", marker);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app test_web_ui_contains_all_c_tabs`
Expected: FAIL (missing `id="tab-wifi"`, `id="tab-syslog"`, `id="tab-nvs"`, etc.).

- [ ] **Step 3: Implement `web_ui.rs` with all 10 tabs, header/footer telemetry badges, and interactive JS controllers**

Create `rust/crates/louder-esp32-app/src/web_ui.rs` with a dark Slate-themed responsive UI (`WEB_UI_HTML`) placed in `.rodata` containing:
1. **Global Header & Status Badges**: `#navtitle` hostname, Recovery badge, Jack/Voltage badge, AirPlay (`enable_airplay`), Spotify (`enable_cspot`), Output mode (`I2S`/`SPDIF`/`BT`), Network interface (`Wi-Fi` RSSI / `Ethernet`).
2. **Tab 1 — Player & DSP (`#tab-player`)**: Full MK5-Rust real-time hardware telemetry grid (1.5s poll of `/status.json`), Transport & Volume controls, Stream Presets (`Ibiza`, `WHYY`, `WDIY`, `Groove`), 10-Band EQ selector, TAS5805M Analog Gain (`AGAIN`) selector, LMS Web UI link, and Anti-Brick Recovery / Commit OTA buttons.
3. **Tab 2 — WiFi (`#tab-wifi`)**: Scanned AP table (`#wifiTable` via `/scan.json` + `/ap.json`), Connect dialog (`POST /connect.json`), Disconnect confirmation (`DELETE /connect.json`), and Ethernet banner.
4. **Tab 3 — Status / Syslog (`#tab-syslog`)**: Live event log table (`#syslogTable` from `/messages.json` with `#clear-syslog`), GPIO Pin Assignments table (`#gpiotable` from `/config.json`), and Core 0/1 & Heap/PSRAM Runtime Tasks table (`#tasks`).
5. **Tab 4 — Audio (`#tab-cfg-audio`)**: Output template selector (`I2S`, `SPDIF`, `BT`), interactive Squeezelite Command-Line Builder (`-n`, `-s`, `-b`, `-c`, `-C`, `-d`, `-e`, `-m`, `-r`, `-R`, `-Z`, `-W`, Disable toggle) serializing to `autoexec1` (`#save-autoexec1`, `#commit-autoexec1`), and `cfg-audio-*` cards.
6. **Tab 5 — System (`#tab-cfg-syst`)**: Service toggles (`enable_cspot`, `enable_bt_sink`, `enable_airplay`, `enable_telnet`, `autoexec`) and device names (`host_name`, `bt_name`, `ap_name`, `cspot_name`) with Save & Apply buttons.
7. **Tab 6 — Hardware (`#tab-cfg-hw`)**: Board preset selector (`Louder-ESP32`, `SqueezeAMP`, `ESP32-LyraT`, `Generic-I2S`) and GPIO configuration cards (`dac_config`, `spdif_config`, `rotary_config`, `i2c_config`, `spi_config`, `display_config`).
8. **Tab 7 — Updates / OTA (`#tab-cfg-fw`)**: Firmware URL OTA trigger (`#fw-url-input` -> `POST /config.json` `fwurl`), local `.bin` file uploader (`#flashfilename`, `#fwUpload` -> `POST /flash.json` with progress bar `#otadiv`), and Reboot to Recovery (`#btn_reboot_recovery`).
9. **Tab 8 — NVS Editor (`#tab-nvs`)**: Dynamic key/type/value editor from `/config.json` with masked secrets, new key row, Commit (`#save-nvs`), JSON export (`#save-as-nvs`), JSON import (`#load-nvs`), and Reboot (`#btn_reboot`).
10. **Tab 9 — Console Commands (`#tab-commands`)**: Dynamic CLI cards from `/commands.json` (`free`, `heap`, `version`, `i2cdetect`, `i2cget`, `i2cset`, `i2cdump`, `system`, `config`, `player`, etc.) posting to `POST /commands.json` and displaying output.
11. **Tab 10 — Credits (`#tab-credits`)**: Credits list and visibility checkboxes (`#show-nvs`, `#show-commands`) persisted in `localStorage`/cookies.
12. **Fixed Footer Bar**: `#foot-fw`, `#foot-if`, `#reboot_nav` (`POST /reboot.json`), and `#reboot_ota_nav` (`POST /reboot_ota.json`).

Ensure `HttpStreamResponse` buffer ceiling in `http_control.rs` streams the full `WEB_UI_HTML` across TCP segments cleanly.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
Expected: PASS across the entire workspace.

- [ ] **Step 5: Commit**

```bash
git -C /home/console-admin/Github/squeezelite-esp32 add rust/crates/louder-esp32-app/src/web_ui.rs rust/crates/louder-esp32-app/src/lib.rs rust/crates/louder-esp32-app/src/http_control.rs
git -C /home/console-admin/Github/squeezelite-esp32 commit -m "feat(web): implement 10-tab web UI achieving full C webapp and MK5-Rust parity"
```

---

### Task 6: Strict Static Audit, Bare-Metal `xtensa-esp32-none-elf` Stage-Only Build, and ELF Section Certification

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs` (if any clippy/audit adjustments needed)

**Interfaces:**
- Consumes: All changes from Tasks 1–5
- Produces: Verified `target/xtensa-esp32-none-elf/release/louder-esp32-app` ELF and staged OTA binary meeting all memory and style gates.

- [ ] **Step 1: Run Google Rust Style Audit and Clippy**

Run:
```bash
/usr/bin/python3 "/home/console-admin/.gemini/config/skills/rust-pro/scripts/google_rust_audit.py" /home/console-admin/Github/squeezelite-esp32/rust --strict
cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings
```
Expected: `0` errors, `0` warnings.

- [ ] **Step 2: Build bare-metal `xtensa-esp32-none-elf` release image and verify section sizes**

Run:
```bash
cargo +esp build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --target xtensa-esp32-none-elf --release -p louder-esp32-app --bin louder-esp32-app
```
Expected: Build succeeds; verify `.stack >= 36,000` B, `.iram0.text <= 131,072` B, `.dram2_uninit <= 98,768` B, and staged `.bin <= 2,686,976` B.
