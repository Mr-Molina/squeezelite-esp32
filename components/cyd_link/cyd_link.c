#include "cyd_link.h"
#include <string.h>
#include <stdio.h>

#if defined(ESP_PLATFORM)
#include "driver/uart.h"
#include "esp_log.h"
#include "esp_task.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#else
// Host test stubs
#include <stdint.h>
#include <stdbool.h>

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
#define UART_NUM_1 1
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
#endif

#if defined(ESP_PLATFORM)
#include "platform_config.h"
#endif

static const char *TAG __attribute__((unused)) = "cyd_link";
#define CYD_LINE_BUF_SIZE 1024
#define CYD_UART_BUF_SIZE 1024
#define CYD_UART_RX_BUF_SIZE (CYD_UART_BUF_SIZE * 2)
#define CYD_UART_TX_BUF_SIZE 1024

static uart_port_t s_uart_num = UART_NUM_1;
static SemaphoreHandle_t s_tx_mutex = NULL;
static cyd_link_cmd_cb_t s_cmd_handler = NULL;
static cyd_link_tx_cb_t s_tx_spy = NULL;
static char s_rx_line[CYD_LINE_BUF_SIZE];
static size_t s_rx_idx = 0;
static bool s_rx_overflow = false;
static bool s_initialized = false;
static bool s_uart_active = false;

void cyd_link_set_cmd_handler(cyd_link_cmd_cb_t handler) {
    s_cmd_handler = handler;
}

void cyd_link_set_tx_spy(cyd_link_tx_cb_t spy) {
    s_tx_spy = spy;
}

void cyd_link_execute_command(const cyd_command_t *cmd) {
    if (cmd && s_cmd_handler) {
        s_cmd_handler(cmd->type, cmd->param);
    }
}

void cyd_link_feed_rx_bytes(const char *buf, size_t len) {
    if (!buf || len == 0) return;
    for (size_t i = 0; i < len; i++) {
        char c = buf[i];
        if (c == '\r') continue;
        if (c == '\n') {
            if (s_rx_overflow) {
                s_rx_overflow = false;
                s_rx_idx = 0;
                continue;
            }
            if (s_rx_idx > 0) {
                s_rx_line[s_rx_idx] = '\0';
                cyd_command_t cmd;
                if (cyd_link_parse_command(s_rx_line, &cmd) == ESP_OK) {
                    cyd_link_execute_command(&cmd);
                } else {
                    ESP_LOGW(TAG, "Malformed CYD command: %s", s_rx_line);
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
                // Overflow guard: drop corrupted line and latch discard until next \n
                ESP_LOGE(TAG, "Line buffer overrun, dropping line");
                s_rx_overflow = true;
                s_rx_idx = 0;
            }
        }
    }
}

#if defined(ESP_PLATFORM)
static void cyd_uart_rx_task(void *pvParameters) {
    (void)pvParameters;
    uint8_t data[128];
    while (1) {
        int len = uart_read_bytes(s_uart_num, data, sizeof(data), pdMS_TO_TICKS(10));
        if (len > 0) {
            cyd_link_feed_rx_bytes((const char *)data, len);
        } else if (len < 0) {
            ESP_LOGW(TAG, "UART read error, flushing input buffer");
            uart_flush_input(s_uart_num);
            vTaskDelay(pdMS_TO_TICKS(100));
        }
    }
}
#endif

esp_err_t cyd_link_send_raw(const char *json_line) {
    if (!json_line || !s_tx_mutex) return ESP_ERR_INVALID_STATE;
    size_t len = strlen(json_line);
    if (len == 0) return ESP_OK;

    if (s_tx_spy) {
        s_tx_spy(json_line, len);
    }

#if defined(ESP_PLATFORM)
    if (!s_uart_active) {
        return ESP_OK;
    }
#endif

    if (xSemaphoreTake(s_tx_mutex, pdMS_TO_TICKS(200)) == pdTRUE) {
        int written = uart_write_bytes(s_uart_num, json_line, len);
        xSemaphoreGive(s_tx_mutex);
        return (written == (int)len) ? ESP_OK : ESP_FAIL;
    }
    return ESP_ERR_TIMEOUT;
}

esp_err_t cyd_link_init(void) {
    if (s_initialized) {
        return ESP_OK;
    }

    if (s_tx_mutex == NULL) {
        s_tx_mutex = xSemaphoreCreateMutex();
        if (s_tx_mutex == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }
    s_initialized = true;

#if defined(ESP_PLATFORM)
    char *cyd_cfg = config_alloc_get_str("cyd_config", NULL, NULL);
    if (!cyd_cfg || strlen(cyd_cfg) == 0) {
        FREE_RESET(cyd_cfg);
        ESP_LOGI(TAG, "CYD link UART disabled (cyd_config not configured in NVS)");
        return ESP_OK;
    }

    int uart_port = 1;
    int tx_pin = -1, rx_pin = -1;
    int baud = 460800;

    // Parse cyd_config: e.g. "uart=1,tx=17,rx=16,baud=460800"
    char *p = cyd_cfg;
    while (p && *p) {
        if (strncmp(p, "uart=", 5) == 0) {
            uart_port = atoi(p + 5);
        } else if (strncmp(p, "tx=", 3) == 0) {
            tx_pin = atoi(p + 3);
        } else if (strncmp(p, "rx=", 3) == 0) {
            rx_pin = atoi(p + 3);
        } else if (strncmp(p, "baud=", 5) == 0) {
            baud = atoi(p + 5);
        }
        p = strchr(p, ',');
        if (p) p++;
    }
    FREE_RESET(cyd_cfg);

    if (tx_pin < 0 || rx_pin < 0) {
        ESP_LOGW(TAG, "CYD link disabled: tx or rx pin missing in cyd_config");
        return ESP_OK;
    }
    if (baud <= 0 || tx_pin > 39 || rx_pin > 39) {
        ESP_LOGE(TAG, "CYD link invalid configuration: tx=%d rx=%d baud=%d", tx_pin, rx_pin, baud);
        return ESP_ERR_INVALID_ARG;
    }

    s_uart_num = (uart_port == 2) ? UART_NUM_2 : UART_NUM_1;

    uart_config_t uart_config = {
        .baud_rate = baud,
        .data_bits = UART_DATA_8_BITS,
        .parity    = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
    };

    esp_err_t err = uart_param_config(s_uart_num, &uart_config);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Failed to configure UART params: %d", err);
        return err;
    }
    err = uart_set_pin(s_uart_num, tx_pin, rx_pin, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set UART pins: %d", err);
        return err;
    }
    err = uart_driver_install(s_uart_num, CYD_UART_RX_BUF_SIZE, CYD_UART_TX_BUF_SIZE, 0, NULL, 0);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Failed to install UART driver: %d", err);
        return err;
    }

    if (xTaskCreate(cyd_uart_rx_task, "cyd_uart_rx", 4096, NULL, ESP_TASK_PRIO_MIN + 2, NULL) != pdPASS) {
        ESP_LOGE(TAG, "Failed to create cyd_uart_rx task");
        return ESP_ERR_NO_MEM;
    }
    s_uart_active = true;
    ESP_LOGI(TAG, "CYD link UART initialized on port %d (TX:%d, RX:%d, baud:%d)", s_uart_num, tx_pin, rx_pin, baud);
#else
    s_uart_active = true;
#endif
    return ESP_OK;
}

