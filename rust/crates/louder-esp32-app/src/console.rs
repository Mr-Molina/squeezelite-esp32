//! Pure-Rust `no_std` Console CLI Registry, Startup `AutoexecRunner`,
//! Bounded `SyslogRingBuffer`, and Port 23 `TelnetSession` Dispatcher.
//!
//! Implements the Phase 3 Console/CLI and Telnet parity requirements:
//! - Built-in command registry (`system`, `i2c`, `config`, `join`, `update`,
//!   `player`, `sleep`, `help`).
//! - `AutoexecRunner` for executing `;`- or `\n`-delimited startup commands.
//! - `SyslogRingBuffer` circular log storage for Telnet stdout mirroring and
//!   `/messages.json`.
//! - `TelnetSession` (`TCP 23`) RFC 854 IAC negotiation stripper and line
//!   command dispatcher.

use core::fmt::Write;
use heapless::{String, Vec};

/// Standard TCP port for the on-device Telnet console server.
pub const TELNET_PORT: u16 = 23;

/// Telnet Interpret As Command (`IAC`) escape byte (`0xFF`, RFC 854).
pub const TELNET_IAC: u8 = 0xFF;
/// Telnet Subnegotiation End (`SE`) byte (`0xF0`).
pub const TELNET_SE: u8 = 0xF0;
/// Telnet Subnegotiation Begin (`SB`) byte (`0xFA`).
pub const TELNET_SB: u8 = 0xFA;
/// Telnet `WILL` option negotiation byte (`0xFB`).
pub const TELNET_WILL: u8 = 0xFB;
/// Telnet `WONT` option negotiation byte (`0xFC`).
pub const TELNET_WONT: u8 = 0xFC;
/// Telnet `DO` option negotiation byte (`0xFD`).
pub const TELNET_DO: u8 = 0xFD;
/// Telnet `DONT` option negotiation byte (`0xFE`).
pub const TELNET_DONT: u8 = 0xFE;

/// Default interactive prompt emitted after Telnet command execution.
pub const TELNET_PROMPT: &str = "louder> ";

/// Low-power sleep mode requested via the `sleep` console command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepMode {
    /// Light sleep preserving SRAM state.
    Light,
    /// Deep sleep waking via RTC timer or GPIO wakeup trigger.
    Deep,
}

/// Player transport action requested via the `player` console command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCliAction {
    /// Start or resume playback.
    Play,
    /// Pause active playback.
    Pause,
    /// Stop playback and mute output.
    Stop,
    /// Set volume percentage (`0..=100`).
    Volume(u8),
    /// Query current player status.
    Status,
}

/// Structured side-effect produced by executing a console CLI command.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliSideEffect {
    /// Pure informational command (`help`, `system info`, `system heap`).
    None,
    /// Perform a normal system reboot (`ota_0`).
    Reboot,
    /// Reboot into factory recovery partition (`0x10000`).
    RebootToRecovery,
    /// Scan hardware `I2C0` bus (`0x08..=0x77`).
    I2cScan,
    /// Read a single register over hardware `I2C0`.
    I2cRead {
        /// 7-bit I2C slave address.
        addr: u8,
        /// 8-bit register address.
        reg: u8,
    },
    /// Write a single register over hardware `I2C0`.
    I2cWrite {
        /// 7-bit I2C slave address.
        addr: u8,
        /// 8-bit register address.
        reg: u8,
        /// 8-bit register value to write.
        val: u8,
    },
    /// Read a key from NVS / FlashJournal config store.
    ConfigGet {
        /// Configuration key name.
        key: String<32>,
    },
    /// Write a key-value pair to NVS / FlashJournal config store.
    ConfigSet {
        /// Configuration key name.
        key: String<32>,
        /// New configuration value.
        value: String<64>,
    },
    /// Erase a key from NVS / FlashJournal config store.
    ConfigErase {
        /// Configuration key name.
        key: String<32>,
    },
    /// Dump all active configuration keys.
    ConfigDump,
    /// Associate with a Wi-Fi AP (`join <ssid> [password]`).
    WifiJoin {
        /// Target Wi-Fi SSID.
        ssid: String<32>,
        /// Optional WPA2 passphrase.
        password: String<64>,
    },
    /// Trigger streaming OTA update from `<url>`.
    OtaUpdate {
        /// HTTP/HTTPS firmware binary URL.
        url: String<128>,
    },
    /// Control audio player transport or volume.
    PlayerControl(PlayerCliAction),
    /// Enter low-power sleep for `duration_secs`.
    EnterSleep {
        /// Requested sleep mode (`Light` or `Deep`).
        mode: SleepMode,
        /// Sleep duration in seconds.
        duration_secs: u32,
    },
}

/// Result of dispatching a single CLI command line.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommandResult {
    /// Whether the command parsed and executed without error.
    pub ok: bool,
    /// Formatted response text for console/Telnet/REST output.
    pub output: String<256>,
    /// Side-effect to be applied by the hardware coordinator.
    pub side_effect: CliSideEffect,
}

/// Metadata descriptor for a registered CLI command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliCommandDescriptor {
    /// Command name (`"system"`, `"i2c"`, etc.).
    pub name: &'static str,
    /// Usage synopsis.
    pub usage: &'static str,
    /// Short description of command behavior.
    pub summary: &'static str,
}

/// Static catalog of the 8 built-in console commands.
pub const BUILTIN_CLI_COMMANDS: [CliCommandDescriptor; 8] = [
    CliCommandDescriptor {
        name: "system",
        usage: "system <info|heap|reboot|recovery>",
        summary: "System status, PSRAM heap stats, and reboot controls",
    },
    CliCommandDescriptor {
        name: "i2c",
        usage: "i2c <scan|get <addr> <reg>|set <addr> <reg> <val>>",
        summary: "Inspect or modify I2C0 peripheral registers",
    },
    CliCommandDescriptor {
        name: "config",
        usage: "config <get <key>|set <key> <val>|erase <key>|dump>",
        summary: "Read, write, or erase NVS / FlashJournal settings",
    },
    CliCommandDescriptor {
        name: "join",
        usage: "join <ssid> [password]",
        summary: "Connect to a Wi-Fi access point and persist credentials",
    },
    CliCommandDescriptor {
        name: "update",
        usage: "update <http(s)://url/firmware.bin>",
        summary: "Stream and verify an OTA firmware image into ota_0",
    },
    CliCommandDescriptor {
        name: "player",
        usage: "player <play|pause|stop|status|vol <0..100>>",
        summary: "Control audio playback and amplifier volume",
    },
    CliCommandDescriptor {
        name: "sleep",
        usage: "sleep [light|deep] <seconds>",
        summary: "Enter light or deep power-saving sleep mode",
    },
    CliCommandDescriptor {
        name: "help",
        usage: "help",
        summary: "Display all available console commands",
    },
];

fn parse_u8_hex_or_dec(token: &str) -> Option<u8> {
    let trimmed = token.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u8>().ok()
    }
}

fn tokenize_cli_line(line: &str) -> Vec<&str, 8> {
    let mut tokens: Vec<&str, 8> = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() && tokens.len() < 8 {
        if let Some(after_quote) = rest.strip_prefix('"') {
            if let Some(end_q) = after_quote.find('"') {
                let _ = tokens.push(&after_quote[..end_q]);
                rest = after_quote[end_q + 1..].trim_start();
            } else {
                let _ = tokens.push(after_quote);
                break;
            }
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let _ = tokens.push(&rest[..end]);
            rest = rest[end..].trim_start();
        }
    }
    tokens
}

/// Pure-Rust `no_std` CLI Command Registry and Dispatcher.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct CliRegistry;

impl CliRegistry {
    /// Creates a new [`CliRegistry`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns the static descriptor table of built-in commands.
    #[must_use]
    pub const fn commands(&self) -> &'static [CliCommandDescriptor] {
        &BUILTIN_CLI_COMMANDS
    }

    /// Parses and executes a single CLI command line.
    #[must_use]
    pub fn execute(&self, raw_line: &str) -> CliCommandResult {
        let trimmed = raw_line.trim();
        let mut out: String<256> = String::new();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return CliCommandResult {
                ok: true,
                output: out,
                side_effect: CliSideEffect::None,
            };
        }

        let tokens = tokenize_cli_line(trimmed);
        let cmd = tokens.first().copied().unwrap_or("");
        match cmd {
            "help" | "?" => {
                let _ = out.push_str("Available commands:\n");
                for desc in &BUILTIN_CLI_COMMANDS {
                    let _ = writeln!(out, "  {:8} - {}", desc.name, desc.usage);
                }
                CliCommandResult {
                    ok: true,
                    output: out,
                    side_effect: CliSideEffect::None,
                }
            }
            "system" => self.exec_system(&tokens, &mut out),
            "reboot" | "restart" => self.exec_system(&["system", "reboot"], &mut out),
            "i2c" => self.exec_i2c(&tokens, &mut out),
            "config" | "nvs" => self.exec_config(&tokens, &mut out),
            "join" => self.exec_join(&tokens, &mut out),
            "update" => self.exec_update(&tokens, &mut out),
            "ota" => self.exec_update(&tokens, &mut out),
            "player" => self.exec_player(&tokens, &mut out),
            "sleep" => self.exec_sleep(&tokens, &mut out),
            _ => {
                let _ = write!(out, "ERR: unknown command '{}' (type 'help')", cmd);
                CliCommandResult {
                    ok: false,
                    output: out,
                    side_effect: CliSideEffect::None,
                }
            }
        }
    }

    fn exec_system(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let sub = tokens
            .get(1)
            .copied()
            .unwrap_or_else(|| match tokens.first().copied() {
                Some("reboot" | "restart") => "reboot",
                _ => "info",
            });
        match sub {
            "info" => {
                let _ = out.push_str("OK: Squeezelite-ESP32 (I2S-4MFlash-Rust) pure-Rust engine");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            "heap" => {
                let _ = out.push_str("OK: PSRAM 4194304B total, rings 2572288B");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            "reboot" | "restart" => {
                let _ = out.push_str("OK: rebooting to ota_0");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::Reboot,
                }
            }
            "recovery" | "factory" => {
                let _ = out.push_str("OK: rebooting to factory recovery (0x10000)");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::RebootToRecovery,
                }
            }
            _ => {
                let _ = out.push_str("ERR: usage: system <info|heap|reboot|recovery>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
        }
    }

    fn exec_i2c(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let sub = tokens.get(1).copied().unwrap_or("");
        match sub {
            "scan" => {
                let _ = out.push_str("OK: scanning I2C0 (0x08..0x77)");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::I2cScan,
                }
            }
            "get" | "read" => {
                if let (Some(a_str), Some(r_str)) = (tokens.get(2), tokens.get(3)) {
                    if let (Some(addr), Some(reg)) =
                        (parse_u8_hex_or_dec(a_str), parse_u8_hex_or_dec(r_str))
                    {
                        let _ = write!(out, "OK: i2c read dev=0x{:02X} reg=0x{:02X}", addr, reg);
                        return CliCommandResult {
                            ok: true,
                            output: out.clone(),
                            side_effect: CliSideEffect::I2cRead { addr, reg },
                        };
                    }
                }
                let _ = out.push_str("ERR: usage: i2c get <addr> <reg>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            "set" | "write" => {
                if let (Some(a_str), Some(r_str), Some(v_str)) =
                    (tokens.get(2), tokens.get(3), tokens.get(4))
                {
                    if let (Some(addr), Some(reg), Some(val)) = (
                        parse_u8_hex_or_dec(a_str),
                        parse_u8_hex_or_dec(r_str),
                        parse_u8_hex_or_dec(v_str),
                    ) {
                        let _ = write!(
                            out,
                            "OK: i2c write dev=0x{:02X} reg=0x{:02X} val=0x{:02X}",
                            addr, reg, val
                        );
                        return CliCommandResult {
                            ok: true,
                            output: out.clone(),
                            side_effect: CliSideEffect::I2cWrite { addr, reg, val },
                        };
                    }
                }
                let _ = out.push_str("ERR: usage: i2c set <addr> <reg> <val>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            _ => {
                let _ = out.push_str("ERR: usage: i2c <scan|get|set>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
        }
    }

    fn exec_config(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let is_sensitive = |k: &str| -> bool {
            let contains_ci = |needle: &str| -> bool {
                k.as_bytes().windows(needle.len()).any(|w| {
                    w.iter()
                        .zip(needle.as_bytes())
                        .all(|(&b1, &b2)| b1.eq_ignore_ascii_case(&b2))
                })
            };
            k.eq_ignore_ascii_case("wifi.pass")
                || k.eq_ignore_ascii_case("wifi_pass")
                || contains_ci("pass")
                || contains_ci("secret")
                || contains_ci("token")
        };
        let sub = tokens.get(1).copied().unwrap_or("");
        match sub {
            "dump" | "list" => {
                if tokens.len() > 2 {
                    let _ = out.push_str("OK: config dump:");
                    for &item in &tokens[2..] {
                        if let Some((k, v)) = item.split_once('=') {
                            let display_v = if is_sensitive(k) { "***" } else { v };
                            let _ = write!(out, " {}={}", k, display_v);
                        } else if is_sensitive(item) {
                            let _ = write!(out, " {}=***", item);
                        } else {
                            let _ = write!(out, " {}", item);
                        }
                    }
                } else {
                    let _ = out.push_str("OK: dumping configuration");
                }
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::ConfigDump,
                }
            }
            "get" => {
                if let Some(&k) = tokens.get(2) {
                    let mut key: String<32> = String::new();
                    if !k.is_empty() && key.push_str(k).is_ok() {
                        let _ = write!(out, "OK: config get {}", k);
                        return CliCommandResult {
                            ok: true,
                            output: out.clone(),
                            side_effect: CliSideEffect::ConfigGet { key },
                        };
                    }
                }
                let _ = out.push_str("ERR: usage: config get <key>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            "set" => {
                if let (Some(&k), Some(&v)) = (tokens.get(2), tokens.get(3)) {
                    let mut key: String<32> = String::new();
                    let mut value: String<64> = String::new();
                    if !k.is_empty() && key.push_str(k).is_ok() && value.push_str(v).is_ok() {
                        let display_v = if k == "wifi.pass" || k == "wifi_pass" {
                            "***"
                        } else {
                            v
                        };
                        let _ = write!(out, "OK: config set {}={}", k, display_v);
                        return CliCommandResult {
                            ok: true,
                            output: out.clone(),
                            side_effect: CliSideEffect::ConfigSet { key, value },
                        };
                    }
                }
                let _ = out.push_str("ERR: usage: config set <key> <val>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            "erase" | "del" => {
                if let Some(&k) = tokens.get(2) {
                    let mut key: String<32> = String::new();
                    if !k.is_empty() && key.push_str(k).is_ok() {
                        let _ = write!(out, "OK: config erase {}", k);
                        return CliCommandResult {
                            ok: true,
                            output: out.clone(),
                            side_effect: CliSideEffect::ConfigErase { key },
                        };
                    }
                }
                let _ = out.push_str("ERR: usage: config erase <key>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            _ => {
                let _ = out.push_str("ERR: usage: config <get|set|erase|dump>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
        }
    }

    fn exec_join(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let Some(&ssid_raw) = tokens.get(1) else {
            let _ = out.push_str("ERR: usage: join <ssid> [password]");
            return CliCommandResult {
                ok: false,
                output: out.clone(),
                side_effect: CliSideEffect::None,
            };
        };
        let pass_raw = tokens.get(2).copied().unwrap_or("");
        if ssid_raw.is_empty()
            || ssid_raw.len() > 32
            || (!pass_raw.is_empty() && !(8..=63).contains(&pass_raw.len()))
        {
            let _ = out.push_str("ERR: invalid SSID or WPA2 passphrase length");
            return CliCommandResult {
                ok: false,
                output: out.clone(),
                side_effect: CliSideEffect::None,
            };
        }
        let mut ssid: String<32> = String::new();
        let mut password: String<64> = String::new();
        let _ = ssid.push_str(ssid_raw);
        let _ = password.push_str(pass_raw);
        let _ = write!(out, "OK: joining SSID '{}'", ssid_raw);
        CliCommandResult {
            ok: true,
            output: out.clone(),
            side_effect: CliSideEffect::WifiJoin { ssid, password },
        }
    }

    fn exec_update(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let Some(&url_raw) = tokens.get(1) else {
            let _ = out.push_str("ERR: usage: update <http(s)://url/firmware.bin>");
            return CliCommandResult {
                ok: false,
                output: out.clone(),
                side_effect: CliSideEffect::None,
            };
        };
        if !(url_raw.starts_with("http://") || url_raw.starts_with("https://")) {
            let _ = out.push_str("ERR: update URL must start with http:// or https://");
            return CliCommandResult {
                ok: false,
                output: out.clone(),
                side_effect: CliSideEffect::None,
            };
        }
        let mut url: String<128> = String::new();
        if url.push_str(url_raw).is_err() {
            let _ = out.push_str("ERR: update URL exceeds 128 bytes");
            return CliCommandResult {
                ok: false,
                output: out.clone(),
                side_effect: CliSideEffect::None,
            };
        }
        let _ = write!(out, "OK: starting OTA update from {}", url_raw);
        CliCommandResult {
            ok: true,
            output: out.clone(),
            side_effect: CliSideEffect::OtaUpdate { url },
        }
    }

    fn exec_player(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let sub = tokens.get(1).copied().unwrap_or("status");
        match sub {
            "play" => {
                let _ = out.push_str("OK: player play");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::PlayerControl(PlayerCliAction::Play),
                }
            }
            "pause" => {
                let _ = out.push_str("OK: player pause");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::PlayerControl(PlayerCliAction::Pause),
                }
            }
            "stop" => {
                let _ = out.push_str("OK: player stop");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::PlayerControl(PlayerCliAction::Stop),
                }
            }
            "status" => {
                let _ = out.push_str("OK: player status");
                CliCommandResult {
                    ok: true,
                    output: out.clone(),
                    side_effect: CliSideEffect::PlayerControl(PlayerCliAction::Status),
                }
            }
            "vol" | "volume" => {
                if let Some(&v_str) = tokens.get(2) {
                    if let Ok(pct) = v_str.parse::<u8>() {
                        if pct <= 100 {
                            let _ = write!(out, "OK: player volume {}%", pct);
                            return CliCommandResult {
                                ok: true,
                                output: out.clone(),
                                side_effect: CliSideEffect::PlayerControl(PlayerCliAction::Volume(
                                    pct,
                                )),
                            };
                        }
                    }
                }
                let _ = out.push_str("ERR: usage: player vol <0..100>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
            _ => {
                let _ = out.push_str("ERR: usage: player <play|pause|stop|status|vol>");
                CliCommandResult {
                    ok: false,
                    output: out.clone(),
                    side_effect: CliSideEffect::None,
                }
            }
        }
    }

    fn exec_sleep(&self, tokens: &[&str], out: &mut String<256>) -> CliCommandResult {
        let (mode, sec_token) = match (tokens.get(1).copied(), tokens.get(2).copied()) {
            (Some("deep"), Some(s)) => (SleepMode::Deep, Some(s)),
            (Some("light"), Some(s)) => (SleepMode::Light, Some(s)),
            (Some(s), None) => (SleepMode::Light, Some(s)),
            _ => (SleepMode::Light, None),
        };
        if let Some(s_str) = sec_token {
            if let Ok(secs) = s_str.parse::<u32>() {
                if secs > 0 {
                    let _ = write!(out, "OK: entering {:?} sleep for {}s", mode, secs);
                    return CliCommandResult {
                        ok: true,
                        output: out.clone(),
                        side_effect: CliSideEffect::EnterSleep {
                            mode,
                            duration_secs: secs,
                        },
                    };
                }
            }
        }
        let _ = out.push_str("ERR: usage: sleep [light|deep] <seconds>");
        CliCommandResult {
            ok: false,
            output: out.clone(),
            side_effect: CliSideEffect::None,
        }
    }
}

/// Startup command sequence runner (`autoexec` NVS script compatibility).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AutoexecRunner {
    commands: Vec<String<96>, 8>,
    cursor: usize,
    enabled: bool,
}

impl AutoexecRunner {
    /// Parses a `;`- or `\n`-delimited startup script into a bounded queue of
    /// up to 8 commands.
    #[must_use]
    pub fn from_script(script: &str, enabled: bool) -> Self {
        let mut commands: Vec<String<96>, 8> = Vec::new();
        for raw_segment in script.split(['\n', ';']) {
            let trimmed = raw_segment.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if commands.len() >= 8 {
                break;
            }
            let mut cmd_str: String<96> = String::new();
            let safe_len = trimmed.floor_char_boundary(96.min(trimmed.len()));
            if cmd_str.push_str(&trimmed[..safe_len]).is_ok() {
                let _ = commands.push(cmd_str);
            }
        }
        Self {
            commands,
            cursor: 0,
            enabled,
        }
    }

    /// Number of unexecuted startup commands remaining.
    #[must_use]
    pub fn remaining(&self) -> usize {
        if !self.enabled {
            return 0;
        }
        self.commands.len().saturating_sub(self.cursor)
    }

    /// Returns `true` if all queued startup commands have been executed or
    /// `autoexec` is disabled.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.remaining() == 0
    }

    /// Resets the execution cursor to the beginning of the startup script.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Executes and advances the next startup command against `registry`.
    pub fn step(&mut self, registry: &CliRegistry) -> Option<CliCommandResult> {
        if !self.enabled || self.cursor >= self.commands.len() {
            return None;
        }
        let line = self.commands[self.cursor].as_str();
        self.cursor += 1;
        Some(registry.execute(line))
    }
}

/// Severity level for entries stored in [`SyslogRingBuffer`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Error condition.
    Error,
    /// Warning condition.
    Warn,
    /// Normal informational message.
    Info,
    /// Verbose debug trace.
    Debug,
}

impl LogLevel {
    /// Returns the uppercase ASCII label for this severity level.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

/// Single circular log record stored in [`SyslogRingBuffer`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyslogEntry {
    /// Monotonic sequence number (`1..`).
    pub seq: u32,
    /// System uptime in milliseconds when logged.
    pub uptime_ms: u64,
    /// Log severity.
    pub level: LogLevel,
    /// Bounded UTF-8 log message text.
    pub text: String<128>,
}

/// Bounded circular log buffer feeding Telnet and `GET /messages.json`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SyslogRingBuffer<const N: usize = 16> {
    entries: [Option<SyslogEntry>; N],
    head: usize,
    len: usize,
    next_seq: u32,
}

impl<const N: usize> Default for SyslogRingBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SyslogRingBuffer<N> {
    /// Creates an empty [`SyslogRingBuffer`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; N],
            head: 0,
            len: 0,
            next_seq: 1,
        }
    }

    /// Number of retained log entries (`0..=N`).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the ring buffer is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clears all stored entries while preserving monotonic sequence numbers.
    pub fn clear(&mut self) {
        for slot in &mut self.entries {
            *slot = None;
        }
        self.head = 0;
        self.len = 0;
    }

    /// Appends a new log message, overwriting the oldest entry when full, and
    /// returns the assigned sequence number.
    pub fn push(&mut self, uptime_ms: u64, level: LogLevel, message: &str) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1).max(1);
        if N == 0 {
            return seq;
        }

        let mut text: String<128> = String::new();
        let safe_len = message.floor_char_boundary(128.min(message.len()));
        let _ = text.push_str(&message[..safe_len]);

        let write_idx = (self.head + self.len) % N;
        self.entries[write_idx] = Some(SyslogEntry {
            seq,
            uptime_ms,
            level,
            text,
        });
        if self.len < N {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % N;
        }
        seq
    }

    /// Returns the entry at `chronological_index` (`0` = oldest retained,
    /// `self.len() - 1` = newest).
    #[must_use]
    pub fn entry(&self, chronological_index: usize) -> Option<&SyslogEntry> {
        if chronological_index >= self.len || N == 0 {
            return None;
        }
        let slot_idx = (self.head + chronological_index) % N;
        self.entries[slot_idx].as_ref()
    }

    /// Formats all retained log entries as newline-terminated plain text into
    /// `out_buf`, returning the number of bytes written.
    pub fn format_plain(&self, out_buf: &mut [u8]) -> usize {
        let mut offset = 0usize;
        for i in 0..self.len {
            let Some(e) = self.entry(i) else {
                continue;
            };
            let mut line: String<192> = String::new();
            let _ = writeln!(
                line,
                "[#{:04} {:>8}ms {:5}] {}",
                e.seq,
                e.uptime_ms,
                e.level.as_str(),
                e.text.as_str()
            );
            let bytes = line.as_bytes();
            if offset + bytes.len() > out_buf.len() {
                break;
            }
            out_buf[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }
        offset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IacState {
    Normal,
    SawIac,
    SawOptionVerb(u8),
    InSubnegotiation,
    SawSubIac,
}

/// Stateful TCP Port 23 Telnet session handler that strips RFC 854 `IAC`
/// negotiation sequences and dispatches completed lines to [`CliRegistry`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct TelnetSession {
    iac_state: IacState,
    line_buf: String<128>,
    skip_next_lf: bool,
    overflowed: bool,
}

impl Default for TelnetSession {
    fn default() -> Self {
        Self::new()
    }
}

impl TelnetSession {
    /// Creates a new [`TelnetSession`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            iac_state: IacState::Normal,
            line_buf: String::new(),
            skip_next_lf: false,
            overflowed: false,
        }
    }

    /// Current partial command line buffered in the session.
    #[must_use]
    pub fn pending_line(&self) -> &str {
        self.line_buf.as_str()
    }

    /// Resets the session state and clears any partial line.
    pub fn reset(&mut self) {
        self.iac_state = IacState::Normal;
        self.line_buf.clear();
        self.skip_next_lf = false;
        self.overflowed = false;
    }

    /// Feeds raw TCP bytes from port 23, stripping Telnet `IAC` negotiations
    /// and dispatching each newline-terminated command via `registry`.
    ///
    /// Writes formatted command replies + `louder> ` prompts into `tx_out`,
    /// returning `(tx_bytes_written, last_side_effect)`.
    pub fn ingest_bytes(
        &mut self,
        rx_bytes: &[u8],
        registry: &CliRegistry,
        tx_out: &mut [u8],
    ) -> (usize, CliSideEffect) {
        let mut tx_len = 0usize;
        let mut last_effect = CliSideEffect::None;

        for &byte in rx_bytes {
            match self.iac_state {
                IacState::Normal => {
                    if byte == TELNET_IAC {
                        self.iac_state = IacState::SawIac;
                        continue;
                    }
                    if self.skip_next_lf && (byte == b'\n' || byte == 0) {
                        self.skip_next_lf = false;
                        continue;
                    }
                    self.skip_next_lf = false;

                    match byte {
                        b'\r' | b'\n' => {
                            if byte == b'\r' {
                                self.skip_next_lf = true;
                            }
                            if self.overflowed {
                                self.overflowed = false;
                                self.line_buf.clear();
                                tx_len = Self::append_reply(
                                    tx_out,
                                    tx_len,
                                    "ERR: line exceeds 128 bytes limit",
                                );
                            } else {
                                let res = registry.execute(self.line_buf.as_str());
                                self.line_buf.clear();
                                if res.side_effect != CliSideEffect::None {
                                    last_effect = res.side_effect;
                                }
                                tx_len = Self::append_reply(tx_out, tx_len, res.output.as_str());
                            }
                        }
                        0x08 | 0x7F => {
                            if !self.overflowed {
                                let _ = self.line_buf.pop();
                            }
                        }
                        0x20..=0x7E => {
                            if self.overflowed {
                                // Line already exceeded capacity; discard remainder.
                            } else if self.line_buf.push(byte as char).is_err() {
                                self.overflowed = true;
                                self.line_buf.clear();
                            }
                        }
                        _ => {}
                    }
                }
                IacState::SawIac => match byte {
                    TELNET_WILL | TELNET_WONT | TELNET_DO | TELNET_DONT => {
                        self.iac_state = IacState::SawOptionVerb(byte);
                    }
                    TELNET_SB => {
                        self.iac_state = IacState::InSubnegotiation;
                    }
                    TELNET_IAC => {
                        if !self.overflowed && self.line_buf.push(TELNET_IAC as char).is_err() {
                            self.overflowed = true;
                            self.line_buf.clear();
                        }
                        self.iac_state = IacState::Normal;
                    }
                    _ => {
                        self.iac_state = IacState::Normal;
                    }
                },
                IacState::SawOptionVerb(_verb) => {
                    // Consume the 3rd byte (option code) and return to Normal.
                    self.iac_state = IacState::Normal;
                }
                IacState::InSubnegotiation => {
                    if byte == TELNET_IAC {
                        self.iac_state = IacState::SawSubIac;
                    }
                }
                IacState::SawSubIac => {
                    if byte == TELNET_SE {
                        self.iac_state = IacState::Normal;
                    } else {
                        self.iac_state = IacState::InSubnegotiation;
                    }
                }
            }
        }

        (tx_len, last_effect)
    }

    fn append_reply(tx_out: &mut [u8], mut offset: usize, output: &str) -> usize {
        if !output.is_empty() {
            let out_bytes = output.as_bytes();
            let can_copy = out_bytes.len().min(tx_out.len().saturating_sub(offset));
            tx_out[offset..offset + can_copy].copy_from_slice(&out_bytes[..can_copy]);
            offset += can_copy;
            if offset + 2 <= tx_out.len() {
                tx_out[offset] = b'\r';
                tx_out[offset + 1] = b'\n';
                offset += 2;
            }
        }
        let prompt_bytes = TELNET_PROMPT.as_bytes();
        let can_prompt = prompt_bytes.len().min(tx_out.len().saturating_sub(offset));
        tx_out[offset..offset + can_prompt].copy_from_slice(&prompt_bytes[..can_prompt]);
        offset + can_prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_registry_all_commands() {
        let reg = CliRegistry::new();
        assert_eq!(reg.commands().len(), 8);

        let help = reg.execute("help");
        assert!(help.ok);
        assert!(help.output.as_str().contains("system"));
        assert!(help.output.as_str().contains("i2c"));

        let sys_reb = reg.execute("system reboot");
        assert_eq!(sys_reb.side_effect, CliSideEffect::Reboot);

        let sys_rec = reg.execute("system recovery");
        assert_eq!(sys_rec.side_effect, CliSideEffect::RebootToRecovery);

        let i2c_scan = reg.execute("i2c scan");
        assert_eq!(i2c_scan.side_effect, CliSideEffect::I2cScan);

        let i2c_rd = reg.execute("i2c get 0x2D 0x03");
        assert_eq!(
            i2c_rd.side_effect,
            CliSideEffect::I2cRead {
                addr: 0x2D,
                reg: 0x03
            }
        );

        let i2c_wr = reg.execute("i2c set 0x2D 0x4C 0x30");
        assert_eq!(
            i2c_wr.side_effect,
            CliSideEffect::I2cWrite {
                addr: 0x2D,
                reg: 0x4C,
                val: 0x30
            }
        );

        let cfg_set = reg.execute("config set player_name \"Basement HiFi\"");
        assert!(cfg_set.ok);
        assert!(matches!(
            cfg_set.side_effect,
            CliSideEffect::ConfigSet { ref key, ref value }
                if key.as_str() == "player_name" && value.as_str() == "Basement HiFi"
        ));

        let join = reg.execute("join \"IoT Net\" \"SecretPass123\"");
        assert!(join.ok);
        assert!(matches!(
            join.side_effect,
            CliSideEffect::WifiJoin { ref ssid, ref password }
                if ssid.as_str() == "IoT Net" && password.as_str() == "SecretPass123"
        ));

        let upd = reg.execute("update https://10.0.0.10/louder.bin");
        assert!(upd.ok);
        assert!(matches!(
            upd.side_effect,
            CliSideEffect::OtaUpdate { ref url }
                if url.as_str() == "https://10.0.0.10/louder.bin"
        ));

        let vol = reg.execute("player vol 65");
        assert_eq!(
            vol.side_effect,
            CliSideEffect::PlayerControl(PlayerCliAction::Volume(65))
        );

        let slp = reg.execute("sleep deep 120");
        assert_eq!(
            slp.side_effect,
            CliSideEffect::EnterSleep {
                mode: SleepMode::Deep,
                duration_secs: 120,
            }
        );
    }

    #[test]
    fn test_autoexec_runner_and_syslog_ring_buffer() {
        let reg = CliRegistry::new();
        let script = "# Boot script\nplayer vol 45; i2c scan\nsystem info";
        let mut runner = AutoexecRunner::from_script(script, true);
        assert_eq!(runner.remaining(), 3);

        let r1 = runner.step(&reg).expect("step 1");
        assert_eq!(
            r1.side_effect,
            CliSideEffect::PlayerControl(PlayerCliAction::Volume(45))
        );
        let r2 = runner.step(&reg).expect("step 2");
        assert_eq!(r2.side_effect, CliSideEffect::I2cScan);
        let r3 = runner.step(&reg).expect("step 3");
        assert_eq!(r3.side_effect, CliSideEffect::None);
        assert!(runner.is_complete());

        let mut ring: SyslogRingBuffer<3> = SyslogRingBuffer::new();
        assert_eq!(ring.push(100, LogLevel::Info, "booting"), 1);
        assert_eq!(ring.push(200, LogLevel::Info, "wifi up"), 2);
        assert_eq!(ring.push(300, LogLevel::Warn, "nack retry"), 3);
        assert_eq!(ring.push(400, LogLevel::Error, "amp fault"), 4);

        assert_eq!(ring.len(), 3);
        assert_eq!(ring.entry(0).unwrap().seq, 2);
        assert_eq!(ring.entry(2).unwrap().seq, 4);

        let mut buf = [0u8; 256];
        let written = ring.format_plain(&mut buf);
        let text = core::str::from_utf8(&buf[..written]).expect("utf8");
        assert!(text.contains("wifi up"));
        assert!(text.contains("amp fault"));
    }

    #[test]
    fn test_telnet_session_iac_stripping_and_dispatch() {
        let reg = CliRegistry::new();
        let mut session = TelnetSession::new();
        let mut tx = [0u8; 512];

        // Interleave IAC WILL ECHO, IAC SB NAWS ... IAC SE, and "player play\r\n"
        let raw_stream: &[u8] = &[
            TELNET_IAC,
            TELNET_WILL,
            0x01,
            b'p',
            b'l',
            b'a',
            TELNET_IAC,
            TELNET_SB,
            0x1F,
            0x00,
            0x50,
            0x00,
            0x18,
            TELNET_IAC,
            TELNET_SE,
            b'y',
            b'e',
            b'r',
            b' ',
            b'p',
            b'l',
            b'a',
            b'y',
            b'\r',
            b'\n',
        ];

        let (tx_len, effect) = session.ingest_bytes(raw_stream, &reg, &mut tx);
        assert_eq!(effect, CliSideEffect::PlayerControl(PlayerCliAction::Play));
        let reply = core::str::from_utf8(&tx[..tx_len]).expect("utf8");
        assert!(reply.contains("OK: player play"));
        assert!(reply.ends_with(TELNET_PROMPT));
    }

    #[test]
    fn test_net_503_telnet_iac_escape_and_subnegotiation() {
        let reg = CliRegistry::new();
        let mut session = TelnetSession::new();
        let mut tx = [0u8; 512];

        // 1. Escaped IAC (0xFF 0xFF) inside regular stream appends literal 0xFF
        let stream1: &[u8] = &[b'a', TELNET_IAC, TELNET_IAC, b'b'];
        session.ingest_bytes(stream1, &reg, &mut tx);
        assert_eq!(session.pending_line(), "a\u{ff}b");

        // 2. Subnegotiation containing escaped IAC (0xFF 0xFF) resumes InSubnegotiation
        // and terminates cleanly on IAC SE
        let mut session2 = TelnetSession::new();
        let stream2: &[u8] = &[
            TELNET_IAC, TELNET_SB, 0x24, TELNET_IAC,
            TELNET_IAC, // escaped IAC inside subnegotiation
            0x01, TELNET_IAC, TELNET_SE, // end subnegotiation
            b'c',
        ];
        session2.ingest_bytes(stream2, &reg, &mut tx);
        assert_eq!(session2.pending_line(), "c");
    }

    #[test]
    fn test_exec_config_redacts_wifi_pass() {
        let reg = CliRegistry::new();

        // 1. config set wifi.pass redaction
        let res_set = reg.execute("config set wifi.pass SuperSecretPassword123");
        assert!(res_set.ok);
        assert_eq!(res_set.output.as_str(), "OK: config set wifi.pass=***");

        // 2. config dump redaction
        let res_dump = reg.execute("config dump player_name=Basement wifi.pass=Secret");
        assert!(res_dump.ok);
        assert!(res_dump.output.contains("wifi.pass=***"));
        assert!(res_dump.output.contains("player_name=Basement"));
    }

    #[test]
    fn test_telnet_session_overflow_discards_line_and_emits_error() {
        let reg = CliRegistry::new();
        let mut session = TelnetSession::new();
        let mut tx = [0u8; 512];

        // Send 130 bytes without a newline (exceeding 128 bytes limit)
        let mut long_cmd = [b'a'; 130];
        long_cmd[0..4].copy_from_slice(b"help");
        session.ingest_bytes(&long_cmd, &reg, &mut tx);

        // Session must be in overflowed state and pending_line cleared
        assert_eq!(session.pending_line(), "");

        // Sending a newline now emits an error response rather than executing
        let (tx_len, effect) = session.ingest_bytes(b"\r\n", &reg, &mut tx);
        assert_eq!(effect, CliSideEffect::None);
        let reply = core::str::from_utf8(&tx[..tx_len]).expect("utf8");
        assert!(reply.contains("ERR: line exceeds 128 bytes limit"));
        assert!(reply.ends_with(TELNET_PROMPT));
    }
}
