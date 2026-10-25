//! Hardware Abstraction Layer (HAL) for Louder-ESP32 / Louder-ESP32-Plus
//!
//! Provides pure-Rust hardware peripheral bridges and drivers:
//! - I2S DMA Audio Output (`I2sDmaDriver`, `EspI2sSink`, `LoopbackI2sDma`)
//! - CYD UART Front-Panel Bridge (`UartPort`, `CydUartBridge`, `LoopbackUartPort`)
//! - Dual-Sector Raw Flash NVS Backend (`FlashPartition`, `NvsFlashBackend`, `MemFlashPartition`)
//! - Hardware/Target I2C Bus Adapter (`TargetI2cBus`, `TargetI2cError`)

use embedded_hal::i2c::{Error as I2cError, ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation};
use squeezelite_audio_core::output::{AudioOutputSink, OutputError};
use squeezelite_config::{FlashJournal, MemConfigStore, StorageError};
use squeezelite_cyd_link::{CydCommand, CydMessage, LinkError};
use squeezelite_tas58xx::registers;

// ============================================================================
// A. I2S DMA Audio Output Driver & Sink
// ============================================================================

/// I2S audio interface bus format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2sFormat {
    I2s,
    LeftJustified,
    Dsp,
}

/// Bit depth for audio samples and I2S bus formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    Bits16 = 16,
    Bits20 = 20,
    Bits24 = 24,
    Bits32 = 32,
}

impl BitDepth {
    pub const fn bits(self) -> u8 {
        self as u8
    }
}

/// Low-level hardware I2S DMA driver abstraction.
pub trait I2sDmaDriver {
    /// Writes raw little-endian 32-bit stereo I2S DMA bytes.
    ///
    /// Must return the number of bytes written, which must be a multiple of 8.
    fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError>;

    /// Sets the hardware clock rate for the I2S master clock and bit clocks.
    fn set_clock_rate(&mut self, sample_rate_hz: u32) -> Result<(), OutputError>;

    /// Enables or disables the I2S DMA transmitter.
    fn set_tx_enabled(&mut self, enabled: bool);
}

/// Hardware-register-accurate loopback DMA driver for host simulation and verification.
#[derive(Debug, Clone)]
pub struct LoopbackI2sDma {
    pub clock_hz: u32,
    pub tx_enabled: bool,
    pub dma_bytes_written: usize,
    pub last_frame_samples: [i32; 2],
}

impl LoopbackI2sDma {
    pub const fn new(sample_rate_hz: u32) -> Self {
        Self {
            clock_hz: sample_rate_hz,
            tx_enabled: true,
            dma_bytes_written: 0,
            last_frame_samples: [0, 0],
        }
    }
}

impl I2sDmaDriver for LoopbackI2sDma {
    fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError> {
        let valid_len = data.len() - (data.len() % 8);
        if valid_len >= 8 {
            let l_bytes: [u8; 4] = data[valid_len - 8..valid_len - 4]
                .try_into()
                .map_err(|_| OutputError::HardwareFault)?;
            let r_bytes: [u8; 4] = data[valid_len - 4..valid_len]
                .try_into()
                .map_err(|_| OutputError::HardwareFault)?;
            self.last_frame_samples = [i32::from_le_bytes(l_bytes), i32::from_le_bytes(r_bytes)];
        }
        self.dma_bytes_written += valid_len;
        Ok(valid_len)
    }

    fn set_clock_rate(&mut self, sample_rate_hz: u32) -> Result<(), OutputError> {
        if !(8_000..=192_000).contains(&sample_rate_hz) {
            return Err(OutputError::UnsupportedSampleRate(sample_rate_hz));
        }
        self.clock_hz = sample_rate_hz;
        Ok(())
    }

    fn set_tx_enabled(&mut self, enabled: bool) {
        self.tx_enabled = enabled;
    }
}

/// ESP32 I2S DMA Audio Sink implementing `AudioOutputSink`.
pub struct EspI2sSink<D: I2sDmaDriver> {
    pub driver: D,
    sample_rate: u32,
    muted: bool,
    frames_written: usize,
    bclk_pin: u8,
    lrclk_pin: u8,
    dout_pin: u8,
    dma_scratch: [u8; 1024], // 128 stereo frames = 1024 bytes (8 bytes/frame)
}

impl<D: I2sDmaDriver> EspI2sSink<D> {
    /// Creates a sink configured for the active board target pinout.
    pub fn for_active_board(driver: D, sample_rate: u32) -> Self {
        Self::new(
            driver,
            sample_rate,
            crate::board::pins::I2S_BCLK as u8,
            crate::board::pins::I2S_LRCLK as u8,
            crate::board::pins::I2S_DOUT as u8,
        )
    }

    /// Creates a sink with explicit pin assignments.
    pub fn new(driver: D, sample_rate: u32, bclk_pin: u8, lrclk_pin: u8, dout_pin: u8) -> Self {
        Self {
            driver,
            sample_rate,
            muted: false,
            frames_written: 0,
            bclk_pin,
            lrclk_pin,
            dout_pin,
            dma_scratch: [0u8; 1024],
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn frames_written(&self) -> usize {
        self.frames_written
    }

    pub fn bclk_pin(&self) -> u8 {
        self.bclk_pin
    }

    pub fn lrclk_pin(&self) -> u8 {
        self.lrclk_pin
    }

    pub fn dout_pin(&self) -> u8 {
        self.dout_pin
    }
}

impl<D: I2sDmaDriver> AudioOutputSink for EspI2sSink<D> {
    type Error = OutputError;

    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let mut total_frames_written = 0;

        // Process in chunks of up to 128 stereo frames (256 i32 samples = 1024 bytes)
        for chunk in frames.chunks(256) {
            let mut offset = 0;
            for pair in chunk.chunks_exact(2) {
                let (l, r) = if self.muted {
                    (0i32, 0i32)
                } else {
                    (pair[0], pair[1])
                };
                self.dma_scratch[offset..offset + 4].copy_from_slice(&l.to_le_bytes());
                self.dma_scratch[offset + 4..offset + 8].copy_from_slice(&r.to_le_bytes());
                offset += 8;
            }

            let chunk_bytes = offset;
            if chunk_bytes == 0 {
                break;
            }

            let mut written_bytes = self.driver.write_dma(&self.dma_scratch[..chunk_bytes])?;
            if written_bytes > chunk_bytes {
                written_bytes = chunk_bytes;
            }

            // HALNET-402: An I2S 32-bit stereo frame is 8 bytes (4 bytes Left + 4 bytes Right).
            // If the DMA driver returned a partial frame (written_bytes % 8 != 0), loop to write
            // the remaining 8 - (written_bytes % 8) bytes of that stereo frame so Left/Right
            // channels never split or swap!
            if written_bytes % 8 != 0 {
                let aligned_end = written_bytes + (8 - (written_bytes % 8));
                let mut retries = 0u8;
                while written_bytes < aligned_end {
                    let w = self
                        .driver
                        .write_dma(&self.dma_scratch[written_bytes..aligned_end])?;
                    if w == 0 {
                        retries += 1;
                        if retries >= 64 {
                            return Err(OutputError::HardwareFault);
                        }
                        continue;
                    }
                    retries = 0;
                    written_bytes = (written_bytes + w).min(aligned_end);
                }
            }

            let frames_from_chunk = written_bytes / 8;
            total_frames_written += frames_from_chunk;
            self.frames_written += frames_from_chunk;

            if written_bytes < chunk_bytes {
                break;
            }
        }

        Ok(total_frames_written)
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Self::Error> {
        if !(8_000..=192_000).contains(&rate) {
            return Err(OutputError::UnsupportedSampleRate(rate));
        }
        self.driver.set_clock_rate(rate)?;
        self.sample_rate = rate;
        Ok(())
    }

    fn set_mute(&mut self, muted: bool) {
        self.muted = muted;
        self.driver.set_tx_enabled(!muted);
    }
}

// ============================================================================
// B. CYD UART Hardware Bridge
// ============================================================================

/// Hardware UART communication port trait.
pub trait UartPort {
    /// Reads up to `buf.len()` bytes from the hardware RX FIFO. Returns bytes read.
    fn read_rx(&mut self, buf: &mut [u8]) -> usize;

    /// Writes `data` bytes to the hardware TX FIFO. Returns bytes written.
    fn write_tx(&mut self, data: &[u8]) -> usize;
}

/// Loopback UART port with bounded buffers for deterministic host testing.
#[derive(Debug, Clone, Default)]
pub struct LoopbackUartPort {
    pub rx_fifo: heapless::Deque<u8, 256>,
    pub tx_fifo: heapless::Vec<u8, 256>,
}

impl LoopbackUartPort {
    pub fn new() -> Self {
        Self {
            rx_fifo: heapless::Deque::new(),
            tx_fifo: heapless::Vec::new(),
        }
    }

    /// Simulates incoming data on the RX line.
    pub fn push_rx(&mut self, data: &[u8]) -> usize {
        let mut pushed = 0;
        for &b in data {
            if self.rx_fifo.push_back(b).is_ok() {
                pushed += 1;
            } else {
                break;
            }
        }
        pushed
    }

    /// Extracts transmitted bytes from the TX buffer.
    pub fn take_tx(&mut self) -> heapless::Vec<u8, 256> {
        let out = self.tx_fifo.clone();
        self.tx_fifo.clear();
        out
    }

    pub fn tx_bytes(&self) -> &[u8] {
        &self.tx_fifo
    }

    pub fn clear_tx(&mut self) {
        self.tx_fifo.clear();
    }
}

impl UartPort for LoopbackUartPort {
    fn read_rx(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        for byte in buf.iter_mut() {
            if let Some(b) = self.rx_fifo.pop_front() {
                *byte = b;
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    fn write_tx(&mut self, data: &[u8]) -> usize {
        let mut count = 0;
        for &b in data {
            if self.tx_fifo.push(b).is_ok() {
                count += 1;
            } else {
                break;
            }
        }
        count
    }
}

/// Bidirectional bridge between a physical UART port and `CydController`.
pub struct CydUartBridge<U: UartPort> {
    pub port: U,
    pub controller: crate::cyd::CydController,
}

impl<U: UartPort> CydUartBridge<U> {
    pub fn new(port: U) -> Self {
        Self {
            port,
            controller: crate::cyd::CydController::new(),
        }
    }

    /// Polls for incoming touch commands from the display over UART.
    pub fn poll_command(&mut self) -> Result<Option<CydCommand>, LinkError> {
        if let Some(cmd) = self.controller.pop_command() {
            return Ok(Some(cmd));
        }

        let mut rx_buf = [0u8; 64];
        let n = self.port.read_rx(&mut rx_buf);
        if n == 0 {
            return Ok(None);
        }

        self.controller.feed_bytes(&rx_buf[..n])
    }

    /// Encodes and transmits a telemetry message to the display over UART,
    /// looping over partial TX writes (`HALNET-407`).
    pub fn send_message(&mut self, msg: &CydMessage) -> Result<usize, LinkError> {
        let buf = self.controller.format_message(msg)?;
        let len = buf.len();
        let mut offset = 0;
        while offset < len {
            let w = self.port.write_tx(&buf[offset..len]);
            if w == 0 {
                break;
            }
            offset += w;
        }
        Ok(offset)
    }

    /// Flushes an outbound telemetry message to the UART TX port across partial writes (`HALNET-407`).
    pub fn flush_outbound(&mut self, msg: &CydMessage) -> Result<usize, LinkError> {
        self.send_message(msg)
    }
}

// ============================================================================
// C. Dual-Sector Raw Flash NVS Backend
// ============================================================================

/// Standard 4 KB flash sector size (`HALNET-408`).
pub const FLASH_SECTOR_SIZE: usize = 4096;

/// Trait abstracting raw 4KB flash partition read and write access.
pub trait FlashPartition {
    /// Reads a 4096-byte sector (0 for Sector A, 1 for Sector B).
    fn read_sector(&self, sector_idx: u8) -> Result<&[u8; FLASH_SECTOR_SIZE], StorageError>;

    /// Writes a 4096-byte sector (0 for Sector A, 1 for Sector B).
    fn write_sector(&mut self, sector_idx: u8, data: &[u8; FLASH_SECTOR_SIZE]) -> Result<(), StorageError>;

    /// Erases a 4096-byte sector (0 for Sector A, 1 for Sector B), setting all bytes to 0xFF.
    fn erase_sector(&mut self, sector_idx: u8) -> Result<(), StorageError> {
        self.write_sector(sector_idx, &[0xFF; FLASH_SECTOR_SIZE])
    }
}

/// In-memory dual-sector flash partition for simulation and verification.
#[derive(Debug, Clone)]
pub struct MemFlashPartition {
    pub sector_a: [u8; FLASH_SECTOR_SIZE],
    pub sector_b: [u8; FLASH_SECTOR_SIZE],
}

impl Default for MemFlashPartition {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFlashPartition {
    pub const fn new() -> Self {
        Self {
            sector_a: [0u8; FLASH_SECTOR_SIZE],
            sector_b: [0u8; FLASH_SECTOR_SIZE],
        }
    }
}

impl FlashPartition for MemFlashPartition {
    fn read_sector(&self, sector_idx: u8) -> Result<&[u8; FLASH_SECTOR_SIZE], StorageError> {
        match sector_idx {
            0 => Ok(&self.sector_a),
            1 => Ok(&self.sector_b),
            _ => Err(StorageError::CapacityExceeded),
        }
    }

    fn write_sector(&mut self, sector_idx: u8, data: &[u8; FLASH_SECTOR_SIZE]) -> Result<(), StorageError> {
        match sector_idx {
            0 => {
                self.sector_a.copy_from_slice(data);
                Ok(())
            }
            1 => {
                self.sector_b.copy_from_slice(data);
                Ok(())
            }
            _ => Err(StorageError::CapacityExceeded),
        }
    }

    fn erase_sector(&mut self, sector_idx: u8) -> Result<(), StorageError> {
        match sector_idx {
            0 => {
                self.sector_a.fill(0xFF);
                Ok(())
            }
            1 => {
                self.sector_b.fill(0xFF);
                Ok(())
            }
            _ => Err(StorageError::CapacityExceeded),
        }
    }
}

/// Dual-sector non-volatile storage flash journal backend (`HALNET-408`).
pub struct NvsFlashBackend<F: FlashPartition> {
    flash: F,
    last_seq: u32,
    scratch: [u8; FLASH_SECTOR_SIZE],
}

impl<F: FlashPartition> NvsFlashBackend<F> {
    pub const fn new(flash: F) -> Self {
        Self {
            flash,
            last_seq: 0,
            scratch: [0u8; FLASH_SECTOR_SIZE],
        }
    }

    pub fn last_seq(&self) -> u32 {
        self.last_seq
    }

    pub fn flash(&self) -> &F {
        &self.flash
    }

    pub fn flash_mut(&mut self) -> &mut F {
        &mut self.flash
    }

    pub fn into_flash(self) -> F {
        self.flash
    }

    /// Loads key-value configuration from dual flash sectors into `store`.
    pub fn load_store<const N: usize>(&mut self, store: &mut MemConfigStore<N>) -> Result<usize, StorageError> {
        let a = self.flash.read_sector(0)?;
        let b = self.flash.read_sector(1)?;

        let count = FlashJournal::load_from_dual_sectors(a, b, store)?;

        let meta_a = FlashJournal::read_sector_meta(a);
        let meta_b = FlashJournal::read_sector_meta(b);
        self.last_seq = match (meta_a, meta_b) {
            (Ok((seq_a, _, _)), Ok((seq_b, _, _))) => {
                if FlashJournal::is_seq_newer_or_equal(seq_a, seq_b) {
                    seq_a
                } else {
                    seq_b
                }
            }
            (Ok((seq_a, _, _)), Err(_)) => seq_a,
            (Err(_), Ok((seq_b, _, _))) => seq_b,
            (Err(_), Err(_)) => 0,
        };

        Ok(count)
    }

    /// Commits the in-memory store atomically to the alternate flash sector with incremented sequence.
    ///
    /// Uses `self.scratch` to avoid allocating two 4KB sector buffers (8KB) on the call stack (`HALNET-408`).
    pub fn commit_store<const N: usize>(&mut self, store: &MemConfigStore<N>) -> Result<u32, StorageError> {
        self.scratch.copy_from_slice(self.flash.read_sector(0)?);
        let mut b = *self.flash.read_sector(1)?;

        let (wrote_to_b, _bytes_written) =
            FlashJournal::save_to_dual_sectors(store, &mut self.scratch, &mut b)?;

        let new_seq = if wrote_to_b {
            self.flash.write_sector(1, &b)?;
            let (seq, _, _) = FlashJournal::read_sector_meta(&b)?;
            seq
        } else {
            self.flash.write_sector(0, &self.scratch)?;
            let (seq, _, _) = FlashJournal::read_sector_meta(&self.scratch)?;
            seq
        };

        self.last_seq = new_seq;
        Ok(new_seq)
    }

    /// Erases all sectors (0 and 1) in the flash partition and resets journal state (`HALNET-408`).
    pub fn erase_all(&mut self) -> Result<(), StorageError> {
        self.flash.erase_sector(0)?;
        self.flash.erase_sector(1)?;
        self.last_seq = 0;
        self.scratch.fill(0xFF);
        Ok(())
    }

    /// Erases a specific sector (0 or 1) in the flash partition.
    pub fn erase_sector(&mut self, sector_idx: u8) -> Result<(), StorageError> {
        self.flash.erase_sector(sector_idx)
    }

    /// Erases flash partition sectors spanning byte range `start..end`.
    pub fn erase_range(&mut self, range: core::ops::Range<usize>) -> Result<(), StorageError> {
        if range.start < FLASH_SECTOR_SIZE {
            self.flash.erase_sector(0)?;
        }
        if range.end > FLASH_SECTOR_SIZE {
            self.flash.erase_sector(1)?;
        }
        if range.start == 0 && range.end >= FLASH_SECTOR_SIZE * 2 {
            self.last_seq = 0;
            self.scratch.fill(0xFF);
        }
        Ok(())
    }
}

// ============================================================================
// D. Hardware/Target I2C Bus Adapter
// ============================================================================

/// Error type for target I2C bus communications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetI2cError {
    AddressNack,
    InvalidRegister,
    BusError,
}

impl core::fmt::Display for TargetI2cError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AddressNack => write!(f, "I2C Address NACK"),
            Self::InvalidRegister => write!(f, "Invalid I2C register"),
            Self::BusError => write!(f, "I2C Bus Error"),
        }
    }
}

impl I2cError for TargetI2cError {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::AddressNack => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
            Self::InvalidRegister => ErrorKind::Other,
            Self::BusError => ErrorKind::Bus,
        }
    }
}

/// Hardware/target I2C bus adapter for TAS5805M/TAS5825M digital amplifier control.
#[derive(Debug, Clone)]
pub struct TargetI2cBus {
    pub sda_pin: u8,
    pub scl_pin: u8,
    pub registers: [u8; 256],
    current_reg: u8,
}

impl Default for TargetI2cBus {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetI2cBus {
    /// Creates a target I2C bus using active board pins.
    pub fn new() -> Self {
        Self::for_active_board()
    }

    /// Creates a target I2C bus with pin mappings from active board configuration.
    pub fn for_active_board() -> Self {
        Self::with_pins(
            crate::board::pins::I2C_SDA as u8,
            crate::board::pins::I2C_SCL as u8,
        )
    }

    /// Creates a target I2C bus with explicit pin assignments and default TAS5805M telemetry.
    pub fn with_pins(sda_pin: u8, scl_pin: u8) -> Self {
        let mut registers = [0u8; 256];
        registers[0x37] = 0x08; // 44.1 kHz detected FS_MON
        registers[0x38] = 64;   // 64 FS BCK_MON
        registers[0x39] = 0x08; // PLL locked CLKDET_STATUS
        registers[0x54] = 0x08; // -4.0 dB AGAIN
        Self {
            sda_pin,
            scl_pin,
            registers,
            current_reg: 0,
        }
    }
}

impl ErrorType for TargetI2cBus {
    type Error = TargetI2cError;
}

impl I2c for TargetI2cBus {
    fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
        for byte in read.iter_mut() {
            *byte = self.registers[self.current_reg as usize];
            self.current_reg = self.current_reg.wrapping_add(1);
        }
        Ok(())
    }

    fn write(&mut self, _address: u8, write: &[u8]) -> Result<(), Self::Error> {
        if write.is_empty() {
            return Ok(());
        }

        self.current_reg = write[0];
        let mut reg = write[0] as usize;
        for &val in &write[1..] {
            if reg < 256 {
                self.registers[reg] = val;
                reg += 1;
            }
        }

        // Special hardware simulation: FAULT_CLEAR register (0x78) bit 7 clears faults (0x70..=0x73)
        if write.len() > 1 && write[0] == registers::FAULT_CLEAR && (write[1] & 0x80) != 0 {
            self.registers[registers::CHAN_FAULT as usize] = 0;
            self.registers[registers::GLOBAL_FAULT1 as usize] = 0;
            self.registers[registers::GLOBAL_FAULT2 as usize] = 0;
            self.registers[registers::OTW as usize] = 0;
        }

        Ok(())
    }

    fn write_read(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error> {
        self.write(address, write)?;
        self.read(address, read)?;
        Ok(())
    }

    fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Read(buf) => self.read(address, buf)?,
                Operation::Write(buf) => self.write(address, buf)?,
            }
        }
        Ok(())
    }
}

// ============================================================================
// E. Wi-Fi Hardware Driver & Adapter (`HALNET-411`)
// ============================================================================

/// Low-level hardware Wi-Fi driver abstraction.
pub trait WifiDriver {
    /// Initiates a station connection to the given SSID and passphrase.
    fn start_connect(&mut self, ssid: &str, passphrase: &str) -> Result<(), &'static str>;

    /// Returns `Some(ipv4_octets)` when the Wi-Fi station link is associated and has an IPv4 lease.
    fn is_connected(&self) -> Option<[u8; 4]>;

    /// Disconnects the Wi-Fi station interface.
    fn disconnect(&mut self);
}

/// Deterministic loopback Wi-Fi driver for host simulation and testing.
#[derive(Debug, Clone, Default)]
pub struct LoopbackWifiDriver {
    pub connected_ip: Option<[u8; 4]>,
    pub last_ssid: heapless::String<32>,
}

impl LoopbackWifiDriver {
    pub const fn new() -> Self {
        Self {
            connected_ip: None,
            last_ssid: heapless::String::new(),
        }
    }
}

impl WifiDriver for LoopbackWifiDriver {
    fn start_connect(&mut self, ssid: &str, _passphrase: &str) -> Result<(), &'static str> {
        self.last_ssid.clear();
        let _ = self.last_ssid.push_str(ssid);
        Ok(())
    }

    fn is_connected(&self) -> Option<[u8; 4]> {
        self.connected_ip
    }

    fn disconnect(&mut self) {
        self.connected_ip = None;
    }
}

/// Bridges `WifiManager` state machine with a hardware `WifiDriver` (`HALNET-411`).
pub struct EspWifiAdapter<D: WifiDriver, S = MemConfigStore<32>> {
    pub driver: D,
    pub manager: crate::wifi::WifiManager<S>,
    pub ip_addr: Option<[u8; 4]>,
}

impl<D: WifiDriver, S: squeezelite_config::KeyValueStorage> EspWifiAdapter<D, S> {
    pub fn new(driver: D, manager: crate::wifi::WifiManager<S>) -> Self {
        Self {
            driver,
            manager,
            ip_addr: None,
        }
    }

    pub fn ip_addr(&self) -> Option<[u8; 4]> {
        self.ip_addr
    }

    /// Polls the hardware Wi-Fi link status and advances the `WifiManager` state machine (`HALNET-411`).
    pub fn poll_connection(&mut self) -> crate::wifi::WifiState {
        if self.manager.state() == crate::wifi::WifiState::Connecting {
            if let Some(ip) = self.driver.is_connected() {
                self.ip_addr = Some(ip);
                self.manager.on_connected();
            }
        } else if self.manager.state() == crate::wifi::WifiState::Connected
            && self.driver.is_connected().is_none()
        {
            self.ip_addr = None;
            self.manager.on_disconnected();
        }
        self.manager.state()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amp::{AmpManager, AmpMode};
    use squeezelite_config::{AudioConfig, WifiCredentials};

    struct PartialDmaDriver {
        step_sizes: Vec<usize>,
        call_idx: usize,
        written: Vec<u8>,
    }

    impl PartialDmaDriver {
        fn new(step_sizes: Vec<usize>) -> Self {
            Self {
                step_sizes,
                call_idx: 0,
                written: Vec::new(),
            }
        }
    }

    impl I2sDmaDriver for PartialDmaDriver {
        fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError> {
            let limit = self
                .step_sizes
                .get(self.call_idx)
                .copied()
                .unwrap_or(data.len());
            self.call_idx += 1;
            let n = data.len().min(limit);
            self.written.extend_from_slice(&data[..n]);
            Ok(n)
        }

        fn set_clock_rate(&mut self, _sample_rate_hz: u32) -> Result<(), OutputError> {
            Ok(())
        }

        fn set_tx_enabled(&mut self, _enabled: bool) {}
    }

    struct PartialTxUartPort {
        max_bytes_per_write: usize,
        tx_log: Vec<u8>,
    }

    impl PartialTxUartPort {
        fn new(max_bytes_per_write: usize) -> Self {
            Self {
                max_bytes_per_write,
                tx_log: Vec::new(),
            }
        }
    }

    impl UartPort for PartialTxUartPort {
        fn read_rx(&mut self, _buf: &mut [u8]) -> usize {
            0
        }

        fn write_tx(&mut self, data: &[u8]) -> usize {
            let n = data.len().min(self.max_bytes_per_write);
            self.tx_log.extend_from_slice(&data[..n]);
            n
        }
    }

    #[test]
    fn test_esp_i2s_sink_partial_dma_frame_completion() {
        // HALNET-402: Simulate DMA driver returning 3 bytes, then 3 bytes, then 2 bytes
        // for the first 8-byte stereo frame. Verify EspI2sSink loops to complete the full
        // 8-byte frame so Left/Right channels are never split or swapped.
        let driver = PartialDmaDriver::new(vec![3, 3, 2]);
        let mut sink = EspI2sSink::for_active_board(driver, 44100);

        let frames = [0x11223344i32, 0x55667788i32, 0x10203040i32, 0x50607080i32];
        let written_frames = sink.write_frames(&frames).unwrap();
        assert_eq!(written_frames, 1);
        assert_eq!(sink.frames_written(), 1);
        assert_eq!(sink.driver.written.len(), 8);
        assert_eq!(&sink.driver.written[0..4], &0x11223344i32.to_le_bytes());
        assert_eq!(&sink.driver.written[4..8], &0x55667788i32.to_le_bytes());
    }

    #[test]
    fn test_cyd_uart_bridge_partial_tx_flush() {
        // HALNET-407: Simulate UART TX FIFO accepting only 3 bytes per write_tx call.
        let port = PartialTxUartPort::new(3);
        let mut bridge = CydUartBridge::new(port);

        let written = bridge
            .flush_outbound(&CydMessage::PlaybackState(squeezelite_cyd_link::PlayState::Playing))
            .unwrap();
        assert_eq!(written, 8);
        assert_eq!(bridge.port.tx_log.as_slice(), b"STATE=P\n");
    }

    #[test]
    fn test_esp_i2s_sink_dma_conversion_and_mute() {
        let dma = LoopbackI2sDma::new(44100);
        let mut sink = EspI2sSink::for_active_board(dma, 44100);

        assert_eq!(sink.sample_rate(), 44100);
        assert!(!sink.is_muted());
        assert_eq!(sink.frames_written(), 0);

        // 1. Write stereo frames and verify LE 32-bit DMA byte output
        let stereo_samples = [100i32, -100i32, 200i32, -200i32];
        let written = sink.write_frames(&stereo_samples).unwrap();
        assert_eq!(written, 2);
        assert_eq!(sink.frames_written(), 2);
        assert_eq!(sink.driver.dma_bytes_written, 16);
        assert_eq!(sink.driver.last_frame_samples, [200, -200]);

        // 2. Mute zeroing test
        sink.set_mute(true);
        assert!(sink.is_muted());
        assert!(!sink.driver.tx_enabled);

        let written_muted = sink.write_frames(&stereo_samples).unwrap();
        assert_eq!(written_muted, 2);
        assert_eq!(sink.frames_written(), 4);
        assert_eq!(sink.driver.last_frame_samples, [0, 0]);

        // 3. Unmute restores normal transmission
        sink.set_mute(false);
        assert!(!sink.is_muted());
        assert!(sink.driver.tx_enabled);

        let written_unmuted = sink.write_frames(&stereo_samples).unwrap();
        assert_eq!(written_unmuted, 2);
        assert_eq!(sink.driver.last_frame_samples, [200, -200]);

        // 4. Sample rate validation bounds (8000..=192000)
        assert!(sink.set_sample_rate(8000).is_ok());
        assert_eq!(sink.sample_rate(), 8000);
        assert_eq!(sink.driver.clock_hz, 8000);

        assert!(sink.set_sample_rate(192000).is_ok());
        assert_eq!(sink.sample_rate(), 192000);

        assert_eq!(
            sink.set_sample_rate(7999),
            Err(OutputError::UnsupportedSampleRate(7999))
        );
        assert_eq!(
            sink.set_sample_rate(192001),
            Err(OutputError::UnsupportedSampleRate(192001))
        );
    }

    #[test]
    fn test_cyd_uart_bridge_tx_and_rx() {
        let port = LoopbackUartPort::new();
        let mut bridge = CydUartBridge::new(port);

        // 1. Send outbound telemetry message
        let tx_len = bridge
            .send_message(&CydMessage::PlaybackState(squeezelite_cyd_link::PlayState::Playing))
            .unwrap();
        assert_eq!(tx_len, 8); // "STATE=P\n" is 8 bytes
        assert_eq!(bridge.port.tx_bytes(), b"STATE=P\n");

        // 2. Clear TX buffer and verify volume encoding
        bridge.port.clear_tx();
        bridge.send_message(&CydMessage::Volume(75)).unwrap();
        assert_eq!(bridge.port.tx_bytes(), b"VOL=75\n");

        // 3. Ingest inbound UART touch command stream
        bridge.port.push_rx(b"CMD=PLAY\nCMD=VOL:80\n");

        let cmd1 = bridge.poll_command().unwrap();
        assert_eq!(cmd1, Some(CydCommand::Play));

        let cmd2 = bridge.poll_command().unwrap();
        assert_eq!(cmd2, Some(CydCommand::SetVolume(80)));

        let cmd3 = bridge.poll_command().unwrap();
        assert_eq!(cmd3, None);
    }

    #[test]
    fn test_nvs_flash_backend_dual_sector_commit_and_reload() {
        let partition = MemFlashPartition::new();
        let mut backend = NvsFlashBackend::new(partition);
        let mut store = MemConfigStore::<16>::new();

        // 1. Populate store with AudioConfig and WifiCredentials
        let mut audio = AudioConfig::default();
        audio.player_name.clear();
        audio.player_name.push_str("LouderStudio").unwrap();
        audio.volume = 72;
        audio.save_to(&mut store).unwrap();

        let creds = WifiCredentials::new("StudioWiFi", "SuperPassphrase123").unwrap();
        creds.save_to(&mut store).unwrap();

        // 2. Commit store to dual flash partition -> Writes Sector A with sequence 1
        let seq1 = backend.commit_store(&store).unwrap();
        assert_eq!(seq1, 1);
        assert_eq!(backend.last_seq(), 1);

        // 3. Update AudioConfig and commit -> Writes Sector B with sequence 2
        audio.volume = 88;
        audio.save_to(&mut store).unwrap();
        let seq2 = backend.commit_store(&store).unwrap();
        assert_eq!(seq2, 2);
        assert_eq!(backend.last_seq(), 2);

        // 4. Simulate complete power cycle by transferring flash partition to new backend
        let flash_sim = backend.into_flash();
        let mut new_backend = NvsFlashBackend::new(flash_sim);
        let mut restored_store = MemConfigStore::<16>::new();

        let loaded_count = new_backend.load_store(&mut restored_store).unwrap();
        assert!(loaded_count >= 2);
        assert_eq!(new_backend.last_seq(), 2);

        let restored_audio = AudioConfig::load_from(&restored_store).unwrap();
        assert_eq!(restored_audio.player_name.as_str(), "LouderStudio");
        assert_eq!(restored_audio.volume, 88);

        let restored_creds = WifiCredentials::load_from(&restored_store).unwrap().unwrap();
        assert_eq!(restored_creds.ssid.as_str(), "StudioWiFi");
        assert_eq!(restored_creds.passphrase.as_str(), "SuperPassphrase123");
    }

    #[test]
    fn test_target_i2c_bus_with_amp_manager() {
        let bus = TargetI2cBus::new();
        let mut amp = AmpManager::for_active_board(bus);

        // Initial state: PowerDown
        assert_eq!(amp.mode(), AmpMode::PowerDown);

        // Initialize TAS58xx into Standby
        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        // Set volume in Standby
        amp.set_volume(0x40).unwrap();
        assert_eq!(amp.current_volume(), 0x40);

        // Start playback
        amp.start_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);

        // Check clean fault status
        let fault = amp.check_faults().unwrap();
        assert!(fault.is_some());
        assert!(fault.unwrap().is_clean());

        // Stop playback back to Standby
        amp.stop_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);
    }

    #[test]
    fn test_esp_wifi_adapter_connecting_to_connected_transition() {
        // HALNET-411: Verify EspWifiAdapter::poll_connection transitions from Connecting -> Connected
        let mut driver = LoopbackWifiDriver::new();
        let mut store = MemConfigStore::<32>::new();
        let creds = WifiCredentials::new("TestNet", "SecretPass123").unwrap();
        creds.save_to(&mut store).unwrap();
        let manager = crate::wifi::WifiManager::with_store(store);
        assert_eq!(manager.state(), crate::wifi::WifiState::Connecting);

        driver.connected_ip = Some([192, 168, 1, 42]);
        let mut adapter = EspWifiAdapter::new(driver, manager);

        let state = adapter.poll_connection();
        assert_eq!(state, crate::wifi::WifiState::Connected);
        assert_eq!(adapter.ip_addr(), Some([192, 168, 1, 42]));
    }

    #[test]
    fn test_i2s_pin_config_mclk_pin_none_for_3wire_bclk_pll() {
        // HALNET-412: Louder-ESP32 uses 3-wire BCLK PLL mode with mclk_pin = None
        let cfg = crate::board::I2sPinConfig::for_active_board();
        assert_eq!(cfg.mclk_pin, None);
        assert_eq!(cfg.bclk_pin, crate::board::pins::I2S_BCLK as u8);
        assert_eq!(cfg.lrclk_pin, crate::board::pins::I2S_LRCLK as u8);
        assert_eq!(cfg.dout_pin, crate::board::pins::I2S_DOUT as u8);
    }

    #[test]
    fn test_nvs_flash_backend_erase_all() {
        let partition = MemFlashPartition::new();
        let mut backend = NvsFlashBackend::new(partition);
        let mut store = MemConfigStore::<16>::new();
        let mut audio = AudioConfig::default();
        audio.player_name.clear();
        audio.player_name.push_str("LouderStudio").unwrap();
        audio.save_to(&mut store).unwrap();

        let seq1 = backend.commit_store(&store).unwrap();
        assert_eq!(seq1, 1);
        assert_eq!(backend.last_seq(), 1);

        backend.erase_all().unwrap();
        assert_eq!(backend.last_seq(), 0);
        assert_eq!(backend.flash().read_sector(0).unwrap(), &[0xFF; FLASH_SECTOR_SIZE]);
        assert_eq!(backend.flash().read_sector(1).unwrap(), &[0xFF; FLASH_SECTOR_SIZE]);

        backend.erase_range(0..4096).unwrap();
        assert_eq!(backend.flash().read_sector(0).unwrap(), &[0xFF; FLASH_SECTOR_SIZE]);
    }
}



