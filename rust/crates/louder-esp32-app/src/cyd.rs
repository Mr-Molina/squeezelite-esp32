//! Cheap Yellow Display (CYD) Hardware & Protocol Controller
//!
//! Manages serial communications with the Cheap Yellow Display UI:
//! - Serializes outbound status updates (`PlaybackState`, `Volume`, `TrackMetadata`, `Progress`).
//! - Parses inbound touch interaction commands (`Play`, `Pause`, `Toggle`, `Next`, `Previous`, `SetVolume`).

use heapless::Vec;
use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, LinkError, PlayState};

/// Controller managing the UART communication link with the CYD front panel.
pub struct CydController {
    tx_buffer: Vec<u8, 256>,
    rx_scratch: Vec<u8, 128>,
    last_state: Option<PlayState>,
    last_volume: Option<u8>,
}

impl Default for CydController {
    fn default() -> Self {
        Self::new()
    }
}

impl CydController {
    pub fn new() -> Self {
        Self {
            tx_buffer: Vec::new(),
            rx_scratch: Vec::new(),
            last_state: None,
            last_volume: None,
        }
    }

    /// Formats an outgoing message for transmission to the display.
    pub fn format_message(&mut self, msg: &CydMessage) -> Result<&[u8], LinkError> {
        match msg {
            CydMessage::PlaybackState(s) => self.last_state = Some(*s),
            CydMessage::Volume(v) => self.last_volume = Some(*v),
            _ => {}
        }
        CydLinkProtocol::encode_message(msg, &mut self.tx_buffer)?;
        Ok(&self.tx_buffer)
    }

    /// Ingests incoming UART bytes from the display and extracts any complete command.
    pub fn feed_bytes(&mut self, bytes: &[u8]) -> Result<Option<CydCommand>, LinkError> {
        for &b in bytes {
            if b == b'\n' {
                let cmd = CydLinkProtocol::parse_command(&self.rx_scratch)?;
                self.rx_scratch.clear();
                return Ok(Some(cmd));
            } else if self.rx_scratch.push(b).is_err() {
                self.rx_scratch.clear();
                return Err(LinkError::BufferOverflow);
            }
        }
        Ok(None)
    }

    /// Returns the last reported playback state.
    pub fn last_state(&self) -> Option<PlayState> {
        self.last_state
    }

    /// Returns the last reported volume level.
    pub fn last_volume(&self) -> Option<u8> {
        self.last_volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cyd_controller_outbound_and_inbound() {
        let mut controller = CydController::new();

        // 1. Format outbound state
        let tx = controller
            .format_message(&CydMessage::PlaybackState(PlayState::Playing))
            .unwrap();
        assert_eq!(tx, b"STATE=P\n");
        assert_eq!(controller.last_state(), Some(PlayState::Playing));

        // 2. Feed inbound touch command: CMD=PAUSE\n
        let cmd = controller.feed_bytes(b"CMD=PAUSE\n").unwrap();
        assert_eq!(cmd, Some(CydCommand::Pause));

        // 3. Feed inbound volume touch: CMD=VOL:80\n
        let vol_cmd = controller.feed_bytes(b"CMD=VOL:80\n").unwrap();
        assert_eq!(vol_cmd, Some(CydCommand::SetVolume(80)));
    }
}
