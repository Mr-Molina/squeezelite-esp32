# Subagent Summary: WO-265 — Phase 1 `HW-PERF-2`: LMS Stream Routing, TCP Flow Control & Telemetry Audit

- **Timestamp**: `2026-11-01T13:53:00-05:00`
- **Subagent Role**: `LMS Server, Stream Routing & TCP Socket Auditor (HW-PERF-2)`
- **Work Order ID**: `WO-265`
- **Target Host / Component**: Lyrion Music Server (`10.0.0.10`) & `"BASEMENT"` (`172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified, Created, or Deleted

| Host | Action | Absolute Path | Rationale |
|---|---|---|---|
| Local (`pc-1`) | Created (Backup) | `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-265-phase1-hw-perf-2-lms-stream-routing.md.bak` | Rule 35 pre-modification backup of WO-265. |
| Local (`pc-1`) | Modified | `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-265-phase1-hw-perf-2-lms-stream-routing.md` | Updated Fulfillment Section with findings `HW-PERF-201`..`HW-PERF-204`. |
| Local (`pc-1`) | Created (Backup) | `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md.bak` | Rule 35 pre-modification backup of work order index. |
| Local (`pc-1`) | Modified | `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` | Updated WO-265 status to `[EXECUTED & VERIFIED]`. |
| Local (`pc-1`) | Created | `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo265_lms_stream_routing.md` | Rule 17 subagent summary. |

---

## 2. Key Empirical Findings & Root Cause Analysis

### Finding `HW-PERF-201`: Direct WAN HTTP Streaming vs LMS LAN Proxy (`mp3StreamingMethod`)
- **Status**: Root cause of `ANOM-PERF-01` / `ANOM-PERF-02` identified.
- **Evidence**:
  - `ss -tnaip` on `10.0.0.10` shows zero port 9000 HTTP sockets from `172.22.1.15`.
  - WAN RTT to Icecast server `cdn-peer021.streaming-pro.com:8024`: **104-110 ms**.
  - ESP32 TCP RX buffer: `audio_rx_buf: [u8; 4096]` (`main.rs:1094`), no RFC 7323 window scaling.
  - Max theoretical TCP throughput over 110ms WAN: `4096 / 0.110 = 297.8 kbps`. A single Wi-Fi jitter event or dropped segment stalls TCP, starving the audio ring.
  - In contrast, LMS LAN proxying offers **~5 ms RTT** (`minrtt: 4.938 ms`), providing `4096 / 0.005 = 6.55 Mbps` theoretical throughput (~22x margin) backed by Linux host window scaling and 212 KB socket buffers.
  - Root cause: Restoring native MP3 pass-through in `WO-256` allowed LMS `canDirectStream()` to return true because `mp3StreamingMethod` defaulted to `0` (Direct Streaming).

### Finding `HW-PERF-202`: SlimProto Flight Window Stalls (`SLIMPROTO_RX_BUF_LEN`)
- **Evidence**: `ss -tnaip` on `10.0.0.10:3483` shows `snd_wnd: 2048`, `mss: 1024`, `retrans: 0/29`.
- **Root Cause**: `slim_rx_buf: [u8; 2048]` in `main.rs:1092` limits the TCP flight size to exactly 2 segments. Burst control frames (e.g. metadata + `strm` + `setd`) exhaust the window, causing 29 retransmissions and 256ms RTO stalls over Wi-Fi.

### Finding `HW-PERF-203`: LMS `output fullness -1` Logging Artifact
- **Evidence**: LMS `server.log.1.gz` records `output_fullness=-1` at line 811, but line 850 in the same debug block records `output fullness: 0`.
- **Root Cause**: In `/usr/share/perl5/Slim/Networking/Slimproto.pm:811`, LMS logs `$stat->{'output_buffer_fullness'} || -1`. In Perl, numeric `0` is falsy, so `0 || -1` evaluates to `-1`. This is a benign logging cosmetic artifact, not protocol or memory corruption.

### Finding `HW-PERF-204`: Stream Buffer Size Mismatch on Wire
- **Evidence**: `server.log.1.gz` records `fullness: 191208 (291%)`, `bufferSize: 65536`.
- **Root Cause**: `squeezelite-proto/src/lib.rs:806` hardcodes `stream_buffer_size: 65536` on the wire while the ESP32 allocates a larger ring buffer (>191 KB). LMS computes `191208 / 65536 * 100 = 291%`.
