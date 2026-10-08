#ifdef NETWORK_WIFI_LOG_LEVEL
#define LOG_LOCAL_LEVEL NETWORK_WIFI_LOG_LEVEL
#endif
#include "network_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include <mbedtls/sha256.h>
#include <string.h>
#include "cJSON.h"
#include "dns_server.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_wifi.h"
#include "esp_wifi_types.h"
#include "lwip/sockets.h"
#include "messaging.h"
#include "network_status.h"
#include "nvs.h"
#include "nvs_flash.h"
#include "nvs_utilities.h"
#include "platform_config.h"
#include "platform_esp32.h"
#include "tools.h"
#include "trace.h"
static void network_wifi_event_handler(void* arg, esp_event_base_t event_base, int32_t event_id, void* event_data);
static char* get_disconnect_code_desc(uint8_t reason);
esp_err_t network_wifi_get_blob(void* target, size_t size, const char* key);
cJSON* accessp_cjson = NULL;

static const char TAG[] = "network_wifi";
const char network_wifi_nvs_namespace[] = "config";
const char ap_list_nsv_namespace[] = "aplist";
/* rrm ctx */
//Roaming support - int rrm_ctx = 0;

uint16_t ap_num = 0;

esp_netif_t* wifi_netif;
esp_netif_t* wifi_ap_netif;

wifi_ap_record_t* accessp_records = NULL;
#define UINT_TO_STRING(buf, val) do { \
    memset((buf), 0x00, sizeof(buf)); \
    strlcpy((buf), (const char*)(val), sizeof(buf)); \
} while(0)

static const char ENC_PREFIX[] = "enc:";

static void get_wifi_device_key(uint8_t* key, size_t key_len) {
    uint8_t mac[6] = {0};
#ifdef ESP_PLATFORM
    esp_read_mac(mac, ESP_MAC_WIFI_STA);
#endif
    // 16-byte domain-specific salt combined with hardware MAC
    static const uint8_t salt[16] = {
        0x57, 0x69, 0x46, 0x69, 0x4e, 0x56, 0x53, 0x4b,
        0x65, 0x79, 0x24, 0x53, 0x65, 0x63, 0x75, 0x72
    };
    uint8_t combined[sizeof(salt) + sizeof(mac)];
    memcpy(combined, salt, sizeof(salt));
    memcpy(combined + sizeof(salt), mac, sizeof(mac));
    uint8_t digest[32];
    mbedtls_sha256(combined, sizeof(combined), digest, 0);
    size_t copy_len = key_len < sizeof(digest) ? key_len : sizeof(digest);
    memcpy(key, digest, copy_len);
}

static char* encrypt_wifi_credentials(const char* plaintext) {
    if (!plaintext || strlen(plaintext) == 0) {
        return strdup_psram("");
    }
    size_t prefix_len = strlen(ENC_PREFIX);
    if (strncmp(plaintext, ENC_PREFIX, prefix_len) == 0) {
        return strdup_psram(plaintext);
    }
    uint8_t key[16];
    get_wifi_device_key(key, sizeof(key));
    size_t plain_len = strlen(plaintext);
    size_t enc_len = prefix_len + (plain_len * 2) + 1;
    char* enc_str = (char*)malloc_init_external(enc_len);
    if (!enc_str) {
        return NULL;
    }
    strcpy(enc_str, ENC_PREFIX);
    for (size_t i = 0; i < plain_len; i++) {
        uint8_t b = (uint8_t)plaintext[i] ^ key[i % sizeof(key)];
        snprintf(enc_str + prefix_len + (i * 2), 3, "%02x", b);
    }
    enc_str[enc_len - 1] = '\0';
    return enc_str;
}

static char* decrypt_wifi_credentials(const char* ciphertext) {
    if (!ciphertext || strlen(ciphertext) == 0) {
        return strdup_psram("");
    }
    size_t prefix_len = strlen(ENC_PREFIX);
    if (strncmp(ciphertext, ENC_PREFIX, prefix_len) == 0) {
        const char* hex_payload = ciphertext + prefix_len;
        size_t hex_len = strlen(hex_payload);
        if (hex_len % 2 != 0) {
            ESP_LOGE(TAG, "Invalid encrypted credentials hex length");
            return strdup_psram(ciphertext);
        }
        uint8_t key[16];
        get_wifi_device_key(key, sizeof(key));
        size_t plain_len = hex_len / 2;
        char* plaintext = (char*)malloc_init_external(plain_len + 1);
        if (!plaintext) {
            return NULL;
        }
        for (size_t i = 0; i < hex_len; i += 2) {
            char byte_chars[3] = { hex_payload[i], hex_payload[i + 1], '\0' };
            unsigned long val = strtoul(byte_chars, NULL, 16);
            plaintext[i / 2] = (char)((uint8_t)val ^ key[(i / 2) % sizeof(key)]);
        }
        plaintext[plain_len] = '\0';
        return plaintext;
    }
    return strdup_psram(ciphertext);
}
typedef struct known_access_point {
    char* ssid;
    char* password;
    bool found;
    uint8_t bssid[6];          /**< MAC address of AP */
    uint8_t primary;           /**< channel of AP */
    wifi_auth_mode_t authmode; /**< authmode of AP */
    uint32_t phy_11b : 1;      /**< bit: 0 flag to identify if 11b mode is enabled or not */
    uint32_t phy_11g : 1;      /**< bit: 1 flag to identify if 11g mode is enabled or not */
    uint32_t phy_11n : 1;      /**< bit: 2 flag to identify if 11n mode is enabled or not */
    uint32_t phy_lr : 1;       /**< bit: 3 flag to identify if low rate is enabled or not */
    time_t last_try;
    SLIST_ENTRY(known_access_point)
    next;  //!< next callback
} known_access_point_t;

/** linked list of command structures */
static EXT_RAM_ATTR SLIST_HEAD(ap_list, known_access_point) s_ap_list;
static SemaphoreHandle_t s_ap_list_mutex = NULL;

static void ap_list_lock(void) {
    if (!s_ap_list_mutex) {
        s_ap_list_mutex = xSemaphoreCreateMutex();
    }
    if (s_ap_list_mutex) {
        xSemaphoreTake(s_ap_list_mutex, portMAX_DELAY);
    }
}
static void ap_list_unlock(void) {
    if (s_ap_list_mutex) {
        xSemaphoreGive(s_ap_list_mutex);
    }
}

known_access_point_t* network_wifi_get_ap_entry(const char* ssid) {
    known_access_point_t* it;

    if (!ssid || strlen(ssid) == 0) {
        ESP_LOGW(TAG, "network_wifi_get_ap_entry Invalid SSID %s", !ssid ? "IS NULL" : "IS BLANK");
        return NULL;
    }

    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        ESP_LOGD(TAG, "Looking for SSID %s = %s ?", ssid, it->ssid);
        if (strcmp(it->ssid, ssid) == 0) {
            ESP_LOGD(TAG, "network_wifi_get_ap_entry SSID %s found! ", ssid);
            ap_list_unlock();
            return it;
        }
    }
    ap_list_unlock();
    return NULL;
}
void network_wifi_erase_ap_item(const char* ssid) {
    if (!ssid || strlen(ssid) == 0) {
        ESP_LOGE(TAG, "network_wifi_remove_ap_entry error empty SSID");
        return;
    }
    ap_list_lock();
    known_access_point_t* it = NULL;
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (it->ssid && strcmp(it->ssid, ssid) == 0) {
            break;
        }
    }
    if (it) {
        ESP_LOGW(TAG, "Removing %s from known list of access points", ssid);
        FREE_AND_NULL(it->ssid);
        FREE_AND_NULL(it->password);
        SLIST_REMOVE(&s_ap_list, it, known_access_point, next);
        FREE_AND_NULL(it);
    }
    ap_list_unlock();
}
void network_wifi_remove_ap_entry(const char* ssid) {
    network_wifi_erase_ap_item(ssid);
}
void network_wifi_clear_ap_list(void) {
    ap_list_lock();
    known_access_point_t* it;
    while ((it = SLIST_FIRST(&s_ap_list)) != NULL) {
        SLIST_REMOVE_HEAD(&s_ap_list, next);
        FREE_AND_NULL(it->ssid);
        FREE_AND_NULL(it->password);
        FREE_AND_NULL(it);
    }
    ap_list_unlock();
}
void network_wifi_empty_known_list() {
    network_wifi_clear_ap_list();
}

const wifi_sta_config_t* network_wifi_get_active_config() {
    static wifi_config_t config;
    esp_err_t err = ESP_OK;
    memset(&config, 0x00, sizeof(config));
    if ((err = esp_wifi_get_config(WIFI_IF_STA, &config)) == ESP_OK) {
        return &config.sta;
    } else {
        ESP_LOGD(TAG, "Could not get wifi STA config: %s", esp_err_to_name(err));
    }
    return NULL;
}

size_t network_wifi_get_known_count() {
    size_t count = 0;
    known_access_point_t* it;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        count++;
    }
    ap_list_unlock();
    return count;
}
size_t network_wifi_get_known_count_in_range() {
    size_t count = 0;
    known_access_point_t* it;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        if(it->found) count++;
    }
    ap_list_unlock();
    return count;
}
esp_err_t network_wifi_add_ap(known_access_point_t* item) {
    known_access_point_t* last = SLIST_FIRST(&s_ap_list);
    if (last == NULL) {
        SLIST_INSERT_HEAD(&s_ap_list, item, next);
    } else {
        known_access_point_t* it;
        while ((it = SLIST_NEXT(last, next)) != NULL) {
            last = it;
        }
        SLIST_INSERT_AFTER(last, item, next);
    }
    return ESP_OK;
}
esp_err_t network_wifi_add_ap_copy(const known_access_point_t* known_ap) {
    known_access_point_t* item = NULL;
    esp_err_t err = ESP_OK;
    
    if (!known_ap) {
        ESP_LOGE(TAG, "Invalid access point entry");
        return ESP_ERR_INVALID_ARG;
    }
    if (!known_ap->ssid || strlen(known_ap->ssid) == 0) {
        ESP_LOGE(TAG, "Invalid access point ssid");
        return ESP_ERR_INVALID_ARG;
    }
    item = malloc_init_external(sizeof(known_access_point_t));
    if (item == NULL) {
        ESP_LOGE(TAG, "Memory allocation failed");
        return ESP_ERR_NO_MEM;
    }
    item->ssid = strdup_psram(known_ap->ssid);
    item->password = strdup_psram(known_ap->password);
    memcpy(&item->bssid, known_ap->bssid, sizeof(item->bssid));
    item->primary = known_ap->primary;
    item->authmode = known_ap->authmode;
    item->phy_11b = known_ap->phy_11b;
    item->phy_11g = known_ap->phy_11g;
    item->phy_11n = known_ap->phy_11n;
    item->phy_lr = known_ap->phy_lr;
    ap_list_lock();
    err = network_wifi_add_ap(item);
    ap_list_unlock();
    return err;
}
const wifi_ap_record_t* network_wifi_get_ssid_info(const char* ssid) {
    if (!accessp_records)
        return NULL;
    char ap_ssid[sizeof(accessp_records[0].ssid) + 1];
    for (int i = 0; i < ap_num; i++) {
        UINT_TO_STRING(ap_ssid, accessp_records[i].ssid);
        if (strcmp(ap_ssid, ssid) == 0) {
            return &accessp_records[i];
        }
    }
    return NULL;
}
esp_err_t network_wifi_add_ap_from_sta_copy(const wifi_sta_config_t* sta) {
    known_access_point_t* item = NULL;
    esp_err_t err = ESP_OK;
    if (!sta) {
        ESP_LOGE(TAG, "Invalid access point entry");
        return ESP_ERR_INVALID_ARG;
    }
    if (!sta->ssid || strlen((char*)sta->ssid) == 0) {
        ESP_LOGE(TAG, "Invalid access point ssid");
        return ESP_ERR_INVALID_ARG;
    }
    item = malloc_init_external(sizeof(known_access_point_t));
    if (item == NULL) {
        ESP_LOGE(TAG, "Memory allocation failed");
        return ESP_ERR_NO_MEM;
    }
    char sta_ssid[sizeof(sta->ssid) + 1];
    char sta_pwd[sizeof(sta->password) + 1];
    UINT_TO_STRING(sta_ssid, sta->ssid);
    UINT_TO_STRING(sta_pwd, sta->password);
    item->ssid = strdup_psram(sta_ssid);
    item->password = strdup_psram(sta_pwd);
    memcpy(&item->bssid, sta->bssid, sizeof(item->bssid));
    item->primary = sta->channel;
    const wifi_ap_record_t* seen = network_wifi_get_ssid_info(item->ssid);
    if (seen) {
        item->authmode = seen->authmode;
        item->phy_11b = seen->phy_11b;
        item->phy_11g = seen->phy_11g;
        item->phy_11n = seen->phy_11n;
        item->phy_lr = seen->phy_lr;
    }
    ap_list_lock();
    err = network_wifi_add_ap(item);
    ap_list_unlock();
    return err;
}

bool network_wifi_is_known_ap(const char* ssid) {
    return network_wifi_get_ap_entry(ssid) != NULL;
}

static bool network_wifi_was_ssid_seen(const char* ssid) {
    if (!accessp_records || ap_num == 0 || ap_num > MAX_AP_NUM) {
        return false;
    }
    char ap_ssid[sizeof(accessp_records[0].ssid) + 1];
    for (int i = 0; i < ap_num; i++) {
        UINT_TO_STRING(ap_ssid, accessp_records[i].ssid);
        if (strcmp(ap_ssid, ssid) == 0) {
            return true;
        }
    }
    return false;
}
void network_wifi_set_found_ap() {
    known_access_point_t* it;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (network_wifi_was_ssid_seen(it->ssid)) {
            it->found = true;
        } else {
            it->found = false;
        }
    }
    ap_list_unlock();
}
bool network_wifi_known_ap_in_range(){
    known_access_point_t* it;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (it->found) {
            ap_list_unlock();
            return true;
        }
    }
    ap_list_unlock();
    return false;
}
const char * network_wifi_get_next_ap_in_range(){
    known_access_point_t* it;
    time_t last_try_min=(esp_timer_get_time() / 1000);
    const char *found_ssid = NULL;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (it->found && it->last_try < last_try_min) {
            last_try_min = it->last_try;
        }
    }
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (it->found && it->last_try == last_try_min) {
            found_ssid = it->ssid;
            break;
        }
    }
    ap_list_unlock();
    return found_ssid;
}

esp_err_t network_wifi_alloc_ap_json(known_access_point_t* item, char** json_string) {
    esp_err_t err = ESP_OK;
    if (!item || !json_string) {
        return ESP_ERR_INVALID_ARG;
    }
    cJSON* cjson_item = cJSON_CreateObject();
    if (!cjson_item) {
        ESP_LOGE(TAG, "Memory allocation failure. Cannot save ap json");
        return ESP_ERR_NO_MEM;
    }
    cJSON_AddStringToObject(cjson_item, "ssid", item->ssid);
    char* enc_pass = encrypt_wifi_credentials(item->password);
    cJSON_AddStringToObject(cjson_item, "pass", enc_pass ? enc_pass : "");
    FREE_AND_NULL(enc_pass);
    cJSON_AddNumberToObject(cjson_item, "chan", item->primary);
    cJSON_AddNumberToObject(cjson_item, "auth", item->authmode);
    char* bssid = network_manager_alloc_get_mac_string(item->bssid);
    if (bssid) {
        cJSON_AddItemToObject(cjson_item, "bssid", cJSON_CreateString(STR_OR_BLANK(bssid)));
    }
    FREE_AND_NULL(bssid);
    cJSON_AddNumberToObject(cjson_item, "b", item->phy_11b ? 1 : 0);
    cJSON_AddNumberToObject(cjson_item, "g", item->phy_11g ? 1 : 0);
    cJSON_AddNumberToObject(cjson_item, "n", item->phy_11n ? 1 : 0);
    cJSON_AddNumberToObject(cjson_item, "low_rate", item->phy_lr ? 1 : 0);

    *json_string = cJSON_PrintUnformatted(cjson_item);
    if (!*json_string) {
        ESP_LOGE(TAG, "Memory allocaiton failed. Cannot save ap entry.");
        err = ESP_ERR_NO_MEM;
    }
    cJSON_Delete(cjson_item);
    return err;
}
bool network_wifi_str2mac(const char* mac, uint8_t* values) {
    if (6 == sscanf(mac, "%hhx:%hhx:%hhx:%hhx:%hhx:%hhx", &values[0], &values[1], &values[2], &values[3], &values[4], &values[5])) {
        return true;
    } else {
        return false;
    }
}

esp_err_t network_wifi_add_json_entry(const char* json_text) {
    esp_err_t err = ESP_OK;
    known_access_point_t known_ap;
    memset(&known_ap, 0, sizeof(known_ap));
    if (!json_text || strlen(json_text) == 0) {
        ESP_LOGE(TAG, "Invalid access point json");
        return ESP_ERR_INVALID_ARG;
    }
    cJSON* cjson_item = cJSON_Parse(json_text);
    if (!cjson_item) {
        ESP_LOGE(TAG, "Invalid JSON in storage");
        return ESP_ERR_INVALID_ARG;
    }
    cJSON* value = cJSON_GetObjectItemCaseSensitive(cjson_item, "ssid");
    if (!value || !cJSON_IsString(value) || strlen(cJSON_GetStringValue(value)) == 0) {
        ESP_LOGE(TAG, "Missing ssid in access point entry");
        err = ESP_ERR_INVALID_ARG;
    } else {
        if (!network_wifi_get_ap_entry(cJSON_GetStringValue(value))) {
            known_ap.ssid = strdup_psram(cJSON_GetStringValue(value));
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "pass");
            if (value && cJSON_IsString(value) && strlen(cJSON_GetStringValue(value)) > 0) {
                known_ap.password = decrypt_wifi_credentials(cJSON_GetStringValue(value));
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "chan");
            if (value) {
                known_ap.primary = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "auth");
            if (value) {
                known_ap.authmode = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "b");
            if (value) {
                known_ap.phy_11b = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "g");
            if (value) {
                known_ap.phy_11g = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "n");
            if (value) {
                known_ap.phy_11n = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "low_rate");
            if (value) {
                known_ap.phy_lr = value->valueint;
            }
            value = cJSON_GetObjectItemCaseSensitive(cjson_item, "bssid");
            if (value && cJSON_IsString(value) && strlen(cJSON_GetStringValue(value)) > 0) {
                network_wifi_str2mac(cJSON_GetStringValue(value), known_ap.bssid);
            }
            err = network_wifi_add_ap_copy(&known_ap);
            if (known_ap.ssid) {
                free(known_ap.ssid);
            }
            if (known_ap.password) {
                memset(known_ap.password, 0, strlen(known_ap.password));
                free(known_ap.password);
            }
        } else {
            ESP_LOGE(TAG, "Duplicate ssid %s found in storage", cJSON_GetStringValue(value));
        }
    }
    cJSON_Delete(cjson_item);
    return err;
}
esp_err_t network_wifi_delete_ap(const char* key) {
    esp_err_t esp_err = ESP_OK;
    if (!key || strlen(key) == 0) {
        ESP_LOGE(TAG, "SSID Empty. Cannot remove ");
        return ESP_ERR_INVALID_ARG;
    }

    known_access_point_t* it = network_wifi_get_ap_entry(key);
    if (!it) {
        ESP_LOGE(TAG, "Unknown AP entry");
        return ESP_ERR_INVALID_ARG;
    }

    /* 
     * Check if we're deleting the active network
     */
    ESP_LOGD(TAG, "Deleting AP %s. Checking if this is the active AP", key);
    const wifi_sta_config_t* config = network_wifi_load_active_config();
    char config_ssid[sizeof(config->ssid) + 1];
    if (config) {
        UINT_TO_STRING(config_ssid, config->ssid);
    }
    if (config && strlen(config_ssid) > 0 && strcmp(config_ssid, it->ssid) == 0) {
        ESP_LOGD(TAG, "Confirmed %s to be the active network. Removing it from flash.", key);
        esp_err = network_wifi_erase_legacy();
        if (esp_err != ESP_OK) {
            ESP_LOGW(TAG, "Legacy network details could not be removed from flash : %s", esp_err_to_name(esp_err));
        }
    }
    ESP_LOGD(TAG, "Removing network %s from the flash AP list", key);
    esp_err = erase_nvs_for_partition(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace, it->ssid);
    if (esp_err != ESP_OK) {
        messaging_post_message(MESSAGING_ERROR, MESSAGING_CLASS_SYSTEM, "Deleting network entry %s error (%s). Error %s", key, ap_list_nsv_namespace, esp_err_to_name(esp_err));
    }
    ESP_LOGD(TAG, "Removing network %s from the known AP list", key);
    network_wifi_remove_ap_entry(it->ssid);
    return esp_err;
}

esp_err_t network_wifi_erase_legacy() {
    esp_err_t err = erase_nvs_for_partition(NVS_DEFAULT_PART_NAME, network_wifi_nvs_namespace, "ssid");
    erase_nvs_for_partition(NVS_DEFAULT_PART_NAME, network_wifi_nvs_namespace, "password");
    if (err == ESP_OK) {
        ESP_LOGW(TAG, "Erased wifi configuration. Disconnecting from network");
        if ((err = esp_wifi_disconnect()) != ESP_OK) {
            ESP_LOGW(TAG, "Could not disconnect from deleted network : %s", esp_err_to_name(err));
        }
    }
    return err;
}

esp_err_t network_wifi_erase_known_ap() {
    network_wifi_empty_known_list();
    esp_err_t err = erase_nvs_partition(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace);
    return err;
}

esp_err_t network_wifi_write_ap(const char* key, const char* value, size_t size) {
    size_t size_override = size > 0 ? size : strlen(value) + 1;
    esp_err_t esp_err = store_nvs_value_len_for_partition(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace, NVS_TYPE_BLOB, key, value, size_override);
    if (esp_err != ESP_OK) {
        messaging_post_message(MESSAGING_ERROR, MESSAGING_CLASS_SYSTEM, "%s (%s). Error %s", key, network_wifi_nvs_namespace, esp_err_to_name(esp_err));
    }
    return esp_err;
}
esp_err_t network_wifi_write_nvs(const char* key, const char* value, size_t size) {
    size_t size_override = size > 0 ? size : strlen(value) + 1;
    esp_err_t esp_err = store_nvs_value_len_for_partition(NVS_DEFAULT_PART_NAME, network_wifi_nvs_namespace, NVS_TYPE_BLOB, key, value, size_override);
    if (esp_err != ESP_OK) {
        messaging_post_message(MESSAGING_ERROR, MESSAGING_CLASS_SYSTEM, "%s (%s). Error %s", key, network_wifi_nvs_namespace, esp_err_to_name(esp_err));
    }
    return esp_err;
}

esp_err_t network_wifi_store_ap_json(known_access_point_t* item) {
    esp_err_t err = ESP_OK;
    size_t size = 0;
    char* json_string = NULL;
    const wifi_sta_config_t* sta = network_wifi_get_active_config();

    if ((err = network_wifi_alloc_ap_json(item, &json_string)) == ESP_OK) {
        // get any existing entry from the nvs and compare
        char* existing = get_nvs_value_alloc_for_partition(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace, NVS_TYPE_BLOB, item->ssid, &size);
        if (!existing || strncmp(existing, json_string, strlen(json_string)) != 0) {
            ESP_LOGI(TAG, "SSID %s was changed or is new. Committing to flash", item->ssid);
            err = network_wifi_write_ap(item->ssid, json_string, 0);
            if (sta) {
                char sta_ssid[sizeof(sta->ssid) + 1];
                UINT_TO_STRING(sta_ssid, sta->ssid);
                if (strlen(sta_ssid) > 0 && strcmp(sta_ssid, item->ssid) == 0) {
                    ESP_LOGI(TAG, "Committing active access point");
                    err = network_wifi_write_nvs("ssid", sta_ssid, 0);
                    if (err == ESP_OK) {
                        char sta_pwd[sizeof(sta->password) + 1];
                        UINT_TO_STRING(sta_pwd, sta->password);
                        char* enc_pw = encrypt_wifi_credentials(STR_OR_BLANK(sta_pwd));
                        err = network_wifi_write_nvs("password", enc_pw ? enc_pw : "", 0);
                        FREE_AND_NULL(enc_pw);
                    }
                    if (err != ESP_OK) {
                        ESP_LOGE(TAG, "Error committing active access point : %s", esp_err_to_name(err));
                    }
                }
            }
        }
        FREE_AND_NULL(existing);
        FREE_AND_NULL(json_string);
    }
    return err;
}

esp_netif_t* network_wifi_get_interface() {
    return wifi_netif;
}
esp_netif_t* network_wifi_get_ap_interface() {
    return wifi_ap_netif;
}
static void network_wifi_apply_country(void) {
    char* country_code = config_alloc_get_default(NVS_TYPE_STR, "country_code", NULL, 0);
    if (!country_code) {
        country_code = config_alloc_get_default(NVS_TYPE_STR, "country", "01", 0);
    }
    wifi_country_t country = {
        .cc = "01",
        .schan = 1,
        .nchan = 14,
        .policy = WIFI_COUNTRY_POLICY_AUTO,
    };
    if (country_code && strlen(country_code) >= 2) {
        strlcpy(country.cc, country_code, sizeof(country.cc));
    }
    FREE_AND_NULL(country_code);
    esp_err_t err = esp_wifi_set_country(&country);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "Failed to set wifi country (%s): %s", country.cc, esp_err_to_name(err));
    } else {
        ESP_LOGI(TAG, "Wi-Fi country set to %s", country.cc);
    }
}
esp_err_t network_wifi_set_sta_mode() {
    if (!wifi_netif) {
        ESP_LOGE(TAG, "Wifi not initialized. Cannot set sta mode");
        return ESP_ERR_INVALID_STATE;
    }
    ESP_LOGD(TAG, "Set Mode to STA");
    esp_err_t err = esp_wifi_set_mode(WIFI_MODE_STA);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Error setting mode to STA: %s", esp_err_to_name(err));
    } else {
        ESP_LOGI(TAG, "Starting wifi");
        err = esp_wifi_start();
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "Error starting wifi: %s", esp_err_to_name(err));
        } else {
            esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
        }
    }
    return err;
}
esp_netif_t* network_wifi_start() {
    MEMTRACE_PRINT_DELTA_MESSAGE( "Starting wifi interface as STA mode");
    accessp_cjson = network_manager_clear_ap_list_json(&accessp_cjson);
    if (!wifi_netif) {
        MEMTRACE_PRINT_DELTA_MESSAGE("Init STA mode - creating default interface. ");
        wifi_netif = esp_netif_create_default_wifi_sta();
        MEMTRACE_PRINT_DELTA_MESSAGE("Initializing Wifi. ");
        wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
        ESP_ERROR_CHECK_WITHOUT_ABORT(esp_wifi_init(&cfg));
        MEMTRACE_PRINT_DELTA_MESSAGE("Registering wifi Handlers");
        //network_wifi_register_handlers();
        ESP_ERROR_CHECK_WITHOUT_ABORT(esp_event_handler_instance_register(WIFI_EVENT,
                                                                          ESP_EVENT_ANY_ID,
                                                                          &network_wifi_event_handler,
                                                                          NULL,
                                                                          NULL));
        MEMTRACE_PRINT_DELTA_MESSAGE("Setting up wifi Storage");
        ESP_ERROR_CHECK_WITHOUT_ABORT(esp_wifi_set_storage(WIFI_STORAGE_RAM));
        network_wifi_apply_country();
    }
    MEMTRACE_PRINT_DELTA_MESSAGE("Setting up wifi mode as STA");
    network_wifi_set_sta_mode();
    MEMTRACE_PRINT_DELTA_MESSAGE("Setting hostname");
    network_set_hostname(wifi_netif);
    MEMTRACE_PRINT_DELTA_MESSAGE("Done starting wifi interface");
    return wifi_netif;
}
void destroy_network_wifi() {
    cJSON_Delete(accessp_cjson);
    accessp_cjson = NULL;
}

bool network_wifi_sta_config_changed() {
    bool changed = true;
    const wifi_sta_config_t* sta = network_wifi_get_active_config();
    if (!sta)
        return false;
    char sta_ssid[sizeof(sta->ssid) + 1];
    char sta_pwd[sizeof(sta->password) + 1];
    UINT_TO_STRING(sta_ssid, sta->ssid);
    UINT_TO_STRING(sta_pwd, sta->password);
    if (strlen(sta_ssid) == 0)
        return false;

    known_access_point_t* known = network_wifi_get_ap_entry(sta_ssid);
    if (known && strcmp(known->ssid, sta_ssid) == 0 &&
        strcmp((char*)known->password, sta_pwd) == 0) {
        changed = false;
    } else {
        ESP_LOGI(TAG, "New network configuration found");
    }
    return changed;
}

esp_err_t network_wifi_save_sta_config() {
    esp_err_t esp_err = ESP_OK;
    known_access_point_t* item = NULL;
    MEMTRACE_PRINT_DELTA_MESSAGE("Config Save");

    const wifi_sta_config_t* sta = network_wifi_get_active_config();
    if (sta) {
        char sta_ssid[sizeof(sta->ssid) + 1];
        UINT_TO_STRING(sta_ssid, sta->ssid);
        if (strlen(sta_ssid) > 0) {
            MEMTRACE_PRINT_DELTA_MESSAGE("Checking if current SSID is known");
            item = network_wifi_get_ap_entry(sta_ssid);
            if (!item) {
                ESP_LOGD(TAG,"New SSID %s found", sta_ssid);
                // this is a new access point. First add it to the end of the AP list
                esp_err = network_wifi_add_ap_from_sta_copy(sta);
            }
        }
    }
    // now traverse the list and commit
    MEMTRACE_PRINT_DELTA_MESSAGE("Saving all known ap as json strings");
    known_access_point_t* it;
    ap_list_lock();
    SLIST_FOREACH(it, &s_ap_list, next) {
        if ((esp_err = network_wifi_store_ap_json(it)) != ESP_OK) {
            ESP_LOGW(TAG, "Error saving wifi ap entry %s : %s", it->ssid, esp_err_to_name(esp_err));
            break;
        }
    }
    ap_list_unlock();
    return esp_err;
}

void network_wifi_load_known_access_points() {
    esp_err_t esp_err;
    size_t size = 0;
    if (network_wifi_get_known_count() > 0) {
        ESP_LOGW(TAG, "Access points already loaded");
        return;
    }
    nvs_iterator_t it = nvs_entry_find(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace, NVS_TYPE_ANY);
    if (it == NULL) {
        ESP_LOGW(TAG, "No known access point found");
        return;
    }
    do {
        nvs_entry_info_t info;
        nvs_entry_info(it, &info);
        if (strstr(info.namespace_name, ap_list_nsv_namespace)) {
            void* value = get_nvs_value_alloc_for_partition(NVS_DEFAULT_PART_NAME, ap_list_nsv_namespace, info.type, info.key, &size);
            if (value == NULL) {
                ESP_LOGE(TAG, "nvs read failed for %s.", info.key);
            } else if ((esp_err = network_wifi_add_json_entry(value)) != ESP_OK) {
                ESP_LOGE(TAG, "Invalid entry or error for %s.", info.key);
            }
            FREE_AND_NULL(value);
        }
        it = nvs_entry_next(it);
    } while (it != NULL);

    return;
}

esp_err_t network_wifi_get_blob(void* target, size_t size, const char* key) {
    esp_err_t esp_err = ESP_OK;
    size_t found_size = 0;
    if (!target) {
        ESP_LOGE(TAG, "%s invalid target pointer", __FUNCTION__);
        return ESP_ERR_INVALID_ARG;
    }
    memset(target, 0x00, size);
    char* value = (char*)get_nvs_value_alloc_for_partition(NVS_DEFAULT_PART_NAME, network_wifi_nvs_namespace, NVS_TYPE_BLOB, key, &found_size);
    if (!value) {
        ESP_LOGD(TAG,"nvs key %s not found.", key);
        esp_err = ESP_FAIL;
    } else {
        memcpy((char*)target, value, size > found_size ? found_size : size);
        FREE_AND_NULL(value);
        ESP_LOGD(TAG,"Successfully loaded key %s", key);
    }
    return esp_err;
}
const wifi_sta_config_t* network_wifi_load_active_config() {
    static wifi_sta_config_t config;
    esp_err_t esp_err = ESP_OK;
    memset(&config, 0x00, sizeof(config));
    config.scan_method = WIFI_ALL_CHANNEL_SCAN;
    MEMTRACE_PRINT_DELTA_MESSAGE("Fetching wifi sta config - ssid.");
    esp_err = network_wifi_get_blob(&config.ssid, sizeof(config.ssid), "ssid");
    if (esp_err == ESP_OK && strlen((char*)config.ssid) > 0) {
        char cfg_ssid[sizeof(config.ssid) + 1];
        UINT_TO_STRING(cfg_ssid, config.ssid);
        ESP_LOGD(TAG,"network_wifi_load_active_config: ssid:%s. Fetching password (if any) ", cfg_ssid);
        char raw_pwd[sizeof(config.password) * 2 + 32];
        memset(raw_pwd, 0, sizeof(raw_pwd));
        if (network_wifi_get_blob(raw_pwd, sizeof(raw_pwd) - 1, "password") != ESP_OK) {
            ESP_LOGW(TAG, "No wifi password found in nvs");
        } else {
            char* dec_pw = decrypt_wifi_credentials(raw_pwd);
            if (dec_pw) {
                strlcpy((char*)config.password, dec_pw, sizeof(config.password));
                memset(dec_pw, 0, strlen(dec_pw));
                free(dec_pw);
            }
        }
        memset(raw_pwd, 0, sizeof(raw_pwd));
    } else {
        if(network_wifi_get_known_count() > 0) {
            ESP_LOGW(TAG, "No wifi ssid found in nvs, but known access points found. Using first known access point.");
            known_access_point_t* ap = SLIST_FIRST(&s_ap_list);
            if (ap) {
                strncpy((char*)&config.ssid, ap->ssid, sizeof(config.ssid));
                strncpy((char*)&config.password, ap->password, sizeof(config.password));
            }
            esp_err = ESP_OK;
        } else {
            ESP_LOGW(TAG, "network manager has no previous configuration. %s", esp_err_to_name(esp_err));
            return NULL;
        }
    }
    return &config;
}
bool network_wifi_load_wifi_sta_config() {
    network_wifi_load_known_access_points();
    const wifi_sta_config_t* config = network_wifi_load_active_config();
    if (config) {
        char cfg_ssid[sizeof(config->ssid) + 1];
        UINT_TO_STRING(cfg_ssid, config->ssid);
        known_access_point_t* item = network_wifi_get_ap_entry(cfg_ssid);
        if (!item) {
            ESP_LOGI(TAG, "Adding legacy/active wifi connection to the known list");
            network_wifi_add_ap_from_sta_copy(config);
        }
    }
    return config && config->ssid[0] != '\0';
}
bool network_wifi_get_config_for_ssid(wifi_config_t* config, const char* ssid) {
    known_access_point_t* item = network_wifi_get_ap_entry(ssid);
    if (!item) {
        ESP_LOGE(TAG, "Unknown ssid %s", ssid);
        return false;
    }
    wifi_sta_config_t* sta = &config->sta;
    memset(config, 0x00, sizeof(wifi_config_t));
    if (item->ssid) {
        strlcpy((char*)sta->ssid, item->ssid, sizeof(sta->ssid));
    }
    if (item->password) {
        strlcpy((char*)sta->password, item->password, sizeof(sta->password));
    }
    sta->scan_method = WIFI_ALL_CHANNEL_SCAN;
    return true;
}

static void network_wifi_event_handler(void* arg, esp_event_base_t event_base, int32_t event_id, void* event_data) {
    if (event_base != WIFI_EVENT)
        return;
    switch (event_id) {

        case WIFI_EVENT_WIFI_READY:
            ESP_LOGD(TAG, "WIFI_EVENT_WIFI_READY");
            break;

        case WIFI_EVENT_SCAN_DONE:
            ESP_LOGD(TAG, "WIFI_EVENT_SCAN_DONE");
            network_async_scan_done();
            break;

        case WIFI_EVENT_STA_AUTHMODE_CHANGE:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_AUTHMODE_CHANGE");
            break;

        case WIFI_EVENT_AP_START:
            ESP_LOGD(TAG, "WIFI_EVENT_AP_START");
            break;

        case WIFI_EVENT_AP_STOP:
            ESP_LOGD(TAG, "WIFI_EVENT_AP_STOP");
            break;

        case WIFI_EVENT_AP_PROBEREQRECVED: {
            wifi_event_ap_probe_req_rx_t* s = (wifi_event_ap_probe_req_rx_t*)event_data;
            char* mac = network_manager_alloc_get_mac_string(s->mac);
            if (mac) {
                ESP_LOGD(TAG, "WIFI_EVENT_AP_PROBEREQRECVED. RSSI: %d, MAC: %s", s->rssi, STR_OR_BLANK(mac));
            }
            FREE_AND_NULL(mac);
        } break;
        case WIFI_EVENT_STA_WPS_ER_SUCCESS:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_WPS_ER_SUCCESS");
            break;
        case WIFI_EVENT_STA_WPS_ER_FAILED:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_WPS_ER_FAILED");
            break;
        case WIFI_EVENT_STA_WPS_ER_TIMEOUT:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_WPS_ER_TIMEOUT");
            break;
        case WIFI_EVENT_STA_WPS_ER_PIN:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_WPS_ER_PIN");
            break;
        case WIFI_EVENT_AP_STACONNECTED: {
            wifi_event_ap_staconnected_t* stac = (wifi_event_ap_staconnected_t*)event_data;
            char* mac = network_manager_alloc_get_mac_string(stac->mac);
            if (mac) {
                ESP_LOGD(TAG, "WIFI_EVENT_AP_STACONNECTED. aid: %d, mac: %s", stac->aid, STR_OR_BLANK(mac));
            }
            FREE_AND_NULL(mac);
        } break;
        case WIFI_EVENT_AP_STADISCONNECTED:
            ESP_LOGD(TAG, "WIFI_EVENT_AP_STADISCONNECTED");
            break;

        case WIFI_EVENT_STA_START:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_START");
            break;

        case WIFI_EVENT_STA_STOP:
            ESP_LOGD(TAG, "WIFI_EVENT_STA_STOP");
            /* LAT-07: Restore power save when station stops / idle */
            esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
            break;

        case WIFI_EVENT_STA_CONNECTED: {
            ESP_LOGD(TAG, "WIFI_EVENT_STA_CONNECTED. ");
            wifi_event_sta_connected_t* s = (wifi_event_sta_connected_t*)event_data;
            char* bssid = network_manager_alloc_get_mac_string(s->bssid);
            char* ssid = strdup_psram((char*)s->ssid);
            if (bssid && ssid) {
                ESP_LOGD(TAG, "WIFI_EVENT_STA_CONNECTED. Channel: %d, Access point: %s, BSSID: %s ", s->channel, STR_OR_BLANK(ssid), (bssid));
            }
            FREE_AND_NULL(bssid);
            FREE_AND_NULL(ssid);
            /* LAT-07: Disable modem sleep / WiFi power save during connected state
             * to eliminate 100-300ms DTIM sleep latency and audio packet dropouts */
            esp_err_t ps_err = esp_wifi_set_ps(WIFI_PS_NONE);
            if (ps_err != ESP_OK) {
                ESP_LOGW(TAG, "Failed to set WiFi power save to WIFI_PS_NONE: %s", esp_err_to_name(ps_err));
            } else {
                ESP_LOGI(TAG, "WiFi power save set to WIFI_PS_NONE (modem sleep disabled for low-latency streaming)");
            }
            network_async(EN_CONNECTED);

        } break;

        case WIFI_EVENT_STA_DISCONNECTED: {
            //		    		structwifi_event_sta_disconnected_t
            //		    		Argument structure for WIFI_EVENT_STA_DISCONNECTED event
            //
            //		    		Public Members
            //
            //		    		uint8_t ssid[32]
            //		    		SSID of disconnected AP
            //
            //		    		uint8_t ssid_len
            //		    		SSID length of disconnected AP
            //
            //		    		uint8_t bssid[6]
            //		    		BSSID of disconnected AP
            //
            //		    		uint8_t reason
            //		    		reason of disconnection
            wifi_event_sta_disconnected_t* s = (wifi_event_sta_disconnected_t*)event_data;
            char* bssid = network_manager_alloc_get_mac_string(s->bssid);
            ESP_LOGW(TAG, "WIFI_EVENT_STA_DISCONNECTED. From BSSID: %s, reason code: %d (%s)", STR_OR_BLANK(bssid), s->reason, get_disconnect_code_desc(s->reason));
            FREE_AND_NULL(bssid);
            /* LAT-07: Restore power saving mode when disconnected / idle */
            esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
            if (s->reason == WIFI_REASON_ROAMING) {
                ESP_LOGI(TAG, "WiFi Roaming to new access point");
            } else {
                network_async_lost_connection((wifi_event_sta_disconnected_t*)event_data);
            }
        } break;

        default:
            break;
    }
}

cJSON* network_wifi_get_new_array_json(cJSON** old) {
    ESP_LOGV(TAG, "network_wifi_get_new_array_json called");
    cJSON* root = *old;
    if (root != NULL) {
        cJSON_Delete(root);
        *old = NULL;
    }
    ESP_LOGV(TAG, "network_wifi_get_new_array_json done");
    return cJSON_CreateArray();
}
void network_wifi_global_init() {
    if (!s_ap_list_mutex) {
        s_ap_list_mutex = xSemaphoreCreateMutex();
    }
    network_wifi_get_new_array_json(&accessp_cjson);
    ESP_LOGD(TAG, "Loading existing wifi configuration (if any)");
    network_wifi_load_wifi_sta_config();
}
void network_wifi_add_access_point_json(cJSON* ap_list, wifi_ap_record_t* ap_rec) {
    cJSON* ap = cJSON_CreateObject();
    if (ap == NULL) {
        ESP_LOGE(TAG, "Unable to allocate memory for access point %s", ap_rec->ssid);
        return;
    }
    cJSON* radio = cJSON_CreateObject();
    if (radio == NULL) {
        ESP_LOGE(TAG, "Unable to allocate memory for access point %s", ap_rec->ssid);
        cJSON_Delete(ap);
        return;
    }
    char ap_rec_ssid[sizeof(ap_rec->ssid) + 1];
    UINT_TO_STRING(ap_rec_ssid, ap_rec->ssid);
    cJSON_AddItemToObject(ap, "ssid", cJSON_CreateString(ap_rec_ssid));
    cJSON_AddBoolToObject(ap, "known", network_wifi_is_known_ap(ap_rec_ssid));
    if (ap_rec->rssi != 0) {
        // only add the rest of the details when record doesn't come from
        // "known" access points that aren't in range
        cJSON_AddNumberToObject(ap, "chan", ap_rec->primary);
        cJSON_AddNumberToObject(ap, "rssi", ap_rec->rssi);
        cJSON_AddNumberToObject(ap, "auth", ap_rec->authmode);

        char* bssid = network_manager_alloc_get_mac_string(ap_rec->bssid);
        if (bssid) {
            cJSON_AddItemToObject(ap, "bssid", cJSON_CreateString(STR_OR_BLANK(bssid)));
        }
        FREE_AND_NULL(bssid);
        cJSON_AddNumberToObject(radio, "b", ap_rec->phy_11b ? 1 : 0);
        cJSON_AddNumberToObject(radio, "g", ap_rec->phy_11g ? 1 : 0);
        cJSON_AddNumberToObject(radio, "n", ap_rec->phy_11n ? 1 : 0);
        cJSON_AddNumberToObject(radio, "low_rate", ap_rec->phy_lr ? 1 : 0);
        cJSON_AddItemToObject(ap, "radio", radio);
    }
    cJSON_AddItemToArray(ap_list, ap);
    ESP_LOGD(TAG, "New access point added: %s", ap_rec_ssid);
}
void network_wifi_generate_access_points_json(cJSON** ap_list) {
    *ap_list = network_wifi_get_new_array_json(ap_list);
    wifi_ap_record_t known_ap;
    known_access_point_t* it;
    if (*ap_list == NULL)
        return;
    for (int i = 0; i < ap_num; i++) {
        network_wifi_add_access_point_json(*ap_list, &accessp_records[i]);
    }
    SLIST_FOREACH(it, &s_ap_list, next) {
        if (!network_wifi_was_ssid_seen(it->ssid)) {
            memset(&known_ap, 0x00, sizeof(known_ap));
            strlcpy((char*)known_ap.ssid, it->ssid, sizeof(known_ap.ssid));
            ESP_LOGD(TAG, "Adding known access point that is not in range: %s", it->ssid);
            network_wifi_add_access_point_json(*ap_list, &known_ap);
        }
    }
    char* ap_list_json = cJSON_PrintUnformatted(*ap_list);
    if (ap_list_json != NULL) {
        ESP_LOGV(TAG, "Full access point list: %s", ap_list_json);
        free(ap_list_json);
    }
}
void network_wifi_set_ipv4val(const char* key, char* default_value, ip4_addr_t* target) {
    char* value = config_alloc_get_default(NVS_TYPE_STR, key, default_value, 0);
    if (value != NULL) {
        ESP_LOGD(TAG, "%s: %s", key, value);
        inet_pton(AF_INET, value, target); /* access point is on a static IP */
    }
    FREE_AND_NULL(value);
}
esp_netif_t* network_wifi_config_ap() {
    esp_netif_ip_info_t info;
    esp_err_t err = ESP_OK;
    char* value = NULL;
    wifi_config_t ap_config = {
        .ap = {
            .ssid_len = 0,
        },
    };
    ESP_LOGI(TAG, "Configuring Access Point.");
    if (!wifi_ap_netif) {
        wifi_ap_netif = esp_netif_create_default_wifi_ap();
    }

    network_wifi_set_ipv4val("ap_ip_address", DEFAULT_AP_IP, (ip4_addr_t*)&info.ip);
    network_wifi_set_ipv4val("ap_ip_gateway", CONFIG_DEFAULT_AP_GATEWAY, (ip4_addr_t*)&info.gw);
    network_wifi_set_ipv4val("ap_ip_netmask", CONFIG_DEFAULT_AP_NETMASK, (ip4_addr_t*)&info.netmask);
    /* In order to change the IP info structure, we have to first stop 
     * the DHCP server on the new interface 
    */
    network_start_stop_dhcps(wifi_ap_netif, false);
    ESP_LOGD(TAG, "Setting tcp_ip info for access point");
    if ((err = esp_netif_set_ip_info(wifi_ap_netif, &info)) != ESP_OK) {
        ESP_LOGE(TAG, "Setting tcp_ip info for interface TCPIP_ADAPTER_IF_AP. Error %s", esp_err_to_name(err));
        return wifi_ap_netif;
    }
    network_start_stop_dhcps(wifi_ap_netif, true);

    /*
		 * Set Access Point configuration
		 */
    value = config_alloc_get_default(NVS_TYPE_STR, "ap_ssid", CONFIG_DEFAULT_AP_SSID, 0);
    if (value != NULL) {
        strlcpy((char*)ap_config.ap.ssid, value, sizeof(ap_config.ap.ssid));
        ESP_LOGI(TAG, "AP SSID: %s", (char*)ap_config.ap.ssid);
    }
    FREE_AND_NULL(value);

    value = config_alloc_get_default(NVS_TYPE_STR, "ap_pwd", DEFAULT_AP_PASSWORD, 0);
    if (value != NULL) {
        strlcpy((char*)ap_config.ap.password, value, sizeof(ap_config.ap.password));
        ESP_LOGI(TAG, "AP Password: [REDACTED]");
        memset(value, 0, strlen(value));
    }
    FREE_AND_NULL(value);

    value = config_alloc_get_default(NVS_TYPE_STR, "ap_channel", STR(CONFIG_DEFAULT_AP_CHANNEL), 0);
    if (value != NULL) {
        ESP_LOGD(TAG, "Channel: %s", value);
        ap_config.ap.channel = atoi(value);
    }
    FREE_AND_NULL(value);

    ap_config.ap.authmode = AP_AUTHMODE;
    ap_config.ap.ssid_hidden = DEFAULT_AP_SSID_HIDDEN;
    ap_config.ap.max_connection = DEFAULT_AP_MAX_CONNECTIONS;
    ap_config.ap.beacon_interval = DEFAULT_AP_BEACON_INTERVAL;

    ESP_LOGD(TAG, "Auth Mode: %d", ap_config.ap.authmode);
    ESP_LOGD(TAG, "SSID Hidden: %d", ap_config.ap.ssid_hidden);
    ESP_LOGD(TAG, "Max Connections: %d", ap_config.ap.max_connection);
    ESP_LOGD(TAG, "Beacon interval: %d", ap_config.ap.beacon_interval);

    network_wifi_apply_country();
    const char* msg = "Setting wifi mode as WIFI_MODE_APSTA";
    ESP_LOGD(TAG, "%s", msg);
    if ((err = esp_wifi_set_mode(WIFI_MODE_APSTA)) != ESP_OK) {
        ESP_LOGE(TAG, "%s. Error %s", msg, esp_err_to_name(err));
        return wifi_ap_netif;
    }
    msg = "Setting wifi AP configuration for WIFI_IF_AP";
    ESP_LOGD(TAG, "%s", msg);
    if ((err = esp_wifi_set_config(WIFI_IF_AP, &ap_config)) != ESP_OK) /* stop AP DHCP server */
    {
        memset(ap_config.ap.password, 0, sizeof(ap_config.ap.password));
        ESP_LOGE(TAG, "%s . Error %s", msg, esp_err_to_name(err));
        return wifi_ap_netif;
    }
    memset(ap_config.ap.password, 0, sizeof(ap_config.ap.password));

    msg = "Setting wifi bandwidth";
    ESP_LOGD(TAG, "%s (%d)", msg, DEFAULT_AP_BANDWIDTH);
    if ((err = esp_wifi_set_bandwidth(WIFI_IF_AP, DEFAULT_AP_BANDWIDTH)) != ESP_OK) /* stop AP DHCP server */
    {
        ESP_LOGE(TAG, "%s failed. Error %s", msg, esp_err_to_name(err));
        return wifi_ap_netif;
    }

    msg = "Setting wifi power save";
    ESP_LOGD(TAG, "%s (%d)", msg, DEFAULT_STA_POWER_SAVE);

    if ((err = esp_wifi_set_ps(DEFAULT_STA_POWER_SAVE)) != ESP_OK) /* stop AP DHCP server */
    {
        ESP_LOGE(TAG, "%s failed. Error %s", msg, esp_err_to_name(err));
        return wifi_ap_netif;
    }

    ESP_LOGD(TAG, "Done configuring Soft Access Point");
    return wifi_ap_netif;
}

void network_wifi_filter_unique(wifi_ap_record_t* aplist, uint16_t* aps) {
    int total_unique;
    wifi_ap_record_t* first_free;
    total_unique = *aps;

    first_free = NULL;

    for (int i = 0; i < *aps - 1; i++) {
        wifi_ap_record_t* ap = &aplist[i];

        /* skip the previously removed APs */
        if (ap->ssid[0] == 0)
            continue;

        /* remove the identical SSID+authmodes */
        for (int j = i + 1; j < *aps; j++) {
            wifi_ap_record_t* ap1 = &aplist[j];
            if ((strcmp((const char*)ap->ssid, (const char*)ap1->ssid) == 0) &&
                (ap->authmode == ap1->authmode)) { /* same SSID, different auth mode is skipped */
                /* save the rssi for the display */
                if ((ap1->rssi) > (ap->rssi))
                    ap->rssi = ap1->rssi;
                /* clearing the record */
                memset(ap1, 0, sizeof(wifi_ap_record_t));
            }
        }
    }
    /* reorder the list so APs follow each other in the list */
    for (int i = 0; i < *aps; i++) {
        wifi_ap_record_t* ap = &aplist[i];
        /* skipping all that has no name */
        if (ap->ssid[0] == 0) {
            /* mark the first free slot */
            if (first_free == NULL)
                first_free = ap;
            total_unique--;
            continue;
        }
        if (first_free != NULL) {
            memcpy(first_free, ap, sizeof(wifi_ap_record_t));
            memset(ap, 0, sizeof(wifi_ap_record_t));
            /* find the next free slot */
            for (int j = 0; j < *aps; j++) {
                if (aplist[j].ssid[0] == 0) {
                    first_free = &aplist[j];
                    break;
                }
            }
        }
    }
    /* update the length of the list */
    *aps = total_unique;
}

char* network_status_alloc_get_ap_list_json() {
    char* str = NULL;
    if (network_status_lock_json_buffer(pdMS_TO_TICKS(1000))) {
        str = cJSON_PrintUnformatted(accessp_cjson);
        network_status_unlock_json_buffer();
    }
    return str;
}
cJSON* network_manager_clear_ap_list_json(cJSON** old) {
    ESP_LOGV(TAG, "network_manager_clear_ap_list_json called");
    cJSON* root = network_wifi_get_new_array_json(old);
    ESP_LOGV(TAG, "network_manager_clear_ap_list_json done");
    return root;
}

esp_err_t network_wifi_built_known_ap_list() {
    if (network_status_lock_json_buffer(pdMS_TO_TICKS(1000))) {
        ESP_LOGD(TAG,"Building known AP list");
        accessp_cjson = network_manager_clear_ap_list_json(&accessp_cjson);
        network_wifi_generate_access_points_json(&accessp_cjson);
        network_status_unlock_json_buffer();
        ESP_LOGD(TAG, "Done building ap JSON list");
    } else {
        ESP_LOGE(TAG, "Failed to lock json buffer");
        return ESP_FAIL;
    }
    return ESP_OK;
}

esp_err_t wifi_scan_done() {
    esp_err_t err = ESP_OK;
    /* As input param, it stores max AP number ap_records can hold. As output param, it receives the actual AP number this API returns.
				 * As a consequence, ap_num MUST be reset to MAX_AP_NUM at every scan */
    ESP_LOGD(TAG, "Getting AP list records");
    uint16_t scan_ap_num = MAX_AP_NUM;
    if ((err = esp_wifi_scan_get_ap_num(&scan_ap_num)) != ESP_OK) {
        ESP_LOGE(TAG, "Failed to retrieve scan results count. Error %s", esp_err_to_name(err));
        return err;
    }
    wifi_ap_record_t* new_records = NULL;
    if (scan_ap_num > 0) {
        new_records = (wifi_ap_record_t*)malloc_init_external(sizeof(wifi_ap_record_t) * scan_ap_num);
        if (!new_records) {
            ESP_LOGE(TAG, "Memory allocation failed for scan records");
            return ESP_ERR_NO_MEM;
        }
        if ((err = esp_wifi_scan_get_ap_records(&scan_ap_num, new_records)) != ESP_OK) {
            ESP_LOGE(TAG, "Failed to retrieve scan results list. Error %s", esp_err_to_name(err));
            free(new_records);
            return err;
        }
    }

    /* make sure the http server isn't trying to access the list while it gets refreshed */
    ESP_LOGD(TAG, "Preparing to build ap JSON list");
    if (network_status_lock_json_buffer(pdMS_TO_TICKS(1000))) {
        FREE_AND_NULL(accessp_records);
        accessp_records = new_records;
        ap_num = scan_ap_num;

        if (ap_num > 0) {
            /* Will remove the duplicate SSIDs from the list and update ap_num */
            network_wifi_filter_unique(accessp_records, &ap_num);
            network_wifi_set_found_ap();
            network_wifi_generate_access_points_json(&accessp_cjson);
            ESP_LOGD(TAG, "Done building ap JSON list");
        } else {
            ESP_LOGD(TAG, "No AP Found.  Emptying the list.");
            accessp_cjson = network_wifi_get_new_array_json(&accessp_cjson);
        }
        network_status_unlock_json_buffer();
    } else {
        ESP_LOGE(TAG, "could not get access to json mutex in wifi_scan");
        if (new_records) {
            free(new_records);
        }
        err = ESP_FAIL;
    }
    return err;
}
bool is_wifi_up() {
    return wifi_netif != NULL;
}
esp_err_t network_wifi_start_scan() {
    wifi_scan_config_t scan_config = {
        .ssid = 0,
        .bssid = 0,
        .channel = 0,
        .scan_type = WIFI_SCAN_TYPE_ACTIVE,
        .show_hidden = true};
    esp_err_t err = ESP_OK;
    ESP_LOGI(TAG, "Initiating wifi network scan");
    if (!is_wifi_up()) {
        messaging_post_message(MESSAGING_WARNING, MESSAGING_CLASS_SYSTEM, "Wifi not started. Cannot scan");
        return ESP_FAIL;
    }
    /* if a scan is already in progress this message is simply ignored thanks to the WIFI_MANAGER_SCAN_BIT uxBit */
    if ((err = esp_wifi_scan_start(&scan_config, false)) != ESP_OK) {
        ESP_LOGW(TAG, "Unable to start scan; %s ", esp_err_to_name(err));
        //						set_status_message(WARNING, "Wifi Connecting. Cannot start scan.");
        messaging_post_message(MESSAGING_WARNING, MESSAGING_CLASS_SYSTEM, "Scanning failed: %s", esp_err_to_name(err));
    }
    return err;
}

bool network_wifi_is_ap_mode() {
    wifi_mode_t mode;
    /* update config to latest and attempt connection */
    return esp_wifi_get_mode(&mode) == ESP_OK && mode == WIFI_MODE_AP;
}
bool network_wifi_is_sta_mode() {
    wifi_mode_t mode;
    /* update config to latest and attempt connection */
    return esp_wifi_get_mode(&mode) == ESP_OK && mode == WIFI_MODE_STA;
}
bool network_wifi_is_ap_sta_mode() {
    wifi_mode_t mode;
    /* update config to latest and attempt connection */
    return esp_wifi_get_mode(&mode) == ESP_OK && mode == WIFI_MODE_APSTA;
}

esp_err_t network_wifi_connect(const char* ssid, const char* password) {
    esp_err_t err = ESP_OK;
    wifi_config_t config;
    memset(&config, 0x00, sizeof(config));
    ESP_LOGD(TAG, "network_wifi_connect");
    if (!is_wifi_up()) {
        messaging_post_message(MESSAGING_WARNING, MESSAGING_CLASS_SYSTEM, "Wifi not started. Cannot connect");
        return ESP_FAIL;
    }
    if (!ssid || strlen(ssid) == 0) {
        ESP_LOGE(TAG, "Cannot connect wifi. wifi config is null!");
        return ESP_ERR_INVALID_ARG;
    }

    wifi_mode_t wifi_mode;
    err = esp_wifi_get_mode(&wifi_mode);
    if (err == ESP_ERR_WIFI_NOT_INIT) {
        ESP_LOGW(TAG, "Wifi not initialized. Attempting to start sta mode");
        network_wifi_start();
    } else if (err != ESP_OK) {
        ESP_LOGE(TAG, "Could not retrieve wifi mode : %s", esp_err_to_name(err));
    } else if (wifi_mode != WIFI_MODE_STA && wifi_mode != WIFI_MODE_APSTA) {
        ESP_LOGD(TAG, "Changing wifi mode to STA");
        err = network_wifi_set_sta_mode();
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "Could not set mode to STA.  Cannot connect to SSID %s", ssid);
            return err;
        }
    }
    // copy configuration and connect
    strlcpy((char*)config.sta.ssid, ssid, sizeof(config.sta.ssid));
    if (password) {
        strlcpy((char*)config.sta.password, password, sizeof(config.sta.password));
    }

    // First Disconnect
    esp_wifi_disconnect();

    config.sta.scan_method = WIFI_ALL_CHANNEL_SCAN;
    if ((err = esp_wifi_set_config(WIFI_IF_STA, &config)) != ESP_OK) {
        ESP_LOGE(TAG, "Failed to set STA configuration. Error %s", esp_err_to_name(err));
    }
    memset(config.sta.password, 0, sizeof(config.sta.password));
    if (err == ESP_OK) {
        ESP_LOGI(TAG, "Wifi Connecting to %s...", ssid);
        if ((err = esp_wifi_connect()) != ESP_OK) {
            ESP_LOGE(TAG, "Failed to initiate wifi connection. Error %s", esp_err_to_name(err));
        }
    }
    return err;
}
esp_err_t network_wifi_connect_next_in_range(){
    const char * ssid = network_wifi_get_next_ap_in_range();
    if(ssid){
        return network_wifi_connect_ssid(ssid);
    }
    return ESP_FAIL;
}
esp_err_t network_wifi_connect_ssid(const char* ssid) {
    known_access_point_t* item = network_wifi_get_ap_entry(ssid);
    if (item) {
        item->last_try = (esp_timer_get_time() / 1000);
        return network_wifi_connect(item->ssid, item->password);
    }
    return ESP_FAIL;
}
esp_err_t network_wifi_connect_active_ssid() {
    const wifi_sta_config_t* config = network_wifi_load_active_config();
    if (config) {
        char cfg_ssid[sizeof(config->ssid) + 1];
        char cfg_pwd[sizeof(config->password) + 1];
        UINT_TO_STRING(cfg_ssid, config->ssid);
        UINT_TO_STRING(cfg_pwd, config->password);
        return network_wifi_connect(cfg_ssid, cfg_pwd);
    }
    return ESP_FAIL;
}
void network_wifi_clear_config() {
    /* erase configuration */
    const wifi_sta_config_t* sta = network_wifi_get_active_config();
    if (sta) {
        char sta_ssid[sizeof(sta->ssid) + 1];
        UINT_TO_STRING(sta_ssid, sta->ssid);
        network_wifi_delete_ap(sta_ssid);
    }
    esp_err_t err = ESP_OK;
    if ((err = esp_wifi_disconnect()) != ESP_OK) {
        ESP_LOGW(TAG, "Could not disconnect from deleted network : %s", esp_err_to_name(err));
    }
    /* LAT-07: Restore power save when explicitly disconnected / idle */
    esp_wifi_set_ps(WIFI_PS_MIN_MODEM);
}

char* get_disconnect_code_desc(uint8_t reason) {
    switch (reason) {
        ENUM_TO_STRING(WIFI_REASON_UNSPECIFIED);
        ENUM_TO_STRING(WIFI_REASON_AUTH_EXPIRE);
        ENUM_TO_STRING(WIFI_REASON_AUTH_LEAVE);
        ENUM_TO_STRING(WIFI_REASON_ASSOC_EXPIRE);
        ENUM_TO_STRING(WIFI_REASON_ASSOC_TOOMANY);
        ENUM_TO_STRING(WIFI_REASON_NOT_AUTHED);
        ENUM_TO_STRING(WIFI_REASON_NOT_ASSOCED);
        ENUM_TO_STRING(WIFI_REASON_ASSOC_LEAVE);
        ENUM_TO_STRING(WIFI_REASON_ASSOC_NOT_AUTHED);
        ENUM_TO_STRING(WIFI_REASON_DISASSOC_PWRCAP_BAD);
        ENUM_TO_STRING(WIFI_REASON_DISASSOC_SUPCHAN_BAD);
        ENUM_TO_STRING(WIFI_REASON_IE_INVALID);
        ENUM_TO_STRING(WIFI_REASON_MIC_FAILURE);
        ENUM_TO_STRING(WIFI_REASON_4WAY_HANDSHAKE_TIMEOUT);
        ENUM_TO_STRING(WIFI_REASON_GROUP_KEY_UPDATE_TIMEOUT);
        ENUM_TO_STRING(WIFI_REASON_IE_IN_4WAY_DIFFERS);
        ENUM_TO_STRING(WIFI_REASON_GROUP_CIPHER_INVALID);
        ENUM_TO_STRING(WIFI_REASON_PAIRWISE_CIPHER_INVALID);
        ENUM_TO_STRING(WIFI_REASON_AKMP_INVALID);
        ENUM_TO_STRING(WIFI_REASON_UNSUPP_RSN_IE_VERSION);
        ENUM_TO_STRING(WIFI_REASON_INVALID_RSN_IE_CAP);
        ENUM_TO_STRING(WIFI_REASON_802_1X_AUTH_FAILED);
        ENUM_TO_STRING(WIFI_REASON_CIPHER_SUITE_REJECTED);
        ENUM_TO_STRING(WIFI_REASON_INVALID_PMKID);
        ENUM_TO_STRING(WIFI_REASON_BEACON_TIMEOUT);
        ENUM_TO_STRING(WIFI_REASON_NO_AP_FOUND);
        ENUM_TO_STRING(WIFI_REASON_AUTH_FAIL);
        ENUM_TO_STRING(WIFI_REASON_ASSOC_FAIL);
        ENUM_TO_STRING(WIFI_REASON_HANDSHAKE_TIMEOUT);
        ENUM_TO_STRING(WIFI_REASON_CONNECTION_FAIL);
        ENUM_TO_STRING(WIFI_REASON_AP_TSF_RESET);
        ENUM_TO_STRING(WIFI_REASON_ROAMING);
    }
    return "";
}
