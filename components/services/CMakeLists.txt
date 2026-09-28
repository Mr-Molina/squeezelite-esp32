if(IDF_VERSION_MAJOR GREATER_EQUAL 5)
    set(extra_services_requires esp_adc)
endif()

idf_component_register(SRC_DIRS .
						INCLUDE_DIRS .
						REQUIRES json tools platform_config display wifi-manager driver freertos ${extra_services_requires}
						PRIV_REQUIRES soc esp32
)
