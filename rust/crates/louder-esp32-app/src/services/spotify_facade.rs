//! Spotify Connect ZeroConf HTTP and Spirc facade service (`TCP 4070`).

extern crate alloc;

use alloc::boxed::Box;

#[cfg(target_arch = "xtensa")]
use smoltcp::iface::SocketHandle;
#[cfg(target_arch = "xtensa")]
use smoltcp::socket::tcp;
#[cfg(target_arch = "xtensa")]
use squeezelite_cspot::spirc::SpircEvent;
#[cfg(target_arch = "xtensa")]
use squeezelite_cspot::spirc::SpircFrame;
use squeezelite_cspot::spirc::SpircStateMachine;

#[cfg(target_arch = "xtensa")]
use crate::board::first_boot_defaults::PLAYER_NAME;
use crate::coordinator::AudioSource;
#[cfg(target_arch = "xtensa")]
use crate::services::AmpPower;
use crate::services::DeviceService;
use crate::services::ServiceCtx;
use crate::services::StatusExtras;

/// Spotify Connect ZeroConf HTTP and Spirc TCP port.
#[cfg(target_arch = "xtensa")]
const SPOTIFY_FACADE_PORT: u16 = 4070;

#[cfg(target_arch = "xtensa")]
enum TcpRxAction {
    None,
    Spirc { seq_nr: u32, event: SpircEvent },
    RawPcm(usize),
}

/// Spotify Connect ZeroConf HTTP and Spirc facade service.
pub struct SpotifyFacadeService {
    #[cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]
    spot_spirc: Box<SpircStateMachine>,
    spot_streaming: bool,
    last_ip: Option<[u8; 4]>,
    #[cfg(target_arch = "xtensa")]
    spot_handle: Option<SocketHandle>,
}

impl Default for SpotifyFacadeService {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyFacadeService {
    /// Creates a new Spotify Connect facade service with PSRAM-allocated state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spot_spirc: Box::new(SpircStateMachine::new()),
            spot_streaming: false,
            last_ip: None,
            #[cfg(target_arch = "xtensa")]
            spot_handle: None,
        }
    }

    /// Returns whether Spotify Connect streaming is currently active.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.spot_streaming
    }

    fn reset_transport(&mut self, ctx: &mut ServiceCtx<'_>) {
        #[cfg(target_arch = "xtensa")]
        if ctx.has_sockets() {
            if let Some(handle) = self.spot_handle {
                ctx.socket_mut::<tcp::Socket>(handle).abort();
            }
        }
        #[cfg(not(target_arch = "xtensa"))]
        let _ = ctx;
        self.spot_streaming = false;
    }

    #[cfg(target_arch = "xtensa")]
    fn poll_tcp(&mut self, ctx: &mut ServiceCtx<'_>, cspot_name: &str) {
        let Some(spot_handle) = self.spot_handle else {
            return;
        };

        let mut req_buf = [0u8; 1024];
        let action = {
            let spot_sock = ctx.socket_mut::<tcp::Socket>(spot_handle);
            if spot_sock.is_open()
                && matches!(
                    spot_sock.state(),
                    tcp::State::CloseWait
                        | tcp::State::TimeWait
                        | tcp::State::Closing
                        | tcp::State::LastAck
                        | tcp::State::FinWait1
                        | tcp::State::FinWait2
                )
            {
                spot_sock.abort();
            }
            if !spot_sock.is_open() {
                let _ = spot_sock.listen(SPOTIFY_FACADE_PORT);
            }
            if !(spot_sock.can_recv() && spot_sock.may_send()) {
                TcpRxAction::None
            } else if let Ok(n) = spot_sock.recv_slice(&mut req_buf) {
                if n == 0 {
                    TcpRxAction::None
                } else if req_buf[..n].starts_with(b"GET ") || req_buf[..n].starts_with(b"POST ") {
                    let body_prefix = b"{\"status\":101,\"statusString\":\"OK\",\
                        \"spotifyError\":0,\"version\":\"2.9.0\",\
                        \"deviceID\":\"c82e18beb8340000000000000000000000000000\",\
                        \"remoteName\":\"";
                    let body_suffix = b"\",\"activeUser\":\"\",\
                        \"publicKey\":\"\",\"deviceType\":\"SPEAKER\",\
                        \"libraryVersion\":\"0.1.0\",\"accountReq\":\"PREMIUM\",\
                        \"brandDisplayName\":\"Squeezelite\",\
                        \"modelDisplayName\":\"ESP32\"}";
                    let hdr = b"HTTP/1.1 200 OK\r\n\
                        Content-Type: application/json\r\n\
                        Connection: close\r\n\r\n";
                    let _ = spot_sock.send_slice(hdr);
                    let _ = spot_sock.send_slice(body_prefix);
                    let _ = spot_sock.send_slice(cspot_name.as_bytes());
                    let _ = spot_sock.send_slice(body_suffix);
                    spot_sock.close();
                    TcpRxAction::None
                } else if let Ok(frame) = SpircFrame::parse(&req_buf[..n]) {
                    match self.spot_spirc.handle_frame(&frame) {
                        Some(event) => TcpRxAction::Spirc {
                            seq_nr: frame.seq_nr,
                            event,
                        },
                        None => TcpRxAction::None,
                    }
                } else {
                    TcpRxAction::RawPcm(n)
                }
            } else {
                TcpRxAction::None
            }
        };

        match action {
            TcpRxAction::None => {}
            TcpRxAction::Spirc { seq_nr, event } => match event {
                SpircEvent::Play | SpircEvent::LoadTrack { .. } => {
                    let granted = if self.spot_streaming {
                        ctx.request_external_source(AudioSource::Spotify)
                    } else {
                        let ok = ctx.claim_external_pcm(
                            AudioSource::Spotify,
                            Some(44_100),
                            AmpPower::EnsurePlay,
                        );
                        if ok {
                            self.spot_streaming = true;
                        }
                        ok
                    };
                    if granted {
                        let notify = self.spot_spirc.build_notify_frame(seq_nr, cspot_name);
                        let _ = ctx
                            .socket_mut::<tcp::Socket>(spot_handle)
                            .send_slice(&notify);
                    }
                }
                SpircEvent::Pause => {
                    self.spot_streaming = false;
                    let _ = ctx.release_external(AudioSource::Spotify);
                }
                SpircEvent::SetVolume(vol_u32) => {
                    let raw_pct = ((vol_u32.min(65_536) * 100) / 65_536) as u8;
                    if let Some(pct) = ctx.set_local_volume(raw_pct) {
                        ctx.config().update(|c| c.volume = pct);
                    }
                }
                SpircEvent::Next
                | SpircEvent::Prev
                | SpircEvent::Seek(_)
                | SpircEvent::Notify
                | SpircEvent::TrackEnded
                | SpircEvent::MetadataUpdated(_) => {}
            },
            TcpRxAction::RawPcm(n) => {
                if self.spot_streaming {
                    let _ = ctx.ingest_external_pcm(&req_buf[..n]);
                }
            }
        }
    }
}

impl DeviceService for SpotifyFacadeService {
    fn name(&self) -> &'static str {
        "spotify_facade"
    }

    fn init(&mut self, ctx: &mut ServiceCtx<'_>) {
        #[cfg(target_arch = "xtensa")]
        if ctx.has_sockets() && self.spot_handle.is_none() {
            let spot_rx: &'static mut [u8] = Box::leak(alloc::vec![0u8; 4096].into_boxed_slice());
            let spot_tx: &'static mut [u8] = Box::leak(alloc::vec![0u8; 2048].into_boxed_slice());
            let spot_sock = tcp::Socket::new(
                tcp::SocketBuffer::new(spot_rx),
                tcp::SocketBuffer::new(spot_tx),
            );
            self.spot_handle = Some(ctx.add_socket(spot_sock));
        }
        #[cfg(not(target_arch = "xtensa"))]
        let _ = ctx;
    }

    fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool) {
        if !up {
            self.reset_transport(ctx);
            self.last_ip = None;
        } else if let Some(ip) = ctx.link_ip() {
            self.last_ip = Some(ip);
        }
    }

    fn poll(&mut self, ctx: &mut ServiceCtx<'_>) {
        if let Some(active) = ctx.active_source() {
            self.spot_streaming = active == AudioSource::Spotify;
        }
        let Some(local_ip) = ctx.link_ip() else {
            return;
        };
        if self.last_ip.is_some_and(|old| old != local_ip) {
            self.reset_transport(ctx);
        }
        self.last_ip = Some(local_ip);
        if ctx.config().is_cspot_enabled() {
            #[cfg(target_arch = "xtensa")]
            if ctx.has_sockets() {
                let mut cspot_name = ctx.config().effective_cspot_name();
                if cspot_name.trim().is_empty() {
                    cspot_name.clear();
                    let _ = cspot_name.push_str(PLAYER_NAME);
                }
                self.poll_tcp(ctx, cspot_name.as_str());
            }
        }
    }

    fn status(&self, out: &mut StatusExtras) {
        out.push_bool("spotify_streaming", self.spot_streaming);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use squeezelite_config::AudioConfig;
    use squeezelite_decoders::PcmDecoder;

    use crate::config_handle::ConfigHandle;
    use crate::coordinator::PlayerCoordinator;
    use crate::io_traits::MockBackend;
    use crate::pipeline::ActiveDecoder;
    use crate::pipeline::StreamPipeline;
    use crate::services::ServiceAudioAdapter;
    use crate::services::StatusValue;
    use crate::sink::SimulatedSink;

    #[test]
    fn spotify_facade_lifecycle_resets_on_ip_change_and_link_down() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut svc = SpotifyFacadeService::new();
        assert_eq!(svc.name(), "spotify_facade");
        assert!(!svc.is_streaming());

        let mut init_ctx = ServiceCtx::new(0, None, &cfg);
        svc.init(&mut init_ctx);
        assert!(!svc.is_streaming());

        let mut up_ctx = ServiceCtx::new(100, Some([172, 22, 1, 77]), &cfg);
        svc.on_link(&mut up_ctx, true);
        assert_eq!(svc.last_ip, Some([172, 22, 1, 77]));

        svc.spot_streaming = true;
        let mut extras = StatusExtras::new();
        svc.status(&mut extras);
        assert_eq!(extras.len(), 1);
        assert_eq!(extras.fields()[0].key, "spotify_streaming");
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(true));

        // Changing IP during poll resets transient streaming state.
        let mut changed_ctx = ServiceCtx::new(150, Some([172, 22, 1, 88]), &cfg);
        svc.poll(&mut changed_ctx);
        assert_eq!(svc.last_ip, Some([172, 22, 1, 88]));
        assert!(!svc.is_streaming());

        svc.spot_streaming = true;
        let mut down_ctx = ServiceCtx::new(200, None, &cfg);
        svc.on_link(&mut down_ctx, false);
        assert!(!svc.is_streaming());
        assert_eq!(svc.last_ip, None);

        extras.clear();
        svc.status(&mut extras);
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(false));
    }

    #[test]
    fn spotify_facade_syncs_streaming_status_with_coordinator_active_source() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut coord = PlayerCoordinator::new([0x24, 0x0A, 0xC4, 0, 0, 1], "test");
        let mut stream_mem = vec![0u8; 4096];
        let mut out_mem = vec![0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            SimulatedSink::new(44_100),
            44_100,
        );
        let mut backend = MockBackend::new();
        let mut svc = SpotifyFacadeService::new();

        assert!(coord.request_source(AudioSource::Spotify));
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx =
                ServiceCtx::new(300, Some([172, 22, 1, 77]), &cfg).with_audio(&mut adapter);
            svc.poll(&mut ctx);
        }
        assert!(svc.is_streaming());

        coord.release_source(AudioSource::Spotify);
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx =
                ServiceCtx::new(400, Some([172, 22, 1, 77]), &cfg).with_audio(&mut adapter);
            svc.poll(&mut ctx);
        }
        assert!(!svc.is_streaming());
    }
}
