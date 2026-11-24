# MK5-Rust Device Service Seams (`DeviceService`)

Architecture and integration reference for plug-in device services in `rust/crates/louder-esp32-app`.

---

## 1. Overview & Design Goals

Before work order F4, background and network features (`mDNS`, `AirPlay`, `Spotify Connect` facade, and debounced settings persistence) were driven inline inside `rust/crates/louder-esp32-app/src/main.rs`. The `DeviceService` seam (`rust/crates/louder-esp32-app/src/services/mod.rs`) extracts each feature into a self-contained module with a uniform lifecycle contract:

- **Modular `DeviceService` architecture**: Each feature lives in `src/services/<feature>.rs`, implements `DeviceService`, and is registered once in `pub fn register_all(reg: &mut ServiceRegistry)`. Core runtime phases in `main.rs` dispatch through `ServiceRegistry` rather than hardcoding per-service calls.
- **PSRAM heap discipline**: ESP32 WROVER internal SRAM (`IRAM`, `.dram2_uninit`, `.bss` / stack, and internal heap) is tightly constrained. All service structs, state machines, and `smoltcp` socket RX/TX/metadata buffers **MUST** be heap-allocated in PSRAM (`Box::new`, `Vec`, `Box::leak(...into_boxed_slice())`).
- **Zero blocking calls on the RTOS / Core 0 superloop**: Everything on Core 0 runs in a single superloop (`pump_device_phase`). The I2S DMA ring is `16 KB` (`2048` stereo frames, ~`46 ms` at `44.1 kHz`), so every `init`, `on_link`, `poll`, `handle_http`, and `on_slim_frame` call **MUST** be non-blocking and bounded per loop lap.
- **Host-testable, platform-free traits**: `ServiceCtx`, `ServiceAudioHost`, `SettingsPersistHost`, `SlimExtension`, and `StatusExtras` decouple service logic from Xtensa HAL peripherals and `smoltcp` sockets so state machines, throttling, HTTP parsing, and audio arbitration run under `cargo test` on the host.

---

## 2. `DeviceService` Trait & Lifecycle Contract

### 2.1 Verbatim `DeviceService` Trait (`src/services/mod.rs`)

```rust
pub trait DeviceService {
    /// Static human-readable service name for diagnostics.
    fn name(&self) -> &'static str;
    /// Called once in bring-up. Adds this service's sockets.
    fn init(&mut self, ctx: &mut ServiceCtx<'_>);
    /// Called when the link gains or loses its address.
    fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool);
    /// Called once per loop lap. Must return without blocking.
    fn poll(&mut self, ctx: &mut ServiceCtx<'_>);
    /// Appends this service's fields to the status report.
    fn status(&self, out: &mut StatusExtras);

    /// Handles an inbound SlimProto frame `(opcode, payload)`.
    ///
    /// Return `true` when handled exclusively (suppressing default
    /// coordinator handling), `false` otherwise.
    fn on_slim_frame(
        &mut self,
        _ctx: &mut ServiceCtx<'_>,
        _opcode: [u8; 4],
        _payload: &[u8],
    ) -> bool {
        false
    }

    /// Handles an incoming HTTP request if owned by this service.
    fn handle_http(
        &mut self,
        _ctx: &mut ServiceCtx<'_>,
        _req: &ServiceHttpRequest<'_>,
        _resp_buf: &mut [u8],
    ) -> ServiceHttpOutcome {
        ServiceHttpOutcome::Unhandled
    }
}
```

`ServiceRegistry` stores `Vec<Box<dyn DeviceService>>` on the heap and drives all registered services in registration order via:

- `ServiceRegistry::new() -> Self`
- `ServiceRegistry::register<S: DeviceService + 'static>(&mut self, service: S)`
- `ServiceRegistry::register_boxed(&mut self, service: Box<dyn DeviceService>)`
- `ServiceRegistry::len(&self) -> usize` / `is_empty(&self) -> bool`
- `ServiceRegistry::init_all(&mut self, ctx: &mut ServiceCtx<'_>)`
- `ServiceRegistry::on_link_all(&mut self, ctx: &mut ServiceCtx<'_>, up: bool)`
- `ServiceRegistry::poll_all(&mut self, ctx: &mut ServiceCtx<'_>)`
- `ServiceRegistry::status_all(&self, out: &mut StatusExtras)`
- `ServiceRegistry::on_slim_frame_all(&mut self, ctx: &mut ServiceCtx<'_>, opcode: [u8; 4], payload: &[u8]) -> bool` (short-circuits on first `true`)
- `ServiceRegistry::handle_http_all(&mut self, ctx: &mut ServiceCtx<'_>, req: &ServiceHttpRequest<'_>, resp_buf: &mut [u8]) -> ServiceHttpOutcome` (short-circuits on first `ServiceHttpOutcome::Responded { .. }`)

### 2.2 Firmware Lifecycle Call Order (`src/main.rs`)

1. **Boot / Bring-Up (`bring_up`)**:
   - Initializes `NetStack` (`init_net_stack`) with `SOCKET_SLOTS = 16` slots.
   - Instantiates `ServiceRegistry::new()`, calls `louder_esp32_app::services::register_all(&mut services)`, builds a `ServiceCtx::new(boot_ms, None, config, &mut sockets).with_sta_mac(sta_mac)`, and calls `services.init_all(&mut ctx)`.
2. **Superloop (`pump_device_phase`)**:
   Each lap executes the device phases in order:
   1. `http_serve_phase(s)`:
      - When a complete HTTP request arrives on port 80, parses `ServiceHttpRequest::parse(&req_buf[..req_len])` and invokes `s.services.handle_http_all(&mut ctx, req, &mut s.http_resp_buf[..])` with `sta_mac`, `audio`, and `slim_outbound` attached.
      - If unhandled (`ServiceHttpOutcome::Unhandled`) and the request is `GET /status.json`, calls `s.services.status_all(&mut status_extras)` and forwards `&status_extras` to `http_control::handle_http_request_streaming_with_services`.
   2. `slim_session_phase(s)`:
      - Wraps `services` and `slim_outbound` in `ServiceSlimExtension::new(services, slim_outbound, lap_ms, Some(local_ip), config)` and drives `coordinator.tick_with_extension(..., &mut slim_ext, now_ms)`.
      - Inbound frames enter `SlimExtension::on_frame` -> `services.on_slim_frame_all` before default `handle_command`; outbound frames queued by services flush after the tick via `SlimExtension::poll_outbound` -> `SlimOutboundQueue::drain_to`.
   3. `s.coordinator.pump_output(s.pipeline, 2)` and `poll_smoltcp_bounded(s)` (`smol_iface.poll(...)`).
   4. `services_poll_phase(s)`:
      - Constructs `ServiceCtx::new(s.now_ms, s.assigned_ip, s.config, &mut s.sockets)` wired with `.with_sta_mac(s.sta_mac)`, `.with_audio(&mut audio)`, `.with_slim_outbound(&mut s.slim_outbound)`, and `.with_settings_persist(&mut settings_host)`.
      - On link edge (`link_up != s.services_link_up`), updates `s.services_link_up = link_up` and calls `s.services.on_link_all(&mut ctx, link_up)`.
      - Unconditionally calls `s.services.poll_all(&mut ctx)`.

---

## 3. `ServiceCtx` Capabilities & Host Bridges

`ServiceCtx<'a>` exposes monotonic time (`now_ms(&self) -> u64`), IPv4 link state (`link_ip(&self) -> Option<[u8; 4]>`), Wi-Fi MAC (`sta_mac(&self) -> [u8; 6]`), shared config (`config(&self) -> &'a ConfigHandle`), and optional capability bridges attached by the caller.

### 3.1 Networking (`SOCKET_SLOTS = 16`)

- `src/main.rs` defines `const SOCKET_SLOTS: usize = 16;`, backing `smoltcp::iface::SocketSet` with a PSRAM-heap-allocated `SocketStorage` slice.
- `NetStack` (`init_net_stack`) allocates **only the 4 base sockets**:
  1. `dhcp_handle` (`dhcpv4::Socket`)
  2. `http_handle` (`tcp::Socket`, port `80`)
  3. `slim_handle` (`tcp::Socket`, SlimProto `3483`)
  4. `audio_handle` (`tcp::Socket`, stream source)
- The remaining **12 slots** are available for plug-in services. During `DeviceService::init` (gated by `#[cfg(target_arch = "xtensa")]` and `ctx.has_sockets()`), services register sockets via:
  - `pub fn has_sockets(&self) -> bool`
  - `pub fn add_socket<T: AnySocket<'static>>(&mut self, socket: T) -> SocketHandle`
  - `pub fn socket<T: AnySocket<'static>>(&self, handle: SocketHandle) -> &T`
  - `pub fn socket_mut<T: AnySocket<'static>>(&mut self, handle: SocketHandle) -> &mut T`
  - `pub fn sockets(&self) -> &SocketSet<'static>` / `sockets_mut(&mut self) -> &mut SocketSet<'static>`
- On host builds (`#[cfg(not(target_arch = "xtensa"))]`), `ServiceCtx::new(now_ms, link_ip, config)` omits `SocketSet`, and `ServiceCtx::without_sockets(now_ms, link_ip, config)` mirrors device SlimProto callbacks where `s.sockets` is already borrowed.

### 3.2 Audio Coordination (`ServiceAudioHost` & `ServiceAudioAdapter`)

External audio sources arbitrate pipeline ownership and volume through `ServiceAudioHost` (`src/services/mod.rs`), implemented for runtime and host tests by `ServiceAudioAdapter<'a, 'p, S: AudioOutputSink, B: AudioBackend, F = fn(u8)>` (constructed via `ServiceAudioAdapter::new` or `ServiceAudioAdapter::with_cyd_volume`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpPower {
    /// Ensure the amplifier clears faults and enters `Play` mode.
    EnsurePlay,
    /// Leave amplifier power state unchanged.
    KeepCurrent,
}

pub trait ServiceAudioHost {
    fn claim_external_pcm(
        &mut self,
        source: AudioSource,
        sample_rate_hz: Option<u32>,
        amp_power: AmpPower,
    ) -> bool;

    fn release_external(&mut self, source: AudioSource);

    fn set_local_volume(&mut self, percent: u8) -> u8;

    fn request_external_source(&mut self, source: AudioSource) -> bool;

    fn ingest_external_pcm(&mut self, bytes: &[u8]) -> bool;

    fn flush_external_buffers(&mut self);

    fn has_external_audio_queued(&self) -> bool;

    fn active_source(&self) -> AudioSource;
}
```

`ServiceCtx` forwards these via:
- `has_audio(&self) -> bool`
- `claim_external_pcm(&mut self, source: AudioSource, sample_rate_hz: Option<u32>, amp_power: AmpPower) -> bool` (returns `false` when detached)
- `release_external(&mut self, source: AudioSource) -> bool` (returns `false` when detached)
- `set_local_volume(&mut self, percent: u8) -> Option<u8>` (clamps `0..=100`, triggers optional CYD callback, returns `None` when detached)
- `request_external_source(&mut self, source: AudioSource) -> bool`
- `ingest_external_pcm(&mut self, bytes: &[u8]) -> bool`
- `flush_external_buffers(&mut self)`
- `has_external_audio_queued(&self) -> bool`
- `active_source(&self) -> Option<AudioSource>`

### 3.3 Settings Persistence (`SettingsPersistHost` & `DeviceSettingsPersistHost`)

```rust
pub trait SettingsPersistHost {
    /// Polls debounced configuration persistence and flushes dirty state to
    /// backing storage when ready.
    ///
    /// Returns `true` when an image was written to storage.
    fn poll_and_persist(&mut self, config: &ConfigHandle, now_ms: u64) -> bool;
}
```

- `SettingsService` (`src/services/settings.rs`) throttles `ctx.poll_settings_persist()` to `SETTINGS_POLL_INTERVAL_MS = 100` (`100 ms`).
- On the device, `DeviceSettingsPersistHost` in `src/main.rs` runs `self.config_writer.poll(config, self.journal, now_ms)`. When a pending flash image is ready, it parks Core 1 (`self.core1.park_for_flash_write()`), erases the `4096`-byte target flash sector, writes payload/footer/header, marks the journal written (`self.journal.mark_flash_written()`), and unparks Core 1 (`self.core1.unpark_after_flash_write()`).
- **Strict isolation rule**: `#[cfg(feature = "runtime-flash-persist")]` **MUST** appear **only** inside `src/main.rs` (enforced by `tests/no_cfg_invariant.rs`). Services and `config_handle.rs` stay completely free of flash/feature gating.

### 3.4 SlimProto Bridge (`SlimExtension`, `SlimOutboundQueue`, `ServiceCtx::send_slim_frame`)

- `SlimExtension` (`src/coordinator.rs`) exposes two hooks to `PlayerCoordinator::tick_with_extension`:
  - `fn on_frame(&mut self, opcode: [u8; 4], payload: &[u8]) -> bool;`
  - `fn poll_outbound(&mut self, sink: &mut dyn SlimOutboundSink, now_ms: u32) -> Result<(), TransportError>;`
- `SlimOutboundQueue` (`src/services/mod.rs`) is a PSRAM-heap `VecDeque<([u8; 4], Vec<u8>)>` capped at `DEFAULT_SLIM_OUTBOUND_CAP: usize = 32` frames (`new`, `with_capacity`, `push`, `len`, `is_empty`, `drain_to`).
- `ServiceSlimExtension` (`src/services/mod.rs`) implements `SlimExtension` over `(&mut ServiceRegistry, &mut SlimOutboundQueue)`.
- Any `DeviceService` with `slim_outbound` attached (during `poll`, `handle_http`, or `on_slim_frame`) queues client-to-server frames via `ctx.send_slim_frame(opcode, payload) -> bool`. Note that `on_frame` runs with `ServiceCtx::without_sockets(...)` because `slim_session_phase` holds mutable borrows to the `slim` and `audio` sockets.

### 3.5 HTTP Status & Control Hooks (`StatusExtras`, `ServiceHttpOutcome`)

- **Status extensions**: `StatusExtras` collects `StatusField { pub key: String, pub value: StatusValue }` via `push_str`, `push_bool`, `push_u64`, and `push_i64` (`StatusValue::{Str, Bool, U64, I64}`). `http_control::write_status_json_with_extras` and `http_control::format_status_json_with_extras` append every field in insertion order (JSON-escaped) before the closing `}` of `/status.json`.
- **HTTP request interception**: `ServiceHttpRequest::parse(req_bytes)` parses `method`, query-stripped `path`, and `body`. Services return `ServiceHttpOutcome::Unhandled` or `ServiceHttpOutcome::Responded { response_len, close_immediately }`.

> [!CAUTION]
> **CRITICAL Caveat — `ServiceHttpOutcome::Responded { close_immediately: false }`**
> In `src/main.rs` (`http_serve_phase`), `http_serve_phase(s)` executes **before** `poll_smoltcp_bounded(s)` (`smol_iface.poll(...)`) in `pump_device_phase(s)`:
> - If a handler returns `ServiceHttpOutcome::Responded { response_len, close_immediately: false }`, `http_serve_phase` queues the bytes with `http_sock.send_slice(...)`, calls `http_sock.close()`, sets `s.pending_http_closing = true`, and sets an `80 ms` `s.pending_close_deadline_ms`. Subsequent `smol_iface.poll(...)` calls flush the queued TX bytes and finish the TCP `FIN` handshake cleanly.
> - Setting `close_immediately: true` calls `http_sock.abort()` **immediately** inside `http_serve_phase` before `smol_iface.poll(...)` can transmit the buffer, sending a TCP `RST` and dropping the queued HTTP response payload.
> - Therefore, **all HTTP handlers returning a response body MUST set `close_immediately: false`**. Reserve `close_immediately: true` strictly for intentional abort/drop cases with no payload to deliver.

---

## 4. Memory & Section Budget Rules (ESP32 WROVER)

- **Zero new static RAM buffers**: Never add new static `.bss` or `.dram2_uninit` buffers for service state or socket storage (`memory_budget_invariant.rs` forbids `FirmwareBssBuffers` / `BSS_BUFFERS` and PSRAM-absent fallback buffers).
- **Zero new IRAM placements**: Never add `#[esp_hal::ram]` or `#[unsafe(link_section = ".iram1")]` outside the strict allow-list in `tests/memory_budget_invariant.rs` (`app_core_loop`, `psram_send_cmd_spi1`, `init_and_map_psram_4mb`, `output_decode_phase`, `write_dma`, and `process_interleaved_iram`).
- **PSRAM heap allocation**: Allocate all service structs and `smoltcp` RX/TX/packet-metadata buffers from the PSRAM heap via `Box::new`, `Vec`, or `Box::leak(alloc::vec![...].into_boxed_slice())` during `DeviceService::new` or `DeviceService::init`.
- **Release ELF section thresholds** enforced by `rust/scripts/check_section_budget.py`:

| Metric / Section | Default Budget Gate | Hard Outer Limit (Invariant 48) | Measured Baseline (F3 Close) |
|---|---|---|---|
| `IRAM` (`.vectors + .rwtext + .rwtext.wifi`) | `<= 119,976 B` (`--max-iram`) | `<= 131,072 B` | `119,720 B` |
| `.dram2_uninit` | `<= 89,048 B` (`--max-dram2`) | `<= 98,768 B` | `88,792 B` |
| `.stack` | `>= 41,256 B` (`--min-stack`) | `>= 28,000 B` | `41,512 B` |
| Internal heap (`bring_up::HEAP` + `STATIC_BT_HEAP`) | `>= 65,536 B` (`--min-heap`) | `>= 65,536 B` | `65,536 B` |

---

## 5. Compile-Time & Test Invariants

| Gate / Test | File / Tool | Enforced Rules |
|---|---|---|
| `service_registry_invariant` | `crates/louder-esp32-app/tests/service_registry_invariant.rs` | Every `.rs` module in `src/services/` except `mod.rs` **MUST** be referenced inside `pub fn register_all`; `src/main.rs` **MUST** define `const SOCKET_SLOTS: usize = 16;`. |
| `no_cfg_invariant` | `crates/louder-esp32-app/tests/no_cfg_invariant.rs` | Zero `target_arch` in `src/coordinator.rs` or `src/pipeline.rs`; zero `target_arch` or `feature =` in `src/config_handle.rs`; `feature = "runtime-flash-persist"` allowed **only** in `src/main.rs`; `src/tasks.rs` has no Core 0 executor. |
| `memory_budget_invariant` | `crates/louder-esp32-app/tests/memory_budget_invariant.rs` | No PSRAM-absent fallback buffers; no static socket buffers; `JOURNAL_IMAGE`, `RAM_JOURNAL`, `CONFIG_HANDLE` in `.dram2_uninit`; AES tables out of DRAM; `STATIC_BT_SINK` removed; IRAM placements strictly on allow-list. |
| `google_rust_audit.py --strict` | `google_rust_audit.py <crate_root>/src --strict` | Enforces `GRS-001`..`GRS-012`: single-module `use` statements (`GRS-001`, no `::` inside `{ ... }`), no glob imports outside `#[cfg(test)]` (`GRS-002`), `lib.rs`/`mod.rs` restricted to `mod` + `pub use` (`GRS-003`), no production `.unwrap()` (`GRS-010`), `#[non_exhaustive]` on cross-team public structs/enums, dual `// JUSTIFICATION:` + `// SAFETY:` on any `unsafe` (`GRS-008`), no `get_` prefixes (`GRS-011`). |

---

## 6. Currently Registered Services

`pub fn register_all(reg: &mut ServiceRegistry)` registers four services in order:

| Service (`name()`) | Struct / Module | Sockets / Ports | Cadence / Timeout | Status Key(s) | Responsibility |
|---|---|---|---|---|---|
| `"mdns"` | `MdnsService` (`services/mdns.rs`) | `1` UDP socket (`5353`, multicast `224.0.0.251`) | `MDNS_ANNOUNCE_INTERVAL_MS = 30_000` (`30 s`); `MDNS_MULTICAST_COOLDOWN_MS = 1_000` (`1 s`) | `mdns_bound` (`bool`) | Builds & caches RAOP (`<= 512 B`) and Spotify Connect (`<= 384 B`) mDNS packets; answers `_raop`/`_airplay`/`_spotify-connect`/`_spotify` queries (`QR == 0`, `QDCOUNT > 0`) and sends periodic announcements. |
| `"airplay"` | `AirPlayService` (`services/airplay.rs`) | `1` TCP (`5000` RTSP: `4096 B` RX / `2048 B` TX) + `1` UDP (`6000` RTP: `16_384 B` RX / `512 B` TX) | `RTP_IDLE_TIMEOUT_MS = 3_000` (`3 s`) | `airplay_streaming` (`bool`) | Drives `RtspServer` (`ANNOUNCE`, `RECORD`, `SET_PARAMETER`, `FLUSH`, `TEARDOWN`) and `RtpAudioReceiver` (`ALAC`/AES-CBC -> 44.1 kHz PCM); claims/releases `AudioSource::AirPlay` and sets volume via `ServiceAudioHost`. |
| `"spotify_facade"` | `SpotifyFacadeService` (`services/spotify_facade.rs`) | `1` TCP socket (`4070`: `4096 B` RX / `2048 B` TX) | Per-lap `poll` | `spotify_streaming` (`bool`) | Serves ZeroConf HTTP `GET`/`POST` JSON info on port `4070`, processes `SpircStateMachine` frames (`Play`, `LoadTrack`, `Pause`, `SetVolume`) and raw PCM ingestion, and claims/releases `AudioSource::Spotify`. |
| `"settings"` | `SettingsService` (`services/settings.rs`) | None | `SETTINGS_POLL_INTERVAL_MS = 100` (`100 ms`) | `settings_writes` (`u64`) | Throttles `ctx.poll_settings_persist()` to `100 ms`, delegating debounced NOR flash writes and Core 1 park/unpark to `DeviceSettingsPersistHost` in `main.rs`. |

*(Total sockets currently used: 4 base + 4 service = **8 of 16** `SOCKET_SLOTS`, leaving **8 slots** free.)*

---

## 7. Step-by-Step Guide to Adding a New Service

### 7.1 Standard Workflow

1. **Create the module**: Add `rust/crates/louder-esp32-app/src/services/<service_name>.rs`. Define a struct whose fields and socket buffers are heap-allocated (`Box`, `Vec`, `Box::leak`) and implement `DeviceService`.
2. **Expose and register**: Add `pub mod <service_name>;` (and optional `pub use`) in `src/services/mod.rs`, and call `reg.register(super::<service_name>::<ServiceStruct>::new());` inside `pub fn register_all` (required by `service_registry_invariant`).
3. **Gate target-only `smoltcp` code cleanly**: Gate `smoltcp` imports, socket handles, and socket I/O with `#[cfg(target_arch = "xtensa")]` inside the service module; never touch `coordinator.rs`, `pipeline.rs`, or `config_handle.rs` with `target_arch` (`no_cfg_invariant`).
4. **Respect HTTP & loop budgets**: Keep `poll` bounded per lap (cap packet drains, e.g. `for _ in 0..4`). If implementing `handle_http`, write into `resp_buf` and return `ServiceHttpOutcome::Responded { response_len, close_immediately: false }`.
5. **Write host unit tests**: Add `#[cfg(test)]` unit tests in the service module exercising `init`, `on_link`, `poll`, `status`, and any `on_slim_frame` / `handle_http` / `ServiceAudioHost` behavior.
6. **Verify all gates** (from `rust/`):
   - `cargo test -p louder-esp32-app --no-fail-fast`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo +esp clippy -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings`
   - `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` + `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app`
   - `python <plugin_root>/skills/rust-pro/scripts/google_rust_audit.py crates/louder-esp32-app/src --strict`

### 7.2 Planned Service Integration Checklists (`P1`–`P4`)

- **P1 — Telnet CLI (`src/services/telnet.rs`)**:
  - Allocate `1` `tcp::Socket` (port `23` or configured CLI port) in `init` with PSRAM `Box::leak` RX/TX buffers; verify `SOCKET_SLOTS` headroom (`9/16` used).
  - Buffer partial command lines in a heap-backed `Vec<u8>` (bounded max line length) inside `poll`; handle TCP state teardown (`CloseWait`..`FinWait2` -> `abort()` + re-`listen`).
  - Route parsed commands to `ctx.config()`, `ctx.set_local_volume(...)`, or `StatusExtras` telemetry without blocking Core 0; close socket gracefully on exit command.
  - Wire in `register_all` and add host tests for line framing, command dispatch, and `on_link(false)` reset.

- **P2 — LMS Discovery & DNS (`src/services/discovery.rs` + `src/services/dns.rs`)**:
  - Allocate `2` `udp::Socket`s in `init` with PSRAM `PacketBuffer` metadata/payload slices (`1` UDP `3483` socket in `discovery.rs` for SlimProto server discovery, `1` UDP `53` socket in `dns.rs` for non-blocking RFC 1035 DNS A-record resolution, matching `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md` §7).
  - Rate-limit outbound SlimProto server discovery broadcasts (`e` / `JSON` discovery packets) using `ctx.now_ms()` so broadcasts never flood on every loop lap, and bound DNS retries/timeouts without blocking IPv4-literal streams.
  - Parse server discovery responses into target LMS IP/port and update `ctx.config()` (respecting NVS/static `lms_ip` overrides); reset discovery retry timers and transient DNS state on `on_link(_, false)` or IP change.
  - Expose discovery and DNS state (`lms_discovered`, last server IP, DNS cache/query counters) via `status(&self, out: &mut StatusExtras)` and host-test packet parsing, backoff, and RFC 1035 DNS codec.

- **P3 — AirPlay Sync / Resend (`src/services/airplay.rs`)**:
  - Reuse `AirPlayService` and allocate UDP timing/control sockets within `SOCKET_SLOTS = 16` only if needed, keeping packet metadata and jitter/resend queues on the PSRAM heap (`Box` / `VecDeque`).
  - Handle RTP sequence gap resend requests and AirPlay sync packets inside `poll_rtp` / dedicated bounded `for _ in 0..N` loops so Core 0 never stalls past the `46 ms` DMA window.
  - Avoid adding any `.dram2_uninit` or IRAM tables; verify `check_section_budget.py` stays below `--max-iram 119976` and `--max-dram2 89048`.
  - Host-test sequence gap detection, sync timestamp math, and `on_link(false)` queue reset.

- **P4 — Settings Persist (`src/services/settings.rs` & `src/main.rs`)**:
  - Keep `SettingsService` in `src/services/settings.rs` as the `100 ms` scheduler (`SETTINGS_POLL_INTERVAL_MS = 100`) calling `ctx.poll_settings_persist()`.
  - Add new persisted fields to `AudioConfig` / `ConfigHandle` cleanly without adding any `#[cfg(feature = "runtime-flash-persist")]` outside `src/main.rs` (`no_cfg_invariant`).
  - Verify `DeviceSettingsPersistHost` continues to bracket flash erase/write with `core1.park_for_flash_write()` / `unpark_after_flash_write()`, and verify host tests in `settings.rs` + `tests/config_persistence.rs` pass.
