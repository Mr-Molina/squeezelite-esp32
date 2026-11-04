//! Pure-Rust `no_std` WIZnet W5500 SPI MACRAW (`Socket 0`) Ethernet Driver.
//!
//! Implements the Phase 3 W5500 SPI Ethernet driver (`W5500Driver`) over
//! `embedded_hal::spi::SpiDevice`:
//! - SPI Variable Data Length Mode (`VDM = 0b00`) 3-byte control-phase frame
//!   header builder (`[Offset_Hi, Offset_Lo, Control_Byte]`).
//! - Common Register (`BSB = 0x00`) MAC (`SHAR`), IP (`SIPR`), Subnet (`SUBR`),
//!   Gateway (`GAR`), Chip Version (`VERSIONR = 0x04`), and `PHYCFGR`
//!   link/speed/duplex status polling.
//! - Socket 0 (`BSB = 0x01`) `MACRAW` mode (`0x04`) initialization with 16 KB
//!   RX/TX internal ring buffers and raw Ethernet frame TX/RX for `smoltcp`.

use embedded_hal::spi::{Operation, SpiDevice};

/// Maximum standard IEEE 802.3 Ethernet frame length (excluding FCS).
pub const MAX_ETHERNET_FRAME_BYTES: usize = 1518;

/// Expected WIZnet W5500 hardware chip version (`VERSIONR` at `0x0039`).
pub const W5500_CHIP_VERSION: u8 = 0x04;

/// Block Select Bits (`BSB[4:0]`) for Common Register block.
pub const BSB_COMMON_REG: u8 = 0b00000;
/// Block Select Bits (`BSB[4:0]`) for Socket 0 Register block.
pub const BSB_S0_REG: u8 = 0b00001;
/// Block Select Bits (`BSB[4:0]`) for Socket 0 16 KB TX Buffer block.
pub const BSB_S0_TX_BUF: u8 = 0b00010;
/// Block Select Bits (`BSB[4:0]`) for Socket 0 16 KB RX Buffer block.
pub const BSB_S0_RX_BUF: u8 = 0b00011;

/// Common Register: Mode Register (`MR`, soft reset bit 7 = `0x80`).
pub const REG_MR: u16 = 0x0000;
/// Common Register: Gateway IPv4 Address (`GAR`, 4 bytes `0x0001..0x0004`).
pub const REG_GAR: u16 = 0x0001;
/// Common Register: Subnet Mask (`SUBR`, 4 bytes `0x0005..0x0008`).
pub const REG_SUBR: u16 = 0x0005;
/// Common Register: Source MAC Address (`SHAR`, 6 bytes `0x0009..0x000E`).
pub const REG_SHAR: u16 = 0x0009;
/// Common Register: Source IPv4 Address (`SIPR`, 4 bytes `0x000F..0x0012`).
pub const REG_SIPR: u16 = 0x000F;
/// Common Register: PHY Configuration Register (`PHYCFGR`, `0x002E`).
pub const REG_PHYCFGR: u16 = 0x002E;
/// Common Register: Chip Version Register (`VERSIONR`, `0x0039`).
pub const REG_VERSIONR: u16 = 0x0039;

/// Socket 0 Register: Mode Register (`Sn_MR`).
pub const S0_MR: u16 = 0x0000;
/// Socket 0 Register: Command Register (`Sn_CR`).
pub const S0_CR: u16 = 0x0001;
/// Socket 0 Register: Interrupt Register (`Sn_IR`).
pub const S0_IR: u16 = 0x0002;
/// Socket 0 Register: Status Register (`Sn_SR`).
pub const S0_SR: u16 = 0x0003;
/// Socket 0 Register: RX Buffer Size in KB (`Sn_RXBUF_SIZE`).
pub const S0_RXBUF_SIZE: u16 = 0x001E;
/// Socket 0 Register: TX Buffer Size in KB (`Sn_TXBUF_SIZE`).
pub const S0_TXBUF_SIZE: u16 = 0x001F;
/// Socket 0 Register: TX Free Size (`Sn_TX_FSR`, 2 bytes big-endian).
pub const S0_TX_FSR: u16 = 0x0020;
/// Socket 0 Register: TX Write Pointer (`Sn_TX_WR`, 2 bytes big-endian).
pub const S0_TX_WR: u16 = 0x0024;
/// Socket 0 Register: RX Received Size (`Sn_RX_RSR`, 2 bytes big-endian).
pub const S0_RX_RSR: u16 = 0x0026;
/// Socket 0 Register: RX Read Pointer (`Sn_RX_RD`, 2 bytes big-endian).
pub const S0_RX_RD: u16 = 0x0028;

/// `Sn_MR` protocol bits for `MACRAW` Ethernet frame mode on Socket 0 (`0x04`).
pub const SOCK_MR_MACRAW: u8 = 0x04;
/// `Sn_MR` MAC filter enable flag (`0x80`).
pub const SOCK_MR_MFEN: u8 = 0x80;
/// `Sn_CR` command: Open socket (`0x01`).
pub const SOCK_CMD_OPEN: u8 = 0x01;
/// `Sn_CR` command: Close socket (`0x10`).
pub const SOCK_CMD_CLOSE: u8 = 0x10;
/// `Sn_CR` command: Transmit buffered frame (`0x20`).
pub const SOCK_CMD_SEND: u8 = 0x20;
/// `Sn_CR` command: Complete RX read window (`0x40`).
pub const SOCK_CMD_RECV: u8 = 0x40;
/// `Sn_SR` status indicating Socket 0 is open in `MACRAW` mode (`0x42`).
pub const SOCK_SR_MACRAW: u8 = 0x42;

/// SPI control-phase read/write direction.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiAccessMode {
    /// Read transfer (`RWB = 0`).
    Read,
    /// Write transfer (`RWB = 1`).
    Write,
}

/// Encodes the 3-byte W5500 SPI header (`[Offset_Hi, Offset_Lo, Control]`)
/// in Variable Data Length Mode (`OM = 0b00`).
#[inline(always)]
#[must_use]
pub const fn encode_spi_header(
    offset: u16,
    block_select: u8,
    mode: SpiAccessMode,
) -> [u8; 3] {
    let rwb_bit = match mode {
        SpiAccessMode::Read => 0u8,
        SpiAccessMode::Write => 1u8 << 2,
    };
    let control = ((block_select & 0x1F) << 3) | rwb_bit;
    let [hi, lo] = offset.to_be_bytes();
    [hi, lo, control]
}

/// Physical Ethernet link speed reported by `PHYCFGR` bit 1 (`SPD`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EthLinkSpeed {
    /// 10BASE-T (10 Mbps).
    Mbps10,
    /// 100BASE-TX (100 Mbps).
    Mbps100,
}

/// Physical Ethernet duplex state reported by `PHYCFGR` bit 2 (`DPX`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EthDuplex {
    /// Half-duplex operation.
    Half,
    /// Full-duplex operation.
    Full,
}

/// Decoded W5500 `PHYCFGR` (`0x002E`) physical link status.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhyStatus {
    /// Whether physical twisted-pair link is up (`LNK`, bit 0).
    pub link_up: bool,
    /// Negotiated link speed (`SPD`, bit 1).
    pub speed: EthLinkSpeed,
    /// Negotiated duplex mode (`DPX`, bit 2).
    pub duplex: EthDuplex,
    /// Raw `PHYCFGR` register byte.
    pub raw: u8,
}

impl PhyStatus {
    /// Decodes a raw `PHYCFGR` byte (`0x002E`).
    #[must_use]
    pub const fn from_phycfgr(raw: u8) -> Self {
        let link_up = (raw & 0x01) != 0;
        let speed = if (raw & 0x02) != 0 {
            EthLinkSpeed::Mbps100
        } else {
            EthLinkSpeed::Mbps10
        };
        let duplex = if (raw & 0x04) != 0 {
            EthDuplex::Full
        } else {
            EthDuplex::Half
        };
        Self {
            link_up,
            speed,
            duplex,
            raw,
        }
    }
}

/// Errors returned by [`W5500Driver`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum W5500Error<E> {
    /// Underlying SPI bus transaction error.
    Spi(E),
    /// `VERSIONR` (`0x0039`) did not return `0x04`.
    UnexpectedChipVersion(u8),
    /// Socket 0 failed to transition into `SOCK_SR_MACRAW` (`0x42`).
    SocketOpenFailed(u8),
    /// Outbound Ethernet frame exceeds [`MAX_ETHERNET_FRAME_BYTES`] (`1518`).
    FrameTooLarge(usize),
    /// Socket 0 TX ring buffer lacks sufficient free space.
    TxBufferFull {
        /// Bytes required by frame.
        required: u16,
        /// Bytes currently free in `S0_TX_FSR`.
        free: u16,
    },
    /// Socket 0 RX ring buffer contained a malformed 2-byte length prefix.
    InvalidRxHeader(u16),
    /// Caller-supplied `out_frame` slice is smaller than the received frame.
    BufferTooSmall {
        /// Frame payload length in bytes.
        required: usize,
        /// Caller buffer length in bytes.
        capacity: usize,
    },
}

/// Pure-Rust W5500 SPI MACRAW (`Socket 0`) Ethernet Driver.
#[non_exhaustive]
#[derive(Debug)]
pub struct W5500Driver<SPI> {
    spi: SPI,
    mac: [u8; 6],
}

/// Socket 0 TX/RX buffer size (16 KiB for MACRAW mode).
pub const S0_BUF_SIZE: u16 = 0x4000;
/// Bitmask for Socket 0 16 KiB ring buffer wrapping (`0x3FFF`).
pub const S0_BUF_MASK: u16 = 0x3FFF;

impl<SPI: SpiDevice> W5500Driver<SPI> {
    /// Wraps an `embedded_hal::spi::SpiDevice` with the given 6-byte hardware
    /// MAC address.
    #[must_use]
    pub const fn new(spi: SPI, mac: [u8; 6]) -> Self {
        Self { spi, mac }
    }

    /// Configured 6-byte IEEE 802.3 MAC address.
    #[must_use]
    pub const fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Consumes the driver and returns the underlying SPI device.
    #[must_use]
    pub fn into_inner(self) -> SPI {
        self.spi
    }

    /// Writes `data` to `offset` inside `block_select`.
    #[inline(always)]
    pub fn write_block(
        &mut self,
        offset: u16,
        block_select: u8,
        data: &[u8],
    ) -> Result<(), W5500Error<SPI::Error>> {
        let hdr = encode_spi_header(offset, block_select, SpiAccessMode::Write);
        let mut ops = [Operation::Write(&hdr), Operation::Write(data)];
        self.spi.transaction(&mut ops).map_err(W5500Error::Spi)
    }

    /// Reads `out.len()` bytes from `offset` inside `block_select`.
    #[inline(always)]
    pub fn read_block(
        &mut self,
        offset: u16,
        block_select: u8,
        out: &mut [u8],
    ) -> Result<(), W5500Error<SPI::Error>> {
        let hdr = encode_spi_header(offset, block_select, SpiAccessMode::Read);
        let mut ops = [Operation::Write(&hdr), Operation::Read(out)];
        self.spi.transaction(&mut ops).map_err(W5500Error::Spi)
    }

    #[inline(always)]
    fn write_u8(
        &mut self,
        offset: u16,
        block_select: u8,
        val: u8,
    ) -> Result<(), W5500Error<SPI::Error>> {
        self.write_block(offset, block_select, &[val])
    }

    #[inline(always)]
    fn read_u8(
        &mut self,
        offset: u16,
        block_select: u8,
    ) -> Result<u8, W5500Error<SPI::Error>> {
        let mut buf = [0u8; 1];
        self.read_block(offset, block_select, &mut buf)?;
        Ok(buf[0])
    }

    #[inline(always)]
    fn write_u16_be(
        &mut self,
        offset: u16,
        block_select: u8,
        val: u16,
    ) -> Result<(), W5500Error<SPI::Error>> {
        let bytes = val.to_be_bytes();
        self.write_block(offset, block_select, &bytes)
    }

    #[inline(always)]
    fn read_u16_be(
        &mut self,
        offset: u16,
        block_select: u8,
    ) -> Result<u16, W5500Error<SPI::Error>> {
        let mut buf = [0u8; 2];
        self.read_block(offset, block_select, &mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    /// Reads the W5500 hardware version register (`VERSIONR` at `0x0039`).
    pub fn chip_version(&mut self) -> Result<u8, W5500Error<SPI::Error>> {
        self.read_u8(REG_VERSIONR, BSB_COMMON_REG)
    }

    /// Reads and decodes the `PHYCFGR` (`0x002E`) link/speed/duplex register.
    pub fn phy_status(&mut self) -> Result<PhyStatus, W5500Error<SPI::Error>> {
        let raw = self.read_u8(REG_PHYCFGR, BSB_COMMON_REG)?;
        Ok(PhyStatus::from_phycfgr(raw))
    }

    /// Programs the common IPv4 registers (`GAR`, `SUBR`, `SIPR`).
    pub fn configure_ip(
        &mut self,
        ip: [u8; 4],
        subnet: [u8; 4],
        gateway: [u8; 4],
    ) -> Result<(), W5500Error<SPI::Error>> {
        self.write_block(REG_GAR, BSB_COMMON_REG, &gateway)?;
        self.write_block(REG_SUBR, BSB_COMMON_REG, &subnet)?;
        self.write_block(REG_SIPR, BSB_COMMON_REG, &ip)
    }

    /// Resets the W5500, verifies `VERSIONR == 0x04`, writes the 6-byte MAC
    /// (`SHAR`), assigns all 16 KB RX/TX buffer RAM to Socket 0, and opens
    /// Socket 0 in `MACRAW` (`0x04`) mode.
    pub fn init_macraw(&mut self) -> Result<(), W5500Error<SPI::Error>> {
        // 1. Software reset (MR = 0x80)
        self.write_u8(REG_MR, BSB_COMMON_REG, 0x80)?;
        // 2. Verify chip version (0x04)
        let ver = self.chip_version()?;
        if ver != W5500_CHIP_VERSION {
            return Err(W5500Error::UnexpectedChipVersion(ver));
        }
        // 3. Program 6-byte hardware MAC address (SHAR)
        self.write_block(REG_SHAR, BSB_COMMON_REG, &self.mac.clone())?;
        // 4. Allocate 16 KB RX and 16 KB TX buffers to Socket 0
        self.write_u8(S0_RXBUF_SIZE, BSB_S0_REG, 16)?;
        self.write_u8(S0_TXBUF_SIZE, BSB_S0_REG, 16)?;
        // 5. Configure Socket 0 for MACRAW with MAC filtering enabled
        self.write_u8(S0_MR, BSB_S0_REG, SOCK_MR_MACRAW | SOCK_MR_MFEN)?;
        // 6. Open Socket 0
        self.write_u8(S0_CR, BSB_S0_REG, SOCK_CMD_OPEN)?;
        // 7. Verify Socket 0 status is SOCK_SR_MACRAW (0x42)
        let sr = self.read_u8(S0_SR, BSB_S0_REG)?;
        if sr != SOCK_SR_MACRAW {
            return Err(W5500Error::SocketOpenFailed(sr));
        }
        Ok(())
    }

    /// Returns the number of unread bytes waiting in Socket 0's RX buffer
    /// (`S0_RX_RSR`).
    pub fn rx_available(&mut self) -> Result<u16, W5500Error<SPI::Error>> {
        self.read_u16_be(S0_RX_RSR, BSB_S0_REG)
    }

    /// Reads `out.len()` bytes from Socket 0 ring buffer, wrapping around
    /// `S0_BUF_SIZE` (16 KiB) with split SPI transactions if needed.
    pub fn read_socket0_ring_block(
        &mut self,
        ptr: u16,
        block_select: u8,
        out: &mut [u8],
    ) -> Result<(), W5500Error<SPI::Error>> {
        let offset = ptr & S0_BUF_MASK;
        let len = out.len() as u16;
        if offset.saturating_add(len) <= S0_BUF_SIZE {
            self.read_block(offset, block_select, out)
        } else {
            let first_len = (S0_BUF_SIZE - offset) as usize;
            self.read_block(offset, block_select, &mut out[..first_len])?;
            self.read_block(0x0000, block_select, &mut out[first_len..])
        }
    }

    /// Writes `data.len()` bytes to Socket 0 ring buffer, wrapping around
    /// `S0_BUF_SIZE` (16 KiB) with split SPI transactions if needed.
    pub fn write_socket0_ring_block(
        &mut self,
        ptr: u16,
        block_select: u8,
        data: &[u8],
    ) -> Result<(), W5500Error<SPI::Error>> {
        let offset = ptr & S0_BUF_MASK;
        let len = data.len() as u16;
        if offset.saturating_add(len) <= S0_BUF_SIZE {
            self.write_block(offset, block_select, data)
        } else {
            let first_len = (S0_BUF_SIZE - offset) as usize;
            self.write_block(offset, block_select, &data[..first_len])?;
            self.write_block(0x0000, block_select, &data[first_len..])
        }
    }

    /// Reads the next complete Ethernet frame from Socket 0's `MACRAW` RX
    /// buffer into `out_frame`, returning the frame length in bytes (`0` if
    /// no frame is waiting).
    pub fn read_frame(
        &mut self,
        out_frame: &mut [u8],
    ) -> Result<usize, W5500Error<SPI::Error>> {
        let rsr = self.rx_available()?;
        if rsr < 2 {
            return Ok(0);
        }
        let rx_rd = self.read_u16_be(S0_RX_RD, BSB_S0_REG)?;
        let mut len_hdr = [0u8; 2];
        self.read_socket0_ring_block(rx_rd, BSB_S0_RX_BUF, &mut len_hdr)?;
        let packet_size = u16::from_be_bytes(len_hdr);
        if packet_size < 2 || packet_size > rsr {
            let new_rx_rd = rx_rd.wrapping_add(2);
            self.write_u16_be(S0_RX_RD, BSB_S0_REG, new_rx_rd)?;
            self.write_u8(S0_CR, BSB_S0_REG, SOCK_CMD_RECV)?;
            return Err(W5500Error::InvalidRxHeader(packet_size));
        }
        let frame_len = usize::from(packet_size - 2);
        if frame_len > out_frame.len() {
            let new_rx_rd = rx_rd.wrapping_add(packet_size);
            self.write_u16_be(S0_RX_RD, BSB_S0_REG, new_rx_rd)?;
            self.write_u8(S0_CR, BSB_S0_REG, SOCK_CMD_RECV)?;
            return Err(W5500Error::BufferTooSmall {
                required: frame_len,
                capacity: out_frame.len(),
            });
        }
        let payload_offset = rx_rd.wrapping_add(2);
        self.read_socket0_ring_block(payload_offset, BSB_S0_RX_BUF, &mut out_frame[..frame_len])?;
        let new_rx_rd = rx_rd.wrapping_add(packet_size);
        self.write_u16_be(S0_RX_RD, BSB_S0_REG, new_rx_rd)?;
        self.write_u8(S0_CR, BSB_S0_REG, SOCK_CMD_RECV)?;
        Ok(frame_len)
    }

    /// Writes a raw Ethernet frame into Socket 0's `MACRAW` TX buffer and
    /// triggers transmission (`SOCK_CMD_SEND`).
    pub fn write_frame(&mut self, frame: &[u8]) -> Result<usize, W5500Error<SPI::Error>> {
        if frame.is_empty() {
            return Ok(0);
        }
        if frame.len() > MAX_ETHERNET_FRAME_BYTES {
            return Err(W5500Error::FrameTooLarge(frame.len()));
        }
        let needed = frame.len() as u16;
        let free = self.read_u16_be(S0_TX_FSR, BSB_S0_REG)?;
        if free < needed {
            return Err(W5500Error::TxBufferFull {
                required: needed,
                free,
            });
        }
        let tx_wr = self.read_u16_be(S0_TX_WR, BSB_S0_REG)?;
        self.write_socket0_ring_block(tx_wr, BSB_S0_TX_BUF, frame)?;
        let new_tx_wr = tx_wr.wrapping_add(needed);
        self.write_u16_be(S0_TX_WR, BSB_S0_REG, new_tx_wr)?;
        self.write_u8(S0_CR, BSB_S0_REG, SOCK_CMD_SEND)?;
        Ok(frame.len())
    }

    /// Receives a UDP or raw frame, un-wedging the RX ring buffer even on errors.
    pub fn recv_udp_frame(
        &mut self,
        buf: &mut [u8],
    ) -> Result<usize, W5500Error<SPI::Error>> {
        self.read_frame(buf)
    }

    /// Alias for [`Self::init_macraw`] initializing hardware and Socket 0.
    pub fn init(&mut self) -> Result<(), W5500Error<SPI::Error>> {
        self.init_macraw()
    }

    /// Executes a Socket command by writing to `S0_CR` and polling until cleared
    /// to 0x00 (bounded to 16 iterations).
    pub fn exec_socket_cmd(
        &mut self,
        block_select: u8,
        cmd: u8,
    ) -> Result<(), W5500Error<SPI::Error>> {
        self.write_u8(S0_CR, block_select, cmd)?;
        for _ in 0..16 {
            let cr = self.read_u8(S0_CR, block_select)?;
            if cr == 0x00 {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Polls physical link and manages state transitions. When recovering from
    /// [`W5500LinkState::LinkDown`] to [`W5500LinkState::LinkUp`], transitions
    /// to [`W5500LinkState::Init`] and re-initializes hardware before entering
    /// [`W5500LinkState::DhcpDiscover`].
    pub fn poll_link(
        &mut self,
        current: W5500LinkState,
    ) -> Result<W5500LinkState, W5500Error<SPI::Error>> {
        let phy = self.phy_status()?;
        if !phy.link_up {
            return Ok(W5500LinkState::LinkDown);
        }
        match current {
            W5500LinkState::LinkDown | W5500LinkState::Init => {
                self.init()?;
                Ok(W5500LinkState::DhcpDiscover)
            }
            W5500LinkState::LinkUp => Ok(W5500LinkState::DhcpDiscover),
            other => Ok(other),
        }
    }
}

/// Link state machine for W5500 Ethernet interface.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum W5500LinkState {
    /// Interface is uninitialized or in reset.
    Init,
    /// Physical Ethernet cable is disconnected.
    LinkDown,
    /// Physical link is up.
    LinkUp,
    /// DHCP discover / IP lease acquisition in progress.
    DhcpDiscover,
    /// Link and IP address are fully configured.
    Configured,
}

/// Helper wrapper representing an active W5500 socket.
#[non_exhaustive]
#[derive(Debug)]
pub struct W5500Socket<'a, SPI> {
    driver: &'a mut W5500Driver<SPI>,
}

impl<'a, SPI: SpiDevice> W5500Socket<'a, SPI> {
    /// Wraps a [`W5500Driver`] reference.
    pub fn new(driver: &'a mut W5500Driver<SPI>) -> Self {
        Self { driver }
    }

    /// Receives a frame from Socket 0, advancing RX pointers even on error.
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, W5500Error<SPI::Error>> {
        self.driver.recv_udp_frame(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_mock::eh1::spi::{Mock as SpiMock, Transaction as SpiTransaction};

    #[test]
    fn test_spi_header_encoding_and_phycfgr_decoding() {
        let hdr_rd = encode_spi_header(REG_VERSIONR, BSB_COMMON_REG, SpiAccessMode::Read);
        assert_eq!(hdr_rd, [0x00, 0x39, 0x00]);

        let hdr_wr = encode_spi_header(S0_MR, BSB_S0_REG, SpiAccessMode::Write);
        assert_eq!(hdr_wr, [0x00, 0x00, (BSB_S0_REG << 3) | 0x04]);

        let phy_up = PhyStatus::from_phycfgr(0b1011_1111);
        assert!(phy_up.link_up);
        assert_eq!(phy_up.speed, EthLinkSpeed::Mbps100);
        assert_eq!(phy_up.duplex, EthDuplex::Full);

        let phy_down = PhyStatus::from_phycfgr(0b1011_1000);
        assert!(!phy_down.link_up);
        assert_eq!(phy_down.speed, EthLinkSpeed::Mbps10);
        assert_eq!(phy_down.duplex, EthDuplex::Half);
    }

    #[test]
    fn test_w5500_init_macraw_and_frame_tx_rx() {
        let mac = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
        let sample_frame: [u8; 14] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x06,
        ];

        let expectations = [
            // 1. Soft reset MR = 0x80
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x00, 0x04]),
            SpiTransaction::write_vec(std::vec![0x80]),
            SpiTransaction::transaction_end(),
            // 2. Read VERSIONR -> 0x04
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x39, 0x00]),
            SpiTransaction::read_vec(std::vec![0x04]),
            SpiTransaction::transaction_end(),
            // 3. Write SHAR (6-byte MAC)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x09, 0x04]),
            SpiTransaction::write_vec(mac.to_vec()),
            SpiTransaction::transaction_end(),
            // 4. Write S0_RXBUF_SIZE = 16, S0_TXBUF_SIZE = 16
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x1E, 0x0C]),
            SpiTransaction::write_vec(std::vec![16]),
            SpiTransaction::transaction_end(),
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x1F, 0x0C]),
            SpiTransaction::write_vec(std::vec![16]),
            SpiTransaction::transaction_end(),
            // 5. Write S0_MR = 0x84 (MACRAW | MFEN)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x00, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x84]),
            SpiTransaction::transaction_end(),
            // 6. Write S0_CR = OPEN (0x01)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x01, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x01]),
            SpiTransaction::transaction_end(),
            // 7. Read S0_SR -> 0x42 (SOCK_SR_MACRAW)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x03, 0x08]),
            SpiTransaction::read_vec(std::vec![0x42]),
            SpiTransaction::transaction_end(),
            // 8. TX frame: Read S0_TX_FSR -> 2048 (0x0800)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x20, 0x08]),
            SpiTransaction::read_vec(std::vec![0x08, 0x00]),
            SpiTransaction::transaction_end(),
            // 9. Read S0_TX_WR -> 0x0100
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x24, 0x08]),
            SpiTransaction::read_vec(std::vec![0x01, 0x00]),
            SpiTransaction::transaction_end(),
            // 10. Write frame to BSB_S0_TX_BUF (0x02 << 3 | 0x04 = 0x14) at 0x0100
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x01, 0x00, 0x14]),
            SpiTransaction::write_vec(sample_frame.to_vec()),
            SpiTransaction::transaction_end(),
            // 11. Update S0_TX_WR = 0x010E
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x24, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x01, 0x0E]),
            SpiTransaction::transaction_end(),
            // 12. Issue S0_CR = SEND (0x20)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x01, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x20]),
            SpiTransaction::transaction_end(),
            // 13. RX frame: Read S0_RX_RSR -> 16 (2-byte hdr + 14-byte frame)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x26, 0x08]),
            SpiTransaction::read_vec(std::vec![0x00, 0x10]),
            SpiTransaction::transaction_end(),
            // 14. Read S0_RX_RD -> 0x0040
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x28, 0x08]),
            SpiTransaction::read_vec(std::vec![0x00, 0x40]),
            SpiTransaction::transaction_end(),
            // 15. Read 2-byte packet_size header from BSB_S0_RX_BUF at 0x0040
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x40, 0x18]),
            SpiTransaction::read_vec(std::vec![0x00, 0x10]),
            SpiTransaction::transaction_end(),
            // 16. Read 14-byte Ethernet frame from BSB_S0_RX_BUF at 0x0042
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x42, 0x18]),
            SpiTransaction::read_vec(sample_frame.to_vec()),
            SpiTransaction::transaction_end(),
            // 17. Update S0_RX_RD = 0x0050
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x28, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x00, 0x50]),
            SpiTransaction::transaction_end(),
            // 18. Issue S0_CR = RECV (0x40)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x01, 0x0C]),
            SpiTransaction::write_vec(std::vec![0x40]),
            SpiTransaction::transaction_end(),
        ];

        let spi = SpiMock::new(&expectations);
        let mut drv = W5500Driver::new(spi, mac);
        drv.init_macraw().expect("init_macraw");

        let sent = drv.write_frame(&sample_frame).expect("write_frame");
        assert_eq!(sent, 14);

        let mut rx_buf = [0u8; 64];
        let rcvd = drv.read_frame(&mut rx_buf).expect("read_frame");
        assert_eq!(rcvd, 14);
        assert_eq!(&rx_buf[..14], &sample_frame);

        let mut spi = drv.into_inner();
        spi.done();
    }

    #[test]
    fn test_net_501_w5500_ring_buffer_wrapping() {
        let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
        let write_data = [0xAA, 0xBB, 0xCC, 0xDD];
        let expectations = [
            // 1. write_socket0_ring_block at 0x3FFE (2 bytes at 0x3FFE, 2 bytes at 0x0000)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x3F, 0xFE, (BSB_S0_TX_BUF << 3) | 0x04]),
            SpiTransaction::write_vec(std::vec![0xAA, 0xBB]),
            SpiTransaction::transaction_end(),
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x00, (BSB_S0_TX_BUF << 3) | 0x04]),
            SpiTransaction::write_vec(std::vec![0xCC, 0xDD]),
            SpiTransaction::transaction_end(),
            // 2. read_socket0_ring_block at 0x3FFE (2 bytes at 0x3FFE, 2 bytes at 0x0000)
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x3F, 0xFE, BSB_S0_RX_BUF << 3]),
            SpiTransaction::read_vec(std::vec![0x11, 0x22]),
            SpiTransaction::transaction_end(),
            SpiTransaction::transaction_start(),
            SpiTransaction::write_vec(std::vec![0x00, 0x00, BSB_S0_RX_BUF << 3]),
            SpiTransaction::read_vec(std::vec![0x33, 0x44]),
            SpiTransaction::transaction_end(),
        ];

        let spi = SpiMock::new(&expectations);
        let mut drv = W5500Driver::new(spi, mac);

        drv.write_socket0_ring_block(0x3FFE, BSB_S0_TX_BUF, &write_data)
            .expect("split tx ring write");

        let mut read_buf = [0u8; 4];
        drv.read_socket0_ring_block(0x3FFE, BSB_S0_RX_BUF, &mut read_buf)
            .expect("split rx ring read");
        assert_eq!(read_buf, [0x11, 0x22, 0x33, 0x44]);

        let mut spi = drv.into_inner();
        spi.done();
    }
}
