//! Multicast DNS (mDNS) responder packet builder for AirPlay 1 (RAOP).
//!
//! Generates RFC 6762 / RFC 6763 conforming binary DNS response records
//! announcing `_raop._tcp.local` and `_airplay._tcp.local` with device
//! parameters matching upstream `components/raop/raop_sink.c`.

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// Device and networking parameters for mDNS announcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MdnsConfig {
    /// Human-readable player name (e.g. "BASEMENT").
    pub player_name: String,
    /// Device 6-byte Ethernet / Wi-Fi MAC address.
    pub mac_address: [u8; 6],
    /// Device 4-byte IPv4 address.
    pub ip_address: [u8; 4],
    /// RTSP listening port (default 5000).
    pub port: u16,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            player_name: "BASEMENT".to_string(),
            mac_address: [0x24, 0x0A, 0xC4, 0x01, 0x02, 0x03],
            ip_address: [192, 168, 1, 50],
            port: 5000,
        }
    }
}

impl MdnsConfig {
    /// Formats MAC address as 12 uppercase hexadecimal characters without colons.
    pub fn mac_hex(&self) -> String {
        format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.mac_address[0],
            self.mac_address[1],
            self.mac_address[2],
            self.mac_address[3],
            self.mac_address[4],
            self.mac_address[5]
        )
    }

    /// Fully qualified RAOP service instance name: `<MAC>@<NAME>._raop._tcp.local`.
    pub fn raop_instance_name(&self) -> String {
        format!("{}@{}.{}", self.mac_hex(), self.player_name, "_raop._tcp.local")
    }

    /// Fully qualified AirPlay service instance name: `<NAME>._airplay._tcp.local`.
    pub fn airplay_instance_name(&self) -> String {
        format!("{}.{}", self.player_name, "_airplay._tcp.local")
    }

    /// Target host name: `<NAME>.local`.
    pub fn host_name(&self) -> String {
        format!("{}.local", self.player_name)
    }
}

/// Binary mDNS response packet encoder.
pub struct MdnsBuilder;

impl MdnsBuilder {
    /// Encodes a dotted domain name into length-prefixed DNS wire labels
    /// ending with a zero byte.
    pub fn encode_dns_name(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for label in name.split('.') {
            if label.is_empty() {
                continue;
            }
            let bytes = label.as_bytes();
            let len = core::cmp::min(bytes.len(), 63) as u8;
            out.push(len);
            out.extend_from_slice(&bytes[..len as usize]);
        }
        out.push(0);
        out
    }

    /// Appends a PTR resource record.
    fn append_ptr_record(
        records: &mut Vec<u8>,
        service_type: &str,
        instance_target: &str,
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(service_type));
        records.extend_from_slice(&12u16.to_be_bytes()); // Type: PTR
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (cache-flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let target_bytes = Self::encode_dns_name(instance_target);
        records.extend_from_slice(&(target_bytes.len() as u16).to_be_bytes());
        records.extend_from_slice(&target_bytes);
    }

    /// Appends an SRV resource record.
    fn append_srv_record(
        records: &mut Vec<u8>,
        instance_name: &str,
        target_host: &str,
        port: u16,
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(instance_name));
        records.extend_from_slice(&33u16.to_be_bytes()); // Type: SRV
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (cache-flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let target_bytes = Self::encode_dns_name(target_host);
        let rd_len = 6 + target_bytes.len() as u16;
        records.extend_from_slice(&rd_len.to_be_bytes());

        records.extend_from_slice(&0u16.to_be_bytes()); // Priority
        records.extend_from_slice(&0u16.to_be_bytes()); // Weight
        records.extend_from_slice(&port.to_be_bytes()); // Port
        records.extend_from_slice(&target_bytes);
    }

    /// Appends a TXT resource record containing key-value attribute strings.
    fn append_txt_record(
        records: &mut Vec<u8>,
        instance_name: &str,
        entries: &[&str],
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(instance_name));
        records.extend_from_slice(&16u16.to_be_bytes()); // Type: TXT
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (cache-flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let mut txt_data = Vec::new();
        for &entry in entries {
            let bytes = entry.as_bytes();
            let len = core::cmp::min(bytes.len(), 255) as u8;
            txt_data.push(len);
            txt_data.extend_from_slice(&bytes[..len as usize]);
        }
        if txt_data.is_empty() {
            txt_data.push(0);
        }

        records.extend_from_slice(&(txt_data.len() as u16).to_be_bytes());
        records.extend_from_slice(&txt_data);
    }

    /// Appends an A (IPv4) host address resource record.
    fn append_a_record(
        records: &mut Vec<u8>,
        host_name: &str,
        ip: &[u8; 4],
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(host_name));
        records.extend_from_slice(&1u16.to_be_bytes()); // Type: A
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (cache-flush)
        records.extend_from_slice(&ttl.to_be_bytes());
        records.extend_from_slice(&4u16.to_be_bytes()); // Length: 4
        records.extend_from_slice(ip);
    }

    /// Builds a full mDNS announcement UDP response packet announcing RAOP
    /// and AirPlay services.
    pub fn build_announcement(config: &MdnsConfig) -> Vec<u8> {
        let mut answers = Vec::new();
        let ttl = 4500u32; // 75 minutes standard mDNS TTL

        // 1. RAOP PTR: _raop._tcp.local -> <MAC>@<NAME>._raop._tcp.local
        let raop_inst = config.raop_instance_name();
        Self::append_ptr_record(&mut answers, "_raop._tcp.local", &raop_inst, ttl);

        // 2. AirPlay PTR: _airplay._tcp.local -> <NAME>._airplay._tcp.local
        let airplay_inst = config.airplay_instance_name();
        Self::append_ptr_record(&mut answers, "_airplay._tcp.local", &airplay_inst, ttl);

        // 3. RAOP SRV
        let host = config.host_name();
        Self::append_srv_record(&mut answers, &raop_inst, &host, config.port, ttl);

        // 4. RAOP TXT matching upstream C raop.c / raop_sink.c
        const RAOP_TXT: &[&str] = &[
            "txtvers=1",
            "ch=2",
            "cn=0,1",
            "et=0,1",
            "sv=false",
            "da=true",
            "sr=44100",
            "ss=16",
            "vn=3",
            "tp=UDP",
            "md=0,1,2",
            "am=airesp32",
            "sm=false",
            "ek=1",
        ];
        Self::append_txt_record(&mut answers, &raop_inst, RAOP_TXT, ttl);

        // 5. AirPlay SRV
        Self::append_srv_record(&mut answers, &airplay_inst, &host, config.port, ttl);

        // 6. AirPlay TXT
        const AIRPLAY_TXT: &[&str] = &[
            "model=AirPort4,107",
            "srcvers=220.68",
        ];
        Self::append_txt_record(&mut answers, &airplay_inst, AIRPLAY_TXT, ttl);

        // 7. A record: <NAME>.local -> IPv4
        Self::append_a_record(&mut answers, &host, &config.ip_address, ttl);

        let answer_count = 7u16;

        // Construct 12-byte DNS header
        let mut packet = Vec::with_capacity(12 + answers.len());
        packet.extend_from_slice(&0u16.to_be_bytes()); // Transaction ID: 0
        // Flags: Standard response (0x8000) | Authoritative (0x0400)
        packet.extend_from_slice(&0x8400u16.to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes()); // Questions: 0
        packet.extend_from_slice(&answer_count.to_be_bytes()); // Answers: 7
        packet.extend_from_slice(&0u16.to_be_bytes()); // Authority RRs: 0
        packet.extend_from_slice(&0u16.to_be_bytes()); // Additional RRs: 0

        packet.extend_from_slice(&answers);
        packet
    }
}
