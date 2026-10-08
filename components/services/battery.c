/*
   This example code is in the Public Domain (or CC0 licensed, at your option.)

   Unless required by applicable law or agreed to in writing, this
   software is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
   CONDITIONS OF ANY KIND, either express or implied.
*/

#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "esp_system.h"
#include "esp_log.h"
#include "esp_idf_version.h"

#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
#include "esp_adc/adc_oneshot.h"
static adc_oneshot_unit_handle_t s_adc1_handle = NULL;
#else
#include "driver/adc.h"
#endif

#include "esp_adc_cal.h"
#include "battery.h"
#include "platform_config.h"

/* 
 There is a bug in esp32 which causes a spurious interrupt on gpio 36/39 when
 using ADC, AMP and HALL sensor. Rather than making battery aware, we just ignore
 if as the interrupt lasts 80ns and should be debounced (and the ADC read does not
 happen very often)
*/ 

#define BATTERY_TIMER	(10*1000)
#define DEFAULT_VREF	1100

static const char *TAG = "battery";

static esp_adc_cal_characteristics_t s_adc_chars;
static bool s_calibrated = false;
static uint32_t s_v_full = 0;

static struct {
	int channel;
	float sum, avg, scale;
	int count;
	int cells, attenuation;
	TimerHandle_t timer;
} battery = { 
	.channel = -1,
	.cells = 2,
};	

void (*battery_handler_svc)(float value, int cells);

/****************************************************************************************
 * 
 */
float battery_value_svc(void) {
	return battery.avg;
 }
 
/****************************************************************************************
 * 
 */
uint8_t battery_level_svc(void) {
	if (battery.cells <= 0 || battery.avg <= 0.0f) return 0;
	float min_v = 3.0f * battery.cells;
	float max_v = 4.2f * battery.cells;
	if (battery.avg <= min_v) return 0;
	if (battery.avg >= max_v) return 100;
	int level = (int)(((battery.avg - min_v) / (max_v - min_v)) * 100.0f);
	return (uint8_t)(level > 100 ? 100 : (level < 0 ? 0 : level));
}

/****************************************************************************************
 * 
 */
static int get_adc_raw(int channel) {
#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
    int raw = 0;
    if (s_adc1_handle) {
        adc_oneshot_read(s_adc1_handle, (adc_channel_t)channel, &raw);
    }
    return raw;
#else
    return adc1_get_raw(channel);
#endif
}

static float read_battery_voltage(void) {
	int raw = get_adc_raw(battery.channel);
	if (raw < 0) {
		return 0.0f;
	}
	if (s_calibrated && s_v_full > 0) {
		uint32_t voltage_mv = esp_adc_cal_raw_to_voltage(raw, &s_adc_chars);
		if (battery.scale > 0.0f) {
			return (float)voltage_mv * battery.scale / (float)s_v_full;
		} else {
			return (float)voltage_mv / 1000.0f;
		}
	}
	return (float)raw * battery.scale / 4095.0f;
}

/****************************************************************************************
 * 
 */
static void battery_callback(TimerHandle_t xTimer) {
	battery.sum += read_battery_voltage();
	if (++battery.count == 30) {
		battery.avg = battery.sum / battery.count;
		battery.sum = battery.count = 0;
		if (battery_handler_svc) (battery_handler_svc)(battery.avg, battery.cells);
		ESP_LOGI(TAG, "Voltage %.2fV", battery.avg);
	}	
}

/****************************************************************************************
 * 
 */
void battery_svc_init(void) {
	char *nvs_item = config_alloc_get_default(NVS_TYPE_STR, "bat_config", "", 0);
	
#ifdef CONFIG_BAT_LOCKED
	char *p = nvs_item;
	asprintf(&nvs_item, CONFIG_BAT_CONFIG ",%s", p);
	free(p);
#endif		

	if (nvs_item) {
		PARSE_PARAM(nvs_item, "channel", '=', battery.channel);
		PARSE_PARAM_FLOAT(nvs_item, "scale", '=', battery.scale);
		PARSE_PARAM(nvs_item, "atten", '=', battery.attenuation);
		PARSE_PARAM(nvs_item, "cells", '=', battery.cells);
		free(nvs_item);
	}	

	if (battery.channel != -1) {
#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
		adc_oneshot_unit_init_cfg_t init_config = {
			.unit_id = ADC_UNIT_1,
		};
		adc_oneshot_new_unit(&init_config, &s_adc1_handle);
		adc_oneshot_chan_cfg_t config = {
			.bitwidth = ADC_BITWIDTH_12,
			.atten = (adc_atten_t)battery.attenuation,
		};
		adc_oneshot_config_channel(s_adc1_handle, (adc_channel_t)battery.channel, &config);
#else
		adc1_config_width(ADC_WIDTH_BIT_12);
		adc1_config_channel_atten(battery.channel, battery.attenuation);
#endif

		if (esp_adc_cal_check_efuse(ESP_ADC_CAL_VAL_EFUSE_TP) == ESP_OK) {
			ESP_LOGI(TAG, "ADC calibration: eFuse Two Point supported");
		} else if (esp_adc_cal_check_efuse(ESP_ADC_CAL_VAL_EFUSE_VREF) == ESP_OK) {
			ESP_LOGI(TAG, "ADC calibration: eFuse Vref supported");
		} else {
			ESP_LOGI(TAG, "ADC calibration: Default Vref used");
		}
		esp_adc_cal_characterize(ADC_UNIT_1, (adc_atten_t)battery.attenuation, ADC_WIDTH_BIT_12, DEFAULT_VREF, &s_adc_chars);
		s_calibrated = true;
		s_v_full = esp_adc_cal_raw_to_voltage(4095, &s_adc_chars);

		battery.avg = read_battery_voltage();    
		battery.timer = xTimerCreate("battery", pdMS_TO_TICKS(BATTERY_TIMER), pdTRUE, NULL, battery_callback);
		xTimerStart(battery.timer, portMAX_DELAY);
		
		ESP_LOGI(TAG, "Battery measure channel: %u, scale %f, atten %d, cells %u, avg %.2fV", battery.channel, battery.scale, battery.attenuation, battery.cells, battery.avg);		
	} else {
		ESP_LOGI(TAG, "No battery");
	}	
}
