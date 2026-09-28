idf_component_register(SRC_DIRS . 
					PRIV_REQUIRES _override esp_common wifi-manager pthread squeezelite-ota platform_console telnet display targets led_strip nvs_flash mdns esp_netif freertos driver
					LDFRAGMENTS "linker.lf"                                            
                    	)
