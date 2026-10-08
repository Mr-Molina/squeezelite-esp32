#include "unity.h"
#if defined(ESP_PLATFORM)
#include "unity_test_runner.h"
#endif

#include "cyd_protocol.h"
#include "cyd_uart.h"
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>

#if !defined(ESP_PLATFORM)
void setUp(void) {
    cyd_client_reset_state();
    cyd_client_set_tx_spy(NULL);
    cyd_client_register_event_callback(NULL);
}
void tearDown(void) {}
#endif

/* Tracking variables for test callbacks */
static int s_callback_count = 0;
static cyd_telemetry_state_t s_last_callback_state;

static void on_test_event(const cyd_telemetry_state_t *state) {
    s_callback_count++;
    if (state) {
        s_last_callback_state = *state;
    }
}

/* Tracking variables for TX spy */
static char s_last_tx_buf[512];
static size_t s_last_tx_len = 0;
static int s_tx_call_count = 0;

static void on_test_tx(const char *data, size_t len) {
    s_tx_call_count++;
    s_last_tx_len = len;
    if (len < sizeof(s_last_tx_buf)) {
        memcpy(s_last_tx_buf, data, len);
        s_last_tx_buf[len] = '\0';
    }
}

/* =========================================================================
 * 1. Protocol Event Parsing Tests
 * ========================================================================= */

void test_protocol_parse_meta_event(void) {
    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));

    const char *line = "{\"event\":\"meta\",\"title\":\"Comfortably Numb\",\"artist\":\"Pink Floyd\",\"album\":\"The Wall\"}\n";
    esp_err_t err = cyd_client_parse_event(line, &state);

    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_STRING("Comfortably Numb", state.title);
    TEST_ASSERT_EQUAL_STRING("Pink Floyd", state.artist);
    TEST_ASSERT_EQUAL_STRING("The Wall", state.album);
    TEST_ASSERT_TRUE(state.link_active);
}

void test_protocol_parse_meta_partial_and_long_strings(void) {
    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));

    // Only title provided
    const char *line1 = "{\"event\":\"meta\",\"title\":\"Solo Song\"}";
    esp_err_t err = cyd_client_parse_event(line1, &state);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_STRING("Solo Song", state.title);
    TEST_ASSERT_EQUAL_STRING("", state.artist);
    TEST_ASSERT_EQUAL_STRING("", state.album);

    // Excessively long string should be safely truncated with null termination
    char long_title[200];
    memset(long_title, 'A', sizeof(long_title) - 1);
    long_title[sizeof(long_title) - 1] = '\0';

    char json_buf[300];
    snprintf(json_buf, sizeof(json_buf), "{\"event\":\"meta\",\"title\":\"%s\"}", long_title);

    err = cyd_client_parse_event(json_buf, &state);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_INT(sizeof(state.title) - 1, strlen(state.title));
    TEST_ASSERT_EQUAL('A', state.title[0]);
    TEST_ASSERT_EQUAL('\0', state.title[sizeof(state.title) - 1]);
}

void test_protocol_parse_status_event(void) {
    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));

    const char *line = "{\"event\":\"status\",\"state\":\"play\",\"elapsed\":125,\"duration\":360,\"vol\":85}\n";
    esp_err_t err = cyd_client_parse_event(line, &state);

    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_STRING("play", state.state);
    TEST_ASSERT_EQUAL_UINT32(125, state.elapsed);
    TEST_ASSERT_EQUAL_UINT32(360, state.duration);
    TEST_ASSERT_EQUAL_UINT8(85, state.vol);
    TEST_ASSERT_TRUE(state.link_active);

    // Negative elapsed and duration should clamp to 0 rather than wrap around to 2^32-1
    const char *line_neg = "{\"event\":\"status\",\"state\":\"play\",\"elapsed\":-1,\"duration\":-10,\"vol\":-5}\n";
    err = cyd_client_parse_event(line_neg, &state);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_UINT32(0, state.elapsed);
    TEST_ASSERT_EQUAL_UINT32(0, state.duration);
    TEST_ASSERT_EQUAL_UINT8(0, state.vol);
}

void test_protocol_parse_sys_event(void) {
    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));

    const char *line = "{\"event\":\"sys\",\"mode\":\"Spotify\",\"name\":\"Living Room\",\"ip\":\"192.168.1.150\"}\n";
    esp_err_t err = cyd_client_parse_event(line, &state);

    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_STRING("Spotify", state.mode);
    TEST_ASSERT_EQUAL_STRING("192.168.1.150", state.ip);
    TEST_ASSERT_TRUE(state.link_active);
}

void test_protocol_parse_invalid_or_malformed(void) {
    cyd_telemetry_state_t state;
    memset(&state, 0, sizeof(state));

    // NULL pointers
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event(NULL, &state));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("{\"event\":\"meta\"}", NULL));

    // Empty or non-JSON string
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("", &state));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("hello world", &state));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("{corrupted json", &state));

    // Missing event field
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("{\"title\":\"No Event\"}", &state));

    // Unknown event type
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_parse_event("{\"event\":\"unknown_xyz\"}", &state));
}

/* =========================================================================
 * 2. Command Formatting Tests
 * ========================================================================= */

void test_protocol_format_commands(void) {
    // Basic transport commands
    const char *basic_cmds[] = {"toggle", "play", "pause", "next", "prev", "sync"};
    for (size_t i = 0; i < sizeof(basic_cmds) / sizeof(basic_cmds[0]); i++) {
        char *formatted = cyd_client_format_command(basic_cmds[i], 0);
        TEST_ASSERT_NOT_NULL(formatted);
        TEST_ASSERT_EQUAL('\n', formatted[strlen(formatted) - 1]);

        char expected[64];
        snprintf(expected, sizeof(expected), "{\"cmd\":\"%s\"}\n", basic_cmds[i]);
        TEST_ASSERT_EQUAL_STRING(expected, formatted);
        free(formatted);
    }

    // Volume command within valid range
    char *vol_cmd = cyd_client_format_command("vol", 42);
    TEST_ASSERT_NOT_NULL(vol_cmd);
    TEST_ASSERT_EQUAL('\n', vol_cmd[strlen(vol_cmd) - 1]);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol\",\"val\":42}\n", vol_cmd);
    free(vol_cmd);

    // Volume boundaries
    vol_cmd = cyd_client_format_command("vol", 0);
    TEST_ASSERT_NOT_NULL(vol_cmd);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol\",\"val\":0}\n", vol_cmd);
    free(vol_cmd);

    vol_cmd = cyd_client_format_command("vol", 100);
    TEST_ASSERT_NOT_NULL(vol_cmd);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol\",\"val\":100}\n", vol_cmd);
    free(vol_cmd);

    // Invalid volume should return NULL
    TEST_ASSERT_NULL(cyd_client_format_command("vol", -1));
    TEST_ASSERT_NULL(cyd_client_format_command("vol", 101));

    // Volume step commands
    char *step_up = cyd_client_format_command("vol_step", 5);
    TEST_ASSERT_NOT_NULL(step_up);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol_step\",\"dir\":5}\n", step_up);
    free(step_up);

    char *step_down = cyd_client_format_command("vol_step", -5);
    TEST_ASSERT_NOT_NULL(step_down);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol_step\",\"dir\":-5}\n", step_down);
    free(step_down);

    // Null or invalid command names
    TEST_ASSERT_NULL(cyd_client_format_command(NULL, 0));
    TEST_ASSERT_NULL(cyd_client_format_command("unsupported_cmd", 0));
}

/* =========================================================================
 * 3. UART Rx Feeding & Chunk Accumulation Tests
 * ========================================================================= */

void test_uart_feed_rx_bytes_and_callbacks(void) {
    s_callback_count = 0;
    cyd_client_register_event_callback(on_test_event);

    const char *line = "{\"event\":\"meta\",\"title\":\"Time\",\"artist\":\"Pink Floyd\",\"album\":\"Dark Side\"}\n";
    cyd_client_feed_rx_bytes(line, strlen(line));

    TEST_ASSERT_EQUAL_INT(1, s_callback_count);
    TEST_ASSERT_EQUAL_STRING("Time", s_last_callback_state.title);
    TEST_ASSERT_EQUAL_STRING("Pink Floyd", s_last_callback_state.artist);
    TEST_ASSERT_TRUE(s_last_callback_state.link_active);

    const cyd_telemetry_state_t *curr = cyd_client_get_state();
    TEST_ASSERT_NOT_NULL(curr);
    TEST_ASSERT_EQUAL_STRING("Time", curr->title);
}

void test_uart_feed_chunked_bytes(void) {
    s_callback_count = 0;
    cyd_client_register_event_callback(on_test_event);

    const char *chunk1 = "{\"event\":\"st";
    const char *chunk2 = "atus\",\"state\":\"pause\",\"el";
    const char *chunk3 = "apsed\":50,\"duration\":200,\"vol\":60}\r\n";

    cyd_client_feed_rx_bytes(chunk1, strlen(chunk1));
    TEST_ASSERT_EQUAL_INT(0, s_callback_count);

    cyd_client_feed_rx_bytes(chunk2, strlen(chunk2));
    TEST_ASSERT_EQUAL_INT(0, s_callback_count);

    cyd_client_feed_rx_bytes(chunk3, strlen(chunk3));
    TEST_ASSERT_EQUAL_INT(1, s_callback_count);

    TEST_ASSERT_EQUAL_STRING("pause", s_last_callback_state.state);
    TEST_ASSERT_EQUAL_UINT32(50, s_last_callback_state.elapsed);
    TEST_ASSERT_EQUAL_UINT32(200, s_last_callback_state.duration);
    TEST_ASSERT_EQUAL_UINT8(60, s_last_callback_state.vol);
}

void test_uart_feed_multiple_events_in_single_buffer(void) {
    s_callback_count = 0;
    cyd_client_register_event_callback(on_test_event);

    const char *multi = "{\"event\":\"sys\",\"mode\":\"LMS\",\"ip\":\"10.0.0.5\"}\n"
                        "{\"event\":\"status\",\"state\":\"play\",\"elapsed\":10,\"duration\":100,\"vol\":50}\n";

    cyd_client_feed_rx_bytes(multi, strlen(multi));
    TEST_ASSERT_EQUAL_INT(2, s_callback_count);

    const cyd_telemetry_state_t *curr = cyd_client_get_state();
    TEST_ASSERT_EQUAL_STRING("LMS", curr->mode);
    TEST_ASSERT_EQUAL_STRING("10.0.0.5", curr->ip);
    TEST_ASSERT_EQUAL_STRING("play", curr->state);
    TEST_ASSERT_EQUAL_UINT32(10, curr->elapsed);
}

void test_uart_feed_buffer_overflow_protection(void) {
    s_callback_count = 0;
    cyd_client_register_event_callback(on_test_event);

    // Feed 600 bytes without newline (overflows 512-byte buffer)
    char spam[600];
    memset(spam, 'X', sizeof(spam));
    cyd_client_feed_rx_bytes(spam, sizeof(spam));
    // Terminate corrupted line
    cyd_client_feed_rx_bytes("\n", 1);

    TEST_ASSERT_EQUAL_INT(0, s_callback_count);

    // Subsequent valid packet should be received and parsed cleanly
    const char *valid = "{\"event\":\"meta\",\"title\":\"Recovered\"}\n";
    cyd_client_feed_rx_bytes(valid, strlen(valid));

    TEST_ASSERT_EQUAL_INT(1, s_callback_count);
    TEST_ASSERT_EQUAL_STRING("Recovered", s_last_callback_state.title);
}

/* =========================================================================
 * 4. Link Heartbeat, Timeout, and Resync Tests
 * ========================================================================= */

void test_uart_heartbeat_and_timeout_resync(void) {
    s_tx_call_count = 0;
    s_callback_count = 0;
    cyd_client_set_tx_spy(on_test_tx);
    cyd_client_register_event_callback(on_test_event);

    // 1. Initial state: link is inactive. Polling at t=0 should transmit sync command
    cyd_client_poll_heartbeat(0);
    TEST_ASSERT_EQUAL_INT(1, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"sync\"}\n", s_last_tx_buf);
    TEST_ASSERT_FALSE(cyd_client_get_state()->link_active);

    // 2. Polling before 3 seconds (t=1.5s): should NOT send duplicate sync
    cyd_client_poll_heartbeat(1500000);
    TEST_ASSERT_EQUAL_INT(1, s_tx_call_count);

    // 3. Polling after 3 seconds (t=3.1s): should retransmit sync command
    cyd_client_poll_heartbeat(3100000);
    TEST_ASSERT_EQUAL_INT(2, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"sync\"}\n", s_last_tx_buf);

    // 4. Valid packet arrives at t=3.5s: link becomes active
    cyd_client_set_mock_time_us(3500000);
    const char *meta = "{\"event\":\"meta\",\"title\":\"Sync OK\"}\n";
    cyd_client_feed_rx_bytes(meta, strlen(meta));
    TEST_ASSERT_TRUE(cyd_client_get_state()->link_active);

    // 5. Polling at t=7.0s (3.5s elapsed < 5s timeout): link stays active, NO sync command sent
    cyd_client_poll_heartbeat(7000000);
    TEST_ASSERT_TRUE(cyd_client_get_state()->link_active);
    TEST_ASSERT_EQUAL_INT(2, s_tx_call_count);

    // 6. Polling at t=8.6s (5.1s elapsed > 5s timeout): link becomes inactive!
    // Callback should fire and sync command should be transmitted
    int cb_count_before = s_callback_count;
    cyd_client_poll_heartbeat(8600000);
    TEST_ASSERT_FALSE(cyd_client_get_state()->link_active);
    TEST_ASSERT_EQUAL_INT(cb_count_before + 1, s_callback_count);
    TEST_ASSERT_FALSE(s_last_callback_state.link_active);
    TEST_ASSERT_EQUAL_INT(3, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"sync\"}\n", s_last_tx_buf);

    // 7. Polling at t=10.0s (1.4s since sync): no new sync sent
    cyd_client_poll_heartbeat(10000000);
    TEST_ASSERT_EQUAL_INT(3, s_tx_call_count);

    // 8. Polling at t=11.7s (3.1s since sync): retransmits sync
    cyd_client_poll_heartbeat(11700000);
    TEST_ASSERT_EQUAL_INT(4, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"sync\"}\n", s_last_tx_buf);
}

/* =========================================================================
 * 5. Send Command and Raw Transmission Tests
 * ========================================================================= */

void test_uart_send_cmd_and_raw(void) {
    s_tx_call_count = 0;
    cyd_client_set_tx_spy(on_test_tx);

    // Send valid command
    esp_err_t err = cyd_client_send_cmd("toggle", 0);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_INT(1, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"toggle\"}\n", s_last_tx_buf);

    // Send valid volume command
    err = cyd_client_send_cmd("vol", 65);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL_INT(2, s_tx_call_count);
    TEST_ASSERT_EQUAL_STRING("{\"cmd\":\"vol\",\"val\":65}\n", s_last_tx_buf);

    // Invalid command should fail before TX
    err = cyd_client_send_cmd("vol", 200);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, err);
    TEST_ASSERT_EQUAL_INT(2, s_tx_call_count);

    err = cyd_client_send_cmd(NULL, 0);
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, err);
    TEST_ASSERT_EQUAL_INT(2, s_tx_call_count);

    // Send raw NULL or empty
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_client_send_raw(NULL));
    TEST_ASSERT_EQUAL(ESP_OK, cyd_client_send_raw(""));
}

void test_client_get_state_copy(void) {
    const char *line = "{\"event\":\"meta\",\"title\":\"Snapshot Track\",\"artist\":\"Snapshot Artist\",\"album\":\"Snapshot Album\"}\n";
    cyd_client_feed_rx_bytes(line, strlen(line));

    cyd_telemetry_state_t copy;
    memset(&copy, 0, sizeof(copy));
    cyd_client_get_state_copy(&copy);

    TEST_ASSERT_EQUAL_STRING("Snapshot Track", copy.title);
    TEST_ASSERT_EQUAL_STRING("Snapshot Artist", copy.artist);
    TEST_ASSERT_EQUAL_STRING("Snapshot Album", copy.album);
    TEST_ASSERT_TRUE(copy.link_active);
}

/* =========================================================================
 * Test Runners
 * ========================================================================= */

#if defined(ESP_PLATFORM)
TEST_CASE("CYD Client Protocol Parse Meta Event", "[cyd_comms]") {
    test_protocol_parse_meta_event();
}
TEST_CASE("CYD Client Protocol Parse Meta Partial and Long", "[cyd_comms]") {
    test_protocol_parse_meta_partial_and_long_strings();
}
TEST_CASE("CYD Client Protocol Parse Status Event", "[cyd_comms]") {
    test_protocol_parse_status_event();
}
TEST_CASE("CYD Client Protocol Parse Sys Event", "[cyd_comms]") {
    test_protocol_parse_sys_event();
}
TEST_CASE("CYD Client Protocol Parse Invalid or Malformed", "[cyd_comms]") {
    test_protocol_parse_invalid_or_malformed();
}
TEST_CASE("CYD Client Protocol Format Commands", "[cyd_comms]") {
    test_protocol_format_commands();
}
TEST_CASE("CYD Client UART Feed Rx Bytes and Callbacks", "[cyd_comms]") {
    test_uart_feed_rx_bytes_and_callbacks();
}
TEST_CASE("CYD Client UART Feed Chunked Bytes", "[cyd_comms]") {
    test_uart_feed_chunked_bytes();
}
TEST_CASE("CYD Client UART Feed Multiple Events In Single Buffer", "[cyd_comms]") {
    test_uart_feed_multiple_events_in_single_buffer();
}
TEST_CASE("CYD Client UART Feed Buffer Overflow Protection", "[cyd_comms]") {
    test_uart_feed_buffer_overflow_protection();
}
TEST_CASE("CYD Client UART Heartbeat and Timeout Resync", "[cyd_comms]") {
    test_uart_heartbeat_and_timeout_resync();
}
TEST_CASE("CYD Client UART Send Cmd and Raw", "[cyd_comms]") {
    test_uart_send_cmd_and_raw();
}
TEST_CASE("CYD Client Get State Copy", "[cyd_comms]") {
    test_client_get_state_copy();
}
#else
int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_protocol_parse_meta_event);
    RUN_TEST(test_protocol_parse_meta_partial_and_long_strings);
    RUN_TEST(test_protocol_parse_status_event);
    RUN_TEST(test_protocol_parse_sys_event);
    RUN_TEST(test_protocol_parse_invalid_or_malformed);
    RUN_TEST(test_protocol_format_commands);
    RUN_TEST(test_uart_feed_rx_bytes_and_callbacks);
    RUN_TEST(test_uart_feed_chunked_bytes);
    RUN_TEST(test_uart_feed_multiple_events_in_single_buffer);
    RUN_TEST(test_uart_feed_buffer_overflow_protection);
    RUN_TEST(test_uart_heartbeat_and_timeout_resync);
    RUN_TEST(test_uart_send_cmd_and_raw);
    RUN_TEST(test_client_get_state_copy);
    return UNITY_END();
}
#endif
