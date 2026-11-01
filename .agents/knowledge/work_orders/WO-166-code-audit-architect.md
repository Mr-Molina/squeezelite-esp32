# Work Order `WO-166`: `/deep-code-audit` Phase 0b — Code Audit Architect (`rust-pro` + Boundary & Robustness Slicing)

- **ID**: `WO-166`
- **Timestamp**: `2026-10-27T17:15:00-04:00`
- **Subagent Role**: Code Audit Architect (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Conversation ID**: `f9b0b95a-d7c5-444a-a60f-dd49b45497d9`

## 1. Scope & Instructions
You are the **Code Audit Architect** (`/deep-code-audit` Phase 0b + `rust-pro` Phase 0/1).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/discovery-protocol.md` (§2), `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md`, `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`, and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Merged Raw Code Scan Report (`WO-158`..`WO-161`)
```yaml
Raw Scan Report:
  Root Path: /home/console-admin/Github/squeezelite-esp32/rust
  Scoped Audit Surface: 42 files, 37,899 LOC across 8 workspace member crates + 2 patched crates (nanomp3-core, vendor/esp-radio)
  Crate Clusters (File counts & LOC):
    louder-esp32-app:
      total_loc: 16018
      files:
        - src/lib.rs (44 LOC, pure wiring)
        - src/main.rs (3131 LOC, bare-metal entrypoint — MUST BE SPLIT into 3 line-range micro-auditors: lines 1-1050, 1051-2100, 2101-3131)
        - src/bt_a2dp.rs (2791 LOC, Dual-Mode HCI/L2CAP/SDP/AVDTP/SBC — MUST BE SPLIT into 2 line-range micro-auditors: lines 1-1400, 1401-2791)
        - src/coordinator.rs (2248 LOC, PlayerCoordinator state machine — MUST BE SPLIT into 2 line-range micro-auditors: lines 1-1100, 1101-2248)
        - src/network.rs (1573 LOC, SlimprotoClient, HttpAudioStream, LmsDiscovery)
        - src/hal.rs (1156 LOC, EspI2sSink, CydUartBridge, NvsFlashBackend)
        - src/pipeline.rs (1140 LOC, StreamPipeline, ActiveDecoder, 10-band EQ)
        - src/http_control.rs (1087 LOC, Port 80 HTTP routes, status.json, cmd.json, DeadManLeaseWatchdog)
        - src/ota_safety.rs (1032 LOC, 4-Gate Anti-Brick, split 12KB NVS parser, otadata)
        - src/amp.rs (805 LOC, AmpManager, volume mapping, fault recovery)
        - src/wifi.rs (394 LOC, WifiManager)
        - src/cyd.rs (257 LOC, CydController)
        - src/sink.rs (198 LOC, NullAudioSink, SimulatedSink)
        - src/board.rs (162 LOC, board pinouts & defaults)
    squeezelite-decoders:
      total_loc: 2818
      files:
        - src/lib.rs (2818 LOC, PcmDecoder, Mp3Decoder, StaticMp3Decoder, FlacDecoder in 4MB PSRAM, AacDecoder, VorbisDecoder — MUST BE SPLIT into 2 line-range micro-auditors: lines 1-1400, 1401-2818)
    cyd-controller-app:
      total_loc: 3526
      files:
        - src/lib.rs (37 LOC), src/main.rs (584 LOC), src/comms.rs (752 LOC), src/drivers.rs (912 LOC), src/ui.rs (1241 LOC)
    squeezelite-proto:
      total_loc: 2193
      files:
        - src/lib.rs (1846 LOC), tests/proto_fuzz.rs (347 LOC)
    squeezelite-audio-core:
      total_loc: 1828
      files:
        - src/lib.rs (19 LOC), src/ringbuffer.rs (521 LOC), src/equalizer.rs (866 LOC), src/output.rs (130 LOC), src/volume.rs (117 LOC), tests/stress_test.rs (175 LOC)
    squeezelite-config:
      total_loc: 1477
      files:
        - src/lib.rs (1157 LOC), tests/config_fuzz.rs (320 LOC)
    squeezelite-tas58xx:
      total_loc: 1344
      files:
        - src/lib.rs (1344 LOC)
    squeezelite-cyd-link:
      total_loc: 917
      files:
        - src/lib.rs (727 LOC), tests/cyd_fuzz.rs (190 LOC)
    nanomp3-core:
      total_loc: 6311
      files:
        - src/lib.rs (226 LOC), src/minimp3.rs (1543 LOC), src/minimp3/layer12.rs (245 LOC), src/minimp3/lanes.rs (178 LOC), src/tests.rs (148 LOC), src/minimp3/tables.rs (3971 LOC constant lookup tables)
    vendor/esp-radio (modified BTDM files):
      total_loc: 1467
      files:
        - src/ble/btdm.rs (459 LOC), src/ble/mod.rs (312 LOC), src/ble/os_adapter_esp32.rs (696 LOC), ld/esp32_provides.x (62 LOC)

  Static Tool & rust-pro Hotspots (WO-161):
    cargo_clippy: PASS (0 warnings, 0 errors)
    cve_audit (WO-157): PASS (0 exploitable CVEs across 230 crates)
    google_rust_audit_strict:
      exit_code: 1
      total_errors: 51
      total_warnings: 809
      GRS-001 (Nested :: in use braces): 3 errors (cyd-controller-app/src/main.rs:30, louder-esp32-app/src/main.rs:437,450)
      GRS-002 (Disallowed glob import): 8 errors (nanomp3-core/src/{minimp3.rs:20,tests.rs:1}, tests/{config_fuzz,cyd_fuzz,proto_fuzz}.rs)
      GRS-003 (Definitions in non-leaf lib.rs): 15 errors (nanomp3-core/src/lib.rs)
      GRS-008 (Unsafe missing dual // JUSTIFICATION: + // SAFETY:): 25 errors (louder-esp32-app/src/main.rs: 15 blocks, squeezelite-audio-core/src/ringbuffer.rs: 9 blocks, nanomp3-core/src/minimp3/layer12.rs: 1 block)
      GRS-010 (Production .unwrap()): 9 warnings (louder-esp32-app/src/main.rs:1232,1253; nanomp3-core: 7 occurrences)
      GRS-011 (#[rustfmt::skip] missing justification comment): 1 warning (nanomp3-core/src/minimp3/layer12.rs:32)
      GRS-012 (Line length >100 code / >80 comment): 799 warnings across 33 files
    Manifest & Lint Config (WO-159):
      rustfmt.toml: MISSING (needs imports_granularity = "Module", max_width = 100, comment_width = 80)
      workspace.lints: MISSING in rust/Cargo.toml
```

### Task Deliverables
1. Slice the entire `rust/` codebase into **14 tightly-scoped Micro-Auditors** (`<= 1,500` LOC each, splitting `main.rs`, `bt_a2dp.rs`, `coordinator.rs`, and `squeezelite-decoders/src/lib.rs` by line ranges) organized into **2 Parallel Cohorts** (Cohort 1: 7 auditors, Cohort 2: 7 auditors).
2. Inject both **functional/robustness/hardware boundary checks** (including the `rust-pro` Bare-Metal `esp-hal 1.2.2` Cold-Boot & MMIO invariants: `RTC_WDT` re-arming, PSRAM `SPI0`/`SPI1`, zero boot-time flash deadlocks, DMA alignment, SlimProto/FLAC/Bluetooth edge cases) AND **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)** hotspots into each auditor's assignment.
3. Return the complete **Committee Roster YAML** directly in your final response. Do NOT modify any source files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `f9b0b95a-d7c5-444a-a60f-dd49b45497d9`
- **Summary**: Sliced the `37,899` LOC Rust workspace across 42 source files into 14 bounded Micro-Auditors (`AUD-01`..`AUD-14`) organized into 2 Parallel Cohorts (`WO-174`..`WO-180` Cohort 1, `WO-181`..`WO-187` Cohort 2), injecting both functional/hardware boundary invariants and `rust-pro` (`GRS-001`..`GRS-012`) static hotspots into each micro-auditor scope.
