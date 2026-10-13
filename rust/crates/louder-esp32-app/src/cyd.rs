//! Cheap Yellow Display (CYD) Hardware & Protocol Controller
//!
//! Manages serial communications with the Cheap Yellow Display UI:
//! - Serializes outbound status updates (`PlaybackState`, `Volume`, `TrackMetadata`, `Progress`).
//! - Parses inbound touch interaction commands (`Play`, `Pause`, `Toggle`, `Next`, `Previous`, `SetVolume`).

use heapless::{Deque, Vec};
use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, LinkError, PlayState};

/// Controller managing the UART communication link with the CYD front panel.
pub struct CydController {
    tx_buffer: Vec<u8, 256>,
    rx_scratch: Vec<u8, 128>,
    command_queue: Deque<CydCommand, 8>,
    discarding_line: bool,
    invalid_frame_count: u32,
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
            command_queue: Deque::new(),
            discarding_line: false,
            invalid_frame_count: 0,
            last_state: None,
            last_volume: None,
        }
    }

    /// Formats an outgoing message for transmission to the display.
    pub fn format_message(&mut self, msg: &CydMessage) -> Result<&[u8], LinkError> {
        CydLinkProtocol::encode_message(msg, &mut self.tx_buffer)?;
        match msg {
            CydMessage::PlaybackState(s) => self.last_state = Some(*s),
            CydMessage::Volume(v) => self.last_volume = Some(*v),
            _ => {}
        }
        Ok(&self.tx_buffer)
    }

    /// Ingests incoming UART bytes and processes all complete commands via callback (CYD-003).
    pub fn feed_all<F: FnMut(CydCommand)>(&mut self, bytes: &[u8], mut on_cmd: F) -> Result<(), LinkError> {
        for &b in bytes {
            if self.discarding_line {
                if b == b'\n' {
                    self.discarding_line = false;
                    self.rx_scratch.clear();
                }
                continue;
            }

            if b == b'\n' {
                let res = CydLinkProtocol::parse_command(&self.rx_scratch);
                // Unconditional scratch clear on delimiter (CYD-002)
                self.rx_scratch.clear();
                match res {
                    Ok(cmd) => on_cmd(cmd),
                    Err(_) => {
                        self.invalid_frame_count = self.invalid_frame_count.saturating_add(1);
                    }
                }
            } else if self.rx_scratch.push(b).is_err() {
                // Scratch overflow: mark line for discard until next \n (CYD-008)
                self.discarding_line = true;
                self.rx_scratch.clear();
            }
        }
        Ok(())
    }

    /// Ingests incoming UART bytes from the display and extracts any complete command.
    pub fn feed_bytes(&mut self, bytes: &[u8]) -> Result<Option<CydCommand>, LinkError> {
        let mut parsed = Vec::<CydCommand, 8>::new();
        self.feed_all(bytes, |cmd| {
            let _ = parsed.push(cmd);
        })?;
        for cmd in parsed {
            let _ = self.command_queue.push_back(cmd);
        }
        Ok(self.command_queue.pop_front())
    }

    /// Pops the next queued command from the multi-command buffer.
    pub fn pop_command(&mut self) -> Option<CydCommand> {
        self.command_queue.pop_front()
    }

    /// Returns the number of invalid frames encountered during command parsing.
    pub fn invalid_frame_count(&self) -> u32 {
        self.invalid_frame_count
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

    #[test]
    fn test_cyd_controller_multi_command_feed() {
        let mut controller = CydController::new();
        let mut commands = std::vec::Vec::new();

        let stream = b"CMD=PLAY\nCMD=VOL:50\nCMD=NEXT\n";
        controller.feed_all(stream, |cmd| commands.push(cmd)).unwrap();

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0], CydCommand::Play);
        assert_eq!(commands[1], CydCommand::SetVolume(50));
        assert_eq!(commands[2], CydCommand::Next);
    }

    #[test]
    fn test_cyd_controller_overflow_discard_recovery() {
        let mut controller = CydController::new();

        // Feed >128 bytes of garbage followed by a valid command
        let mut overlong = [b'X'; 150];
        overlong[149] = b'\n';

        controller.feed_bytes(&overlong).unwrap();

        // Next valid command must parse cleanly without corruption
        let cmd = controller.feed_bytes(b"CMD=PREV\n").unwrap();
        assert_eq!(cmd, Some(CydCommand::Previous));
    }

    #[test]
    fn test_cyd_controller_multi_command_feed_bytes_queue() {
        let mut controller = CydController::new();

        // Feed multiple commands in a single byte buffer (LOGIC-401)
        let first = controller.feed_bytes(b"CMD=PLAY\nCMD=VOL:50\nCMD=PAUSE\n").unwrap();
        assert_eq!(first, Some(CydCommand::Play));

        // Subsequent call with empty slice pops the next command from queue
        let second = controller.feed_bytes(&[]).unwrap();
        assert_eq!(second, Some(CydCommand::SetVolume(50)));

        // pop_command() pops the third command without needing feed_bytes
        let third = controller.pop_command();
        assert_eq!(third, Some(CydCommand::Pause));

        // Queue is now empty
        assert_eq!(controller.pop_command(), None);
        assert_eq!(controller.feed_bytes(&[]).unwrap(), None);
    }

    #[test]
    fn test_cyd_controller_invalid_frame_counter() {
        let mut controller = CydController::new();
        assert_eq!(controller.invalid_frame_count(), 0);

        // Feed invalid frames and check counter increments (LOGIC-403)
        controller.feed_bytes(b"INVALID_CMD\n").unwrap();
        assert_eq!(controller.invalid_frame_count(), 1);

        let cmd = controller.feed_bytes(b"CMD=UNKNOWN\nCMD=PLAY\nCMD=BAD\n").unwrap();
        // 2 more invalid frames: UNKNOWN and BAD (total 3)
        assert_eq!(controller.invalid_frame_count(), 3);
        // Valid command PLAY was returned
        assert_eq!(cmd, Some(CydCommand::Play));
        assert_eq!(controller.pop_command(), None);
    }

    #[test]
    fn test_cyd_controller_format_message_cache_order() {
        let mut controller = CydController::new();
        assert_eq!(controller.last_state(), None);
        assert_eq!(controller.last_volume(), None);

        // State and volume updated after successful encode (LOGIC-402)
        controller.format_message(&CydMessage::PlaybackState(PlayState::Playing)).unwrap();
        assert_eq!(controller.last_state(), Some(PlayState::Playing));

        controller.format_message(&CydMessage::Volume(65)).unwrap();
        assert_eq!(controller.last_volume(), Some(65));
    }

    #[test]
    fn test_cyd_controller_vol_syntax_tolerance() {
        let mut controller = CydController::new();
        // Accepts both VOL: and VOL= (LOGIC-405)
        let cmd1 = controller.feed_bytes(b"CMD=VOL:42\n").unwrap();
        assert_eq!(cmd1, Some(CydCommand::SetVolume(42)));

        let cmd2 = controller.feed_bytes(b"CMD=VOL=84\n").unwrap();
        assert_eq!(cmd2, Some(CydCommand::SetVolume(84)));
    }
}
