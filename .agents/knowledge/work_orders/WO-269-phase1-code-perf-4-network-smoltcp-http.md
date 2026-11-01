# Work Order: WO-269 — Phase 1 `CODE-PERF-4`: Network Stack (`network.rs`, `net_device.rs`, `wifi.rs`, `http_control.rs`) `smoltcp` Throughput, Socket Buffers & Poll Cadence Auditor

- **Work Order ID**: `WO-269`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Network Autonomy, smoltcp Polling & TCP Socket Buffer Auditor (CODE-PERF-4)` (`Model: "flash"`, `TypeName: "research"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{network.rs,net_device.rs,wifi.rs,http_control.rs,eth_w5500.rs,ota_safety.rs}`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Read-Only Constraint
Audit `network.rs`, `net_device.rs`, `wifi.rs`, and `http_control.rs` (plus their callers in `main.rs`) for `smoltcp` socket buffer sizing, TCP window stalls on WAN HTTP streams (`ANOM-PERF-02`), per-iteration polling overhead, and HTTP response latency (`NET-401`, `NET-402`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **TCP Receive Window (`HTTP_STREAM_RX_BUF_LEN` & `SLIMPROTO_RX_BUF_LEN`) vs WAN RTT**:
   - Inspect `main.rs` and `network.rs` where `smoltcp` TCP socket RX/TX buffers are allocated for the HTTP audio stream socket and SlimProto socket.
   - Calculate the maximum TCP throughput (`Window / RTT`) for a direct WAN HTTP stream (`cdn-peer021.streaming-pro.com:8024`, RTT `60–150` ms) vs a LAN stream (`10.0.0.10`, RTT `2` ms), and check if the HTTP stream TCP RX buffer is in internal DRAM or 4 MB PSRAM!
2. **`net_poll_phase` & `iface.poll` Packet Drain Loop (`main.rs` & `network.rs` & `net_device.rs`)**:
   - Does `net_poll_phase` call `iface.poll(...)` only ONCE per main loop iteration (processing at most 1 Wi-Fi RX packet per outer loop!), or does it loop until `iface.poll(...)` returns `PollResult::None` / RX queue is drained?!
   - Check how many bytes `http_socket.recv_slice(...)` reads per iteration into `stream_buffer`! If it only reads `1,024` or `1,460` bytes once per outer loop iteration while the outer loop also runs `multi_proto_poll`, `bt_poll_phase`, etc., how does that cap network ingress throughput?
3. **`http_control.rs` ZeroConf DH Key Derivation (`WO-254` `SEC-503`) & Endpoint Overhead**:
   - In `WO-259`, `GET /?action=getInfo` took `386.7 ms` TTFB (whereas `/status.json` took `78–95 ms`)! Why did `/?action=getInfo` take `386.7 ms`?! Check if `ZeroConfServer::new()` or `handle_request` is performing a 768-bit / 1024-bit Diffie-Hellman modular exponentiation (`montgomery_mod_pow`) synchronously on Core 0 on EVERY `GET /?action=getInfo` request, blocking audio/network on Core 0 for ~350 ms!

## 3. Deliverable
Return a structured list of findings (`NET-401`, `NET-402`, ...) with exact `file:line` citations, latency/throughput math, and concrete code remediations.
