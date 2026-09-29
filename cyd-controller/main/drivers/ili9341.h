#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#if defined(ESP_PLATFORM)
#include "esp_err.h"
#else
typedef int esp_err_t;
#ifndef ESP_OK
#define ESP_OK 0
#endif
#ifndef ESP_FAIL
#define ESP_FAIL -1
#endif
#ifndef ESP_ERR_INVALID_ARG
#define ESP_ERR_INVALID_ARG 0x102
#endif
#ifndef ESP_ERR_INVALID_STATE
#define ESP_ERR_INVALID_STATE 0x103
#endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Hardware Pin Reference for CYD (ESP32-2432S028R) TFT SPI */
#define CYD_TFT_MISO 12
#define CYD_TFT_MOSI 13
#define CYD_TFT_SCLK 14
#define CYD_TFT_CS   15
#define CYD_TFT_DC   2
#define CYD_TFT_RST  -1
#define CYD_TFT_BL   21

/* TFT Resolution (Landscape) */
#define CYD_TFT_WIDTH  320
#define CYD_TFT_HEIGHT 240

/**
 * @brief Initialize the ILI9341 display controller and backlight.
 *
 * Configures SPI2_HOST at 40MHz, configures DC/CS GPIOs, sets up
 * backlight PWM on GPIO 21, and sends the standard ILI9341 initialization sequence.
 *
 * @return ESP_OK on success, or an error code.
 */
esp_err_t ili9341_init(void);

/**
 * @brief Flush a buffer of 16-bit RGB565 color pixels to the display window.
 *
 * Automatically clamps (x1, y1) and (x2, y2) to the screen dimensions:
 * [0, CYD_TFT_WIDTH - 1] and [0, CYD_TFT_HEIGHT - 1].
 *
 * @param x1 Left column (0-indexed)
 * @param y1 Top row (0-indexed)
 * @param x2 Right column (inclusive)
 * @param y2 Bottom row (inclusive)
 * @param color_data Pointer to RGB565 pixel data
 */
void ili9341_flush(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const uint16_t *color_data);

/**
 * @brief Set the backlight brightness percentage (0-100%).
 *
 * @param percent Brightness level from 0 (off) to 100 (maximum brightness).
 */
void ili9341_set_backlight(uint8_t percent);

/**
 * @brief Test inspection helper: Retrieve the window coordinates of the last flush call.
 *
 * @param[out] out_x1 Clamped x1 of last flush
 * @param[out] out_y1 Clamped y1 of last flush
 * @param[out] out_x2 Clamped x2 of last flush
 * @param[out] out_y2 Clamped y2 of last flush
 */
void ili9341_get_last_flush_window(int32_t *out_x1, int32_t *out_y1, int32_t *out_x2, int32_t *out_y2);

/**
 * @brief Test inspection helper: Retrieve total count of valid flushes.
 */
uint32_t ili9341_get_flush_count(void);

/**
 * @brief Test inspection helper: Retrieve last configured backlight percentage.
 */
uint8_t ili9341_get_backlight(void);

#ifdef __cplusplus
}
#endif
