idf_component_register(SRCS "unit_tests.c" "test_cyd_integration.c"
                            "../../cyd-controller/main/comms/cyd_protocol.c"
                            "../../cyd-controller/main/comms/cyd_uart.c"
                            "../../cyd-controller/main/ui/ui_theme.c"
                            "../../cyd-controller/main/ui/ui_now_playing.c"
                            "../../cyd-controller/main/ui/lvgl_mock.c"
                    INCLUDE_DIRS "."
                                 "../../components/cyd_link"
                                 "../../components/services"
                                 "../../cyd-controller/main/comms"
                                 "../../cyd-controller/main/ui"
                    REQUIRES cyd_link services)
