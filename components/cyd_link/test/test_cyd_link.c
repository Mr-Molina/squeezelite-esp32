#include "unity.h"
#if !defined(TEST_CASE) && __has_include("unity_test_runner.h")
#include "unity_test_runner.h"
#endif
#include "cyd_link_dispatch.h"
#include "cyd_link.h"
#include <string.h>
#include <stdlib.h>

TEST_CASE("CYD Link Formats Metadata Event Correctly", "[cyd_link]") {
    char *json = cyd_link_format_meta("Time", "Pink Floyd", "Dark Side");
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"title\":\"Time\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"artist\":\"Pink Floyd\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"album\":\"Dark Side\""));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);
}

TEST_CASE("CYD Link Formats Metadata Event With Null and Special Characters", "[cyd_link]") {
    // Null inputs should fallback to empty string
    char *json = cyd_link_format_meta(NULL, NULL, NULL);
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"title\":\"\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"artist\":\"\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"album\":\"\""));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);

    // Special characters: quotes, backslashes, UTF-8
    json = cyd_link_format_meta("Song \"Quoted\"", "Artist \\ Backslash", "日本語 Album");
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"title\":\"Song \\\"Quoted\\\"\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"artist\":\"Artist \\\\ Backslash\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "日本語 Album"));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);
}

TEST_CASE("CYD Link Formats Status Event Correctly", "[cyd_link]") {
    char *json = cyd_link_format_status("play", 45, 240, 80);
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"status\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"state\":\"play\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"elapsed\":45"));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"duration\":240"));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"vol\":80"));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);

    // Null state defaults to "stop"
    json = cyd_link_format_status(NULL, 0, 0, 0);
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"status\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"state\":\"stop\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"elapsed\":0"));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"duration\":0"));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"vol\":0"));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);
}

TEST_CASE("CYD Link Formats System Event Correctly", "[cyd_link]") {
    char *json = cyd_link_format_sys("LMS", "Living Room HiFi", "192.168.1.150");
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"sys\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"mode\":\"LMS\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"name\":\"Living Room HiFi\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"ip\":\"192.168.1.150\""));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);

    // Null mode defaults to "idle", name to "Squeezelite", ip to ""
    json = cyd_link_format_sys(NULL, NULL, NULL);
    TEST_ASSERT_NOT_NULL(json);
    TEST_ASSERT_NOT_NULL(strstr(json, "\"event\":\"sys\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"mode\":\"idle\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"name\":\"Squeezelite\""));
    TEST_ASSERT_NOT_NULL(strstr(json, "\"ip\":\"\""));
    TEST_ASSERT_EQUAL('\n', json[strlen(json) - 1]);
    free(json);
}

TEST_CASE("CYD Link Parses Command Correctly", "[cyd_link]") {
    cyd_command_t cmd;
    esp_err_t err;

    // Volume command
    err = cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":75}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOLUME, cmd.type);
    TEST_ASSERT_EQUAL(75, cmd.param);

    // Volume boundary values 0 and 100
    err = cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":0}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOLUME, cmd.type);
    TEST_ASSERT_EQUAL(0, cmd.param);

    err = cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":100}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOLUME, cmd.type);
    TEST_ASSERT_EQUAL(100, cmd.param);

    // Vol step command (positive and negative)
    err = cyd_link_parse_command("{\"cmd\":\"vol_step\",\"dir\":1}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOL_STEP, cmd.type);
    TEST_ASSERT_EQUAL(1, cmd.param);

    err = cyd_link_parse_command("{\"cmd\":\"vol_step\",\"dir\":-1}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOL_STEP, cmd.type);
    TEST_ASSERT_EQUAL(-1, cmd.param);

    // Toggle command
    err = cyd_link_parse_command("{\"cmd\":\"toggle\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_TOGGLE, cmd.type);

    // Play command
    err = cyd_link_parse_command("{\"cmd\":\"play\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_PLAY, cmd.type);

    // Pause command
    err = cyd_link_parse_command("{\"cmd\":\"pause\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_PAUSE, cmd.type);

    // Next command
    err = cyd_link_parse_command("{\"cmd\":\"next\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_NEXT, cmd.type);

    // Prev command
    err = cyd_link_parse_command("{\"cmd\":\"prev\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_PREV, cmd.type);

    // Sync command
    err = cyd_link_parse_command("{\"cmd\":\"sync\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_SYNC, cmd.type);
}

TEST_CASE("CYD Link Rejects Malformed or Oversized Command", "[cyd_link]") {
    cyd_command_t cmd;

    // NULL pointer handling
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command(NULL, &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"toggle\"}\n", NULL));

    // Non-JSON invalid string
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("invalid json string\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("\n", &cmd));

    // Missing 'cmd' property or wrong types
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"foo\":\"bar\"}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":123}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"unknown_action\"}\n", &cmd));

    // Volume out-of-range (<0 or >100) or non-numeric
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":150}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":-1}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":\"75\"}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol\"}\n", &cmd));

    // Vol step missing or non-numeric dir
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol_step\"}\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol_step\",\"dir\":\"up\"}\n", &cmd));
}

static bool s_toggle_called = false;
static void test_cmd_callback(cyd_cmd_type_t type, int32_t param) {
    (void)param;
    if (type == CYD_CMD_TOGGLE) s_toggle_called = true;
}

TEST_CASE("CYD Link Processes Line and Dispatches", "[cyd_link]") {
    s_toggle_called = false;
    cyd_link_set_cmd_handler(test_cmd_callback);
    const char *cmd_line = "{\"cmd\":\"toggle\"}\n";
    cyd_link_feed_rx_bytes(cmd_line, strlen(cmd_line));
    TEST_ASSERT_TRUE(s_toggle_called);
}

static int s_vol_val = -1;
static void test_vol_callback(cyd_cmd_type_t type, int32_t param) {
    (void)type;
    if (type == CYD_CMD_VOLUME) s_vol_val = param;
}

TEST_CASE("CYD Link Accumulates Fragmented Chunks Across Feeds", "[cyd_link]") {
    s_vol_val = -1;
    cyd_link_set_cmd_handler(test_vol_callback);
    const char *part1 = "{\"cmd\":\"vo";
    const char *part2 = "l\",\"val\":42}\r";
    const char *part3 = "\n";
    cyd_link_feed_rx_bytes(part1, strlen(part1));
    TEST_ASSERT_EQUAL(-1, s_vol_val);
    cyd_link_feed_rx_bytes(part2, strlen(part2));
    TEST_ASSERT_EQUAL(-1, s_vol_val);
    cyd_link_feed_rx_bytes(part3, strlen(part3));
    TEST_ASSERT_EQUAL(42, s_vol_val);
}

static int s_cmd_count = 0;
static void test_multi_callback(cyd_cmd_type_t type, int32_t param) {
    (void)type;
    (void)param;
    s_cmd_count++;
}

TEST_CASE("CYD Link Handles Multiple Commands In Single Buffer", "[cyd_link]") {
    s_cmd_count = 0;
    cyd_link_set_cmd_handler(test_multi_callback);
    const char *batch = "{\"cmd\":\"play\"}\n{\"cmd\":\"pause\"}\n{\"cmd\":\"next\"}\n";
    cyd_link_feed_rx_bytes(batch, strlen(batch));
    TEST_ASSERT_EQUAL(3, s_cmd_count);
}

static bool s_overflow_recovers = false;
static void test_overflow_callback(cyd_cmd_type_t type, int32_t param) {
    (void)param;
    if (type == CYD_CMD_PREV) s_overflow_recovers = true;
}

TEST_CASE("CYD Link Guards Against Buffer Overflow and Recovers", "[cyd_link]") {
    s_overflow_recovers = false;
    cyd_link_set_cmd_handler(test_overflow_callback);
    // Send 600 characters without newline (exceeding CYD_LINE_BUF_SIZE of 512)
    char junk[600];
    memset(junk, 'a', sizeof(junk));
    cyd_link_feed_rx_bytes(junk, sizeof(junk));
    // Terminate junk line
    cyd_link_feed_rx_bytes("\n", 1);
    TEST_ASSERT_FALSE(s_overflow_recovers);

    // Send a valid command now to verify receiver recovered cleanly
    const char *valid_cmd = "{\"cmd\":\"prev\"}\n";
    cyd_link_feed_rx_bytes(valid_cmd, strlen(valid_cmd));
    TEST_ASSERT_TRUE(s_overflow_recovers);
}

TEST_CASE("CYD Link Handles Edge Cases and Null Inputs", "[cyd_link]") {
    // Null inputs to feed
    cyd_link_feed_rx_bytes(NULL, 10);
    cyd_link_feed_rx_bytes("test", 0);

    // Standalone CR/LF should not trigger empty callbacks
    s_cmd_count = 0;
    cyd_link_set_cmd_handler(test_multi_callback);
    cyd_link_feed_rx_bytes("\n\n\r\r\n", 5);
    TEST_ASSERT_EQUAL(0, s_cmd_count);

    // Send raw uninitialized state check
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_STATE, cyd_link_send_raw(NULL));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_STATE, cyd_link_send_raw("{\"test\":1}\n"));

    // Direct execute command helper
    cyd_command_t cmd = { .type = CYD_CMD_PLAY, .param = 0 };
    s_cmd_count = 0;
    cyd_link_set_cmd_handler(test_multi_callback);
    cyd_link_execute_command(&cmd);
    TEST_ASSERT_EQUAL(1, s_cmd_count);
    cyd_link_execute_command(NULL);
    TEST_ASSERT_EQUAL(1, s_cmd_count);
}

