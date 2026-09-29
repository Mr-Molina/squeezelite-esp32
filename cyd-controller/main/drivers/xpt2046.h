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

/* Hardware Pin Reference for CYD (ESP32-2432S028R) Touch SPI */
#define CYD_TOUCH_MISO 39
#define CYD_TOUCH_MOSI 32
#define CYD_TOUCH_SCLK 25
#define CYD_TOUCH_CS   33
#define CYD_TOUCH_IRQ  36

/* Calibration Default Constants for 2.8" CYD Panel */
#define XPT2046_RAW_X_MIN 300
#define XPT2046_RAW_X_MAX 3800
#define XPT2046_RAW_Y_MIN 300
#define XPT2046_RAW_Y_MAX 3800

/**
 * @brief Initialize the XPT2046 touch controller.
 *
 * Configures SPI3_HOST (or VSPI) at 2MHz, sets up CS GPIO 33
 * and IRQ GPIO 36.
 *
 * @return ESP_OK on success, or an error code.
 */
esp_err_t xpt2046_init(void);

/**
 * @brief Read touch coordinates if touched (IRQ low).
 *
 * Takes multiple samples, applies noise rejection filtering,
 * maps to 320x240 landscape coordinates, and clamps to screen bounds.
 *
 * @param[out] out_x Clamped X coordinate (0 to 319)
 * @param[out] out_y Clamped Y coordinate (0 to 239)
 * @return true if screen is touched and coordinates are valid, false otherwise.
 */
bool xpt2046_read(int16_t *out_x, int16_t *out_y);

/**
 * @brief Calibrate and clamp raw ADC touch values to screen coordinates (320x240 landscape).
 *
 * Maps [XPT2046_RAW_X_MIN, XPT2046_RAW_X_MAX] to [0, 319],
 * and [XPT2046_RAW_Y_MIN, XPT2046_RAW_Y_MAX] to [0, 239].
 * Clamps coordinates to [0, 319] and [0, 239].
 *
 * @param raw_x Raw ADC value along X
 * @param raw_y Raw ADC value along Y
 * @param[out] out_x Calibrated and clamped X coordinate (0 to 319)
 * @param[out] out_y Calibrated and clamped Y coordinate (0 to 239)
 */
void xpt2046_calibrate_raw(int16_t raw_x, int16_t raw_y, int16_t *out_x, int16_t *out_y);

/**
 * @brief Configure custom calibration parameters at runtime.
 *
 * @param x_min Minimum raw ADC value for X
 * @param x_max Maximum raw ADC value for X
 * @param y_min Minimum raw ADC value for Y
 * @param y_max Maximum raw ADC value for Y
 */
void xpt2046_set_calibration(int16_t x_min, int16_t x_max, int16_t y_min, int16_t y_max);

/**
 * @brief Test helper to simulate touch inputs in mock/test environments.
 *
 * @param touched Whether the simulated screen is currently pressed
 * @param raw_x Simulated raw X ADC value
 * @param raw_y Simulated raw Y ADC value
 */
void xpt2046_set_mock_touch(bool touched, int16_t raw_x, int16_t raw_y);

#ifdef __cplusplus
}
#endif
