# Work Order `WO-127`: `/deep-hardware-audit` Phase 1 Micro-Auditor 6 — TCP Transport Queue, LMS Transcode Pipeline & Sibling Bridge Appliance

- **ID**: `WO-127`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: TCP Transport Queue, LMS Transcode Pipeline & Sibling Bridge Appliance Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: TCP Transport Queue, LMS Transcode Pipeline & Sibling Bridge Appliance Micro-Auditor
2. **Target & Scope**: Companion LMS Bridge Host `10.0.0.10` (`lyrion`, Debian 13 aarch64, Linux `6.12.33-v8+`, LMS `9.1.1`) and its live TCP interconnects (`:3483` SlimProto & `:9000` HTTP Audio Stream) to `[BASEMENT]` (`172.22.1.15`, `c8:2e:18:be:b8:34`).
3. **Layer Mandate**: Audit bottom-up: L1/L2 (Ethernet interface `enxb827eb4b7845` link health, packet drops/errors, TCP socket `10.0.0.10:9000 -> 172.22.1.15:56007` `send_q: 173,371` B, TCP window/MSS/retransmits via `ss -tni`) -> L3 (LMS Transcode Pipeline: why `SqueezeDSP` `PID 14341` + `lame` `PID 14340` + `lame --preset insane 320 kbps` `PID 14349` is running for `c8:2e:18:be:b8:34` when `WO-061` set `plugin.squeezedsp:enabled = 0`! Inspect `/var/lib/squeezeboxserver/prefs/player/c8:2e:18:be:b8:34.prefs` and `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs` and `custom-convert.conf`!) -> L4 (Sibling bridge daemons: `ShairTunes2W` `avahi-publish-service` `PID 14338` + `shairport_helper`, UPnP `MediaRenderer.xml`, `CastBridge` `PID 12150`).
4. **Seeds**: `System, Kernel & Hardware Event Logs`, `Network Interfaces, Switches & VoIP/Appliance Hardware`, `Drivers & Kernel Modules`. Compliance: `NIST-800-53:AU-6`, `NIST-800-53:CM-6`, `CIS-LOG-4.1`, `HW-FAULT:LINK-CRC`.
5. **Pre-Identified Scout Anomalies**:
   - Anomaly A: On `10.0.0.10`, `WO-117` found `lame` (`PID 14340`) -> `SqueezeDSP` (`PID 14341`) -> `lame --preset insane` (`PID 14349`, re-encoding 128 kbps radio to 320 kbps MP3!) running for `c8:2e:18:be:b8:34`, and `WO-119` found `plugin.squeezedsp:enabled: null`! Re-encoding a 128 kbps radio stream to 320 kbps (`--preset insane`) triples the network byte rate (`40 KB/s` instead of `16 KB/s`), bloats `send_q` to `173,371` B, and increases ESP32 MP3 decoder bit-reservoir/Huffman load by 2.5x!
   - Anomaly B: Check whether `shairport_helper` is actively listening on TCP port `36201` alongside `avahi-publish-service` (`PID 14338`) on `10.0.0.10`.
6. **Shell & Script Safety**: Read-only SSH commands to `10.0.0.10` (`ssh -o BatchMode=yes 10.0.0.10 "..."`). **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry inspection. Do NOT modify any files, prefs, or services.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
