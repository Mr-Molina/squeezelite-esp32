#include "cyd_link_dispatch.h"
#include "cJSON.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

static char *append_newline(char *json) {
    if (!json) return NULL;
    size_t len = strlen(json);
    char *out = malloc(len + 2);
    if (!out) {
        cJSON_free(json);
        return NULL;
    }
    memcpy(out, json, len);
    out[len] = '\n';
    out[len + 1] = '\0';
    cJSON_free(json);
    return out;
}

char *cyd_link_format_meta(const char *title, const char *artist, const char *album) {
    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;
    cJSON_AddStringToObject(root, "event", "meta");
    cJSON_AddStringToObject(root, "title", title ? title : "");
    cJSON_AddStringToObject(root, "artist", artist ? artist : "");
    cJSON_AddStringToObject(root, "album", album ? album : "");
    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}

char *cyd_link_format_status(const char *state, uint32_t elapsed, uint32_t duration, uint8_t vol) {
    char buf[128];
    int len = snprintf(buf, sizeof(buf),
                       "{\"event\":\"status\",\"state\":\"%s\",\"elapsed\":%u,\"duration\":%u,\"vol\":%u}\n",
                       state ? state : "stop",
                       (unsigned int)elapsed,
                       (unsigned int)duration,
                       (unsigned int)vol);
    if (len <= 0 || (size_t)len >= sizeof(buf)) {
        return NULL;
    }
    char *out = (char *)malloc((size_t)len + 1);
    if (!out) {
        return NULL;
    }
    memcpy(out, buf, (size_t)len + 1);
    return out;
}

char *cyd_link_format_sys(const char *mode, const char *name, const char *ip) {
    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;
    cJSON_AddStringToObject(root, "event", "sys");
    cJSON_AddStringToObject(root, "mode", mode ? mode : "idle");
    cJSON_AddStringToObject(root, "name", name ? name : "Squeezelite");
    cJSON_AddStringToObject(root, "ip", ip ? ip : "");
    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}

esp_err_t cyd_link_parse_command(const char *json_str, cyd_command_t *out_cmd) {
    if (!json_str || !out_cmd) return ESP_ERR_INVALID_ARG;
    out_cmd->type = CYD_CMD_UNKNOWN;
    out_cmd->param = 0;

    cJSON *root = cJSON_Parse(json_str);
    if (!root) return ESP_ERR_INVALID_ARG;

    cJSON *cmd_item = cJSON_GetObjectItem(root, "cmd");
    if (!cJSON_IsString(cmd_item) || !cmd_item->valuestring) {
        cJSON_Delete(root);
        return ESP_ERR_INVALID_ARG;
    }

    const char *cmd = cmd_item->valuestring;
    esp_err_t ret = ESP_OK;

    if (strcmp(cmd, "toggle") == 0) out_cmd->type = CYD_CMD_TOGGLE;
    else if (strcmp(cmd, "play") == 0) out_cmd->type = CYD_CMD_PLAY;
    else if (strcmp(cmd, "pause") == 0) out_cmd->type = CYD_CMD_PAUSE;
    else if (strcmp(cmd, "next") == 0) out_cmd->type = CYD_CMD_NEXT;
    else if (strcmp(cmd, "prev") == 0) out_cmd->type = CYD_CMD_PREV;
    else if (strcmp(cmd, "sync") == 0) out_cmd->type = CYD_CMD_SYNC;
    else if (strcmp(cmd, "vol") == 0) {
        cJSON *val = cJSON_GetObjectItem(root, "val");
        if (cJSON_IsNumber(val) && val->valueint >= 0 && val->valueint <= 100) {
            out_cmd->type = CYD_CMD_VOLUME;
            out_cmd->param = val->valueint;
        } else {
            ret = ESP_ERR_INVALID_ARG;
        }
    } else if (strcmp(cmd, "vol_step") == 0) {
        cJSON *dir = cJSON_GetObjectItem(root, "dir");
        if (cJSON_IsNumber(dir)) {
            out_cmd->type = CYD_CMD_VOL_STEP;
            out_cmd->param = dir->valueint;
        } else {
            ret = ESP_ERR_INVALID_ARG;
        }
    } else {
        ret = ESP_ERR_INVALID_ARG;
    }

    cJSON_Delete(root);
    return ret;
}
