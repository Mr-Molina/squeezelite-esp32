#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"
#include "cyd_link_dispatch.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*cyd_link_cmd_cb_t)(cyd_cmd_type_t type, int32_t param);

esp_err_t cyd_link_init(void);
esp_err_t cyd_link_send_raw(const char *json_line);
void cyd_link_set_cmd_handler(cyd_link_cmd_cb_t handler);
void cyd_link_feed_rx_bytes(const char *buf, size_t len);
void cyd_link_execute_command(const cyd_command_t *cmd);

#ifdef __cplusplus
}
#endif
