# Work Order: WO-081 — Sonocotta Louder-ESP32 Hardware Schematic, Pinout, NVS Config & Board Capabilities Audit

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-081` |
| **Timestamp** | `2026-10-25T11:06:00-04:00` |
| **Assigned Role** | Hardware & Board Capabilities Auditor (`Model: "pro"`, `TypeName: "research"`) |
| **Target Scope** | Repository C/Rust files (`components/`, `main/`, `rust/`), NVS `0x9000` configuration of `"BASEMENT"` (`172.22.1.15`) & sibling players (`KITCHEN`, `DININGROOM`, `LIVINGROOM`), and Sonocotta Louder-ESP32 hardware specifications |
| **Status** | `[IN PROGRESS]` |

## 1. Objective
Conduct an exhaustive, zero-blind-spot audit of all hardware features on the **Sonocotta Louder-ESP32** board (`ESP32-WROVER` 4 MB Flash / 4 MB PSRAM + TI `TAS5805M`) across our repository, NVS configurations, and documentation to identify every physical peripheral, pin, header, and hardware capability that is currently unused or ignored by `louder-esp32-app`.

## 2. Required Inspection Areas
1. **Legacy C Firmware & NVS Ground Truth (`components/`, `sdkconfig*`, NVS `0x9000`)**:
   - Search the repository (`components/`, `main/`, `boards/`, `sdkconfig*`, `rust/`) and any NVS dumps / work orders for all hardware configurations associated with `I2S-4MFlash` / Sonocotta Louder-ESP32:
     - `dac_config` (`model=TAS5805M`, `bck=26`, `ws=25`, `do=22`, `sda=21`, `scl=27`, `mute=33:0` or `pwdn=33`, `fault=34`)
     - `spi_config` / `eth_config` (W5500 SPI Ethernet header on Louder-ESP32: `clk`, `mosi`, `miso`, `cs`, `int`, `rst`)
     - Addressable RGB LED / WS2812 (`led_vu_config` / status LED GPIO on Louder-ESP32)
     - Infrared (IR) Receiver GPIO (`ir_config` / NEC IR sensor on Louder-ESP32)
     - I2C / SPI OLED Display (`display_config`), Rotary Encoder (`rotary_config`), GPIO Expander (`gpio_exp_config`), Battery / Voltage sense (`bat_config`)
     - CYD UART2 link (`GPIO4` TX, `GPIO36` RX)
2. **ESP32-WROVER Silicon & SoC Peripherals**:
   - **4 MB Quad-SPI PSRAM** (`GPIO16` CS, `GPIO17` CLK — how C firmware and `esp-hal 1.0.0` initialize and map `0x3F80_0000..0x3FC0_0000`, DMA constraints on PSRAM vs internal SRAM).
   - **Dual-Core Xtensa LX6 (`PRO_CPU` Core 0 + `APP_CPU` Core 1 at 240 MHz)**: verify how `esp-hal 1.0.0` (`esp_hal::system::CpuControl`) starts Core 1 (`APP_CPU`) and how work was partitioned across Core 0 and Core 1 in the C firmware vs our single-core Rust loop.
   - **Hardware Audio PLL (`APLL`)**: verify how ESP32 `I2S0` uses `APLL` (`rtc_clk_apll_enable` / `I2S0_CLKM_CONF_REG` `clk_en` / `clka_en`) for zero-drift `44,100.00 Hz` / `48,000.00 Hz` master clock generation vs our current `PLL_D2` (`160 MHz`) fractional divider.
   - **Hardware RMT, PCNT, LEDC, MCPWM, ADC, Crypto (SHA/AES/RSA/RNG), and Flash Partitions** (`settings` partition at `0x140000`, `nvs` at `0x9000`).

## 3. Deliverable
Return a structured Markdown report listing every hardware feature of the Sonocotta Louder-ESP32 & ESP32-WROVER, its exact GPIO/register/partition mapping, whether it is currently `[USED]`, `[PARTIALLY USED]`, or `[IGNORED / UNUSED]` in `louder-esp32-app`, and the exact `esp-hal 1.0.0` / register mechanism to enable and maximize it.
