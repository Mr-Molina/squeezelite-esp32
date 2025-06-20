# Freenove ESP32-WROVER-CAM Audio-Only Configuration

## 🎵 Optimal GPIO Mapping for Audio Device

### **I2S Audio Output (Using Former Camera Pins)**
```
I2S Function      GPIO   Original Camera Use    Wire Color Suggestion
═══════════════════════════════════════════════════════════════════════
BCK (Bit Clock)    26    SIOD (Camera I2C SDA)    Blue
WS (Word Select)   25    VSYNC (Camera sync)       White  
DO (Data Out)      22    PCLK (Camera clock)       Green
```

### **I2C Control (For DAC Configuration)**
```
I2C Function      GPIO   Original Camera Use
═══════════════════════════════════════════════
SDA (Data)         21    XCLK (Camera clock)
SCL (Clock)        27    SIOC (Camera I2C SCL)
```

### **Control Interface**
```
Control Function   GPIO   Original Camera Use    Purpose
═══════════════════════════════════════════════════════
Rotary Encoder A   18    Y4 (Camera data)       Volume/Track
Rotary Encoder B   19    Y5 (Camera data)       Volume/Track  
Rotary Button      5     Y3 (Camera data)       Play/Pause
Power Button       4     Y2 (Camera data)       Power On/Off
Green LED          2     Built-in LED           Status Good
Red LED           23    HREF (Camera sync)      Status Error
Jack Detection    34    Y8 (Camera data)       Headphone detect
Amp Control       35    Y9 (Camera data)       Amplifier on/off
```

### **Available for Additional Features**
```
GPIO   Original Camera Use    Available For
═══════════════════════════════════════════════
36     Y6 (Camera data)       Button/Sensor input
39     Y7 (Camera data)       Button/Sensor input  
0      Boot (pull-up)         Button (advanced)
1      TX (UART)              Debug/Button
3      RX (UART)              Debug/Button
12     HSPI MISO              SPI Display
13     HSPI MOSI              SPI Display
14     HSPI CLK               SPI Display
15     HSPI CS                SPI Display
16     Free GPIO              Any purpose
17     Free GPIO              Any purpose
32     Free GPIO              Any purpose
33     Free GPIO              Any purpose
```

## 🛠️ Configuration Commands

### **ESP-IDF MenuConfig Settings**
```bash
# Navigate to Audio I2S Settings
Component config → Audio → I2S Settings:
CONFIG_I2S_BCK_IO=26
CONFIG_I2S_WS_IO=25  
CONFIG_I2S_DO_IO=22

# I2C for DAC control
Component config → Audio → I2C Settings:
CONFIG_I2C_SDA=21
CONFIG_I2C_SCL=27

# GPIO Controls
CONFIG_SET_GPIO="2=green:1,23=red:1,34=jack:0,35=amp:1"
CONFIG_ROTARY_ENCODER="A=18,B=19,SW=5,volume"
```

### **Recommended Audio Setup**
```bash
# High-quality I2S DAC examples:
- PCM5102A (32-bit, 384kHz capable)
- ES9023 (24-bit, 192kHz)  
- MAX98357A (Class D amplifier with I2S)

# Connection example for PCM5102A:
VCC  → 3.3V
GND  → GND  
SCK  → GPIO 26 (BCK)
LCK  → GPIO 25 (WS)
DIN  → GPIO 22 (DO)
```

## 📋 **Build Configuration**

Since camera is disabled, optimize for audio performance:

```bash
# Disable camera components
CONFIG_CAMERA_ENABLED=n

# Optimize for audio
CONFIG_AUDIO_BOARD_CUSTOM=y
CONFIG_FREERTOS_HZ=1000
CONFIG_ESP32_DEFAULT_CPU_FREQ_240=y

# Memory optimization  
CONFIG_SPIRAM_USE_MALLOC=y
CONFIG_SPIRAM_USE_CAPS_ALLOC=y
```

## 🎯 **Advantages of This Configuration**

✅ **Excellent Audio Quality** - Dedicated I2S pins, no interference  
✅ **Full Control Interface** - Rotary encoder, buttons, status LEDs  
✅ **Expandable** - Many free GPIOs for future features  
✅ **Professional Features** - Jack detection, amp control  
✅ **Easy Programming** - Built-in USB programmer  
✅ **Stable Power** - USB power with good regulation  

## 🚀 **Next Steps**

1. **Wire your I2S DAC** using the GPIO mapping above
2. **Build and flash** using existing VS Code tasks
3. **Configure audio settings** via web interface
4. **Add controls** (rotary encoder, buttons) as needed
