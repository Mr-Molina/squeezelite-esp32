# Work Order `WO-053`: `/deep-code-audit` Micro-Auditor 1 — AirPlay (RAOP / RTSP / mDNS) Feature-Parity & Architecture Audit

- **ID**: `WO-053`
- **Timestamp**: `2026-10-24T16:11:30-04:00`
- **Subagent Role**: AirPlay / RAOP & mDNS Feature-Parity Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of AirPlay / RAOP support across the repository (`components/`, `main/`, and `rust/crates/`) and live Lyrion Music Server (`10.0.0.10:9000`):
1. Inspect the C `squeezelite-esp32` codebase (`components/`, `main/`) to document exactly how AirPlay (`raop`, RTSP server, mDNS `_raop._tcp.local`, UDP RTP audio/control/timing sockets, AES-128-CBC / RSA airport key decryption, ALAC/PCM decoding, volume/metadata sync with `services` / `squeezelite`) is implemented in C.
2. Inspect `rust/crates/*` to verify whether any AirPlay / RAOP / mDNS code exists in the pure-Rust workspace (`0%` vs partial).
3. Query live Lyrion Music Server (`http://10.0.0.10:9000/jsonrpc.js`) to check what AirPlay / ShairTunes2W / bridge plugins are installed or active on LMS (since LMS can also expose Squeezebox players like `"BASEMENT"` as AirPlay targets on the LAN via `ShairTunes2W`, in addition to on-device RAOP).
4. Formulate structured findings (`FA-AIRPLAY-*`) detailing:
   - What exists in C (`components/...`) vs what is missing in `rust/crates/*`.
   - Exact protocol, crypto, memory (`dram_seg` / `.bss`), and `smoltcp` UDP/TCP socket requirements to implement pure-Rust AirPlay (`squeezelite-airplay` / RAOP) on `louder-esp32-app`, plus LMS `ShairTunes2W` bridge status.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify any repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3) and do not interrupt `172.22.1.15` playback.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `a39edab1-f86f-4ea7-8901-21ba39652da9` (`AirPlay & mDNS Feature-Parity Auditor`)
- **Key Findings (`FA-AIRPLAY-001`..`FA-AIRPLAY-004`)**:
  - `FA-AIRPLAY-001`: C `components/raop/raop.c`, `rtp.c`, and `raop_sink.c` implement an AirPlay 1 (RAOP) receiver on TCP port `5000` (`_raop._tcp.local`), with 3 UDP RTP ports (`DATA`, `CONTROL`, `TIMING`), 1024-bit Airport RSA key (`PKCS#1 v1.5` `Apple-Challenge` + `RSA-OAEP-SHA1` `rsaaeskey`), `AES-128-CBC` RTP packet decryption, ALAC/PCM decoding, and a `423,360`-byte PSRAM buffer carved from `outputbuf`.
  - `FA-AIRPLAY-002`: `rust/crates/*` has `0.0%` AirPlay/RAOP/mDNS code, and `louder-esp32-app/Cargo.toml` currently does not enable `socket-udp` in `smoltcp 0.12.0`.
  - `FA-AIRPLAY-003`: Live LMS `10.0.0.10:9000` has `CastBridge v3.5.4` installed & active, and has `ShairTunes2W v1.9.17` ("Airtunes for LMS") available in `/settings/server/plugins.html` (currently unchecked/disabled), which can expose `"BASEMENT"` as an AirPlay target directly from LMS `10.0.0.10`.
  - `FA-AIRPLAY-004`: Native pure-Rust on-device AirPlay (`squeezelite-airplay`) requires `smoltcp` `socket-udp` + multicast (`224.0.0.251:5353`), RTSP server (`TCP 5000`), RSA-1024 (`PKCS#1 v1.5` + `OAEP-SHA1`), `AES-128-CBC`, ALAC/L16 decoder, and RTP jitter buffering.
