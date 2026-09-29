#include "unity.h"
#if defined(ESP_PLATFORM)
#include "unity_test_runner.h"
#endif

#include "ui_theme.h"
#include "ui_now_playing.h"
#include "cyd_protocol.h"
#include "cyd_uart.h"
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>

#if !defined(ESP_PLATFORM)
/* Tracking for outbound spy commands */
static char s_last_sent_cmd[64];
static int32_t s_last_sent_val = -1;
static int s_cmd_send_count = 0;

static void test_tx_spy(const char *data, size_t len) {
    (void)len;
    // Intercept outbound command strings
    if (strstr(data, "\"cmd\":\"prev\"")) {
        strncpy(s_last_sent_cmd, "prev", sizeof(s_last_sent_cmd) - 1);
        s_cmd_send_count++;
    } else if (strstr(data, "\"cmd\":\"next\"")) {
        strncpy(s_last_sent_cmd, "next", sizeof(s_last_sent_cmd) - 1);
        s_cmd_send_count++;
    } else if (strstr(data, "\"cmd\":\"toggle\"")) {
        strncpy(s_last_sent_cmd, "toggle", sizeof(s_last_sent_cmd) - 1);
        s_cmd_send_count++;
    } else if (strstr(data, "\"cmd\":\"vol\"")) {
        strncpy(s_last_sent_cmd, "vol", sizeof(s_last_sent_cmd) - 1);
        s_cmd_send_count++;
    }
}

void setUp(void) {
    s_last_sent_cmd[0] = '\0';
    s_last_sent_val = -1;
    s_cmd_send_count = 0;
    lv_init();
    cyd_client_reset_state();
    cyd_client_set_tx_spy(test_tx_spy);
}

void tearDown(void) {
}
#endif

/* =========================================================================
 * 1. Time Formatting Tests
 * ========================================================================= */

void test_ui_format_time_zero(void) {
    char buf[32];
    ui_now_playing_format_time(0, buf, sizeof(buf));
    TEST_ASSERT_EQUAL_STRING("00:00", buf);
}

void test_ui_format_time_under_one_hour(void) {
    char buf[32];
    ui_now_playing_format_time(65, buf, sizeof(buf));
    TEST_ASSERT_EQUAL_STRING("01:05", buf);

    ui_now_playing_format_time(59, buf, sizeof(buf));
    TEST_ASSERT_EQUAL_STRING("00:59", buf);

    ui_now_playing_format_time(599, buf, sizeof(buf));
    TEST_ASSERT_EQUAL_STRING("09:59", buf);

    ui_now_playing_format_time(3599, buf, sizeof(buf));
    TEST_ASSERT_EQUAL_STRING("59:59", buf);
}

void test_ui_format_time_one_hour_and_above(void) {
    char buf[32];
    ui_now_playing_format_time(3600, buf, sizeof(buf));
    // Accept either 01:00:00 or 60:00 depending on hours representation
    bool match_3600 = (strcmp(buf, "01:00:00") == 0 || strcmp(buf, "60:00") == 0);
    TEST_ASSERT_TRUE(match_3600);

    ui_now_playing_format_time(3661, buf, sizeof(buf));
    bool match_3661 = (strcmp(buf, "01:01:01") == 0 || strcmp(buf, "61:01") == 0);
    TEST_ASSERT_TRUE(match_3661);

    ui_now_playing_format_time(7322, buf, sizeof(buf));
    bool match_7322 = (strcmp(buf, "02:02:02") == 0 || strcmp(buf, "122:02") == 0);
    TEST_ASSERT_TRUE(match_7322);
}

void test_ui_format_time_null_and_boundary(void) {
    // Should not crash on NULL or 0 length
    ui_now_playing_format_time(120, NULL, 0);

    char small_buf[4];
    ui_now_playing_format_time(65, small_buf, sizeof(small_buf));
    TEST_ASSERT_EQUAL_INT('\0', small_buf[sizeof(small_buf) - 1]);
}

/* =========================================================================
 * 2. Volume Throttling Tests (10 Hz rate limiter = 100ms)
 * ========================================================================= */

void test_ui_volume_throttling_within_window(void) {
    // Within 100ms should throttle (returns true)
    TEST_ASSERT_TRUE(ui_now_playing_should_throttle_volume(50, 0));
    TEST_ASSERT_TRUE(ui_now_playing_should_throttle_volume(99, 0));
    TEST_ASSERT_TRUE(ui_now_playing_should_throttle_volume(1050, 1000));
    TEST_ASSERT_TRUE(ui_now_playing_should_throttle_volume(1099, 1000));
}

void test_ui_volume_throttling_after_window(void) {
    // At or after 100ms should not throttle (returns false)
    TEST_ASSERT_FALSE(ui_now_playing_should_throttle_volume(100, 0));
    TEST_ASSERT_FALSE(ui_now_playing_should_throttle_volume(150, 0));
    TEST_ASSERT_FALSE(ui_now_playing_should_throttle_volume(1100, 1000));
    TEST_ASSERT_FALSE(ui_now_playing_should_throttle_volume(1250, 1000));
}

void test_ui_volume_throttling_timer_wraparound(void) {
    // 32-bit unsigned wrap around: 0xFFFFFF9C to 0x00000010 is 0x74 = 116 ms
    uint32_t last_sent = 0xFFFFFF9C;
    uint32_t now = 0x00000010;
    TEST_ASSERT_FALSE(ui_now_playing_should_throttle_volume(now, last_sent));

    // Wrap around but within 50ms: 0xFFFFFFFE to 0x00000020 is 0x22 = 34 ms
    last_sent = 0xFFFFFFFE;
    now = 0x00000020;
    TEST_ASSERT_TRUE(ui_now_playing_should_throttle_volume(now, last_sent));
}

/* =========================================================================
 * 3. UI State Update and Widget Tests
 * ========================================================================= */

void test_ui_state_update_null_safe(void) {
    ui_theme_init();
    ui_now_playing_create();

    // Passing NULL should not crash
    ui_now_playing_update(NULL);
    TEST_ASSERT_TRUE(true);
}

void test_ui_state_update_active_playback(void) {
    ui_theme_init();
    ui_now_playing_create();

    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));
    strncpy(state.title, "Bohemian Rhapsody", sizeof(state.title) - 1);
    strncpy(state.artist, "Queen", sizeof(state.artist) - 1);
    strncpy(state.album, "A Night at the Opera", sizeof(state.album) - 1);
    strncpy(state.state, "play", sizeof(state.state) - 1);
    strncpy(state.mode, "SPOTIFY", sizeof(state.mode) - 1);
    strncpy(state.ip, "192.168.1.42", sizeof(state.ip) - 1);
    state.elapsed = 120;
    state.duration = 360;
    state.vol = 65;
    state.link_active = true;

    ui_now_playing_update(&state);

    const cyd_telemetry_state_t *cached = ui_now_playing_get_cached_state();
    TEST_ASSERT_NOT_NULL(cached);
    TEST_ASSERT_EQUAL_STRING("Bohemian Rhapsody", cached->title);
    TEST_ASSERT_EQUAL_STRING("Queen", cached->artist);
    TEST_ASSERT_EQUAL_STRING("A Night at the Opera", cached->album);
    TEST_ASSERT_EQUAL_STRING("play", cached->state);
    TEST_ASSERT_EQUAL_STRING("SPOTIFY", cached->mode);
    TEST_ASSERT_EQUAL_STRING("192.168.1.42", cached->ip);
    TEST_ASSERT_EQUAL_UINT32(120, cached->elapsed);
    TEST_ASSERT_EQUAL_UINT32(360, cached->duration);
    TEST_ASSERT_EQUAL_UINT8(65, cached->vol);
    TEST_ASSERT_TRUE(cached->link_active);

    // Progress percentage: (120 * 100) / 360 = 33%
    TEST_ASSERT_EQUAL_INT(33, ui_now_playing_get_progress_percent());
    // Active playback should display pause symbol
    TEST_ASSERT_EQUAL_STRING(LV_SYMBOL_PAUSE, ui_now_playing_get_play_pause_icon());
    // Online indicator active
    TEST_ASSERT_FALSE(ui_now_playing_is_offline());
}

void test_ui_state_update_paused_and_offline(void) {
    ui_theme_init();
    ui_now_playing_create();

    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));
    strncpy(state.title, "Track 1", sizeof(state.title) - 1);
    strncpy(state.artist, "Artist 1", sizeof(state.artist) - 1);
    strncpy(state.state, "pause", sizeof(state.state) - 1);
    strncpy(state.mode, "LMS", sizeof(state.mode) - 1);
    strncpy(state.ip, "192.168.1.50", sizeof(state.ip) - 1);
    state.elapsed = 0;
    state.duration = 200;
    state.vol = 50;
    state.link_active = false;

    ui_now_playing_update(&state);

    const cyd_telemetry_state_t *cached = ui_now_playing_get_cached_state();
    TEST_ASSERT_NOT_NULL(cached);
    TEST_ASSERT_FALSE(cached->link_active);
    TEST_ASSERT_TRUE(ui_now_playing_is_offline());
    // Paused playback should display play symbol
    TEST_ASSERT_EQUAL_STRING(LV_SYMBOL_PLAY, ui_now_playing_get_play_pause_icon());
    // Progress percentage: 0%
    TEST_ASSERT_EQUAL_INT(0, ui_now_playing_get_progress_percent());
}

void test_ui_transport_button_actions(void) {
    ui_theme_init();
    ui_now_playing_create();

    // Simulate clicking prev
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_btn_click(UI_BTN_PREV);
    TEST_ASSERT_EQUAL_STRING("prev", s_last_sent_cmd);

    // Simulate clicking toggle
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_btn_click(UI_BTN_PLAY_PAUSE);
    TEST_ASSERT_EQUAL_STRING("toggle", s_last_sent_cmd);

    // Simulate clicking next
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_btn_click(UI_BTN_NEXT);
    TEST_ASSERT_EQUAL_STRING("next", s_last_sent_cmd);
}

void test_ui_volume_slider_throttled_dispatch(void) {
    ui_theme_init();
    ui_now_playing_create();

    // First change at t=10ms should send
    s_cmd_send_count = 0;
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_slider_change(40, 10);
    TEST_ASSERT_EQUAL_INT(1, s_cmd_send_count);
    TEST_ASSERT_EQUAL_STRING("vol", s_last_sent_cmd);

    // Rapid second change at t=50ms (within 100ms) should be throttled
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_slider_change(45, 50);
    TEST_ASSERT_EQUAL_INT(1, s_cmd_send_count); // Still 1, not incremented

    // Third change at t=120ms (>= 100ms from t=10ms) should send
    s_last_sent_cmd[0] = '\0';
    ui_now_playing_simulate_slider_change(50, 120);
    TEST_ASSERT_EQUAL_INT(2, s_cmd_send_count);
    TEST_ASSERT_EQUAL_STRING("vol", s_last_sent_cmd);
}

#if defined(ESP_PLATFORM)
TEST_CASE("CYD UI Time Formatting Zero", "[cyd_ui]") {
    test_ui_format_time_zero();
}
TEST_CASE("CYD UI Time Formatting Under One Hour", "[cyd_ui]") {
    test_ui_format_time_under_one_hour();
}
TEST_CASE("CYD UI Time Formatting Over One Hour", "[cyd_ui]") {
    test_ui_format_time_one_hour_and_above();
}
TEST_CASE("CYD UI Time Formatting Null and Boundary", "[cyd_ui]") {
    test_ui_format_time_null_and_boundary();
}
TEST_CASE("CYD UI Volume Throttling Within Window", "[cyd_ui]") {
    test_ui_volume_throttling_within_window();
}
TEST_CASE("CYD UI Volume Throttling After Window", "[cyd_ui]") {
    test_ui_volume_throttling_after_window();
}
TEST_CASE("CYD UI Volume Throttling Timer Wraparound", "[cyd_ui]") {
    test_ui_volume_throttling_timer_wraparound();
}
TEST_CASE("CYD UI State Update Null Safe", "[cyd_ui]") {
    test_ui_state_update_null_safe();
}
TEST_CASE("CYD UI State Update Active Playback", "[cyd_ui]") {
    test_ui_state_update_active_playback();
}
TEST_CASE("CYD UI State Update Paused and Offline", "[cyd_ui]") {
    test_ui_state_update_paused_and_offline();
}
TEST_CASE("CYD UI Transport Button Actions", "[cyd_ui]") {
    test_ui_transport_button_actions();
}
TEST_CASE("CYD UI Volume Slider Throttled Dispatch", "[cyd_ui]") {
    test_ui_volume_slider_throttled_dispatch();
}
#else
int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_ui_format_time_zero);
    RUN_TEST(test_ui_format_time_under_one_hour);
    RUN_TEST(test_ui_format_time_one_hour_and_above);
    RUN_TEST(test_ui_format_time_null_and_boundary);
    RUN_TEST(test_ui_volume_throttling_within_window);
    RUN_TEST(test_ui_volume_throttling_after_window);
    RUN_TEST(test_ui_volume_throttling_timer_wraparound);
    RUN_TEST(test_ui_state_update_null_safe);
    RUN_TEST(test_ui_state_update_active_playback);
    RUN_TEST(test_ui_state_update_paused_and_offline);
    RUN_TEST(test_ui_transport_button_actions);
    RUN_TEST(test_ui_volume_slider_throttled_dispatch);
    return UNITY_END();
}
#endif
