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
#ifndef ESP_ERR_NO_MEM
#define ESP_ERR_NO_MEM 0x101
#endif
#ifndef ESP_ERR_INVALID_ARG
#define ESP_ERR_INVALID_ARG 0x102
#endif
#ifndef ESP_ERR_INVALID_STATE
#define ESP_ERR_INVALID_STATE 0x103
#endif
#ifndef ESP_ERR_TIMEOUT
#define ESP_ERR_TIMEOUT 0x107
#endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Telemetry state received from the Squeezelite-ESP32 host.
 */
typedef struct {
    char title[128];
    char artist[128];
    char album[128];
    char state[16];   // "play", "pause", "stop"
    char mode[16];    // e.g. "LMS", "Spotify", "AirPlay"
    char ip[32];      // host IP address
    uint32_t elapsed;
    uint32_t duration;
    uint8_t vol;
    bool link_active;
} cyd_telemetry_state_t;

/**
 * @brief Event notification callback signature.
 */
typedef void (*cyd_event_cb_t)(const cyd_telemetry_state_t *state);

/**
 * @brief Parse an incoming NDJSON telemetry event line and update state.
 *
 * Supported events: "meta", "status", "sys".
 * Automatically sets state->link_active = true upon successful parsing.
 *
 * @param line Pointer to null-terminated NDJSON string
 * @param state Pointer to state structure to update
 * @return ESP_OK on success, ESP_ERR_INVALID_ARG on invalid/malformed JSON or unknown event.
 */
esp_err_t cyd_client_parse_event(const char *line, cyd_telemetry_state_t *state);

/**
 * @brief Format an outbound NDJSON command with trailing newline.
 *
 * Supported commands: "toggle", "play", "pause", "next", "prev", "sync",
 * "vol" (requires 0 <= val <= 100), "vol_step" (val is step delta).
 *
 * @param cmd_name Command name string
 * @param val Associated parameter value (used for "vol" and "vol_step")
 * @return Dynamically allocated string with trailing '\n', or NULL on failure.
 *         Caller must free() returned string.
 */
char *cyd_client_format_command(const char *cmd_name, int32_t val);

#ifdef __cplusplus
}
#endif
