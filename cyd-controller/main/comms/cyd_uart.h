#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include "cyd_protocol.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Hardware Pin Reference for CYD (ESP32-2432S028R) UART2 Link */
#define CYD_UART_NUM      UART_NUM_2
#define CYD_UART_TX_PIN   27
#define CYD_UART_RX_PIN   22
#define CYD_UART_BAUD     460800

/**
 * @brief Spy callback type for intercepting outbound UART transmission.
 */
typedef void (*cyd_tx_spy_cb_t)(const char *data, size_t len);

/**
 * @brief Initialize UART2 and start background receive task and heartbeat timer.
 *
 * Configures UART_NUM_2 on TX: GPIO 27, RX: GPIO 22 at 460800 8N1.
 *
 * @return ESP_OK on success, or an error code.
 */
esp_err_t cyd_client_uart_init(void);

/**
 * @brief Format and transmit a command via UART under mutex protection.
 *
 * @param cmd_name Command name string
 * @param val Associated parameter value
 * @return ESP_OK on success, or error code.
 */
esp_err_t cyd_client_send_cmd(const char *cmd_name, int32_t val);

/**
 * @brief Transmit raw string under mutex protection.
 *
 * @param json_line Null-terminated string to send
 * @return ESP_OK on success, or error code.
 */
esp_err_t cyd_client_send_raw(const char *json_line);

/**
 * @brief Register a callback function invoked when telemetry state changes.
 *
 * @param cb Callback function pointer
 */
void cyd_client_register_event_callback(cyd_event_cb_t cb);

/**
 * @brief Retrieve current cached telemetry state.
 *
 * @return Pointer to read-only telemetry state
 */
const cyd_telemetry_state_t *cyd_client_get_state(void);

/**
 * @brief Retrieve a thread-safe snapshot copy of current cached telemetry state.
 *
 * Copies current state into the provided destination buffer under s_state_mutex protection.
 *
 * @param out Pointer to output telemetry state structure
 */
void cyd_client_get_state_copy(cyd_telemetry_state_t *out);

/**
 * @brief Feed raw bytes into the line receiver accumulator.
 *
 * Strips '\r', buffers up to 1024 bytes, and parses/dispatches on '\n'.
 * Protects against buffer overrun by discarding corrupted lines.
 *
 * @param buf Pointer to byte buffer
 * @param len Number of bytes
 */
void cyd_client_feed_rx_bytes(const char *buf, size_t len);

/**
 * @brief Poll link heartbeat and handle timeout / auto-resync.
 *
 * If link was active and >5s elapsed since last packet, marks link_active = false
 * and sends sync command.
 * If link is inactive, retransmits sync command every 3s.
 *
 * @param current_time_us Current timestamp in microseconds
 */
void cyd_client_poll_heartbeat(int64_t current_time_us);

/**
 * @brief Register a test spy callback to intercept transmitted UART data.
 *
 * @param spy Callback function pointer
 */
void cyd_client_set_tx_spy(cyd_tx_spy_cb_t spy);

/**
 * @brief Reset telemetry state and receiver line buffer (for testing or re-init).
 */
void cyd_client_reset_state(void);

/**
 * @brief Set mock timestamp in microseconds for host unit testing.
 *
 * @param us Timestamp in microseconds
 */
void cyd_client_set_mock_time_us(int64_t us);

#ifdef __cplusplus
}
#endif
