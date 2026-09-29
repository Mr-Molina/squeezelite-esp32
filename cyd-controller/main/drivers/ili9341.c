#include "ili9341.h"
#include <string.h>

#if defined(ESP_PLATFORM)
#include "driver/spi_master.h"
#include "driver/gpio.h"
#include "driver/ledc.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "ili9341";
static spi_device_handle_t s_spi = NULL;

static void ili9341_spi_pre_transfer_callback(spi_transaction_t *t) {
    int dc = (int)(intptr_t)t->user;
    gpio_set_level((gpio_num_t)CYD_TFT_DC, dc);
}

static void ili9341_send_cmd(uint8_t cmd) {
    spi_transaction_t t;
    memset(&t, 0, sizeof(t));
    t.length = 8;
    t.tx_buffer = &cmd;
    t.user = (void *)(intptr_t)0; // DC = 0 for command
    spi_device_polling_transmit(s_spi, &t);
}

static void ili9341_send_data(const uint8_t *data, int len) {
    if (len <= 0) return;
    spi_transaction_t t;
    memset(&t, 0, sizeof(t));
    t.length = len * 8;
    t.tx_buffer = data;
    t.user = (void *)(intptr_t)1; // DC = 1 for data
    spi_device_polling_transmit(s_spi, &t);
}
#endif

static int32_t s_last_flush_x1 = 0;
static int32_t s_last_flush_y1 = 0;
static int32_t s_last_flush_x2 = 0;
static int32_t s_last_flush_y2 = 0;
static uint32_t s_flush_count = 0;
static uint8_t s_backlight_percent = 100;
static bool s_initialized = false;

esp_err_t ili9341_init(void) {
#if defined(ESP_PLATFORM)
    // Configure DC GPIO pin
    gpio_config_t io_conf = {
        .pin_bit_mask = (1ULL << CYD_TFT_DC),
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    gpio_config(&io_conf);
    gpio_set_level((gpio_num_t)CYD_TFT_DC, 1);

    // Initialize SPI bus on SPI2_HOST (HSPI)
    spi_bus_config_t buscfg = {
        .mosi_io_num = CYD_TFT_MOSI,
        .miso_io_num = CYD_TFT_MISO,
        .sclk_io_num = CYD_TFT_SCLK,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .max_transfer_sz = CYD_TFT_WIDTH * CYD_TFT_HEIGHT * 2 + 8,
    };
    esp_err_t ret = spi_bus_initialize(SPI2_HOST, &buscfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK && ret != ESP_ERR_INVALID_STATE) {
        ESP_LOGE(TAG, "Failed to initialize SPI bus: %d", ret);
        return ret;
    }

    // Attach ILI9341 SPI device
    spi_device_interface_config_t devcfg = {
        .clock_speed_hz = 40 * 1000 * 1000, // 40 MHz clock
        .mode = 0,                          // SPI mode 0 (CPOL=0, CPHA=0)
        .spics_io_num = CYD_TFT_CS,
        .queue_size = 7,
        .pre_cb = ili9341_spi_pre_transfer_callback,
    };
    ret = spi_bus_add_device(SPI2_HOST, &devcfg, &s_spi);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to add SPI device: %d", ret);
        return ret;
    }

    // Configure Backlight via LEDC PWM on GPIO 21
    ledc_timer_config_t ledc_timer = {
        .speed_mode       = LEDC_LOW_SPEED_MODE,
        .timer_num        = LEDC_TIMER_0,
        .duty_resolution  = LEDC_TIMER_10_BIT,
        .freq_hz          = 5000,
        .clk_cfg          = LEDC_AUTO_CLK
    };
    ledc_timer_config(&ledc_timer);

    ledc_channel_config_t ledc_channel = {
        .speed_mode     = LEDC_LOW_SPEED_MODE,
        .channel        = LEDC_CHANNEL_0,
        .timer_sel      = LEDC_TIMER_0,
        .intr_type      = LEDC_INTR_DISABLE,
        .gpio_num       = CYD_TFT_BL,
        .duty           = 1023, // 100% duty initial
        .hpoint         = 0
    };
    ledc_channel_config(&ledc_channel);

    // ILI9341 Initialization Sequence
    ili9341_send_cmd(0x01); // Software Reset
    vTaskDelay(pdMS_TO_TICKS(100));

    ili9341_send_cmd(0x28); // Display OFF

    // Power Control B (0xCF)
    ili9341_send_cmd(0xCF);
    const uint8_t d_cf[] = {0x00, 0xC1, 0x30};
    ili9341_send_data(d_cf, sizeof(d_cf));

    // Power on Sequence Control (0xED)
    ili9341_send_cmd(0xED);
    const uint8_t d_ed[] = {0x64, 0x03, 0x12, 0x81};
    ili9341_send_data(d_ed, sizeof(d_ed));

    // Driver Timing Control A (0xE8)
    ili9341_send_cmd(0xE8);
    const uint8_t d_e8[] = {0x85, 0x00, 0x78};
    ili9341_send_data(d_e8, sizeof(d_e8));

    // Power Control A (0xCB)
    ili9341_send_cmd(0xCB);
    const uint8_t d_cb[] = {0x39, 0x2C, 0x00, 0x34, 0x02};
    ili9341_send_data(d_cb, sizeof(d_cb));

    // Pump Ratio Control (0xF7)
    ili9341_send_cmd(0xF7);
    const uint8_t d_f7[] = {0x20};
    ili9341_send_data(d_f7, sizeof(d_f7));

    // Driver Timing Control B (0xEA)
    ili9341_send_cmd(0xEA);
    const uint8_t d_ea[] = {0x00, 0x00};
    ili9341_send_data(d_ea, sizeof(d_ea));

    // Power Control 1 (0xC0)
    ili9341_send_cmd(0xC0);
    const uint8_t d_c0[] = {0x23};
    ili9341_send_data(d_c0, sizeof(d_c0));

    // Power Control 2 (0xC1)
    ili9341_send_cmd(0xC1);
    const uint8_t d_c1[] = {0x10};
    ili9341_send_data(d_c1, sizeof(d_c1));

    // VCOM Control 1 (0xC5)
    ili9341_send_cmd(0xC5);
    const uint8_t d_c5[] = {0x3E, 0x28};
    ili9341_send_data(d_c5, sizeof(d_c5));

    // VCOM Control 2 (0xC7)
    ili9341_send_cmd(0xC7);
    const uint8_t d_c7[] = {0x86};
    ili9341_send_data(d_c7, sizeof(d_c7));

    // MADCTL: Memory Access Control (0x36) - Landscape MV=1, BGR=1 (0x28)
    ili9341_send_cmd(0x36);
    const uint8_t d_36[] = {0x28};
    ili9341_send_data(d_36, sizeof(d_36));

    // Pixel Format Set: 16-bit RGB565 (0x55)
    ili9341_send_cmd(0x3A);
    const uint8_t d_3a[] = {0x55};
    ili9341_send_data(d_3a, sizeof(d_3a));

    // Frame Rate Control (0xB1)
    ili9341_send_cmd(0xB1);
    const uint8_t d_b1[] = {0x00, 0x18};
    ili9341_send_data(d_b1, sizeof(d_b1));

    // Display Function Control (0xB6)
    ili9341_send_cmd(0xB6);
    const uint8_t d_b6[] = {0x08, 0x82, 0x27};
    ili9341_send_data(d_b6, sizeof(d_b6));

    // 3Gamma Function Disable (0xF2)
    ili9341_send_cmd(0xF2);
    const uint8_t d_f2[] = {0x00};
    ili9341_send_data(d_f2, sizeof(d_f2));

    // Gamma Curve Selected (0x26)
    ili9341_send_cmd(0x26);
    const uint8_t d_26[] = {0x01};
    ili9341_send_data(d_26, sizeof(d_26));

    // Positive Gamma Correction (0xE0)
    ili9341_send_cmd(0xE0);
    const uint8_t d_e0[] = {0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E, 0x09, 0x00};
    ili9341_send_data(d_e0, sizeof(d_e0));

    // Negative Gamma Correction (0xE1)
    ili9341_send_cmd(0xE1);
    const uint8_t d_e1[] = {0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31, 0x36, 0x0F};
    ili9341_send_data(d_e1, sizeof(d_e1));

    // Sleep OUT (0x11)
    ili9341_send_cmd(0x11);
    vTaskDelay(pdMS_TO_TICKS(120));

    // Display ON (0x29)
    ili9341_send_cmd(0x29);
    vTaskDelay(pdMS_TO_TICKS(20));

    ESP_LOGI(TAG, "ILI9341 display initialized successfully (320x240 landscape)");
#endif
    s_initialized = true;
    return ESP_OK;
}

void ili9341_flush(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const uint16_t *color_data) {
    if (!color_data) return;

    // Normalize inverted coordinates if necessary
    if (x1 > x2) {
        int32_t tmp = x1;
        x1 = x2;
        x2 = tmp;
    }
    if (y1 > y2) {
        int32_t tmp = y1;
        y1 = y2;
        y2 = tmp;
    }

    // Check if window is completely out of visible screen bounds
    if (x2 < 0 || y2 < 0 || x1 >= CYD_TFT_WIDTH || y1 >= CYD_TFT_HEIGHT) {
        return;
    }

    // Clamp coordinates to screen bounds
    if (x1 < 0) x1 = 0;
    if (y1 < 0) y1 = 0;
    if (x2 >= CYD_TFT_WIDTH) x2 = CYD_TFT_WIDTH - 1;
    if (y2 >= CYD_TFT_HEIGHT) y2 = CYD_TFT_HEIGHT - 1;

    // Safety guard after clamping
    if (x1 > x2 || y1 > y2) {
        return;
    }

    s_last_flush_x1 = x1;
    s_last_flush_y1 = y1;
    s_last_flush_x2 = x2;
    s_last_flush_y2 = y2;
    s_flush_count++;

#if defined(ESP_PLATFORM)
    if (!s_spi) return;

    // Column Address Set (CASET: 0x2A)
    ili9341_send_cmd(0x2A);
    uint8_t col_data[4] = {
        (uint8_t)((x1 >> 8) & 0xFF),
        (uint8_t)(x1 & 0xFF),
        (uint8_t)((x2 >> 8) & 0xFF),
        (uint8_t)(x2 & 0xFF)
    };
    ili9341_send_data(col_data, sizeof(col_data));

    // Page Address Set (PASET: 0x2B)
    ili9341_send_cmd(0x2B);
    uint8_t page_data[4] = {
        (uint8_t)((y1 >> 8) & 0xFF),
        (uint8_t)(y1 & 0xFF),
        (uint8_t)((y2 >> 8) & 0xFF),
        (uint8_t)(y2 & 0xFF)
    };
    ili9341_send_data(page_data, sizeof(page_data));

    // Memory Write (RAMWR: 0x2C)
    ili9341_send_cmd(0x2C);
    size_t pixel_count = (size_t)(x2 - x1 + 1) * (size_t)(y2 - y1 + 1);
    size_t byte_count = pixel_count * 2;

    const uint8_t *p = (const uint8_t *)color_data;
    while (byte_count > 0) {
        size_t chunk = (byte_count > 4096) ? 4096 : byte_count;
        ili9341_send_data(p, (int)chunk);
        p += chunk;
        byte_count -= chunk;
    }
#endif
}

void ili9341_set_backlight(uint8_t percent) {
    if (percent > 100) percent = 100;
    s_backlight_percent = percent;

#if defined(ESP_PLATFORM)
    uint32_t duty = (uint32_t)((percent * 1023) / 100);
    ledc_set_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0, duty);
    ledc_update_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0);
#endif
}

void ili9341_get_last_flush_window(int32_t *out_x1, int32_t *out_y1, int32_t *out_x2, int32_t *out_y2) {
    if (out_x1) *out_x1 = s_last_flush_x1;
    if (out_y1) *out_y1 = s_last_flush_y1;
    if (out_x2) *out_x2 = s_last_flush_x2;
    if (out_y2) *out_y2 = s_last_flush_y2;
}

uint32_t ili9341_get_flush_count(void) {
    return s_flush_count;
}

uint8_t ili9341_get_backlight(void) {
    return s_backlight_percent;
}
