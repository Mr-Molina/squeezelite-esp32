# Work Order `WO-181`: `/deep-code-audit` Phase 1 (`AUD-08`) — Bluetooth Classic A2DP Sink Part 1 (HCI Transport & L2CAP Channel Engine) Auditor

- **ID**: `WO-181`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-08` — Bluetooth Classic A2DP Sink Part 1 (HCI & L2CAP) Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-08`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` (lines `1..=1400`)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Bluetooth HCI, SSP Pairing & L2CAP Signaling (`bt_a2dp.rs:1..=1400`)**:
   - Audit the 17-step Dual-Mode HCI initialization sequence (`Reset`, `WriteEventMask`, `WriteSimplePairingMode`, `WriteLocalName`, `WriteClassOfDevice` `0x240414`, `WriteScanEnable` `0x03`, `LeSetAdvertisingParameters`, `LeSetAdvertisingData`, `LeSetAdvertiseEnable`).
   - Audit ACL reassembly (`acl_reassembly_buf`), packet boundary bounds checks, SSP (`UserConfirmationRequestReply`, `IoCapabilityRequestReply`, `LinkKeyRequestNegativeReply`), and L2CAP channel table (`SDP 0x0001`, `AVDTP 0x0019`, `AVRCP 0x0017`, `EchoRequest` `0x08`, `ch.remote_cid` routing).
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across `bt_a2dp.rs:1..=1400` (including `GRS-012` line lengths).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `2d0cdd6b-13f5-4eb3-b5be-1e0a603d4e97`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `louder-esp32-app/src/bt_a2dp.rs:1..=1400`. Identified 9 findings (`SEC-001` `EVT_IO_CAPABILITY_REQUEST` sets `Dedicated Bonding 0x02` instead of `General Bonding 0x04` at line 1400; `LOGIC-001` `EVT_COMMAND_COMPLETE`/`EVT_COMMAND_STATUS` advance state machine without checking status == `0x00`; `LOGIC-002` `SbcDecoder::new()` sets `subbands: 8` with zeroed `offset` array so `reset(8)` is skipped on first 8-subband stream; `CONC-001` `calculate_bits` loop lacks lower bound `bitslice <= -32`; `LOGIC-003` Dual-Channel bitpool allocation; `SEC-002`/`SEC-003` `send_cmd` and `send_l2cap` bounds checks; `LOGIC-004` single active connection guard; `QA-001` `GRS-012` comment widths).
