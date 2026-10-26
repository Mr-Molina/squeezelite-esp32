---
name: deep-code-analysis
description: Project-specialized deep code analysis engine for squeezelite-esp32 (Profile 1: Embedded IoT & MCU + Profile 6: Polyglot Rust Workspace).
---

<!-- GENERATOR_VERSION: 1.0.0 -->
<!-- GENERATED_DATE: 2026-10-20 -->
<!-- TARGET_PROFILES: Profile 1: Embedded IoT & MCU (Primary), Profile 6: Polyglot Generic Rust (Secondary) -->

# Deep Code Analysis (squeezelite-esp32)

## Overview

Specialized analytical engine tailored for **squeezelite-esp32** running **Profile 1: Embedded IoT & MCU (ESP32/ESP32-S3, FreeRTOS, bare-metal `esp-hal`)** and **Profile 6: Pure-Rust Cargo Workspace (`rust/`)**.
Anchored to workspace root: [`/home/console-admin/Github/squeezelite-esp32`](file:///home/console-admin/Github/squeezelite-esp32)

---

## Active Project Knowledge Ledgers

| Ledger | Path | Description |
| :--- | :--- | :--- |
| **Device & System State** | [`DEVICE_STATE.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DEVICE_STATE.md) | Hardware configurations, offsets, and board variants |
| **Active Bug Tracker** | [`ACTIVE_BUGS.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/ACTIVE_BUGS.md) | Active failure reports, panics, and bug hypotheses |
| **Diagnostic Scratchpad** | [`DIAGNOSTIC_LOG.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md) | Persistent empirical tool findings (Rule 11/17 scratchpad) |
| **Pattern Library** | [`PATTERN_LIBRARY.yaml`](file:///home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml) | Verified architectural patterns and legacy shims |

---

## Tailored 5-Phase Analytical Workflow

### Phase 1: Surface & Interface Mapping
Audit Rust workspace traits (`rust/crates/*/src/lib.rs`), SlimProto packet layouts, TAS5805M I2C register maps, CYD UART wire protocol, and C component headers (`main/`, `components/`).

### Phase 2: Control-Flow & Call-Tree Tracing
Trace `PlayerCoordinator`, `StreamPipeline`, `SlimprotoClient`, `HttpAudioStream`, `AmpManager`, and `NowPlayingUi` state machines and compare against upstream C control flow.

### Phase 3: Branching, Preconditions & Failure Modes
Audit ringbuffer bounds, decoder frame resync, non-blocking socket state transitions, I2C fault recovery, and UART line framing.

### Phase 4: Low-Level & Empirical Tool Verification
Execute `cargo check`, `cargo clippy`, `cargo test`, `cargo tree`, and `git diff`/`git log` against upstream branches.

### Phase 5: Synthesis & Artifact Delivery
* Output final synthesis to: [`code_analysis_report.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/code_analysis_report.md)

---

<!-- USER_CUSTOM_START -->
## User & Repository Customizations
- Primary branch for pure-Rust firmware: `MK5-Rust` (`rust/` workspace with 8 crates).
- Hardware targets: Louder-ESP32 (`ESP32-WROVER-N8R8`), Louder-ESP32-Plus (`ESP32-S3-WROOM-N8R8`), Cheap Yellow Display (`ESP32-2432S028R`).
<!-- USER_CUSTOM_END -->
