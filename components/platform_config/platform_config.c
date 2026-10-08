/*
 *  Squeezelite for esp32
 *
 *  (c) Sebastien 2019
 *      Philippe G. 2019, philippe_44@outlook.com
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 */
#include "platform_config.h"
#include "nvs_utilities.h"
#include "platform_esp32.h"
#include <stdio.h>
#include <string.h>
#include <errno.h>
#include "esp_system.h"
#include "esp_log.h"
#include "esp_console.h"
#include "esp_vfs_dev.h"
#include "driver/uart.h"
#include "linenoise/linenoise.h"
#include "argtable3/argtable3.h"

#include "nvs.h"
#include "nvs_flash.h"
#include "nvs_utilities.h"
#include "cJSON.h"
#include "freertos/FreeRTOS.h"
#include "freertos/timers.h"
#include "freertos/event_groups.h"
#include "tools.h"
#include "trace.h"

#define CONFIG_COMMIT_DELAY 1000
#define LOCK_MAX_WAIT 20*CONFIG_COMMIT_DELAY
static const char * TAG = "config";
EXT_RAM_ATTR static cJSON * nvs_json=NULL;
EXT_RAM_ATTR static TimerHandle_t timer;
EXT_RAM_ATTR static SemaphoreHandle_t config_mutex = NULL;
EXT_RAM_ATTR static SemaphoreHandle_t s_commit_mutex = NULL;
EXT_RAM_ATTR static EventGroupHandle_t config_group;
/* @brief indicate that the ESP32 is currently connected. */
EXT_RAM_ATTR static const int CONFIG_NO_COMMIT_PENDING = BIT0;
EXT_RAM_ATTR static const int CONFIG_LOAD_BIT = BIT1;

bool config_lock(TickType_t xTicksToWait);
void config_unlock();
extern esp_err_t nvs_load_config();
void config_raise_change(bool flag);
cJSON_bool config_is_entry_changed(cJSON * entry);
bool config_set_group_bit(int bit_num,bool flag);
cJSON * config_set_value_safe(nvs_type_t nvs_type, const char *key,const void * value);
static void vCallbackFunction( TimerHandle_t xTimer );
void config_set_entry_changed_flag(cJSON * entry, cJSON_bool flag);
static esp_err_t config_get_numeric_value(nvs_type_t nvs_type, const char *key, void *value);

#define IMPLEMENT_SET_DEFAULT(t,nt) void config_set_default_## t (const char *key, t  value){\
	t val = value;\
	config_set_default(nt, key, &val, 0); }
#define IMPLEMENT_GET_NUM(t,nt) esp_err_t config_get_## t (const char *key, t *  value){\
		if (value == NULL) return ESP_ERR_INVALID_ARG;\
		return config_get_numeric_value(nt, key, value);\
}
static void * malloc_fn(size_t sz){

	void * ptr = is_recovery_running?malloc(sz):heap_caps_malloc(sz, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
	if(ptr==NULL){
		ESP_LOGE(TAG,"malloc_fn:  unable to allocate memory!");
	}
	return ptr;
}
void init_cJSON(){
	static cJSON_Hooks hooks;
	hooks.malloc_fn=&malloc_fn;
	cJSON_InitHooks(&hooks);
}
void config_init(){
	ESP_LOGD(TAG, "Creating mutex for Config");
	MEMTRACE_PRINT_DELTA();
	config_mutex = xSemaphoreCreateRecursiveMutex();
	s_commit_mutex = xSemaphoreCreateMutex();
	MEMTRACE_PRINT_DELTA();
	ESP_LOGD(TAG, "Creating event group");
	MEMTRACE_PRINT_DELTA();
	config_group = xEventGroupCreate();
	MEMTRACE_PRINT_DELTA();
	ESP_LOGD(TAG, "Loading config from nvs");

	init_cJSON();
	MEMTRACE_PRINT_DELTA();
	config_lock(portMAX_DELAY);
	if(nvs_json !=NULL){
		cJSON_Delete(nvs_json);
	}
	nvs_json = cJSON_CreateObject();

	config_set_group_bit(CONFIG_LOAD_BIT,true);
	MEMTRACE_PRINT_DELTA();
	nvs_load_config();
	MEMTRACE_PRINT_DELTA();
	config_set_group_bit(CONFIG_LOAD_BIT,false);
	config_unlock();
	MEMTRACE_PRINT_DELTA();
	esp_register_shutdown_handler(config_flush_now);
	config_start_timer();
}

void config_start_timer(){
	ESP_LOGD(TAG, "Starting config timer");
	timer = xTimerCreate("configTimer", pdMS_TO_TICKS(CONFIG_COMMIT_DELAY), pdFALSE, NULL, vCallbackFunction);
    if( xTimerStart( timer , pdMS_TO_TICKS(CONFIG_COMMIT_DELAY) ) != pdPASS )    {
        ESP_LOGE(TAG, "config commitment timer failed to start.");
    }

}

nvs_type_t  config_get_item_type(cJSON * entry){
	if(entry==NULL){
		ESP_LOGE(TAG,"null pointer received!");
		return 0;
	}
	cJSON * item_type = cJSON_GetObjectItemCaseSensitive(entry, "type");
	if(item_type ==NULL ) {
		ESP_LOGE(TAG, "Item type not found! ");
		return 0;
	}
	ESP_LOGD(TAG,"Found item type %f",item_type->valuedouble);
	return item_type->valuedouble;
}


cJSON * config_set_value_safe(nvs_type_t nvs_type, const char *key,  const void * value){
	cJSON * entry = cJSON_CreateObject();

	double numvalue = 0;
	if(entry == NULL) {
		ESP_LOGE(TAG, "Unable to allocate memory for entry %s",key);
		return NULL;
	}

cJSON * existing = cJSON_GetObjectItemCaseSensitive(nvs_json, key);
	if(existing !=NULL && nvs_type == NVS_TYPE_STR && config_get_item_type(existing) != NVS_TYPE_STR  ) {
		ESP_LOGW(TAG, "Storing numeric value from string");
		numvalue = atof((char *)value);
		cJSON_AddNumberToObject(entry,"value", numvalue	);
		nvs_type_t exist_type = config_get_item_type(existing);
		ESP_LOGW(TAG, "Stored  value %f from string %s as type %d",numvalue, (char *)value,exist_type);
		cJSON_AddNumberToObject(entry,"type", exist_type);
	}
	else {
		cJSON_AddNumberToObject(entry,"type", nvs_type	);
		switch (nvs_type) {
			case NVS_TYPE_I8:
				cJSON_AddNumberToObject(entry,"value", *(int8_t*)value	);
				break;
			case NVS_TYPE_I16:
				cJSON_AddNumberToObject(entry,"value", *(int16_t*)value	);
				break;
			case NVS_TYPE_I32:
				cJSON_AddNumberToObject(entry,"value", *(int32_t*)value	);
				break;
			case NVS_TYPE_U8:
				cJSON_AddNumberToObject(entry,"value", *(uint8_t*)value	);
				break;
			case NVS_TYPE_U16:
				cJSON_AddNumberToObject(entry,"value", *(uint16_t*)value	);
				break;
			case NVS_TYPE_U32:
				cJSON_AddNumberToObject(entry,"value", *(uint32_t*)value	);
				break;
			case NVS_TYPE_STR:
				cJSON_AddStringToObject(entry, "value", (char *)value);
				break;
			case NVS_TYPE_I64:
			case NVS_TYPE_U64:
			default:
				ESP_LOGE(TAG, "nvs type %u not supported", nvs_type);
				break;
		}
	}
	if(existing!=NULL ) {
		ESP_LOGV(TAG, "Changing existing entry [%s].", key);
		if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
			char * exist_str = cJSON_PrintUnformatted(existing);
			if(exist_str!=NULL){
				ESP_LOGV(TAG,"Existing entry: %s", exist_str);
				free(exist_str);
			}
			else {
				ESP_LOGV(TAG,"Failed to print existing entry");
			}
		}
		// set commit flag as equal so we can compare
		cJSON_AddBoolToObject(entry,"chg",config_is_entry_changed(existing));
		if(!cJSON_Compare(entry,existing,false)){
			if (LOG_LOCAL_LEVEL >= ESP_LOG_DEBUG) {
				char * entry_str = cJSON_PrintUnformatted(entry);
				if(entry_str!=NULL){
					ESP_LOGD(TAG,"New config object: \n%s", entry_str );
					free(entry_str);
				}
				else {
					ESP_LOGD(TAG,"Failed to print entry");
				}
			}
			ESP_LOGI(TAG, "Setting changed flag config [%s]", key);
			config_set_entry_changed_flag(entry,true);
			ESP_LOGI(TAG, "Updating config [%s]", key);
			cJSON_ReplaceItemInObject(nvs_json,key, entry);
			if (LOG_LOCAL_LEVEL >= ESP_LOG_DEBUG) {
				char * entry_str = cJSON_PrintUnformatted(entry);
				if(entry_str!=NULL){
					ESP_LOGD(TAG,"New config: %s", entry_str );
					free(entry_str);
				}
				else {
					ESP_LOGD(TAG,"Failed to print entry");
				}
			}
		}
		else {
			ESP_LOGD(TAG, "Config not changed. ");
			cJSON_Delete(entry);
			entry = existing;
		}
	}
	else {
		// This is a new entry.
		config_set_entry_changed_flag(entry,true);
		cJSON_AddItemToObject(nvs_json, key, entry);
	}

	return entry;
}

nvs_type_t config_get_entry_type(cJSON * entry){
	if(entry==NULL){
		ESP_LOGE(TAG,"null pointer received!");
		return 0;
	}
	cJSON * entry_type = cJSON_GetObjectItemCaseSensitive(entry, "type");
	if(entry_type ==NULL ) {
		ESP_LOGE(TAG, "Entry type not found in nvs cache for existing setting.");
		return 0;
	}
	ESP_LOGV(TAG,"Found type %s",type_to_str(entry_type->valuedouble));
	return entry_type->valuedouble;
}
void config_set_entry_changed_flag(cJSON * entry, cJSON_bool flag){
	ESP_LOGV(TAG, "config_set_entry_changed_flag: begin");
	if(entry==NULL){
		ESP_LOGE(TAG,"null pointer received!");
		return;
	}
	bool bIsConfigLoading=((xEventGroupGetBits(config_group) & CONFIG_LOAD_BIT)!=0);
	bool changedFlag=bIsConfigLoading?false:flag;
	ESP_LOGV(TAG, "config_set_entry_changed_flag: retrieving chg flag from entry");
	cJSON * changed = cJSON_GetObjectItemCaseSensitive(entry, "chg");
	if(changed ==NULL ) {
		ESP_LOGV(TAG, "config_set_entry_changed_flag: chg flag not found. Adding. ");
		cJSON_AddBoolToObject(entry,"chg",changedFlag);
	}
	else {
		ESP_LOGV(TAG, "config_set_entry_changed_flag: Existing change flag found. ");
		if(cJSON_IsTrue(changed) && changedFlag){
			ESP_LOGW(TAG, "Commit flag not changed!");
		}
		else{
			ESP_LOGV(TAG, "config_set_entry_changed_flag: Updating change flag to %s",changedFlag?"TRUE":"FALSE");
			changed->type = changedFlag?cJSON_True:cJSON_False ;
		}
	}

	if(changedFlag) {
		ESP_LOGV(TAG, "config_set_entry_changed_flag: Calling config_raise_change. ");
		config_raise_change(true);
	}
	ESP_LOGV(TAG, "config_set_entry_changed_flag: done. ");
}
cJSON_bool config_is_entry_changed(cJSON * entry){
	if(entry==NULL){
		ESP_LOGE(TAG,"null pointer received!");
		return true;
	}
	cJSON * changed = cJSON_GetObjectItemCaseSensitive(entry, "chg");
	if(changed ==NULL ) {
		ESP_LOGE(TAG, "Change flag not found! ");
		return true;
	}
	return cJSON_IsTrue(changed);
}




static esp_err_t config_extract_entry_value_direct(cJSON *entry, nvs_type_t nvs_type, void *out_val) {
	if (entry == NULL || out_val == NULL) {
		return ESP_ERR_INVALID_ARG;
	}
	cJSON *entry_value = cJSON_GetObjectItemCaseSensitive(entry, "value");
	if (entry_value == NULL) {
		return ESP_ERR_NOT_FOUND;
	}
	nvs_type_t type = config_get_entry_type(entry);
	if (nvs_type != type) {
		return ESP_ERR_INVALID_ARG;
	}
	switch (nvs_type) {
		case NVS_TYPE_I8:
			*(int8_t *)out_val = (int8_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_U8:
			*(uint8_t *)out_val = (uint8_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_I16:
			*(int16_t *)out_val = (int16_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_U16:
			*(uint16_t *)out_val = (uint16_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_I32:
			*(int32_t *)out_val = (int32_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_U32:
			*(uint32_t *)out_val = (uint32_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_I64:
			*(int64_t *)out_val = (int64_t)entry_value->valuedouble;
			return ESP_OK;
		case NVS_TYPE_U64:
			*(uint64_t *)out_val = (uint64_t)entry_value->valuedouble;
			return ESP_OK;
		default:
			return ESP_ERR_NOT_SUPPORTED;
	}
}

static esp_err_t config_get_numeric_value(nvs_type_t nvs_type, const char *key, void *value) {
	if (key == NULL || value == NULL) {
		return ESP_ERR_INVALID_ARG;
	}
	if (nvs_json == NULL) {
		ESP_LOGE(TAG, "configuration not loaded!");
		return ESP_FAIL;
	}
	if (!config_lock(LOCK_MAX_WAIT / portTICK_PERIOD_MS)) {
		ESP_LOGE(TAG, "Unable to lock config");
		return ESP_ERR_TIMEOUT;
	}
	cJSON *entry = cJSON_GetObjectItemCaseSensitive(nvs_json, key);
	if (entry == NULL) {
		config_unlock();
		return ESP_ERR_NOT_FOUND;
	}
	esp_err_t err = config_extract_entry_value_direct(entry, nvs_type, value);
	config_unlock();
	return err;
}

esp_err_t config_get_value_int8(const char *key, int8_t *value) {
	return config_get_numeric_value(NVS_TYPE_I8, key, value);
}

esp_err_t config_get_value_uint8(const char *key, uint8_t *value) {
	return config_get_numeric_value(NVS_TYPE_U8, key, value);
}

esp_err_t config_get_value_int16(const char *key, int16_t *value) {
	return config_get_numeric_value(NVS_TYPE_I16, key, value);
}

esp_err_t config_get_value_uint16(const char *key, uint16_t *value) {
	return config_get_numeric_value(NVS_TYPE_U16, key, value);
}

esp_err_t config_get_value_int32(const char *key, int32_t *value) {
	return config_get_numeric_value(NVS_TYPE_I32, key, value);
}

esp_err_t config_get_value_uint32(const char *key, uint32_t *value) {
	return config_get_numeric_value(NVS_TYPE_U32, key, value);
}

esp_err_t config_get_value_direct(nvs_type_t nvs_type, const char *key, void *value) {
	return config_get_numeric_value(nvs_type, key, value);
}

void * config_safe_alloc_get_entry_value(nvs_type_t nvs_type, cJSON * entry){
	void * value=NULL;
	if(entry==NULL){
		ESP_LOGE(TAG,"null pointer received!");
		return NULL;
	}
	ESP_LOGV(TAG, "getting config value type %s", type_to_str(nvs_type));
	cJSON * entry_value = cJSON_GetObjectItemCaseSensitive(entry, "value");
	if(entry_value==NULL ) {
		if (LOG_LOCAL_LEVEL >= ESP_LOG_ERROR) {
			char * entry_str = cJSON_PrintUnformatted(entry);
			if(entry_str!=NULL){
				ESP_LOGE(TAG, "Missing config value!. Object: \n%s", entry_str);
				free(entry_str);
			}
			else{
				ESP_LOGE(TAG, "Missing config value");
			}
		}
		return NULL;
	}

	nvs_type_t type = config_get_entry_type(entry);
	if(nvs_type != type){
		// requested value type different than the stored type
		if (LOG_LOCAL_LEVEL >= ESP_LOG_ERROR) {
			char * entry_str = cJSON_PrintUnformatted(entry);
			if(entry_str!=NULL){
				ESP_LOGE(TAG, "Requested value type %s, found value type %s instead, Object: \n%s", type_to_str(nvs_type), type_to_str(type),entry_str);
				free(entry_str);
			}
			else{
				ESP_LOGE(TAG, "Requested value type %s, found value type %s instead", type_to_str(nvs_type), type_to_str(type));
			}
		}

		return NULL;
	}

	size_t sz = 0;
	switch (nvs_type) {
		case NVS_TYPE_I8:
		case NVS_TYPE_U8:
			sz = sizeof(uint8_t);
			break;
		case NVS_TYPE_I16:
		case NVS_TYPE_U16:
			sz = sizeof(uint16_t);
			break;
		case NVS_TYPE_I32:
		case NVS_TYPE_U32:
			sz = sizeof(uint32_t);
			break;
		case NVS_TYPE_I64:
		case NVS_TYPE_U64:
			sz = sizeof(uint64_t);
			break;
		case NVS_TYPE_STR:
			if(!cJSON_IsString(entry_value)){
				if (LOG_LOCAL_LEVEL >= ESP_LOG_ERROR) {
					char * entry_str = cJSON_PrintUnformatted(entry);
					if(entry_str!=NULL){
						ESP_LOGE(TAG, "requested value type string, config type is different. key: %s, value: %s, type %d, Object: \n%s",
								str_or_null(entry_value->string),
								str_or_null(entry_value->valuestring),
								entry_value->type,
								str_or_null(entry_str));
						free(entry_str);
					}
					else {
						ESP_LOGE(TAG, "requested value type string, config type is different. key: %s, value: %s, type %d",
								str_or_null(entry_value->string),
								str_or_null(entry_value->valuestring),
								entry_value->type);
					}
				}
			}
			else {
				const char *str_val = cJSON_GetStringValue(entry_value);
				if(str_val != NULL){
					size_t len = strlen(str_val);
					value = (void *)malloc_init_external(len + 1);
					if(value != NULL){
						memcpy(value, str_val, len);
						((char *)value)[len] = '\0';
					}
					else {
						if (LOG_LOCAL_LEVEL >= ESP_LOG_ERROR) {
							char * entry_str = cJSON_PrintUnformatted(entry);
							if(entry_str != NULL){
								ESP_LOGE(TAG, "strdup failed on value for object \n%s", entry_str);
								free(entry_str);
							}
							else {
								ESP_LOGE(TAG, "strdup failed on value");
							}
						}
					}
				}
			}
			return value;
		case NVS_TYPE_BLOB:
		default:
			ESP_LOGE(TAG, "Unsupported type %d", nvs_type);
			return NULL;
	}

	if (sz > 0) {
		value = malloc_init_external(sz);
		if (value != NULL) {
			if (config_extract_entry_value_direct(entry, nvs_type, value) != ESP_OK) {
				free(value);
				value = NULL;
			}
		}
	}
	return value;
}

static esp_err_t config_nvs_set_value(nvs_handle nvs, nvs_type_t type, const char *key, const void *data) {
	esp_err_t err = ESP_OK;
	if (type == NVS_TYPE_I8) {
		err = nvs_set_i8(nvs, key, *(int8_t *) data);
	} else if (type == NVS_TYPE_U8) {
		err = nvs_set_u8(nvs, key, *(uint8_t *) data);
	} else if (type == NVS_TYPE_I16) {
		err = nvs_set_i16(nvs, key, *(int16_t *) data);
	} else if (type == NVS_TYPE_U16) {
		err = nvs_set_u16(nvs, key, *(uint16_t *) data);
	} else if (type == NVS_TYPE_I32) {
		err = nvs_set_i32(nvs, key, *(int32_t *) data);
	} else if (type == NVS_TYPE_U32) {
		err = nvs_set_u32(nvs, key, *(uint32_t *) data);
	} else if (type == NVS_TYPE_I64) {
		err = nvs_set_i64(nvs, key, *(int64_t *) data);
	} else if (type == NVS_TYPE_U64) {
		err = nvs_set_u64(nvs, key, *(uint64_t *) data);
	} else if (type == NVS_TYPE_STR) {
		err = nvs_set_str(nvs, key, (const char *) data);
	} else {
		ESP_LOGE(TAG, "Unsupported NVS type %d for key %s", type, key);
		err = ESP_ERR_NVS_TYPE_MISMATCH;
	}
	return err;
}

typedef struct dirty_entry_s {
	char *key;
	nvs_type_t type;
	union {
		int8_t i8;
		uint8_t u8;
		int16_t i16;
		uint16_t u16;
		int32_t i32;
		uint32_t u32;
		int64_t i64;
		uint64_t u64;
		char *str;
	} val;
	bool written;
	struct dirty_entry_s *next;
} dirty_entry_t;

void config_commit_to_nvs(){
	if (s_commit_mutex == NULL) {
		s_commit_mutex = xSemaphoreCreateMutex();
	}
	if (s_commit_mutex != NULL && xSemaphoreTake(s_commit_mutex, LOCK_MAX_WAIT/portTICK_PERIOD_MS) != pdTRUE) {
		ESP_LOGE(TAG, "config_commit_to_nvs: Unable to obtain commit lock");
		return;
	}

	ESP_LOGI(TAG,"Committing configuration to nvs. Locking config object.");
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "config_commit_to_nvs: Unable to lock config for commit ");
		if(s_commit_mutex) xSemaphoreGive(s_commit_mutex);
		return ;
	}
	if(nvs_json==NULL){
		ESP_LOGE(TAG, ": cJSON nvs cache object not set.");
		config_unlock();
		if(s_commit_mutex) xSemaphoreGive(s_commit_mutex);
		return;
	}
	ESP_LOGV(TAG,"config_commit_to_nvs. Config Locked!");

	dirty_entry_t *dirty_head = NULL;
	dirty_entry_t **dirty_tail = &dirty_head;

	cJSON *entry = nvs_json->child;
	while(entry != NULL){
		if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
			char * entry_str = cJSON_PrintUnformatted(entry);
			if(entry_str!=NULL){
				ESP_LOGV(TAG,"config_commit_to_nvs processing item %s",entry_str);
				free(entry_str);
			}
		}

		if(config_is_entry_changed(entry)){
			if(!entry->string || entry->string[0] == '\0'){
				ESP_LOGE(TAG, "Cannot commit entry with empty or NULL key name");
				entry = entry->next;
				continue;
			}

			nvs_type_t type = config_get_entry_type(entry);
			cJSON *entry_val = cJSON_GetObjectItemCaseSensitive(entry, "value");
			if(entry_val == NULL){
				if (LOG_LOCAL_LEVEL >= ESP_LOG_ERROR) {
					char * entry_str = cJSON_PrintUnformatted(entry);
					if(entry_str!=NULL){
						ESP_LOGE(TAG, "Unable to retrieve value for key %s, Object: \n%s", entry->string, entry_str);
						free(entry_str);
					} else {
						ESP_LOGE(TAG, "Unable to retrieve value for key %s", entry->string);
					}
				}
				entry = entry->next;
				continue;
			}

			dirty_entry_t *item = (dirty_entry_t *)malloc(sizeof(dirty_entry_t));
			if(item == NULL){
				ESP_LOGE(TAG, "Unable to allocate dirty_entry for key %s", entry->string);
				entry = entry->next;
				continue;
			}
			memset(item, 0, sizeof(*item));
			item->key = strdup_psram(entry->string);
			if(item->key == NULL){
				ESP_LOGE(TAG, "Unable to duplicate key %s", entry->string);
				free(item);
				entry = entry->next;
				continue;
			}
			item->type = type;

			bool valid = true;
			if(type == NVS_TYPE_STR){
				if(cJSON_IsString(entry_val) && entry_val->valuestring){
					item->val.str = strdup_psram(entry_val->valuestring);
					if(item->val.str == NULL){
						valid = false;
					}
				} else {
					valid = false;
				}
			} else {
				switch (type) {
					case NVS_TYPE_I8:  item->val.i8  = (int8_t)entry_val->valuedouble; break;
					case NVS_TYPE_U8:  item->val.u8  = (uint8_t)entry_val->valuedouble; break;
					case NVS_TYPE_I16: item->val.i16 = (int16_t)entry_val->valuedouble; break;
					case NVS_TYPE_U16: item->val.u16 = (uint16_t)entry_val->valuedouble; break;
					case NVS_TYPE_I32: item->val.i32 = (int32_t)entry_val->valuedouble; break;
					case NVS_TYPE_U32: item->val.u32 = (uint32_t)entry_val->valuedouble; break;
					case NVS_TYPE_I64: item->val.i64 = (int64_t)entry_val->valuedouble; break;
					case NVS_TYPE_U64: item->val.u64 = (uint64_t)entry_val->valuedouble; break;
					default:
						ESP_LOGE(TAG, "Unsupported NVS type %d for key %s", type, entry->string);
						valid = false;
						break;
				}
			}

			if(!valid){
				ESP_LOGE(TAG, "Failed to copy value for dirty key %s", entry->string);
				if(item->key) free(item->key);
				free(item);
				entry = entry->next;
				continue;
			}

			*dirty_tail = item;
			dirty_tail = &item->next;
		}
		entry = entry->next;
	}

	/* Release config_mutex BEFORE slow flash writes! Audio tasks will not be blocked. */
	config_unlock();

	if(dirty_head == NULL){
		ESP_LOGI(TAG, "No dirty configuration entries to commit.");
		if(config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
			config_raise_change(false);
			config_unlock();
		}
		if(s_commit_mutex) xSemaphoreGive(s_commit_mutex);
		return;
	}

	nvs_handle nvs = 0;
	esp_err_t open_err = nvs_open_from_partition(settings_partition, current_namespace, NVS_READWRITE, &nvs);
	if(open_err != ESP_OK){
		ESP_LOGE(TAG, "config_commit_to_nvs: Unable to open nvs partition %s (%s)", settings_partition, esp_err_to_name(open_err));
		while(dirty_head != NULL){
			dirty_entry_t *next = dirty_head->next;
			if(dirty_head->key) free(dirty_head->key);
			if(dirty_head->type == NVS_TYPE_STR && dirty_head->val.str) free(dirty_head->val.str);
			free(dirty_head);
			dirty_head = next;
		}
		if(s_commit_mutex) xSemaphoreGive(s_commit_mutex);
		return;
	}

	bool commit_needed = false;
	bool all_committed = true;

	for(dirty_entry_t *curr = dirty_head; curr != NULL; curr = curr->next){
		ESP_LOGD(TAG, "Staging entry %s value to nvs.", curr->key);
		const void *val_ptr = NULL;
		switch (curr->type) {
			case NVS_TYPE_I8:  val_ptr = &curr->val.i8; break;
			case NVS_TYPE_U8:  val_ptr = &curr->val.u8; break;
			case NVS_TYPE_I16: val_ptr = &curr->val.i16; break;
			case NVS_TYPE_U16: val_ptr = &curr->val.u16; break;
			case NVS_TYPE_I32: val_ptr = &curr->val.i32; break;
			case NVS_TYPE_U32: val_ptr = &curr->val.u32; break;
			case NVS_TYPE_I64: val_ptr = &curr->val.i64; break;
			case NVS_TYPE_U64: val_ptr = &curr->val.u64; break;
			case NVS_TYPE_STR: val_ptr = curr->val.str; break;
			default: break;
		}
		esp_err_t err = config_nvs_set_value(nvs, curr->type, curr->key, val_ptr);
		if(err != ESP_OK){
			ESP_LOGE(TAG, "Error writing value to nvs for key %s: %s", curr->key, esp_err_to_name(err));
			all_committed = false;
		} else {
			curr->written = true;
			commit_needed = true;
		}
		taskYIELD();  /* allows the freeRTOS scheduler to take over if needed. */
	}

	if(commit_needed){
		ESP_LOGI(TAG, "Executing single consolidated nvs_commit.");
		esp_err_t commit_err = nvs_commit(nvs);
		if(commit_err != ESP_OK){
			ESP_LOGE(TAG, "Error committing nvs changes: %s", esp_err_to_name(commit_err));
			all_committed = false;
			for(dirty_entry_t *curr = dirty_head; curr != NULL; curr = curr->next){
				curr->written = false;
			}
		} else {
			ESP_LOGI(TAG, "Successfully committed configuration to nvs.");
		}
	}
	nvs_close(nvs);

	/* Re-acquire config_mutex only to clear dirty flags for committed entries */
	if(config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		for(dirty_entry_t *curr = dirty_head; curr != NULL; curr = curr->next){
			if(curr->written){
				cJSON *cached_entry = cJSON_GetObjectItemCaseSensitive(nvs_json, curr->key);
				if(cached_entry != NULL){
					bool value_matches = false;
					nvs_type_t cached_type = config_get_entry_type(cached_entry);
					if(cached_type == curr->type){
						cJSON *cached_val = cJSON_GetObjectItemCaseSensitive(cached_entry, "value");
						if(cached_val != NULL){
							if(curr->type == NVS_TYPE_STR){
								if(cJSON_IsString(cached_val) && cached_val->valuestring && curr->val.str){
									value_matches = (strcmp(cached_val->valuestring, curr->val.str) == 0);
								}
							} else {
								switch (curr->type) {
									case NVS_TYPE_I8:  value_matches = ((int8_t)cached_val->valuedouble == curr->val.i8); break;
									case NVS_TYPE_U8:  value_matches = ((uint8_t)cached_val->valuedouble == curr->val.u8); break;
									case NVS_TYPE_I16: value_matches = ((int16_t)cached_val->valuedouble == curr->val.i16); break;
									case NVS_TYPE_U16: value_matches = ((uint16_t)cached_val->valuedouble == curr->val.u16); break;
									case NVS_TYPE_I32: value_matches = ((int32_t)cached_val->valuedouble == curr->val.i32); break;
									case NVS_TYPE_U32: value_matches = ((uint32_t)cached_val->valuedouble == curr->val.u32); break;
									case NVS_TYPE_I64: value_matches = ((int64_t)cached_val->valuedouble == curr->val.i64); break;
									case NVS_TYPE_U64: value_matches = ((uint64_t)cached_val->valuedouble == curr->val.u64); break;
									default: break;
								}
							}
						}
					}
					if(value_matches){
						config_set_entry_changed_flag(cached_entry, false);
					} else {
						ESP_LOGI(TAG, "Entry %s modified during commit; retaining dirty flag", curr->key);
						all_committed = false;
					}
				}
			}
		}

		bool any_remaining = false;
		for(cJSON *e = nvs_json->child; e != NULL; e = e->next){
			if(config_is_entry_changed(e)){
				any_remaining = true;
				break;
			}
		}
		if(!any_remaining && all_committed){
			ESP_LOGV(TAG, "config_commit_to_nvs. Resetting the global commit flag.");
			config_raise_change(false);
		}
		config_unlock();
	} else {
		ESP_LOGE(TAG, "config_commit_to_nvs: Unable to re-lock config to clear changed flags");
	}

	while(dirty_head != NULL){
		dirty_entry_t *next = dirty_head->next;
		if(dirty_head->key) free(dirty_head->key);
		if(dirty_head->type == NVS_TYPE_STR && dirty_head->val.str) free(dirty_head->val.str);
		free(dirty_head);
		dirty_head = next;
	}

	if(s_commit_mutex) xSemaphoreGive(s_commit_mutex);
	ESP_LOGI(TAG,"Done Committing configuration to nvs.");
}

void config_flush_now(void) {
	ESP_LOGI(TAG, "Flushing configuration immediately to NVS.");
	if(config_has_changes()){
		config_commit_to_nvs();
	}
}

bool config_has_changes(){
	return  (xEventGroupGetBits(config_group) & CONFIG_NO_COMMIT_PENDING)==0;
}


bool wait_for_commit(){
	if(config_has_changes()){
		config_flush_now();
	}
	int retries = 5;
	bool commit_pending=(xEventGroupGetBits(config_group) & CONFIG_NO_COMMIT_PENDING)==0;
	while (commit_pending && retries > 0){
		ESP_LOGW(TAG,"Waiting for config commit ...");
		commit_pending = (xEventGroupWaitBits(config_group, CONFIG_NO_COMMIT_PENDING,pdFALSE, pdTRUE, (CONFIG_COMMIT_DELAY*2) / portTICK_PERIOD_MS) & CONFIG_NO_COMMIT_PENDING)==0;
		if(commit_pending){
			retries--;
			ESP_LOGW(TAG,"Timeout waiting for config commit.");
	    }
	    else {
	    	ESP_LOGI(TAG,"Config committed!");
	    }
	}
	return !commit_pending;
}

bool config_lock(TickType_t xTicksToWait) {
	ESP_LOGV(TAG, "Locking config json object");
	if( xSemaphoreTakeRecursive( config_mutex, xTicksToWait ) == pdTRUE ) {
		ESP_LOGV(TAG, "config Json object locked!");
		return true;
	}
	else {
		ESP_LOGE(TAG, "Semaphore take failed. Unable to lock config Json object mutex");
		return false;
	}
}

void config_unlock() {
	ESP_LOGV(TAG, "Unlocking json buffer!");
	xSemaphoreGiveRecursive( config_mutex );
}

static volatile bool s_commit_task_running = false;

static void config_commit_task(void *pvParameters) {
	config_commit_to_nvs();
	s_commit_task_running = false;
	vTaskDelete(NULL);
}

static void vCallbackFunction( TimerHandle_t xTimer ) {
	static int cnt=0;
	if(config_has_changes()){
		if(!s_commit_task_running){
			ESP_LOGI(TAG, "configuration has some uncommitted entries");
			s_commit_task_running = true;
			if(xTaskCreate(config_commit_task, "cfg_commit", 3072, NULL, ESP_TASK_PRIO_MIN + 1, NULL) != pdPASS){
				ESP_LOGE(TAG, "Failed to create cfg_commit task");
				s_commit_task_running = false;
			}
		}
	}
	else{
		if(++cnt>=15){
			ESP_LOGV(TAG,"commit timer: commit flag not set");
			cnt=0;
		}
	}
	xTimerReset( xTimer, 10 );
}
void config_raise_change(bool change_found){
	if(config_set_group_bit(CONFIG_NO_COMMIT_PENDING,!change_found))
	{
		ESP_LOGD(TAG,"Config commit set to %s",change_found?"Pending Commit":"Committed");
	}
}
bool config_set_group_bit(int bit_num,bool flag){
	bool result = true;
	int curFlags=xEventGroupGetBits(config_group);
	if((curFlags & CONFIG_LOAD_BIT) && bit_num == CONFIG_NO_COMMIT_PENDING ){
		ESP_LOGD(TAG,"Loading config, ignoring changes");
		result = false;
	}
	if(result){
		bool curBit=(xEventGroupGetBits(config_group) & bit_num);
		if(curBit == flag){
			ESP_LOGV(TAG,"Flag %d already %s", bit_num, flag?"Set":"Cleared");
			result = false;
		}
	}
	if(result){
		ESP_LOGV(TAG,"%s Flag %d ", flag?"Setting":"Clearing",bit_num);
		if(!flag){
			xEventGroupClearBits(config_group, bit_num);
		}
		else {
			xEventGroupSetBits(config_group, bit_num);
		}
	}
	return result;
}

void config_set_default(nvs_type_t type, const char *key, const void * default_value, size_t blob_size) {
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "Unable to lock config");
		return;
	}

	ESP_LOGV(TAG, "Checking if key %s exists in nvs cache for type %s.", key,type_to_str(type));
	cJSON * entry = cJSON_GetObjectItemCaseSensitive(nvs_json, key);

	if(entry !=NULL){
		ESP_LOGV(TAG, "Entry found.");
	}
	else {
		// Value was not found
		ESP_LOGW(TAG, "Adding default value for [%s].", key);
		entry=config_set_value_safe(type, key, default_value);
		if(entry == NULL){
			ESP_LOGE(TAG, "Failed to add value to cache!");
		}
		if (LOG_LOCAL_LEVEL >= ESP_LOG_DEBUG) {
			char * entry_str = cJSON_PrintUnformatted(entry);
			if(entry_str!=NULL){
				ESP_LOGD(TAG, "Value added to default for object: \n%s",entry_str);
				free(entry_str);
			}
		}
	}

	config_unlock();

}

void config_delete_key(const char *key){
	nvs_handle nvs;
	ESP_LOGD(TAG, "Deleting nvs entry for [%s]", key);
	if (s_commit_mutex == NULL) {
		s_commit_mutex = xSemaphoreCreateMutex();
	}
	if (s_commit_mutex != NULL && xSemaphoreTake(s_commit_mutex, LOCK_MAX_WAIT/portTICK_PERIOD_MS) != pdTRUE) {
		ESP_LOGE(TAG, "Unable to lock commit mutex for delete");
		return;
	}
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "Unable to lock config for delete");
		if (s_commit_mutex) xSemaphoreGive(s_commit_mutex);
		return ;
	}
	esp_err_t err = nvs_open_from_partition(settings_partition, current_namespace, NVS_READWRITE, &nvs);
	if (err == ESP_OK) {
		err = nvs_erase_key(nvs, key);
		if (err == ESP_OK) {
			ESP_LOGD(TAG, "key [%s] erased from nvs.",key);
			err = nvs_commit(nvs);
			if (err == ESP_OK) {
				ESP_LOGD(TAG, "nvs erase committed.");
			}
			else {
				ESP_LOGE(TAG, "Unable to commit nvs erase operation for key [%s]. %s.",key,esp_err_to_name(err));
			}
		}
		else if (err == ESP_ERR_NVS_NOT_FOUND) {
			ESP_LOGD(TAG, "key [%s] not found in nvs.", key);
		}
		else {
			ESP_LOGE(TAG, "Unable to delete nvs key [%s]. %s. ",key, esp_err_to_name(err));
		}
		nvs_close(nvs);
	}
	else {
		ESP_LOGE(TAG, "Error opening nvs: %s. Unable to delete nvs key [%s].",esp_err_to_name(err),key);
	}

	if (err == ESP_OK || err == ESP_ERR_NVS_NOT_FOUND) {
		if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
			char * struc_str = cJSON_PrintUnformatted(nvs_json);
			if(struc_str!=NULL){
				ESP_LOGV(TAG, "Structure before delete \n%s", struc_str);
				free(struc_str);
			}
		}
		cJSON * entry = cJSON_DetachItemFromObjectCaseSensitive(nvs_json, key);
		if(entry !=NULL){
			ESP_LOGI(TAG, "Removing config key [%s]", entry->string);
			cJSON_Delete(entry);
			if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
				char * struc_str = cJSON_PrintUnformatted(nvs_json);
				if(struc_str!=NULL){
					ESP_LOGV(TAG, "Structure after delete \n%s", struc_str);
					free(struc_str);
				}
			}
		}
		else {
			ESP_LOGW(TAG, "Unable to remove config key [%s]: not found.", key);
		}
	} else {
		ESP_LOGE(TAG, "NVS key deletion failed for [%s]; retaining in-memory cache.", key);
	}
	config_unlock();
	if (s_commit_mutex) xSemaphoreGive(s_commit_mutex);
}

void config_reset_cache(void) {
	if (config_mutex == NULL) {
		return;
	}
	if (!config_lock(LOCK_MAX_WAIT / portTICK_PERIOD_MS)) {
		ESP_LOGE(TAG, "Unable to lock config for cache reset");
		return;
	}
	if (nvs_json != NULL) {
		cJSON_Delete(nvs_json);
	}
	nvs_json = cJSON_CreateObject();
	config_raise_change(false);
	config_unlock();
}

void config_invalidate_cache(void) {
	config_reset_cache();
}

void * config_alloc_get(nvs_type_t nvs_type, const char *key) {
	return config_alloc_get_default(nvs_type, key, NULL, 0);
}
cJSON * config_alloc_get_cjson(const char *key){
	char * conf_str = config_alloc_get_default(NVS_TYPE_STR, key, NULL, 0);
	if(conf_str==NULL){
		ESP_LOGE(TAG, "Unable to get config value for key [%s]", key);
		return NULL;
	}
	cJSON * conf_json = cJSON_Parse(conf_str);
	free(conf_str);
	if(conf_json==NULL){
		ESP_LOGE(TAG, "Unable to parse config value for key [%s]", key);
		return NULL;
	}
	return conf_json;
}
esp_err_t config_set_cjson_str_and_free(const char *key, cJSON *value){
	char * value_str = cJSON_PrintUnformatted(value);
	if(value_str==NULL){
		ESP_LOGE(TAG, "Unable to print cJSON for key [%s]", key);
		cJSON_Delete(value);
		return ESP_ERR_INVALID_ARG;
	}
	esp_err_t err = config_set_value(NVS_TYPE_STR,key, value_str);
	free(value_str);
	cJSON_Delete(value);
	return err;
}
void config_get_uint16t_from_str(const char *key, uint16_t *value, uint16_t default_value){
	char * str_value = config_alloc_get(NVS_TYPE_STR, key);
	if(str_value == NULL){
		*value = default_value;
		return ;
	}
	char *endptr = NULL;
	errno = 0;
	unsigned long val = strtoul(str_value, &endptr, 10);
	if (endptr == str_value || (*endptr != '\0' && *endptr != '\r' && *endptr != '\n') || errno != 0 || val > 65535) {
		ESP_LOGW(TAG, "Invalid uint16 string '%s' for key [%s], using default %u", str_value, key, default_value);
		*value = default_value;
	} else {
		*value = (uint16_t)val;
	}
	free(str_value);
}

void * config_alloc_get_str(const char *key, char *lead, char *fallback) {
	if (lead && *lead) return strdup_psram(lead);
	char *value = config_alloc_get_default(NVS_TYPE_STR, key, NULL, 0);
	if ((!value || !*value) && fallback) {
		if (value) free(value);
		value = strdup_psram(fallback);
	}
	return value;
}

void * config_alloc_get_default(nvs_type_t nvs_type, const char *key, void * default_value, size_t blob_size) {

	void * value = NULL;
	ESP_LOGV(TAG, "Retrieving key %s from nvs cache for type %s.", key,type_to_str(nvs_type));
	if(nvs_json==NULL){
		ESP_LOGE(TAG,"configuration not loaded!");
		return value;
	}
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "Unable to lock config");
		return value;
	}
	ESP_LOGD(TAG,"Getting config entry for key %s",key);
	cJSON * entry = cJSON_GetObjectItemCaseSensitive(nvs_json, key);
	if(entry !=NULL){
		ESP_LOGV(TAG, "Entry found, getting value.");
		value = config_safe_alloc_get_entry_value(nvs_type, entry);
	}
	else if(default_value!=NULL){
		// Value was not found
		ESP_LOGW(TAG, "Adding new config value for key [%s]",key);
		entry=config_set_value_safe(nvs_type, key, default_value);
		if(entry == NULL){
			ESP_LOGE(TAG, "Failed to add value to cache");
		}
		else {
			if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
				char * entry_str = cJSON_PrintUnformatted(entry);
				if(entry_str!=NULL){
					ESP_LOGV(TAG, "Value added configuration object for key [%s]: \n%s", entry->string,entry_str);
					free(entry_str);
				}
				else {
					ESP_LOGV(TAG, "Value added configuration object for key [%s]", entry->string);
				}
			}
			value = config_safe_alloc_get_entry_value(nvs_type, entry);
		}
	}
	else{
		ESP_LOGW(TAG,"Value not found for key %s",key);
	}
	config_unlock();
	return value;
}
char * config_alloc_get_json(bool bFormatted){
	char * json_buffer = NULL;
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "Unable to lock config after %d ms",LOCK_MAX_WAIT);
		return strdup_psram("{\"error\":\"Unable to lock configuration object.\"}");
	}
	cJSON *clone = cJSON_Duplicate(nvs_json, true);
	if(clone != NULL){
		cJSON_DeleteItemFromObject(clone, "telnet_pwd");
		cJSON_DeleteItemFromObject(clone, "a2dp_spin");
		if(bFormatted){
			json_buffer= cJSON_Print(clone);
		}
		else {
			json_buffer= cJSON_PrintUnformatted(clone);
		}
		cJSON_Delete(clone);
	}
	config_unlock();
	return json_buffer;
}
esp_err_t config_set_value(nvs_type_t nvs_type, const char *key, const void * value){
	esp_err_t result = ESP_OK;
	if(!key ||!key[0]){
		ESP_LOGW(TAG,"Empty key passed. Ignoring entry!");
		return ESP_ERR_INVALID_ARG;
	}
	if(!config_lock(LOCK_MAX_WAIT/portTICK_PERIOD_MS)){
		ESP_LOGE(TAG, "Unable to lock config after %d ms",LOCK_MAX_WAIT);
		return ESP_FAIL;
	}
	cJSON * entry = config_set_value_safe(nvs_type, key, value);
	if(entry == NULL){
		result = ESP_FAIL;
	}
	else{
		if (LOG_LOCAL_LEVEL >= ESP_LOG_VERBOSE) {
			char * entry_str = cJSON_PrintUnformatted(entry);
			if(entry_str!=NULL){
				ESP_LOGV(TAG,"config_set_value result: \n%s",entry_str);
				free(entry_str);
			}
			else {
				ESP_LOGV(TAG,"config_set_value completed");
			}
		}
	}
	config_unlock();
	return result;
}
cJSON* cjson_update_string(cJSON** root, const char* key, const char* value) {
	if (*root == NULL) {
		*root = cJSON_CreateObject();
		if (*root == NULL) {
			ESP_LOGE(TAG, "Error creating cJSON object!");
		}
	}
	if (!key || !value || strlen(key) == 0) {
		ESP_LOGE(TAG, "cjson_update_string. Invalid key or value passed! key: %s, value: %s", STR_OR_ALT(key, ""), STR_OR_ALT(value, ""));
		return *root;
	}
	cJSON* cjsonvalue = cJSON_GetObjectItemCaseSensitive(*root, key);
	if (cjsonvalue) {
		if (!cJSON_IsString(cjsonvalue)) {
			ESP_LOGD(TAG, "Replacing non-string item %s with string: %s", key, value);
			cJSON_ReplaceItemInObject(*root, key, cJSON_CreateString(value));
		} else {
			const char *cur = cJSON_GetStringValue(cjsonvalue);
			if (cur == NULL || strcasecmp(cur, value) != 0) {
				ESP_LOGD(TAG, "Value %s changed from %s to %s", key, STR_OR_ALT(cur, ""), value);
				cJSON_SetValuestring(cjsonvalue, value);
			}
		}
	} else if(!cjsonvalue){
		ESP_LOGD(TAG, "Adding new value %s: %s", key, value);
		cJSON_AddItemToObject(*root, key, cJSON_CreateString(value));
	}
    return *root;
}
cJSON* cjson_update_number(cJSON** root, const char* key, int value) {

	if (*root == NULL) {
		*root = cJSON_CreateObject();
	}
	if (key  && strlen(key) != 0) {
		cJSON* cjsonvalue = cJSON_GetObjectItemCaseSensitive(*root, key);
		if (cjsonvalue) {
			if (!cJSON_IsNumber(cjsonvalue)) {
				ESP_LOGD(TAG, "Replacing non-number item %s with number: %d", key, value);
				cJSON_ReplaceItemInObject(*root, key, cJSON_CreateNumber(value));
			} else {
				cJSON_SetNumberValue(cjsonvalue, value);
			}
		} else {
			cJSON_AddNumberToObject(*root, key, value);
		}
	}
    return *root;
}
IMPLEMENT_SET_DEFAULT(uint8_t,NVS_TYPE_U8);
IMPLEMENT_SET_DEFAULT(int8_t,NVS_TYPE_I8);
IMPLEMENT_SET_DEFAULT(uint16_t,NVS_TYPE_U16);
IMPLEMENT_SET_DEFAULT(int16_t,NVS_TYPE_I16);
IMPLEMENT_SET_DEFAULT(uint32_t,NVS_TYPE_U32);
IMPLEMENT_SET_DEFAULT(int32_t,NVS_TYPE_I32);

IMPLEMENT_GET_NUM(uint8_t,NVS_TYPE_U8);
IMPLEMENT_GET_NUM(int8_t,NVS_TYPE_I8);
IMPLEMENT_GET_NUM(uint16_t,NVS_TYPE_U16);
IMPLEMENT_GET_NUM(int16_t,NVS_TYPE_I16);
IMPLEMENT_GET_NUM(uint32_t,NVS_TYPE_U32);
IMPLEMENT_GET_NUM(int32_t,NVS_TYPE_I32);
