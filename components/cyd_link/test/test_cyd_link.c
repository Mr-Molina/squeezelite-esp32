#include "unity.h"
#if !defined(TEST_CASE) && __has_include("unity_test_runner.h")
#include "unity_test_runner.h"
#endif

#if !defined(TEST_CASE)
typedef struct {
    const char *name;
    void (*fn)(void);
} cyd_test_t;
static cyd_test_t s_tests[32];
static int s_test_count = 0;
#define TEST_CONCAT_INNER(a, b) a##b
#define TEST_CONCAT(a, b) TEST_CONCAT_INNER(a, b)
#define TEST_CASE(name, tag) \
    static void TEST_CONCAT(test_fn_, __LINE__)(void); \
    __attribute__((constructor)) static void TEST_CONCAT(reg_, __LINE__)(void) { \
        s_tests[s_test_count++] = (cyd_test_t){ name, TEST_CONCAT(test_fn_, __LINE__) }; \
    } \
    static void TEST_CONCAT(test_fn_, __LINE__)(void)
#endif
#include "cyd_link_dispatch.h"
#include "cyd_link.h"
#include "cyd_link_hooks.h"
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
    // Send 1200 characters without newline (exceeding CYD_LINE_BUF_SIZE of 1024)
    char junk[1200];
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

TEST_CASE("CYD Link Handles Payloads Larger Than 512 Bytes Up To 1024", "[cyd_link]") {
    s_cmd_count = 0;
    cyd_link_set_cmd_handler(test_multi_callback);
    // Construct ~800 byte valid JSON command line to verify MEM-01 support
    char large_payload[850];
    int offset = snprintf(large_payload, sizeof(large_payload), "{\"cmd\":\"play\",\"pad\":\"");
    memset(large_payload + offset, 'X', 700);
    snprintf(large_payload + offset + 700, sizeof(large_payload) - (offset + 700), "\"}\n");

    cyd_link_feed_rx_bytes(large_payload, strlen(large_payload));
    TEST_ASSERT_EQUAL(1, s_cmd_count);
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

// Mock audio control handlers and output_volume for testing
#if !defined(ESP_PLATFORM)
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
#endif

static bool s_mock_toggle = false;
static bool s_mock_play = false;
static bool s_mock_pause = false;
static bool s_mock_next = false;
static bool s_mock_prev = false;
static bool s_mock_volup = false;
static bool s_mock_voldown = false;
static int s_mock_vol_val = -1;

static void mock_toggle_handler(bool pressed) { if (pressed) s_mock_toggle = true; }
static void mock_play_handler(bool pressed) { if (pressed) s_mock_play = true; }
static void mock_pause_handler(bool pressed) { if (pressed) s_mock_pause = true; }
static void mock_next_handler(bool pressed) { if (pressed) s_mock_next = true; }
static void mock_prev_handler(bool pressed) { if (pressed) s_mock_prev = true; }
static void mock_volup_handler(bool pressed) { if (pressed) s_mock_volup = true; }
static void mock_voldown_handler(bool pressed) { if (pressed) s_mock_voldown = true; }

actrls_handler get_ctrl_handler(actrls_action_e action) {
    switch (action) {
        case ACTRLS_TOGGLE: return mock_toggle_handler;
        case ACTRLS_PLAY: return mock_play_handler;
        case ACTRLS_PAUSE: return mock_pause_handler;
        case ACTRLS_NEXT: return mock_next_handler;
        case ACTRLS_PREV: return mock_prev_handler;
        case ACTRLS_VOLUP: return mock_volup_handler;
        case ACTRLS_VOLDOWN: return mock_voldown_handler;
        default: return NULL;
    }
}

void output_volume(uint8_t val) {
    s_mock_vol_val = (int)val;
}

static char s_tx_log[16][256];
static int s_tx_log_count = 0;
static void test_capture_tx(const char *data, size_t len) {
    if (s_tx_log_count < 16 && len < 256) {
        memcpy(s_tx_log[s_tx_log_count], data, len);
        s_tx_log[s_tx_log_count][len] = '\0';
        s_tx_log_count++;
    }
}

TEST_CASE("CYD Link Hooks Cache Metadata and Broadcast Full Sync", "[cyd_link]") {
    cyd_link_init();
    cyd_link_set_tx_spy(test_capture_tx);
    s_tx_log_count = 0;

    cyd_link_set_cached_meta("Bohemian Rhapsody", "Queen", "A Night at the Opera");
    cyd_link_hook_playback_state("play");
    cyd_link_hook_timer(120, 355);

    // Verify timer hook emitted status event
    TEST_ASSERT_TRUE(s_tx_log_count > 0);
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[s_tx_log_count - 1], "\"event\":\"status\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[s_tx_log_count - 1], "\"state\":\"play\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[s_tx_log_count - 1], "\"elapsed\":120"));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[s_tx_log_count - 1], "\"duration\":355"));

    // Reset capture and test broadcast full sync
    s_tx_log_count = 0;
    cyd_link_broadcast_full_sync();

    // Broadcast full sync sends sys, meta, status in order
    TEST_ASSERT_EQUAL(3, s_tx_log_count);
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"event\":\"sys\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[1], "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[1], "\"title\":\"Bohemian Rhapsody\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[1], "\"artist\":\"Queen\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[1], "\"album\":\"A Night at the Opera\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[2], "\"event\":\"status\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[2], "\"state\":\"play\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[2], "\"elapsed\":120"));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[2], "\"duration\":355"));

    // Test hook_metadata updates cache and sends meta event
    s_tx_log_count = 0;
    cyd_link_hook_metadata("Pink Floyd", "The Dark Side", "Money");
    TEST_ASSERT_EQUAL(1, s_tx_log_count);
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"title\":\"Money\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"artist\":\"Pink Floyd\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"album\":\"The Dark Side\""));

    cyd_link_set_tx_spy(NULL);
}

TEST_CASE("CYD Link Hooks Dispatch Commands to Audio Control Handlers", "[cyd_link]") {
    cyd_link_hooks_init();

    // Toggle
    s_mock_toggle = false;
    const char *toggle_cmd = "{\"cmd\":\"toggle\"}\n";
    cyd_link_feed_rx_bytes(toggle_cmd, strlen(toggle_cmd));
    TEST_ASSERT_TRUE(s_mock_toggle);

    // Play
    s_mock_play = false;
    const char *play_cmd = "{\"cmd\":\"play\"}\n";
    cyd_link_feed_rx_bytes(play_cmd, strlen(play_cmd));
    TEST_ASSERT_TRUE(s_mock_play);

    // Pause
    s_mock_pause = false;
    const char *pause_cmd = "{\"cmd\":\"pause\"}\n";
    cyd_link_feed_rx_bytes(pause_cmd, strlen(pause_cmd));
    TEST_ASSERT_TRUE(s_mock_pause);

    // Next
    s_mock_next = false;
    const char *next_cmd = "{\"cmd\":\"next\"}\n";
    cyd_link_feed_rx_bytes(next_cmd, strlen(next_cmd));
    TEST_ASSERT_TRUE(s_mock_next);

    // Prev
    s_mock_prev = false;
    const char *prev_cmd = "{\"cmd\":\"prev\"}\n";
    cyd_link_feed_rx_bytes(prev_cmd, strlen(prev_cmd));
    TEST_ASSERT_TRUE(s_mock_prev);

    // Vol Step Up
    s_mock_volup = false;
    const char *volup_cmd = "{\"cmd\":\"vol_step\",\"dir\":1}\n";
    cyd_link_feed_rx_bytes(volup_cmd, strlen(volup_cmd));
    TEST_ASSERT_TRUE(s_mock_volup);

    // Vol Step Down
    s_mock_voldown = false;
    const char *voldown_cmd = "{\"cmd\":\"vol_step\",\"dir\":-1}\n";
    cyd_link_feed_rx_bytes(voldown_cmd, strlen(voldown_cmd));
    TEST_ASSERT_TRUE(s_mock_voldown);

    // Volume Set
    s_mock_vol_val = -1;
    const char *vol_cmd = "{\"cmd\":\"vol\",\"val\":85}\n";
    cyd_link_feed_rx_bytes(vol_cmd, strlen(vol_cmd));
    TEST_ASSERT_EQUAL(85, s_mock_vol_val);

    // Sync command from CYD triggers sync broadcast
    cyd_link_set_tx_spy(test_capture_tx);
    s_tx_log_count = 0;
    const char *sync_cmd = "{\"cmd\":\"sync\"}\n";
    cyd_link_feed_rx_bytes(sync_cmd, strlen(sync_cmd));
    TEST_ASSERT_EQUAL(3, s_tx_log_count);
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[0], "\"event\":\"sys\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[1], "\"event\":\"meta\""));
    TEST_ASSERT_NOT_NULL(strstr(s_tx_log[2], "\"event\":\"status\""));
    cyd_link_set_tx_spy(NULL);
}

#if !defined(ESP_PLATFORM)
void setUp(void) {}
void tearDown(void) {}

int main(void) {
    UNITY_BEGIN();
    for (int i = s_test_count - 1; i >= 0; i--) {
        UnityDefaultTestRun(s_tests[i].fn, s_tests[i].name, s_test_count - i);
    }
    return UNITY_END();
}
#endif

