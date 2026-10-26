# Work Order `WO-075`: `/deep-hardware-audit` Phase 0a Scout 4 — Configuration Surface & Diagnostic Tool Prober

- **ID**: `WO-075`
- **Timestamp**: `2026-10-25T09:40:00-04:00`
- **Subagent Role**: Configuration Surface & Diagnostic Tool Prober (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 4 (Configuration Surface & Diagnostic Tool Prober)** for `/deep-hardware-audit`.
Inventory the hardware/bridge configuration surfaces and available diagnostic tools across `172.22.1.15`, `10.0.0.10`, and `10.0.0.90`:
1. **Embedded Hardware Configuration Surface (`172.22.1.15` / `rust/crates/`)**:
   - Check PSRAM configuration in `rust/crates/louder-esp32-app/Cargo.toml` and `src/main.rs` (is `esp-hal` `psram` feature enabled or unconfigured?).
   - Check TAS5805M register configuration in `rust/crates/squeezelite-tas58xx/src/lib.rs` and `src/main.rs` (fault pin `GPIO34` polling vs I2C `0x70..0x73` read-back, `0x4C` digital volume, `otadata_prearmed_factory` single-boot trial mode).
   - Check `I2S0` DMA buffer configuration (`dma_tx_stream_buffer!(8192, 1024)`), `ring_storage` (`16,384` B), and `smoltcp` socket buffer sizes in `src/main.rs`.
2. **LMS & Network Bridge Configuration Surface (`10.0.0.10`)**:
   - Via read-only SSH to `10.0.0.10`, check:
     - `/etc/avahi/avahi-daemon.conf` (is `enable-reflector` enabled? Which interfaces does `10.0.0.10` have?)
     - Active listening ports for `shairport_helper`, `squeezeboxserver`, and UPnP (`ss -tulnp | grep -E 'squeeze|shair|avahi|1900|5353|9000|3483'`).
     - Installed diagnostic and audio bridge tools on `10.0.0.10` (`command -v avahi-browse avahi-publish librespot ffmpeg sox flac lame tcpdump ip`).
3. **Strict Constraints**:
   - Read-only queries only (<60s).
   - **NEVER touch `/dev/ttyUSB0`** (Rule 3).
   - Return pure structured YAML configuration & tooling baseline.

## 2. Fulfillment Section
*(To be completed)*
