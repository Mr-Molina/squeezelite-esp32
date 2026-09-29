#pragma once

#include "lvgl.h"

#ifdef __cplusplus
extern "C" {
#endif

/* 320x240 Landscape Color Definitions */
#define UI_COLOR_HEX_BG             0x121212
#define UI_COLOR_HEX_SURFACE        0x1E1E1E
#define UI_COLOR_HEX_SURFACE_LIGHT  0x282828
#define UI_COLOR_HEX_ACCENT         0x1DB954   /* Spotify Green */
#define UI_COLOR_HEX_TEXT_PRIMARY   0xFFFFFF   /* Crisp White */
#define UI_COLOR_HEX_TEXT_SECONDARY 0xAAAAAA   /* Muted Gray */
#define UI_COLOR_HEX_OFFLINE        0xE74C3C   /* Warning Red */
#define UI_COLOR_HEX_ONLINE         0x1DB954   /* Connected Green */

/* Global Style Declarations */
extern lv_style_t style_screen;
extern lv_style_t style_surface;
extern lv_style_t style_status_bar;
extern lv_style_t style_btn;
extern lv_style_t style_btn_accent;
extern lv_style_t style_slider_main;
extern lv_style_t style_slider_indic;
extern lv_style_t style_slider_knob;
extern lv_style_t style_text_title;
extern lv_style_t style_text_subtitle;
extern lv_style_t style_text_time;
extern lv_style_t style_badge_online;
extern lv_style_t style_badge_offline;

/**
 * @brief Initialize all styles and colors for the CYD Now Playing theme.
 */
void ui_theme_init(void);

#ifdef __cplusplus
}
#endif
