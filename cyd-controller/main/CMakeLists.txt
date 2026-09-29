idf_component_register(
    SRCS "drivers/ili9341.c" "drivers/xpt2046.c" "comms/cyd_protocol.c" "comms/cyd_uart.c" "ui/lvgl_mock.c" "ui/ui_theme.c" "ui/ui_now_playing.c" "main.c"
    INCLUDE_DIRS "drivers" "comms" "ui" "."
    REQUIRES driver esp_timer cjson freertos
)
