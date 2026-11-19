# P1 Telnet CLI & Unified Command Table (`feat/telnet-cli`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the on-device Telnet CLI server on TCP port `23` (`console::TELNET_PORT = 23`) as a `DeviceService` (`src/services/telnet.rs`), merge the duplicate CLI command tables from `http_control.rs` (`execute_web_cli_command`) and `console.rs` (`CliRegistry`), execute `AutoexecRunner` startup commands at boot when enabled, and verify all commands, single-client enforcement, and clean socket teardown on `DEVBENCH`.

**Architecture:** Anchored to the F4 `DeviceService` seam (`rust/crates/louder-esp32-app/src/services/mod.rs`) and `docs/mk5-rust-services.md`.
- **Port Clarification (Verified Codebase Ground Truth):** Device-side Telnet listens on **TCP port `23`** (`console::TELNET_PORT = 23`, `TelnetSession` in `src/console.rs:17, 934-1080`, `docs/mk5-rust-services.md:261-265`, and `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md:466`). *(Note: TCP `9090` is LMS's external server-side JSON/CLI port, not an on-device listener; P1 allocates `1` TCP socket on port `23` only, taking `SOCKET_SLOTS` from `8/16` to `9/16`.)*
- **PSRAM Heap Discipline:** `TelnetService` (`src/services/telnet.rs`) stores `Box<TelnetSession>`, `Box<CliRegistry>`, `Box<AutoexecRunner>`, and PSRAM-heap RX/TX socket buffers (`Box::leak(alloc::vec![0u8; ...].into_boxed_slice())` inside `#[cfg(target_arch = "xtensa")]` `init`). Zero new `.bss`, `.data`, `.dram2_uninit` statics, and zero `.iram1` placements.
- **No Blocking Core 0:** `poll` reads/dispatches at most bounded bytes per lap, handles RFC 854 `IAC` negotiation via `TelnetSession::ingest_bytes`, applies `CliSideEffect` through `ServiceCtx` (`config()`, `set_local_volume()`, `claim_external_pcm()`, `release_external()`, `send_slim_frame()` if SlimProto notifications apply), and tears down half-closed TCP states (`CloseWait`..`FinWait2`) cleanly so dropped clients free the single port `23` socket immediately.

**Tech Stack:** Rust `no_std` (`alloc`, `heapless`, `smoltcp`), host toolchain `1.92.0`, Espressif `esp` toolchain `1.97.0.0`, `devbench.py`, `check_section_budget.py`, `google_rust_audit.py`.

**Branch & Worktree Isolation:**
- Branch: `feat/telnet-cli` created from post-F4 `MK5-Rust` (`foundation/service-seams` merged).
- Worktree: isolated git worktree on `MK5-Rust`; rebase onto `MK5-Rust` before `DEVBENCH` check and again before opening the PR (`gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust`).
- Never push directly to `MK5-Rust` and never merge.

**Hardware Lock & Safety Rules:**
- Acquire lock before flashing `DEVBENCH`: `python rust/scripts/devbench.py acquire feat/telnet-cli` (release with `release` when finished).
- Pre-flash MAC check: `espflash board-info --port COM13` must equal `ac:15:18:fd:93:9c`. Never flash `BASEMENT` (`c8:2e:18:be:b8:34`) or `CYD` (`d4:d4:da:5a:e7:3c`).
- Never touch `BASEMENT` (`172.22.1.15`), `/dev/ttyUSB0` on `pc-1`, or `POST /reboot_ota.json`.
- At most 2 flash attempts per check (`devbench.py flash`). Build with `SL_DEVICE_NAME=DEVBENCH`.

**Memory & Socket Budget Gates (P1 Allowance = `1,148 B` DRAM, `0 B` IRAM, `1` TCP Socket):**
- Sockets: `+1` TCP socket (port `23`, `9 / 16` `SOCKET_SLOTS` used, `7` free).
- `IRAM` (`.vectors + .rwtext + .rwtext.wifi`): `<= 119,976 B` (Hard Invariant 48: `<= 131,072 B`; P1 IRAM allowance `0 B`).
- `.dram2_uninit`: `<= 89,048 B` (may raise `--max-dram2` / `--min-stack` in `check_section_budget.py` by at most `1,148 B` total DRAM only if needed, never beyond Invariant 48).
- `.stack`: `>= 41,256 B` (Hard Invariant 48: `>= 28,000 B`).
- Internal heap (`bring_up::HEAP + STATIC_BT_HEAP`): `>= 65,536 B` (zero allowance).

---

### Task 1: Unify CLI Command Tables in `src/console.rs` & Reuse from `http_control.rs`

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/console.rs` (`BUILTIN_CLI_COMMANDS`, `CliRegistry`, `CliSideEffect`)
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs` (`execute_web_cli_command`, lines `1320-1518`)

**Context:** Two separate command tables exist today:
1. `src/console.rs` (`CliRegistry` with 8 commands: `system`, `i2c`, `config`, `join`, `update`, `player`, `sleep`, `help`).
2. `src/http_control.rs:1320-1518` (`execute_web_cli_command` handling `free`/`heap`, `version`, `i2cdetect`, `i2cget`/`i2cset`/`i2cdump`/`i2c`, `system`/`restart`/`recovery`/`restart_ota`, `config`, `join`, `update`, `player`, `sleep`, `help`, and `cfg-*` flags).
P1 merges both command tables into `src/console.rs` so Telnet and Web HTTP `/commands.json` / `ExecuteCliCommand` share one canonical parser and command set without duplicating logic.

- [ ] **Step 1: Write failing host unit tests in `src/console.rs` and `src/http_control.rs`**

Add unit tests in `src/console.rs`:
1. `cli_registry_supports_merged_web_aliases`:
   - `free` and `heap` map to `system heap` behavior (`ok == true`).
   - `version` maps to `CliSideEffect::None` (`ok == true`).
   - `i2cdetect` maps to `CliSideEffect::I2cScan`.
   - `i2cget` / `i2cset` / `i2cdump` alias cleanly to `i2c` equivalents or return usage when args are omitted.
   - `restart` / `restart_ota` map to `CliSideEffect::Reboot`; `recovery` maps to `CliSideEffect::RebootToRecovery`.
   - `cfg-syst-name DevBench` / `cfg-*` key-value commands parse cleanly into `CliSideEffect::ConfigSet` (or `CliSideEffect::ConfigBatch`) without failing as unknown commands.
   - `exit` / `quit` produce a `CliSideEffect::Disconnect` (for graceful Telnet close).
2. `help` output lists all unified commands concisely within `String<256>` (or paginated/summary form).

- [ ] **Step 2: Run tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app console::tests::cli_registry_supports_merged_web_aliases
```
Expected: FAIL (`unknown command` for `free`, `version`, `i2cdetect`, `restart`, `cfg-syst-name`, `exit`).

- [ ] **Step 3: Implement unified command parsing in `src/console.rs` and delegate from `http_control.rs`**

1. In `src/console.rs`:
   - Add `CliSideEffect::Disconnect` (`#[non_exhaustive]` already present on `CliSideEffect`).
   - Normalize command aliases inside `CliRegistry::execute`:
     - `"free" | "heap"` -> `system heap`
     - `"version"` -> formatted firmware version/project response (`ok: true`, `CliSideEffect::None`)
     - `"restart" | "restart_ota"` -> `CliSideEffect::Reboot`
     - `"recovery"` -> `CliSideEffect::RebootToRecovery`
     - `"i2cdetect"` -> `CliSideEffect::I2cScan`
     - `"i2cget"` / `"i2cset"` / `"i2cdump"` -> forward arguments to `exec_i2c` (or accept 0-arg read summary safely for web compatibility)
     - `v if v.starts_with("cfg-")` -> parse `--key=val`, `--key val`, `key=val`, or `cfg-syst-name <name>` into `CliSideEffect::ConfigSet { key, value }`
     - `"exit" | "quit"` -> `"OK: closing session"`, `CliSideEffect::Disconnect`
   - Update `BUILTIN_CLI_COMMANDS` / `help` output while preserving existing `CliCommandDescriptor` tests.
2. In `src/http_control.rs` (`execute_web_cli_command`, lines `1320-1518`):
   - Call `crate::console::CliRegistry::new().execute(cmd_str)` to parse and validate the command!
   - Apply side effects (`CliSideEffect::ConfigSet`, `CliSideEffect::None`, etc.) using `snap` and `config` to enrich telemetry responses (`free`/`heap`, `version`, `system`, `player`, `i2c`) and push to `syslog`, eliminating the duplicate command-matching table.

- [ ] **Step 4: Run tests and strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app console::tests
cargo test -p louder-esp32-app http_control::tests
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All `console` and `http_control` tests PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/console.rs rust/crates/louder-esp32-app/src/http_control.rs
git commit -m "refactor(cli): unify console and web CLI command tables in console.rs"
```

---

### Task 2: Build Platform-Free `TelnetService` State Machine & `AutoexecRunner` Integration (`src/services/telnet.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/config_handle.rs` (expose `&self` interior-mutability helpers for `apply_key_value` / `set_host_name` / NVS key erase so `&ConfigHandle` from `ServiceCtx::config()` can apply CLI config mutations cleanly)
- Create: `rust/crates/louder-esp32-app/src/services/telnet.rs`

**Context:** Every `DeviceService` splits platform-free logic (host-testable under `cargo test`) from `#[cfg(target_arch = "xtensa")]` `smoltcp` socket I/O. In `src/services/telnet.rs`:
- `TelnetService` holds:
  - `session: Box<TelnetSession>` (PSRAM heap)
  - `registry: Box<CliRegistry>` (PSRAM heap)
  - `autoexec: Option<Box<AutoexecRunner>>` (PSRAM heap, initialized lazily once from `ctx.config().nvs_override("autoexec1")` and `ctx.config().is_autoexec_enabled()`)
  - `tx_scratch: Vec<u8>` (PSRAM heap, e.g. `1024 B` capacity)
  - `client_connected: bool`, `prompt_sent: bool`, `close_requested: bool`, `autoexec_ran: bool`, `commands_executed: u32`, `autoexec_commands_ran: u32`
  - `#[cfg(target_arch = "xtensa")] telnet_handle: Option<SocketHandle>`
- Side-effect execution (`apply_side_effect(&mut self, ctx: &mut ServiceCtx<'_>, effect: CliSideEffect)`):
  - `CliSideEffect::PlayerControl(action)`:
    - `PlayerCliAction::Volume(pct)` -> `ctx.set_local_volume(pct)` (via `ServiceAudioHost`), plus optional `ctx.send_slim_frame(...)` only if SlimProto notification is needed (`AudioSource::SlimProto`);
    - `PlayerCliAction::Play` / `Pause` / `Stop` / `Status` -> inspect `ctx.active_source()` / `ctx.is_audio_playing()` and format concise status or queue SlimProto `BUTN` / `IR` or local `ServiceAudioHost` state cleanly without panicking when detached.
  - `CliSideEffect::ConfigSet { key, value }` / `ConfigErase { key }`:
    - `ServiceCtx::config(&self) -> &'a ConfigHandle` returns a shared `&ConfigHandle`, while `ConfigHandle::apply_key_value(&mut self, key: &str, value: &str) -> bool` (`src/config_handle.rs:533`) and `ConfigHandle::set_host_name(&mut self, name: &str) -> bool` (`src/config_handle.rs:270`) currently take `&mut self` (because `host_name` sits outside the internal `Mutex`). Update `src/config_handle.rs` first — either move `host_name` inside `HandleState`'s `Mutex` and convert `set_host_name` / `apply_autoexec1_flags` / `apply_key_value` to `&self` using `self.update(...)`, or add a `&self` helper (e.g. `apply_key_value_shared(&self, key, value)` via `clone_handle()` + `apply_key_value()` + `sync_from()`, plus an `&self` NVS-override erase helper for `ConfigErase`) — before calling them via `ctx.config()` from `TelnetService`.
  - `CliSideEffect::Disconnect`:
    - Set `self.close_requested = true`.
  - Informative side effects (`ConfigDump`, `ConfigGet`, `I2cScan`, `Reboot`, `RebootToRecovery`, `EnterSleep`, `WifiJoin`, `OtaUpdate`):
    - Log to runtime syslog; for hardware-only actions (`Reboot`, `RebootToRecovery`, `I2cScan`, `WifiJoin`, `OtaUpdate`, `EnterSleep`), either execute safely if host hooks exist or return an explicit status line without blocking Core 0 or adding disallowed `main.rs` edits (remember Phase 1 rule: do NOT edit `pump_device_phase` or `DeviceRuntimeState` directly!).

- [ ] **Step 1: Write failing host unit tests inside `src/services/telnet.rs`**

Create `rust/crates/louder-esp32-app/src/services/telnet.rs` with `#[cfg(test)] mod tests` testing:
1. `telnet_service_runs_autoexec_once_at_startup`:
   - Configure `ConfigHandle` with `autoexec = "1"` and `autoexec1 = "help; player vol 42"`.
   - Attach mock `ServiceAudioHost` recording `set_local_volume(42)`.
   - Call `svc.init(&mut ctx)` and `svc.poll(&mut ctx)` twice.
   - Assert `autoexec` ran once (`autoexec_commands_ran() == 2`), `set_local_volume(42)` was invoked, and second `poll` does not re-run `autoexec`.
2. `telnet_service_skips_autoexec_when_disabled`:
   - `autoexec = "0"` -> `autoexec_commands_ran() == 0`.
3. `telnet_service_processes_iac_and_dispatches_commands`:
   - Feed RFC 854 `IAC WILL ECHO` + `"player vol 55\r\n"` + `"config dump\r\n"` + `"exit\r\n"` through `TelnetService::process_rx_bytes(&mut self, &mut ctx, rx, &mut tx_out)`.
   - Assert mock `ServiceAudioHost` received `set_local_volume(55)`, `tx_out` contains `"OK: player volume 55%"`, `"louder> "`, and `"OK: closing session"`, and `svc.close_requested()` is `true`.
4. `telnet_service_on_link_down_resets_session`:
   - Partial command buffered (`"sys"`), call `svc.on_link(&mut ctx, false)`, assert `pending_line()` is empty and `client_connected` is `false`.
5. `telnet_service_status_reports_expected_fields`:
   - Assert `StatusExtras` contains `telnet_connected` (`Bool`) and `telnet_commands` (`U64`).

- [ ] **Step 2: Run tests (expect compilation failure before module is wired or implemented)**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::telnet::tests
```
Expected: FAIL (`unresolved module` or unimplemented methods).

- [ ] **Step 3: Add `&self` config helpers in `src/config_handle.rs` and implement platform-free state machine in `src/services/telnet.rs`**

First update `rust/crates/louder-esp32-app/src/config_handle.rs` so `&ConfigHandle` (from `ctx.config()`) supports CLI config writes/erases cleanly (convert `set_host_name` / `apply_autoexec1_flags` / `apply_key_value` to `&self` using `HandleState`'s `Mutex`, or add `&self` interior-mutability helpers using `clone_handle()` / `update()` / `sync_from()` and NVS key erase). Then implement `TelnetService` (without `#[cfg(target_arch = "xtensa")]` socket I/O yet, or with clean helper boundaries):
- `pub const TELNET_RX_BUF_BYTES: usize = 2048;`
- `pub const TELNET_TX_BUF_BYTES: usize = 4096;`
- `pub const TELNET_MAX_RX_PER_LAP: usize = 512;`
- All buffers/structs allocated via `Box::new` / `Vec::with_capacity` (zero static RAM).
- Keep `status(&self, out: &mut StatusExtras)` reporting `telnet_connected` (`bool`) and `telnet_commands` (`u64`).

- [ ] **Step 4: Wire `src/services/telnet.rs` into `src/services/mod.rs` (Task 3 prep or combined)**

Wait — `tests/service_registry_invariant.rs` checks that every `.rs` file in `src/services/` is referenced inside `register_all` in `src/services/mod.rs`! So add `pub mod telnet;` and `reg.register(super::telnet::TelnetService::new());` inside `register_all` in `src/services/mod.rs` when creating `services/telnet.rs` so `cargo test --workspace` stays green at every commit!

- [ ] **Step 5: Run host tests + `service_registry_invariant` + strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::telnet::tests
cargo test -p louder-esp32-app --test service_registry_invariant
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All PASS, 0 audit violations.

- [ ] **Step 6: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/config_handle.rs rust/crates/louder-esp32-app/src/services/telnet.rs rust/crates/louder-esp32-app/src/services/mod.rs
git commit -m "feat(telnet): add PSRAM-backed TelnetService state machine, autoexec runner, and registry wiring"
```

---

### Task 3: Add `#[cfg(target_arch = "xtensa")]` `smoltcp` TCP Port `23` Transport (`9/16` `SOCKET_SLOTS`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/services/telnet.rs`

**Context:** Follow the exact `smoltcp` PSRAM socket allocation and lifecycle pattern used in `src/services/spotify_facade.rs` and `docs/mk5-rust-services.md`:
- `SOCKET_SLOTS = 16` (`src/main.rs:1818`). Currently `8` slots are used (`4` base + `4` services: `mdns`, `airplay` x2, `spotify_facade`). `TelnetService` adds `1` TCP socket (`9/16` used, `7` free).
- **Single-client policy (`parity-branch-distribution.md:466`):** With `1` `tcp::Socket` listening on `TELNET_PORT` (`23`), while a client holds the `Established` socket, port `23` does not accept a second simultaneous connection (TCP SYN receives RST from `smoltcp` when no other socket is listening on `23`). Once the client sends `exit`/`quit` (`close()`) or drops (`CloseWait`..`FinWait2`), `TelnetService` resets the session and immediately calls `listen(TELNET_PORT)` so the next client can connect cleanly.

- [ ] **Step 1: Implement `#[cfg(target_arch = "xtensa")]` socket allocation in `TelnetService::init`**

Inside `DeviceService::init(&mut self, ctx: &mut ServiceCtx<'_>)`:
```rust
#[cfg(target_arch = "xtensa")]
if ctx.has_sockets() && self.telnet_handle.is_none() {
    let rx_buf = tcp::SocketBuffer::new(Box::leak(
        alloc::vec![0u8; TELNET_RX_BUF_BYTES].into_boxed_slice(),
    ));
    let tx_buf = tcp::SocketBuffer::new(Box::leak(
        alloc::vec![0u8; TELNET_TX_BUF_BYTES].into_boxed_slice(),
    ));
    let sock = tcp::Socket::new(rx_buf, tx_buf);
    self.telnet_handle = Some(ctx.add_socket(sock));
}
```
*(Note: Keep `crate::services::ServiceCtx` free of any `target_arch` leakage in `coordinator.rs` / `pipeline.rs` / `config_handle.rs` per `tests/no_cfg_invariant.rs`.)*

- [ ] **Step 2: Implement non-blocking `poll_tcp` and `reset_transport` in `TelnetService`**

In `poll_tcp(&mut self, ctx: &mut ServiceCtx<'_>)` (`#[cfg(target_arch = "xtensa")]`):
1. If `ctx.link_ip().is_none()`: call `reset_transport(ctx)` and return.
2. Acquire `sock = ctx.socket_mut::<tcp::Socket>(handle)`:
   - If `sock.is_open()` and `sock.state()` is `CloseWait | TimeWait | Closing | LastAck | FinWait1 | FinWait2`: call `sock.abort()`, `self.session.reset()`, `self.client_connected = false`, `self.prompt_sent = false`, `self.close_requested = false`.
   - If `!sock.is_open()`: call `let _ = sock.listen(TELNET_PORT);` and return if not yet `may_send()`.
   - On transition to `may_send()` when `!self.prompt_sent`:
     - Send greeting banner (`"Squeezelite-ESP32 Telnet CLI\r\nlouder> "`) via `sock.send_slice(...)`, set `self.client_connected = true`, `self.prompt_sent = true`.
   - Bounded RX read (`let mut rx_buf = [0u8; TELNET_MAX_RX_PER_LAP];`):
     - If `sock.can_recv()` and `sock.recv_slice(&mut rx_buf)` yields `n > 0`:
       - Drop mutable borrow on `sock`, run `self.process_rx_bytes(ctx, &rx_buf[..n])` (which dispatches `CliSideEffect` through `ctx` and populates `self.tx_scratch`).
       - Re-borrow `sock` and call `sock.send_slice(&self.tx_scratch)` if non-empty.
   - If `self.close_requested`:
     - Call `sock.close()`, reset `self.session.reset()`, `self.client_connected = false`, `self.prompt_sent = false`, `self.close_requested = false`.
3. In `on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool)`:
   - On `!up`: call `reset_transport(ctx)` (`sock.abort()`, `self.session.reset()`, clear `client_connected`/`prompt_sent`/`close_requested`).

- [ ] **Step 3: Run all 8 CI host & firmware gates + section budget + `google_rust_audit.py --strict`**

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
```
*(If `check_section_budget.py` fails solely on `.dram2_uninit` or `.stack` by `<= 1,148 B` — P1's DRAM allowance from `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md` §7 — adjust `--max-dram2` / `--min-stack` defaults in `rust/scripts/check_section_budget.py` by at most `1,148 B` in the same PR and document the delta in the work-order file. Never raise `--max-iram` or lower `--min-heap`.)*

- [ ] **Step 4: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/telnet.rs
git commit -m "feat(telnet): wire Xtensa smoltcp TCP 23 socket with PSRAM buffers"
```

---

### Task 4: Live `DEVBENCH` TCP Port `23` & `autoexec` Verification

**Files:**
- Create: `.agents/knowledge/work_orders/WO-P1-telnet-cli.md`
- Modify: `.agents/sessions.md`, `docs/mk5-rust-services.md`, `docs/mk5-rust-todo.md`

- [ ] **Step 1: Rebase onto `MK5-Rust` and re-run all 8 CI + ELF gates**

```powershell
git fetch origin
git rebase origin/MK5-Rust
```
Re-run all 8 gates from Task 3 Step 3.

- [ ] **Step 2: Acquire `DEVBENCH` lock and verify MAC (`ac:15:18:fd:93:9c`)**

```powershell
python rust/scripts/devbench.py acquire feat/telnet-cli
espflash board-info --port COM13
```
Confirm MAC is `ac:15:18:fd:93:9c`. Abort immediately if `c8:2e:18:be:b8:34` (`BASEMENT`) or `d4:d4:da:5a:e7:3c` (`CYD`).

- [ ] **Step 3: Build `DEVBENCH` release image and flash via `devbench.py` (max 2 attempts)**

```powershell
$env:SL_DEVICE_NAME = 'DEVBENCH'
Push-Location rust
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
Pop-Location
python rust/scripts/devbench.py flash feat/telnet-cli rust/target/xtensa-esp32-none-elf/release/louder-esp32-app
```
Verify serial boot reaches `[ANTI-BRICK GATE 1]`, DHCP lease, and `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"`.

- [ ] **Step 4: Run live `DEVBENCH` TCP `23` Telnet & `autoexec` check**

1. Verify `autoexec` executed at boot via serial log / `/messages.json`.
2. Connect via Telnet / TCP client to `<DEVBENCH_IP>:23`:
   - Confirm banner + `louder> ` prompt.
   - Run every unified command (`help`, `system info`, `free`, `version`, `i2cdetect`, `config dump`, `player status`, `player vol 35`, `cfg-syst-name DEVBENCH`). Verify accurate responses and zero Core 0 audio glitches.
3. While Client 1 is connected, attempt a second concurrent TCP connection to `<DEVBENCH_IP>:23`; verify Client 2 is refused cleanly without disrupting Client 1.
4. Drop Client 1 abruptly (TCP RST / socket close) and reconnect immediately; verify the socket was freed and Client 3 connects cleanly and receives `louder> `.
5. Send `exit` and verify graceful disconnect.
6. Release lock: `python rust/scripts/devbench.py release feat/telnet-cli`.

- [ ] **Step 5: Record evidence, update docs/sessions, commit, and open PR**

Save serial log, `check_section_budget.py` output, and Telnet transcript in `.agents/knowledge/work_orders/WO-P1-telnet-cli.md`. Prepend session entry in `.agents/sessions.md` and update status in `docs/mk5-rust-todo.md` to `"Wired"` (never `"Certified"`). Commit and open PR against `MK5-Rust`:
```powershell
git add .agents/knowledge/work_orders/WO-P1-telnet-cli.md .agents/sessions.md docs/mk5-rust-services.md docs/mk5-rust-todo.md
git commit -m "docs(p1): record DEVBENCH telnet CLI and autoexec verification"
git push -u origin feat/telnet-cli
gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust --head feat/telnet-cli
```

---

## Verification Checklist (Pre-PR Promotion)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes (including `console::tests`, `http_control::tests`, `services::telnet::tests`, `tests/service_registry_invariant.rs`, `tests/no_cfg_invariant.rs`, `tests/memory_budget_invariant.rs`)
- [ ] `python -m unittest discover -s rust/scripts -p "test_*.py"` passes
- [ ] `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` passes
- [ ] `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` passes
- [ ] `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` + `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app` passes (`IRAM <= 119,976 B`, `.dram2_uninit <= 89,048 B` or `<= 90,196 B` within P1's `1,148 B` DRAM allowance, `.stack >= 41,256 B`, `internal heap >= 65,536 B`)
- [ ] `python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict` passes (0 violations)
- [ ] Live `DEVBENCH` (`COM13`, MAC `ac:15:18:fd:93:9c`) check passed: TCP port `23` responds to all commands, `autoexec` runs at boot, second concurrent client refused cleanly, dropped client frees socket; lock released.
