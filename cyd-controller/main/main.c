#include <stdio.h>
#include <string.h>
#include <stdbool.h>

#include "ili9341.h"
#include "xpt2046.h"
#include "cyd_uart.h"
#include "cyd_protocol.h"
#include "ui_theme.h"
#include "ui_now_playing.h"
#include "lvgl.h"

#if defined(ESP_PLATFORM)
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_timer.h"

static const char *TAG = "cyd_main";

#define DISP_BUF_SIZE (CYD_TFT_WIDTH * 20)
static lv_color_t s_buf1[DISP_BUF_SIZE];
static lv_disp_draw_buf_t s_disp_buf;
static lv_disp_drv_t s_disp_drv;
static lv_indev_drv_t s_indev_drv;

static void disp_flush_cb(lv_disp_drv_t *drv, const lv_area_t *area, lv_color_t *color_p) {
    ili9341_flush(area->x1, area->y1, area->x2, area->y2, (const uint16_t *)color_p);
    lv_disp_flush_ready(drv);
}

static void touch_read_cb(lv_indev_drv_t *drv, lv_indev_data_t *data) {
    (void)drv;
    int16_t x = 0, y = 0;
    bool touched = xpt2046_read(&x, &y);
    if (touched) {
        data->point.x = x;
        data->point.y = y;
        data->state = LV_INDEV_STATE_PRESSED;
    } else {
        data->state = LV_INDEV_STATE_RELEASED;
    }
}

static void gui_task(void *pvParameters) {
    (void)pvParameters;
    ESP_LOGI(TAG, "CYD GUI Task running");
    int64_t last_tick_us = esp_timer_get_time();
    while (1) {
        cyd_ui_lock();
        uint32_t sleep_ms = lv_timer_handler();
        cyd_ui_unlock();

        if (sleep_ms == 0) {
            taskYIELD();
        } else {
            // Dynamic sleep when idle, capped to ensure touch and telemetry responsiveness
            if (sleep_ms > 50) {
                sleep_ms = 50;
            }
            TickType_t delay_ticks = pdMS_TO_TICKS(sleep_ms);
            vTaskDelay(delay_ticks > 0 ? delay_ticks : 1);
        }

        int64_t now_us = esp_timer_get_time();
        uint32_t elapsed_ms = (uint32_t)((now_us - last_tick_us) / 1000LL);
        if (elapsed_ms > 0) {
            lv_tick_inc(elapsed_ms);
            last_tick_us += (int64_t)elapsed_ms * 1000LL;
        }
    }
}

void app_main(void) {
    ESP_LOGI(TAG, "Starting Cheap Yellow Display Controller...");

    // 1. Initialize Display and Backlight PWM
    ESP_ERROR_CHECK(ili9341_init());
    ili9341_set_backlight(100);

    // 2. Initialize XPT2046 Touch Controller
    ESP_ERROR_CHECK(xpt2046_init());

    // 3. Initialize LVGL Graphic Library and Drivers
    lv_init();
    lv_disp_draw_buf_init(&s_disp_buf, s_buf1, NULL, DISP_BUF_SIZE);

    lv_disp_drv_init(&s_disp_drv);
    s_disp_drv.hor_res = CYD_TFT_WIDTH;
    s_disp_drv.ver_res = CYD_TFT_HEIGHT;
    s_disp_drv.flush_cb = disp_flush_cb;
    s_disp_drv.draw_buf = &s_disp_buf;
    lv_disp_drv_register(&s_disp_drv);

    lv_indev_drv_init(&s_indev_drv);
    s_indev_drv.type = LV_INDEV_TYPE_POINTER;
    s_indev_drv.read_cb = touch_read_cb;
    lv_indev_drv_register(&s_indev_drv);

    // 4. Initialize UI Theme and Now Playing screen
    ui_theme_init();
    ui_now_playing_create();

    // 5. Initialize UART Link and Register Telemetry Callback
    cyd_client_register_event_callback(ui_now_playing_update);
    ESP_ERROR_CHECK(cyd_client_uart_init());

    // 6. Request initial state synchronization
    cyd_client_send_cmd("sync", 0);

    // 7. Launch GUI event loop task
    xTaskCreatePinnedToCore(gui_task, "gui_task", 4096, NULL, 5, NULL, 1);
}
#else
// Host / testing entry point
int cyd_main(void) {
    ui_theme_init();
    ui_now_playing_create();
    cyd_client_register_event_callback(ui_now_playing_update);
    cyd_client_send_cmd("sync", 0);
    return 0;
}
#endif
