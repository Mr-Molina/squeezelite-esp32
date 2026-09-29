#include "cyd_protocol.h"
#include "cJSON.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

static char *append_newline(char *json) {
    if (!json) return NULL;
    size_t len = strlen(json);
    char *out = (char *)malloc(len + 2);
    if (!out) {
        free(json);
        return NULL;
    }
    memcpy(out, json, len);
    out[len] = '\n';
    out[len + 1] = '\0';
    free(json);
    return out;
}

esp_err_t cyd_client_parse_event(const char *line, cyd_telemetry_state_t *state) {
    if (!line || !state || *line == '\0') {
        return ESP_ERR_INVALID_ARG;
    }

    cJSON *root = cJSON_Parse(line);
    if (!root) {
        return ESP_ERR_INVALID_ARG;
    }

    cJSON *ev = cJSON_GetObjectItem(root, "event");
    if (!cJSON_IsString(ev) || !ev->valuestring) {
        cJSON_Delete(root);
        return ESP_ERR_INVALID_ARG;
    }

    const char *event_name = ev->valuestring;
    esp_err_t ret = ESP_OK;

    if (strcmp(event_name, "meta") == 0) {
        cJSON *title = cJSON_GetObjectItem(root, "title");
        if (cJSON_IsString(title) && title->valuestring) {
            strncpy(state->title, title->valuestring, sizeof(state->title) - 1);
            state->title[sizeof(state->title) - 1] = '\0';
        }
        cJSON *artist = cJSON_GetObjectItem(root, "artist");
        if (cJSON_IsString(artist) && artist->valuestring) {
            strncpy(state->artist, artist->valuestring, sizeof(state->artist) - 1);
            state->artist[sizeof(state->artist) - 1] = '\0';
        }
        cJSON *album = cJSON_GetObjectItem(root, "album");
        if (cJSON_IsString(album) && album->valuestring) {
            strncpy(state->album, album->valuestring, sizeof(state->album) - 1);
            state->album[sizeof(state->album) - 1] = '\0';
        }
        state->link_active = true;
    } else if (strcmp(event_name, "status") == 0) {
        cJSON *st = cJSON_GetObjectItem(root, "state");
        if (cJSON_IsString(st) && st->valuestring) {
            strncpy(state->state, st->valuestring, sizeof(state->state) - 1);
            state->state[sizeof(state->state) - 1] = '\0';
        }
        cJSON *elapsed = cJSON_GetObjectItem(root, "elapsed");
        if (cJSON_IsNumber(elapsed)) {
            state->elapsed = (uint32_t)elapsed->valueint;
        }
        cJSON *duration = cJSON_GetObjectItem(root, "duration");
        if (cJSON_IsNumber(duration)) {
            state->duration = (uint32_t)duration->valueint;
        }
        cJSON *vol = cJSON_GetObjectItem(root, "vol");
        if (cJSON_IsNumber(vol)) {
            int v = vol->valueint;
            if (v < 0) v = 0;
            if (v > 100) v = 100;
            state->vol = (uint8_t)v;
        }
        state->link_active = true;
    } else if (strcmp(event_name, "sys") == 0) {
        cJSON *mode = cJSON_GetObjectItem(root, "mode");
        if (cJSON_IsString(mode) && mode->valuestring) {
            strncpy(state->mode, mode->valuestring, sizeof(state->mode) - 1);
            state->mode[sizeof(state->mode) - 1] = '\0';
        }
        cJSON *ip = cJSON_GetObjectItem(root, "ip");
        if (cJSON_IsString(ip) && ip->valuestring) {
            strncpy(state->ip, ip->valuestring, sizeof(state->ip) - 1);
            state->ip[sizeof(state->ip) - 1] = '\0';
        }
        state->link_active = true;
    } else {
        ret = ESP_ERR_INVALID_ARG;
    }

    cJSON_Delete(root);
    return ret;
}

char *cyd_client_format_command(const char *cmd_name, int32_t val) {
    if (!cmd_name) return NULL;

    cJSON *root = cJSON_CreateObject();
    if (!root) return NULL;

    cJSON_AddStringToObject(root, "cmd", cmd_name);

    if (strcmp(cmd_name, "vol") == 0) {
        if (val < 0 || val > 100) {
            cJSON_Delete(root);
            return NULL;
        }
        cJSON_AddNumberToObject(root, "val", val);
    } else if (strcmp(cmd_name, "vol_step") == 0) {
        cJSON_AddNumberToObject(root, "dir", val);
    } else if (strcmp(cmd_name, "toggle") == 0 ||
               strcmp(cmd_name, "play") == 0 ||
               strcmp(cmd_name, "pause") == 0 ||
               strcmp(cmd_name, "next") == 0 ||
               strcmp(cmd_name, "prev") == 0 ||
               strcmp(cmd_name, "sync") == 0) {
        // Simple command without additional arguments
    } else {
        cJSON_Delete(root);
        return NULL;
    }

    char *rendered = cJSON_PrintUnformatted(root);
    cJSON_Delete(root);
    return append_newline(rendered);
}
