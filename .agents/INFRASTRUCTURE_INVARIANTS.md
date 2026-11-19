# Infrastructure Invariants & Mechanical Guardrails

The following mechanical invariants are binding safety rules enforced across sessions and workflows:

## Invariant 13: Debug Artifact Cleanup
- **Rule**: Prior to completing a session handoff, all temporary debug scripts, scratch logs, and transient build artifacts must be sanitized or removed.

## Invariant 14: Three Strikes Rule for Troubleshooting
- **Rule**: Maximum of **three attempts** allowed to solve a specific technical error or system failure. If the third attempt fails, all modification attempts MUST stop immediately and a Deep Research Request must be compiled.

## Invariant 24: Credential Leak Scan
- **Rule**: No plain-text API keys, passwords, private tokens, or secrets may be committed or persisted in public artifacts. A secret scan must run prior to session closing.

## Invariant 28: Mechanical Mitigations for Recurring Failures
- **Rule**: Any failure identified as RECURRING must be addressed using mechanical mitigations (strict MUST/MUST NOT directives or automated scripts), not advisory comments.

## Invariant 29: Script Verification Requirement
- **Rule**: After creating or modifying helper scripts (such as Python cleaner or audit scripts), the agent MUST run a syntax check/CLI test execution before marking the task complete.

## Invariant 30: Tool Name Schema Integrity
- **Rule**: Imported skills MUST be validated against active runtime tool declarations (search_web, run_command, invoke_subagent, etc.) before integration into global config.

## Invariant 31: Python Code Generation Safety
- **Rule**: The agent MUST NOT embed Python code containing quotes or regex patterns inside PowerShell Set-Content @"..."@ heredocs. The agent MUST use native file tools or clean Python script writes to avoid string quote escaping corruption.

## Invariant 32: Strict Verification Exit Criteria
- **Rule**: When verifying script execution per Invariant 29, the agent MUST NOT accept CLI output with non-zero exit codes or usage/help messages as verification. The agent MUST execute scripts against a valid test input file and verify an exit code 0 result.

## Invariant 33: Sequential Operations Delegation Threshold
- **Rule**: The Orchestrator MUST NOT perform sequential script debugging, regex refactoring, or multi-file auditing on the main thread if the task requires more than 3 consecutive file/command operations. It MUST delegate these specialized tasks to subagents.

## Invariant 34: Orchestrator Delegation Mandate
- **Rule**: The Orchestrator agent coordinates tasks and must delegate heavy specialized sub-tasks (CVE scans, cleanups, deep research) to subagents.

## Invariant 35: Subagent Scope and Reporting
- **Rule**: Subagents operate within defined scope and must report findings back to the Orchestrator.

## Invariant 36: Invariant and Docstring Preservation
- **Rule**: Subagents must preserve existing codebase invariants and docstrings unless explicitly ordered to mutate them.

## Invariant 37: Subagent Command Exit Verification
- **Rule**: Subagents must verify all command exit codes before reporting task completion.

## Invariant 38: Three Strikes Deep Research Transition
- **Rule**: On the 3rd failed attempt to resolve a technical error or on user third-strike override, agents MUST halt all code mutations, compile all environment diagnostics, hypotheses, and logs into a structured Deep Research Request, and await verified research findings before resuming.

## Invariant 39: Evidence Before Assertions (Anti-Guessing)
- **Rule**: Agents MUST cite specific evidence (command output, API response, log line) before stating any root cause. Agents MUST NOT present a hypothesis as a conclusion or propose a fix in the same message as discovering an unverified issue.

## Invariant 40: Live Ground Truth Requirement (Anti-Stale Information)
- **Rule**: When evaluating active infrastructure state, switch port assignments, link health, or online status, agents MUST query live infrastructure (SSH, API, CLI, SNMP) rather than relying on static documentation or legacy CSV export dumps.

## Invariant 41: Anti-Simulated Tool Verification Gate
- **Rule**: Agents MUST NOT assert or imply browser rendering, UI interactions, or tool executions occurred unless backed by actual tool calls with live responses or captured image artifacts. If a tool fails to attach, agents MUST immediately notify the user.

## Invariant 42: Staged Deployment & Manual Upload Gate
- **Rule**: When deploying hardware configurations requiring manual or external staging (FTP, TFTP, file servers), file generation and device restart commands MUST be strictly decoupled into separate phases with an explicit user confirmation gate between them. Tooling and subagents MUST recognize and preserve the complete Rust crate topology: 10 workspace crates (`squeezelite-proto`, `squeezelite-audio-core`, `squeezelite-tas58xx`, `squeezelite-decoders`, `squeezelite-cyd-link`, `squeezelite-config`, `louder-esp32-app`, `cyd-controller-app`, `squeezelite-raop`, `squeezelite-cspot`), standalone `nanomp3-core`, and 5 vendored crates (`esp-radio`, `syom`, `lewton`, `alac`, `opus-rs`).

## Invariant 43: Ground Truth Entity & Acronym Verification
- **Rule**: Agents MUST verify institutional names, client identities, organization titles, and domain acronym expansions against authoritative repository documentation or device backups before scaffolding UI templates or configuration copy.

## Invariant 44: End-to-End Control, Stream Resync & Sibling-Daemon Regression Verification Gate
- **Rule**: (1) Interactive UI controls MUST NOT be certified via static HTML byte-count checks (`curl GET /`); verification MUST execute at least one state-changing control request against the live endpoint and confirm backend state mutation. (2) Streaming audio decoders MUST advance/consume bytes on all error paths (`CorruptedFrame`, `NoFrame`, `ReservoirUnavailable`, oversized false-sync `frame_bytes`) so a single corrupt byte never deadlocks the ringbuffer. (3) Before and after restarting a parent service daemon (e.g., `logitechmediaserver`), agents MUST verify all previously active child bridge processes (`shairport_helper`, UPnP) remain running.

## Invariant 45: Hardware & Workspace Feature-Maximization Audit Gate
- **Rule**: Before declaring an embedded firmware port feature-complete, agents MUST audit the target board's full hardware schematic (`sdkconfig.defaults`, external PSRAM, secondary CPU cores, amplifier DSP/analog-gain/modulation registers, and flash partitions) and existing workspace crates (`squeezelite-audio-core`, `squeezelite-config`, `squeezelite-decoders`) to ensure hardware peripherals (such as 4 MB Quad-SPI PSRAM and `APP_CPU`) are initialized and utilized rather than starving ringbuffers in internal `.bss`.

## Invariant 46: 8-Byte Stereo Frame DMA Alignment & Underrun Zero-Fill Gate
- **Rule**: All pushes into ESP32 32-bit stereo circular I2S DMA (`DmaTxCircularBuffer`, where 1 stereo frame = 8 bytes: 4-byte R + 4-byte L) MUST clamp byte counts and `available_bytes()` to a multiple of 8 bytes (`& !7`), and `ensure_running()` MUST zero-fill the circular DMA buffer and pre-push aligned silence before restarting a halted transfer so stale audio samples are never replayed on underrun recovery.

## Invariant 47: Split NVS Page-0 Namespace Pinning, Gate-1-First Boot Order, & Extension Certifier Source-Edit Ban
- **Rule**: (1) When reading a multi-page (`16 KB`) ESP-IDF NVS partition (`0x9000..0xD000`) through a bounded (`12 KB`) scratch buffer, Page 0 (`0x9000..0xA000`, containing namespace ID definitions `config_ns_id`/`aplist_ns_id`) MUST remain pinned in `nvs_buf[0..4096]` when reading upper pages (`0xB000..0xD000`, Pages 2–3) into `nvs_buf[4096..12288]`. (2) Gate 1 (`otadata` `0xD000..0xF000` erase + `0xFF` read-back verification) and boot-time flash reads (`nvs` and `settings`) MUST execute while single-core BEFORE starting PSRAM or Core 1 (`APP_CPU`), and Core 1's entry function MUST be placed in IRAM (`#[esp_hal::ram]`). (3) `Extension Certifier` subagents are strictly forbidden from editing repository source files (`src/*`, `Cargo.toml`) or exceeding 2 flash attempts; on the first OTA boot rollback, the certifier MUST immediately halt and return `[BLOCKED / ERROR]`.

## Invariant 48: Bare-Metal `.dram2_uninit` Field Parity, PSRAM Region-0 Priority & `>= 64 KB` Internal Heap Gate
- **Rule**: (1) Every field added to `StreamPipeline` in `rust/crates/louder-esp32-app/src/pipeline.rs` MUST have a matching `core::ptr::addr_of_mut!((*ptr).<field>).write(...)` initializer in `rust/crates/louder-esp32-app/src/main.rs::init_static_pipeline()` so `MaybeUninit<StreamPipeline>` in `.dram2_uninit` never contains uninitialized pointer/niche bits (`Option<Box<Resampler>>`). (2) On boards with external PSRAM (`4 MB` `ESP32-WROVER`), `init_and_map_psram_4mb()` MUST be registered as `esp_alloc::HEAP` Region 0 BEFORE any `MemoryCapability::Internal` DRAM region so general `Box`/`Vec` allocations (`prewarm_opus`, `Resampler`, `audio_rx_vec`, `slim_rx_vec`) land in PSRAM. (3) Total `MemoryCapability::Internal` heap (`.bss` `heap_allocator!` + `.dram2_uninit` `STATIC_BT_HEAP`) MUST remain `>= 65,536` B (`65 KB`) whenever `esp-radio` Wi-Fi and BTDM coexist, while keeping `.stack >= 28,000` B (owner decision 2026-11-16; was `36,000` B, enforced in CI by `rust/scripts/check_section_budget.py`), `.iram0.text <= 131,072` B, and `.dram2_uninit <= 98,768` B. Note (F3, 2026-11-18): these four figures are the hard outer limits. `rust/scripts/check_section_budget.py` enforces tighter defaults in CI, the values measured at the close of `WO-F3` with 256 B of slack (IRAM `<= 119,976` B, `.dram2_uninit <= 89,048` B, `.stack >= 41,256` B, internal heap `>= 65,536` B from `board::INTERNAL_HEAP_BYTES` + `board::BT_HEAP_BYTES`), and refuses a limit set beyond the hard ones. A feature branch moves a default only by its allowance in section 7 of `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md`.

## Invariant 49: Live Streaming Throughput, PSRAM Socket Window, Single Device Path & Latency Certification Gate
- **Rule**: (1) Every live certification on `"BASEMENT"` (`172.22.1.15`) MUST measure a `>= 15s` steady-state playback window and verify `frames_played/s` in `44,080..44,120` fps (`>= 99.95%` of `44.1 kHz`), `i2c_acks/s <= 3.05` ACKs/s (`FaultStatus::has_critical_amp_fault()` + 30s fault-recovery cooldown in `main.rs`, the only device path), and cached `GET /?action=getInfo` TTFB `< 100 ms` (`ZEROCONF_DH_CACHE`). (2) When `4 MB` PSRAM is active, `smoltcp` TCP receive buffers MUST be allocated in PSRAM (`audio_rx_buf >= 65,536` B, `slim_rx_buf >= 8,192` B) with immediate egress `smol_iface.poll(...)` after socket drain, `PowerSaveMode::None` re-applied after Wi-Fi association, and LMS `mp3StreamingMethod == 1` (LAN proxy). (3) `rust/crates/louder-esp32-app/src/main.rs` (`pump_device_phase`) is the only device firmware path: `tasks.rs` holds the core-1 executor and the `CTRL`/`EVT` channel types and MUST NOT regain a second copy of the device loop (enforced by `tests/no_cfg_invariant.rs::tasks_has_no_core0_executor`). Any bug fix applied to `StaticMp3Decoder` MUST still be audited against `Mp3Decoder`, and the other way round.

## Invariant 50: Production 3D Scene Hygiene & Diagnostic Quarantine
- **Rule**: Isolate or purge debug cameras, temporary shaders, and obsolete armature/mesh datablocks before finalizing 3D deliverables.

## Invariant 51: Windows Interactive Desktop `WinSta0\Default` Routing & Physical HWND Visibility
- **Rule**: Route interactive GUI launches to `WinSta0\Default` and verify `IsWindowVisible(hwnd)` with non-zero `GetWindowRect` bounds before claiming visibility.

## Invariant 52: Fail-Safe Win32 Window Management & Process PID Scoping
- **Rule**: Scope Win32 window calls (`EnumWindows`, `ShowWindow`, `SetForegroundWindow`) to `target_pid > 0`, exclude OS shell classes (`Progman`, `WorkerW`, `Shell_TrayWnd`, `IME`), and abort if matches > 5.

## Invariant 53: PowerShell Inline Python Escaping & Syntax Hardening
- **Rule**: Ban complex inline `python -c` (f-strings, nested quotes, semicolons, multi-line code) and bash `\"` escaping in `pwsh`; use `.py` files.

## Invariant 65: Single-Target Troubleshooting Isolation & Post-Completion Scope Boundary Gate (GLB-012 / PM-2026-1026-007)
- **Rule**: Isolate fixes strictly to the failing unit; never modify working files unprompted. Stop cleanly at 100% completion without pitching out-of-scope follow-ups or artificial decision menus (`PM-2026-1026-007`).

## Invariant 66: Real-Time User Narration, Output Hygiene & Anti-Silent Tool Chaining Gate
- **Rule**: Emit a clean 1–2 sentence status update on every Orchestrator tool-calling turn (including `/wakeup` Turn 1); ban silent tool chaining and thought leakage.

## Invariant 67: Embedded Appliance Boot/GPT, Wi-Fi, Klipper Motion & Vendor IPC Daemon Safety
- **Rule**: Never write raw MBR/GPT sectors (`dd`, `sgdisk`, `sfdisk`, `parted`, `fdisk`) or disable vendor IPC daemons (`app-server`, `upgrade-server`, `master-server`, `log_main`).
- Arm a Dead-Man's Rollback Watchdog and verify `/etc/init.d/*` or Wi-Fi/network edits via cold reboot; on Klipper, verify `G28` after `HOME_CUR`/`driver_SGTHRS` edits, keep `noz_ex_com`, split G-code on `;`, and ban high exhaust on `ASA`/`ABS`/`PC`/`PA`.

## Invariant 68: Bounded Subagent Lifecycle (<= 25 Steps / 180s), Unconditional 60s Heartbeat Timer, Telemetry Wakeup & Scope Slicing Gate (PAT-2026-1014-001 / GLB-017 / PM-2026-1026-001 / PM-2026-1026-008 / PM-2026-1102-007)
- **Rule**: Bound `invoke_subagent` prompts to `<= 25 steps / 180s` (`<= 12–15 steps / 90s` default), `<= 1–3 files`, `<= 4 tasks`, and `Parent Conversation ID: <id>`.
- Arm one `schedule(DurationSeconds=60, TimerCondition="any")` timer on subagent dispatch/message; on 60s wakeup, run `parse_transcript.py --heartbeat` + `manage_subagents(list)` (ban `read_file` on `transcript.jsonl`), and intervene/kill if `>25 steps` or `>180s`.

## Invariant 69: User-Intent Preservation & Anti-Feature-Stripping Gate (PAT-2026-1014-002 / PM-2026-1026-002)
- **Rule**: Never resolve a bug or test failure by silently disabling or downgrading an enabled feature (`ADAPTIVE_BED_MESH`, `transfer_call`, call forwarding) without prior user consent.

## Invariant 70: Mandatory Terminal Proposal Summary Gate (GLB-023 / PM-2026-1106-001 / IMP-2026-1106-001)
- **MUST**: Every final response that asks the user for permission or stops working MUST end with a structured `### Proposal Summary` block as the last thing printed to the screen (enumerating `#`, `Target System`, `Proposed Action`, `Scope / Affected Entities`, `Command / Mechanism`, and `Mode`, or explicitly stating `None — Task 100% complete` when zero actions remain per Invariant 65).
- **MUST NOT**: Never bury permission requests in prose subsections or print additional content after the terminal Proposal Summary and approval prompt. Full rule: `C:\Users\jmolina\.gemini\config\rules\orchestrator-delegation-and-narration.md`.

## Historical / Retired Global Invariants Index
Retired/merged global numbers (1–99) are never reused:
- **Invariants 1–12**: Consolidated into `/wakeup`, `/goodbye`, `wakeup_bootstrap.py`, and Invariant 13.
- **Invariants 15–23**: Consolidated into `single-target-troubleshooting.md`, `verify-autonomous-work.md`, and Invariants 14, 24, 28, 29.
- **Invariants 25–27**: Consolidated into tool hygiene and verification gates (Invariants 28–32).
- **Invariant 38**: Merged into Invariant 14 (Three Strikes).
- **Invariants 44–49**: Consolidated into `no-guessing.md` and Invariants 39–43.
- **Invariants 54–64**: Consolidated into `powershell-syntax-hardening.md`, `orchestrator-delegation-and-narration.md`, and Invariants 50–53, 65–68.




