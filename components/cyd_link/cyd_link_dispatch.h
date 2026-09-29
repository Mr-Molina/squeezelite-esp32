#pragma once
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

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
