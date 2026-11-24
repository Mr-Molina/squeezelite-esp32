# P5 Feasibility Study: Native Spotify Connect on `louder-esp32-app` (`MK5-Rust`)

Date: 2026-11-18  
Branch: `study/spotify-connect`  
Base Commit: `3a3aab435f4aad7b83b0e517456be88cbd78b9ab` (`MK5-Rust`)  
Scope: Read-only feasibility study (no firmware edits, no hardware interaction).

---

## Executive Verdict

**Verdict: NO-GO for native on-device Spotify Connect on classic ESP32 (`louder-esp32-app`).**  
**Recommended Alternative: LAN Proxy / LMS `Spotty` plugin** (offloading TLS 1.3, OAuth/`login5`, Dealer WebSockets, and `librespot` protocol churn to the Lyrion Music Server host, streaming standard PCM/FLAC to the ESP32 over SlimProto at `0 B` additional ESP32 RAM/flash cost).

Summary of the four blocking constraints established by workspace inspection:
1. **Internal Heap LWM (`5,128 B`) & Stack Margin**: After F3 memory rebudgeting, runtime `int_heap_min_free` is **`5,128 B`** (`int_heap_free = 13,428 B`) with Wi-Fi and BTDM coexisting. Although TLS record buffers (`~32 KB`) can live in external PSRAM under `embedded-tls`, Wi-Fi driver/socket internal allocations, X.509/RSA verification scratch, and handshake call stacks risk exhausting the `5,128 B` internal heap floor, while `esp-mbedtls` is wholly non-viable.
2. **Ecosystem & Dependency Mismatch**: `embedded-tls 0.19.0` (the `no_std` TLS crate present in the local Cargo cache) is **TLS 1.3 only**, requires **Rust `edition = "2024"`**, `embedded-io = "0.7"`, and `heapless = "0.9"` (versus workspace `edition = "2021"`, `embedded-io = "0.6"`, `heapless = "0.8"`), and blocks synchronously in `TlsConnection::open()` unless driven via an async adapter — dangerous on the single-core pre-D1b audio loop.
3. **Protocol Deprecation vs. Current `squeezelite-cspot`**: `rust/crates/squeezelite-cspot` implements legacy `cspot` primitives (Oakley Group 1 DH, Shannon cipher, Mercury `0xB2..0xB5`, Spirc v2, AES-128-CTR + `0xA7`-stripped Vorbis), whereas Spotify has deprecated legacy username/password and AP `0xAB` flows in favor of HTTPS `login5`, OAuth2, Dealer WebSockets, and `spclient` Connect-State. Meanwhile, `main.rs:4426-4533` currently wires only a static port-`4070` facade.
4. **Licensing & ToS Risk**: The workspace is `GPL-3.0-or-later` (`rust/Cargo.toml:20`). Official Spotify eSDK binaries are proprietary/NDA-only; unofficial reverse-engineered clients cannot commit `client_secret` credentials in a GPL repository and face continuous protocol breakage.

---

## 1. `no_std` ESP32 TLS Options & Footprint vs. F3 Budgets

### 1.1 Target Hardware Memory Budgets

Baseline (`MK5-Rust`, `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md` §3 & `rust/crates/louder-esp32-app/src/main.rs:1198,2610-2640`):
- **Pre-F3 `MK5-Rust` baseline**: IRAM free `1,436 B`, `.dram2_uninit` free `248 B`, `.stack` `28,528 B` (`528 B` over `28,000 B` floor), internal heap `64,512 B` (`52,224 B` `heap_allocator!` + `12,288 B` `STATIC_BT_HEAP` in `.dram2_uninit`), external 4 MiB PSRAM (`0x3F80_0000..0x3FC0_0000`, `~1.2 MiB` free after `512 KiB` stream + `2,000 KiB` output ringbuffers), `ota_0` flash `679,872 B` free.
- **F3 tightened budgets & measured runtime headroom** (from P5 study brief):

| Metric | F3 Measured / Used | F3 Budget / Ceiling | Remaining Headroom | Notes |
|---|---:|---:|---:|---|
| IRAM (`.vectors` + `.rwtext` + `.rwtext.wifi`) | `119,720 B` | `119,976 B` (F3) / `131,072 B` (Inv 48) | **`256 B`** (F3) | All reclaimed IRAM reserved for D1 |
| `.dram2_uninit` | `88,792 B` | `89,048 B` (F3) / `98,768 B` (Inv 48) | **`256 B`** (F3) | Phase 1 branches capped at ~10% allowance each |
| Internal heap free (steady state) | — | `>= 65,536 B` total (Inv 48) | **`13,428 B`** | Shared by `esp-radio` Wi-Fi + BTDM |
| Internal heap low-water mark (`int_heap_min_free`) | — | `> 0 B` | **`5,128 B`** | **Critical bottleneck** |
| External PSRAM (`esp_alloc::HEAP` Region 0) | `~2.8 MiB` | `4,194,304 B` | `~1.2 MiB` | General `Box`/`Vec` allocations land here |
| `ota_0` Flash free | — | — | `~679 KB` | Flash is not the primary bottleneck |

Per `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md` §Global Constraints and §7, feature branches may add **zero** unbudgeted `.data`/`.bss`/`.dram2_uninit` statics and **zero** `.iram1`/`#[ram]` code.

### 1.2 Option Comparison

| Dimension | Option A: `embedded-tls` (`0.19.0`) | Option B: `esp-mbedtls` (C FFI) | Option C: Local LAN Proxy / LMS `Spotty` |
|---|---|---|---|
| **Presence in Workspace / Cache** | In local Cargo cache (`embedded-tls-0.19.0`); **not** in `rust/Cargo.lock` | **Absent** from both `Cargo.lock` and Cargo cache | LMS SlimProto pipeline already wired and certified |
| **TLS Versions** | **TLS 1.3 only** (`Aes128GcmSha256`, `Aes256GcmSha384`) | TLS 1.2 + TLS 1.3 | Full host OS OpenSSL / Rustls (`librespot`) |
| **Internal DRAM / Heap Impact** | `0 B` static buffers if boxed in PSRAM; however RSA cert verify (`alloc`), deep handshake call stack, and `esp-radio` Wi-Fi buffers press the **`5,128 B` LWM** | **Severe (Fatal)**: C `malloc`/`calloc` for SSL context, I/O buffers (~32 KB), X.509 DER chains, and MPI bignums (~45–80 KB/session) risks immediate OOM on `5,128 B` internal LWM | **`0 B`** |
| **PSRAM Buffer Cost** | Caller-supplied RX + TX record buffers: `16,640 B` RX (`embedded-tls/src/blocking.rs:49-50`) + `4,096–16,640 B` TX (`~21–33 KB`) + `8–16 KB` `smoltcp` TCP RX buffer per active HTTPS socket | ~45–80 KB if C `malloc` redirected to PSRAM (requires custom ESP-IDF glue not present in bare-metal `esp-hal`) | **`0 B`** |
| **IRAM Cost** | `0 B` (pure flash `.text`; no `#[ram]`) | Risk of pulling C IRAM/DRAM symbols unless linker-audited | **`0 B`** |
| **Flash Cost (`.text`/`.rodata`)** | Est. `~80–160 KB` (`aes-gcm`, `p256`, `sha2`, `hkdf`, `hmac`, `ecdsa`, `rsa`, `rustpki`/`webpki`, root CA certs) | Est. `~150–250 KB` C library + root CA bundle | **`0 B`** |
| **Build / Crate Compatibility** | **Blocked without workspace edits**: `embedded-tls 0.19.0` uses `edition = "2024"`, `embedded-io = "0.7"`, `heapless = "0.9"`, vs. workspace `edition = "2021"`, `embedded-io = "0.6"`, `heapless = "0.8"` | **Blocked**: requires C cross-compiler (`xtensa-esp32-elf-gcc`) + ESP-IDF mbedTLS headers + `unsafe` FFI bindings | **Immediate** (no new crate dependencies) |
| **Single-Core Audio Impact** | Blocking `open()` / SW P-256 + RSA verify stalls `pump_device_phase` for hundreds of ms (I2S DMA underrun risk pre-D1b) | Same blocking handshake CPU cost on Xtensa LX6 without pre-emption | **None** (ordinary HTTP/SlimProto stream) |

#### Detailed Notes on `embedded-tls 0.19.0` (`%USERPROFILE%\.cargo\registry\src\...\embedded-tls-0.19.0`)
- Inspected `Cargo.toml`, `src/lib.rs`, `src/config.rs`, and `src/blocking.rs`:
  - `embedded-tls` is `no_std` and supports `TlsConnection::new(socket, read_buf, write_buf)` with zero mandatory `alloc` for `Aes128GcmSha256` + `NoVerify`, **except** server certificate validation with RSA certificates (`feature = "rsa"` -> `rsa = "0.9.9"`, `rustpki` -> `der = "0.8.0-rc.2"`, `const-oid = "0.10.1"`) explicitly requires `alloc` (`Cargo.toml:70-74`).
  - `blocking.rs:48-50` notes that standard TLS servers emit up to `16,384 B` plaintext records (`16,640 B` encrypted), so `record_read_buf` must be `16,640 B` unless the peer honors `MaxFragmentLength` (public CDNs and Spotify endpoints generally do **not** honor RFC 6066 `max_fragment_length`).
  - `NoVerify` / `UnsecureProvider` (`config.rs:94-115,206`) skips certificate and signature checks, which would expose OAuth tokens and session credentials to MITM interception.
  - Because `embedded-tls 0.19.0` enforces `edition = "2024"` and `embedded-io = "0.7"` / `heapless = "0.9"`, a throwaway `cargo +esp check` cannot drop it into `louder-esp32-app` without modifying workspace dependencies or adding adapter crates — an explicit constraint boundary per the P5 study brief.

---

## 2. Authentication, Access Point, Mercury, and Audio Playback Flow

### 2.1 Legacy `cspot` Flow (Reference: `components/spotify/cspot/src/`)

The vendored C++ `cspot` library in `components/spotify/cspot/` illustrates the multi-stage network architecture required even under the legacy protocol:
1. **LAN Discovery (mDNS + ZeroConf HTTP)**:
   - Advertise `_spotify-connect._tcp.local` on UDP `5353` (`TXT` records `VERSION=1.0`, `CPath=/`, `Stack=SP`).
   - Serve `GET /?action=getInfo` (returning device metadata + base64 768-bit Oakley Group 1 DH `publicKey`) and `POST /?action=addUser` (receiving `userName`, `clientKey`, and encrypted `blob` wrapped with DH shared secret + SHA-1 + AES-128-CTR + HMAC-SHA1 + PBKDF2-HMAC-SHA1 + AES-192-ECB).
2. **Access Point Discovery (`HTTPS`)**:
   - `ApResolve.cpp:29` queries `https://apresolve.spotify.com/` over **TLS** to retrieve `ap_list` (`ap-*.spotify.com:4070` / `443`), plus dealer/spclient endpoints.
3. **Access Token Fetch (`HTTPS` — OAuth / `login5`)**:
   - `AccessKeyFetcher.cpp:79-80` posts to `https://accounts.spotify.com/api/token` over **TLS**. (Note: `AccessKeyFetcher.cpp:75` uses `grant_type=client_credentials` with a hardcoded `client_id` + `client_secret`, which does not authorize user playback on current endpoints without user OAuth/PKCE or `login5`.)
4. **AP TCP Session & Shannon Tunnel (`Session.cpp:35-103`)**:
   - Connect TCP to resolved AP, exchange `ClientHello` (`0x00, 0x04`) and `APResponseMessage` (verifying AP server RSA signature + completing DH key exchange), and initialize bidirectional `Shannon` stream cipher + MAC (`send_key`, `recv_key`).
   - Send `AP_LOGIN_REQUEST` (`0xAB`) with `LoginCredentials`; receive `APWelcome` (`0xAC`) carrying `reusable_auth_credentials` (`0x01`) or `APLError` (`0xAD`).
5. **Control & Metadata (`Mercury` / `Spirc` / `Dealer` + `spclient`)**:
   - Legacy `cspot` subscribes to `hm://remote/user/<username>/` over Mercury (`0xB2..0xB5`) and exchanges `spirc.proto` `Frame` messages (`kPlay`, `kPause`, `kNext`, `kPrev`, `kSetVolume`, `kNotify`).
   - Track GIDs are resolved via `hm://metadata/4/track/<gid>` (or `spclient` HTTPS) to obtain `AudioFile` `file_id`s (`OGG_VORBIS_96/160/320`).
6. **Audio Key & CDN Stream (`AudioKey` + `HTTPS Range` + `AES-128-CTR` + `Ogg Vorbis`)**:
   - Request 16-byte AES key over AP Shannon tunnel (`0x0C` `AudioKeyRequest` -> `0x0D` `AudioKeySuccess` / `0x0E` `AudioKeyFailure`).
   - Resolve CDN URL via `storage-resolve` (`HTTPS`), stream encrypted file chunks via `HTTPS GET` with `Range:` headers (`CDNAudioFile.cpp:47`), decrypt in-place with AES-128-CTR (`AUDIO_AES_IV` + `byte_offset / 16`), strip the `167 B` (`0xA7`) Spotify Ogg header (`SPOTIFY_HEADER_SIZE`), and decode via `VorbisDecoder` (`lewton`).

### 2.2 Deprecation & Modern Protocol Reality

- **Legacy AP `0xAB` & Username/Password Deprecation**: Spotify has deprecated legacy username/password and unauthenticated ZeroConf->AP `0xAB` login paths on many accounts/regions in favor of **OAuth2 Authorization Code + PKCE** (`accounts.spotify.com`) and **`login5.spotify.com/v3/login`** (protobuf over **HTTPS TLS** with Hashcash / client-token challenge).
- **Spirc / Mercury Migration**: Modern Spotify Connect controllers increasingly rely on **Connect-State** (`https://spclient.wg.spotify.com/connect-state/v1/...`) and persistent **Dealer WebSockets** (`wss://...`) rather than legacy Mercury `hm://remote/user/...` Spirc frames alone.
- **Implication**: Even finishing legacy `cspot` wiring does **not** guarantee reliable Connect pairing with current Spotify mobile/desktop apps without also building an HTTPS TLS 1.2/1.3 client, WebSocket client, `login5` client-token/hashcash flow, and OAuth token management.

---

## 3. Audit of Existing `squeezelite-cspot` & `main.rs` Coverage

Inspected all modules in `rust/crates/squeezelite-cspot/` (`#![no_std]`, `#![forbid(unsafe_code)]`) and `rust/crates/louder-esp32-app/src/main.rs`:

### 3.1 What Exists and Works (Unit-Tested Host `no_std + alloc` Primitives)

| Module | Implemented Surface | Gaps / Notes |
|---|---|---|
| `crypto.rs` | `BigUint`, `montgomery_mod_pow`, `OakleyGroup1Dh` (768-bit), `Sha1`, `hmac_sha1`, `pbkdf2_hmac_sha1`, `Aes128Ctr`, `Aes192Ecb`, `big_num_add`, `base64_{encode,decode}`, `constant_time_eq` | No SHA-256/SHA-384, no AES-GCM, no P-256/X25519, no RSA signature verification (needed for AP `APResponseMessage` and TLS) |
| `shannon.rs` | Full `Shannon` stream cipher & MAC (`key`, `nonce`, `stream`, `maconly`, `encrypt`, `decrypt`, `finish`, `finish_and_verify`) with `Drop` zeroization | Complete for AP tunnel framing; requires an AP TCP state machine to drive it |
| `zeroconf.rs` | `SpotifyMdnsConfig` / `SpotifyMdnsBuilder` (`_spotify-connect._tcp.local`), `ZeroConfServer::handle_request` (`getInfo`, `addUser`), `decode_blob`, `ReusableCredentials` (protobuf + AES-128-CTR + HMAC-SHA1 flash blob) | Never called by `main.rs`; `getInfo` advertises legacy `version: "2.7.1"` and `AudioDongle`; no OAuth/PKCE ZeroConf fields |
| `mercury.rs` | `MercuryType` opcodes, `MercuryHeader` protobuf, `MercuryPacket` multipart assembly, `AudioKeyPacket` (`0x0C..0x0E`) | Framing codec only; no session/sequence multiplexing state machine or `spclient` HTTPS transport |
| `spirc.rs` | `TrackMetadata` (`title`, `artist`, `album`, `duration_ms`, `artwork_image_id`), `SpircFrame` protobuf codec, `SpircStateMachine` (`kPlay`, `kPause`, `kNext`, `kPrev`, `kSetVolume`, `build_notify_frame`) | `TrackMetadata` lacks `AudioFile` list (`file_id` GID + bitrate format) needed for AudioKey/CDN; no Connect-State / Dealer support |
| `cdn.rs` | `AudioKeyDecryptor` (`AUDIO_AES_IV`, `0xA7` strip, arbitrary seek), `CdnAudioDecryptor`, `CdnStreamFeeder` feeding `squeezelite_decoders::VorbisDecoder` (`lewton`) | Never wired in `main.rs`; `CdnStreamFeeder` assumes raw chunks are pushed by an external HTTPS Range fetcher |

### 3.2 What Is Stubbed in `louder-esp32-app/src/main.rs`

In `rust/crates/louder-esp32-app/src/main.rs:4200-4242` and `4426-4533`:
- **mDNS / Port Mismatch**: `SpotifyMdnsBuilder` advertises `port: 80` (`main.rs:4205`), whereas the Spotify TCP socket listens on `4070` (`main.rs:4443`), and the HTTP server on port `80` does not delegate `action=getInfo`/`addUser` to `ZeroConfServer`.
- **Static `getInfo` Facade**: On any `GET` or `POST` on TCP `4070`, `main.rs:4449-4466` returns a hardcoded JSON string with `"publicKey":""` and hardcoded `deviceID` `"c82e18beb834..."`, ignoring `ZeroConfServer` and `OakleyGroup1Dh`.
- **Unencrypted Spirc/PCM Facade**: On non-HTTP packets on TCP `4070`, `main.rs:4467-4529` attempts `SpircFrame::parse` directly on plaintext TCP bytes (bypassing AP handshake, Shannon cipher, and Mercury), configures `ActiveDecoder::Pcm` (`16-bit, 2 ch`, `main.rs:4479-4481`) instead of `CdnStreamFeeder` / `VorbisDecoder`, and ingests raw TCP payload as PCM.
- **Missing Infrastructure**: No TLS 1.2/1.3 client, no HTTP/HTTPS client with `Range:` support, no DNS resolver yet on `MK5-Rust` (queued in P2), no WebSocket client, no CSPRNG wiring into `squeezelite-cspot`, and `SOCKET_SLOTS = 8` on `MK5-Rust` is currently 100% full (F4 raises it to `16`).

---

## 4. Licensing & Legal Position

1. **Repository License (`GPL-3.0-or-later`)**:
   - `rust/Cargo.toml:20` sets `[workspace.package] license = "GPL-3.0-or-later"`.
   - `embedded-tls` (`Apache-2.0`) and `lewton` (`MIT/Apache-2.0`) are license-compatible with `GPL-3.0-or-later`.
   - No proprietary Spotify eSDK binaries are linked in the Rust workspace.
2. **Spotify Embedded SDK (`eSDK`) Incompatibility**:
   - Official Spotify Connect certification requires Spotify's proprietary, NDA-gated `eSDK` binary blob and per-product commercial licensing, which is legally incompatible with `GPL-3.0-or-later` firmware distribution and practically incompatible with a `5,128 B` internal heap LWM on classic ESP32.
3. **Unofficial `librespot` / `cspot` Protocol & Spotify Developer Terms Risks**:
   - Replicating Spotify's private AP/Dealer/CDN decryption protocol violates Spotify's Terms of Service / Developer Policy (no DRM circumvention, no unauthorized streaming clients, no embedding third-party `client_id` / `client_secret` keys).
   - Committing any `client_secret` or extracted key in a public `GPL-3.0-or-later` repository is both a policy violation and an Invariant 24 / security failure.
   - Because the protocol is undocumented and actively rotated, any native ESP32 client carries ongoing maintenance risk outside the repository's control.

---

## 5. Go / No-Go Verdict & Disposition Plan

### 5.1 Decision: **NO-GO** for Native On-Device Spotify Connect in the Current Plan

Do **not** schedule a Phase 1/2/3 implementation branch for native on-device TLS + Spotify Connect on `louder-esp32-app`. The combination of:
- `int_heap_min_free = 5,128 B` (leaving negligible margin for TLS + Wi-Fi/BTDM coexistence),
- `embedded-tls 0.19.0` TLS-1.3-only + `edition = "2024"` / `embedded-io 0.7` / `heapless 0.9` incompatibility,
- single-core CPU starvation during software P-256/RSA handshakes pre-D1b,
- missing OAuth2/`login5`/Dealer WebSocket/`spclient`/HTTPS-Range layers beyond legacy `squeezelite-cspot`, and
- `GPL-3.0-or-later` vs. Spotify proprietary/ToS constraints

makes native on-device implementation low-value and high-risk relative to the rest of the parity plan (`F3 -> F4 -> P1..P4 -> D1a/P6 -> D1b`).

### 5.2 Recommended Path (Immediate, Zero ESP32 Cost)

1. **Use LMS `Spotty` Plugin (LAN Offload)**: Run Spotify Connect on the local Lyrion Music Server host (`Spotty`), which handles TLS, OAuth/`login5`, and CDN decryption on host OS hardware and delivers clean PCM/FLAC over SlimProto to `louder-esp32-app` with **`0 B`** extra internal heap, **`0 B`** IRAM, and **`0`** extra sockets.
2. **Retain or Retire the Port-4070 Facade Separately**: Keep `squeezelite-cspot` and the `4070` facade untouched for now per the parity plan ("The Spotify facade stays as it is until the P5 study reports"), or gate/remove the misleading non-functional `_spotify-connect` mDNS announcement in a tiny future cleanup PR if it confuses Spotify clients on the LAN.

### 5.3 Conditional Phased Plan (Only If Owner Explicitly Re-Opens Native Connect Post-D1b)

If the owner later chooses to revisit native on-device Connect despite the **No-Go** recommendation, work MUST be gated on all prerequisites below and split into four serial branches:

| Phase | Gate / Prerequisites | Deliverable & Verification |
|---|---|---|
| **Gate 0 (Precondition)** | `F4` (`SOCKET_SLOTS = 16`), `P2` (DNS resolver), and `D1b` (Core 1 decode/output isolation) merged; measured `int_heap_min_free >= 16,384 B` with Wi-Fi + BTDM active | Re-measure internal heap LWM and `.stack` watermark on `DEVBENCH` |
| **Phase S1 (`spike/tls13-psram`)** | Workspace `embedded-io` / `heapless` alignment (or thin `0.6<->0.7` adapter); `embedded-tls` with PSRAM-allocated `16,640 B` RX / `4,096 B` TX record buffers and `rustpki` CA verification on Core 0 | Measure exact `.rodata`/`.text`, `.stack`, and `int_heap_min_free` delta during live HTTPS `Range:` fetch against `DEVBENCH` without audio underrun |
| **Phase S2 (`feat/cspot-auth-control`)** | Wire `ZeroConfServer` on port `80`/`4070` with hardware RNG `OakleyGroup1Dh`, user-supplied OAuth/`login5` token flow (no embedded secrets), `apresolve`, and Dealer WebSocket / Connect-State | Host unit tests + `DEVBENCH` pairing test with Spotify mobile client |
| **Phase S3 (`feat/cspot-cdn-vorbis`)** | Wire `CdnStreamFeeder` (`AudioKeyDecryptor` + `VorbisDecoder`) fed by PSRAM-buffered HTTPS `Range:` client into `services/spotify.rs` via `claim_external_pcm` | 1-hour playback soak on `DEVBENCH` with zero I2S underruns and `int_heap_min_free` within F3+ budget |
