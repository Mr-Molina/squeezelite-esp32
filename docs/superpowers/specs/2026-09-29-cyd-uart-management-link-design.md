# Architecture Specification: CYD Touch Screen Management Link for Squeezelite-ESP32

- **Author**: Squeezelite-ESP32 Core Team & AI Agent
- **Date**: 2026-09-29
- **Status**: Validated & Approved
- **Target Hardware**: Squeezelite-ESP32 Host (WROVER / WROOM / A1S) + Cheap Yellow Display (CYD ESP32-2432S028R)

---

## 1. Executive Summary & Intent

This specification defines a dedicated, bidirectional communication link between an ESP32 device running **squeezelite-esp32** (the Host audio player) and an external **CYD (ESP32-2432S028R)** touch display module (the Client management controller). 

The link enables the CYD to operate as a local, low-latency, touch-driven "Now Playing" console and transport remote:
- Displaying real-time track metadata (Title, Artist, Album), elapsed time, total duration, volume level, and active source mode (LMS, Spotify, AirPlay, Bluetooth).
- Allowing touch-based playback control (Play, Pause, Toggle, Next, Previous) and volume adjustment directly manipulating Squeezelite's playback pipeline.
- Communicating over an isolated, wired hardware UART serial connection using Newline-Delimited JSON (NDJSON).

---

## 2. Hardware Architecture & Physical Interconnect

### 2.1 Physical Interconnect
Communication occurs over a direct 3-wire asynchronous serial connection at 3.3V logic levels:

```mermaid
flowchart LR
    subgraph Host ["Squeezelite-ESP32 Host"]
        H_TX["UART TX (Default: GPIO 17)"]
        H_RX["UART RX (Default: GPIO 16)"]
        H_GND["GND"]
    end

    subgraph Client ["CYD ESP32-2432S028R Client"]
        C_RX["UART RX (Connector P3: IO22)"]
        C_TX["UART TX (Connector P3: IO27)"]
        C_GND["GND"]
    end

    H_TX -->|"3.3V Serial (115200 8N1)"| C_RX
    H_RX <--|"3.3V Serial (115200 8N1)"| C_TX
    H_GND --- C_GND
```

- **Squeezelite Host UART**:
  - Uses secondary hardware UART (`UART_NUM_1` or `UART_NUM_2`), keeping `UART_NUM_0` dedicated to bootloader and FreeRTOS console logs.
  - Pin routing is fully configurable at runtime via NVS string `cyd_config` (e.g. `uart=1,tx=17,rx=16,baud=115200`).
- **CYD Display Board (ESP32-2432S028R)**:
  - Uses secondary UART assigned to pins on Connector P3 (IO22 as RX, IO27 as TX) or CN1 (IO1/IO3) to prevent contention with the onboard CH340 USB-to-UART programming interface.
- **Serial Configuration**:
  - Baud rate: 115200 bps (configurable up to 921600 bps).
  - Data bits: 8, Parity: None, Stop bits: 1 (8N1).
  - Hardware flow control: Disabled (RTS/CTS not required for low-bandwidth telemetry).

---

## 3. Protocol Specification: Newline-Delimited JSON (NDJSON)

All communication consists of discrete, single-line JSON objects terminated by a single newline character `\n` (`0x0A`). Trailing carriage returns `\r` (`0x0D`) are ignored. Maximum line length is strictly bounded at 512 bytes.

### 3.1 Host $\rightarrow$ CYD Telemetry Events

#### Event 1: Track Metadata (`event: meta`)
Broadcast when a new track begins playing or metadata changes across any active streaming sink (LMS, Spotify, AirPlay, Bluetooth).

```json
{"event":"meta","title":"Time","artist":"Pink Floyd","album":"The Dark Side of the Moon"}
```
- `title` *(string)*: Track name.
- `artist` *(string)*: Artist or performer name.
- `album` *(string)*: Album name (may be empty string if unavailable).

#### Event 2: Playback Status & Progress (`event: status`)
Broadcast when playback state changes (play, pause, stop), upon volume adjustment, and periodically at 1 Hz during active playback to synchronize elapsed track time.

```json
{"event":"status","state":"play","elapsed":74,"duration":413,"vol":65}
```
- `state` *(string)*: `"play"`, `"pause"`, or `"stop"`.
- `elapsed` *(integer)*: Elapsed track duration in seconds.
- `duration` *(integer)*: Total track duration in seconds. Set to `0` if duration is unknown (e.g. internet radio stream).
- `vol` *(integer)*: Current player volume level from `0` to `100`.

#### Event 3: System & Mode Status (`event: sys`)
Broadcast on initial boot, network state changes, or in response to a `sync` command.

```json
{"event":"sys","mode":"LMS","name":"Living Room HiFi","ip":"192.168.1.150"}
```
- `mode` *(string)*: Current active source (`"LMS"`, `"Spotify"`, `"AirPlay"`, `"BT"`, or `"idle"`).
- `name` *(string)*: Configured player device name.
- `ip` *(string)*: Current IPv4 address on active network interface.

---

### 3.2 CYD $\rightarrow$ Host Control Commands

Emitted by the CYD controller in response to user touch interactions.

#### Command 1: Playback Transport
- **Toggle Play/Pause**:
  ```json
  {"cmd":"toggle"}
  ```
- **Explicit Play**:
  ```json
  {"cmd":"play"}
  ```
- **Explicit Pause**:
  ```json
  {"cmd":"pause"}
  ```
- **Next Track**:
  ```json
  {"cmd":"next"}
  ```
- **Previous Track**:
  ```json
  {"cmd":"prev"}
  ```

#### Command 2: Volume Control
- **Absolute Volume**:
  ```json
  {"cmd":"vol","val":75}
  ```
  `val` *(integer)*: Absolute target volume from `0` to `100`.
- **Relative Volume Step**:
  ```json
  {"cmd":"vol_step","dir":1}
  ```
  `dir` *(integer)*: `1` for volume up, `-1` for volume down.

#### Command 3: Full State Sync Request
- **Sync**:
  ```json
  {"cmd":"sync"}
  ```
  Instructs Squeezelite-ESP32 to immediately emit a complete burst of current state: `sys` followed by `meta` and `status`. Used upon CYD boot or connection re-establishment.

---

## 4. Squeezelite-ESP32 Subsystem: `components/cyd_link`

### 4.1 Component Structure
The host integration is isolated within `components/cyd_link/`:
- `cyd_link.h`: Public API declarations.
- `cyd_link.c`: FreeRTOS UART ringbuffer task, UART configuration, line framing, and mutex-protected TX serializer.
- `cyd_link_dispatch.c`: JSON command unpacker and dispatcher to `actrls` and `output_volume`.
- `cyd_link_hooks.c`: Event hooks subscribing to `displayer_metadata`, `displayer_timer`, and network status updates.
- `CMakeLists.txt`: ESP-IDF component registration.

### 4.2 Control Integration with `actrls`
Inbound commands map directly to Squeezelite's unified control abstraction:
```c
void cyd_link_dispatch_command(const char *cmd_name, int param) {
    if (strcmp(cmd_name, "toggle") == 0) {
        actrls_handler h = get_ctrl_handler(ACTRLS_TOGGLE);
        if (h) h(true);
    } else if (strcmp(cmd_name, "play") == 0) {
        actrls_handler h = get_ctrl_handler(ACTRLS_PLAY);
        if (h) h(true);
    } else if (strcmp(cmd_name, "pause") == 0) {
        actrls_handler h = get_ctrl_handler(ACTRLS_PAUSE);
        if (h) h(true);
    } else if (strcmp(cmd_name, "next") == 0) {
        actrls_handler h = get_ctrl_handler(ACTRLS_NEXT);
        if (h) h(true);
    } else if (strcmp(cmd_name, "prev") == 0) {
        actrls_handler h = get_ctrl_handler(ACTRLS_PREV);
        if (h) h(true);
    } else if (strcmp(cmd_name, "vol") == 0) {
        output_volume(param);
    } else if (strcmp(cmd_name, "sync") == 0) {
        cyd_link_broadcast_full_sync();
    }
}
```
This guarantees that regardless of which engine is streaming (LMS, Spotify, AirPlay, Bluetooth), the command invokes the correct active handler without duplicating sink-specific logic.

### 4.3 Telemetry Hooks
Even when no physical display is configured on the Squeezelite host (e.g. headless DAC installations), `cyd_link` hooks into:
1. `displayer_metadata(artist, album, title)`: Generates and queues the `meta` event.
2. `displayer_timer(mode, elapsed, duration)`: Generates and queues the `status` event.
3. Network connection callbacks: Generates the `sys` event.

---

## 5. CYD Touch GUI Subproject: `cyd-controller`

### 5.1 Architecture & Build Environment
`cyd-controller` is an ESP-IDF v4.4/v5.x subproject located in the root directory:
```
cyd-controller/
├── CMakeLists.txt
├── sdkconfig.defaults
├── main/
│   ├── CMakeLists.txt
│   ├── main.c              # FreeRTOS tasks & peripheral initialization
│   ├── ui/
│   │   ├── ui_now_playing.c # LVGL screen, labels, buttons, progress bar
│   │   └── ui_theme.c       # Dark modern theme styling
│   ├── comms/
│   │   ├── cyd_uart.c       # FreeRTOS UART ringbuffer receiver
│   │   └── cyd_protocol.c   # NDJSON parser & command sender
│   └── drivers/
│       ├── ili9341.c        # 320x240 LCD SPI driver
│       └── xpt2046.c        # Resistive touch SPI driver
```

### 5.2 Screen Layout (320x240 Landscape)
- **Top Status Bar (24 px)**:
  - Source Mode Badge (e.g., `[SPOTIFY]`, `[LMS]`).
  - Player Host Name (`"Living Room HiFi"`).
  - Link Health Indicator (`[OK]` or `[Reconnecting...]`).
- **Main Content Area (130 px)**:
  - Track Title: 20 pt bold font, with LVGL circular scrolling (`LV_LABEL_LONG_SCROLL_CIRCULAR`) if text exceeds screen width.
  - Artist & Album: 14 pt regular font, scrolling or truncated.
  - Progress Bar: Visual slider with elapsed time (left) and remaining/total duration (right).
- **Transport & Volume Bar (86 px)**:
  - Left / Center: Previous Track `[|<<]`, Play/Pause `[> / ||]`, Next Track `[>>|]`.
  - Right: Volume Slider with real-time numeric percentage indicator.

### 5.3 Touch Event Throttling
Volume slider adjustments are throttled using an LVGL timer to emit at most one `{"cmd":"vol","val":V}` command every 100 ms (10 Hz), preventing UART TX queue starvation during rapid touch scrubbing.

---

## 6. Fault Handling & Resilience

1. **Self-Healing Framing**:
   - The line parser accumulates bytes until `\n`. If a line exceeds 512 bytes without `\n`, the buffer resets immediately to prevent stack/heap corruption.
   - Any line failing `cJSON_Parse()` is discarded with a debug warning without causing task panics.
2. **Heartbeat & Reconnection**:
   - If no valid telemetry packet is received by CYD for $> 5$ seconds, the GUI displays a reconnecting banner.
   - CYD initiates periodic `{"cmd":"sync"}` probes every 3 seconds until communication resumes.
   - Upon receiving `{"cmd":"sync"}`, Squeezelite-ESP32 re-emits `sys`, `meta`, and `status`, fully repopulating the display.
3. **Memory Safety & Invariant Compliance**:
   - All `cJSON` pointers allocated during string creation or JSON parsing are freed within the exact same function scope using `cJSON_Delete()`.
   - FreeRTOS UART ringbuffer allocations are statically sized at boot time.

---

## 7. Verification & Testing Plan

1. **Protocol Unit Tests (`components/cyd_link/test/test_cyd_link.c`)**:
   - Validate serialization of track metadata including special characters and UTF-8 strings.
   - Test deserialization of valid commands, unknown commands, out-of-range volume values, and truncated JSON lines.
2. **Console Simulation Probe (`cyd-test`)**:
   - Add a diagnostic CLI command to Squeezelite-ESP32 console (`cyd-test send <json>` and `cyd-test status`) to verify UART transmission and receipt without physical display attached.
3. **CYD Compilation Gate**:
   - Clean compilation of `cyd-controller` under ESP-IDF (`idf.py build`).
4. **End-to-End Loopback Verification**:
   - Loopback verification linking TX to RX to confirm bidirectional framing, command execution, and state reflection.
