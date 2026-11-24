//! mDNS responder service (`UDP 5353`) for AirPlay and Spotify Connect.

extern crate alloc;

#[cfg(target_arch = "xtensa")]
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(target_arch = "xtensa")]
use smoltcp::iface::SocketHandle;
#[cfg(target_arch = "xtensa")]
use smoltcp::socket::udp;
#[cfg(target_arch = "xtensa")]
use smoltcp::wire::IpAddress;
#[cfg(target_arch = "xtensa")]
use smoltcp::wire::IpEndpoint;
use squeezelite_cspot::zeroconf::SpotifyMdnsBuilder;
use squeezelite_cspot::zeroconf::SpotifyMdnsConfig;
use squeezelite_raop::mdns::MdnsBuilder;
use squeezelite_raop::mdns::MdnsConfig;

use crate::board::first_boot_defaults::PLAYER_NAME;
use crate::config_handle::ConfigHandle;
use crate::services::DeviceService;
use crate::services::ServiceCtx;
use crate::services::StatusExtras;

/// Minimum interval between multicast mDNS replies in milliseconds.
pub const MDNS_MULTICAST_COOLDOWN_MS: u64 = 1_000;

/// Maximum cached AirPlay RAOP mDNS packet size.
const MAX_RAOP_MDNS_LEN: usize = 512;

/// Maximum cached Spotify Connect mDNS packet size.
const MAX_SPOT_MDNS_LEN: usize = 384;

/// Periodic unsolicited mDNS announcement interval in milliseconds.
#[cfg(target_arch = "xtensa")]
const MDNS_ANNOUNCE_INTERVAL_MS: u64 = 30_000;

#[cfg(target_arch = "xtensa")]
const MDNS_UDP_META: usize = 4;

#[cfg(target_arch = "xtensa")]
const MDNS_UDP_PAYLOAD: usize = 1024;

#[cfg(target_arch = "xtensa")]
struct UdpMdnsBuffers {
    rx_meta: [udp::PacketMetadata; MDNS_UDP_META],
    rx_payload: [u8; MDNS_UDP_PAYLOAD],
    tx_meta: [udp::PacketMetadata; MDNS_UDP_META],
    tx_payload: [u8; MDNS_UDP_PAYLOAD],
}

#[cfg(target_arch = "xtensa")]
impl UdpMdnsBuffers {
    const fn new() -> Self {
        Self {
            rx_meta: [udp::PacketMetadata::EMPTY; MDNS_UDP_META],
            rx_payload: [0u8; MDNS_UDP_PAYLOAD],
            tx_meta: [udp::PacketMetadata::EMPTY; MDNS_UDP_META],
            tx_payload: [0u8; MDNS_UDP_PAYLOAD],
        }
    }
}

/// Returns `true` when `payload` is an mDNS service query (`QR == 0`,
/// non-zero `QDCOUNT`) targeting AirPlay or Spotify Connect labels.
#[must_use]
pub fn is_mdns_service_query(payload: &[u8]) -> bool {
    if payload.len() < 12 {
        return false;
    }
    if (payload[2] & 0x80) != 0 {
        return false;
    }
    let qdcount = u16::from_be_bytes([payload[4], payload[5]]);
    if qdcount == 0 {
        return false;
    }
    let body = &payload[12..];
    is_airplay_query_body(body) || is_spotify_query_body(body)
}

fn is_airplay_query_body(body: &[u8]) -> bool {
    body.windows(5).any(|w| w.eq_ignore_ascii_case(b"_raop"))
        || body.windows(8).any(|w| w.eq_ignore_ascii_case(b"_airplay"))
}

fn is_spotify_query_body(body: &[u8]) -> bool {
    body.windows(16)
        .any(|w| w.eq_ignore_ascii_case(b"_spotify-connect"))
        || body.windows(8).any(|w| w.eq_ignore_ascii_case(b"_spotify"))
}

/// Returns `true` if a multicast mDNS reply may be sent at `now_ms`.
#[must_use]
pub const fn can_send_mdns_multicast_reply(last_reply_ms: Option<u64>, now_ms: u64) -> bool {
    match last_reply_ms {
        None => true,
        Some(prev_ms) => now_ms.saturating_sub(prev_ms) >= MDNS_MULTICAST_COOLDOWN_MS,
    }
}

fn resolve_cspot_name(config: &ConfigHandle) -> heapless::String<32> {
    let mut name = config.effective_cspot_name();
    if name.trim().is_empty() {
        name.clear();
        let _ = name.push_str(PLAYER_NAME);
    }
    name
}

fn resolve_raop_name(config: &ConfigHandle) -> heapless::String<32> {
    let mut name = config
        .nvs_override("host_name")
        .and_then(|h| {
            let t = h.trim();
            if t.is_empty() {
                None
            } else {
                let mut out = heapless::String::<32>::new();
                let max_len = t.floor_char_boundary(t.len().min(32));
                let _ = out.push_str(&t[..max_len]);
                Some(out)
            }
        })
        .unwrap_or_else(|| {
            config.read(|c| {
                let t = c.player_name.trim();
                if !t.is_empty() {
                    let mut out = heapless::String::<32>::new();
                    let max_len = t.floor_char_boundary(t.len().min(32));
                    let _ = out.push_str(&t[..max_len]);
                    out
                } else {
                    config.device_name()
                }
            })
        });
    if name.trim().is_empty() {
        name.clear();
        let _ = name.push_str(PLAYER_NAME);
    }
    name
}

/// Device service managing `UDP 5353` mDNS announcements and query replies.
pub struct MdnsService {
    bound: bool,
    last_announce_ms: u64,
    last_reply_ms: Option<u64>,
    cached_raop_mdns: Vec<u8>,
    cached_spot_mdns: Vec<u8>,
    cached_ip: [u8; 4],
    cached_raop_name: heapless::String<32>,
    cached_cspot_name: heapless::String<32>,
    #[cfg(target_arch = "xtensa")]
    handle: Option<SocketHandle>,
}

impl Default for MdnsService {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsService {
    /// Creates an unbound [`MdnsService`] with empty packet caches.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bound: false,
            last_announce_ms: 0,
            last_reply_ms: None,
            cached_raop_mdns: Vec::new(),
            cached_spot_mdns: Vec::new(),
            cached_ip: [0; 4],
            cached_raop_name: heapless::String::new(),
            cached_cspot_name: heapless::String::new(),
            #[cfg(target_arch = "xtensa")]
            handle: None,
        }
    }

    fn refresh_caches(
        &mut self,
        local_ip: [u8; 4],
        sta_mac: [u8; 6],
        config: &ConfigHandle,
        airplay_enabled: bool,
        cspot_enabled: bool,
    ) -> bool {
        let raop_name = resolve_raop_name(config);
        let cspot_name = resolve_cspot_name(config);
        let ip_changed = self.cached_ip != [0, 0, 0, 0] && local_ip != self.cached_ip;
        let ip_or_name_changed = local_ip != self.cached_ip
            || raop_name.as_str() != self.cached_raop_name.as_str()
            || cspot_name.as_str() != self.cached_cspot_name.as_str();
        if ip_or_name_changed {
            self.cached_raop_mdns.clear();
            self.cached_spot_mdns.clear();
            self.cached_ip = local_ip;
            self.cached_raop_name = raop_name.clone();
            self.cached_cspot_name = cspot_name.clone();
        }
        if airplay_enabled && self.cached_raop_mdns.is_empty() {
            let mcfg = MdnsConfig {
                player_name: String::from(raop_name.as_str()),
                mac_address: sta_mac,
                ip_address: local_ip,
                port: 5000,
            };
            let mut rpkt = MdnsBuilder::build_announcement(&mcfg);
            rpkt.truncate(MAX_RAOP_MDNS_LEN);
            self.cached_raop_mdns = rpkt;
        }
        if cspot_enabled && self.cached_spot_mdns.is_empty() {
            let scfg = SpotifyMdnsConfig {
                player_name: String::from(cspot_name.as_str()),
                mac_address: sta_mac,
                ip_address: local_ip,
                port: 80,
            };
            let mut spkt = SpotifyMdnsBuilder::build_announcement(&scfg);
            spkt.truncate(MAX_SPOT_MDNS_LEN);
            self.cached_spot_mdns = spkt;
        }
        ip_changed
    }
}

impl DeviceService for MdnsService {
    fn name(&self) -> &'static str {
        "mdns"
    }

    fn init(&mut self, ctx: &mut ServiceCtx<'_>) {
        #[cfg(target_arch = "xtensa")]
        if self.handle.is_none() && ctx.has_sockets() {
            let mdns_bufs = Box::leak(Box::new(UdpMdnsBuffers::new()));
            let sock = udp::Socket::new(
                udp::PacketBuffer::new(&mut mdns_bufs.rx_meta[..], &mut mdns_bufs.rx_payload[..]),
                udp::PacketBuffer::new(&mut mdns_bufs.tx_meta[..], &mut mdns_bufs.tx_payload[..]),
            );
            self.handle = Some(ctx.add_socket(sock));
        }
        #[cfg(not(target_arch = "xtensa"))]
        let _ = ctx;
    }

    fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool) {
        if !up {
            #[cfg(target_arch = "xtensa")]
            if let Some(handle) = self.handle {
                if ctx.has_sockets() {
                    ctx.socket_mut::<udp::Socket>(handle).close();
                }
            }
            #[cfg(not(target_arch = "xtensa"))]
            let _ = ctx;
            self.bound = false;
            self.last_reply_ms = None;
            self.last_announce_ms = 0;
        } else {
            let _ = ctx;
            self.last_announce_ms = 0;
        }
    }

    fn poll(&mut self, ctx: &mut ServiceCtx<'_>) {
        let Some(local_ip) = ctx.link_ip() else {
            return;
        };
        let airplay_enabled = ctx.config().is_airplay_enabled();
        let cspot_enabled = ctx.config().is_cspot_enabled();
        if !airplay_enabled && !cspot_enabled {
            return;
        }
        let ip_changed = self.refresh_caches(
            local_ip,
            ctx.sta_mac(),
            ctx.config(),
            airplay_enabled,
            cspot_enabled,
        );

        #[cfg(target_arch = "xtensa")]
        {
            let Some(handle) = self.handle else {
                return;
            };
            if !ctx.has_sockets() {
                return;
            }
            let now_ms = ctx.now_ms();
            let mdns_sock = ctx.socket_mut::<udp::Socket>(handle);
            if ip_changed {
                mdns_sock.close();
                self.bound = false;
                self.last_reply_ms = None;
                self.last_announce_ms = 0;
            }
            if !self.bound && mdns_sock.bind(5353).is_ok() {
                self.bound = true;
            }
            if !self.bound {
                return;
            }

            let multicast_ep = IpEndpoint::new(IpAddress::v4(224, 0, 0, 251), 5353);
            let mut should_announce_raop = false;
            let mut should_announce_spotify = false;
            let mut raop_ep = multicast_ep;
            let mut spot_ep = multicast_ep;

            if self.last_announce_ms == 0
                || now_ms.saturating_sub(self.last_announce_ms) >= MDNS_ANNOUNCE_INTERVAL_MS
            {
                self.last_announce_ms = now_ms.max(1);
                should_announce_raop = airplay_enabled;
                should_announce_spotify = cspot_enabled;
            }

            for _ in 0..4 {
                if !mdns_sock.can_recv() {
                    break;
                }
                let Ok((payload, meta)) = mdns_sock.recv() else {
                    break;
                };
                if !is_mdns_service_query(payload) {
                    continue;
                }
                let body = &payload[12..];
                if airplay_enabled && is_airplay_query_body(body) {
                    should_announce_raop = true;
                    if meta.endpoint.port != 5353 {
                        raop_ep = meta.endpoint;
                    }
                }
                if cspot_enabled && is_spotify_query_body(body) {
                    should_announce_spotify = true;
                    if meta.endpoint.port != 5353 {
                        spot_ep = meta.endpoint;
                    }
                }
            }

            let multicast_allowed = can_send_mdns_multicast_reply(self.last_reply_ms, now_ms);
            let mut sent_multicast = false;
            if airplay_enabled
                && should_announce_raop
                && !self.cached_raop_mdns.is_empty()
                && mdns_sock.can_send()
                && (raop_ep != multicast_ep || multicast_allowed)
                && mdns_sock
                    .send_slice(&self.cached_raop_mdns, raop_ep)
                    .is_ok()
                && raop_ep == multicast_ep
            {
                sent_multicast = true;
            }
            if cspot_enabled
                && should_announce_spotify
                && !self.cached_spot_mdns.is_empty()
                && mdns_sock.can_send()
                && (spot_ep != multicast_ep || multicast_allowed)
                && mdns_sock
                    .send_slice(&self.cached_spot_mdns, spot_ep)
                    .is_ok()
                && spot_ep == multicast_ep
            {
                sent_multicast = true;
            }
            if sent_multicast {
                self.last_reply_ms = Some(now_ms);
            }
        }
        #[cfg(not(target_arch = "xtensa"))]
        {
            if ip_changed {
                self.bound = false;
                self.last_reply_ms = None;
                self.last_announce_ms = 0;
            }
        }
    }

    fn status(&self, out: &mut StatusExtras) {
        out.push_bool("mdns_bound", self.bound);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::StatusValue;
    use squeezelite_config::AudioConfig;

    #[test]
    fn test_mdns_service_query_drops_short_payloads() {
        assert!(!is_mdns_service_query(&[]));
        assert!(!is_mdns_service_query(&[0u8; 11]));
    }

    #[test]
    fn test_mdns_service_query_drops_qr1_responses() {
        let mut pkt = vec![
            0x00, 0x00, 0x84, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        pkt.extend_from_slice(b"\x05_raop\x04_tcp\x05local\x00");
        assert!(
            !is_mdns_service_query(&pkt),
            "mDNS response with QR=1 must be dropped"
        );
    }

    #[test]
    fn test_mdns_service_query_accepts_qr0_service_queries() {
        let mut raop_q = vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        raop_q.extend_from_slice(b"\x05_raop\x04_tcp\x05local\x00");
        assert!(
            is_mdns_service_query(&raop_q),
            "mDNS query with QR=0 and QDCOUNT=1 for _raop must pass"
        );

        let mut spot_q = vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        spot_q.extend_from_slice(b"\x10_spotify-connect\x04_tcp\x05local\x00");
        assert!(
            is_mdns_service_query(&spot_q),
            "mDNS query with QR=0 and QDCOUNT=1 for _spotify-connect must pass"
        );

        raop_q[5] = 0x00;
        assert!(
            !is_mdns_service_query(&raop_q),
            "mDNS query with QDCOUNT=0 must be dropped"
        );
    }

    #[test]
    fn test_mdns_multicast_reply_rate_limit_1000ms() {
        assert!(can_send_mdns_multicast_reply(None, 0));
        assert!(can_send_mdns_multicast_reply(None, 500));
        assert!(!can_send_mdns_multicast_reply(Some(1_000), 1_000));
        assert!(!can_send_mdns_multicast_reply(Some(1_000), 1_999));
        assert!(can_send_mdns_multicast_reply(Some(1_000), 2_000));
        assert!(can_send_mdns_multicast_reply(Some(1_000), 5_000));
    }

    #[test]
    fn mdns_service_lifecycle_caches_and_resets_on_link_down() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mut svc = MdnsService::new();
        assert_eq!(svc.name(), "mdns");

        let mut init_ctx = ServiceCtx::new(0, None, &cfg).with_sta_mac(mac);
        svc.init(&mut init_ctx);
        assert!(!svc.bound);

        let mut up_ctx = ServiceCtx::new(100, Some([172, 22, 1, 50]), &cfg).with_sta_mac(mac);
        svc.on_link(&mut up_ctx, true);
        svc.poll(&mut up_ctx);
        assert_eq!(svc.cached_ip, [172, 22, 1, 50]);
        assert!(!svc.cached_raop_mdns.is_empty());
        assert!(!svc.cached_spot_mdns.is_empty());

        svc.bound = true;
        svc.last_reply_ms = Some(150);
        svc.last_announce_ms = 100;

        let mut extras = StatusExtras::new();
        svc.status(&mut extras);
        assert_eq!(extras.len(), 1);
        assert_eq!(extras.fields()[0].key, "mdns_bound");
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(true));

        let mut down_ctx = ServiceCtx::new(200, None, &cfg).with_sta_mac(mac);
        svc.on_link(&mut down_ctx, false);
        assert!(!svc.bound);
        assert_eq!(svc.last_reply_ms, None);
        assert_eq!(svc.last_announce_ms, 0);

        extras.clear();
        svc.status(&mut extras);
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(false));
    }
}
