#include "ui_now_playing.h"
#include "ui_theme.h"
#include "cyd_uart.h"
#include <stdio.h>
#include <string.h>

#if defined(ESP_PLATFORM)
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
static SemaphoreHandle_t s_ui_mutex = NULL;
#endif

void cyd_ui_lock(void) {
#if defined(ESP_PLATFORM)
    if (s_ui_mutex) xSemaphoreTake(s_ui_mutex, portMAX_DELAY);
#endif
}

void cyd_ui_unlock(void) {
#if defined(ESP_PLATFORM)
    if (s_ui_mutex) xSemaphoreGive(s_ui_mutex);
#endif
}

/* UI Widget Handles */
static lv_obj_t *s_status_bar = NULL;
static lv_obj_t *s_label_mode = NULL;
static lv_obj_t *s_label_ip = NULL;
static lv_obj_t *s_badge_link = NULL;

static lv_obj_t *s_center_panel = NULL;
static lv_obj_t *s_label_title = NULL;
static lv_obj_t *s_label_artist_album = NULL;
static lv_obj_t *s_bar_progress = NULL;
static lv_obj_t *s_label_time_elapsed = NULL;
static lv_obj_t *s_label_time_total = NULL;

static lv_obj_t *s_transport_panel = NULL;
static lv_obj_t *s_btn_prev = NULL;
static lv_obj_t *s_btn_play_pause = NULL;
static lv_obj_t *s_label_play_pause = NULL;
static lv_obj_t *s_btn_next = NULL;
static lv_obj_t *s_label_vol_icon = NULL;
static lv_obj_t *s_slider_vol = NULL;

/* Telemetry Cache and Throttling State */
static cyd_telemetry_state_t s_cached_state;
static uint32_t s_last_vol_sent_ms = 0;
static bool s_has_sent_first_vol = false;
static uint32_t s_mock_slider_time_ms = 0;

/* Helper: get current time in ms */
static uint32_t get_now_ms(void) {
#if defined(ESP_PLATFORM)
    return (uint32_t)(esp_timer_get_time() / 1000ULL);
#else
    return s_mock_slider_time_ms;
#endif
}

/* Event Callbacks */
static void on_prev_btn_event(lv_event_t *e) {
    if (lv_event_get_code(e) == LV_EVENT_CLICKED) {
        cyd_client_send_cmd("prev", 0);
    }
}

static void on_play_pause_btn_event(lv_event_t *e) {
    if (lv_event_get_code(e) == LV_EVENT_CLICKED) {
        cyd_client_send_cmd("toggle", 0);
    }
}

static void on_next_btn_event(lv_event_t *e) {
    if (lv_event_get_code(e) == LV_EVENT_CLICKED) {
        cyd_client_send_cmd("next", 0);
    }
}

static void on_volume_slider_event(lv_event_t *e) {
    lv_event_code_t code = lv_event_get_code(e);
    if (code == LV_EVENT_VALUE_CHANGED) {
        lv_obj_t *slider = lv_event_get_target(e);
        int32_t vol = lv_slider_get_value(slider);
        uint32_t now = get_now_ms();

        if (s_has_sent_first_vol && ui_now_playing_should_throttle_volume(now, s_last_vol_sent_ms)) {
            return;
        }

        s_last_vol_sent_ms = now;
        s_has_sent_first_vol = true;
        cyd_client_send_cmd("vol", vol);
    } else if (code == LV_EVENT_RELEASED) {
        // Flush trailing edge value upon touch release
        lv_obj_t *slider = lv_event_get_target(e);
        int32_t vol = lv_slider_get_value(slider);
        s_last_vol_sent_ms = get_now_ms();
        s_has_sent_first_vol = true;
        cyd_client_send_cmd("vol", vol);
    }
}

void ui_now_playing_create(void) {
#if defined(ESP_PLATFORM)
    if (!s_ui_mutex) {
        s_ui_mutex = xSemaphoreCreateMutex();
    }
#endif
    memset(&s_cached_state, 0, sizeof(s_cached_state));
    s_last_vol_sent_ms = 0;
    s_has_sent_first_vol = false;

    lv_obj_t *scr = lv_scr_act();
    lv_obj_add_style(scr, &style_screen, LV_PART_MAIN);

    /* 1. Top Status Bar (Height 28px) */
    s_status_bar = lv_obj_create(scr);
    lv_obj_set_pos(s_status_bar, 0, 0);
    lv_obj_set_size(s_status_bar, 320, 28);
    lv_obj_add_style(s_status_bar, &style_status_bar, LV_PART_MAIN);

    s_label_mode = lv_label_create(s_status_bar);
    lv_obj_set_pos(s_label_mode, 8, 4);
    lv_label_set_text(s_label_mode, "[STANDBY]");
    lv_obj_add_style(s_label_mode, &style_text_subtitle, LV_PART_MAIN);

    s_label_ip = lv_label_create(s_status_bar);
    lv_obj_set_pos(s_label_ip, 110, 4);
    lv_label_set_text(s_label_ip, "---.---.---.---");
    lv_obj_add_style(s_label_ip, &style_text_subtitle, LV_PART_MAIN);

    s_badge_link = lv_label_create(s_status_bar);
    lv_obj_set_pos(s_badge_link, 240, 4);
    lv_label_set_text(s_badge_link, LV_SYMBOL_WARNING " OFFLINE");
    lv_obj_add_style(s_badge_link, &style_badge_offline, LV_PART_MAIN);

    /* 2. Center Metadata & Progress Panel (Height 140px) */
    s_center_panel = lv_obj_create(scr);
    lv_obj_set_pos(s_center_panel, 8, 34);
    lv_obj_set_size(s_center_panel, 304, 134);
    lv_obj_add_style(s_center_panel, &style_surface, LV_PART_MAIN);

    // Track Title (with marquee circular scroll)
    s_label_title = lv_label_create(s_center_panel);
    lv_obj_set_pos(s_label_title, 12, 10);
    lv_obj_set_width(s_label_title, 280);
    lv_label_set_long_mode(s_label_title, LV_LABEL_LONG_SCROLL_CIRCULAR);
    lv_label_set_text(s_label_title, "No Track Playing");
    lv_obj_add_style(s_label_title, &style_text_title, LV_PART_MAIN);

    // Artist & Album
    s_label_artist_album = lv_label_create(s_center_panel);
    lv_obj_set_pos(s_label_artist_album, 12, 42);
    lv_obj_set_width(s_label_artist_album, 280);
    lv_label_set_text(s_label_artist_album, "Ready to stream");
    lv_obj_add_style(s_label_artist_album, &style_text_subtitle, LV_PART_MAIN);

    // Progress Bar
    s_bar_progress = lv_bar_create(s_center_panel);
    lv_obj_set_pos(s_bar_progress, 12, 80);
    lv_obj_set_size(s_bar_progress, 280, 8);
    lv_bar_set_range(s_bar_progress, 0, 100);
    lv_bar_set_value(s_bar_progress, 0, LV_ANIM_OFF);
    lv_obj_add_style(s_bar_progress, &style_slider_main, LV_PART_MAIN);
    lv_obj_add_style(s_bar_progress, &style_slider_indic, LV_PART_INDICATOR);

    // Time Labels
    s_label_time_elapsed = lv_label_create(s_center_panel);
    lv_obj_set_pos(s_label_time_elapsed, 12, 96);
    lv_label_set_text(s_label_time_elapsed, "00:00");
    lv_obj_add_style(s_label_time_elapsed, &style_text_time, LV_PART_MAIN);

    s_label_time_total = lv_label_create(s_center_panel);
    lv_obj_set_pos(s_label_time_total, 240, 96);
    lv_label_set_text(s_label_time_total, "00:00");
    lv_obj_add_style(s_label_time_total, &style_text_time, LV_PART_MAIN);

    /* 3. Bottom Transport Bar (Height 60px) */
    s_transport_panel = lv_obj_create(scr);
    lv_obj_set_pos(s_transport_panel, 0, 174);
    lv_obj_set_size(s_transport_panel, 320, 64);
    lv_obj_add_style(s_transport_panel, &style_screen, LV_PART_MAIN);

    // Prev Button
    s_btn_prev = lv_btn_create(s_transport_panel);
    lv_obj_set_pos(s_btn_prev, 12, 10);
    lv_obj_set_size(s_btn_prev, 44, 44);
    lv_obj_add_style(s_btn_prev, &style_btn, LV_PART_MAIN);
    lv_obj_add_event_cb(s_btn_prev, on_prev_btn_event, LV_EVENT_CLICKED, NULL);
    lv_obj_t *lbl_prev = lv_label_create(s_btn_prev);
    lv_label_set_text(lbl_prev, LV_SYMBOL_PREV);
    lv_obj_align(lbl_prev, LV_ALIGN_CENTER, 0, 0);

    // Play / Pause Button
    s_btn_play_pause = lv_btn_create(s_transport_panel);
    lv_obj_set_pos(s_btn_play_pause, 64, 8);
    lv_obj_set_size(s_btn_play_pause, 48, 48);
    lv_obj_add_style(s_btn_play_pause, &style_btn_accent, LV_PART_MAIN);
    lv_obj_add_event_cb(s_btn_play_pause, on_play_pause_btn_event, LV_EVENT_CLICKED, NULL);
    s_label_play_pause = lv_label_create(s_btn_play_pause);
    lv_label_set_text(s_label_play_pause, LV_SYMBOL_PLAY);
    lv_obj_align(s_label_play_pause, LV_ALIGN_CENTER, 0, 0);

    // Next Button
    s_btn_next = lv_btn_create(s_transport_panel);
    lv_obj_set_pos(s_btn_next, 120, 10);
    lv_obj_set_size(s_btn_next, 44, 44);
    lv_obj_add_style(s_btn_next, &style_btn, LV_PART_MAIN);
    lv_obj_add_event_cb(s_btn_next, on_next_btn_event, LV_EVENT_CLICKED, NULL);
    lv_obj_t *lbl_next = lv_label_create(s_btn_next);
    lv_label_set_text(lbl_next, LV_SYMBOL_NEXT);
    lv_obj_align(lbl_next, LV_ALIGN_CENTER, 0, 0);

    // Volume Icon
    s_label_vol_icon = lv_label_create(s_transport_panel);
    lv_obj_set_pos(s_label_vol_icon, 175, 22);
    lv_label_set_text(s_label_vol_icon, LV_SYMBOL_VOLUME_MAX);
    lv_obj_add_style(s_label_vol_icon, &style_text_subtitle, LV_PART_MAIN);

    // Volume Slider
    s_slider_vol = lv_slider_create(s_transport_panel);
    lv_obj_set_pos(s_slider_vol, 195, 24);
    lv_obj_set_size(s_slider_vol, 115, 16);
    lv_slider_set_range(s_slider_vol, 0, 100);
    lv_slider_set_value(s_slider_vol, 50, LV_ANIM_OFF);
    lv_obj_add_style(s_slider_vol, &style_slider_main, LV_PART_MAIN);
    lv_obj_add_style(s_slider_vol, &style_slider_indic, LV_PART_INDICATOR);
    lv_obj_add_style(s_slider_vol, &style_slider_knob, LV_PART_KNOB);
    lv_obj_add_event_cb(s_slider_vol, on_volume_slider_event, LV_EVENT_VALUE_CHANGED, NULL);
    lv_obj_add_event_cb(s_slider_vol, on_volume_slider_event, LV_EVENT_RELEASED, NULL);
}

void ui_now_playing_format_time(uint32_t seconds, char *buf, size_t buf_len) {
    if (!buf || buf_len == 0) return;

    if (seconds >= 3600) {
        uint32_t h = seconds / 3600;
        uint32_t m = (seconds % 3600) / 60;
        uint32_t s = seconds % 60;
        snprintf(buf, buf_len, "%02u:%02u:%02u", (unsigned int)h, (unsigned int)m, (unsigned int)s);
    } else {
        uint32_t m = seconds / 60;
        uint32_t s = seconds % 60;
        snprintf(buf, buf_len, "%02u:%02u", (unsigned int)m, (unsigned int)s);
    }
    buf[buf_len - 1] = '\0';
}

bool ui_now_playing_should_throttle_volume(uint32_t now_ms, uint32_t last_sent_ms) {
    uint32_t diff = now_ms - last_sent_ms;
    return diff < 100;
}

void ui_now_playing_update(const cyd_telemetry_state_t *state) {
    if (!state) return;

    cyd_ui_lock();

    // Cache copy of current telemetry
    s_cached_state = *state;

    // 1. Update Mode and IP
    if (s_label_mode) {
        char mode_str[32];
        snprintf(mode_str, sizeof(mode_str), "[%s]", state->mode[0] ? state->mode : "STANDBY");
        const char *cur_mode = lv_label_get_text(s_label_mode);
        if (!cur_mode || strcmp(cur_mode, mode_str) != 0) {
            lv_label_set_text(s_label_mode, mode_str);
        }
    }

    if (s_label_ip) {
        const char *ip_str = state->ip[0] ? state->ip : "---.---.---.---";
        const char *cur_ip = lv_label_get_text(s_label_ip);
        if (!cur_ip || strcmp(cur_ip, ip_str) != 0) {
            lv_label_set_text(s_label_ip, ip_str);
        }
    }

    // 2. Update Link Status Badge
    if (s_badge_link) {
        if (state->link_active) {
            const char *cur_badge = lv_label_get_text(s_badge_link);
            if (!cur_badge || strcmp(cur_badge, LV_SYMBOL_OK " ONLINE") != 0) {
                lv_label_set_text(s_badge_link, LV_SYMBOL_OK " ONLINE");
                lv_obj_set_style_text_color(s_badge_link, lv_color_hex(UI_COLOR_HEX_ONLINE), LV_PART_MAIN);
            }
        } else {
            const char *cur_badge = lv_label_get_text(s_badge_link);
            if (!cur_badge || strcmp(cur_badge, LV_SYMBOL_WARNING " OFFLINE") != 0) {
                lv_label_set_text(s_badge_link, LV_SYMBOL_WARNING " OFFLINE");
                lv_obj_set_style_text_color(s_badge_link, lv_color_hex(UI_COLOR_HEX_OFFLINE), LV_PART_MAIN);
            }
        }
    }

    // 3. Update Title
    if (s_label_title) {
        const char *title_str = (state->title[0] != '\0') ? state->title : "No Track Playing";
        const char *cur_title = lv_label_get_text(s_label_title);
        if (!cur_title || strcmp(cur_title, title_str) != 0) {
            lv_label_set_text(s_label_title, title_str);
        }
    }

    // 4. Update Artist & Album Subtitle
    if (s_label_artist_album) {
        char subtitle[288];
        if (state->artist[0] != '\0' && state->album[0] != '\0') {
            snprintf(subtitle, sizeof(subtitle), "%s • %s", state->artist, state->album);
        } else if (state->artist[0] != '\0') {
            snprintf(subtitle, sizeof(subtitle), "%s", state->artist);
        } else if (state->album[0] != '\0') {
            snprintf(subtitle, sizeof(subtitle), "%s", state->album);
        } else {
            strncpy(subtitle, "Ready to stream", sizeof(subtitle) - 1);
            subtitle[sizeof(subtitle) - 1] = '\0';
        }
        const char *cur_sub = lv_label_get_text(s_label_artist_album);
        if (!cur_sub || strcmp(cur_sub, subtitle) != 0) {
            lv_label_set_text(s_label_artist_album, subtitle);
        }
    }

    // 5. Update Progress Bar and Time Labels
    int progress_pct = 0;
    if (state->duration > 0) {
        progress_pct = (int)((state->elapsed * 100ULL) / state->duration);
        if (progress_pct > 100) progress_pct = 100;
    }

    if (s_bar_progress) {
        if (lv_bar_get_value(s_bar_progress) != progress_pct) {
            lv_bar_set_value(s_bar_progress, progress_pct, LV_ANIM_OFF);
        }
    }

    if (s_label_time_elapsed) {
        char elapsed_buf[32];
        ui_now_playing_format_time(state->elapsed, elapsed_buf, sizeof(elapsed_buf));
        const char *cur_elapsed = lv_label_get_text(s_label_time_elapsed);
        if (!cur_elapsed || strcmp(cur_elapsed, elapsed_buf) != 0) {
            lv_label_set_text(s_label_time_elapsed, elapsed_buf);
        }
    }

    if (s_label_time_total) {
        char total_buf[32];
        ui_now_playing_format_time(state->duration, total_buf, sizeof(total_buf));
        const char *cur_total = lv_label_get_text(s_label_time_total);
        if (!cur_total || strcmp(cur_total, total_buf) != 0) {
            lv_label_set_text(s_label_time_total, total_buf);
        }
    }

    // 6. Update Play/Pause Dynamic Icon
    if (s_label_play_pause) {
        const char *symbol = (strcmp(state->state, "play") == 0) ? LV_SYMBOL_PAUSE : LV_SYMBOL_PLAY;
        const char *cur_sym = lv_label_get_text(s_label_play_pause);
        if (!cur_sym || strcmp(cur_sym, symbol) != 0) {
            lv_label_set_text(s_label_play_pause, symbol);
        }
    }

    // 7. Update Volume Slider
    if (s_slider_vol) {
        if (lv_slider_get_value(s_slider_vol) != state->vol) {
            lv_slider_set_value(s_slider_vol, state->vol, LV_ANIM_OFF);
        }
    }

    cyd_ui_unlock();
}

const cyd_telemetry_state_t *ui_now_playing_get_cached_state(void) {
    return &s_cached_state;
}

int ui_now_playing_get_progress_percent(void) {
    return s_bar_progress ? lv_bar_get_value(s_bar_progress) : 0;
}

const char *ui_now_playing_get_play_pause_icon(void) {
    return s_label_play_pause ? lv_label_get_text(s_label_play_pause) : "";
}

bool ui_now_playing_is_offline(void) {
    return !s_cached_state.link_active;
}

void ui_now_playing_simulate_btn_click(ui_btn_id_t btn_id) {
    lv_obj_t *target = NULL;
    switch (btn_id) {
        case UI_BTN_PREV: target = s_btn_prev; break;
        case UI_BTN_PLAY_PAUSE: target = s_btn_play_pause; break;
        case UI_BTN_NEXT: target = s_btn_next; break;
        default: break;
    }
    if (target) {
        lv_event_send(target, LV_EVENT_CLICKED, NULL);
    }
}

void ui_now_playing_simulate_slider_change(int32_t vol, uint32_t now_ms) {
    s_mock_slider_time_ms = now_ms;
    if (s_slider_vol) {
        lv_slider_set_value(s_slider_vol, vol, LV_ANIM_OFF);
        lv_event_send(s_slider_vol, LV_EVENT_VALUE_CHANGED, NULL);
    }
}
