#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include "cyd_protocol.h"
#include "lvgl.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    UI_BTN_PREV = 0,
    UI_BTN_PLAY_PAUSE,
    UI_BTN_NEXT
} ui_btn_id_t;

/**
 * @brief Acquire LVGL UI synchronization mutex.
 */
void cyd_ui_lock(void);

/**
 * @brief Release LVGL UI synchronization mutex.
 */
void cyd_ui_unlock(void);

/**
 * @brief Build and display the 320x240 landscape Now Playing UI.
 */
void ui_now_playing_create(void);

/**
 * @brief Thread-safe telemetry update handler.
 *
 * Updates track title, artist/album, progress bar, elapsed/total times,
 * play/pause dynamic icon, volume slider, mode badge, IP address, and link status.
 *
 * @param state Telemetry state received from host (safe if NULL).
 */
void ui_now_playing_update(const cyd_telemetry_state_t *state);

/**
 * @brief Format seconds into "MM:SS" (or "HH:MM:SS" if >= 3600 seconds).
 *
 * @param seconds Duration in seconds
 * @param buf Output string buffer
 * @param buf_len Size of output buffer
 */
void ui_now_playing_format_time(uint32_t seconds, char *buf, size_t buf_len);

/**
 * @brief Validate whether volume change events should be throttled (10 Hz rate limit = 100ms).
 *
 * @param now_ms Current timestamp in milliseconds
 * @param last_sent_ms Timestamp of last sent volume command in milliseconds
 * @return true if event should be throttled (dropped), false if allowed to send
 */
bool ui_now_playing_should_throttle_volume(uint32_t now_ms, uint32_t last_sent_ms);

/**
 * @brief Retrieve internal cached telemetry state.
 */
const cyd_telemetry_state_t *ui_now_playing_get_cached_state(void);

/**
 * @brief Retrieve current progress percentage (0..100).
 */
int ui_now_playing_get_progress_percent(void);

/**
 * @brief Retrieve string symbol displayed on Play/Pause button.
 */
const char *ui_now_playing_get_play_pause_icon(void);

/**
 * @brief Retrieve whether link is currently offline / disconnected.
 */
bool ui_now_playing_is_offline(void);

/**
 * @brief Simulate a transport button press event (for test verification).
 *
 * @param btn_id Button ID (UI_BTN_PREV, UI_BTN_PLAY_PAUSE, UI_BTN_NEXT)
 */
void ui_now_playing_simulate_btn_click(ui_btn_id_t btn_id);

/**
 * @brief Simulate a volume slider drag event (for test verification).
 *
 * @param vol New volume level (0..100)
 * @param now_ms Simulation timestamp in milliseconds
 */
void ui_now_playing_simulate_slider_change(int32_t vol, uint32_t now_ms);

#ifdef __cplusplus
}
#endif
