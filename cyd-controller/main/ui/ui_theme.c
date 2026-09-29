#include "ui_theme.h"

lv_style_t style_screen;
lv_style_t style_surface;
lv_style_t style_status_bar;
lv_style_t style_btn;
lv_style_t style_btn_accent;
lv_style_t style_slider_main;
lv_style_t style_slider_indic;
lv_style_t style_slider_knob;
lv_style_t style_text_title;
lv_style_t style_text_subtitle;
lv_style_t style_text_time;
lv_style_t style_badge_online;
lv_style_t style_badge_offline;

void ui_theme_init(void) {
    // 1. Root Screen Style
    lv_style_init(&style_screen);
    lv_style_set_bg_color(&style_screen, lv_color_hex(UI_COLOR_HEX_BG));
    lv_style_set_bg_opa(&style_screen, LV_OPA_COVER);
    lv_style_set_pad_all(&style_screen, 0);

    // 2. Status Bar Style (28px height)
    lv_style_init(&style_status_bar);
    lv_style_set_bg_color(&style_status_bar, lv_color_hex(UI_COLOR_HEX_SURFACE));
    lv_style_set_bg_opa(&style_status_bar, LV_OPA_COVER);
    lv_style_set_pad_all(&style_status_bar, 4);

    // 3. Surface Card Style (Center panel)
    lv_style_init(&style_surface);
    lv_style_set_bg_color(&style_surface, lv_color_hex(UI_COLOR_HEX_SURFACE));
    lv_style_set_bg_opa(&style_surface, LV_OPA_COVER);
    lv_style_set_radius(&style_surface, 8);
    lv_style_set_pad_all(&style_surface, 8);

    // 4. Transport Button Style (Normal)
    lv_style_init(&style_btn);
    lv_style_set_bg_color(&style_btn, lv_color_hex(UI_COLOR_HEX_SURFACE_LIGHT));
    lv_style_set_bg_opa(&style_btn, LV_OPA_COVER);
    lv_style_set_text_color(&style_btn, lv_color_hex(UI_COLOR_HEX_TEXT_PRIMARY));
    lv_style_set_radius(&style_btn, 22);

    // 5. Transport Accent Button Style (Play/Pause)
    lv_style_init(&style_btn_accent);
    lv_style_set_bg_color(&style_btn_accent, lv_color_hex(UI_COLOR_HEX_ACCENT));
    lv_style_set_bg_opa(&style_btn_accent, LV_OPA_COVER);
    lv_style_set_text_color(&style_btn_accent, lv_color_hex(UI_COLOR_HEX_TEXT_PRIMARY));
    lv_style_set_radius(&style_btn_accent, 24);

    // 6. Slider Main Track Style
    lv_style_init(&style_slider_main);
    lv_style_set_bg_color(&style_slider_main, lv_color_hex(UI_COLOR_HEX_SURFACE_LIGHT));
    lv_style_set_bg_opa(&style_slider_main, LV_OPA_COVER);
    lv_style_set_radius(&style_slider_main, 4);

    // 7. Slider / Progress Indicator Style
    lv_style_init(&style_slider_indic);
    lv_style_set_bg_color(&style_slider_indic, lv_color_hex(UI_COLOR_HEX_ACCENT));
    lv_style_set_bg_opa(&style_slider_indic, LV_OPA_COVER);
    lv_style_set_radius(&style_slider_indic, 4);

    // 8. Slider Knob Style
    lv_style_init(&style_slider_knob);
    lv_style_set_bg_color(&style_slider_knob, lv_color_hex(UI_COLOR_HEX_TEXT_PRIMARY));
    lv_style_set_bg_opa(&style_slider_knob, LV_OPA_COVER);
    lv_style_set_radius(&style_slider_knob, 8);

    // 9. Typography Styles
    lv_style_init(&style_text_title);
    lv_style_set_text_color(&style_text_title, lv_color_hex(UI_COLOR_HEX_TEXT_PRIMARY));

    lv_style_init(&style_text_subtitle);
    lv_style_set_text_color(&style_text_subtitle, lv_color_hex(UI_COLOR_HEX_TEXT_SECONDARY));

    lv_style_init(&style_text_time);
    lv_style_set_text_color(&style_text_time, lv_color_hex(UI_COLOR_HEX_TEXT_SECONDARY));

    // 10. Status Badges
    lv_style_init(&style_badge_online);
    lv_style_set_text_color(&style_badge_online, lv_color_hex(UI_COLOR_HEX_ONLINE));

    lv_style_init(&style_badge_offline);
    lv_style_set_text_color(&style_badge_offline, lv_color_hex(UI_COLOR_HEX_OFFLINE));
}
