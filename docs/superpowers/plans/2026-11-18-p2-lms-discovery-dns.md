# P2 LMS UDP Discovery & Non-Blocking DNS A-Record Resolution (`feat/lms-discovery-dns`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement non-blocking SlimProto UDP port `3483` LMS server discovery (`services/discovery.rs`) and non-blocking UDP DNS A-record resolution (`services/dns.rs`) as F4 `DeviceService` modules with PSRAM-backed socket/state allocation, wire discovered and DNS-resolved endpoints into the SlimProto/stream connection path without blocking Core 0, and verify discovery, hostname stream playback, and DNS-unreachable IPv4 fallback on `DEVBENCH`.

**Architecture:** Anchored to the F4 `DeviceService` seam (`rust/crates/louder-esp32-app/src/services/mod.rs`) and `docs/mk5-rust-services.md`.
- **Two `DeviceService` Modules (`2` UDP Sockets within `SOCKET_SLOTS = 16`):** Per `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md:467` and `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md:182`, P2 owns `rust/crates/louder-esp32-app/src/services/discovery.rs` (`DiscoveryService`, `1` UDP socket bound to ephemeral/3483 broadcast discovery) and `rust/crates/louder-esp32-app/src/services/dns.rs` (`DnsService`, `1` UDP socket for RFC 1035 DNS A-record queries on UDP port `53`). Both modules are registered in `services::register_all` (`src/services/mod.rs:913`) to satisfy `tests/service_registry_invariant.rs`.
- **PSRAM Heap Discipline:** All service state machines, bounded DNS cache entries, `DiscoveryResponseInfo` scratch, and `smoltcp::socket::udp::PacketBuffer` metadata/payload slices **MUST** be heap-allocated in PSRAM (`Box::new`, `Vec`, `Box::leak(...into_boxed_slice())` inside `#[cfg(target_arch = "xtensa")]` `init`). Zero new `.data`, `.bss`, or `.dram2_uninit` statics, and zero `.iram1` / `#[ram]` placements.
- **Zero Blocking on Core 0:** `poll` executes bounded packet drains (`for _ in 0..4`) and rate-limited UDP broadcasts/queries driven by `ctx.now_ms()`, returning immediately so the `16 KB` (`~46 ms` at `44.1 kHz`) I2S DMA window is never starved.
- **Reuse of `squeezelite-proto` Discovery Codec:** Reuses `squeezelite_proto::{DiscoveryBuilder, DiscoveryParser, DiscoveredServer}` (`rust/crates/squeezelite-proto/src/lib.rs:1238-1383`), extending `squeezelite-proto` only for any missing `NAME`/`VERS` string extraction in `E` responses if needed, keeping `squeezelite-proto` `#![no_std]` and zero-allocation.
- **Plug-in Boundary Guard (`parity-branch-distribution.md:27`):** `ServiceCtx` (`src/services/mod.rs:441`) exposes `config()` (`&ConfigHandle`), `link_ip()`, `now_ms()`, and `sockets`. Note that `main.rs` currently populates `s.lms_ip` from `config.effective_lms_server_ip()` only at boot (`main.rs:2380-2388`) and in `sync_config_to_runtime` (`main.rs:2893-2906`), and `dhcpv4::Event::Configured` (`main.rs:3150-3192`) does not yet forward `config.dns_servers` to `ConfigHandle` or `ServiceCtx`. Keep all runtime/state coordination through `ConfigHandle` / `services/` hooks; if an edit to `pump_device_phase`, `DeviceRuntimeState`, or socket setup in `main.rs` appears required and no plug-in point exists, **stop and report** the missing plug-in point rather than bypassing F4 seams.

**Tech Stack:** Rust `no_std` (`alloc`, `heapless`, `smoltcp`), `squeezelite-proto`, host toolchain `1.92.0`, Espressif `esp` toolchain `1.97.0.0`, `devbench.py`, `check_section_budget.py`, `google_rust_audit.py`.

**Branch & Worktree Isolation:**
- Branch: `feat/lms-discovery-dns` created from post-F4 `MK5-Rust` (`foundation/service-seams` merged).
- Worktree: isolated git worktree on `MK5-Rust`; rebase onto `MK5-Rust` before `DEVBENCH` check and again before opening the PR (`gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust`).
- Never push directly to `MK5-Rust` and never merge.

**Hardware Lock & Safety Rules:**
- Acquire lock before flashing `DEVBENCH`: `python rust/scripts/devbench.py acquire feat/lms-discovery-dns` (release with `release` when finished).
- Pre-flash MAC check: `espflash board-info --port COM13` must equal `ac:15:18:fd:93:9c`. Never flash `BASEMENT` (`c8:2e:18:be:b8:34`) or `CYD` (`d4:d4:da:5a:e7:3c`).
- Never touch `BASEMENT` (`172.22.1.15`), `/dev/ttyUSB0` on `pc-1`, or `POST /reboot_ota.json`.
- At most 2 flash attempts per check (`devbench.py flash`). Build with `SL_DEVICE_NAME=DEVBENCH`.

**Memory & Socket Budget Gates (P2 Allowance = `1,148 B` DRAM, `0 B` IRAM, `2` UDP Sockets):**
- Sockets: `+2` UDP sockets (`1` discovery + `1` DNS; `10 / 16` used from F4 baseline or `11 / 16` after P1, always `<= 16` `SOCKET_SLOTS`).
- `IRAM` (`.vectors + .rwtext + .rwtext.wifi`): `<= 119,976 B` (Hard Invariant 48: `<= 131,072 B`; P2 IRAM allowance `0 B`).
- `.dram2_uninit`: `<= 89,048 B` (may raise `--max-dram2` / `--min-stack` defaults in `rust/scripts/check_section_budget.py` by at most `1,148 B` total DRAM only if needed, never beyond Invariant 48).
- `.stack`: `>= 41,256 B` (Hard Invariant 48: `>= 28,000 B`).
- Internal heap (`bring_up::HEAP + STATIC_BT_HEAP`): `>= 65,536 B` (zero allowance).

---

### Task 1: Extend SlimProto UDP `e`/`E` Discovery Response Parsing (`NAME`, `VERS`, `JSON`, `CLIP`, `PORT`)

**Files:**
- Modify: `rust/crates/squeezelite-proto/src/lib.rs` (`DiscoveredServer`, `DiscoveryParser` around lines `1236-1383`)

**Context:** `DiscoveryBuilder::build_query` (`b"eJSON\0CLIP\0"`) and `DiscoveryParser::parse_response` already exist in `squeezelite-proto/src/lib.rs:1243-1383` and parse `JSON`, `CLIP`, `PORT`, and 2-byte binary ports while skipping `NAME`, `VERS`, `UUID`, and `IPAD` TLVs. P2 needs the parsed `NAME` and `VERS` strings alongside `json_port` and `cli_port` (and the UDP source IPv4 + SlimProto port `3483`) for `StatusExtras` diagnostics and server selection while keeping existing `DiscoveredServer { json_port, cli_port }` callers unbroken.

- [ ] **Step 1: Write failing host unit tests in `rust/crates/squeezelite-proto/src/lib.rs`**

Add tests in `squeezelite-proto/src/lib.rs` (`mod tests`, around line `1720`):
1. `test_discovery_metadata_extracts_name_vers_and_json_port`:
   - Parse TLV response `b"ENAME\x06MyLMS1VERS\x058.5.0JSON\x049000CLIP\x049090"` via `DiscoveryParser::parse_response_metadata(...)`.
   - Assert `name == Some("MyLMS1")`, `vers == Some("8.5.0")`, `json_port == 9000`, `cli_port == 9090`.
2. `test_discovery_metadata_extracts_null_terminated_tags_and_rejects_query_echo`:
   - Parse `b"ENAMEsqueezeserver\0VERS8.5.2\0JSON9002\0CLIP9092\0"`; assert `name == Some("squeezeserver")`, `vers == Some("8.5.2")`, `json_port == 9002`.
   - Pass `b"eJSON\0CLIP\0"` (client query echo); assert `None`.

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p squeezelite-proto test_discovery_metadata
```
Expected: FAIL (`parse_response_metadata` does not exist yet).

- [ ] **Step 3: Implement `DiscoveryMetadata` and `DiscoveryParser::parse_response_metadata` in `squeezelite-proto/src/lib.rs`**

1. Define `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct DiscoveryMetadata<'a> { pub json_port: u16, pub cli_port: u16, pub name: Option<&'a str>, pub vers: Option<&'a str> }` (zero-allocation borrows from the UDP RX buffer).
2. Implement `DiscoveryParser::parse_response_metadata(buf: &[u8]) -> Option<DiscoveryMetadata<'_>>` sharing the TLV / null-terminated parsing loop with `parse_response` (no duplicated parser loop: `parse_response` delegates to `parse_response_metadata` and projects `{ json_port, cli_port }`).

- [ ] **Step 4: Run `squeezelite-proto` tests and strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p squeezelite-proto
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/squeezelite-proto/src --strict
```
Expected: All `squeezelite-proto` tests PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/squeezelite-proto/src/lib.rs
git commit -m "feat(proto): extract NAME and VERS tags in SlimProto UDP discovery parser"
```

---

### Task 2: Build Platform-Free `DiscoveryService` State Machine & UDP `3483` Socket (`src/services/discovery.rs`)

**Files:**
- Create: `rust/crates/louder-esp32-app/src/services/discovery.rs`
- Modify: `rust/crates/louder-esp32-app/src/services/mod.rs` (add `pub mod discovery;` and `reg.register(...)` in `register_all` so `service_registry_invariant` passes)

**Context:** Per `docs/mk5-rust-services.md:267-271` and `parity-branch-distribution.md:467`:
- `DiscoveryService` activates when link is up AND no explicit static/NVS LMS server IP override is configured (`ctx.config().effective_lms_server_ip().is_none()`, or `0.0.0.0`), while respecting `first_boot_defaults::LMS_SERVER_IP` fallback policy cleanly.
- Rate-limits outbound UDP `3483` broadcast queries (`b"eJSON\0CLIP\0"` to `255.255.255.255:3483`) using exponential backoff (`1_000 ms`, `2_000 ms`, `4_000 ms`, `8_000 ms`, capped at `30_000 ms` steady-state re-probe, reset on `on_link(_, false)` or config change).
- On receiving a valid `E` response from `remote_ip:3483`, records `discovered_ip`, `slim_port = 3483`, `json_port`, `cli_port`, `server_name`, `server_vers`, and writes `discovered_ip` into the shared config target (only when static/NVS override is absent, without blocking Core 0 or triggering unguarded flash writes).
- Exposes `lms_discovered` (`bool`), `lms_discovered_ip` (`Option<String>`), `lms_json_port` (`u64`), and `lms_server_name` (`Option<String>`) in `status(&self, out: &mut StatusExtras)`.

- [ ] **Step 1: Write failing host unit tests inside `src/services/discovery.rs`**

Create `rust/crates/louder-esp32-app/src/services/discovery.rs` with `#[cfg(test)] mod tests`:
1. `discovery_service_skips_broadcast_when_static_ip_configured`:
   - Configure `ConfigHandle` with `lms_server_ip = Some([172, 22, 1, 50])`.
   - Call `on_link(&mut ctx, true)` and `poll_step(&mut ctx)` -> assert `should_send_probe(100) == false` and `discovery_active() == false`.
2. `discovery_service_rate_limits_probes_and_resets_on_link_down`:
   - Configure `ConfigHandle` with `lms_server_ip = None` and no `autoexec1 -s`.
   - `on_link` up at `t = 100`: first probe due immediately; record probe sent at `t = 100`, verify `should_send_probe(500) == false`, next probe due after backoff (`t >= 1_100`, then `2_000`, `4_000`, `8_000`, capped at `30_000`).
   - `on_link` down -> resets backoff timer, clears active link flag, clears transient discovered state.
3. `discovery_service_parses_udp_response_updates_state_and_status`:
   - Feed `b"ENAME\x05MyLMSVERS\x058.5.0JSON\x049000CLIP\x049090"` from source `[172, 22, 1, 42]:3483` via `handle_rx_datagram(&mut ctx, [172, 22, 1, 42], 3483, payload)`.
   - Assert `discovered_ip() == Some([172, 22, 1, 42])`, `json_port() == Some(9000)`, `server_name() == Some("MyLMS")`, `StatusExtras` contains `lms_discovered = true`, `lms_discovered_ip = "172.22.1.42"`.
   - Feed malformed / echo `b"eJSON\0CLIP\0"` -> ignored, state unchanged.

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::discovery::tests
```
Expected: FAIL (`unresolved module` / missing symbols).

- [ ] **Step 3: Implement `DiscoveryService` (`src/services/discovery.rs`) with `#[cfg(target_arch = "xtensa")]` UDP `3483` socket and register in `src/services/mod.rs`**

1. Keep `DiscoveryService` fields PSRAM-heap friendly (`Box<DiscoveryState>`, `Vec` / `heapless::String<32>` inside boxed state; zero statics).
2. Inside `#[cfg(target_arch = "xtensa")] DeviceService::init`:
   - Allocate `UdpDiscoveryBuffers` (`rx_meta: [PacketMetadata; 4]`, `tx_meta: [PacketMetadata; 2]`, `rx_payload: [u8; 1024]`, `tx_payload: [u8; 256]`) via `Box::leak(Box::new(...))` in PSRAM.
   - Register `udp::Socket` via `ctx.add_socket(sock)` (`self.udp_handle = Some(...)`).
3. Inside `#[cfg(target_arch = "xtensa")] poll_udp`:
   - Bind ephemeral/discovery port (`0` or `3483`) when closed; close on `on_link(false)` or IP change.
   - Drain up to `4` datagrams per lap (`for _ in 0..4`), calling `handle_rx_datagram`.
   - If `should_send_probe(now_ms)` and `DiscoveryBuilder::build_query(&mut query_buf)` succeeds, send UDP broadcast to `(IpAddress::Ipv4(Ipv4Address::BROADCAST), 3483)`.
4. Wire `pub mod discovery;` and `reg.register(super::discovery::DiscoveryService::new());` inside `src/services/mod.rs` (check `pub use` parity as well).

- [ ] **Step 4: Run host tests + `service_registry_invariant` + strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::discovery::tests
cargo test -p louder-esp32-app --test service_registry_invariant
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/discovery.rs rust/crates/louder-esp32-app/src/services/mod.rs
git commit -m "feat(discovery): add non-blocking SlimProto UDP 3483 DiscoveryService"
```

---

### Task 3: Build Non-Blocking RFC 1035 UDP DNS A-Record Resolver (`src/services/dns.rs`)

**Files:**
- Create: `rust/crates/louder-esp32-app/src/services/dns.rs`
- Modify: `rust/crates/louder-esp32-app/src/services/mod.rs` (add `pub mod dns;` and `reg.register(super::dns::DnsService::new());`)

**Context:** `rust/crates/louder-esp32-app/Cargo.toml:77` enables `smoltcp = "0.12.0"` with `"socket-udp"` and `"socket-dns"` already active. Rather than relying on `smoltcp::socket::dns::Socket`'s fixed-size/compile-time server array and static query slot constraints, `DnsService` uses a single non-blocking raw `smoltcp::socket::udp::Socket` (UDP port `53`) paired with a minimal, pure-Rust RFC 1035 DNS A-record query encoder and response parser (handling RFC 1035 label compression pointers `0xC0` cleanly with loop-depth bounds so malformed packets cannot spin Core 0). This keeps all codec, caching, and timeout/retry state (`8` PSRAM-heap cached entries with TTL expiry, IPv4 literal fast-path bypass for `"172.22.1.15"`, `1_500 ms` per-attempt retry up to `3` attempts so unreachable DNS never blocks IPv4-addressed streams) 100% host-testable under `cargo test` while consuming `1` `smoltcp` UDP socket (`ctx.add_socket`) targeting the configured/DHCP DNS server (port `53`, falling back to gateway IP or `1.1.1.1`/`8.8.8.8` when DHCP DNS is unavailable).

- [ ] **Step 1: Write failing host unit tests inside `src/services/dns.rs`**

Create `rust/crates/louder-esp32-app/src/services/dns.rs` with `#[cfg(test)] mod tests`:
1. `dns_codec_encodes_a_query_and_parses_compressed_response`:
   - Encode query for `"lms.home.arpa"` with `tx_id = 0x1234` -> verify header (`RD = 1`, `QDCOUNT = 1`), QNAME labels (`\x03lms\x04home\x04arpa\x00`), `QTYPE = 1 (A)`, `QCLASS = 1 (IN)`.
   - Feed synthetic DNS response with compressed answer pointer (`0xC0 0x0C`), `TYPE = 1`, `CLASS = 1`, `TTL = 120`, `RDLENGTH = 4`, `RDATA = [172, 22, 1, 20]` (plus an upstream CNAME RR before the A RR to prove multi-RR skipping works).
   - Assert resolved `[172, 22, 1, 20]` and `ttl_secs == 120`.
2. `dns_codec_rejects_malformed_compression_loops_and_mismatched_txid`:
   - Feed self-referential `0xC0 0x0C` label pointer -> returns `Err(DnsError::MalformedPacket)` without infinite loop.
   - Feed truncated packet or wrong `tx_id` -> ignored cleanly.
3. `dns_service_bypasses_ipv4_literals_caches_ttl_and_handles_unreachable_timeout`:
   - `resolve_or_enqueue("172.22.1.15", now_ms)` immediately returns `DnsLookup::Resolved([172, 22, 1, 15])` even when link is down or DNS server is unreachable.
   - Enqueue `"radio.example.com"` at `t = 100`; verify outbound query packet built; simulate timeout past retry budget (`3` x `1_500 ms`) with no reply -> transitions to `DnsLookup::Failed` cleanly without stalling; subsequent IPv4 literal lookup still returns `Resolved` immediately.
   - Simulate valid reply -> cached hit returns `Resolved` until `now_ms > expires_at_ms`.
   - `on_link(false)` clears in-flight queries and transient cache.

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::dns::tests
```
Expected: FAIL (`unresolved module` / missing types).

- [ ] **Step 3: Implement `DnsService` (`src/services/dns.rs`) with `#[cfg(target_arch = "xtensa")]` UDP socket and register in `src/services/mod.rs`**

1. Allocate all `DnsService` state (`Box<DnsResolverState>`) and `UdpDnsBuffers` (`rx_meta: [PacketMetadata; 4]`, `tx_meta: [PacketMetadata; 2]`, `rx_payload: [u8; 1024]`, `tx_payload: [u8; 512]`) on the PSRAM heap via `Box::new` / `Box::leak`.
2. Add `1` UDP socket in `init` via `ctx.add_socket(sock)` (`#[cfg(target_arch = "xtensa")]`).
3. Drain up to `4` inbound DNS UDP responses per `poll` lap (`for _ in 0..4`), match `tx_id`, populate cache, and transmit queued pending queries (`tx_id` incremented monotonically) when `may_send` and `now_ms >= next_retry_ms`.
4. Report `dns_queries` (`u64`), `dns_hits` (`u64`), and `dns_failures` (`u64`) in `status(&self, out: &mut StatusExtras)`.
5. Wire `pub mod dns;` and `reg.register(super::dns::DnsService::new());` in `src/services/mod.rs`.

- [ ] **Step 4: Run host tests + `service_registry_invariant` + strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::dns::tests
cargo test -p louder-esp32-app --test service_registry_invariant
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/dns.rs rust/crates/louder-esp32-app/src/services/mod.rs
git commit -m "feat(dns): add PSRAM-backed non-blocking UDP DNS A-record DnsService"
```

---

### Task 4: Wire Discovered LMS Endpoint & Hostname Resolution into Slim/Stream Connection Config (Non-Blocking)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/config_handle.rs` (runtime-only `discovered_lms_ip` / `dns_servers` / hostname resolution queue & cache bridge on `ConfigHandle` — zero flash writes, zero `#[cfg(feature = "runtime-flash-persist")]`, preserving `tests/no_cfg_invariant.rs`)
- Modify (check F4 seam boundary carefully):
  - `rust/crates/louder-esp32-app/src/services/discovery.rs` (publish discovered LMS IPv4 to `ConfigHandle` runtime field when no explicit `lms_server_ip` / `lms_ip` / `autoexec1 -s <ip>` override exists)
  - `rust/crates/louder-esp32-app/src/services/dns.rs` (service pending hostname requests queued on `ConfigHandle`, e.g., `lms_hostname` from `-s <host>` or stream `Host:` hostname)
  - `rust/crates/louder-esp32-app/src/coordinator.rs` (`start_stream` `Host:` hostname inspection when `server_ip == [0, 0, 0, 0]` or stream URL specifies a remote host)
  - `rust/crates/louder-esp32-app/src/main.rs` (ONLY if existing DHCP/slim hooks allow feeding `config.dns_servers` in `dhcpv4::Event::Configured` at `main.rs:3150` and reading `effective_lms_server_ip()` in `slim_session_phase` at `main.rs:3835`; per `parity-branch-distribution.md:27`, **stop and report** if `pump_device_phase` or `DeviceRuntimeState` layout changes would be required)

**Context:** Look closely at the existing codebase:
1. `ConfigHandle::effective_lms_server_ip()` (`src/config_handle.rs:388`) checks typed `lms_server_ip`, NVS `lms_server_ip`/`lms_ip`, and `autoexec1 -s <ip>`. If none is set (or if `-s <hostname>` is used), it returns `None` and `main.rs:2383` falls back to `first_boot_defaults::LMS_SERVER_IP`.
2. In `main.rs:3835-3850` (`slim_session_phase`), when `!open && s.config.is_autoexec_enabled() && s.slim_reconnect.may_connect(now_ms)`, it connects `slim_sock` to `(s.lms_ip, s.lms_port)`. If `DiscoveryService` finds LMS on the LAN while `s.lms_ip` was only a fallback/unset address, `s.lms_ip` and `s.coordinator.lms_ip` need to pick up the discovered/resolved IP without blocking Core 0 or overwriting persistent flash config.
3. In `coordinator.rs:2165-2175` (`start_stream`), `strm` currently uses `self.lms_ip` when `p.server_ip == [0, 0, 0, 0]` and otherwise `p.server_ip`. If `http_headers` carries `Host: <hostname[:port]>` for an external stream (e.g., internet radio URL with `server_ip == [0, 0, 0, 0]` or unresolved hostname), extracting the `Host:` hostname and queueing non-blocking DNS lookup on `ConfigHandle` lets `DnsService` resolve it asynchronously, while IPv4-addressed streams (`p.server_ip != [0, 0, 0, 0]` or numeric `Host:`) connect immediately even when DNS is unreachable.

- [ ] **Step 1: Write failing host unit tests in `src/config_handle.rs` and `src/coordinator.rs`**

1. In `src/config_handle.rs`:
   - `effective_lms_server_ip_prefers_static_override_over_discovered`:
     - When `lms_server_ip = None` and `runtime_discovered_lms_ip` is set to `[172, 22, 1, 77]`, `effective_runtime_lms_ip()` returns `Some([172, 22, 1, 77])` without marking `is_dirty()` (no flash write!).
     - When `lms_server_ip = Some([172, 22, 1, 15])`, `effective_runtime_lms_ip()` returns `Some([172, 22, 1, 15])`, ignoring discovered IP.
   - `config_handle_extracts_lms_hostname_from_autoexec_or_nvs`:
     - `autoexec1 = "-s lms.local:3483"` -> `configured_lms_hostname()` returns `Some("lms.local")`; numeric `"-s 172.22.1.15"` returns `None`.
2. In `src/coordinator.rs`:
   - Test `parse_http_host_header(http_headers)` extracts `("stream.example.org", None)` from `GET /live.mp3 HTTP/1.0\r\nHost: stream.example.org\r\n\r\n` and skips numeric IPv4 literals (`172.22.1.15:9000`).

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app effective_lms_server_ip_prefers_static_override_over_discovered
cargo test -p louder-esp32-app parse_http_host_header
```
Expected: FAIL (`method/function not found`).

- [ ] **Step 3: Implement non-blocking `ConfigHandle` runtime discovery/DNS coordination + `coordinator.rs` `Host:` helper**

1. Add transient (non-persisted) runtime fields inside `ConfigHandle` (mutex-protected, heap/inline `heapless`, zero `target_arch` or `feature =` gates per `tests/no_cfg_invariant.rs`):
   - `discovered_lms_ip: Option<[u8; 4]>`
   - `dns_servers: heapless::Vec<[u8; 4], 3>`
   - `pending_dns_host: Option<heapless::String<64>>` / `resolved_dns_host: Option<(heapless::String<64>, [u8; 4])>`
2. Wire `DiscoveryService` -> `ctx.config().set_discovered_lms_ip(ip)` when `!ctx.config().has_explicit_lms_server()`.
3. Wire `DnsService` -> read `ctx.config().dns_servers()`, poll `pending_dns_host` / `configured_lms_hostname()`, publish resolved IPv4 via `ctx.config().set_resolved_dns_host(host, ip)`.
4. Verify if any `main.rs` hook (`dhcpv4::Event::Configured` forwarding `config.dns_servers` and `slim_session_phase` reading `effective_runtime_lms_ip()` before `connect`) touches `pump_device_phase` or `DeviceRuntimeState`. Keep `pump_device_phase` and `DeviceRuntimeState` untouched; if any missing plug-in point blocks wiring without touching `DeviceRuntimeState` / `pump_device_phase`, stop and report per `parity-branch-distribution.md:27`.

- [ ] **Step 4: Run all 8 CI host & firmware gates + section budget + `google_rust_audit.py --strict`**

Run (from `rust/` and repo root):
```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app
```
And from repo root:
```powershell
python -m unittest discover -s rust/scripts -p "test_*.py"
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/squeezelite-proto/src --strict
```
*(If `check_section_budget.py` fails solely on `.dram2_uninit` or `.stack` by `<= 1,148 B` — P2's DRAM allowance from `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md:182` — adjust `--max-dram2` / `--min-stack` defaults in `rust/scripts/check_section_budget.py` by at most `1,148 B` in the same PR and document the delta in the work-order file. Never raise `--max-iram` or lower `--min-heap`.)*

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/config_handle.rs rust/crates/louder-esp32-app/src/services/discovery.rs rust/crates/louder-esp32-app/src/services/dns.rs rust/crates/louder-esp32-app/src/coordinator.rs
git commit -m "feat(lms-discovery-dns): wire non-blocking discovery and DNS endpoints via ConfigHandle"
```

---

### Task 5: Live `DEVBENCH` Discovery, Hostname Stream & DNS-Unreachable Soak Verification

**Files:**
- Create: `.agents/knowledge/work_orders/WO-P2-lms-discovery-dns.md`
- Modify: `.agents/sessions.md`, `docs/mk5-rust-services.md`, `docs/mk5-rust-todo.md`

- [ ] **Step 1: Rebase onto `MK5-Rust` and re-run all 8 CI + ELF gates**

```powershell
git fetch origin
git rebase origin/MK5-Rust
```
Re-run all 8 gates from Task 4 Step 4.

- [ ] **Step 2: Acquire `DEVBENCH` lock and verify MAC (`ac:15:18:fd:93:9c`)**

```powershell
python rust/scripts/devbench.py acquire feat/lms-discovery-dns
espflash board-info --port COM13
```
Confirm MAC is `ac:15:18:fd:93:9c`. Abort immediately if `c8:2e:18:be:b8:34` (`BASEMENT`) or `d4:d4:da:5a:e7:3c` (`CYD`). Never touch `BASEMENT` (`172.22.1.15`).

- [ ] **Step 3: Build `DEVBENCH` release image and flash via `devbench.py` (max 2 attempts)**

```powershell
$env:SL_DEVICE_NAME = 'DEVBENCH'
Push-Location rust
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
Pop-Location
python rust/scripts/devbench.py flash feat/lms-discovery-dns rust/target/xtensa-esp32-none-elf/release/louder-esp32-app
```
Verify serial boot reaches `[ANTI-BRICK GATE 1]`, DHCP lease, and `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"`.

- [ ] **Step 4: Execute `DEVBENCH` parity checks & soak (`BASEMENT` strictly untouched)**

1. **Auto-discovery with no server configured:** Clear/unset static `lms_server_ip` on `DEVBENCH` (keep `BASEMENT` completely untouched), reboot/reconnect `DEVBENCH`, verify UDP `3483` discovery finds the LAN LMS (`lms_discovered: true` in `/status.json`) and connects SlimProto (`HELO`).
2. **Hostname stream playback:** Trigger a stream URL with a hostname via LMS on `DEVBENCH` (e.g., radio stream URL with DNS hostname) and confirm DNS A-record resolution (`dns_hits` increments in `/status.json`) and smooth audio playback with zero underruns.
3. **DNS unreachable resilience:** Point DNS server to a blackhole IP / block port `53` in test config and start an IP-addressed LMS stream on `DEVBENCH`; verify IPv4 playback is completely unaffected (`0` stalls, `0` Core 0 watchdog resets) over a `10`-minute soak.
4. Release lock: `python rust/scripts/devbench.py release feat/lms-discovery-dns`.

- [ ] **Step 5: Record evidence, update docs/sessions, commit, and open PR**

Save serial log, `/status.json` snapshots, and `check_section_budget.py` output in `.agents/knowledge/work_orders/WO-P2-lms-discovery-dns.md`. Prepend session entry in `.agents/sessions.md` and update parity status in `docs/mk5-rust-todo.md` to `"Wired"` (never `"Certified"`). Commit and open PR against `MK5-Rust`:
```powershell
git add .agents/knowledge/work_orders/WO-P2-lms-discovery-dns.md .agents/sessions.md docs/mk5-rust-services.md docs/mk5-rust-todo.md
git commit -m "docs(p2): record DEVBENCH LMS discovery and DNS soak verification"
git push -u origin feat/lms-discovery-dns
gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust --head feat/lms-discovery-dns
```

---

## Verification Checklist (Pre-PR Promotion)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes (including `squeezelite-proto` discovery tests, `services::discovery::tests`, `services::dns::tests`, `config_handle::tests`, `tests/service_registry_invariant.rs`, `tests/no_cfg_invariant.rs`, `tests/memory_budget_invariant.rs`)
- [ ] `python -m unittest discover -s rust/scripts -p "test_*.py"` passes
- [ ] `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` passes
- [ ] `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` passes
- [ ] `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` + `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app` passes (`IRAM <= 119,976 B`, `.dram2_uninit <= 89,048 B` or `<= 90,196 B` within P2's `1,148 B` DRAM allowance, `.stack >= 41,256 B`, `internal heap >= 65,536 B`)
- [ ] `python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict` and `squeezelite-proto/src --strict` pass (0 violations)
- [ ] Live `DEVBENCH` (`COM13`, MAC `ac:15:18:fd:93:9c`) check passed (`BASEMENT` untouched): UDP `3483` LMS auto-discovery connects, hostname stream plays, IPv4 stream unaffected when DNS is unreachable; lock released.
