#include "cyd_uart.h"
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

#if defined(ESP_PLATFORM)
#include "driver/uart.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "esp_task.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#else
// Host test stubs
#ifndef ESP_LOGI
#define ESP_LOGI(tag, fmt, ...) do { (void)(tag); } while(0)
#endif
#ifndef ESP_LOGW
#define ESP_LOGW(tag, fmt, ...) do { (void)(tag); } while(0)
#endif
#ifndef ESP_LOGE
#define ESP_LOGE(tag, fmt, ...) do { (void)(tag); } while(0)
#endif
#ifndef ESP_ERROR_CHECK
#define ESP_ERROR_CHECK(x) (void)(x)
#endif

typedef int uart_port_t;
#define UART_NUM_2 2
#define UART_DATA_8_BITS 0
#define UART_PARITY_DISABLE 0
#define UART_STOP_BITS_1 1
#define UART_HW_FLOWCTRL_DISABLE 0
#define UART_PIN_NO_CHANGE -1

typedef struct {
    int baud_rate;
    int data_bits;
    int parity;
    int stop_bits;
    int flow_ctrl;
} uart_config_t;

typedef void *SemaphoreHandle_t;
typedef void *TaskHandle_t;
#define pdTRUE 1
#define pdFALSE 0
#define pdMS_TO_TICKS(ms) (ms)
#ifndef ESP_TASK_PRIO_MIN
#define ESP_TASK_PRIO_MIN 1
#endif

#ifndef portMAX_DELAY
#define portMAX_DELAY 0xFFFFFFFFUL
#endif

static inline SemaphoreHandle_t xSemaphoreCreateMutex(void) {
    static int dummy = 1;
    return (SemaphoreHandle_t)&dummy;
}
static inline int xSemaphoreTake(SemaphoreHandle_t sem, int ticks) { (void)sem; (void)ticks; return pdTRUE; }
static inline void xSemaphoreGive(SemaphoreHandle_t sem) { (void)sem; }
static inline int uart_param_config(uart_port_t u, const uart_config_t *c) { (void)u; (void)c; return 0; }
static inline int uart_set_pin(uart_port_t u, int tx, int rx, int rts, int cts) { (void)u; (void)tx; (void)rx; (void)rts; (void)cts; return 0; }
static inline int uart_driver_install(uart_port_t u, int rx, int tx, int qs, void *q, int f) { (void)u; (void)rx; (void)tx; (void)qs; (void)q; (void)f; return 0; }
static inline int uart_write_bytes(uart_port_t u, const char *src, size_t size) { (void)u; (void)src; return (int)size; }
static inline int uart_read_bytes(uart_port_t u, void *buf, uint32_t len, int ticks) { (void)u; (void)buf; (void)len; (void)ticks; return 0; }
typedef void (*TaskFunction_t)(void *);
static inline int xTaskCreate(TaskFunction_t fn, const char *n, int s, void *p, int prio, TaskHandle_t *h) {
    (void)fn; (void)n; (void)s; (void)p; (void)prio; (void)h; return 1;
}

static int64_t s_mock_time_us = 0;
static inline int64_t esp_timer_get_time(void) {
    return s_mock_time_us;
}
#endif

static const char *TAG __attribute__((unused)) = "cyd_uart";

#define CYD_LINE_BUF_SIZE 1024
#define CYD_UART_RX_BUF_SIZE 2048
#define CYD_UART_TX_BUF_SIZE 1024
#define CYD_HEARTBEAT_TIMEOUT_US  5000000LL  // 5 seconds
#define CYD_RESYNC_INTERVAL_US    3000000LL  // 3 seconds

static SemaphoreHandle_t s_tx_mutex = NULL;
static SemaphoreHandle_t s_state_mutex = NULL;
static TaskHandle_t s_rx_task_handle __attribute__((unused)) = NULL;
static cyd_event_cb_t s_event_cb = NULL;
static cyd_tx_spy_cb_t s_tx_spy = NULL;

static cyd_telemetry_state_t s_state;
static char s_rx_line[CYD_LINE_BUF_SIZE];
static size_t s_rx_idx = 0;
static bool s_rx_overflow = false;

static int64_t s_last_rx_time_us = 0;
static int64_t s_last_sync_time_us = -1LL;
static bool s_initialized = false;

void cyd_client_set_mock_time_us(int64_t us) {
#if !defined(ESP_PLATFORM)
    s_mock_time_us = us;
#else
    (void)us;
#endif
}

void cyd_client_reset_state(void) {
    if (s_tx_mutex == NULL) {
        s_tx_mutex = xSemaphoreCreateMutex();
    }
    if (s_state_mutex == NULL) {
        s_state_mutex = xSemaphoreCreateMutex();
    }
    if (s_state_mutex) xSemaphoreTake(s_state_mutex, portMAX_DELAY);
    memset(&s_state, 0, sizeof(s_state));
    strncpy(s_state.state, "stop", sizeof(s_state.state) - 1);
    s_state.link_active = false;
    s_rx_idx = 0;
    s_rx_overflow = false;
    s_last_rx_time_us = 0;
    s_last_sync_time_us = -1LL;
    if (s_state_mutex) xSemaphoreGive(s_state_mutex);
}

void cyd_client_register_event_callback(cyd_event_cb_t cb) {
    s_event_cb = cb;
}

void cyd_client_set_tx_spy(cyd_tx_spy_cb_t spy) {
    s_tx_spy = spy;
}

const cyd_telemetry_state_t *cyd_client_get_state(void) {
    return &s_state;
}

void cyd_client_get_state_copy(cyd_telemetry_state_t *out) {
    if (!out) return;
    if (s_state_mutex) xSemaphoreTake(s_state_mutex, portMAX_DELAY);
    *out = s_state;
    if (s_state_mutex) xSemaphoreGive(s_state_mutex);
}

esp_err_t cyd_client_send_raw(const char *json_line) {
    if (!json_line) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_tx_mutex == NULL) {
        s_tx_mutex = xSemaphoreCreateMutex();
        if (s_tx_mutex == NULL) {
            return ESP_ERR_INVALID_STATE;
        }
    }

    size_t len = strlen(json_line);
    if (len == 0) {
        return ESP_OK;
    }

    if (s_tx_spy) {
        s_tx_spy(json_line, len);
    }

    if (xSemaphoreTake(s_tx_mutex, pdMS_TO_TICKS(200)) == pdTRUE) {
        int written = uart_write_bytes(CYD_UART_NUM, json_line, len);
        xSemaphoreGive(s_tx_mutex);
        return (written == (int)len) ? ESP_OK : ESP_FAIL;
    }
    return ESP_ERR_TIMEOUT;
}

esp_err_t cyd_client_send_cmd(const char *cmd_name, int32_t val) {
    if (!cmd_name) {
        return ESP_ERR_INVALID_ARG;
    }
    char *formatted = cyd_client_format_command(cmd_name, val);
    if (!formatted) {
        return ESP_ERR_INVALID_ARG;
    }
    esp_err_t ret = cyd_client_send_raw(formatted);
    free(formatted);
    return ret;
}

void cyd_client_feed_rx_bytes(const char *buf, size_t len) {
    if (!buf || len == 0) return;

    for (size_t i = 0; i < len; i++) {
        char c = buf[i];
        if (c == '\r') {
            continue;
        }
        if (c == '\n') {
            if (s_rx_overflow) {
                s_rx_overflow = false;
                s_rx_idx = 0;
                continue;
            }
            if (s_rx_idx > 0) {
                s_rx_line[s_rx_idx] = '\0';
                bool parsed_ok = false;
                cyd_telemetry_state_t state_copy;
                if (s_state_mutex) xSemaphoreTake(s_state_mutex, portMAX_DELAY);
                if (cyd_client_parse_event(s_rx_line, &s_state) == ESP_OK) {
                    s_last_rx_time_us = esp_timer_get_time();
                    s_state.link_active = true;
                    parsed_ok = true;
                    state_copy = s_state;
                }
                if (s_state_mutex) xSemaphoreGive(s_state_mutex);

                if (parsed_ok) {
                    if (s_event_cb) {
                        s_event_cb(&state_copy);
                    }
                } else {
                    ESP_LOGW(TAG, "Malformed or unhandled CYD event: %s", s_rx_line);
                }
                s_rx_idx = 0;
            }
        } else {
            if (s_rx_overflow) {
                continue;
            }
            if (s_rx_idx < sizeof(s_rx_line) - 1) {
                s_rx_line[s_rx_idx++] = c;
            } else {
                // Overflow guard: drop corrupted line
                ESP_LOGE(TAG, "Line buffer overrun, dropping line");
                s_rx_overflow = true;
                s_rx_idx = 0;
            }
        }
    }
}

void cyd_client_poll_heartbeat(int64_t current_time_us) {
#if !defined(ESP_PLATFORM)
    s_mock_time_us = current_time_us;
#endif

    bool need_sync = false;
    bool notify_state = false;
    cyd_telemetry_state_t state_copy;

    if (s_state_mutex) xSemaphoreTake(s_state_mutex, portMAX_DELAY);
    if (s_state.link_active) {
        if ((current_time_us - s_last_rx_time_us) > CYD_HEARTBEAT_TIMEOUT_US) {
            s_state.link_active = false;
            s_last_sync_time_us = current_time_us;
            need_sync = true;
            notify_state = true;
            state_copy = s_state;
        }
    } else {
        // Link is inactive: retransmit sync command every 3s
        if (s_last_sync_time_us < 0 || (current_time_us - s_last_sync_time_us) >= CYD_RESYNC_INTERVAL_US) {
            s_last_sync_time_us = current_time_us;
            need_sync = true;
        }
    }
    if (s_state_mutex) xSemaphoreGive(s_state_mutex);

    if (need_sync) {
        cyd_client_send_cmd("sync", 0);
    }
    if (notify_state && s_event_cb) {
        s_event_cb(&state_copy);
    }
}

#if defined(ESP_PLATFORM)
static void cyd_client_rx_task(void *pvParameters) {
    (void)pvParameters;
    uint8_t data[128];
    while (1) {
        int len = uart_read_bytes(CYD_UART_NUM, data, sizeof(data), pdMS_TO_TICKS(10));
        if (len > 0) {
            cyd_client_feed_rx_bytes((const char *)data, len);
        }
        if (ulTaskNotifyTake(pdTRUE, 0) > 0) {
            cyd_client_poll_heartbeat(esp_timer_get_time());
        }
    }
}

static void cyd_heartbeat_timer_cb(void *arg) {
    (void)arg;
    if (s_rx_task_handle) {
        xTaskNotifyGive(s_rx_task_handle);
    }
}
#endif

esp_err_t cyd_client_uart_init(void) {
    if (s_initialized) {
        return ESP_OK;
    }

    if (s_tx_mutex == NULL) {
        s_tx_mutex = xSemaphoreCreateMutex();
        if (s_tx_mutex == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

    if (s_state_mutex == NULL) {
        s_state_mutex = xSemaphoreCreateMutex();
        if (s_state_mutex == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

#if defined(ESP_PLATFORM)
    uart_config_t uart_config = {
        .baud_rate = CYD_UART_BAUD,
        .data_bits = UART_DATA_8_BITS,
        .parity    = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
    };

    ESP_ERROR_CHECK(uart_param_config(CYD_UART_NUM, &uart_config));
    ESP_ERROR_CHECK(uart_set_pin(CYD_UART_NUM, CYD_UART_TX_PIN, CYD_UART_RX_PIN,
                                 UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));
    ESP_ERROR_CHECK(uart_driver_install(CYD_UART_NUM, CYD_UART_RX_BUF_SIZE, CYD_UART_TX_BUF_SIZE, 0, NULL, 0));

    xTaskCreate(cyd_client_rx_task, "cyd_uart_rx", 3072, NULL, ESP_TASK_PRIO_MIN + 2, &s_rx_task_handle);

    const esp_timer_create_args_t timer_args = {
        .callback = cyd_heartbeat_timer_cb,
        .name = "cyd_heartbeat"
    };
    esp_timer_handle_t timer;
    ESP_ERROR_CHECK(esp_timer_create(&timer_args, &timer));
    ESP_ERROR_CHECK(esp_timer_start_periodic(timer, 500000)); // 500ms
    ESP_LOGI(TAG, "CYD client UART initialized on port %d (TX:%d, RX:%d)",
             CYD_UART_NUM, CYD_UART_TX_PIN, CYD_UART_RX_PIN);
#endif

    s_initialized = true;
    return ESP_OK;
}
