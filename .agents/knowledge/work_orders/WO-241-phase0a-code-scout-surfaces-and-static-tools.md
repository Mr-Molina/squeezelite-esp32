# Work Order: WO-241 — Phase 0a Code Scout 2: `MK5-Rust` (`e7fa3411`) Architecture Surface, ELF Size & Static Tool Prober

- **Work Order ID**: `WO-241`
- **Timestamp**: `2026-11-01T08:33:00-05:00`
- **Assigned Subagent Role**: `Architecture Surface & Static Tool Prober` (`Model: "flash"`, `TypeName: "self"`, read-only)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Discovery Guardrails
Execute a fast (<60s), strictly read-only Phase 0a static tool probe and boundary surface inventory on `/home/console-admin/Github/squeezelite-esp32/rust` (`e7fa3411`):
1. Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py --strict` across all 11 crate `src/` directories in `/home/console-admin/Github/squeezelite-esp32/rust/crates/*/src`.
2. Run `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`.
3. Run `/home/console-admin/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20240906/xtensa-esp-elf/bin/xtensa-esp32-elf-size -A /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` to capture exact ELF section sizes (`.stack`, `.dram2_uninit`, `.bss`, `.data`, `.text`, `.rodata`).
4. Catalog all `unsafe` blocks (file and line numbers) across `/home/console-admin/Github/squeezelite-esp32/rust/crates/` and list all HTTP/TCP/UDP/UART/I2C/Bluetooth entry points in `louder-esp32-app`.

## 2. Hard Safety Constraints
- **Read-Only Mandate**: Do NOT modify any source files or run any flash commands.
- **Strict Absolute Paths (`Rule 24`)**: Every path and command must begin with `/`.
- **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` is empty before completing.

## 3. Deliverables
- Append YAML static tool & surface inventory to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-241-phase0a-code-scout-surfaces-and-static-tools.md` and return the YAML summary in your final message.

---

## 4. Fulfillment Section

```yaml
work_order_id: WO-241
status: "[EXECUTED & VERIFIED]"
timestamp: "2026-11-01T08:38:00-05:00"
static_audit:
  command: "/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py --strict /home/console-admin/Github/squeezelite-esp32/rust/crates/*/src"
  result: "PASS"
  errors: 0
  warnings: 0
clippy_audit:
  command: "/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings"
  result: "PASS"
  warnings: 0
elf_size_profile:
  binary: "/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app"
  tool: "/home/console-admin/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20260121/xtensa-esp-elf/bin/xtensa-esp32-elf-size"
  sections:
    text: 1406801
    rodata: 207552
    rodata_wifi: 42484
    rwtext: 27232
    rwtext_wifi: 88304
    data: 18992
    data_wifi: 2120
    bss: 72544
    stack: 37416
    dram2_uninit: 95256
    flash_appdesc: 256
    vectors: 1024
    xt_prop: 312228
    xt_lit: 11304
    comment: 253
    eh_frame: 60
    xtensa_info: 56
    eh_frame_hdr: 28
    espressif_metadata: 21
  total_bytes: 2323931
unsafe_audit:
  total_unsafe_items: 45
  by_crate:
    cyd-controller-app: 0
    nanomp3-core: 1
    louder-esp32-app: 35
    squeezelite-audio-core: 9
    squeezelite-config: 0
    squeezelite-cspot: 0
    squeezelite-cyd-link: 0
    squeezelite-decoders: 0
    squeezelite-proto: 0
    squeezelite-raop: 0
    squeezelite-tas58xx: 0
  catalog:
    - crate: "nanomp3-core"
      file: "crates/nanomp3-core/src/minimp3/layer12.rs"
      line: 241
      type: "unsafe block"
      description: "Cast/transmute to uninit granule buffer &mut [f32; 1152]"
    - crate: "squeezelite-audio-core"
      file: "crates/squeezelite-audio-core/src/ringbuffer.rs"
      line: 63
      type: "unsafe impl"
      description: "unsafe impl<'a> Send for AudioRingBuffer<'a>"
    - crate: "squeezelite-audio-core"
      file: "crates/squeezelite-audio-core/src/ringbuffer.rs"
      lines: [138, 148, 243, 253, 286, 296]
      type: "unsafe block"
      description: "Direct slice pointer slicing and arithmetic in lock-free ringbuffer"
    - crate: "squeezelite-audio-core"
      file: "crates/squeezelite-audio-core/src/ringbuffer.rs"
      lines: [364, 388]
      type: "unsafe impl"
      description: "unsafe impl Send for Producer and Consumer"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      line: 500
      type: "unsafe attribute"
      description: "#[unsafe(no_mangle)] for firmware app entry"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [752, 757, 762, 767]
      type: "unsafe block"
      description: "Direct MMIO read/write_volatile to I2S0_CLKM_CONF_ADDR and I2S0_SAMPLE_RATE_CONF_ADDR"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [1012, 1063, 1101, 1105, 1534, 1537, 1540, 1589, 1606]
      type: "unsafe attribute"
      description: "#[unsafe(link_section = \".dram2_uninit\")] for static uninit buffer pools"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [1038, 1068, 1134, 1568, 1593, 1616, 1693, 1705, 2385, 2436, 2544]
      type: "unsafe block"
      description: "addr_of_mut! / MaybeUninit static pointer access for buffers and bt_sink"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [1145, 1228]
      type: "unsafe fn"
      description: "psram_send_cmd_spi1 and init_and_map_psram_4mb hardware initialization"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [1163, 1290, 2362, 2369, 2385]
      type: "unsafe block"
      description: "Direct MMIO SPI1 and MMU page table manipulation for 4MB PSRAM mapping"
    - crate: "louder-esp32-app"
      file: "crates/louder-esp32-app/src/main.rs"
      lines: [2254, 2263, 2466, 2524]
      type: "unsafe block"
      description: "Core 1 app launch register setup and APP_CORE_STACK pointer dereference"
boundary_surfaces:
  http:
    server:
      port: 80
      file: "crates/louder-esp32-app/src/http_control.rs"
      routes:
        - "GET / (and GET /status.json): JSON status report (playback, volume, Wi-Fi RSSI/IP, I2C statistics, Bluetooth state, heap)"
        - "POST /connect.json: Wi-Fi STA credential association (ssid, password)"
        - "POST /recovery.json: Reboot to recovery partition"
        - "POST /reboot.json: Flush settings and reboot to active OTA0 partition"
        - "POST /reboot_ota.json: Flush settings and reboot into alternate OTA partition"
    client:
      file: "crates/louder-esp32-app/src/network.rs"
      functionality: "HttpAudioStream client fetching MPEG/PCM/FLAC audio streams from LMS or web streams over TCP (HTTP GET, redirection support)"
  tcp:
    listeners:
      - port: 80
        protocol: "HTTP 1.1 / JSON API"
        file: "crates/louder-esp32-app/src/http_control.rs"
      - port: 23
        protocol: "Telnet console (TELNET_PORT)"
        file: "crates/louder-esp32-app/src/console.rs"
      - port: 5000
        protocol: "AirPlay / RAOP RTSP streaming session"
        crate: "squeezelite-raop"
    clients:
      - port: 3483
        protocol: "SlimProto LMS control session (HELO, STAT, RESP, SETD)"
        file: "crates/louder-esp32-app/src/tasks.rs, crates/squeezelite-proto"
      - port: "variable"
        protocol: "HTTP streaming audio stream connection"
        file: "crates/louder-esp32-app/src/network.rs"
      - port: "variable"
        protocol: "Spotify Connect daemon/client"
        crate: "squeezelite-cspot"
  udp:
    listeners_and_clients:
      - port: 3483
        protocol: "LMS Auto-Discovery Client over UDP broadcast (255.255.255.255:3483)"
        file: "crates/louder-esp32-app/src/network.rs"
      - port: 5353
        protocol: "mDNS / DNS-SD Zeroconf discovery responder"
        file: "crates/louder-esp32-app/src/network.rs"
      - port: 123
        protocol: "NTP / SNTP time synchronization client"
        file: "crates/louder-esp32-app/src/network.rs"
      - ports: "dynamic (timing, control, audio)"
        protocol: "RAOP RTP/RTCP audio and synchronization transport"
        crate: "squeezelite-raop"
  uart:
    interfaces:
      - port: "UART0"
        pins: "Default TX/RX (USB-UART)"
        baud: 115200
        protocol: "Console CLI / REPL (system, i2c, config, join, update, reboot)"
        file: "crates/louder-esp32-app/src/console.rs"
      - port: "UART2"
        pins: "Louder-ESP32 Wrover: TX=GPIO 4, RX=GPIO 36; Louder-ESP32-S3: TX=GPIO 43, RX=GPIO 44"
        baud: 115200
        protocol: "CYD (Cheap Yellow Display) 2.8-inch touch panel bridge (CydUartBridge)"
        file: "crates/louder-esp32-app/src/cyd.rs"
        inbound_commands: ["CMD=VOL:<val>", "CMD=PLAY", "CMD=PAUSE", "CMD=NEXT", "CMD=PREV"]
        outbound_messages: ["METADATA=...", "STATE=...", "TIME=..."]
  i2c:
    interfaces:
      - port: "I2C0 Master"
        pins: "Louder-ESP32 Wrover: SDA=GPIO 21, SCL=GPIO 27; Louder-ESP32-S3: SDA=GPIO 8, SCL=GPIO 9"
        clock: "100 kHz / 400 kHz"
        devices:
          - name: "Texas Instruments TAS5805M / TAS5825M Class-D Audio Amplifier"
            addresses: ["0x2D (Wrover ADR=GND)", "0x4C (S3 ADR=DVDD)", "0x2C (TAS5805M primary)"]
            registers: ["Fault Clear (0x78)", "Digital Volume (0x4C)", "Device Control (0x03)", "Book/Page switching"]
            file: "crates/louder-esp32-app/src/amp.rs, crates/squeezelite-tas58xx"
          - name: "Console I2C Diagnostics"
            operations: ["i2c scan", "i2c get", "i2c set"]
            file: "crates/louder-esp32-app/src/console.rs"
  bluetooth:
    mode: "Dual-Mode Bluetooth Classic BR/EDR A2DP Sink (BtA2dpSink)"
    transport: "ESP32 VHCI Transport (esp-hal::peripherals::BT)"
    local_name: "Louder-ESP32-Audio"
    file: "crates/louder-esp32-app/src/bt_a2dp.rs"
    protocols:
      - "HCI (Host Controller Interface): Commands, event loops, connection accept/teardown"
      - "L2CAP PSM 0x0001 (SDP Server): Exposes A2DP Audio Sink (0x110B) Service Record"
      - "L2CAP PSM 0x0019 (AVDTP): Audio stream negotiation and streaming state machine"
      - "L2CAP PSM 0x0017 (AVCTP / AVRCP): Transport control and volume synchronization"
      - "SBC Audio Decoder: Fixed-point polyphase synthesis filterbank decoding 16-bit stereo PCM"
```

