idf_component_register(
    SRCS "drivers/ili9341.c" "drivers/xpt2046.c"
    INCLUDE_DIRS "drivers" "."
    REQUIRES driver esp_timer
)
