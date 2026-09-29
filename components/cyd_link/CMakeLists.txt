idf_component_register(
    SRCS "cyd_link.c" "cyd_link_dispatch.c" "cyd_link_hooks.c"
    INCLUDE_DIRS "."
    REQUIRES json driver freertos services
)
