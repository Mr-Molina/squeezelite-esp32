#include "cyd_link_hooks.h"
#include "cyd_link.h"
#include "cyd_link_dispatch.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

#if defined(ESP_PLATFORM)
#include "audio_controls.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "platform_config.h"
#include "network_status.h"
static SemaphoreHandle_t s_cache_mutex = NULL;
#else
// Host test stubs
typedef enum {
    ACTRLS_NONE = -1, ACTRLS_POWER, ACTRLS_VOLUP, ACTRLS_VOLDOWN, ACTRLS_TOGGLE, ACTRLS_PLAY,
    ACTRLS_PAUSE, ACTRLS_STOP, ACTRLS_REW, ACTRLS_FWD, ACTRLS_PREV, ACTRLS_NEXT,
    BCTRLS_UP, BCTRLS_DOWN, BCTRLS_LEFT, BCTRLS_RIGHT,
    BCTRLS_PS0,BCTRLS_PS1,BCTRLS_PS2,BCTRLS_PS3,BCTRLS_PS4,BCTRLS_PS5,BCTRLS_PS6,BCTRLS_PS7,BCTRLS_PS8,BCTRLS_PS9,
    KNOB_LEFT, KNOB_RIGHT, KNOB_PUSH,
    ACTRLS_SLEEP,
    ACTRLS_REMAP, ACTRLS_MAX
} actrls_action_e;
typedef void (*actrls_handler)(bool pressed);
actrls_handler get_ctrl_handler(actrls_action_e action);

#ifndef ESP_LOGW
#define ESP_LOGW(tag, fmt, ...) do { (void)(tag); } while(0)
#endif
#ifndef ESP_LOGI
#define ESP_LOGI(tag, fmt, ...) do { (void)(tag); } while(0)
#endif
#endif

// Weak fallback definition for output_volume so it links everywhere
__attribute__((weak)) void output_volume(uint8_t val) {
    (void)val;
}

#if !defined(ESP_PLATFORM)
__attribute__((weak)) actrls_handler get_ctrl_handler(actrls_action_e action) {
    (void)action;
    return NULL;
}
#endif

static const char *TAG __attribute__((unused)) = "cyd_hooks";
static char s_cached_title[128] = "Idle";
static char s_cached_artist[128] = "";
static char s_cached_album[128] = "";
static char s_cached_state[16] = "stop";
static uint32_t s_cached_elapsed = 0;
static uint32_t s_cached_duration = 0;
static uint8_t s_cached_vol = 50;

static void cache_lock(void) {
#if defined(ESP_PLATFORM)
    if (s_cache_mutex) xSemaphoreTake(s_cache_mutex, portMAX_DELAY);
#endif
}

static void cache_unlock(void) {
#if defined(ESP_PLATFORM)
    if (s_cache_mutex) xSemaphoreGive(s_cache_mutex);
#endif
}

void cyd_link_set_cached_meta(const char *title, const char *artist, const char *album) {
    cache_lock();
    if (title) {
        strncpy(s_cached_title, title, sizeof(s_cached_title) - 1);
        s_cached_title[sizeof(s_cached_title) - 1] = '\0';
    }
    if (artist) {
        strncpy(s_cached_artist, artist, sizeof(s_cached_artist) - 1);
        s_cached_artist[sizeof(s_cached_artist) - 1] = '\0';
    }
    if (album) {
        strncpy(s_cached_album, album, sizeof(s_cached_album) - 1);
        s_cached_album[sizeof(s_cached_album) - 1] = '\0';
    }
    cache_unlock();
}

void cyd_link_hook_metadata(const char *artist, const char *album, const char *title) {
    cyd_link_set_cached_meta(title, artist, album);
    cache_lock();
    char *msg = cyd_link_format_meta(s_cached_title, s_cached_artist, s_cached_album);
    cache_unlock();
    if (msg) {
        cyd_link_send_raw(msg);
        free(msg);
    }
}

void cyd_link_hook_timer(uint32_t elapsed, uint32_t duration) {
    cache_lock();
    s_cached_elapsed = elapsed;
    s_cached_duration = duration;
    char *msg = cyd_link_format_status(s_cached_state, s_cached_elapsed, s_cached_duration, s_cached_vol);
    cache_unlock();
    if (msg) {
        cyd_link_send_raw(msg);
        free(msg);
    }
}

void cyd_link_hook_playback_state(const char *state) {
    cache_lock();
    if (state) {
        strncpy(s_cached_state, state, sizeof(s_cached_state) - 1);
        s_cached_state[sizeof(s_cached_state) - 1] = '\0';
    }
    uint32_t el = s_cached_elapsed;
    uint32_t dur = s_cached_duration;
    cache_unlock();
    cyd_link_hook_timer(el, dur);
}

void cyd_link_broadcast_full_sync(void) {
    const char *mode = "LMS";
    const char *player = "Squeezelite-ESP32";
    char ip_buf[32] = "0.0.0.0";
#if defined(ESP_PLATFORM)
    char *sta_ip = network_status_get_sta_ip_string();
    if (sta_ip) {
        if (strlen(sta_ip) > 0) {
            strncpy(ip_buf, sta_ip, sizeof(ip_buf) - 1);
            ip_buf[sizeof(ip_buf) - 1] = '\0';
        }
    }
    char *name = config_alloc_get_str("player_name", NULL, NULL);
    if (name) {
        player = name;
    }
#endif

    char *sys_msg = cyd_link_format_sys(mode, player, ip_buf);
#if defined(ESP_PLATFORM)
    if (name) free(name);
#endif
    if (sys_msg) {
        cyd_link_send_raw(sys_msg);
        free(sys_msg);
    }

    cache_lock();
    char *meta_msg = cyd_link_format_meta(s_cached_title, s_cached_artist, s_cached_album);
    char *stat_msg = cyd_link_format_status(s_cached_state, s_cached_elapsed, s_cached_duration, s_cached_vol);
    cache_unlock();

    if (meta_msg) {
        cyd_link_send_raw(meta_msg);
        free(meta_msg);
    }
    if (stat_msg) {
        cyd_link_send_raw(stat_msg);
        free(stat_msg);
    }
}


static void on_cyd_command(cyd_cmd_type_t type, int32_t param) {
    actrls_handler handler = NULL;
    switch (type) {
        case CYD_CMD_TOGGLE:
            handler = get_ctrl_handler(ACTRLS_TOGGLE);
            if (handler) handler(true);
            break;
        case CYD_CMD_PLAY:
            handler = get_ctrl_handler(ACTRLS_PLAY);
            if (handler) handler(true);
            break;
        case CYD_CMD_PAUSE:
            handler = get_ctrl_handler(ACTRLS_PAUSE);
            if (handler) handler(true);
            break;
        case CYD_CMD_NEXT:
            handler = get_ctrl_handler(ACTRLS_NEXT);
            if (handler) handler(true);
            break;
        case CYD_CMD_PREV:
            handler = get_ctrl_handler(ACTRLS_PREV);
            if (handler) handler(true);
            break;
        case CYD_CMD_VOLUME:
            s_cached_vol = (uint8_t)param;
            output_volume(s_cached_vol);
            break;
        case CYD_CMD_VOL_STEP:
            handler = (param > 0) ? get_ctrl_handler(ACTRLS_VOLUP) : get_ctrl_handler(ACTRLS_VOLDOWN);
            if (handler) handler(true);
            break;
        case CYD_CMD_SYNC:
            cyd_link_broadcast_full_sync();
            break;
        default:
            ESP_LOGW(TAG, "Unhandled command type: %d", type);
            break;
    }
}

void cyd_link_hooks_init(void) {
#if defined(ESP_PLATFORM)
    if (!s_cache_mutex) {
        s_cache_mutex = xSemaphoreCreateMutex();
    }
#endif
    cyd_link_set_cmd_handler(on_cyd_command);
}
