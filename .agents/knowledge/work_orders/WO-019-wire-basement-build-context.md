# Work Order: WO-019 — Wire `172.22.1.15` (`"BASEMENT"`) Build Context & Live LMS Connection into `louder-esp32-app`

**Work Order ID:** WO-019  
**Timestamp:** 2026-10-20T22:35:00-04:00  
**Assigned Subagent Role:** Rust App Implementer  
**Target Files:**
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
**Backup Files:**
- `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-board.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-amp.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/louder-main.rs.bak`
**Defect IDs:** `HW-PREP-05`  
**Task Goal:** Incorporate the live `172.22.1.15` (`"BASEMENT"`) hardware & network build context into `louder-esp32-app/src/{board.rs,amp.rs,main.rs}` (`DEFAULT_LMS_SERVER_IP = [10, 0, 0, 10]`, `DEFAULT_LMS_SERVER_PORT = 3483`, `DEFAULT_PLAYER_NAME = "BASEMENT"`, `DEFAULT_WIFI_SSID = "iot"`, `DEFAULT_PLAYER_MAC = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34]`, `AmpManager::start_playback_via_hiz`, and live LMS connection loop in `main.rs`).

---

## 1. Exact Requirements
1. First copy current `board.rs`, `amp.rs`, and `main.rs` to their backup paths in `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/`.
2. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`**:
   - Add target deployment constants matching `172.22.1.15` (`"BASEMENT"`):
     ```rust
     /// Default Lyrion Music Server (LMS) IPv4 address from target build context (`10.0.0.10`).
     pub const DEFAULT_LMS_SERVER_IP: [u8; 4] = [10, 0, 0, 10];
     /// Default Lyrion Music Server (LMS) SlimProto TCP/UDP port (`3483`).
     pub const DEFAULT_LMS_SERVER_PORT: u16 = 3483;
     /// Default deployed player node name (`"BASEMENT"` on `172.22.1.15`).
     pub const DEFAULT_PLAYER_NAME: &str = "BASEMENT";
     /// Default deployed Wi-Fi SSID (`"iot"` on `172.22.1.15`).
     pub const DEFAULT_WIFI_SSID: &str = "iot";
     /// Hardware MAC address of the `172.22.1.15` `"BASEMENT"` ESP32 unit (`c8:2e:18:be:b8:34`).
     pub const BASEMENT_PLAYER_MAC: [u8; 6] = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34];
     ```
   - Add unit test in `board.rs` verifying these constants.
3. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`**:
   - Add `pub fn start_playback_via_hiz(&mut self) -> Result<(), DriverError<E>>` on `AmpManager<I2C>`:
     - Mirrors `start_playback(&mut self)`, except it invokes `driver.try_start_playback_via_hiz()` (writing `DIG_VOL_CTRL`, `FAULT_CLEAR = 0x80`, `DEVICE_CTRL_2 = 0x02` Hi-Z -> `DEVICE_CTRL_2 = 0x03` Play, matching `172.22.1.15`'s `dac_controlset` `{"init":[{"reg":3,"val":2},{"reg":3,"val":3}]}`).
   - Add unit test `test_amp_manager_start_playback_via_hiz` in `amp.rs`.
4. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`**:
   - Populate `audio_cfg.lms_server_ip = Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_IP)` and `audio_cfg.lms_server_port = Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_PORT)` by default, and allow overriding player name via `std::env::var("SL_PLAYER_NAME")` and LMS server address via `std::env::var("SL_LMS_SERVER")`.
   - Use `amp.start_playback_via_hiz().expect("Failed to start playback mode via Hi-Z on TAS5805M")` in `main()` so the startup sequence matches `172.22.1.15`'s `dac_controlset` (`0x02 -> 0x03`).
   - Support optional live LMS network connection in `main()`: if `std::env::var("SL_LMS_CONNECT").as_deref() == Ok("1")` (or on `#[cfg(target_arch = "xtensa")]`), connect a `SlimprotoClient` to the configured LMS server (`10.0.0.10:3483` or `SL_LMS_SERVER`) using `SlimprotoClient::connect_with_options(lms_addr, mac, &audio_cfg.player_name, false, false)` and pass ` client.as_mut()` into `run_firmware_tick_with_network(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp, client.as_mut(), now_ms)`.
5. **Verification**:
   - Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
   - Run `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets --all-features -- -D warnings`
   - Run a live 5-tick handshake verification against `10.0.0.10:3483` using a non-destructive test player name (`SL_PLAYER_NAME="MK5-Rust-Test" SL_LMS_CONNECT="1" /home/console-admin/.cargo/bin/cargo run --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`) so we don't kick `"BASEMENT"` (`c8:2e:18:be:b8:34`) off LMS while verifying live SlimProto connectivity!
6. Wait for all background tasks to complete (`manage_task(list)` = 0 active tasks per Rule 21), write a Rule 17 summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo019_basement_context_wiring.md`, and update the Fulfillment Section below to `[EXECUTED & VERIFIED]`.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T22:43:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo019_basement_context_wiring.md`
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Verification Evidence:**
  - `cargo test --workspace`: 158 passed, 0 failed.
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`: 0 warnings, exit 0.
  - Live Slimproto test against `10.0.0.10:3483` with `SL_PLAYER_NAME="MK5-Rust-Test" SL_LMS_CONNECT="1"`: connected successfully, executed 5 ticks, exit 0.
