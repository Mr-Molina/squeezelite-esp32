idf_component_register(SRCS "unit_tests.c" "test_cyd_integration.c"
                    INCLUDE_DIRS "."
                                 "../../components/cyd_link"
                                 "../../components/services"
                                 "../../cyd-controller/main/comms"
                                 "../../cyd-controller/main/ui"
                    REQUIRES cyd_link services)