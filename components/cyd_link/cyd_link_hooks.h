#pragma once

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

void cyd_link_hooks_init(void);
void cyd_link_hook_metadata(const char *artist, const char *album, const char *title);
void cyd_link_hook_timer(uint32_t elapsed, uint32_t duration);
void cyd_link_hook_playback_state(const char *state);
void cyd_link_broadcast_full_sync(void);
void cyd_link_set_cached_meta(const char *title, const char *artist, const char *album);

#ifdef __cplusplus
}
#endif
