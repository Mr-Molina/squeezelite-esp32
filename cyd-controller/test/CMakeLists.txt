idf_component_register(
    SRCS "test_cyd_drivers.c" "test_cyd_comms.c" "test_cyd_ui.c"
    INCLUDE_DIRS "."
    REQUIRES unity driver cjson main
)
