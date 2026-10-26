# Work Order: WO-023 — Audit `esp-hal 1.2.2` I2S/DMA, `esp-storage 0.10.0`, `esp-bootloader-esp-idf 0.6.0`, and `esp-wifi`/`esp-radio` + `smoltcp` APIs

- **Work Order ID:** `WO-023`
- **Timestamp:** `2026-10-21T07:05:00-04:00`
- **Assigned Role:** `Discovery Scout / Research Specialist`
- **Status:** `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Perform a fast, read-only technical audit of the exact crates and APIs needed to add all missing bare-metal hardware and networking functionality to `louder-esp32-app` (`xtensa-esp32-none-elf`):
1. **Local Cargo Registry Inspection (`/home/console-admin/.cargo/registry/src/*`)**:
   - Inspect `esp-hal-1.2.2` for:
     - `i2s::master::I2s` (or `i2s`) + `dma` (exact types, constructors, pins, and blocking/polling DMA write methods on ESP32).
     - `system::software_reset()` or RTC reset APIs.
   - Inspect `esp-storage-0.10.0` for `FlashStorage` APIs (`embedded_storage::ReadStorage` / `Storage` / `NorFlash`, `read`, `write`) to read/write raw flash offsets (`0x9000` NVS, `0xD000` `otadata`, `0x3F0000` `settings`).
   - Inspect `esp-bootloader-esp-idf-0.6.0` for `ota` / `partitions` / `otadata` helpers (how to switch active boot partition back to `factory` / `recovery` at `0x10000`).
2. **Pure-Rust Wi-Fi + TCP/IP (`esp-wifi` / `esp-radio` + `smoltcp` + `esp-alloc`)**:
   - Check `/home/console-admin/.cargo/registry/` or use `cargo search` / `search_web` to find the exact versions of `esp-wifi` (or `esp-radio`), `smoltcp`, and `esp-alloc` that depend on `esp-hal = 1.2.2`.
   - Check whether `esp-wifi` / `esp-alloc` require any `unsafe` blocks in the binary crate (and if so, whether isolating them in a dedicated hardware adapter or relaxing `#![forbid(unsafe_code)]` to `#![deny(unsafe_code)]` with `// SAFETY:` contracts per `rust-pro` is required).
3. **NVS / Wi-Fi Passphrase on `172.22.1.15`**:
   - Check if `esp-storage::FlashStorage` can read the ESP-IDF NVS partition at `0x9000`..`0xD000` or if `SL_WIFI_SSID` / `SL_WIFI_PASS` compile-time `option_env!` + runtime NVS storage in `0x3F0000` (`settings`) is the cleanest mechanism.

Do NOT modify any repository source files. Write all findings into the Fulfillment Section of this Work Order.

---

## Fulfillment Section

### 1. `esp-hal 1.2.2` I2S, DMA, and System Reset Audit

#### Critical Prerequisite: `unstable` Feature Gate
On ESP32 (Xtensa LX6), `peripherals.I2S0` and `peripherals.DMA_I2S0` are defined as unstable singletons in `esp-metadata-generated` (`_generated_esp32.rs:4862, 4868`).
**Empirical Finding**: Unless `features = ["esp32", "unstable"]` is enabled on `esp-hal`, `peripherals.I2S0` and `peripherals.DMA_I2S0` are compiled as `pub(crate)` and cannot be accessed by external crates! Downstream crates must declare:
```toml
esp-hal = { version = "1.2.2", features = ["esp32", "unstable"] }
```

#### I2S Master API (`esp_hal::i2s::master::*`)
* **Constructor**:
  ```rust
  use esp_hal::i2s::master::{Channels, DataFormat, I2s, Rate, TdmConfig};

  let config = TdmConfig::new_tdm_philips()
      .with_sample_rate(Rate::from_hz(44100))
      .with_data_format(DataFormat::Data16Channel16)
      .with_channels(Channels::STEREO);

  let i2s = I2s::new(
      peripherals.I2S0,
      peripherals.DMA_I2S0,
      config,
  )?;
  ```
  *(Note: ESP32 uses `i2s_version = "1"`; `BuilderLite` on `TdmConfig` generates `with_sample_rate` and `with_data_format` directly).*

* **Pin Binding & Transmitter Construction (`TxCreator`)**:
  ```rust
  let mut tx = i2s.i2s_tx
      .with_bclk(peripherals.GPIO26)  // BCK
      .with_ws(peripherals.GPIO25)    // WS / LRCK
      .with_dout(peripherals.GPIO22)  // DOUT / DATA
      .build();
  ```
  If MCLK is needed on ESP32, `i2s.with_mclk(pin)` requires `pin: impl ClkPin<'d>` (supported on `GPIO0`, `GPIO1`, `GPIO3`).

* **DMA Buffering & Transmission**:
  * Macro: `esp_hal::dma_tx_buffer!(SIZE)` allocates statically linked DMA descriptors and buffer as `DmaTxBuf`.
  * Transmission:
    ```rust
    // Initiate non-blocking DMA transfer
    let transfer = tx.write(dma_buf).map_err(|(e, _tx, _buf)| e)?;

    // Blocking wait until finished (or poll with transfer.is_done()):
    let (res, tx_back, mut buf_back) = transfer.wait();
    res?;
    tx = tx_back;
    ```

#### System Reset API
* Function: `esp_hal::system::software_reset() -> !`
  * Fully stable, public function in `esp_hal::system`.
  * Triggers immediate ROM software reset: `crate::rom::software_reset()`.

---

### 2. `esp-storage 0.10.0` Flash Read/Write Audit

* **Constructor & Hardware Instance**:
  ```rust
  use esp_storage::FlashStorage;
  let mut flash = FlashStorage::new(peripherals.FLASH);
  ```
* **Multi-Core Safety on ESP32**:
  By default, `FlashStorage` checks whether `Cpu::AppCpu` is active. For bare-metal applications running single-core on Core 0, `is_running(Cpu::AppCpu)` returns `false` and writes proceed without errors. For automatic core parking during flash writes, chain:
  ```rust
  let mut flash = FlashStorage::new(peripherals.FLASH).multicore_auto_park();
  ```
* **Read / Write APIs**:
  * Inherent methods (no trait import required):
    * `flash.read(offset: u32, bytes: &mut [u8]) -> Result<(), FlashStorageError>`
    * `flash.write(offset: u32, bytes: &[u8]) -> Result<(), FlashStorageError>` (handles read-modify-write and auto-erases sectors).
    * `flash.read_nor(offset: u32, bytes: &mut [u8])` / `flash.write_nor(...)` / `flash.erase(from: u32, to: u32)` for direct NOR operations.
  * Trait integration: with `features = ["esp32", "embedded-storage"]`, implements `embedded_storage::ReadStorage`, `Storage`, and `embedded_storage::nor_flash::NorFlash`.
* **Flash Geometry & Offsets**:
  * Flash sector size is `4096` bytes (`FlashStorage::SECTOR_SIZE`), word size is `4` bytes.
  * Capable of reading and writing arbitrary flash partitions:
    * `0x9000` (NVS partition, 16 KB)
    * `0xD000` (`otadata` partition, 8 KB)
    * `0x10000` (`recovery` factory app partition, ~1.31 MB)
    * `0x160000` (`ota_0` app partition, ~2.56 MB)
    * `0x3F0000` (`settings` NVS partition, 64 KB)

---

### 3. `esp-bootloader-esp-idf 0.6.0` Partition Table & OTA Rollback Audit

* **Partition Table Reader**:
  ```rust
  use esp_bootloader_esp_idf::partitions::{read_partition_table, PartitionType, DataPartitionSubType, AppPartitionSubType};
  let mut pt_buffer = [0u8; 3072];
  let pt = read_partition_table(&mut flash, &mut pt_buffer)?;
  ```
* **OTA Partition Switching to Recovery (`factory` at `0x10000`)**:
  ```rust
  use esp_bootloader_esp_idf::ota::Ota;

  let ota_partition = pt.find_partition(PartitionType::Data(DataPartitionSubType::Ota))?
      .expect("Missing otadata partition");
  let ota_count = pt.find_partitions(PartitionType::App(AppPartitionSubType::AnyOta)).count();

  let mut ota = Ota::new(ota_partition.as_flash_region(&mut flash)?, ota_count)?;
  // Resetting active app to Factory clears otadata slots 0 and 1 to 0xFF:
  ota.set_current_app_partition(AppPartitionSubType::Factory)?;

  // Boot into factory / recovery immediately:
  esp_hal::system::software_reset();
  ```
* **Low-Level Alternative (Zero-Dependency Rollback)**:
  Directly erasing or writing `0xFF` to the two 4096-byte sectors of `otadata` at `0xD000` and `0xE000` via `FlashStorage::erase(0xD000, 0xF000)` causes the ESP-IDF 2nd-stage bootloader to fall back to the factory recovery partition (`0x10000`) on next boot.

---

### 4. Pure-Rust Wi-Fi + Networking Ecosystem Audit

#### Crate Renaming & Version Compatibility
* **`esp-wifi` vs `esp-radio`**:
  * `esp-wifi = 0.15.1` depends on legacy `esp-hal = "1.0.0-rc.0"`.
  * **Starting with `esp-hal 1.2.0`, `esp-wifi` was superseded and renamed to `esp-radio`!**
  * `esp-radio = "1.0.0-beta.1"` has dependency `esp-hal = "~1.2.0"`, which matches `esp-hal = "1.2.2"` exactly.
* **Heap Allocator**:
  * `esp-alloc = "0.11.0"` matches `esp-hal = "1.2.2"` and `esp-radio = "1.0.0-beta.1"`.
  * Heap setup: `esp_alloc::heap_allocator!(size: 64 * 1024)`.
* **Network Stack Transition (`smoltcp` -> `embassy-net`)**:
  * In `esp-radio 1.0.0-beta.1`, the `smoltcp` feature was removed (`MIGRATING-0.17.0.md:122`).
  * The official networking stack is `embassy-net` (`embassy-net-driver = "0.2.0"`).
  * `esp_radio::wifi::Interface::station()` implements `embassy_net_driver::Driver`.
* **Unsafe Code Policy (`rust-pro` Alignment)**:
  * Both `esp_alloc::heap_allocator!` and `esp_radio::wifi::WifiController::new` internally execute `unsafe` blocks to register memory regions and interface with the proprietary Espressif Wi-Fi PHY blob.
  * In `louder-esp32-app`, `#![forbid(unsafe_code)]` must be relaxed to `#![deny(unsafe_code)]` with explicit `// SAFETY:` contracts, or encapsulated within a dedicated `hardware` adapter module.

---

### 5. NVS / Wi-Fi Credentials on `172.22.1.15`

* **ESP-IDF NVS Partition Structure (`components/wifi-manager/network_wifi.c`)**:
  * Squeezelite-esp32 stores STA Wi-Fi credentials in the `nvs` partition (`0x9000`..`0xD000`) under namespace `"config"`:
    * Key `"ssid"`: stored as a raw byte blob (`NVS_TYPE_BLOB`).
    * Key `"password"`: stored as a byte blob (`NVS_TYPE_BLOB`).
    * **Encryption Header**: If encrypted via `encrypt_wifi_credentials()`, the password begins with `"enc:v1:"` followed by hex-encoded XOR ciphertext keyed by `SHA256(salt + MAC)`. If unencrypted, it is plaintext ASCII.
  * Squeezelite-esp32 also registers the `settings` NVS partition at `0x3F0000`..`0x400000` (64 KB).
* **Credential Acquisition Strategy**:
  1. **Dual-Tier Resolution**:
     * Tier 1: Check compile-time `option_env!("SL_WIFI_SSID")` and `option_env!("SL_WIFI_PASS")`.
     * Tier 2: Scan `0x9000` (NVS) for namespace `"config"` and key `"ssid"` / `"password"`. If unencrypted (no `"enc:v1:"` prefix), use directly.
  2. **Dedicated Settings Sector**: Alternatively, write a raw struct `[u8; 128]` to `0x3F0000` (`settings`) containing magic bytes, SSID, and password for zero-parsing runtime retrieval.

---

### Summary Checklist of Crates for `louder-esp32-app`
| Crate | Version | Key Features | Purpose |
|---|---|---|---|
| `esp-hal` | `1.2.2` | `["esp32", "unstable"]` | I2S master, DMA, GPIO, software reset |
| `esp-storage` | `0.10.0` | `["esp32", "embedded-storage"]` | Raw flash read/write at `0x9000`, `0xD000`, `0x3F0000` |
| `esp-bootloader-esp-idf` | `0.6.0` | `["esp32"]` | Partition table parsing, OTA rollback to `factory` |
| `esp-alloc` | `0.11.0` | `["esp32"]` | Global heap allocator for Wi-Fi buffers |
| `esp-radio` | `1.0.0-beta.1` | `["esp32", "wifi", "unstable"]` | Hardware Wi-Fi controller (`esp-wifi` successor) |
| `embassy-net` | `0.7.0` / `0.9.1` | `["tcp", "dhcpv4"]` | Pure-Rust network stack pairing with `esp-radio` |

