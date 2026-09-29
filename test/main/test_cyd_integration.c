#include "unity.h"
#if defined(ESP_PLATFORM)
#include "unity_test_runner.h"
#endif

#include "cyd_link.h"
#include "cyd_link_dispatch.h"
#include "cyd_link_hooks.h"

#if defined(ESP_PLATFORM)
#include "audio_controls.h"
#else
typedef enum {
    ACTRLS_NONE = -1, ACTRLS_POWER, ACTRLS_VOLUP, ACTRLS_VOLDOWN, ACTRLS_TOGGLE, ACTRLS_PLAY,
    ACTRLS_PAUSE, ACTRLS_STOP, ACTRLS_REW, ACTRLS_FWD, ACTRLS_PREV, ACTRLS_NEXT,
    BCTRLS_UP, BCTRLS_DOWN, BCTRLS_LEFT, BCTRLS_RIGHT,
    BCTRLS_PS0,BCTRLS_PS1,BCTRLS_PS2,BCTRLS_PS3,BCTRLS_PS4,BCTRLS_PS5,BCTRLS_PS6,BCTRLS_PS7,BCTRLS_PS8,BCTRLS_PS9,
    KNOB_LEFT, KNOB_RIGHT, KNOB_PUSH,
    ACTRLS_SLEEP,
    ACTRLS_REMAP, ACTRLS_MAX
} actrls_action_e;
typedef void (*actrls_handler)(bool pressed);
actrls_handler get_ctrl_handler(actrls_action_e action);
#endif

#include "cyd_protocol.h"
#include "cyd_uart.h"
#include "ui_theme.h"
#include "ui_now_playing.h"

#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>

/* =========================================================================
 * Mock Audio Controls & Volume Handlers
 * ========================================================================= */
static int s_mock_volume = -1;
static actrls_action_e s_last_action = ACTRLS_NONE;
static int s_action_count = 0;

static void mock_action_handler(bool pressed, actrls_action_e act) {
    if (pressed) {
        s_last_action = act;
        s_action_count++;
    }
}

static void h_toggle(bool p)  { mock_action_handler(p, ACTRLS_TOGGLE); }
static void h_play(bool p)    { mock_action_handler(p, ACTRLS_PLAY); }
static void h_pause(bool p)   { mock_action_handler(p, ACTRLS_PAUSE); }
static void h_next(bool p)    { mock_action_handler(p, ACTRLS_NEXT); }
static void h_prev(bool p)    { mock_action_handler(p, ACTRLS_PREV); }
static void h_volup(bool p)   { mock_action_handler(p, ACTRLS_VOLUP); }
static void h_voldown(bool p) { mock_action_handler(p, ACTRLS_VOLDOWN); }

#if !defined(ESP_PLATFORM)
actrls_handler get_ctrl_handler(actrls_action_e action) {
    switch (action) {
        case ACTRLS_TOGGLE:  return h_toggle;
        case ACTRLS_PLAY:    return h_play;
        case ACTRLS_PAUSE:   return h_pause;
        case ACTRLS_NEXT:    return h_next;
        case ACTRLS_PREV:    return h_prev;
        case ACTRLS_VOLUP:   return h_volup;
        case ACTRLS_VOLDOWN: return h_voldown;
        default:             return NULL;
    }
}

void output_volume(uint8_t val) {
    s_mock_volume = (int)val;
}
#endif

/* =========================================================================
 * Simulated Bidirectional Wire Pipe
 * ========================================================================= */
#define WIRE_BUF_SIZE 2048

typedef struct {
    char buf[WIRE_BUF_SIZE];
    size_t len;
    bool direct_forward;
    int packet_count;
} wire_pipe_t;

static wire_pipe_t s_host_to_client_pipe;
static wire_pipe_t s_client_to_host_pipe;

static void pipe_init(void) {
    memset(&s_host_to_client_pipe, 0, sizeof(s_host_to_client_pipe));
    memset(&s_client_to_host_pipe, 0, sizeof(s_client_to_host_pipe));
    s_host_to_client_pipe.direct_forward = true;
    s_client_to_host_pipe.direct_forward = true;
}

static void on_host_tx(const char *data, size_t len) {
    if (s_host_to_client_pipe.len + len < WIRE_BUF_SIZE) {
        memcpy(s_host_to_client_pipe.buf + s_host_to_client_pipe.len, data, len);
        s_host_to_client_pipe.len += len;
        s_host_to_client_pipe.buf[s_host_to_client_pipe.len] = '\0';
    }
    s_host_to_client_pipe.packet_count++;
    if (s_host_to_client_pipe.direct_forward) {
        cyd_client_feed_rx_bytes(data, len);
    }
}

static void on_client_tx(const char *data, size_t len) {
    if (s_client_to_host_pipe.len + len < WIRE_BUF_SIZE) {
        memcpy(s_client_to_host_pipe.buf + s_client_to_host_pipe.len, data, len);
        s_client_to_host_pipe.len += len;
        s_client_to_host_pipe.buf[s_client_to_host_pipe.len] = '\0';
    }
    s_client_to_host_pipe.packet_count++;
    if (s_client_to_host_pipe.direct_forward) {
        cyd_link_feed_rx_bytes(data, len);
    }
}

static void pipe_flush_client_to_host(void) {
    if (s_client_to_host_pipe.len > 0) {
        cyd_link_feed_rx_bytes(s_client_to_host_pipe.buf, s_client_to_host_pipe.len);
        s_client_to_host_pipe.len = 0;
        s_client_to_host_pipe.buf[0] = '\0';
    }
}

/* =========================================================================
 * Test Setup and Teardown
 * ========================================================================= */
void setUp(void) {
    cyd_link_init();
    cyd_link_hooks_init();
    cyd_client_reset_state();
    pipe_init();
    cyd_link_set_tx_spy(on_host_tx);
    cyd_client_set_tx_spy(on_client_tx);
    s_mock_volume = -1;
    s_last_action = ACTRLS_NONE;
    s_action_count = 0;
}

void tearDown(void) {
    cyd_link_set_tx_spy(NULL);
    cyd_client_set_tx_spy(NULL);
    cyd_client_register_event_callback(NULL);
}

/* =========================================================================
 * Integration Test Cases
 * ========================================================================= */

/**
 * Scenario 1: Host-to-Client Telemetry Loopback
 * Host emits metadata and timer status -> wire forwards NDJSON -> client state matches.
 */
void test_integration_host_to_client_telemetry_loopback(void) {
    setUp();

    // 1. Host emits metadata and timer status
    cyd_link_hook_metadata("Eagles", "Hotel California", "Hotel California");
    cyd_link_hook_timer(125, 390);

    // 2. Verify wire traffic recorded in pipe
    TEST_ASSERT_TRUE(s_host_to_client_pipe.packet_count >= 2);
    TEST_ASSERT_NOT_NULL(strstr(s_host_to_client_pipe.buf, "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(s_host_to_client_pipe.buf, "\"event\":\"status\""));

    // 3. Verify CYD client telemetry state matches
    const cyd_telemetry_state_t *client_state = cyd_client_get_state();
    TEST_ASSERT_NOT_NULL(client_state);
    TEST_ASSERT_EQUAL_STRING("Hotel California", client_state->title);
    TEST_ASSERT_EQUAL_STRING("Eagles", client_state->artist);
    TEST_ASSERT_EQUAL_STRING("Hotel California", client_state->album);
    TEST_ASSERT_EQUAL_UINT32(125, client_state->elapsed);
    TEST_ASSERT_EQUAL_UINT32(390, client_state->duration);
    TEST_ASSERT_TRUE(client_state->link_active);

    // 4. Host updates playback state
    cyd_link_hook_playback_state("play");
    client_state = cyd_client_get_state();
    TEST_ASSERT_EQUAL_STRING("play", client_state->state);
    TEST_ASSERT_EQUAL_UINT32(125, client_state->elapsed);
    TEST_ASSERT_EQUAL_UINT32(390, client_state->duration);

    cyd_link_hook_playback_state("pause");
    client_state = cyd_client_get_state();
    TEST_ASSERT_EQUAL_STRING("pause", client_state->state);

    tearDown();
}

/**
 * Scenario 2: Client-to-Host Command Loopback
 * CYD client sends commands -> wire forwards NDJSON -> host audio controls and volume update.
 */
void test_integration_client_to_host_command_loopback(void) {
    setUp();

    // 1. Volume command loopback
    esp_err_t err = cyd_client_send_cmd("vol", 85);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(85, s_mock_volume);

    // Volume boundaries 0 and 100
    err = cyd_client_send_cmd("vol", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(0, s_mock_volume);

    err = cyd_client_send_cmd("vol", 100);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(100, s_mock_volume);

    // 2. Transport commands loopback
    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("play", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_PLAY, s_last_action);

    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("pause", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_PAUSE, s_last_action);

    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("next", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_NEXT, s_last_action);

    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("prev", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_PREV, s_last_action);

    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("toggle", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_TOGGLE, s_last_action);

    // 3. Volume step loopback
    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("vol_step", 1);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_VOLUP, s_last_action);

    s_last_action = ACTRLS_NONE;
    err = cyd_client_send_cmd("vol_step", -1);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(ACTRLS_VOLDOWN, s_last_action);

    tearDown();
}

/**
 * Scenario 3: Full Bidirectional Sync Flow
 * Client sends sync -> host receives -> host transmits full burst (sys, meta, status) -> client updates all fields.
 */
void test_integration_full_bidirectional_sync_flow(void) {
    setUp();

    // 1. Host caches initial metadata, playback state, timer, and volume
    cyd_link_set_cached_meta("Stairway to Heaven", "Led Zeppelin", "Led Zeppelin IV");
    cyd_link_hook_playback_state("play");
    cyd_link_hook_timer(240, 482);
    cyd_client_send_cmd("vol", 75);

    // 2. Clear pipe log and reset client state to uninitialized
    pipe_init();
    cyd_client_reset_state();
    const cyd_telemetry_state_t *client_state = cyd_client_get_state();
    TEST_ASSERT_FALSE(client_state->link_active);
    TEST_ASSERT_EQUAL_STRING("", client_state->title);

    // 3. Client initiates sync request
    esp_err_t err = cyd_client_send_cmd("sync", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);

    // 4. Verify host transmitted full burst (sys, meta, status)
    TEST_ASSERT_TRUE(s_host_to_client_pipe.packet_count >= 3);
    TEST_ASSERT_NOT_NULL(strstr(s_host_to_client_pipe.buf, "\"event\":\"sys\""));
    TEST_ASSERT_NOT_NULL(strstr(s_host_to_client_pipe.buf, "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(s_host_to_client_pipe.buf, "\"event\":\"status\""));

    // 5. Verify client telemetry state matches host across all fields
    client_state = cyd_client_get_state();
    TEST_ASSERT_TRUE(client_state->link_active);
    TEST_ASSERT_EQUAL_STRING("LMS", client_state->mode);
    TEST_ASSERT_EQUAL_STRING("0.0.0.0", client_state->ip);
    TEST_ASSERT_EQUAL_STRING("Stairway to Heaven", client_state->title);
    TEST_ASSERT_EQUAL_STRING("Led Zeppelin", client_state->artist);
    TEST_ASSERT_EQUAL_STRING("Led Zeppelin IV", client_state->album);
    TEST_ASSERT_EQUAL_STRING("play", client_state->state);
    TEST_ASSERT_EQUAL_UINT32(240, client_state->elapsed);
    TEST_ASSERT_EQUAL_UINT32(482, client_state->duration);
    TEST_ASSERT_EQUAL_UINT8(75, client_state->vol);

    tearDown();
}

/**
 * Scenario 4: Heartbeat Timeout and Auto-Recovery
 * Client detects timeout (>5s) -> link_active == false -> sends sync -> host responds -> link_active recovers.
 */
void test_integration_heartbeat_timeout_and_auto_recovery(void) {
    setUp();

    // 1. Establish active link at t = 1.0s
    cyd_client_set_mock_time_us(1000000);
    cyd_link_set_cached_meta("Aja", "Steely Dan", "Aja");
    cyd_link_hook_playback_state("play");
    cyd_link_hook_timer(50, 477);

    const cyd_telemetry_state_t *client_state = cyd_client_get_state();
    TEST_ASSERT_TRUE(client_state->link_active);

    // 2. Buffer client-to-host transmissions to observe timeout state before response
    s_client_to_host_pipe.direct_forward = false;
    s_client_to_host_pipe.len = 0;
    s_client_to_host_pipe.buf[0] = '\0';
    s_client_to_host_pipe.packet_count = 0;

    // 3. Advance client clock by 6.0s (t = 7.0s) without packets (>5s timeout)
    cyd_client_set_mock_time_us(7000000);
    cyd_client_poll_heartbeat(7000000);

    // 4. Verify client transitioned to offline and emitted sync command
    client_state = cyd_client_get_state();
    TEST_ASSERT_FALSE(client_state->link_active);
    TEST_ASSERT_EQUAL(1, s_client_to_host_pipe.packet_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"sync\"}\n", s_client_to_host_pipe.buf);

    // 5. Restore direct forwarding and deliver buffered sync command to host
    s_client_to_host_pipe.direct_forward = true;
    pipe_flush_client_to_host();

    // 6. Host processed sync and responded with full sync burst over the wire
    client_state = cyd_client_get_state();
    TEST_ASSERT_TRUE(client_state->link_active);
    TEST_ASSERT_EQUAL_STRING("Aja", client_state->title);
    TEST_ASSERT_EQUAL_STRING("Steely Dan", client_state->artist);
    TEST_ASSERT_EQUAL_STRING("Aja", client_state->album);
    TEST_ASSERT_EQUAL_UINT32(50, client_state->elapsed);
    TEST_ASSERT_EQUAL_UINT32(477, client_state->duration);

    tearDown();
}

/**
 * Scenario 5: Fragmented Wire Loopback
 * Verify packet accumulation and reconstruction across byte fragments over simulated wire.
 */
void test_integration_fragmented_wire_loopback(void) {
    setUp();

    const char *line = "{\"event\":\"meta\",\"title\":\"Fragmented\",\"artist\":\"Test Artist\",\"album\":\"Test Album\"}\n";
    size_t len = strlen(line);

    for (size_t i = 0; i < len; i++) {
        cyd_client_feed_rx_bytes(&line[i], 1);
    }

    const cyd_telemetry_state_t *state = cyd_client_get_state();
    TEST_ASSERT_TRUE(state->link_active);
    TEST_ASSERT_EQUAL_STRING("Fragmented", state->title);
    TEST_ASSERT_EQUAL_STRING("Test Artist", state->artist);
    TEST_ASSERT_EQUAL_STRING("Test Album", state->album);

    tearDown();
}

/**
 * Scenario 6: End-to-End LVGL UI Integration
 * Host telemetry -> wire -> client UART -> UI now playing display -> UI touch action -> wire -> host action.
 */
void test_integration_ui_now_playing_loopback(void) {
    setUp();

    // 1. Initialize UI theme and screen
    ui_theme_init();
    ui_now_playing_create();

    // 2. Subscribe UI updater to client UART telemetry
    cyd_client_register_event_callback(ui_now_playing_update);

    // 3. Host emits metadata and status
    cyd_link_hook_metadata("Pink Floyd", "Dark Side", "Time");
    cyd_link_hook_playback_state("play");
    cyd_link_hook_timer(180, 420);

    // 4. Verify client state was received and passed to UI
    const cyd_telemetry_state_t *state = cyd_client_get_state();
    TEST_ASSERT_TRUE(state->link_active);
    TEST_ASSERT_EQUAL_STRING("Time", state->title);
    TEST_ASSERT_EQUAL_STRING("Pink Floyd", state->artist);
    TEST_ASSERT_EQUAL_STRING("Dark Side", state->album);
    TEST_ASSERT_EQUAL_STRING("play", state->state);

    // 5. Client UI triggers transport command
    s_last_action = ACTRLS_NONE;
    cyd_client_send_cmd("pause", 0);
    TEST_ASSERT_EQUAL(ACTRLS_PAUSE, s_last_action);

    tearDown();
}

/* =========================================================================
 * Test Runners
 * ========================================================================= */
#if defined(ESP_PLATFORM)
TEST_CASE("CYD Integration Host-to-Client Telemetry Loopback", "[cyd_integration]") {
    test_integration_host_to_client_telemetry_loopback();
}
TEST_CASE("CYD Integration Client-to-Host Command Loopback", "[cyd_integration]") {
    test_integration_client_to_host_command_loopback();
}
TEST_CASE("CYD Integration Full Bidirectional Sync Flow", "[cyd_integration]") {
    test_integration_full_bidirectional_sync_flow();
}
TEST_CASE("CYD Integration Heartbeat Timeout and Auto-Recovery", "[cyd_integration]") {
    test_integration_heartbeat_timeout_and_auto_recovery();
}
TEST_CASE("CYD Integration Fragmented Wire Loopback", "[cyd_integration]") {
    test_integration_fragmented_wire_loopback();
}
TEST_CASE("CYD Integration UI Now Playing Loopback", "[cyd_integration]") {
    test_integration_ui_now_playing_loopback();
}
#else
int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_integration_host_to_client_telemetry_loopback);
    RUN_TEST(test_integration_client_to_host_command_loopback);
    RUN_TEST(test_integration_full_bidirectional_sync_flow);
    RUN_TEST(test_integration_heartbeat_timeout_and_auto_recovery);
    RUN_TEST(test_integration_fragmented_wire_loopback);
    RUN_TEST(test_integration_ui_now_playing_loopback);
    return UNITY_END();
}
#endif
