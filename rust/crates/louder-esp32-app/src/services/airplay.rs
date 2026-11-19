//! AirPlay 1 (RAOP) RTSP control and RTP audio service.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

#[cfg(target_arch = "xtensa")]
use smoltcp::iface::SocketHandle;
#[cfg(target_arch = "xtensa")]
use smoltcp::socket::{tcp, udp};
use squeezelite_raop::crypto::airport_express_key;
#[cfg(target_arch = "xtensa")]
use squeezelite_raop::rtp::RtpAudioReceiver;
use squeezelite_raop::rtsp::RtspServer;
#[cfg(target_arch = "xtensa")]
use squeezelite_raop::rtsp::{RtspMethod, RtspRequest, ServerState};

#[cfg(target_arch = "xtensa")]
use crate::coordinator::AudioSource;
#[cfg(target_arch = "xtensa")]
use crate::services::AmpPower;
use crate::services::{DeviceService, ServiceCtx, StatusExtras};

/// AirPlay RTSP control TCP port.
#[cfg(target_arch = "xtensa")]
const AIRPLAY_RTSP_PORT: u16 = 5000;

/// AirPlay RTP audio UDP port.
#[cfg(target_arch = "xtensa")]
const AIRPLAY_RTP_PORT: u16 = 6000;

/// Maximum time without RTP packets before releasing AirPlay ownership.
#[cfg(target_arch = "xtensa")]
const RTP_IDLE_TIMEOUT_MS: u64 = 3_000;

/// Metadata sizing for the AirPlay RTP audio UDP socket (port 6000).
#[cfg(target_arch = "xtensa")]
const RTP_UDP_RX_META: usize = 8;
#[cfg(target_arch = "xtensa")]
const RTP_UDP_TX_META: usize = 2;

#[cfg(target_arch = "xtensa")]
struct UdpRtpMetaBuffers {
    rx_meta: [udp::PacketMetadata; RTP_UDP_RX_META],
    tx_meta: [udp::PacketMetadata; RTP_UDP_TX_META],
}

#[cfg(target_arch = "xtensa")]
impl UdpRtpMetaBuffers {
    const fn new() -> Self {
        Self {
            rx_meta: [udp::PacketMetadata::EMPTY; RTP_UDP_RX_META],
            tx_meta: [udp::PacketMetadata::EMPTY; RTP_UDP_TX_META],
        }
    }
}

/// AirPlay 1 (RAOP) RTSP control and RTP audio receiver service.
pub struct AirPlayService {
    raop_rtsp: Option<Box<RtspServer>>,
    #[cfg(target_arch = "xtensa")]
    raop_rtp: Box<RtpAudioReceiver>,
    raop_streaming: bool,
    #[cfg(target_arch = "xtensa")]
    last_rtp_packet_ms: u64,
    rtsp_rx_acc: Vec<u8>,
    last_ip: Option<[u8; 4]>,
    #[cfg(target_arch = "xtensa")]
    rtsp_handle: Option<SocketHandle>,
    #[cfg(target_arch = "xtensa")]
    rtp_handle: Option<SocketHandle>,
}

impl Default for AirPlayService {
    fn default() -> Self {
        Self::new()
    }
}

impl AirPlayService {
    /// Creates a new AirPlay RAOP service with heap-backed state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            raop_rtsp: None,
            #[cfg(target_arch = "xtensa")]
            raop_rtp: Box::new(RtpAudioReceiver::new()),
            raop_streaming: false,
            #[cfg(target_arch = "xtensa")]
            last_rtp_packet_ms: 0,
            rtsp_rx_acc: Vec::with_capacity(2048),
            last_ip: None,
            #[cfg(target_arch = "xtensa")]
            rtsp_handle: None,
            #[cfg(target_arch = "xtensa")]
            rtp_handle: None,
        }
    }

    fn ensure_rtsp(&mut self, sta_mac: [u8; 6]) -> &mut RtspServer {
        self.raop_rtsp.get_or_insert_with(|| {
            Box::new(RtspServer::new(
                airport_express_key(),
                sta_mac,
                [0, 0, 0, 0],
            ))
        })
    }

    fn reset_transport(&mut self, ctx: &mut ServiceCtx<'_>) {
        #[cfg(target_arch = "xtensa")]
        {
            if ctx.has_sockets() {
                if let Some(handle) = self.rtsp_handle {
                    ctx.socket_mut::<tcp::Socket>(handle).abort();
                }
                if let Some(handle) = self.rtp_handle {
                    ctx.socket_mut::<udp::Socket>(handle).close();
                }
            }
            self.last_rtp_packet_ms = 0;
        }
        #[cfg(not(target_arch = "xtensa"))]
        let _ = ctx;
        self.raop_streaming = false;
        self.rtsp_rx_acc.clear();
    }

    #[cfg(target_arch = "xtensa")]
    fn sync_rtp_from_rtsp(&mut self, disable_if_missing_key: bool) {
        let Some(rtsp) = self.raop_rtsp.as_ref() else {
            return;
        };
        if let (Some(key), Some(iv)) = (rtsp.session_key(), rtsp.aes_iv()) {
            self.raop_rtp.configure_crypto(key, iv);
        } else if disable_if_missing_key {
            self.raop_rtp.disable_crypto();
        }
        if let Some(sdp) = rtsp.sdp_session() {
            if let Some(ref fmtp) = sdp.fmtp {
                let _ = self.raop_rtp.configure_alac(fmtp);
            }
        }
    }

    #[cfg(target_arch = "xtensa")]
    fn poll_rtsp(&mut self, ctx: &mut ServiceCtx<'_>) {
        let Some(rtsp_handle) = self.rtsp_handle else {
            return;
        };
        let now_ms = ctx.now_ms();
        let sta_mac = ctx.sta_mac();
        let _ = self.ensure_rtsp(sta_mac);

        let handled_method = {
            let rtsp_sock = ctx.socket_mut::<tcp::Socket>(rtsp_handle);
            if rtsp_sock.is_open()
                && matches!(
                    rtsp_sock.state(),
                    tcp::State::CloseWait
                        | tcp::State::TimeWait
                        | tcp::State::Closing
                        | tcp::State::LastAck
                        | tcp::State::FinWait1
                        | tcp::State::FinWait2
                )
            {
                rtsp_sock.abort();
                self.rtsp_rx_acc.clear();
            }
            if !rtsp_sock.is_open() {
                let _ = rtsp_sock.listen(AIRPLAY_RTSP_PORT);
                self.rtsp_rx_acc.clear();
            }
            if !(rtsp_sock.can_recv() && rtsp_sock.may_send()) {
                None
            } else {
                let mut chunk = [0u8; 512];
                let mut out_method = None;
                if let Ok(n) = rtsp_sock.recv_slice(&mut chunk) {
                    if n > 0 {
                        self.rtsp_rx_acc.extend_from_slice(&chunk[..n]);
                        if self.rtsp_rx_acc.len() > 4096 {
                            self.rtsp_rx_acc.clear();
                        }
                        if self.rtsp_rx_acc.windows(4).any(|w| w == b"\r\n\r\n") {
                            let parsed = if let Ok(req) = RtspRequest::parse(&self.rtsp_rx_acc) {
                                let body_start = if req.body.is_empty() {
                                    self.rtsp_rx_acc.len()
                                } else {
                                    req.body.as_ptr() as usize - self.rtsp_rx_acc.as_ptr() as usize
                                };
                                if self.rtsp_rx_acc.len() >= body_start + req.content_length {
                                    let method = req.method;
                                    let resp_res = self
                                        .raop_rtsp
                                        .as_mut()
                                        .map(|rtsp| rtsp.handle_request(&req));
                                    resp_res.map(|res| (method, res))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some((method, resp_res)) = parsed {
                                if let Ok(resp) = resp_res {
                                    let resp_bytes = resp.to_bytes();
                                    let _ = rtsp_sock.send_slice(&resp_bytes);
                                    out_method = Some(method);
                                }
                                self.rtsp_rx_acc.clear();
                            }
                        }
                    }
                }
                out_method
            }
        };

        let Some(method) = handled_method else {
            return;
        };
        match method {
            RtspMethod::Announce => {
                self.sync_rtp_from_rtsp(true);
            }
            RtspMethod::Record => {
                if ctx.claim_external_pcm(AudioSource::AirPlay, None, AmpPower::KeepCurrent) {
                    self.sync_rtp_from_rtsp(false);
                    self.raop_streaming = true;
                    self.last_rtp_packet_ms = now_ms;
                }
            }
            RtspMethod::SetParameter => {
                let raw_pct = self
                    .raop_rtsp
                    .as_ref()
                    .map_or(0, |rtsp| (rtsp.volume() * 100.0) as u8);
                if let Some(pct) = ctx.set_local_volume(raw_pct) {
                    ctx.config().update(|c| c.volume = pct);
                }
            }
            RtspMethod::Flush => {
                ctx.flush_external_buffers();
            }
            RtspMethod::Teardown => {
                self.raop_streaming = false;
                let _ = ctx.release_external(AudioSource::AirPlay);
            }
            _ => {}
        }
    }

    #[cfg(target_arch = "xtensa")]
    fn poll_rtp(&mut self, ctx: &mut ServiceCtx<'_>) {
        let Some(rtp_handle) = self.rtp_handle else {
            return;
        };
        let now_ms = ctx.now_ms();
        {
            let rtp_sock = ctx.socket_mut::<udp::Socket>(rtp_handle);
            if !rtp_sock.is_open() {
                let _ = rtp_sock.bind(AIRPLAY_RTP_PORT);
            }
        }
        let mut pkt_buf = [0u8; 1536];
        for _ in 0..4 {
            let pkt_len = {
                let rtp_sock = ctx.socket_mut::<udp::Socket>(rtp_handle);
                if !rtp_sock.can_recv() {
                    break;
                }
                match rtp_sock.recv_slice(&mut pkt_buf) {
                    Ok((n, _ep)) => Some(n),
                    Err(_) => None,
                }
            };
            let Some(n) = pkt_len else {
                break;
            };
            self.last_rtp_packet_ms = now_ms;
            if let Ok((pcm_i16, _hdr)) = self.raop_rtp.process_packet(&pkt_buf[..n]) {
                let granted = if self.raop_streaming {
                    ctx.request_external_source(AudioSource::AirPlay)
                } else {
                    let ok = ctx.claim_external_pcm(
                        AudioSource::AirPlay,
                        Some(44_100),
                        AmpPower::EnsurePlay,
                    );
                    if ok {
                        self.raop_streaming = true;
                    }
                    ok
                };
                if granted {
                    let pcm_bytes: &[u8] = bytemuck::cast_slice(pcm_i16);
                    let _ = ctx.ingest_external_pcm(pcm_bytes);
                }
            }
        }
        let rtsp_streaming = self
            .raop_rtsp
            .as_ref()
            .is_some_and(|rtsp| rtsp.state() == ServerState::Streaming);
        let rtp_timed_out = now_ms.saturating_sub(self.last_rtp_packet_ms) >= RTP_IDLE_TIMEOUT_MS;
        if self.raop_streaming
            && (!rtsp_streaming || rtp_timed_out)
            && !ctx.has_external_audio_queued()
        {
            self.raop_streaming = false;
            let _ = ctx.release_external(AudioSource::AirPlay);
        }
    }
}

impl DeviceService for AirPlayService {
    fn name(&self) -> &'static str {
        "airplay"
    }

    fn init(&mut self, ctx: &mut ServiceCtx<'_>) {
        let _ = self.ensure_rtsp(ctx.sta_mac());
        #[cfg(target_arch = "xtensa")]
        if ctx.has_sockets() {
            if self.rtsp_handle.is_none() {
                let rtsp_rx: &'static mut [u8] =
                    Box::leak(alloc::vec![0u8; 4096].into_boxed_slice());
                let rtsp_tx: &'static mut [u8] =
                    Box::leak(alloc::vec![0u8; 2048].into_boxed_slice());
                let rtsp_sock = tcp::Socket::new(
                    tcp::SocketBuffer::new(rtsp_rx),
                    tcp::SocketBuffer::new(rtsp_tx),
                );
                self.rtsp_handle = Some(ctx.add_socket(rtsp_sock));
            }
            if self.rtp_handle.is_none() {
                let rtp_meta = Box::leak(Box::new(UdpRtpMetaBuffers::new()));
                let rtp_rx_payload: &'static mut [u8] =
                    Box::leak(alloc::vec![0u8; 16_384].into_boxed_slice());
                let rtp_tx_payload: &'static mut [u8] =
                    Box::leak(alloc::vec![0u8; 512].into_boxed_slice());
                let rtp_sock = udp::Socket::new(
                    udp::PacketBuffer::new(&mut rtp_meta.rx_meta[..], rtp_rx_payload),
                    udp::PacketBuffer::new(&mut rtp_meta.tx_meta[..], rtp_tx_payload),
                );
                self.rtp_handle = Some(ctx.add_socket(rtp_sock));
            }
        }
    }

    fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool) {
        if !up {
            self.reset_transport(ctx);
            self.last_ip = None;
        } else if let Some(ip) = ctx.link_ip() {
            self.last_ip = Some(ip);
            let mac = ctx.sta_mac();
            self.ensure_rtsp(mac).set_ip_address(ip);
        }
    }

    fn poll(&mut self, ctx: &mut ServiceCtx<'_>) {
        let Some(local_ip) = ctx.link_ip() else {
            return;
        };
        if !ctx.config().is_airplay_enabled() {
            return;
        }
        if self.last_ip.is_some_and(|old| old != local_ip) {
            self.reset_transport(ctx);
        }
        self.last_ip = Some(local_ip);
        let sta_mac = ctx.sta_mac();
        self.ensure_rtsp(sta_mac).set_ip_address(local_ip);

        #[cfg(target_arch = "xtensa")]
        if ctx.has_sockets() {
            self.poll_rtsp(ctx);
            self.poll_rtp(ctx);
        }
    }

    fn status(&self, out: &mut StatusExtras) {
        out.push_bool("airplay_streaming", self.raop_streaming);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_handle::ConfigHandle;
    use crate::services::StatusValue;
    use squeezelite_config::AudioConfig;

    #[test]
    fn airplay_service_lifecycle_updates_ip_and_resets_on_link_down() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mac = [0x24, 0x0A, 0xC4, 0x11, 0x22, 0x33];
        let mut svc = AirPlayService::new();
        assert_eq!(svc.name(), "airplay");

        let mut init_ctx = ServiceCtx::new(0, None, &cfg).with_sta_mac(mac);
        svc.init(&mut init_ctx);
        assert!(svc.raop_rtsp.is_some());
        assert!(!svc.raop_streaming);

        let mut up_ctx = ServiceCtx::new(100, Some([172, 22, 1, 77]), &cfg).with_sta_mac(mac);
        svc.on_link(&mut up_ctx, true);
        assert_eq!(svc.last_ip, Some([172, 22, 1, 77]));

        svc.raop_streaming = true;
        svc.rtsp_rx_acc.extend_from_slice(b"OPTIONS * RTSP/1.0\r\n");

        // Changing IP during poll resets transient streaming and RX buffers.
        let mut changed_ctx = ServiceCtx::new(150, Some([172, 22, 1, 88]), &cfg).with_sta_mac(mac);
        svc.poll(&mut changed_ctx);
        assert_eq!(svc.last_ip, Some([172, 22, 1, 88]));
        assert!(!svc.raop_streaming);
        assert!(svc.rtsp_rx_acc.is_empty());

        svc.raop_streaming = true;
        let mut extras = StatusExtras::new();
        svc.status(&mut extras);
        assert_eq!(extras.len(), 1);
        assert_eq!(extras.fields()[0].key, "airplay_streaming");
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(true));

        let mut down_ctx = ServiceCtx::new(200, None, &cfg).with_sta_mac(mac);
        svc.on_link(&mut down_ctx, false);
        assert!(!svc.raop_streaming);
        assert_eq!(svc.last_ip, None);
        assert!(svc.rtsp_rx_acc.is_empty());

        extras.clear();
        svc.status(&mut extras);
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(false));
    }
}
