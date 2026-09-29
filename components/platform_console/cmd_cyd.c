#include "cyd_link.h"
#include "cyd_link_hooks.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#if defined(ESP_PLATFORM)
#include "esp_console.h"
#include "esp_log.h"
#else
typedef struct {
    const char *command;
    const char *help;
    const char *hint;
    int (*func)(int argc, char **argv);
    void *argtable;
} esp_console_cmd_t;
__attribute__((weak)) int esp_console_cmd_register(const esp_console_cmd_t *cmd) {
    (void)cmd;
    return 0;
}
#endif

static const char *TAG __attribute__((unused)) = "cmd_cyd";

static int do_cyd_cmd(int argc, char **argv) {
    if (argc < 2) {
        printf("Usage: cyd <sync|play|pause|next|prev|vol <0-100>>\n");
        return 0;
    }

    if (strcmp(argv[1], "sync") == 0) {
        printf("Triggering full CYD sync...\n");
        cyd_link_broadcast_full_sync();
    } else if (strcmp(argv[1], "play") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"play\"}\n", 15);
    } else if (strcmp(argv[1], "pause") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"pause\"}\n", 16);
    } else if (strcmp(argv[1], "next") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"next\"}\n", 15);
    } else if (strcmp(argv[1], "prev") == 0) {
        cyd_link_feed_rx_bytes("{\"cmd\":\"prev\"}\n", 15);
    } else if (strcmp(argv[1], "vol") == 0) {
        if (argc < 3) {
            printf("Usage: cyd vol <0-100>\n");
            return 1;
        }
        int val = atoi(argv[2]);
        if (val < 0 || val > 100) {
            printf("Volume must be between 0 and 100\n");
            return 1;
        }
        char vol_buf[32];
        int len = snprintf(vol_buf, sizeof(vol_buf), "{\"cmd\":\"vol\",\"val\":%d}\n", val);
        cyd_link_feed_rx_bytes(vol_buf, len);
    } else {
        printf("Unknown subcommand: %s\n", argv[1]);
        printf("Usage: cyd <sync|play|pause|next|prev|vol <0-100>>\n");
        return 1;
    }

    return 0;
}

void register_cyd(void) {
    const esp_console_cmd_t cmd = {
        .command = "cyd",
        .help = "CYD Touch Link Diagnostic Tool",
        .hint = NULL,
        .func = &do_cyd_cmd,
    };
    esp_console_cmd_register(&cmd);
}
