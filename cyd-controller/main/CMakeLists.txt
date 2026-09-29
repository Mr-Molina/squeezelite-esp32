idf_component_register(
    SRCS "drivers/ili9341.c" "drivers/xpt2046.c" "comms/cyd_protocol.c" "comms/cyd_uart.c"
    INCLUDE_DIRS "drivers" "comms" "."
    REQUIRES driver esp_timer cjson freertos
)
