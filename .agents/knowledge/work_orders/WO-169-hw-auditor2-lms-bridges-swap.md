# Work Order `WO-169`: `/deep-hardware-audit` Phase 1 Auditor 2 (`HW-AUD-02`) — LMS `10.0.0.10` Sibling Bridges (`ShairTunes2W`, `UPnP/DLNA`, `CastBridge`, `SqueezeDSP`) & Memory/Swap Micro-Auditor

- **ID**: `WO-169`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: LMS `10.0.0.10` Sibling Bridges & Memory/Swap Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `LMS 10.0.0.10 Sibling Bridges (ShairTunes2W AirPlay shairport_helper, UPnP/DLNA, CastBridge, SqueezeDSP) & Memory/Swap Micro-Auditor` (`HW-AUD-02`)
2. **Target & Scope**: Remote LMS Appliance `10.0.0.10` (`ShairTunes2W` `shairport_helper` & `avahi-publish-service`, `Slim::Plugin::UPnP::MediaRenderer`, `CastBridge` `squeeze2cast`, `SqueezeDSP`, and RAM/Swap `free -m` / `/proc/swaps`).
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: `906 MiB` physical RAM & `345/453 MiB` (`76%`) swap usage (`free -m`, `ps aux --sort=-%mem | head -n 15`).
   - `L2-Bus-Interconnect`: mDNS `_raop._tcp` (`avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 36297`) vs listening TCP port `36297` (`ss -lntp | grep 36297`), and SSDP/UPnP port `35379` / `9000`.
   - `L3-Driver-Firmware-Config`: `/var/lib/squeezeboxserver/prefs/plugin/{shairtunes2w,castbridge,squeezedsp,upnp}.prefs` and `/var/lib/squeezeboxserver/prefs/SqueezeDSP/Settings/c8_2e_18_be_b8_34.settings.json`.
   - `L4-Service-Logs`: Why `shairport_helper` is not listed in `ps` (whether `ShairTunes2W` spawns `shairport_helper` on RTSP `RECORD` or holds the RTSP listener inside Perl `lyrionmusicserver` on port `36297`, which is currently blocked by `PID 9717` CPU spin!).
4. **Seeds**: Network Interfaces, Switches & Appliance Hardware; Storage Media & Memory (`NIST-800-53:CM-6`, `NIST-800-53:AU-6`, `CIS-HW-1.1`, `HW-FAULT:BUS-DROPOUT`).
5. **Pre-Identified Scout Anomalies**:
   - `avahi-publish-service` (`PID 9887`) advertises `A1C6A30629A8@BASEMENT _raop._tcp 36297`, while `UPnP/MediaRenderer.xml` timed out when Port 9000 backlog hit `4097/4096`.
   - Swap usage on `10.0.0.10` is `345 MiB / 453 MiB` (`76%`).
6. **Shell & Script Safety**: Use `/usr/bin/ssh -o BatchMode=yes -o ConnectTimeout=5 10.0.0.10 "..."` with bounded read-only commands.
7. **Constraint**: **Read-only telemetry inspection. Do NOT modify any configs, drivers, firmware, or services.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `b4008c01-5ffe-40c4-b057-c236d42cab7a`
- **Summary**: Verified `SqueezeDSP` is 100% bypassed (`bypass: '1'`) for `c8:2e:18:be:b8:34`, `CastBridge` (`PID 10672`) and `avahi-publish-service` (`PID 9887` `_raop._tcp 36297`) are running (`shairport_helper` is an on-demand worker spawned on RTSP `RECORD`), and `/dev/zram0` swap is at `84.9%` (`vm.swappiness=60`) (`STORAGE-001`, `NET-001`, `BUS-001`, `NET-002`, `CFG-001`, `LOG-001`).
