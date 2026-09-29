#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Basic coordinate and color types */
typedef int16_t lv_coord_t;

typedef union {
    struct {
        uint16_t blue : 5;
        uint16_t green : 6;
        uint16_t red : 5;
    } ch;
    uint16_t full;
} lv_color16_t;

typedef lv_color16_t lv_color_t;
typedef uint8_t lv_opa_t;

#define LV_OPA_TRANSP 0
#define LV_OPA_COVER  255

static inline lv_color_t lv_color_make(uint8_t r, uint8_t g, uint8_t b) {
    lv_color_t c;
    c.ch.red = (r >> 3) & 0x1F;
    c.ch.green = (g >> 2) & 0x3F;
    c.ch.blue = (b >> 3) & 0x1F;
    return c;
}

static inline lv_color_t lv_color_hex(uint32_t hex) {
    uint8_t r = (uint8_t)((hex >> 16) & 0xFF);
    uint8_t g = (uint8_t)((hex >> 8) & 0xFF);
    uint8_t b = (uint8_t)(hex & 0xFF);
    return lv_color_make(r, g, b);
}

/* Enums and flags */
typedef enum {
    LV_RES_INV = 0,
    LV_RES_OK = 1,
} lv_res_t;

typedef enum {
    LV_EVENT_ALL = 0,
    LV_EVENT_PRESSED,
    LV_EVENT_PRESSING,
    LV_EVENT_PRESS_LOST,
    LV_EVENT_SHORT_CLICKED,
    LV_EVENT_LONG_PRESSED,
    LV_EVENT_LONG_PRESSED_REPEAT,
    LV_EVENT_CLICKED,
    LV_EVENT_RELEASED,
    LV_EVENT_SCROLL_BEGIN,
    LV_EVENT_SCROLL_END,
    LV_EVENT_SCROLL,
    LV_EVENT_GESTURE,
    LV_EVENT_KEY,
    LV_EVENT_FOCUSED,
    LV_EVENT_DEFOCUSED,
    LV_EVENT_LEAVE,
    LV_EVENT_HIT_TEST,
    LV_EVENT_COVER_CHECK,
    LV_EVENT_REFR_EXT_DRAW_SIZE,
    LV_EVENT_DRAW_MAIN_BEGIN,
    LV_EVENT_DRAW_MAIN,
    LV_EVENT_DRAW_MAIN_END,
    LV_EVENT_DRAW_POST_BEGIN,
    LV_EVENT_DRAW_POST,
    LV_EVENT_DRAW_POST_END,
    LV_EVENT_DRAW_PART_BEGIN,
    LV_EVENT_DRAW_PART_END,
    LV_EVENT_VALUE_CHANGED,
    LV_EVENT_INSERT,
    LV_EVENT_REFRESH,
    LV_EVENT_READY,
    LV_EVENT_CANCEL,
} lv_event_code_t;

typedef enum {
    LV_ALIGN_DEFAULT = 0,
    LV_ALIGN_TOP_LEFT,
    LV_ALIGN_TOP_MID,
    LV_ALIGN_TOP_RIGHT,
    LV_ALIGN_BOTTOM_LEFT,
    LV_ALIGN_BOTTOM_MID,
    LV_ALIGN_BOTTOM_RIGHT,
    LV_ALIGN_LEFT_MID,
    LV_ALIGN_RIGHT_MID,
    LV_ALIGN_CENTER,
} lv_align_t;

typedef enum {
    LV_LABEL_LONG_WRAP = 0,
    LV_LABEL_LONG_DOT,
    LV_LABEL_LONG_SCROLL,
    LV_LABEL_LONG_SCROLL_CIRCULAR,
    LV_LABEL_LONG_CLIP,
} lv_label_long_mode_t;

typedef enum {
    LV_ANIM_OFF = 0,
    LV_ANIM_ON = 1,
} lv_anim_enable_t;

typedef enum {
    LV_INDEV_TYPE_NONE = 0,
    LV_INDEV_TYPE_POINTER,
    LV_INDEV_TYPE_KEYPAD,
    LV_INDEV_TYPE_BUTTON,
    LV_INDEV_TYPE_ENCODER,
} lv_indev_type_t;

typedef enum {
    LV_INDEV_STATE_RELEASED = 0,
    LV_INDEV_STATE_PRESSED = 1,
} lv_indev_state_t;

typedef uint32_t lv_style_selector_t;
#define LV_STATE_DEFAULT   0x0000
#define LV_STATE_CHECKED   0x0001
#define LV_STATE_FOCUSED   0x0002
#define LV_STATE_FOCUS_KEY 0x0004
#define LV_STATE_EDITED    0x0008
#define LV_STATE_HOVERED   0x0010
#define LV_STATE_PRESSED   0x0020
#define LV_STATE_SCROLLED  0x0040
#define LV_STATE_DISABLED  0x0080

#define LV_PART_MAIN       0x000000
#define LV_PART_SCROLLBAR  0x010000
#define LV_PART_INDICATOR  0x020000
#define LV_PART_KNOB       0x030000

/* Symbols (Font Awesome mappings) */
#define LV_SYMBOL_PLAY         "\xEF\x81\x8B"
#define LV_SYMBOL_PAUSE        "\xEF\x81\x8C"
#define LV_SYMBOL_PREV         "\xEF\x81\x88"
#define LV_SYMBOL_NEXT         "\xEF\x81\x89"
#define LV_SYMBOL_VOLUME_MAX   "\xEF\x80\xA8"
#define LV_SYMBOL_VOLUME_MID   "\xEF\x80\xA7"
#define LV_SYMBOL_VOLUME_SMALL "\xEF\x80\xA6"
#define LV_SYMBOL_MUTE         "\xEF\x80\xA6"
#define LV_SYMBOL_WARNING      "\xEF\x81\xB1"
#define LV_SYMBOL_OK           "\xEF\x80\x8C"
#define LV_SYMBOL_CLOSE        "\xEF\x80\x8D"
#define LV_SYMBOL_WIFI         "\xEF\x87\xEB"

/* Styles */
typedef struct {
    lv_color_t bg_color;
    lv_opa_t bg_opa;
    lv_color_t text_color;
    lv_coord_t border_width;
    lv_color_t border_color;
    lv_coord_t radius;
    lv_coord_t pad_top;
    lv_coord_t pad_bottom;
    lv_coord_t pad_left;
    lv_coord_t pad_right;
} lv_style_t;

/* Structures */
typedef struct _lv_obj_t lv_obj_t;
typedef struct _lv_event_t lv_event_t;
typedef void (*lv_event_cb_t)(lv_event_t *e);

struct _lv_event_t {
    lv_obj_t *target;
    lv_event_code_t code;
    void *user_data;
    void *param;
};

#define LV_MAX_EVENT_CBS 4
struct _lv_obj_t {
    lv_coord_t x;
    lv_coord_t y;
    lv_coord_t w;
    lv_coord_t h;
    lv_obj_t *parent;
    char text[256];
    int32_t val;
    int32_t min;
    int32_t max;
    lv_label_long_mode_t long_mode;
    lv_event_cb_t event_cbs[LV_MAX_EVENT_CBS];
    lv_event_code_t event_filters[LV_MAX_EVENT_CBS];
    void *event_user_datas[LV_MAX_EVENT_CBS];
    uint8_t event_cb_count;
    void *user_data;
};

typedef struct {
    lv_coord_t x1;
    lv_coord_t y1;
    lv_coord_t x2;
    lv_coord_t y2;
} lv_area_t;

typedef struct {
    void *buf1;
    void *buf2;
    uint32_t size;
} lv_disp_draw_buf_t;

typedef struct _lv_disp_drv_t {
    lv_disp_draw_buf_t *draw_buf;
    void (*flush_cb)(struct _lv_disp_drv_t *disp_drv, const lv_area_t *area, lv_color_t *color_p);
    lv_coord_t hor_res;
    lv_coord_t ver_res;
    void *user_data;
} lv_disp_drv_t;

typedef struct _lv_disp_t lv_disp_t;

typedef struct {
    struct {
        lv_coord_t x;
        lv_coord_t y;
    } point;
    lv_indev_state_t state;
} lv_indev_data_t;

typedef struct _lv_indev_drv_t {
    lv_indev_type_t type;
    void (*read_cb)(struct _lv_indev_drv_t *indev_drv, lv_indev_data_t *data);
    void *user_data;
} lv_indev_drv_t;

typedef struct _lv_indev_t lv_indev_t;

/* Core functions */
void lv_init(void);
uint32_t lv_timer_handler(void);
void lv_tick_inc(uint32_t tick_period);

/* Display Driver */
void lv_disp_draw_buf_init(lv_disp_draw_buf_t *draw_buf, void *buf1, void *buf2, uint32_t size_in_px_cnt);
void lv_disp_drv_init(lv_disp_drv_t *driver);
lv_disp_t *lv_disp_drv_register(lv_disp_drv_t *driver);
void lv_disp_flush_ready(lv_disp_drv_t *disp_drv);

/* Indev Driver */
void lv_indev_drv_init(lv_indev_drv_t *driver);
lv_indev_t *lv_indev_drv_register(lv_indev_drv_t *driver);

/* Objects & Styles */
lv_obj_t *lv_scr_act(void);
lv_obj_t *lv_obj_create(lv_obj_t *parent);
void lv_obj_set_size(lv_obj_t *obj, lv_coord_t w, lv_coord_t h);
void lv_obj_set_width(lv_obj_t *obj, lv_coord_t w);
void lv_obj_set_height(lv_obj_t *obj, lv_coord_t h);
void lv_obj_set_pos(lv_obj_t *obj, lv_coord_t x, lv_coord_t y);
void lv_obj_set_x(lv_obj_t *obj, lv_coord_t x);
void lv_obj_set_y(lv_obj_t *obj, lv_coord_t y);
void lv_obj_align(lv_obj_t *obj, lv_align_t align, lv_coord_t x_ofs, lv_coord_t y_ofs);
void lv_obj_align_to(lv_obj_t *obj, const lv_obj_t *base, lv_align_t align, lv_coord_t x_ofs, lv_coord_t y_ofs);
void lv_obj_add_style(lv_obj_t *obj, lv_style_t *style, lv_style_selector_t selector);
void lv_obj_set_style_text_color(lv_obj_t *obj, lv_color_t value, lv_style_selector_t selector);
void lv_obj_add_event_cb(lv_obj_t *obj, lv_event_cb_t event_cb, lv_event_code_t filter, void *user_data);
void lv_event_send(lv_obj_t *obj, lv_event_code_t code, void *param);
lv_obj_t *lv_event_get_target(lv_event_t *e);
lv_event_code_t lv_event_get_code(lv_event_t *e);
void *lv_event_get_user_data(lv_event_t *e);

/* Style API */
void lv_style_init(lv_style_t *style);
void lv_style_set_bg_color(lv_style_t *style, lv_color_t value);
void lv_style_set_bg_opa(lv_style_t *style, lv_opa_t value);
void lv_style_set_text_color(lv_style_t *style, lv_color_t value);
void lv_style_set_border_width(lv_style_t *style, lv_coord_t value);
void lv_style_set_border_color(lv_style_t *style, lv_color_t value);
void lv_style_set_radius(lv_style_t *style, lv_coord_t value);
void lv_style_set_pad_all(lv_style_t *style, lv_coord_t value);
void lv_style_set_pad_top(lv_style_t *style, lv_coord_t value);
void lv_style_set_pad_bottom(lv_style_t *style, lv_coord_t value);
void lv_style_set_pad_left(lv_style_t *style, lv_coord_t value);
void lv_style_set_pad_right(lv_style_t *style, lv_coord_t value);

/* Label API */
lv_obj_t *lv_label_create(lv_obj_t *parent);
void lv_label_set_text(lv_obj_t *obj, const char *text);
void lv_label_set_text_fmt(lv_obj_t *obj, const char *fmt, ...);
void lv_label_set_long_mode(lv_obj_t *obj, lv_label_long_mode_t mode);
const char *lv_label_get_text(const lv_obj_t *obj);

/* Bar API */
lv_obj_t *lv_bar_create(lv_obj_t *parent);
void lv_bar_set_range(lv_obj_t *obj, int32_t min, int32_t max);
void lv_bar_set_value(lv_obj_t *obj, int32_t val, lv_anim_enable_t anim);
int32_t lv_bar_get_value(const lv_obj_t *obj);
int32_t lv_bar_get_min_value(const lv_obj_t *obj);
int32_t lv_bar_get_max_value(const lv_obj_t *obj);

/* Slider API */
lv_obj_t *lv_slider_create(lv_obj_t *parent);
void lv_slider_set_range(lv_obj_t *obj, int32_t min, int32_t max);
void lv_slider_set_value(lv_obj_t *obj, int32_t val, lv_anim_enable_t anim);
int32_t lv_slider_get_value(const lv_obj_t *obj);
int32_t lv_slider_get_min_value(const lv_obj_t *obj);
int32_t lv_slider_get_max_value(const lv_obj_t *obj);

/* Button API */
lv_obj_t *lv_btn_create(lv_obj_t *parent);

#ifdef __cplusplus
}
#endif
