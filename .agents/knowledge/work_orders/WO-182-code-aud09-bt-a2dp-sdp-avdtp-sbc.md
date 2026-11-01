# Work Order `WO-182`: `/deep-code-audit` Phase 1 (`AUD-09`) — Bluetooth Classic A2DP Sink Part 2 (SDP Records, AVDTP & SBC Decoder) Auditor

- **ID**: `WO-182`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-09` — Bluetooth Classic A2DP Sink Part 2 (SDP, AVDTP & SBC) Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-09`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` (lines `1401..=2791`)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **SDP, AVDTP 1.3 Signaling & Pure-Rust SBC Decoder (`bt_a2dp.rs:1401..=2791`)**:
   - Audit SDP `ServiceSearchRequest`, `ServiceAttributeRequest`, and `ServiceSearchAttributeRequest` handling and continuation state bounds.
   - Audit AVDTP 1.3 signaling (`Discover`, `GetCapabilities`, `GetAllCapabilities`, `SetConfiguration`, `Open`, `Start`, `Suspend`, `Close`, `Abort`, `DelayReport`) and media transport channel detection.
   - Audit `SbcDecoder` (syncword `0x9C`, CRC-8, bit allocation `Loudness`/`SNR`, Joint Stereo coupling, scale factors, polyphase synthesis filterbank, and integer overflow/saturation bounds).
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across `bt_a2dp.rs:1401..=2791` (including `GRS-012` line lengths).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `9a09000b-107a-4f53-bdac-72114d71e548`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `louder-esp32-app/src/bt_a2dp.rs:1401..=2791`. Identified 11 findings (`LOGIC-001` ACL reassembly `4 + l2cap_len > 1024` locks `acl_reassembly.active = true`; `LOGIC-002` `sdp_matches_audio_sink` ignores 32-bit `0x1A` and 128-bit `0x1C` UUIDs from iOS/Android; `LOGIC-003`/`004` SDP `MaximumAttributeByteCount` and `SDP_ErrorResponse`; `LOGIC-005` `AVDTP_DISCOVER` `InUse` flag should reflect `self.avdtp_state != AvdtpStreamState::Idle`; `LOGIC-006` AVDTP wildcard match emits `Response Accept` instead of `General Reject`; `LOGIC-008` `handle_avdtp_media` should assert `AvdtpStreamState::Streaming`; `LOGIC-010` `next_local_cid` wrap guard; `LOGIC-011` `GRS-012` line widths).
