# Work Order `WO-120`: `/deep-code-audit` Phase 0b — Code Audit Architect & Committee Assembly

- **ID**: `WO-120`
- **Timestamp**: `2026-10-25T20:25:45-04:00`
- **Subagent Role**: Audit Architect (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are the **Phase 0b Code Audit Architect** for `/deep-code-audit` of the `MK5-Rust` repository (`/home/console-admin/Github/squeezelite-esp32/rust`).
Read:
- `/home/console-admin/.gemini/config/skills/deep-code-audit/references/discovery-protocol.md` (§2)
- `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md`
- `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`
- `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md` (especially Invariants 44–47)

### Consolidated Phase 0a Raw Code Scan Report (`WO-112`..`WO-115`)
- **Scale**: Medium Polyglot/Embedded Pure-Rust Workspace (`9` crates, `38` `.rs` files, `31,395` total LOC).
- **LOC per Crate / File Cluster (`WO-112`)**:
  1. `louder-esp32-app` (`12,287` LOC across 13 files):
     - `src/main.rs` (`2,865` LOC — bare-metal `xtensa` entry, Gate 1 `otadata`, PSRAM, Core 1 `APP_CPU`, `I2S0` DMA, `I2C0`, `UART2`, `smoltcp` 4-socket loop)
     - `src/coordinator.rs` (`2,127` LOC — `PlayerCoordinator`, SlimProto lifecycle, CYD telemetry sync)
     - `src/network.rs` (`1,495` LOC — `SlimprotoClient`, `HttpAudioStream`, ICY metadata, redirects, chunked encoding)
     - `src/hal.rs` (`1,108` LOC — `NvsFlashBackend`, `EspI2sSink`, `CydUartBridge`, `TargetI2cBus`, `EspWifiAdapter`)
     - `src/http_control.rs` (`977` LOC — Port 80 HTTP server, `/status.json`, `/cmd.json`, `/recovery.json`, `/reboot.json`, `DASHBOARD_HTML`, `DeadManLeaseWatchdog`)
     - `src/pipeline.rs` (`970` LOC — `StreamPipeline`, multi-codec dispatcher, 10-band EQ, PSRAM/ringbuffer pumping)
     - `src/ota_safety.rs` (`951` LOC — 4-Gate Anti-Brick verification, `load_nvs_wifi_credentials_split_12kb`, `derive_wifi_nvs_key`, `verify_ota_binary_header`)
     - `src/amp.rs` (`803` LOC — `AmpManager`, `TAS5805M` lifecycle, volume, analog gain `AGAIN`, live telemetry)
     - `src/wifi.rs` (`349` LOC), `src/cyd.rs` (`240` LOC), `src/sink.rs` (`198` LOC), `src/board.rs` (`162` LOC), `src/lib.rs` (`42` LOC)
  2. `cyd-controller-app` (`3,448` LOC across 5 files): `src/ui.rs` (`1,198`), `src/drivers.rs` (`912`), `src/comms.rs` (`741`), `src/main.rs` (`560`), `src/lib.rs` (`37`).
  3. `squeezelite-proto` (`2,161` LOC): `src/lib.rs` (`1,814`), `tests/proto_fuzz.rs` (`347`).
  4. `squeezelite-decoders` (`1,876` LOC): `src/lib.rs` (`1,876` — `PcmDecoder`, `StaticMp3Decoder`, `Mp3Decoder`, `FlacDecoder`, `AacDecoder`, `VorbisDecoder`).
  5. `squeezelite-audio-core` (`1,775` LOC): `src/equalizer.rs` (`850`), `src/ringbuffer.rs` (`495`), `src/output.rs` (`130`), `src/volume.rs` (`106`), `tests/stress_test.rs` (`175`), `src/lib.rs` (`19`).
  6. `squeezelite-config` (`1,457` LOC): `src/lib.rs` (`1,137` — `FlashJournal`, `DualSectorFlashStore`, `AudioConfig`), `tests/config_fuzz.rs` (`320`).
  7. `squeezelite-tas58xx` (`1,257` LOC): `src/lib.rs` (`1,257` — `Tas58xx` type-state driver, `1SPW`, `AGAIN`, `read_telemetry`, fault recovery).
  8. `squeezelite-cyd-link` (`825` LOC): `src/lib.rs` (`665`), `tests/cyd_fuzz.rs` (`160`).
  9. `nanomp3-core` (`6,309` LOC): `src/minimp3.rs` (`1,541`), `src/minimp3/layer12.rs` (`245`), `src/minimp3/lanes.rs` (`178`), `src/lib.rs` (`226`), `src/tests.rs` (`148`), `src/minimp3/tables.rs` (`3,971` static lookup tables).
- **Static Hotspots (`WO-115`)**:
  - `cargo clippy --workspace --all-targets`: `0` warnings, `0` errors.
  - `unsafe` sites (`18` unsafe blocks, `3` unsafe `Send` impls, `5` unsafe attributes):
    - `louder-esp32-app/src/main.rs`: lines `489`, `888`, `929`, `1034`, `1051`, `1054`, `1057`, `1060`, `1063`, `1109`, `1206`, `1207`, `1210`, `1239`, `1263` (plus `set_clock_rate` register writes, PSRAM slice construction, `APP_CPU` parking/MMU sync).
    - `squeezelite-audio-core/src/ringbuffer.rs`: lines `56`, `118`, `122`, `180`, `184`, `212`, `216`, `280`, `299` (`UnsafeCell` SPSC ringbuffer & `Send` impls).
    - `nanomp3-core/src/minimp3.rs:1098` and `src/minimp3/layer12.rs:230` (pointer cast on `grbuf`).

### Deliverable
Slice the entire Rust workspace into tightly-scoped **Phase 1 Micro-Auditors** (strictly `<= 1,500` LOC of logic per micro-auditor; split giant files like `louder-esp32-app/src/main.rs` (`2,865` LOC) and `src/coordinator.rs` (`2,127` LOC) by line range or subsystem concern!). Organize them into 2 parallel cohorts (~6–8 micro-auditors per cohort, 12–14 total micro-auditors) and return the YAML **Committee Roster** with exact file/line scopes, static hotspots, checklist seeds, and constructive engineering prompts (`GLB-016` compliant). Do NOT read or audit full source files yourself.

## 2. Fulfillment Section
*(To be completed by subagent)*
