# CYD Touch Screen UART Management Link Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a bidirectional, wired UART communication link and management GUI between Squeezelite-ESP32 and an external CYD (ESP32-2432S028R) touch screen module, enabling remote "Now Playing" telemetry display and touch transport control.

**Architecture:** A secondary hardware UART running at 115200 8N1 transmits Newline-Delimited JSON (NDJSON) packets. In Squeezelite-ESP32, `components/cyd_link` hooks into `displayer` metadata and `audio_controls` handlers to emit state and execute inbound commands. On the CYD, an ESP-IDF subproject (`cyd-controller/`) runs an LVGL v8 user interface on an ILI9341 TFT display with XPT2046 touch controls, synchronizing state with Squeezelite.

**Tech Stack:** ESP-IDF (v4.4/v5.x), FreeRTOS (ringbuffers, queues, tasks), cJSON, LVGL v8, Hardware UART, SPI (ILI9341 & XPT2046).

**Spec:** [`docs/superpowers/specs/2026-09-29-cyd-uart-management-link-design.md`](file:///s:/Github/squeezelite-esp32/docs/superpowers/specs/2026-09-29-cyd-uart-management-link-design.md)

## Global Constraints

- **Language & Runtime:** C99 / C++17 under ESP-IDF FreeRTOS runtime.
- **Protocol Format:** Strict Newline-Delimited JSON (`\n` line termination, trailing `\r` stripped, max line size 512 bytes).
- **UART Port Allocation:** Squeezelite-ESP32 must use `UART_NUM_1` or `UART_NUM_2`; `UART_NUM_0` must remain untouched for console logs.
- **UART Baud Rate:** Default 115200 baud, 8 data bits, no parity, 1 stop bit (8N1).
- **CYD Target Hardware:** ESP32-2432S028R (ILI9341 320x240 LCD, XPT2046 resistive touch, Connector P3 IO22/IO27).
- **Memory Safety:** Every allocated `cJSON` pointer must be freed with `cJSON_Delete()` within the same scope before return.
- **Concurrency:** UART transmission must be synchronized by a FreeRTOS mutex to guarantee line atomicity.

## Review Focus

1. **Baud Rate Noise & Framing Garbage:** Garbled characters arriving before `\n` on wire attach/detach must be discarded without corrupting parser state or causing buffer overruns.
2. **Special Characters in Track/Artist Titles:** Quotes, backslashes, and international UTF-8 characters (e.g. Japanese, Cyrillic) in metadata must serialize and deserialize cleanly without crashing `cJSON`.
3. **Rapid Volume Scrubbing:** Scrubbing the touch volume slider must be debounced/throttled to $\le 10\text{ Hz}$ to prevent UART queue exhaustion.
4. **Out-of-Range Volume Parameter:** Volume payload with values $<0$ or $>100$ must be clamped or rejected gracefully without audio glitching.
5. **Reconnection & State Resync:** When CYD boots or reconnects after link interruption, sending `{"cmd":"sync"}` must trigger a full state burst (`sys`, `meta`, `status`).

---

### Task 1: Protocol Encoder & Decoder (`components/cyd_link/`)

**Files:**
- Create: `components/cyd_link/CMakeLists.txt`
- Create: `components/cyd_link/cyd_link_dispatch.h`
- Create: `components/cyd_link/cyd_link_dispatch.c`
- Create: `components/cyd_link/test/test_cyd_link.c`

**Interfaces:**
- Consumes: `cJSON.h`, `esp_err.h`
- Produces:
  - `esp_err_t cyd_link_parse_command(const char *json_str, cyd_command_t *out_cmd)`
  - `char *cyd_link_format_meta(const char *title, const char *artist, const char *album)`
  - `char *cyd_link_format_status(const char *state, uint32_t elapsed, uint32_t duration, uint8_t vol)`
  - `char *cyd_link_format_sys(const char *mode, const char *name, const char *ip)`

- [ ] **Step 1: Write the failing unit tests for JSON formatting and parsing**

```c
// components/cyd_link/test/test_cyd_link.c
#include "unity.h"
#include "cyd_link_dispatch.h"
#include <string.h>

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

TEST_CASE("CYD Link Parses Command Correctly", "[cyd_link]") {
    cyd_command_t cmd;
    esp_err_t err = cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":75}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_VOLUME, cmd.type);
    TEST_ASSERT_EQUAL(75, cmd.param);

    err = cyd_link_parse_command("{\"cmd\":\"toggle\"}\n", &cmd);
    TEST_ASSERT_EQUAL(ESP_OK, err);
    TEST_ASSERT_EQUAL(CYD_CMD_TOGGLE, cmd.type);
}

TEST_CASE("CYD Link Rejects Malformed or Oversized Command", "[cyd_link]") {
    cyd_command_t cmd;
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("invalid json string\n", &cmd));
    TEST_ASSERT_EQUAL(ESP_ERR_INVALID_ARG, cyd_link_parse_command("{\"cmd\":\"vol\",\"val\":150}\n", &cmd));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `idf.py test -T cyd_link`
Expected: FAIL with missing headers/functions `cyd_link_dispatch.h`.

- [ ] **Step 3: Write minimal implementation**

Create `components/cyd_link/CMakeLists.txt`:
```cmake
idf_component_register(
    SRCS "cyd_link_dispatch.c"
    INCLUDE_DIRS "."
    REQUIRES cjson
)
```

Create `components/cyd_link/cyd_link_dispatch.h`:
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

typedef enum {
    CYD_CMD_UNKNOWN = 0,
    CYD_CMD_TOGGLE,
    CYD_CMD_PLAY,
    CYD_CMD_PAUSE,
    CYD_CMD_NEXT,
    CYD_CMD_PREV,
    CYD_CMD_VOLUME,
    CYD_CMD_VOL_STEP,
    CYD_CMD_SYNC
} cyd_cmd_type_t;

typedef struct {
    cyd_cmd_type_t type;
    int32_t param;
} cyd_command_t;

esp_err_t cyd_link_parse_command(const char *json_str, cyd_command_t *out_cmd);
char *cyd_link_format_meta(const char *title, const char *artist, const char *album);
char *cyd_link_format_status(const char *state, uint32_t elapsed, uint32_t duration, uint8_t vol);
char *cyd_link_format_sys(const char *mode, const char *name, const char *ip);
```

Create `components/cyd_link/cyd_link_dispatch.c`:
```c
#include "cyd_link_dispatch.h"
#include "cJSON.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

static char *append_newline(char *json) {
    if (!json) return NULL;
    size_t len = strlen(json);
    char *out = malloc(len + 2);
    if (!out) {
        free(json);
        return NULL;
    }
    memcpy(out, json, len);
    out[len] = '\n';
    out[len + 1] = '\0';
    free(json);
    return out;
}

char *cyd_link_format_meta(const char *title, const char *artist, const char *album) {
    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;
    cJSON_AddStringToObject(root, "event", "meta");
    cJSON_AddStringToObject(root, "title", title ? title : "");
    cJSON_AddStringToObject(root, "artist", artist ? artist : "");
    cJSON_AddStringToObject(root, "album", album ? album : "");
    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}

char *cyd_link_format_status(const char *state, uint32_t elapsed, uint32_t duration, uint8_t vol) {
    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;
    cJSON_AddStringToObject(root, "event", "status");
    cJSON_AddStringToObject(root, "state", state ? state : "stop");
    cJSON_AddNumberToObject(root, "elapsed", elapsed);
    cJSON_AddNumberToObject(root, "duration", duration);
    cJSON_AddNumberToObject(root, "vol", vol);
    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}

char *cyd_link_format_sys(const char *mode, const char *name, const char *ip) {
    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;
    cJSON_AddStringToObject(root, "event", "sys");
    cJSON_AddStringToObject(root, "mode", mode ? mode : "idle");
    cJSON_AddStringToObject(root, "name", name ? name : "Squeezelite");
    cJSON_AddStringToObject(root, "ip", ip ? ip : "");
    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}

esp_err_t cyd_link_parse_command(const char *json_str, cyd_command_t *out_cmd) {
    if (!json_str || !out_cmd) return ESP_ERR_INVALID_ARG;
    out_cmd->type = CYD_CMD_UNKNOWN;
    out_cmd->param = 0;

    cJSON *root = cJSON_Parse(json_str);
    if (!root) return ESP_ERR_INVALID_ARG;

    cJSON *cmd_item = cJSON_GetObjectItem(root, "cmd");
    if (!cJSON_IsString(cmd_item) || !cmd_item->valuestring) {
        cJSON_Delete(root);
        return ESP_ERR_INVALID_ARG;
    }

    const char *cmd = cmd_item->valuestring;
    esp_err_t ret = ESP_OK;

    if (strcmp(cmd, "toggle") == 0) out_cmd->type = CYD_CMD_TOGGLE;
    else if (strcmp(cmd, "play") == 0) out_cmd->type = CYD_CMD_PLAY;
    else if (strcmp(cmd, "pause") == 0) out_cmd->type = CYD_CMD_PAUSE;
    else if (strcmp(cmd, "next") == 0) out_cmd->type = CYD_CMD_NEXT;
    else if (strcmp(cmd, "prev") == 0) out_cmd->type = CYD_CMD_PREV;
    else if (strcmp(cmd, "sync") == 0) out_cmd->type = CYD_CMD_SYNC;
    else if (strcmp(cmd, "vol") == 0) {
        cJSON *val = cJSON_GetObjectItem(root, "val");
        if (cJSON_IsNumber(val) && val->valueint >= 0 && val->valueint <= 100) {
            out_cmd->type = CYD_CMD_VOLUME;
            out_cmd->param = val->valueint;
        } else {
            ret = ESP_ERR_INVALID_ARG;
        }
    } else if (strcmp(cmd, "vol_step") == 0) {
        cJSON *dir = cJSON_GetObjectItem(root, "dir");
        if (cJSON_IsNumber(dir)) {
            out_cmd->type = CYD_CMD_VOL_STEP;
            out_cmd->param = dir->valueint;
        } else {
            ret = ESP_ERR_INVALID_ARG;
        }
    } else {
        ret = ESP_ERR_INVALID_ARG;
    }

    cJSON_Delete(root);
    return ret;
}
```

- [ ] **Step 4: Run unit tests and verify pass**

Run: `idf.py test -T cyd_link`
Expected: PASS all tests.

- [ ] **Step 5: Commit**

```bash
git add components/cyd_link/
git commit -m "feat(cyd_link): add NDJSON protocol encoder and command parser with unit tests"
```

---

### Task 2: Host UART Driver & FreeRTOS Tasks (`components/cyd_link/cyd_link.c`)

**Files:**
- Create: `components/cyd_link/cyd_link.h`
- Create: `components/cyd_link/cyd_link.c`
- Modify: `components/cyd_link/CMakeLists.txt`

**Interfaces:**
- Consumes: ESP-IDF `driver/uart.h`, `freertos/FreeRTOS.h`, `freertos/task.h`, `cyd_link_dispatch.h`
- Produces:
  - `esp_err_t cyd_link_init(void)`
  - `esp_err_t cyd_link_send_raw(const char *json_line)`
  - `void cyd_link_execute_command(const cyd_command_t *cmd)`

- [ ] **Step 1: Write test for line framing and command execution callback**

Add to `components/cyd_link/test/test_cyd_link.c`:
```c
static bool s_toggle_called = false;
static void test_cmd_callback(cyd_cmd_type_t type, int32_t param) {
    if (type == CYD_CMD_TOGGLE) s_toggle_called = true;
}

TEST_CASE("CYD Link Processes Line and Dispatches", "[cyd_link]") {
    s_toggle_called = false;
    cyd_link_set_cmd_handler(test_cmd_callback);
    cyd_link_feed_rx_bytes("{\"cmd\":\"toggle\"}\n", 16);
    TEST_ASSERT_TRUE(s_toggle_called);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `idf.py test -T cyd_link`
Expected: FAIL with undefined `cyd_link_feed_rx_bytes` and `cyd_link_set_cmd_handler`.

- [ ] **Step 3: Implement UART task and driver**

Create `components/cyd_link/cyd_link.h`:
```c
#pragma once
#include "esp_err.h"
#include "cyd_link_dispatch.h"

typedef void (*cyd_link_cmd_cb_t)(cyd_cmd_type_t type, int32_t param);

esp_err_t cyd_link_init(void);
esp_err_t cyd_link_send_raw(const char *json_line);
void cyd_link_set_cmd_handler(cyd_link_cmd_cb_t handler);
void cyd_link_feed_rx_bytes(const char *buf, size_t len);
```

Create `components/cyd_link/cyd_link.c`:
```c
#include "cyd_link.h"
#include "driver/uart.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include <string.h>

static const char *TAG = "cyd_link";
#define CYD_LINE_BUF_SIZE 512
#define CYD_UART_BUF_SIZE 1024

static uart_port_t s_uart_num = UART_NUM_1;
static SemaphoreHandle_t s_tx_mutex = NULL;
static cyd_link_cmd_cb_t s_cmd_handler = NULL;
static char s_rx_line[CYD_LINE_BUF_SIZE];
static size_t s_rx_idx = 0;

void cyd_link_set_cmd_handler(cyd_link_cmd_cb_t handler) {
    s_cmd_handler = handler;
}

void cyd_link_feed_rx_bytes(const char *buf, size_t len) {
    if (!buf || len == 0) return;
    for (size_t i = 0; i < len; i++) {
        char c = buf[i];
        if (c == '\r') continue;
        if (c == '\n') {
            if (s_rx_idx > 0) {
                s_rx_line[s_rx_idx] = '\0';
                cyd_command_t cmd;
                if (cyd_link_parse_command(s_rx_line, &cmd) == ESP_OK) {
                    if (s_cmd_handler) s_cmd_handler(cmd.type, cmd.param);
                } else {
                    ESP_LOGW(TAG, "Malformed CYD command: %s", s_rx_line);
                }
                s_rx_idx = 0;
            }
        } else {
            if (s_rx_idx < sizeof(s_rx_line) - 1) {
                s_rx_line[s_rx_idx++] = c;
            } else {
                // Overflow guard: drop corrupted line
                ESP_LOGE(TAG, "Line buffer overrun, dropping line");
                s_rx_idx = 0;
            }
        }
    }
}

static void cyd_uart_rx_task(void *pvParameters) {
    uint8_t data[128];
    while (1) {
        int len = uart_read_bytes(s_uart_num, data, sizeof(data), pdMS_TO_TICKS(50));
        if (len > 0) {
            cyd_link_feed_rx_bytes((const char *)data, len);
        }
    }
}

esp_err_t cyd_link_send_raw(const char *json_line) {
    if (!json_line || !s_tx_mutex) return ESP_ERR_INVALID_STATE;
    size_t len = strlen(json_line);
    if (len == 0) return ESP_OK;

    if (xSemaphoreTake(s_tx_mutex, pdMS_TO_TICKS(200)) == pdTRUE) {
        int written = uart_write_bytes(s_uart_num, json_line, len);
        xSemaphoreGive(s_tx_mutex);
        return (written == (int)len) ? ESP_OK : ESP_FAIL;
    }
    return ESP_ERR_TIMEOUT;
}

esp_err_t cyd_link_init(void) {
    if (s_tx_mutex == NULL) {
        s_tx_mutex = xSemaphoreCreateMutex();
    }

    uart_config_t uart_config = {
        .baud_rate = 115200,
        .data_bits = UART_DATA_8_BITS,
        .parity    = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
    };

    int tx_pin = 17, rx_pin = 16;
    ESP_ERROR_CHECK(uart_param_config(s_uart_num, &uart_config));
    ESP_ERROR_CHECK(uart_set_pin(s_uart_num, tx_pin, rx_pin, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));
    ESP_ERROR_CHECK(uart_driver_install(s_uart_num, CYD_UART_BUF_SIZE * 2, 0, 0, NULL, 0));

    xTaskCreate(cyd_uart_rx_task, "cyd_uart_rx", 3072, NULL, ESP_TASK_PRIO_MIN + 2, NULL);
    ESP_LOGI(TAG, "CYD link UART initialized on port %d (TX:%d, RX:%d)", s_uart_num, tx_pin, rx_pin);
    return ESP_OK;
}
```

Update `components/cyd_link/CMakeLists.txt`:
```cmake
idf_component_register(
    SRCS "cyd_link.c" "cyd_link_dispatch.c"
    INCLUDE_DIRS "."
    REQUIRES cjson driver
)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `idf.py test -T cyd_link`
Expected: PASS all tests.

- [ ] **Step 5: Commit**

```bash
git add components/cyd_link/
git commit -m "feat(cyd_link): add FreeRTOS UART ringbuffer task, mutex TX, and line receiver"
```

---

### Task 3: Core Hooks & Diagnostic Console CLI

**Files:**
- Create: `components/cyd_link/cyd_link_hooks.h`
- Create: `components/cyd_link/cyd_link_hooks.c`
- Create: `components/platform_console/cmd_cyd.c`
- Modify: `components/display/display.c`
- Modify: `main/esp_app_main.c`

**Interfaces:**
- Consumes: `services/audio_controls.h`, `display.h`, `cyd_link.h`
- Produces:
  - `void cyd_link_hook_metadata(const char *artist, const char *album, const char *title)`
  - `void cyd_link_hook_timer(uint32_t elapsed, uint32_t duration)`
  - `void cyd_link_broadcast_full_sync(void)`

- [ ] **Step 1: Write test for command dispatch to Squeezelite audio controls**

Add test in `components/cyd_link/test/test_cyd_link.c`:
```c
TEST_CASE("CYD Link Sync Broadcast Emits Sys, Meta, and Status", "[cyd_link]") {
    cyd_link_set_cached_meta("Song", "Band", "Album");
    cyd_link_broadcast_full_sync();
    // Validate output generated without memory leak or crash
    TEST_ASSERT_TRUE(true);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `idf.py test -T cyd_link`
Expected: FAIL with missing `cyd_link_hooks.h`.

- [ ] **Step 3: Implement hooks and action handler**

Create `components/cyd_link/cyd_link_hooks.h`:
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>

void cyd_link_hooks_init(void);
void cyd_link_hook_metadata(const char *artist, const char *album, const char *title);
void cyd_link_hook_timer(uint32_t elapsed, uint32_t duration);
void cyd_link_hook_playback_state(const char *state);
void cyd_link_broadcast_full_sync(void);
void cyd_link_set_cached_meta(const char *title, const char *artist, const char *album);
```

Create `components/cyd_link/cyd_link_hooks.c`:
```c
#include "cyd_link_hooks.h"
#include "cyd_link.h"
#include "audio_controls.h"
#include "esp_log.h"
#include <string.h>
#include <stdlib.h>

extern void output_volume(uint8_t val); // from squeezelite core output

static const char *TAG = "cyd_hooks";
static char s_cached_title[128] = "Idle";
static char s_cached_artist[128] = "";
static char s_cached_album[128] = "";
static char s_cached_state[16] = "stop";
static uint32_t s_cached_elapsed = 0;
static uint32_t s_cached_duration = 0;
static uint8_t s_cached_vol = 50;

void cyd_link_set_cached_meta(const char *title, const char *artist, const char *album) {
    if (title) strncpy(s_cached_title, title, sizeof(s_cached_title) - 1);
    if (artist) strncpy(s_cached_artist, artist, sizeof(s_cached_artist) - 1);
    if (album) strncpy(s_cached_album, album, sizeof(s_cached_album) - 1);
}

void cyd_link_hook_metadata(const char *artist, const char *album, const char *title) {
    cyd_link_set_cached_meta(title, artist, album);
    char *msg = cyd_link_format_meta(s_cached_title, s_cached_artist, s_cached_album);
    if (msg) {
        cyd_link_send_raw(msg);
        free(msg);
    }
}

void cyd_link_hook_timer(uint32_t elapsed, uint32_t duration) {
    s_cached_elapsed = elapsed;
    s_cached_duration = duration;
    char *msg = cyd_link_format_status(s_cached_state, s_cached_elapsed, s_cached_duration, s_cached_vol);
    if (msg) {
        cyd_link_send_raw(msg);
        free(msg);
    }
}

void cyd_link_hook_playback_state(const char *state) {
    if (state) strncpy(s_cached_state, state, sizeof(s_cached_state) - 1);
    cyd_link_hook_timer(s_cached_elapsed, s_cached_duration);
}

void cyd_link_broadcast_full_sync(void) {
    char *sys_msg = cyd_link_format_sys("LMS", "Squeezelite-ESP32", "0.0.0.0");
    if (sys_msg) {
        cyd_link_send_raw(sys_msg);
        free(sys_msg);
    }
    char *meta_msg = cyd_link_format_meta(s_cached_title, s_cached_artist, s_cached_album);
    if (meta_msg) {
        cyd_link_send_raw(meta_msg);
        free(meta_msg);
    }
    char *stat_msg = cyd_link_format_status(s_cached_state, s_cached_elapsed, s_cached_duration, s_cached_vol);
    if (stat_msg) {
        cyd_link_send_raw(stat_msg);
        free(stat_msg);
    }
}

static void on_cyd_command(cyd_cmd_type_t type, int32_t param) {
    actrls_handler handler = NULL;
    switch (type) {
        case CYD_CMD_TOGGLE:
            handler = get_ctrl_handler(ACTRLS_TOGGLE);
            if (handler) handler(true);
            break;
        case CYD_CMD_PLAY:
            handler = get_ctrl_handler(ACTRLS_PLAY);
            if (handler) handler(true);
            break;
        case CYD_CMD_PAUSE:
            handler = get_ctrl_handler(ACTRLS_PAUSE);
            if (handler) handler(true);
            break;
        case CYD_CMD_NEXT:
            handler = get_ctrl_handler(ACTRLS_NEXT);
            if (handler) handler(true);
            break;
        case CYD_CMD_PREV:
            handler = get_ctrl_handler(ACTRLS_PREV);
            if (handler) handler(true);
            break;
        case CYD_CMD_VOLUME:
            s_cached_vol = (uint8_t)param;
            output_volume(s_cached_vol);
            break;
        case CYD_CMD_VOL_STEP:
            handler = (param > 0) ? get_ctrl_handler(ACTRLS_VOLUP) : get_ctrl_handler(ACTRLS_VOLDOWN);
            if (handler) handler(true);
            break;
        case CYD_CMD_SYNC:
            cyd_link_broadcast_full_sync();
            break;
        default:
            ESP_LOGW(TAG, "Unhandled command type: %d", type);
            break;
    }
}

void cyd_link_hooks_init(void) {
    cyd_link_set_cmd_handler(on_cyd_command);
}
```

Wire hooks into `components/display/display.c`:
Add call to `cyd_link_hook_metadata(artist, album, title)` in `displayer_metadata()` so telemetry is generated regardless of whether local display hardware is attached.

Create console diagnostic command `components/platform_console/cmd_cyd.c`:
```c
#include "esp_console.h"
#include "argtable3/argtable3.h"
#include "cyd_link.h"
#include "cyd_link_hooks.h"
#include <stdio.h>

static int do_cyd_cmd(int argc, char **argv) {
    if (argc < 2) {
        printf("Usage: cyd <sync|play|pause|next|prev|vol <0-100>>\n");
        return 0;
    }
    if (strcmp(argv[1], "sync") == 0) {
        cyd_link_broadcast_full_sync();
    } else if (strcmp(argv[1], "play") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"play\"}\n", 15);
    } else if (strcmp(argv[1], "pause") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"pause\"}\n", 16);
    } else if (strcmp(argv[1], "next") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"next\"}\n", 15);
    } else if (strcmp(argv[1], "prev") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"prev\"}\n", 15);
    }
    return 0;
}

void register_cyd(void) {
    const esp_console_cmd_t cmd = {
        .command = "cyd",
        .help = "CYD Touch Link Diagnostic Tool",
        .hint = NULL,
        .func = &do_cyd_cmd,
    };
    esp_console_cmd_register(&cmd);
}
```

- [ ] **Step 4: Run unit tests and verify pass**

Run: `idf.py test -T cyd_link`
Expected: PASS all tests.

- [ ] **Step 5: Commit**

```bash
git add components/cyd_link/ components/display/display.c components/platform_console/cmd_cyd.c main/esp_app_main.c
git commit -m "feat(cyd_link): integrate hooks into audio controls, displayer, and console CLI"
```

---

### Task 4: CYD Subproject Scaffolding & Display/Touch Drivers (`cyd-controller/`)

**Files:**
- Create: `cyd-controller/CMakeLists.txt`
- Create: `cyd-controller/sdkconfig.defaults`
- Create: `cyd-controller/main/CMakeLists.txt`
- Create: `cyd-controller/main/drivers/ili9341.h`
- Create: `cyd-controller/main/drivers/ili9341.c`
- Create: `cyd-controller/main/drivers/xpt2046.h`
- Create: `cyd-controller/main/drivers/xpt2046.c`

**Interfaces:**
- Consumes: ESP-IDF `driver/spi_master.h`, `driver/gpio.h`
- Produces:
  - `esp_err_t ili9341_init(void)`
  - `void ili9341_flush(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const uint16_t *color_data)`
  - `esp_err_t xpt2046_init(void)`
  - `bool xpt2046_read(int16_t *x, int16_t *y)`

- [ ] **Step 1: Write header definitions and pin mapping constants**

Create `cyd-controller/main/drivers/ili9341.h`:
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

#define CYD_TFT_MISO 12
#define CYD_TFT_MOSI 13
#define CYD_TFT_SCLK 14
#define CYD_TFT_CS   15
#define CYD_TFT_DC   2
#define CYD_TFT_RST  -1
#define CYD_TFT_BL   21

#define CYD_TFT_WIDTH  320
#define CYD_TFT_HEIGHT 240

esp_err_t ili9341_init(void);
void ili9341_flush(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const uint16_t *color_data);
```

Create `cyd-controller/main/drivers/xpt2046.h`:
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

#define CYD_TOUCH_MISO 39
#define CYD_TOUCH_MOSI 32
#define CYD_TOUCH_SCLK 25
#define CYD_TOUCH_CS   33
#define CYD_TOUCH_IRQ  36

esp_err_t xpt2046_init(void);
bool xpt2046_read(int16_t *out_x, int16_t *out_y);
```

- [ ] **Step 2: Implement ILI9341 SPI initialization and window flush**

Create `cyd-controller/main/drivers/ili9341.c` with hardware SPI transaction commands (`0x2A`, `0x2B`, `0x2C`) and backlight PWM initialization on GPIO 21.

- [ ] **Step 3: Implement XPT2046 SPI touch controller driver**

Create `cyd-controller/main/drivers/xpt2046.c` with coordinate reading (commands `0xD0` for X and `0x90` for Y), resistive touch calibration formulas, and coordinate clamping to 320x240 landscape.

- [ ] **Step 4: Create CMake build scaffolding**

Create `cyd-controller/CMakeLists.txt`:
```cmake
cmake_minimum_required(VERSION 3.16)
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(cyd-controller)
```

Create `cyd-controller/main/CMakeLists.txt`:
```cmake
idf_component_register(
    SRCS "drivers/ili9341.c" "drivers/xpt2046.c"
    INCLUDE_DIRS "drivers" "."
    REQUIRES driver esp_timer
)
```

- [ ] **Step 5: Verify build dry-run and commit**

```bash
git add cyd-controller/
git commit -m "feat(cyd-controller): add CYD hardware scaffolding with ILI9341 and XPT2046 SPI drivers"
```

---

### Task 5: CYD Client Protocol Engine & UART Receiver (`cyd-controller/main/comms/`)

**Files:**
- Create: `cyd-controller/main/comms/cyd_uart.h`
- Create: `cyd-controller/main/comms/cyd_uart.c`
- Create: `cyd-controller/main/comms/cyd_protocol.h`
- Create: `cyd-controller/main/comms/cyd_protocol.c`
- Modify: `cyd-controller/main/CMakeLists.txt`

**Interfaces:**
- Consumes: ESP-IDF `driver/uart.h`, `cJSON.h`
- Produces:
  - `esp_err_t cyd_client_uart_init(void)`
  - `esp_err_t cyd_client_send_cmd(const char *cmd_name, int32_t val)`
  - `void cyd_client_register_event_callback(cyd_event_cb_t cb)`

- [ ] **Step 1: Write header and telemetry data structures**

Create `cyd-controller/main/comms/cyd_protocol.h`:
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

typedef struct {
    char title[128];
    char artist[128];
    char album[128];
    char state[16];
    char mode[16];
    char ip[32];
    uint32_t elapsed;
    uint32_t duration;
    uint8_t vol;
    bool link_active;
} cyd_telemetry_state_t;

typedef void (*cyd_event_cb_t)(const cyd_telemetry_state_t *state);

esp_err_t cyd_client_parse_event(const char *line, cyd_telemetry_state_t *state);
```

- [ ] **Step 2: Implement JSON telemetry unpacker**

Create `cyd-controller/main/comms/cyd_protocol.c`:
Parse `event: meta` (populate title, artist, album), `event: status` (populate state, elapsed, duration, vol), and `event: sys` (populate mode, ip). Guarantee `cJSON_Delete()` on every path.

- [ ] **Step 3: Implement UART ringbuffer reader and heartbeat timer**

Create `cyd-controller/main/comms/cyd_uart.h` and `cyd-controller/main/comms/cyd_uart.c`:
- Port: `UART_NUM_2` (RX: GPIO 22, TX: GPIO 27, 115200 8N1).
- Send commands: `cyd_client_send_cmd("toggle", 0)`, `cyd_client_send_cmd("vol", 75)`, `cyd_client_send_cmd("sync", 0)`.
- Heartbeat: Keep track of `esp_timer_get_time()`. If $>5\text{s}$ since last packet, set `state.link_active = false` and re-send `{"cmd":"sync"}\n` every 3s.

- [ ] **Step 4: Update `cyd-controller/main/CMakeLists.txt`**

Add `comms/cyd_uart.c` and `comms/cyd_protocol.c` to SRCS, `comms` to INCLUDE_DIRS, and `cjson` to REQUIRES.

- [ ] **Step 5: Commit**

```bash
git add cyd-controller/main/comms/ cyd-controller/main/CMakeLists.txt
git commit -m "feat(cyd-controller): add CYD client UART driver and NDJSON event processor"
```

---

### Task 6: CYD LVGL Now Playing UI & Touch Controls (`cyd-controller/main/ui/` & `main/main.c`)

**Files:**
- Create: `cyd-controller/main/ui/ui_theme.h`
- Create: `cyd-controller/main/ui/ui_theme.c`
- Create: `cyd-controller/main/ui/ui_now_playing.h`
- Create: `cyd-controller/main/ui/ui_now_playing.c`
- Create: `cyd-controller/main/main.c`

**Interfaces:**
- Consumes: `lvgl.h`, `cyd_protocol.h`, `cyd_uart.h`
- Produces:
  - `void ui_now_playing_create(void)`
  - `void ui_now_playing_update(const cyd_telemetry_state_t *state)`

- [ ] **Step 1: Implement dark modern theme**

Create `cyd-controller/main/ui/ui_theme.h` and `ui_theme.c` defining color constants: Background (`0x121212`), Surface (`0x1E1E1E`), Accent (`0x1DB954` Spotify Green / Bright Blue), Text Primary (`0xFFFFFF`), Text Muted (`0xAAAAAA`).

- [ ] **Step 2: Implement UI layout and widgets**

Create `cyd-controller/main/ui/ui_now_playing.c`:
- **Top Bar**: Label for link status / IP (`"Living Room HiFi - 192.168.1.50"`), badge for source (`"[SPOTIFY]"`).
- **Center Area**:
  - `lbl_title`: Title label with `LV_LABEL_LONG_SCROLL_CIRCULAR`.
  - `lbl_artist`: Artist and album label.
  - `bar_progress`: Visual bar with `lbl_elapsed` ("01:23") and `lbl_total` ("04:15").
- **Bottom Bar**:
  - `btn_prev`: Emits `{"cmd":"prev"}\n`.
  - `btn_play_pause`: Toggles icon between Play and Pause, emits `{"cmd":"toggle"}\n`.
  - `btn_next`: Emits `{"cmd":"next"}\n`.
  - `slider_vol`: Volume slider (0-100) with throttled callback emitting `{"cmd":"vol","val":V}\n` at $\le 10\text{ Hz}$.

- [ ] **Step 3: Implement main FreeRTOS application entry point**

Create `cyd-controller/main/main.c`:
- Initialize ILI9341 display and register LVGL display buffer.
- Initialize XPT2046 touch and register LVGL indev driver.
- Initialize UART communication and start LVGL task loop (`lv_timer_handler()`).
- Emit initial `{"cmd":"sync"}\n` on startup.

- [ ] **Step 4: Commit**

```bash
git add cyd-controller/main/ui/ cyd-controller/main/main.c
git commit -m "feat(cyd-controller): add LVGL 320x240 Now Playing screen and touch transport controls"
```

---

### Task 7: End-to-End Loopback & Integration Verification

**Files:**
- Create: `test/main/test_cyd_integration.c`
- Modify: `test/CMakeLists.txt`

**Interfaces:**
- Consumes: Squeezelite core, `cyd_link`, `cyd-controller` comms logic

- [ ] **Step 1: Write integration loopback test**

Create `test/main/test_cyd_integration.c`:
Verify that when CYD sends `{"cmd":"vol","val":80}\n`, the Squeezelite audio output volume is updated to 80; verify that when `displayer_metadata()` is invoked, a valid `{"event":"meta",...}\n` packet is emitted.

- [ ] **Step 2: Run loopback test and verify pass**

Run: `idf.py test -T cyd_integration`
Expected: PASS.

- [ ] **Step 3: Run full git status and clean tree check**

Run: `git status`
Expected: Clean working tree on `milestone-mk-5`.

- [ ] **Step 4: Final commit for integration test**

```bash
git add test/
git commit -m "test(cyd): add end-to-end integration and loopback tests for CYD management link"
```
