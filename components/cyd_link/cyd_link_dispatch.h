#pragma once
#include <stdint.h>
#include <stdbool.h>
#if defined(ESP_PLATFORM)
#include "esp_err.h"
#else
typedef int esp_err_t;
#ifndef ESP_OK
#define ESP_OK 0
#endif
#ifndef ESP_FAIL
#define ESP_FAIL -1
#endif
#ifndef ESP_ERR_NO_MEM
#define ESP_ERR_NO_MEM 0x101
#endif
#ifndef ESP_ERR_INVALID_ARG
#define ESP_ERR_INVALID_ARG 0x102
#endif
#ifndef ESP_ERR_INVALID_STATE
#define ESP_ERR_INVALID_STATE 0x103
#endif
#ifndef ESP_ERR_TIMEOUT
#define ESP_ERR_TIMEOUT 0x107
#endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    CYD_CMD_UNKNOWN = 0,
    CYD_CMD_TOGGLE,
    CYD_CMD_PLAY,
    CYD_CMD_PAUSE,
    CYD_CMD_NEXT,
    CYD_CMD_PREV,
    CYD_CMD_VOLUME,
    CYD_CMD_VOL_STEP,
    CYD_CMD_SYNC
} cyd_cmd_type_t;

typedef struct {
    cyd_cmd_type_t type;
    int32_t param;
} cyd_command_t;

esp_err_t cyd_link_parse_command(const char *json_str, cyd_command_t *out_cmd);
char *cyd_link_format_meta(const char *title, const char *artist, const char *album);
char *cyd_link_format_status(const char *state, uint32_t elapsed, uint32_t duration, uint8_t vol);
char *cyd_link_format_sys(const char *mode, const char *name, const char *ip);

#ifdef __cplusplus
}
#endif
