#include "xpt2046.h"
#include "ili9341.h"
#include <string.h>

#if defined(ESP_PLATFORM)
#include "driver/spi_master.h"
#include "driver/gpio.h"
#include "esp_log.h"

static const char *TAG = "xpt2046";
static spi_device_handle_t s_touch_spi = NULL;

#define XPT2046_SAMPLE_COUNT 5

static uint16_t xpt2046_read_raw(uint8_t cmd) {
    uint8_t tx_data[3] = { cmd, 0x00, 0x00 };
    uint8_t rx_data[3] = { 0, 0, 0 };

    spi_transaction_t t;
    memset(&t, 0, sizeof(t));
    t.length = 24; // 3 bytes
    t.tx_buffer = tx_data;
    t.rx_buffer = rx_data;

    esp_err_t ret = spi_device_polling_transmit(s_touch_spi, &t);
    if (ret != ESP_OK) return 0;

    // 12-bit ADC value in bits 14..3 of 16-bit response
    uint16_t val = ((((uint16_t)rx_data[1]) << 8) | rx_data[2]) >> 3;
    return val & 0x0FFF;
}

static bool xpt2046_sample_filtered(int16_t *avg_x, int16_t *avg_y) {
    int32_t sum_x = 0;
    int32_t sum_y = 0;
    int valid_count = 0;

    for (int i = 0; i < XPT2046_SAMPLE_COUNT; i++) {
        uint16_t sx = xpt2046_read_raw(0xD0); // Channel X
        uint16_t sy = xpt2046_read_raw(0x90); // Channel Y

        if (sx >= 100 && sx <= 4050 && sy >= 100 && sy <= 4050) {
            sum_x += sx;
            sum_y += sy;
            valid_count++;
        }
    }

    if (valid_count < 3) {
        return false;
    }

    if (avg_x) *avg_x = (int16_t)(sum_x / valid_count);
    if (avg_y) *avg_y = (int16_t)(sum_y / valid_count);
    return true;
}
#else
static bool s_mock_touched = false;
static int16_t s_mock_raw_x = 0;
static int16_t s_mock_raw_y = 0;
#endif

static int16_t s_cal_x_min = XPT2046_RAW_X_MIN;
static int16_t s_cal_x_max = XPT2046_RAW_X_MAX;
static int16_t s_cal_y_min = XPT2046_RAW_Y_MIN;
static int16_t s_cal_y_max = XPT2046_RAW_Y_MAX;

esp_err_t xpt2046_init(void) {
#if defined(ESP_PLATFORM)
    // Configure IRQ GPIO (GPIO 36 is GPI input only)
    gpio_config_t irq_conf = {
        .pin_bit_mask = (1ULL << CYD_TOUCH_IRQ),
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    gpio_config(&irq_conf);

    // Initialize SPI bus on SPI3_HOST (VSPI)
    spi_bus_config_t buscfg = {
        .mosi_io_num = CYD_TOUCH_MOSI,
        .miso_io_num = CYD_TOUCH_MISO,
        .sclk_io_num = CYD_TOUCH_SCLK,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .max_transfer_sz = 32,
    };
    esp_err_t ret = spi_bus_initialize(SPI3_HOST, &buscfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK && ret != ESP_ERR_INVALID_STATE) {
        ESP_LOGE(TAG, "Failed to initialize Touch SPI bus: %d", ret);
        return ret;
    }

    // Attach XPT2046 SPI device (2MHz clock, Mode 0)
    spi_device_interface_config_t devcfg = {
        .clock_speed_hz = 2 * 1000 * 1000, // 2 MHz
        .mode = 0,                         // SPI mode 0
        .spics_io_num = CYD_TOUCH_CS,
        .queue_size = 3,
    };
    ret = spi_bus_add_device(SPI3_HOST, &devcfg, &s_touch_spi);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to add Touch SPI device: %d", ret);
        return ret;
    }

    ESP_LOGI(TAG, "XPT2046 touch driver initialized (IRQ: %d, CS: %d)", CYD_TOUCH_IRQ, CYD_TOUCH_CS);
#endif
    return ESP_OK;
}

void xpt2046_calibrate_raw(int16_t raw_x, int16_t raw_y, int16_t *out_x, int16_t *out_y) {
    if (!out_x && !out_y) return;

    int32_t x_range = (int32_t)s_cal_x_max - (int32_t)s_cal_x_min;
    int32_t y_range = (int32_t)s_cal_y_max - (int32_t)s_cal_y_min;

    if (x_range <= 0) x_range = 1;
    if (y_range <= 0) y_range = 1;

    int32_t cal_x = (int32_t)(raw_x - s_cal_x_min) * (CYD_TFT_WIDTH - 1) / x_range;
    int32_t cal_y = (int32_t)(raw_y - s_cal_y_min) * (CYD_TFT_HEIGHT - 1) / y_range;

    // Clamp coordinates to [0, CYD_TFT_WIDTH - 1] and [0, CYD_TFT_HEIGHT - 1]
    if (cal_x < 0) cal_x = 0;
    if (cal_x >= CYD_TFT_WIDTH) cal_x = CYD_TFT_WIDTH - 1;

    if (cal_y < 0) cal_y = 0;
    if (cal_y >= CYD_TFT_HEIGHT) cal_y = CYD_TFT_HEIGHT - 1;

    if (out_x) *out_x = (int16_t)cal_x;
    if (out_y) *out_y = (int16_t)cal_y;
}

bool xpt2046_read(int16_t *out_x, int16_t *out_y) {
#if defined(ESP_PLATFORM)
    if (!s_touch_spi) return false;

    // IRQ is active LOW (0 = touched)
    if (gpio_get_level((gpio_num_t)CYD_TOUCH_IRQ) != 0) {
        return false;
    }

    int16_t raw_x = 0, raw_y = 0;
    if (!xpt2046_sample_filtered(&raw_x, &raw_y)) {
        return false;
    }

    xpt2046_calibrate_raw(raw_x, raw_y, out_x, out_y);
    return true;
#else
    if (!s_mock_touched) {
        return false;
    }
    xpt2046_calibrate_raw(s_mock_raw_x, s_mock_raw_y, out_x, out_y);
    return true;
#endif
}

void xpt2046_set_calibration(int16_t x_min, int16_t x_max, int16_t y_min, int16_t y_max) {
    s_cal_x_min = x_min;
    s_cal_x_max = x_max;
    s_cal_y_min = y_min;
    s_cal_y_max = y_max;
}

void xpt2046_set_mock_touch(bool touched, int16_t raw_x, int16_t raw_y) {
#if !defined(ESP_PLATFORM)
    s_mock_touched = touched;
    s_mock_raw_x = raw_x;
    s_mock_raw_y = raw_y;
#else
    (void)touched;
    (void)raw_x;
    (void)raw_y;
#endif
}
