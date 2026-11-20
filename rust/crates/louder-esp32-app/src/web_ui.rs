//! Embedded 10-tab single-page Web UI (`WEB_UI_HTML`) in `.rodata` (DROM).
//!
//! Combines the original Squeezelite-ESP32 C webapp tabs (`WiFi`, `Syslog`,
//! `Audio`, `System`, `Hardware`, `Updates/OTA`, `NVS Editor`, `Commands`,
//! `Credits`) with the MK5-Rust real-time hardware & DSP telemetry dashboard
//! (`Player & DSP`).

/// Complete 10-tab dark-Slate single-page web application served by `GET /`
/// and `GET /index.html` from flash `.rodata` without heap allocation.
pub const WEB_UI_HTML: &str = concat!(
    "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">",
    "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
    "<title>Louder-ESP32 Control</title><style>",
    ":root{--bg:#0f172a;--card:#1e293b;--bdr:#334155;--txt:#f8fafc;",
    "--mut:#94a3b8;--acc:#38bdf8;--ok:#10b981;--warn:#f59e0b;--err:#ef4444}",
    "*{box-sizing:border-box}body{font-family:system-ui,-apple-system,sans-serif;",
    "margin:0;padding:0 0 56px;background:var(--bg);color:var(--txt);font-size:13px}",
    "header{background:#020617;border-bottom:1px solid var(--bdr);padding:8px 12px;",
    "display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:8px}",
    ".brand{display:flex;align-items:center;gap:8px;font-weight:700;font-size:16px;",
    "color:var(--acc)}.badges{display:flex;flex-wrap:wrap;gap:6px}",
    ".badge{padding:2px 8px;border-radius:999px;font-size:11px;background:var(--card);",
    "border:1px solid var(--bdr);color:var(--txt)}",
    ".badge.ok{border-color:var(--ok);color:var(--ok)}",
    ".badge.warn{border-color:var(--warn);color:var(--warn)}",
    "nav{display:flex;flex-wrap:nowrap;overflow-x:auto;white-space:nowrap;gap:4px;",
    "background:#090d16;padding:6px 12px;border-bottom:1px solid var(--bdr)}",
    "nav button{flex:none;position:relative;background:var(--card);color:var(--mut);",
    "border:1px solid var(--bdr);padding:6px 10px;border-radius:6px;cursor:pointer;",
    "font-size:12px;font-weight:600}",
    "nav button[aria-selected=\"true\"],nav button.active,nav button:hover{",
    "color:var(--txt);border-color:var(--acc);background:#1e3a5f}",
    "nav button[aria-selected=\"true\"]::after{content:\"\";position:absolute;",
    "left:4px;right:4px;bottom:0;border-bottom:2px solid var(--acc)}",
    "main{max-width:1100px;margin:0 auto;padding:12px}",
    ".tab-pane{display:none}.tab-pane.active{display:block}",
    ".grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:12px}",
    ".card{background:var(--card);border:1px solid var(--bdr);border-radius:8px;",
    "padding:12px;margin-bottom:12px}",
    "h3{margin:0 0 8px;color:var(--acc);font-size:14px;border-bottom:1px solid var(--bdr);",
    "padding-bottom:4px}",
    "table{width:100%;border-collapse:collapse;font-size:12px}",
    "th,td{padding:5px 6px;border:1px solid var(--bdr);text-align:left}",
    "th{background:#0f172a;color:var(--mut)}",
    "button,.btn{margin:2px;padding:5px 10px;background:#334155;color:#fff;",
    "border:1px solid #475569;border-radius:4px;cursor:pointer;font-size:12px}",
    "button:hover{border-color:var(--acc)}button.pri{background:#0284c7;border-color:#38bdf8}",
    "button.ok{background:#059669;border-color:#10b981}",
    "button.danger{background:#991b1b;border-color:#ef4444}",
    "input,select,textarea{background:#0f172a;color:var(--txt);border:1px solid var(--bdr);",
    "border-radius:4px;padding:4px 8px;font-size:12px;width:100%}",
    "input[type=checkbox]{width:auto}.row{display:flex;gap:8px;align-items:center;",
    "margin-bottom:6px}.row label{min-width:130px;color:var(--mut)}",
    "a{color:var(--acc);text-decoration:none}",
    "footer{position:fixed;bottom:0;left:0;right:0;background:#020617;",
    "border-top:1px solid var(--bdr);padding:6px 12px;display:flex;flex-wrap:wrap;",
    "align-items:center;justify-content:space-between;font-size:11px;z-index:20}",
    "pre{background:#020617;padding:8px;border-radius:4px;overflow-x:auto;",
    "border:1px solid var(--bdr);font-size:11px;margin:6px 0 0}",
    ".eq-wrap{display:flex;gap:4px;margin:8px 0;flex-wrap:wrap}",
    ".eq-col{flex:1;min-width:24px;text-align:center}.eq-lbl{font-size:10px}",
    "</style></head><body>",
    "<header><div class=\"brand\"><span>Louder-ESP32 Control</span>",
    "<span id=\"navtitle\" class=\"badge ok\">BASEMENT</span></div>",
    "<div class=\"badges\">",
    "<span id=\"badge-rec\" class=\"badge\">OTA0: -</span>",
    "<span id=\"badge-jack\" class=\"badge\">Jack: 0 | 0mV</span>",
    "<span id=\"badge-airplay\" class=\"badge ok\">AirPlay: ON</span>",
    "<span id=\"badge-cspot\" class=\"badge ok\">Spotify: ON</span>",
    "<span id=\"badge-out\" class=\"badge ok\">Out: I2S</span>",
    "<span id=\"badge-net\" class=\"badge\">Wi-Fi (-)</span>",
    "</div></header>",
    "<nav id=\"main-nav\" role=\"tablist\" aria-label=\"Device Control Sections\">",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-player\" aria-selected=\"true\" ",
    "class=\"active\" onclick=\"showTab('tab-player',this)\">Player &amp; DSP</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-wifi\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-wifi',this)\">WiFi</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-syslog\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-syslog',this)\">Status / Syslog</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-audio\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-cfg-audio',this)\">Audio</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-syst\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-cfg-syst',this)\">System</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-hw\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-cfg-hw',this)\">Hardware</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-fw\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-cfg-fw',this)\">Updates / OTA</button>",
    "<button type=\"button\" role=\"tab\" id=\"nav-nvs\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-nvs',this)\">NVS Editor</button>",
    "<button type=\"button\" role=\"tab\" id=\"nav-cmd\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-commands',this)\">Console Commands</button>",
    "<button type=\"button\" role=\"tab\" id=\"tabbtn-credits\" aria-selected=\"false\" ",
    "onclick=\"showTab('tab-credits',this)\">Credits</button>",
    "</nav><main>",
    // Tab 1: Player & DSP
    "<section id=\"tab-player\" role=\"tabpanel\" aria-labelledby=\"tabbtn-player\" ",
    "class=\"tab-pane active\"><div class=\"grid\">",
    "<div class=\"card\"><h3>Live Hardware &amp; DSP Telemetry</h3><table>",
    "<tr><td>Net/RSSI</td><td id=\"n\">-</td></tr>",
    "<tr><td>LMS</td><td id=\"l\">-</td></tr>",
    "<tr><td>Amp/Up</td><td id=\"s\">-</td></tr>",
    "<tr><td>Audio</td><td id=\"f\">-</td></tr>",
    "<tr><td>PSRAM</td><td id=\"p\">-</td></tr>",
    "<tr><td>Core 1</td><td id=\"c\">-</td></tr>",
    "<tr><td>TAS5805M</td><td id=\"t\">-</td></tr>",
    "<tr><td>EQ / NVS</td><td id=\"e\">-</td></tr>",
    "<tr><td>I2C</td><td id=\"i\">-</td></tr>",
    "<tr><td>Firmware</td><td id=\"w\">-</td></tr>",
    "</table></div>",
    "<div class=\"card\"><h3>Transport &amp; Volume</h3>",
    "<button type=\"button\" class=\"ok\" onclick=\"cmd('play')\">&#9654;Play</button>",
    "<button type=\"button\" onclick=\"cmd('pause')\">&#10074;&#10074;Pause</button>",
    "<button type=\"button\" onclick=\"cmd('stop')\">&#9632;Stop</button>",
    "<button type=\"button\" onclick=\"cmd('prev')\">&#9198;Prev</button>",
    "<button type=\"button\" onclick=\"cmd('next')\">Next&#9197;</button><br>",
    "<button type=\"button\" onclick=\"cmd('voldown')\">-5%</button>",
    "<button type=\"button\" onclick=\"cmd('vol25')\">25%</button>",
    "<button type=\"button\" onclick=\"cmd('vol40')\">40%</button>",
    "<button type=\"button\" onclick=\"cmd('vol55')\">55%</button>",
    "<button type=\"button\" onclick=\"cmd('vol70')\">70%</button>",
    "<button type=\"button\" onclick=\"cmd('vol85')\">85%</button>",
    "<button type=\"button\" onclick=\"cmd('volup')\">+5%</button>",
    "<h3>Presets</h3>",
    "<button type=\"button\" class=\"pri\" onclick=\"cmd('preset1')\">Ibiza</button>",
    "<button type=\"button\" class=\"pri\" onclick=\"cmd('preset2')\">WHYY</button>",
    "<button type=\"button\" class=\"pri\" onclick=\"cmd('preset3')\">WDIY</button>",
    "<button type=\"button\" class=\"pri\" onclick=\"cmd('preset4')\">Groove</button>",
    "<h3>Equalizer (10-Band)</h3>",
    "<button type=\"button\" onclick=\"cmd('eq_flat')\">Flat</button>",
    "<button type=\"button\" onclick=\"cmd('eq_bass')\">Bass Boost</button>",
    "<button type=\"button\" onclick=\"cmd('eq_loudness')\">Loudness</button>",
    "<button type=\"button\" onclick=\"cmd('eq_vocal')\">Vocal</button>",
    "<button type=\"button\" onclick=\"cmd('eq_rock')\">Rock</button>",
    "<div id=\"eq-sliders\" class=\"eq-wrap\">",
    "<div class=\"eq-col\"><label for=\"eq-b0\" id=\"eq-v0\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b0\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(0,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b1\" id=\"eq-v1\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b1\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(1,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b2\" id=\"eq-v2\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b2\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(2,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b3\" id=\"eq-v3\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b3\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(3,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b4\" id=\"eq-v4\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b4\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(4,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b5\" id=\"eq-v5\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b5\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(5,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b6\" id=\"eq-v6\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b6\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(6,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b7\" id=\"eq-v7\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b7\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(7,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b8\" id=\"eq-v8\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b8\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(8,this.value)\" onchange=\"saveEq()\"></div>",
    "<div class=\"eq-col\"><label for=\"eq-b9\" id=\"eq-v9\" class=\"eq-lbl\">0dB</label>",
    "<input type=\"range\" id=\"eq-b9\" min=\"-12\" max=\"12\" value=\"0\" ",
    "oninput=\"onEqInput(9,this.value)\" onchange=\"saveEq()\"></div>",
    "</div>",
    "<h3>Analog Gain (AGAIN)</h3>",
    "<button type=\"button\" onclick=\"cmd('again0')\">0 dB (29.5V)</button>",
    "<button type=\"button\" onclick=\"cmd('again4')\">-4 dB (20V PD)</button>",
    "<button type=\"button\" onclick=\"cmd('again6')\">-6 dB (15V)</button>",
    "<button type=\"button\" onclick=\"cmd('again8')\">-8 dB (12V)</button><br><br>",
    "<a id=\"ll\" href=\"#\" target=\"_blank\" rel=\"noopener\">",
    "&rarr; Open Full Lyrion UI</a>",
    "<h3>System &amp; Anti-Brick Safety</h3>",
    "<button type=\"button\" class=\"danger\" ",
    "onclick=\"postReboot('/recovery.json','Boot factory recovery (0x10000)?')\">",
    "Reboot to Recovery (0x10000)</button>",
    "<button type=\"button\" ",
    "onclick=\"postReboot('/reboot_ota.json','Commit OTA &amp; Reboot?')\">",
    "Commit OTA &amp; Reboot</button>",
    "</div></div></section>",
    // Tab 2: WiFi
    "<section id=\"tab-wifi\" role=\"tabpanel\" aria-labelledby=\"tabbtn-wifi\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Wi-Fi Access Points &amp; Network Interface</h3>",
    "<div id=\"eth-banner\" class=\"badge\" style=\"margin-bottom:8px\">",
    "Interface: Wi-Fi STA</div>",
    "<button type=\"button\" class=\"pri\" onclick=\"scanWifi()\">Scan Wi-Fi Networks</button>",
    "<button type=\"button\" class=\"danger\" onclick=\"disconnectWifi()\">",
    "Disconnect Current AP</button>",
    "<table id=\"wifiTable\" style=\"margin-top:8px\"><thead>",
    "<tr><th>SSID</th><th>Channel</th><th>RSSI</th><th>Auth</th><th>Action</th></tr>",
    "</thead><tbody id=\"wifiTableBody\"><tr><td colspan=\"5\">Click Scan...</td></tr>",
    "</tbody></table></div>",
    "<div class=\"card\"><h3>Connect to Access Point</h3>",
    "<form id=\"wifi-form\" onsubmit=\"event.preventDefault();connectWifi()\">",
    "<div class=\"row\"><label for=\"wifi-ssid\">SSID</label>",
    "<input id=\"wifi-ssid\" name=\"ssid\" maxlength=\"32\" autocorrect=\"off\" ",
    "autocapitalize=\"none\" spellcheck=\"false\" placeholder=\"SSID\"></div>",
    "<div class=\"row\"><label for=\"wifi-pass\">Password</label>",
    "<input id=\"wifi-pass\" name=\"password\" type=\"password\" maxlength=\"64\" ",
    "autocomplete=\"current-password\" placeholder=\"WPA2 Passphrase\"></div>",
    "<button type=\"submit\" class=\"ok\">Join Network</button>",
    "<span id=\"wifi-status\" role=\"status\" aria-live=\"polite\"></span>",
    "</form></div></section>",
    // Tab 3: Status / Syslog
    "<section id=\"tab-syslog\" role=\"tabpanel\" aria-labelledby=\"tabbtn-syslog\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>System Event Log (`/messages.json`)</h3>",
    "<button type=\"button\" onclick=\"loadSyslog()\">Refresh Log</button>",
    "<button type=\"button\" id=\"clear-syslog\" onclick=\"clearSyslog()\">Clear View</button>",
    "<table id=\"syslogTable\" style=\"margin-top:6px\"><thead>",
    "<tr><th>Seq</th><th>Uptime (ms)</th><th>Level</th><th>Class</th><th>Message</th></tr>",
    "</thead><tbody id=\"syslogBody\"></tbody></table></div>",
    "<div class=\"grid\"><div class=\"card\"><h3>Hardware GPIO Assignments</h3>",
    "<table id=\"gpiotable\"><thead><tr><th>GPIO</th><th>Signal</th>",
    "<th>Subsystem</th><th>Fixed</th></tr></thead><tbody id=\"gpioBody\"></tbody></table></div>",
    "<div class=\"card\"><h3>Runtime Tasks &amp; Memory (`#tasks`)</h3>",
    "<table id=\"tasks\"><thead><tr><th>Task / Subsystem</th><th>Core</th>",
    "<th>State</th><th>Telemetry</th></tr></thead><tbody id=\"tasksBody\">",
    "<tr><td>embassy_net_wifi</td><td>Core 0</td><td>Running</td><td id=\"tk-net\">-</td></tr>",
    "<tr><td>audio_decode_i2s</td><td>Core 1</td><td id=\"tk-c1\">Active</td>",
    "<td id=\"tk-loops\">-</td></tr>",
    "<tr><td>psram_ring_alloc</td><td>Quad-SPI</td><td>4 MiB</td><td id=\"tk-psram\">-</td></tr>",
    "</tbody></table></div></div></section>",
    // Tab 4: Audio
    "<section id=\"tab-cfg-audio\" role=\"tabpanel\" aria-labelledby=\"tabbtn-audio\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Audio Output Template &amp; Squeezelite Command-Line Builder</h3>",
    "<div class=\"row\"><label for=\"audio-tmpl\">Output Template</label>",
    "<select id=\"audio-tmpl\" onchange=\"applyAudioTemplate()\">",
    "<option value=\"I2S\">I2S Digital Amp (TAS5805M)</option>",
    "<option value=\"SPDIF\">S/PDIF Optical/Coax</option>",
    "<option value=\"BT\">Bluetooth A2DP Speaker</option></select></div>",
    "<div class=\"grid\">",
    "<div><div class=\"row\"><label for=\"sq-name\">Player Name (-n)</label>",
    "<input id=\"sq-name\" maxlength=\"32\" autocorrect=\"off\" autocapitalize=\"none\" ",
    "spellcheck=\"false\" value=\"BASEMENT\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-server\">LMS Server (-s)</label>",
    "<input id=\"sq-server\" maxlength=\"64\" autocorrect=\"off\" autocapitalize=\"none\" ",
    "spellcheck=\"false\" placeholder=\"ip[:port]\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-buf\">Buffers (-b)</label>",
    "<input id=\"sq-buf\" maxlength=\"32\" value=\"500:2000\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-codecs\">Codecs (-c)</label>",
    "<input id=\"sq-codecs\" maxlength=\"64\" value=\"flc,pcm,mp3,aac,ogg\" ",
    "oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-idle\">Idle Timeout (-C)</label>",
    "<input id=\"sq-idle\" maxlength=\"16\" value=\"30\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-debug\">Log Level (-d)</label>",
    "<input id=\"sq-debug\" maxlength=\"48\" value=\"all=info\" ",
    "oninput=\"onAudioInput()\"></div></div>",
    "<div><div class=\"row\"><label for=\"sl-e\">Exclude Codecs (-e)</label>",
    "<input id=\"sl-e\" maxlength=\"32\" placeholder=\"dsd\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sl-m\">MAC Address (-m)</label>",
    "<input id=\"sl-m\" maxlength=\"24\" autocorrect=\"off\" autocapitalize=\"none\" ",
    "spellcheck=\"false\" placeholder=\"24:0a:c4:xx:xx:xx\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-rates\">Sample Rates (-r)</label>",
    "<input id=\"sq-rates\" maxlength=\"32\" value=\"44100-96000\" ",
    "oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-resample\">Resample (-R)</label>",
    "<input id=\"sq-resample\" maxlength=\"32\" placeholder=\"u:::28\" ",
    "oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sl-Z\">Max Rate (-Z)</label>",
    "<input id=\"sl-Z\" maxlength=\"16\" value=\"96000\" oninput=\"onAudioInput()\"></div>",
    "<div class=\"row\"><label for=\"sq-flac\">WAV/AIFF Header (-W)</label>",
    "<input type=\"checkbox\" id=\"sq-flac\" checked onchange=\"onAudioInput()\">",
    "<label for=\"sl-dis\">Disable Squeezelite</label>",
    "<input type=\"checkbox\" id=\"sl-dis\" onchange=\"onAudioInput()\"></div></div></div>",
    "<div class=\"row\"><label for=\"autoexec1-preview\">Generated `autoexec1`</label>",
    "<input id=\"autoexec1-preview\" maxlength=\"192\" oninput=\"audioDirty=false\" ",
    "value=\"squeezelite -o I2S -b 500:2000 -d all=info -C 30 -W\"></div>",
    "<button type=\"button\" id=\"save-autoexec1\" class=\"pri\" onclick=\"saveAudio(false)\">",
    "Save `autoexec1`</button>",
    "<button type=\"button\" id=\"save-audio\" class=\"ok\" onclick=\"saveAudio(true)\">",
    "Save &amp; Apply (`cfg-audio-tmpl`)</button></div></section>",
    // Tab 5: System
    "<section id=\"tab-cfg-syst\" role=\"tabpanel\" aria-labelledby=\"tabbtn-syst\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>System Services &amp; Device Identities (`cfg-syst-*`)</h3>",
    "<div class=\"row\"><label for=\"cfg-host_name\">Host Name (`host_name`)</label>",
    "<input id=\"cfg-host_name\" maxlength=\"32\" autocorrect=\"off\" autocapitalize=\"none\" ",
    "spellcheck=\"false\" value=\"BASEMENT\"></div>",
    "<div class=\"row\"><label for=\"cfg-cspot_name\">Spotify Name (`cspot_name`)</label>",
    "<input id=\"cfg-cspot_name\" maxlength=\"32\" value=\"BASEMENT\"></div>",
    "<div class=\"row\"><label for=\"cfg-bt_name\">Bluetooth Name (`bt_name`)</label>",
    "<input id=\"cfg-bt_name\" maxlength=\"32\" value=\"BASEMENT-BT\"></div>",
    "<div class=\"row\"><label for=\"cfg-ap_name\">AP Fallback Name (`ap_name`)</label>",
    "<input id=\"cfg-ap_name\" maxlength=\"32\" value=\"squeezelite-esp32\"></div>",
    "<div class=\"row\"><label for=\"cfg-enable_airplay\">",
    "Enable AirPlay (`enable_airplay`)</label>",
    "<input type=\"checkbox\" id=\"cfg-enable_airplay\" checked></div>",
    "<div class=\"row\"><label for=\"cfg-enable_cspot\">Enable Spotify (`enable_cspot`)</label>",
    "<input type=\"checkbox\" id=\"cfg-enable_cspot\" checked></div>",
    "<div class=\"row\"><label for=\"cfg-enable_bt_sink\">",
    "Enable BT Sink (`enable_bt_sink`)</label>",
    "<input type=\"checkbox\" id=\"cfg-enable_bt_sink\" checked></div>",
    "<div class=\"row\"><label for=\"cfg-enable_telnet\">Enable Telnet (`enable_telnet`)</label>",
    "<input type=\"checkbox\" id=\"cfg-enable_telnet\"></div>",
    "<div class=\"row\"><label for=\"cfg-autoexec\">Run Autoexec (`autoexec`)</label>",
    "<input type=\"checkbox\" id=\"cfg-autoexec\" checked></div>",
    "<button type=\"button\" class=\"ok\" onclick=\"saveSystemConfig()\">",
    "Save &amp; Apply System</button>",
    "</div></section>",
    // Tab 6: Hardware
    "<section id=\"tab-cfg-hw\" role=\"tabpanel\" aria-labelledby=\"tabbtn-hw\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Hardware Board Preset &amp; Peripheral Pinout (`cfg-hw-*`)</h3>",
    "<div class=\"row\"><label for=\"hw-preset\">Board Model Preset</label>",
    "<select id=\"hw-preset\" onchange=\"applyHwPreset()\">",
    "<option value=\"Louder-ESP32\">Louder-ESP32 (TAS5805M I2S0 + I2C0)</option>",
    "<option value=\"SqueezeAMP\">SqueezeAMP (TAS5756M)</option>",
    "<option value=\"ESP32-LyraT\">ESP32-LyraT (ES8388)</option>",
    "<option value=\"Generic-I2S\">Generic I2S DAC (PCM5102A)</option></select></div>",
    "<div class=\"row\"><label for=\"hw-dac_config\">DAC (`dac_config`)</label>",
    "<input id=\"hw-dac_config\" maxlength=\"192\" ",
    "value=\"model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=17:0\"></div>",
    "<div class=\"row\"><label for=\"hw-spdif_config\">S/PDIF (`spdif_config`)</label>",
    "<input id=\"hw-spdif_config\" maxlength=\"192\" value=\"bck=26,ws=25,do=-1\"></div>",
    "<div class=\"row\"><label for=\"hw-rotary_config\">Rotary (`rotary_config`)</label>",
    "<input id=\"hw-rotary_config\" maxlength=\"192\" value=\"A=-1,B=-1,SW=-1\"></div>",
    "<div class=\"row\"><label for=\"hw-i2c_config\">I2C Bus (`i2c_config`)</label>",
    "<input id=\"hw-i2c_config\" maxlength=\"192\" ",
    "value=\"sda=21,scl=27,speed=400000,port=0\"></div>",
    "<div class=\"row\"><label for=\"hw-spi_config\">SPI Bus (`spi_config`)</label>",
    "<input id=\"hw-spi_config\" maxlength=\"192\" value=\"mosi=-1,miso=-1,clk=-1\"></div>",
    "<div class=\"row\"><label for=\"hw-display_config\">Display (`display_config`)</label>",
    "<input id=\"hw-display_config\" maxlength=\"192\" value=\"type=none\"></div>",
    "<div class=\"row\"><label for=\"led-vu\">LED VU Config (`led_vu_config`)</label>",
    "<input id=\"led-vu\" maxlength=\"192\" placeholder=\"WS2812:12:8\"></div>",
    "<div class=\"row\"><label for=\"led-brightness\">LED Brightness (`led_brightness`)</label>",
    "<input id=\"led-brightness\" type=\"number\" min=\"0\" max=\"255\" placeholder=\"64\"></div>",
    "<button type=\"button\" id=\"save-hw\" class=\"ok\" onclick=\"saveHardwareConfig()\">",
    "Save Hardware Config</button>",
    "</div></section>",
    // Tab 7: Updates / OTA
    "<section id=\"tab-cfg-fw\" role=\"tabpanel\" aria-labelledby=\"tabbtn-fw\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Firmware Updates &amp; Partition Safety (`ota_0` @ 0x160000)</h3>",
    "<div class=\"row\"><label for=\"fw-url-input\">Remote Firmware URL</label>",
    "<input id=\"fw-url-input\" maxlength=\"192\" autocorrect=\"off\" autocapitalize=\"none\" ",
    "spellcheck=\"false\" placeholder=\"http://host:8000/louder-esp32.bin\"></div>",
    "<button type=\"button\" class=\"pri\" onclick=\"triggerUrlOta()\">",
    "Start URL OTA (`fwurl`)</button>",
    "<hr style=\"border-color:var(--bdr);margin:12px 0\">",
    "<div class=\"row\"><label for=\"flashfilename\">Local Binary (`.bin`)</label>",
    "<input type=\"file\" id=\"flashfilename\" accept=\".bin\"></div>",
    "<button type=\"button\" id=\"fwUpload\" class=\"ok\" onclick=\"uploadLocalOta()\">",
    "Upload &amp; Flash to `ota_0`</button>",
    "<div id=\"otadiv\" role=\"status\" aria-live=\"polite\" ",
    "style=\"margin-top:8px;padding:6px;background:#020617;",
    "border:1px solid var(--bdr);border-radius:4px\">OTA Status: Idle</div>",
    "<hr style=\"border-color:var(--bdr);margin:12px 0\">",
    "<button type=\"button\" id=\"btn_reboot_recovery\" class=\"danger\" ",
    "onclick=\"postReboot('/recovery.json','Boot factory recovery (0x10000)?')\">",
    "Reboot to Factory Recovery</button>",
    "</div></section>",
    // Tab 8: NVS Editor
    "<section id=\"tab-nvs\" role=\"tabpanel\" aria-labelledby=\"nav-nvs\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Non-Volatile Storage (NVS / FlashJournal) Editor</h3>",
    "<div style=\"margin-bottom:8px\">",
    "<button type=\"button\" onclick=\"loadConfig()\">Reload NVS</button>",
    "<button type=\"button\" id=\"save-nvs\" class=\"ok\" onclick=\"commitNvsTable()\">",
    "Commit NVS</button>",
    "<button type=\"button\" id=\"save-as-nvs\" class=\"pri\" onclick=\"exportNvsJson()\">",
    "Export JSON</button>",
    "<button type=\"button\" id=\"load-nvs\" onclick=\"importNvsJson()\">Import JSON</button>",
    "<button type=\"button\" id=\"btn_reboot\" class=\"danger\" ",
    "onclick=\"postReboot('/reboot.json','Reboot device?')\">Reboot Device</button></div>",
    "<table id=\"nvsTable\"><thead><tr><th>Key</th><th>Type</th><th>Value</th>",
    "<th>Action</th></tr></thead><tbody id=\"nvsBody\"></tbody>",
    "<tfoot><tr><td><input id=\"nvs-new-key\" maxlength=\"24\" autocorrect=\"off\" ",
    "autocapitalize=\"none\" spellcheck=\"false\" aria-label=\"New NVS key\" ",
    "placeholder=\"new_key\"></td>",
    "<td><select id=\"nvs-new-type\" aria-label=\"New NVS type\">",
    "<option value=\"33\">str (33)</option>",
    "<option value=\"1\">u8 (1)</option><option value=\"4\">u32 (4)</option></select></td>",
    "<td><input id=\"nvs-new-val\" maxlength=\"192\" aria-label=\"New NVS value\" ",
    "placeholder=\"value\"></td>",
    "<td><button type=\"button\" class=\"pri\" onclick=\"addNvsRow()\">Add / Set</button>",
    "</td></tr></tfoot>",
    "</table></div></section>",
    // Tab 9: Console Commands
    "<section id=\"tab-commands\" role=\"tabpanel\" aria-labelledby=\"nav-cmd\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Interactive Console &amp; Diagnostic Commands (`/commands.json`)</h3>",
    "<form id=\"cli-form\" ",
    "onsubmit=\"event.preventDefault();runCliCmd(D.getElementById('cli-input').value)\">",
    "<div class=\"row\"><input id=\"cli-input\" maxlength=\"192\" autocorrect=\"off\" ",
    "autocapitalize=\"none\" spellcheck=\"false\" aria-label=\"Console command\" ",
    "placeholder=\"e.g. i2cdetect, free, version, player, system\">",
    "<button type=\"submit\" class=\"pri\">Execute</button></div></form>",
    "<div id=\"cmd-buttons\" style=\"margin:8px 0\">",
    "<button type=\"button\" onclick=\"runCliCmd('help')\">help</button>",
    "<button type=\"button\" onclick=\"runCliCmd('free')\">free</button>",
    "<button type=\"button\" onclick=\"runCliCmd('heap')\">heap</button>",
    "<button type=\"button\" onclick=\"runCliCmd('version')\">version</button>",
    "<button type=\"button\" onclick=\"runCliCmd('i2cdetect')\">i2cdetect</button>",
    "<button type=\"button\" onclick=\"runCliCmd('system')\">system</button>",
    "<button type=\"button\" onclick=\"runCliCmd('config')\">config</button>",
    "<button type=\"button\" onclick=\"runCliCmd('player')\">player</button></div>",
    "<pre id=\"cli-output\" role=\"status\" aria-live=\"polite\">Ready.</pre>",
    "</div></section>",
    // Tab 10: Credits
    "<section id=\"tab-credits\" role=\"tabpanel\" aria-labelledby=\"tabbtn-credits\" ",
    "class=\"tab-pane\" hidden><div class=\"card\">",
    "<h3>Squeezelite-ESP32 &amp; Pure-Rust MK5 Engine Credits</h3>",
    "<p><strong>Squeezelite-ESP32</strong>: Philippe44, Sle118, Ralphy, Triode.</p>",
    "<p><strong>Louder-ESP32 Pure-Rust Engine (MK5)</strong>: Zero-unsafe audio pipeline, ",
    "TAS5805M I2C shadow-register resilience, 4-gate anti-brick OTA watchdog, ",
    "AirPlay RAOP, Spotify Connect (cspot), and Bluetooth A2DP sink.</p>",
    "<h3>Advanced UI Tab Visibility</h3>",
    "<div class=\"row\"><input type=\"checkbox\" id=\"show-nvs\" checked ",
    "onchange=\"toggleAdvTabs()\"><label for=\"show-nvs\">Show NVS Editor Tab</label></div>",
    "<div class=\"row\"><input type=\"checkbox\" id=\"show-commands\" checked ",
    "onchange=\"toggleAdvTabs()\"><label for=\"show-commands\">Show Console Commands Tab</label>",
    "</div></div></section></main>",
    // Fixed Footer
    "<footer><div><span id=\"foot-fw\">FW: I2S-4MFlash-Rust</span> | ",
    "<span id=\"foot-if\">IF: -</span></div>",
    "<div><button type=\"button\" id=\"reboot_nav\" ",
    "onclick=\"postReboot('/reboot.json','Reboot device?')\">Reboot</button>",
    "<button type=\"button\" id=\"reboot_ota_nav\" class=\"pri\" ",
    "onclick=\"postReboot('/reboot_ota.json','Commit OTA &amp; Reboot?')\">",
    "Commit OTA &amp; Reboot</button></div></footer>",
    // JavaScript Controller
    "<script>'use strict';",
    "var pF=0,pT=0,D=document,nvsCache={},audioDirty=false;",
    "var apiQ=Promise.resolve(),apiBusy=0;",
    "function apiFetch(u,o){apiBusy++;",
    "var p=apiQ.catch(function(){}).then(function(){return fetch(u,o);});",
    "apiQ=p.catch(function(){});",
    "return p.finally(function(){apiBusy--;});}",
    "function showToast(m,e){var t=D.getElementById('ui-toast');",
    "if(!t){t=D.createElement('div');t.id='ui-toast';t.setAttribute('role','status');",
    "t.style.cssText='position:fixed;bottom:18px;right:18px;padding:10px 16px;",
    "border-radius:6px;font-weight:600;z-index:9999;transition:opacity .2s';",
    "D.body.appendChild(t);}t.style.background=e?'#ef4444':'#22c55e';t.style.color='#fff';",
    "t.textContent=m;t.style.opacity='1';",
    "clearTimeout(t._tm);t._tm=setTimeout(function(){t.style.opacity='0';},3000);}",
    "function g(k,v){var e=D.getElementById(k);if(e)e.innerText=v;}",
    "function setBadge(id,txt,ok){var e=D.getElementById(id);if(!e)return;",
    "e.innerText=txt;e.classList.toggle('ok',!!ok);}",
    "function mkTd(v){var td=D.createElement('td');",
    "td.textContent=(v===undefined||v===null)?'':String(v);return td;}",
    "function authLabel(x){var m={0:'OPEN',1:'WEP',2:'WPA-PSK',3:'WPA2-PSK',",
    "4:'WPA/WPA2',5:'WPA2-ENT',6:'WPA3-PSK',7:'WPA2/WPA3'};",
    "return(m[x]!==undefined)?m[x]:String(x===undefined||x===null?'':x);}",
    "function nvsVal(k){var v=nvsCache[k];",
    "return(v&&typeof v==='object'&&v.value!==undefined)?v.value:v;}",
    "function sV(id,k,c){var e=D.getElementById(id);if(!e||e===D.activeElement)return;",
    "var v=nvsVal(k);if(v!==undefined&&v!==null){",
    "if(c||e.type==='checkbox')e.checked=(v===true||v==='1'||v===1||v==='true');",
    "else e.value=v;}}",
    "function postReboot(url,msg){if(!confirm(msg))return;",
    "apiFetch(url,{method:'POST'}).then(function(r){",
    "if(!r.ok)throw new Error('HTTP '+r.status);showToast('Rebooting device...');})",
    ".catch(function(e){showToast(e.message||'Reboot failed',true);});}",
    "function showTab(id,btn){",
    "D.querySelectorAll('.tab-pane').forEach(function(p){",
    "var on=(p.id===id);p.classList.toggle('active',on);p.hidden=!on;});",
    "D.querySelectorAll('#main-nav button').forEach(function(b){",
    "var sel=(b===btn);b.classList.toggle('active',sel);",
    "b.setAttribute('aria-selected',sel?'true':'false');});",
    "if(id==='tab-syslog'){loadSyslog();loadConfig();}",
    "if(id==='tab-cfg-audio'||id==='tab-nvs'||id==='tab-cfg-syst'||id==='tab-cfg-hw')loadConfig();",
    "if(id==='tab-wifi')scanWifi();}",
    "function cmd(a){return apiFetch('/cmd.json?act='+encodeURIComponent(a),{method:'POST'})",
    ".then(function(){if(a.indexOf('eq_')===0)return loadEq();})",
    ".then(function(){return u();}).catch(function(){});}",
    "function onEqInput(i,v){var lb=D.getElementById('eq-v'+i);if(lb)lb.innerText=v+'dB';}",
    "function saveEq(){var b=[];for(var i=0;i<10;i++){",
    "var e=D.getElementById('eq-b'+i);b.push(e?e.value:'0');}",
    "return postConfigKey('equalizer',b.join(',')).then(function(){",
    "showToast('EQ updated');return u();})",
    ".catch(function(e){showToast(e.message||'EQ save failed',true);});}",
    "function u(){if(apiBusy>0)return Promise.resolve();var n=Date.now();",
    "return apiFetch('/status.json').then(function(r){return r.json();}).then(function(d){",
    "var h=d.lms_ip+':'+(d.lms_port||9000);var ll=D.getElementById('ll');",
    "if(ll){ll.href='http://'+h+'/';ll.textContent='\\u2192 Open Full Lyrion UI ('+h+')';}",
    "g('n',d.ip+' ('+d.ssid+' '+d.rssi+'dBm)');",
    "g('l',h+' (CLI '+d.lms_cport+')');",
    "var up=d.uptime_ms/1000|0;",
    "g('s',d.amp_mode+' ('+d.vol_pct+'%) | Up '+(up/60|0)+':'+((up%60<10)?'0':'')+(up%60));",
    "var fps=(pT>0&&n>pT)?((d.frames_played-pF)*1000/(n-pT)|0):0;pF=d.frames_played;pT=n;",
    "g('f',fps+' fps ('+d.frames_played+') | '+d.codec);",
    "g('p',(d.psram_used_bytes/1024|0)+' / '+(d.psram_free_bytes/1024|0)+' KB');",
    "g('c',(d.core1_active?'Active':'Halted')+' ('+d.core1_loops+' loops)');",
    "g('t',d.tas_fs_hz+'Hz|'+d.tas_bck_ratio+'x|'+(d.tas_pll_locked?'PLL-Lock':'Unlocked')+",
    "'|-'+(d.again_code*0.5).toFixed(1)+'dB'+(d.tas_automuted?'(Muted)':''));",
    "g('e',d.eq_preset+' | Seq '+d.settings_seq);",
    "g('i','ACK:'+d.i2c_acks+'/NACK:'+d.i2c_nacks);",
    "g('w',d.version+' ('+(d.otadata_prearmed_factory?'Pre-Armed':'Normal')+')');",
    "setBadge('badge-rec',d.otadata_prearmed_factory?'OTA0 / Pre-Armed':",
    "(d.recovery?'Recovery':'OTA0 / Confirmed'),!d.recovery);",
    "g('badge-jack','Jack: '+(d.Jack||'0')+' | '+(d.Voltage||0)+'mV');",
    "setBadge('badge-net','Wi-Fi ('+(d.rssi!==undefined?d.rssi+'dBm':'-')+')',",
    "d.urc===0||d.urc===undefined);",
    "g('eth-banner','Interface: '+(d.if||'wifi').toUpperCase()+' ('+(d.ip||'-')+",
    "', SSID: '+(d.ssid||'-')+')');",
    "g('foot-fw','FW: '+d.version);g('foot-if','IF: '+(d.if||'wifi')+' ('+(d.ip||'-')+')');",
    "g('tk-net',d.ip+' / RSSI '+d.rssi+'dBm');",
    "g('tk-c1',d.core1_active?'Active':'Halted');g('tk-loops',d.core1_loops+' loops');",
    "g('tk-psram',(d.psram_free_bytes/1024|0)+' KB free');",
    "}).catch(function(){});}",
    "function scanWifi(){return apiFetch('/ap.json').then(function(r){return r.json();})",
    ".then(function(aps){var tb=D.getElementById('wifiTableBody');if(!tb)return;tb.textContent='';",
    "aps.forEach(function(a){var tr=D.createElement('tr');",
    "tr.appendChild(mkTd(a.ssid));tr.appendChild(mkTd(a.chan));",
    "tr.appendChild(mkTd((a.rssi===undefined||a.rssi===null?'':a.rssi)+' dBm'));",
    "tr.appendChild(mkTd(authLabel(a.auth)));",
    "var td=D.createElement('td'),btn=D.createElement('button');btn.type='button';",
    "btn.textContent='Select';btn.addEventListener('click',function(){",
    "var i=D.getElementById('wifi-ssid');if(i)i.value=a.ssid||'';});",
    "td.appendChild(btn);tr.appendChild(td);tb.appendChild(tr);});}).catch(function(){});}",
    "function connectWifi(){var s=D.getElementById('wifi-ssid').value.trim();",
    "var p=D.getElementById('wifi-pass').value;",
    "if(!s){g('wifi-status','Error: SSID required');showToast('SSID required',true);return;}",
    "if(p.length>64){g('wifi-status','Error: Password > 64 chars');",
    "showToast('Password > 64 chars',true);return;}",
    "apiFetch('/connect.json',{method:'POST',",
    "headers:{'Content-Type':'application/x-www-form-urlencoded'},",
    "body:'ssid='+encodeURIComponent(s)+'&password='+encodeURIComponent(p)})",
    ".then(function(r){if(!r.ok)throw new Error('HTTP '+r.status);return r.json();})",
    ".then(function(d){if(d&&d.ok===false)throw new Error(d.error||'Connect failed');",
    "g('wifi-status',d.status||'connecting');showToast('Wi-Fi: '+(d.status||'connecting'));})",
    ".catch(function(e){g('wifi-status','Error: '+(e.message||'failed'));",
    "showToast(e.message||'Wi-Fi connect failed',true);});}",
    "function disconnectWifi(){if(!confirm('Disconnect Wi-Fi?'))return;",
    "apiFetch('/connect.json',{method:'DELETE'})",
    ".then(function(r){if(!r.ok)throw new Error('HTTP '+r.status);",
    "g('wifi-status','disconnected');showToast('Wi-Fi disconnected');})",
    ".catch(function(e){showToast(e.message||'Disconnect failed',true);});}",
    "function loadSyslog(){return apiFetch('/messages.json').then(function(r){return r.json();})",
    ".then(function(d){var tb=D.getElementById('syslogBody');",
    "if(!tb||!d||!d.messages)return d;tb.textContent='';",
    "d.messages.forEach(function(m){var tr=D.createElement('tr');",
    "tr.appendChild(mkTd(m.seq));tr.appendChild(mkTd(m.uptime_ms));",
    "tr.appendChild(mkTd(m.level));tr.appendChild(mkTd(m.class||''));",
    "tr.appendChild(mkTd(m.msg));tb.appendChild(tr);});return d;}).catch(function(){});}",
    "function clearSyslog(){var tb=D.getElementById('syslogBody');if(tb)tb.textContent='';}",
    "function loadConfig(){return apiFetch('/config.json').then(function(r){return r.json();})",
    ".then(function(d){",
    "if(d.gpio){var gb=D.getElementById('gpioBody');if(gb){gb.textContent='';",
    "d.gpio.forEach(function(p){var tr=D.createElement('tr');",
    "tr.appendChild(mkTd('GPIO '+p.gpio));tr.appendChild(mkTd(p.name));",
    "tr.appendChild(mkTd(p.group));tr.appendChild(mkTd(p.fixed?'Fixed':'User'));",
    "gb.appendChild(tr);});}}",
    "if(d.config){nvsCache=d.config;var hn=nvsVal('host_name');if(hn)g('navtitle',hn);",
    "renderNvsTable(d.config);",
    "['host_name','cspot_name','bt_name','ap_name'].forEach(function(k){sV('cfg-'+k,k);});",
    "['enable_airplay','enable_cspot','enable_bt_sink','enable_telnet','autoexec']",
    ".forEach(function(k){sV('cfg-'+k,k,true);});",
    "['dac_config','spdif_config','rotary_config','i2c_config','spi_config','display_config']",
    ".forEach(function(k){sV('hw-'+k,k);});",
    "sV('led-vu','led_vu_config');if(nvsVal('led_vu')!==undefined)sV('led-vu','led_vu');",
    "sV('led-brightness','led_brightness');",
    "var apOn=String(nvsVal('enable_airplay'))!=='0'&&String(nvsVal('enable_airplay'))!=='false';",
    "setBadge('badge-airplay',apOn?'AirPlay: ON':'AirPlay: OFF',apOn);",
    "var csOn=String(nvsVal('enable_cspot'))!=='0'&&String(nvsVal('enable_cspot'))!=='false';",
    "setBadge('badge-cspot',csOn?'Spotify: ON':'Spotify: OFF',csOn);",
    "var dc=String(nvsVal('dac_config')||''),dm=dc.match(/model=([^,\\s]+)/);",
    "var hp=D.getElementById('hw-preset');if(hp&&hp!==D.activeElement&&dm){",
    "if(dm[1]==='TAS5805M')hp.value='Louder-ESP32';",
    "else if(dm[1]==='TAS5756M')hp.value='SqueezeAMP';",
    "else if(dm[1]==='ES8388')hp.value='ESP32-LyraT';",
    "else if(dm[1]==='PCM5102A')hp.value='Generic-I2S';}",
    "sV('autoexec1-preview','autoexec1');",
    "var a1=nvsVal('autoexec1')||'';",
    "var m=function(fl){if(!a1)return '';",
    "var re=new RegExp('(?:^|\\\\s)'+fl+'\\\\s+([^\\\\s]+)');",
    "var x=a1.match(re);return x?x[1]:'';};",
    "var sF=function(id,val,isC){var el=D.getElementById(id);if(el&&el!==D.activeElement){",
    "if(isC)el.checked=!!val;else el.value=(val===undefined||val===null)?'':String(val);}};",
    "sF('sl-dis',String(nvsVal('autoexec'))==='0'||",
    "(nvsVal('autoexec1')===''&&nvsVal('autoexec1')!==undefined),true);",
    "sF('sq-name',m('-n')||nvsVal('host_name'));",
    "sF('sq-server',m('-s')||nvsVal('lms_server_ip')||nvsVal('lms_ip'));",
    "var oVal=m('-o');",
    "if(a1){",
    "var tmpl=D.getElementById('audio-tmpl');",
    "if(tmpl&&tmpl!==D.activeElement&&(oVal==='I2S'||oVal==='SPDIF'||oVal==='BT'))tmpl.value=oVal;",
    "sF('sq-buf',m('-b'));",
    "sF('sq-codecs',m('-c'));sF('sq-rates',m('-r'));sF('sq-idle',m('-C'));",
    "sF('sq-debug',m('-d'));sF('sq-resample',m('-R')||m('-u'));",
    "sF('sl-Z',m('-Z'));sF('sl-e',m('-e'));sF('sl-m',m('-m'));",
    "sF('sq-flac',/(?:^|\\s)-W(?:\\s|$)/.test(a1),true);}",
    "var outLbl=(oVal&&oVal!=='I2S')?oVal:(dm?'I2S ('+dm[1]+')':'I2S');",
    "setBadge('badge-out','Out: '+outLbl,true);",
    "audioDirty=false;}",
    "return loadEq();}).catch(function(){});}",
    "function loadEq(){return apiFetch('/eq.json').then(function(r){return r.json();})",
    ".then(function(d){if(d&&Array.isArray(d.bands)){d.bands.forEach(function(b,i){",
    "var sl=D.getElementById('eq-b'+i);if(sl&&sl!==D.activeElement)sl.value=b;",
    "var lb=D.getElementById('eq-v'+i);if(lb)lb.innerText=b+'dB';});}",
    "}).catch(function(){});}",
    "function renderNvsTable(cfg){var nb=D.getElementById('nvsBody');",
    "if(!nb)return;nb.textContent='';",
    "Object.keys(cfg).forEach(function(k){var v=cfg[k];",
    "if(!v||typeof v!=='object'||!('value' in v))return;",
    "var val=v.value,typ=v.type||33;",
    "var tr=D.createElement('tr');",
    "tr.appendChild(mkTd(k));tr.appendChild(mkTd(typ));",
    "var tdI=D.createElement('td'),inp=D.createElement('input');",
    "inp.id='nvs-val-'+k;inp.setAttribute('data-nvs-key',k);inp.maxLength=192;",
    "inp.value=(val===undefined||val===null)?'':String(val);",
    "tdI.appendChild(inp);tr.appendChild(tdI);",
    "var tdB=D.createElement('td'),btn=D.createElement('button');btn.type='button';",
    "btn.textContent='Save';btn.addEventListener('click',function(){",
    "postConfigKey(k,inp.value,typ).then(function(){showToast('Saved '+k);})",
    ".catch(function(e){showToast(e.message||'Save failed',true);});});",
    "tdB.appendChild(btn);tr.appendChild(tdB);nb.appendChild(tr);});}",
    "function postConfigKey(k,v,t){",
    "return apiFetch('/config.json',{method:'POST',",
    "headers:{'Content-Type':'application/x-www-form-urlencoded'},",
    "body:'key='+encodeURIComponent(k)+'&value='+encodeURIComponent(v)})",
    ".then(function(r){if(!r.ok)throw new Error('HTTP '+r.status);return r.json();})",
    ".then(function(d){if(d&&d.ok===false)throw new Error(d.error||'Save failed');",
    "var prev=nvsCache[k];",
    "nvsCache[k]={value:String(v),type:t||(prev&&typeof prev==='object'&&prev.type)||33};",
    "return d;});}",
    "function postConfigBatch(pairs,msg){var p=Promise.resolve();",
    "pairs.forEach(function(kv){p=p.then(function(){return postConfigKey(kv[0],kv[1]);});});",
    "return p.then(function(){return loadConfig();})",
    ".then(function(){showToast(msg||'Configuration saved');return true;})",
    ".catch(function(e){showToast(e.message||'Save failed',true);return false;});}",
    "function addNvsRow(){var k=D.getElementById('nvs-new-key').value.trim();",
    "var t=parseInt(D.getElementById('nvs-new-type').value,10)||33;",
    "var v=D.getElementById('nvs-new-val').value;",
    "if(!k){showToast('Key required',true);return;}",
    "if(t===1||t===4){if(!/^\\d+$/.test(v.trim())){",
    "showToast('Numeric value required for u8/u32',true);return;}",
    "if(t===1&&parseInt(v,10)>255){showToast('u8 value must be 0..255',true);return;}}",
    "postConfigKey(k,v,t).then(function(){showToast('Saved '+k);return loadConfig();})",
    ".catch(function(e){showToast(e.message||'Save failed',true);});}",
    "function commitNvsTable(){",
    "var inps=D.querySelectorAll('#nvsTable input[data-nvs-key]');var chg=[];",
    "inps.forEach(function(i){var k=i.getAttribute('data-nvs-key'),v=i.value,c=nvsVal(k);",
    "if(c===undefined||String(c)!==String(v))chg.push([k,v]);});",
    "if(chg.length===0){showToast('No modified entries');return;}",
    "postConfigBatch(chg,'NVS Configuration Committed');}",
    "function exportNvsJson(){var b=new Blob([JSON.stringify(nvsCache,null,2)],",
    "{type:'application/json'});var a=D.createElement('a');a.href=URL.createObjectURL(b);",
    "a.download='louder-esp32-nvs.json';a.click();}",
    "function importNvsJson(){var p=prompt('Paste NVS JSON key-value object:');",
    "if(!p)return;try{var o=JSON.parse(p);",
    "if(!o||typeof o!=='object'||Array.isArray(o))throw new Error('Expected JSON object');",
    "var pairs=[];Object.keys(o).forEach(function(k){var v=o[k];",
    "pairs.push([k,(v&&typeof v==='object'&&v.value!==undefined)?v.value:v]);});",
    "if(pairs.length===0){showToast('No NVS entries in JSON');return;}",
    "postConfigBatch(pairs,'Imported NVS JSON');}catch(e){",
    "showToast('Invalid JSON: '+(e.message||'parse error'),true);}}",
    "function onAudioInput(){audioDirty=true;buildAutoexec1();}",
    "function applyAudioTemplate(){onAudioInput();}",
    "function buildAutoexec1(){if(D.getElementById('sl-dis').checked){",
    "D.getElementById('autoexec1-preview').value='';return;}",
    "var o=D.getElementById('audio-tmpl').value||'I2S';",
    "var c='squeezelite -o '+o;",
    "var gV=function(id){var e=D.getElementById(id);return e?e.value.trim():'';};",
    "var n=gV('sq-name');if(n)c+=' -n '+n;",
    "var s=gV('sq-server');if(s)c+=' -s '+s;",
    "var b=gV('sq-buf');if(b)c+=' -b '+b;",
    "var cd=gV('sq-codecs');if(cd)c+=' -c '+cd;",
    "var C=gV('sq-idle');if(C)c+=' -C '+C;",
    "var d=gV('sq-debug');if(d)c+=' -d '+d;",
    "var e=gV('sl-e');if(e)c+=' -e '+e;",
    "var m=gV('sl-m');if(m)c+=' -m '+m;",
    "var r=gV('sq-rates');if(r)c+=' -r '+r;",
    "var R=gV('sq-resample');if(R)c+=' -R '+R;",
    "var Z=gV('sl-Z');if(Z)c+=' -Z '+Z;",
    "var W=D.getElementById('sq-flac');if(W&&W.checked)c+=' -W';",
    "D.getElementById('autoexec1-preview').value=c;}",
    "function saveAudio(commit){",
    "var dis=D.getElementById('sl-dis'),pre=D.getElementById('autoexec1-preview');",
    "if(dis&&dis.checked){if(pre)pre.value='';}",
    "else if(audioDirty||!pre||pre.value.trim()===''){buildAutoexec1();}",
    "var c=pre?pre.value.trim():'';",
    "var pairs=[['autoexec1',c],['autoexec',(dis&&dis.checked)?'0':'1']];",
    "var srv=D.getElementById('sq-server');",
    "if(srv&&srv.value.trim())pairs.push(['lms_server_ip',srv.value.trim()]);",
    "var nm=D.getElementById('sq-name'),nv=nm?nm.value.trim():'';",
    "if(nv)pairs.push(['host_name',nv]);",
    "postConfigBatch(pairs,'Audio Configuration Saved').then(function(ok){",
    "if(!ok)return;audioDirty=false;if(nv)g('navtitle',nv);",
    "if(commit&&c)loadSyslog();});}",
    "var saveAutoexec1=saveAudio;",
    "function saveSystemConfig(){var pairs=[];",
    "['host_name','cspot_name','bt_name','ap_name'].forEach(function(k){",
    "var e=D.getElementById('cfg-'+k);if(e)pairs.push([k,e.value]);});",
    "['enable_airplay','enable_cspot','enable_bt_sink','enable_telnet','autoexec']",
    ".forEach(function(k){var e=D.getElementById('cfg-'+k);",
    "if(e)pairs.push([k,e.checked?'1':'0']);});",
    "var hn=D.getElementById('cfg-host_name');",
    "postConfigBatch(pairs,'System Configuration Saved').then(function(ok){",
    "if(ok&&hn)g('navtitle',hn.value);});}",
    "function applyHwPreset(){var m=D.getElementById('hw-preset').value;",
    "var d=D.getElementById('hw-dac_config');if(!d)return;",
    "if(m==='Louder-ESP32')d.value='model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=17:0';",
    "else if(m==='SqueezeAMP')d.value='model=TAS5756M,bck=26,ws=25,do=22,sda=21,scl=27,mute=17:0';",
    "else if(m==='ESP32-LyraT')d.value='model=ES8388,bck=5,ws=25,do=26,di=35,sda=18,scl=23';",
    "else if(m==='Generic-I2S')d.value='model=PCM5102A,bck=26,ws=25,do=22';}",
    "function saveHardwareConfig(){var pairs=[];",
    "['dac_config','spdif_config','rotary_config','i2c_config','spi_config','display_config']",
    ".forEach(function(k){var e=D.getElementById('hw-'+k);if(e)pairs.push([k,e.value]);});",
    "var vu=D.getElementById('led-vu');if(vu)pairs.push(['led_vu_config',vu.value.trim()]);",
    "var br=D.getElementById('led-brightness');",
    "if(br)pairs.push(['led_brightness',br.value.trim()]);",
    "postConfigBatch(pairs,'Hardware Configuration Saved');}",
    "function triggerUrlOta(){var u=D.getElementById('fw-url-input').value.trim();",
    "if(!u){showToast('Firmware URL required',true);return;}",
    "postConfigKey('fwurl',u).then(function(){",
    "g('otadiv','OTA URL staged: '+u);showToast('OTA URL staged');})",
    ".catch(function(e){g('otadiv','OTA URL error: '+(e.message||'failed'));",
    "showToast(e.message||'OTA URL failed',true);});}",
    "function uploadLocalOta(){var f=D.getElementById('flashfilename');",
    "if(!f||!f.files||!f.files[0]){showToast('No file selected',true);return;}",
    "g('otadiv','Starting streaming OTA to ota_0 (0x160000)...');",
    "apiFetch('/flash.json',{method:'POST',body:f.files[0]})",
    ".then(function(r){if(!r.ok)throw new Error('HTTP '+r.status);return r.json();})",
    ".then(function(d){if(d&&d.ok===false)throw new Error(d.error||'OTA failed');",
    "g('otadiv','OTA State: '+d.ota_state+' ('+d.target_partition+')');",
    "showToast('OTA Flash: '+d.ota_state);})",
    ".catch(function(e){g('otadiv','OTA Flash failed: '+(e.message||'error'));",
    "showToast('OTA Flash failed',true);});}",
    "function runCliCmd(c){var cmdStr=(c||'').trim();if(!cmdStr)return;",
    "apiFetch('/commands.json',{method:'POST',",
    "headers:{'Content-Type':'application/json'},body:JSON.stringify({command:cmdStr})})",
    ".then(function(r){if(!r.ok)throw new Error('HTTP '+r.status);return r.json();})",
    ".then(function(d){if(d&&d.ok===false)throw new Error(d.error||'Command failed');",
    "if(d&&d.output){g('cli-output','> '+cmdStr+'\\n'+d.output);return loadSyslog();}",
    "return loadSyslog().then(function(m){",
    "var msgs=(m&&m.messages)?m.messages.filter(function(x){",
    "return x.class!=='MESSAGING_CLASS_STATS';}):[];",
    "var last=msgs.length?msgs[msgs.length-1].msg:(d.Result||'OK');",
    "g('cli-output','> '+cmdStr+'\\n'+last);});})",
    ".catch(function(e){g('cli-output','Error: '+(e.message||'Command failed'));",
    "showToast(e.message||'Command failed',true);});}",
    "function toggleAdvTabs(){",
    "var sn=D.getElementById('show-nvs').checked;",
    "var sc=D.getElementById('show-commands').checked;",
    "var nn=D.getElementById('nav-nvs'),nc=D.getElementById('nav-cmd');",
    "nn.style.display=sn?'':'none';nn.hidden=!sn;",
    "nc.style.display=sc?'':'none';nc.hidden=!sc;",
    "try{localStorage.setItem('show_nvs',sn?'1':'0');",
    "localStorage.setItem('show_cmd',sc?'1':'0');}catch(e){}}",
    "(function(){try{var sn=localStorage.getItem('show_nvs');",
    "var sc=localStorage.getItem('show_cmd');",
    "var en=D.getElementById('show-nvs'),ec=D.getElementById('show-commands');",
    "if(en&&sn!==null)en.checked=(sn!=='0');",
    "if(ec&&sc!==null)ec.checked=(sc!=='0');",
    "toggleAdvTabs();}catch(e){}})();",
    "loadConfig().finally(function(){u();});setInterval(u,1500);</script></body></html>"
);

#[cfg(test)]
mod tests {
    use super::WEB_UI_HTML;

    fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> &'a str {
        let (_, tail) = haystack
            .split_once(start)
            .unwrap_or_else(|| panic!("missing start marker: {start}"));
        let (slice, _) = tail
            .split_once(end)
            .unwrap_or_else(|| panic!("missing end marker: {end}"));
        slice
    }

    /// Checks whether `{}` / `()` / `[]` delimiters are balanced outside JS
    /// string, template, comment, and regex literals.
    fn check_js_delimiters_balanced(src: &str) -> Result<(), &'static str> {
        let bytes = src.as_bytes();
        let mut stack: std::vec::Vec<u8> = std::vec::Vec::new();
        let mut i = 0usize;
        let mut prev_sig: u8 = b';';

        while i < bytes.len() {
            let b = bytes[i];
            if b.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                let mut closed = false;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err("unclosed block comment");
                }
                continue;
            }
            if b == b'\'' || b == b'"' || b == b'`' {
                let quote = b;
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err("unclosed string literal");
                }
                prev_sig = quote;
                continue;
            }
            if b == b'/'
                && matches!(
                    prev_sig,
                    b'=' | b'('
                        | b','
                        | b'!'
                        | b'&'
                        | b'|'
                        | b'?'
                        | b':'
                        | b';'
                        | b'{'
                        | b'}'
                        | b'['
                )
            {
                i += 1;
                let mut in_class = false;
                let mut closed = false;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'[' {
                        in_class = true;
                    } else if bytes[i] == b']' {
                        in_class = false;
                    } else if bytes[i] == b'/' && !in_class {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err("unclosed regex literal");
                }
                prev_sig = b'/';
                continue;
            }
            match b {
                b'{' | b'(' | b'[' => {
                    stack.push(b);
                    prev_sig = b;
                }
                b'}' | b')' | b']' => {
                    let expected = match b {
                        b'}' => b'{',
                        b')' => b'(',
                        _ => b'[',
                    };
                    if stack.pop() != Some(expected) {
                        return Err("mismatched closing delimiter");
                    }
                    prev_sig = b;
                }
                _ => {
                    prev_sig = b;
                }
            }
            i += 1;
        }
        if !stack.is_empty() {
            return Err("unclosed opening delimiter");
        }
        Ok(())
    }

    #[test]
    fn test_script_blocks_have_balanced_delimiters_and_valid_js_syntax() {
        assert_eq!(
            check_js_delimiters_balanced("function f(){if(a){if(b){x();}}"),
            Err("unclosed opening delimiter")
        );
        assert_eq!(
            check_js_delimiters_balanced("Promise.resolve().then(d=>{if(a){if(b){x();}}).catch();"),
            Err("mismatched closing delimiter")
        );

        let mut rem = WEB_UI_HTML;
        let mut count = 0usize;
        while let Some((_, after_open)) = rem.split_once("<script>") {
            let (script, rest) = after_open
                .split_once("</script>")
                .unwrap_or_else(|| panic!("unclosed <script> tag"));
            assert_eq!(check_js_delimiters_balanced(script), Ok(()));

            if let Ok(ver) = std::process::Command::new("node").arg("--version").output() {
                if ver.status.success() {
                    let tmp_path = std::env::temp_dir().join(std::format!(
                        "louder_web_ui_check_{}_{count}.js",
                        std::process::id()
                    ));
                    std::fs::write(&tmp_path, script)
                        .unwrap_or_else(|e| panic!("failed to write temp js: {e}"));
                    let out = std::process::Command::new("node")
                        .arg("--check")
                        .arg(&tmp_path)
                        .output()
                        .unwrap_or_else(|e| panic!("failed to run node --check: {e}"));
                    let _ = std::fs::remove_file(&tmp_path);
                    assert!(
                        out.status.success(),
                        "node --check failed: {}",
                        std::string::String::from_utf8_lossy(&out.stderr)
                    );
                }
            }
            count += 1;
            rem = rest;
        }
        assert!(count >= 1);
    }

    #[test]
    fn test_web_ui_has_no_inner_html_sinks() {
        assert!(!WEB_UI_HTML.contains(".innerHTML"));
        assert!(WEB_UI_HTML.contains("function mkTd("));
        assert!(WEB_UI_HTML.contains("D.createElement('td')"));
        assert!(WEB_UI_HTML.contains(".textContent"));
    }

    #[test]
    fn test_scan_wifi_builds_safe_dom_without_inline_onclick() {
        let body = extract_between(WEB_UI_HTML, "function scanWifi()", "function connectWifi()");
        assert!(body.contains("mkTd(a.ssid)"));
        assert!(body.contains("mkTd(a.chan)"));
        assert!(body.contains("mkTd(authLabel(a.auth))"));
        assert!(body.contains("D.createElement('button')"));
        assert!(body.contains("btn.type='button'"));
        assert!(body.contains("addEventListener('click'"));
        assert!(!body.contains("onclick"));
        assert!(!body.contains("innerHTML"));
    }

    #[test]
    fn test_render_nvs_table_builds_safe_dom_without_inline_onclick() {
        let body = extract_between(
            WEB_UI_HTML,
            "function renderNvsTable(cfg)",
            "function postConfigKey(",
        );
        assert!(body.contains("mkTd(k)"));
        assert!(body.contains("mkTd(typ)"));
        assert!(body.contains("D.createElement('input')"));
        assert!(body.contains("setAttribute('data-nvs-key',k)"));
        assert!(body.contains("D.createElement('button')"));
        assert!(body.contains("addEventListener('click'"));
        assert!(!body.contains("onclick"));
        assert!(!body.contains("innerHTML"));
    }

    #[test]
    fn test_syslog_and_gpio_tables_use_safe_dom_cells() {
        let syslog = extract_between(
            WEB_UI_HTML,
            "function loadSyslog()",
            "function clearSyslog()",
        );
        assert!(syslog.contains("mkTd(m.msg)"));
        assert!(syslog.contains("mkTd(m.class||'')"));
        assert!(!syslog.contains("innerHTML"));

        let gpio = extract_between(WEB_UI_HTML, "function loadConfig()", "function loadEq()");
        assert!(gpio.contains("mkTd('GPIO '+p.gpio)"));
        assert!(gpio.contains("mkTd(p.name)"));
        assert!(gpio.contains("mkTd(p.group)"));
        assert!(!gpio.contains("innerHTML"));
    }

    #[test]
    fn test_adv_tab_visibility_restored_before_initial_poll() {
        let script = extract_between(WEB_UI_HTML, "<script>", "</script>");
        let restore_idx = script
            .find("localStorage.getItem('show_nvs')")
            .unwrap_or_else(|| panic!("missing show_nvs restore"));
        let cmd_idx = script
            .find("localStorage.getItem('show_cmd')")
            .unwrap_or_else(|| panic!("missing show_cmd restore"));
        let poll_idx = script
            .find("loadConfig().finally(function(){u();});setInterval(u,1500);")
            .unwrap_or_else(|| panic!("missing initial poll"));
        assert!(restore_idx < poll_idx);
        assert!(cmd_idx < poll_idx);
        assert!(script[restore_idx..poll_idx].contains("toggleAdvTabs()"));
    }

    #[test]
    fn test_tdd_fix_b_startup_and_audio_tab_invoke_load_config() {
        let script = extract_between(WEB_UI_HTML, "<script>", "</script>");
        let show_tab = extract_between(script, "function showTab(id,btn)", "function cmd(");
        assert!(show_tab.contains("tab-cfg-audio"));
        assert!(show_tab.contains("loadConfig()"));

        let tail_start = script
            .find("toggleAdvTabs();")
            .unwrap_or_else(|| panic!("missing toggleAdvTabs startup call"));
        let startup_tail = &script[tail_start..];
        assert!(startup_tail.contains("loadConfig()"));
    }

    #[test]
    fn test_tdd_fix_b_load_config_populates_server_and_name_fallbacks() {
        let body = extract_between(WEB_UI_HTML, "function loadConfig()", "function loadEq()");
        assert!(body.contains("m('-n')||nvsVal('host_name')"));
        assert!(body.contains("m('-s')||nvsVal('lms_server_ip')||nvsVal('lms_ip')"));
        let fallback_idx = body
            .find("m('-s')||nvsVal('lms_server_ip')||nvsVal('lms_ip')")
            .unwrap_or_else(|| panic!("missing lms_server_ip fallback"));
        let a1_if_idx = body
            .find("if(a1){")
            .unwrap_or_else(|| panic!("missing if(a1) guard"));
        assert!(fallback_idx < a1_if_idx);
    }

    #[test]
    fn test_tdd_fix_b_post_config_key_validates_response_before_cache_mutation() {
        let body = extract_between(
            WEB_UI_HTML,
            "function postConfigKey(k,v,t)",
            "function postConfigBatch(",
        );
        assert!(body.contains("!r.ok"));
        assert!(body.contains("d.ok===false"));
        assert!(body.contains("throw new Error"));
        let fetch_idx = body
            .find("apiFetch('/config.json'")
            .unwrap_or_else(|| panic!("missing apiFetch call"));
        let cache_idx = body
            .find("nvsCache[k]=")
            .unwrap_or_else(|| panic!("missing nvsCache assignment"));
        assert!(fetch_idx < cache_idx);
    }

    #[test]
    fn test_tdd_fix_b_save_audio_syncs_autoexec1_and_applies_explicit_overrides_after() {
        let body = extract_between(
            WEB_UI_HTML,
            "function saveAudio(commit)",
            "var saveAutoexec1=saveAudio;",
        );
        let build_idx = body
            .find("buildAutoexec1()")
            .unwrap_or_else(|| panic!("missing buildAutoexec1() sync in saveAudio"));
        let read_idx = body
            .find("var c=")
            .unwrap_or_else(|| panic!("missing preview value read"));
        assert!(build_idx < read_idx);
        let pairs_idx = body
            .find("['autoexec1',c]")
            .unwrap_or_else(|| panic!("missing autoexec1 pair"));
        let srv_idx = body
            .find("'lms_server_ip'")
            .unwrap_or_else(|| panic!("missing lms_server_ip pair"));
        let host_idx = body
            .find("'host_name'")
            .unwrap_or_else(|| panic!("missing host_name pair"));
        assert!(pairs_idx < srv_idx);
        assert!(pairs_idx < host_idx);
    }

    #[test]
    fn test_api_fetch_queue_recovery_and_no_hardcoded_device_endpoints() {
        assert!(!WEB_UI_HTML.contains("172.22.1.15"));
        assert!(!WEB_UI_HTML.contains("10.0.0.10:9000"));
        assert!(!WEB_UI_HTML.contains("10.0.0.10"));
        assert!(WEB_UI_HTML.contains("role=\"tablist\""));
        assert!(WEB_UI_HTML.contains("onchange=\"saveEq()\""));
        assert!(WEB_UI_HTML.contains("onEqInput(0,this.value)"));

        let api_fetch =
            extract_between(WEB_UI_HTML, "function apiFetch(u,o)", "function showToast(");
        assert!(api_fetch.contains("apiQ.catch("));
        assert!(api_fetch.contains("apiQ=p.catch("));
    }
}
