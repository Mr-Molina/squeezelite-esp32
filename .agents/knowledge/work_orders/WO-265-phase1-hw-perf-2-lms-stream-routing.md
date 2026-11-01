# Work Order: WO-265 — Phase 1 `HW-PERF-2`: LMS (`10.0.0.10`), Direct WAN vs LAN Proxy Stream Routing & TCP Socket Auditor

- **Work Order ID**: `WO-265`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `LMS Server, Stream Routing & TCP Socket Auditor (HW-PERF-2)` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**: Lyrion Music Server (`10.0.0.10`) & `"BASEMENT"` (`172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Read-Only Constraint
Audit LMS (`10.0.0.10`) stream routing, `strm` frame parameters, and TCP socket flow control to diagnose `ANOM-PERF-02` and `ANOM-PERF-01`.
- **Constraint**: Strictly read-only inspection. Do NOT restart services or modify files on `10.0.0.10`.

## 2. Inspection Checklist
1. **Direct WAN HTTP Stream (`server_ip = 0`) vs LMS Local Proxy (`10.0.0.10:9000`)**:
   - On `10.0.0.10`, inspect `/var/log/squeezeboxserver/server.log.1.gz` (and `/var/lib/squeezeboxserver/prefs/server.prefs` / player prefs for `c8:2e:18:be:b8:34`) to see the exact `strm` frame LMS sent to `c8:2e:18:be:b8:34` before vs after `WO-256`/`WO-258`.
   - Specifically verify: when `ibizaglobalradio.mp3` (`http://cdn-peer021.streaming-pro.com:8024/ibizaglobalradio.mp3`) is streamed directly from the internet (`server_ip = 0`) by the ESP32 (`172.22.1.15`) vs proxied by LMS (`10.0.0.10:9000`), what is the ESP32's advertised TCP Receive Window (`rcv_wnd` / `HTTP_STREAM_RX_BUF_LEN` in `main.rs`) and what is the Bandwidth-Delay Product (`BDP = RTT * Bitrate`)?
2. **`STAT-STMt` `output fullness = -1` Investigation**:
   - Inspect `/var/log/squeezeboxserver/server.log.1.gz` on `10.0.0.10` where `STAT-STMt` shows `output fullness -1` (`0xFFFFFFFF`?) and trace why LMS logs `output fullness -1` (check how LMS parses the 36-byte `STAT` packet in `Slim/Player/Squeezebox2.pm` or `Slim/Networking/Slimproto.pm` on `10.0.0.10` vs what `squeezelite-proto` sends!).
3. **SlimProto TCP Socket `snd_wnd: 2048` & Retransmissions**:
   - Check why `ss -tnaip` on `10.0.0.10` reports `snd_wnd: 2048` (`SLIMPROTO_RX_BUF_LEN` on ESP32) and `retrans: 0/22`.
4. **Output Schema**:
   - Report findings using `HW-PERF-201`, `HW-PERF-202`, etc., with Severity, Safety Tier, Empirical Evidence, Root Cause, Remediation, Rollback, and Verification Read-Back.

## 3. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T13:52:00-05:00`
- **Backup Created**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-265-phase1-hw-perf-2-lms-stream-routing.md.bak`
- **Active Tasks Barrier**: Verified `manage_task(Action: "list")` returned 0 active background tasks.

### 3.1 Empirical Findings Summary

#### Finding `HW-PERF-201`: Unbuffered Direct WAN HTTP Stream Routing Induced by SqueezeDSP Transcode Removal (`mp3StreamingMethod` Default)
- **Severity**: HIGH
- **Safety Tier**: `TIER-2` (LMS Player Configuration / Audio Routing)
- **Empirical Evidence**:
  1. `ss -tnaip` on `10.0.0.10` confirms **0 active HTTP sockets** on port 9000 to `172.22.1.15`. The ESP32 connects directly to WAN host `cdn-peer021.streaming-pro.com:8024` (`149.202.20.156`).
  2. Measured WAN TCP connect latency to `cdn-peer021.streaming-pro.com:8024` is **103.8 ms to 110.2 ms RTT** (`curl -w "%{time_connect}\n"`).
  3. The ESP32 firmware configures `audio_rx_buf: [u8; 4096]` (`main.rs:1094`, `2123`), creating a fixed smoltcp advertised receive window of **`rcv_wnd = 4,096` bytes** with no RFC 7323 window scaling.
  4. Stream bitrate is **128 kbps** (16,000 B/s). Bandwidth-Delay Product over WAN: `BDP = 0.110 s * 128 kbps = 1,760 bytes` (nominal), but theoretical maximum TCP throughput over 110ms RTT is `4,096 B / 0.110 s = 37,236 B/s = 297.8 kbps`.
  5. Under realistic Wi-Fi latency spikes (RTT rising to 200 ms), the TCP throughput limit collapses to `4096 / 0.200 = 163.8 kbps`, leaving no headroom for packet loss recovery or buffer refill.
  6. In contrast, local LAN proxying via LMS (`10.0.0.10`) has an RTT of **~5 ms** (`minrtt: 4.938 ms`), yielding a TCP throughput ceiling of `4,096 B / 0.005 s = 819.2 KB/s = 6.55 Mbps` (~22x throughput), backed by Linux host TCP window scaling (`wscale: 8`) and 212 KB OS socket buffers.
- **Root Cause**:
  - In `Slim/Player/Protocols/HTTP.pm:435`, LMS checks `mp3StreamingMethod`. If unset (the default `0`), LMS permits direct WAN streaming for native formats (`command eq '-'`).
  - Prior to `WO-256`, SqueezeDSP had a custom transcode override in `custom-convert.conf` (`lame | SqueezeDSP | lame`), which forced LMS to proxy via `GET /stream.mp3` (`server_ip = 0`).
  - When `WO-256` restored native MP3 passthrough (`command eq '-'`), LMS executed `canDirectStream()`, which returned true. LMS sent `strm-s` pointing directly to WAN IP `cdn-peer021.streaming-pro.com:8024`, starving the ESP32's tiny 4KB TCP socket buffer and causing `ANOM-PERF-01`.
- **Remediation**:
  - Configure `mp3StreamingMethod: '1'` (Proxied Streaming) for `"BASEMENT"` (`c8:2e:18:be:b8:34`) in LMS via JSON-RPC: `["c8:2e:18:be:b8:34", ["playerpref", "mp3StreamingMethod", "1"]]`.
  - This forces LMS to ingest the WAN stream over gigabit Ethernet with full Linux TCP windowing, and serve it to the ESP32 over the 5ms LAN link via `GET /stream.mp3?player=c8:2e:18:be:b8:34`.
- **Rollback**: Revert `mp3StreamingMethod` to `'0'` in `server.prefs`.
- **Verification Read-Back**:
  - `ss -tnaip 'sport = :9000 and dst 172.22.1.15'` on `10.0.0.10` confirms an active LAN HTTP streaming socket.
  - `GET http://172.22.1.15/status.json` verifies audio playback sustained at 44,100 fps.

---

#### Finding `HW-PERF-202`: SlimProto Socket Retransmission Bursts & Stalls Caused by 2048-Byte Flight Window (`SLIMPROTO_RX_BUF_LEN`)
- **Severity**: MEDIUM
- **Safety Tier**: `TIER-1` (Firmware Buffer Allocation)
- **Empirical Evidence**:
  1. `/usr/bin/ss -tnaip` on `10.0.0.10` for SlimProto port 3483 reports:
     `snd_wnd: 2048`, `wscale: 0,8`, `mss: 1024`, `rtt: 55.144/16.06`, `rto: 256`, `retrans: 0/29`, `bytes_retrans: 862`.
  2. In `rust/crates/louder-esp32-app/src/main.rs:1092` and `2116`, `slim_rx_buf` is sized at `[u8; 2048]`.
  3. Smoltcp does not implement RFC 7323 window scaling (`wscale: 0`), so LMS's send window is hard-capped at 2,048 bytes.
  4. With MSS = 1024 bytes, the maximum flight size is exactly **2 TCP segments**.
  5. Any burst of LMS control frames (e.g. `strm`, `setd`, display text) immediately fills the 2 KB flight window. If a single Wi-Fi frame is delayed, LMS stalls, hits its 256 ms RTO, and triggers TCP retransmissions (29 retransmissions observed).
- **Root Cause**: `slim_rx_buf` is undersized at 2,048 bytes (2 MSS segments), creating zero burst tolerance over Wi-Fi with jitter.
- **Remediation**: Expand `slim_rx_buf` in `FirmwareBssBuffers` (`main.rs:1092`) from 2,048 bytes to 4,096 or 8,192 bytes (4 to 8 segments) to allow uninhibited control flow.
- **Rollback**: Revert `slim_rx_buf` to 2,048 bytes.
- **Verification Read-Back**: `ss -tnaip` on `10.0.0.10` confirms `snd_wnd: 4096` or `8192` with 0 retransmissions during active control bursts.

---

#### Finding `HW-PERF-203`: Benign Logging Artifact: `output fullness -1` in LMS `server.log` Caused by Perl Falsy `||` Operator
- **Severity**: LOW (Diagnostic Telemetry Bug in LMS)
- **Safety Tier**: `TIER-3` (Documentation / Observability)
- **Empirical Evidence**:
  1. LMS `server.log.1.gz` records:
     `[26-10-31 00:03:34.0633] Slim::Networking::Slimproto::_stat_handler (811) c8:2e:18:be:b8:34: STAT-STMt: fullness=191208, output_fullness=-1, elapsed=3.369`
  2. The exact same packet logged at line 850 in `Slimproto.pm` reveals:
     `output size: 65536`
     `output fullness: 0`
  3. Source inspection of `/usr/share/perl5/Slim/Networking/Slimproto.pm:811`:
     `$stat->{'output_buffer_fullness'} || -1`
  4. In Perl, the integer `0` evaluates to boolean FALSE. The expression `0 || -1` evaluates to `-1`.
  5. Multiple players on LMS exhibit this identical pattern (e.g. `f8:b3:b7:64:6a:a4: STAT-STMt: fullness=0, output_fullness=-1`).
- **Root Cause**: Perl's `||` operator treats valid numeric `0` as falsy, substituting `-1`. It should have used the defined-or operator `//` (`$stat->{'output_buffer_fullness'} // -1`). This is a benign logging cosmetic artifact, not memory corruption or protocol mismatch.
- **Remediation**: Informational. No code change required in squeezelite-esp32. Recognize in future audits that LMS `output_fullness=-1` means the client reported `output_buffer_fullness = 0`.
- **Rollback**: N/A.
- **Verification Read-Back**: LMS debug block at line 850 confirms wire value is `0`.

---

#### Finding `HW-PERF-204`: Stream Buffer Capacity Mismatch on Wire (`stream_buffer_size: 65536` vs `fullness: 191208` = 291%)
- **Severity**: LOW
- **Safety Tier**: `TIER-1` (Protocol Wire Parity)
- **Empirical Evidence**:
  1. LMS `server.log.1.gz` shows:
     `fullness: 191208 (291%)`
     `bufferSize 65536`
     `fullness 191208`
  2. `rust/crates/squeezelite-proto/src/lib.rs:806` and `845` hardcode `stream_buffer_size: 65536` in `StatBuilder::build_stat()` and `build_stat_with_metrics()`.
  3. The runtime stream buffer in `louder-esp32-app` (`pipeline.stream_buffer`) has a capacity in PSRAM greater than 64 KB (allowing fullness to exceed 191 KB).
  4. LMS calculates fullness percentage as `int(fullness / bufferSize * 100)`, resulting in nonsensical `291%`.
- **Root Cause**: `squeezelite-proto` hardcodes `stream_buffer_size = 65536` rather than populating the packet field from `pipeline.stream_buffer.capacity()`.
- **Remediation**: Update `build_stat_with_metrics` to take and encode the actual stream ring capacity.
- **Rollback**: Revert to 65,536.
- **Verification Read-Back**: LMS `server.log` reports `fullness <= 100%`.
