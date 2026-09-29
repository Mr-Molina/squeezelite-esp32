#include "lvgl.h"
#include <string.h>
#include <stdio.h>
#include <stdarg.h>

#define MAX_MOCK_OBJS 512
static lv_obj_t s_mock_objs[MAX_MOCK_OBJS];
static size_t s_mock_obj_count = 0;
static lv_obj_t s_mock_scr;
static bool s_mock_inited = false;

void lv_init(void) {
    memset(s_mock_objs, 0, sizeof(s_mock_objs));
    s_mock_obj_count = 0;
    memset(&s_mock_scr, 0, sizeof(s_mock_scr));
    s_mock_scr.w = 320;
    s_mock_scr.h = 240;
    s_mock_inited = true;
}

uint32_t lv_timer_handler(void) {
    return 10;
}

void lv_tick_inc(uint32_t tick_period) {
    (void)tick_period;
}

void lv_disp_draw_buf_init(lv_disp_draw_buf_t *draw_buf, void *buf1, void *buf2, uint32_t size_in_px_cnt) {
    if (draw_buf) {
        draw_buf->buf1 = buf1;
        draw_buf->buf2 = buf2;
        draw_buf->size = size_in_px_cnt;
    }
}

void lv_disp_drv_init(lv_disp_drv_t *driver) {
    if (driver) {
        memset(driver, 0, sizeof(lv_disp_drv_t));
        driver->hor_res = 320;
        driver->ver_res = 240;
    }
}

lv_disp_t *lv_disp_drv_register(lv_disp_drv_t *driver) {
    return (lv_disp_t *)driver;
}

void lv_disp_flush_ready(lv_disp_drv_t *disp_drv) {
    (void)disp_drv;
}

void lv_indev_drv_init(lv_indev_drv_t *driver) {
    if (driver) {
        memset(driver, 0, sizeof(lv_indev_drv_t));
    }
}

lv_indev_t *lv_indev_drv_register(lv_indev_drv_t *driver) {
    return (lv_indev_t *)driver;
}

lv_obj_t *lv_scr_act(void) {
    if (!s_mock_inited) {
        lv_init();
    }
    return &s_mock_scr;
}

lv_obj_t *lv_obj_create(lv_obj_t *parent) {
    if (s_mock_obj_count >= MAX_MOCK_OBJS) {
        return &s_mock_objs[MAX_MOCK_OBJS - 1];
    }
    lv_obj_t *obj = &s_mock_objs[s_mock_obj_count++];
    memset(obj, 0, sizeof(lv_obj_t));
    obj->parent = parent ? parent : lv_scr_act();
    return obj;
}

void lv_obj_set_size(lv_obj_t *obj, lv_coord_t w, lv_coord_t h) {
    if (obj) {
        obj->w = w;
        obj->h = h;
    }
}

void lv_obj_set_width(lv_obj_t *obj, lv_coord_t w) {
    if (obj) obj->w = w;
}

void lv_obj_set_height(lv_obj_t *obj, lv_coord_t h) {
    if (obj) obj->h = h;
}

void lv_obj_set_pos(lv_obj_t *obj, lv_coord_t x, lv_coord_t y) {
    if (obj) {
        obj->x = x;
        obj->y = y;
    }
}

void lv_obj_set_x(lv_obj_t *obj, lv_coord_t x) {
    if (obj) obj->x = x;
}

void lv_obj_set_y(lv_obj_t *obj, lv_coord_t y) {
    if (obj) obj->y = y;
}

void lv_obj_align(lv_obj_t *obj, lv_align_t align, lv_coord_t x_ofs, lv_coord_t y_ofs) {
    if (!obj) return;
    (void)align;
    obj->x = x_ofs;
    obj->y = y_ofs;
}

void lv_obj_align_to(lv_obj_t *obj, const lv_obj_t *base, lv_align_t align, lv_coord_t x_ofs, lv_coord_t y_ofs) {
    if (!obj || !base) return;
    (void)align;
    obj->x = base->x + x_ofs;
    obj->y = base->y + y_ofs;
}

void lv_obj_add_style(lv_obj_t *obj, lv_style_t *style, lv_style_selector_t selector) {
    (void)obj;
    (void)style;
    (void)selector;
}

void lv_obj_set_style_text_color(lv_obj_t *obj, lv_color_t value, lv_style_selector_t selector) {
    (void)obj;
    (void)value;
    (void)selector;
}

void lv_obj_add_event_cb(lv_obj_t *obj, lv_event_cb_t event_cb, lv_event_code_t filter, void *user_data) {
    if (!obj || !event_cb) return;
    if (obj->event_cb_count < LV_MAX_EVENT_CBS) {
        obj->event_cbs[obj->event_cb_count] = event_cb;
        obj->event_filters[obj->event_cb_count] = filter;
        obj->event_user_datas[obj->event_cb_count] = user_data;
        obj->event_cb_count++;
    }
}

void lv_event_send(lv_obj_t *obj, lv_event_code_t code, void *param) {
    if (!obj) return;
    for (uint8_t i = 0; i < obj->event_cb_count; i++) {
        if (obj->event_filters[i] == LV_EVENT_ALL || obj->event_filters[i] == code) {
            lv_event_t e;
            e.target = obj;
            e.code = code;
            e.user_data = obj->event_user_datas[i];
            e.param = param;
            obj->event_cbs[i](&e);
        }
    }
}

lv_obj_t *lv_event_get_target(lv_event_t *e) {
    return e ? e->target : NULL;
}

lv_event_code_t lv_event_get_code(lv_event_t *e) {
    return e ? e->code : LV_EVENT_ALL;
}

void *lv_event_get_user_data(lv_event_t *e) {
    return e ? e->user_data : NULL;
}

void lv_style_init(lv_style_t *style) {
    if (style) memset(style, 0, sizeof(lv_style_t));
}

void lv_style_set_bg_color(lv_style_t *style, lv_color_t value) {
    if (style) style->bg_color = value;
}

void lv_style_set_bg_opa(lv_style_t *style, lv_opa_t value) {
    if (style) style->bg_opa = value;
}

void lv_style_set_text_color(lv_style_t *style, lv_color_t value) {
    if (style) style->text_color = value;
}

void lv_style_set_border_width(lv_style_t *style, lv_coord_t value) {
    if (style) style->border_width = value;
}

void lv_style_set_border_color(lv_style_t *style, lv_color_t value) {
    if (style) style->border_color = value;
}

void lv_style_set_radius(lv_style_t *style, lv_coord_t value) {
    if (style) style->radius = value;
}

void lv_style_set_pad_all(lv_style_t *style, lv_coord_t value) {
    if (style) {
        style->pad_top = value;
        style->pad_bottom = value;
        style->pad_left = value;
        style->pad_right = value;
    }
}

void lv_style_set_pad_top(lv_style_t *style, lv_coord_t value) {
    if (style) style->pad_top = value;
}

void lv_style_set_pad_bottom(lv_style_t *style, lv_coord_t value) {
    if (style) style->pad_bottom = value;
}

void lv_style_set_pad_left(lv_style_t *style, lv_coord_t value) {
    if (style) style->pad_left = value;
}

void lv_style_set_pad_right(lv_style_t *style, lv_coord_t value) {
    if (style) style->pad_right = value;
}

lv_obj_t *lv_label_create(lv_obj_t *parent) {
    lv_obj_t *obj = lv_obj_create(parent);
    return obj;
}

void lv_label_set_text(lv_obj_t *obj, const char *text) {
    if (!obj) return;
    if (text) {
        strncpy(obj->text, text, sizeof(obj->text) - 1);
        obj->text[sizeof(obj->text) - 1] = '\0';
    } else {
        obj->text[0] = '\0';
    }
}

void lv_label_set_text_fmt(lv_obj_t *obj, const char *fmt, ...) {
    if (!obj || !fmt) return;
    va_list args;
    va_start(args, fmt);
    vsnprintf(obj->text, sizeof(obj->text), fmt, args);
    obj->text[sizeof(obj->text) - 1] = '\0';
    va_end(args);
}

void lv_label_set_long_mode(lv_obj_t *obj, lv_label_long_mode_t mode) {
    if (obj) obj->long_mode = mode;
}

const char *lv_label_get_text(const lv_obj_t *obj) {
    return obj ? obj->text : "";
}

lv_obj_t *lv_bar_create(lv_obj_t *parent) {
    lv_obj_t *obj = lv_obj_create(parent);
    if (obj) {
        obj->min = 0;
        obj->max = 100;
        obj->val = 0;
    }
    return obj;
}

void lv_bar_set_range(lv_obj_t *obj, int32_t min, int32_t max) {
    if (obj) {
        obj->min = min;
        obj->max = max;
    }
}

void lv_bar_set_value(lv_obj_t *obj, int32_t val, lv_anim_enable_t anim) {
    (void)anim;
    if (obj) {
        if (val < obj->min) val = obj->min;
        if (val > obj->max) val = obj->max;
        obj->val = val;
    }
}

int32_t lv_bar_get_value(const lv_obj_t *obj) {
    return obj ? obj->val : 0;
}

int32_t lv_bar_get_min_value(const lv_obj_t *obj) {
    return obj ? obj->min : 0;
}

int32_t lv_bar_get_max_value(const lv_obj_t *obj) {
    return obj ? obj->max : 100;
}

lv_obj_t *lv_slider_create(lv_obj_t *parent) {
    lv_obj_t *obj = lv_obj_create(parent);
    if (obj) {
        obj->min = 0;
        obj->max = 100;
        obj->val = 0;
    }
    return obj;
}

void lv_slider_set_range(lv_obj_t *obj, int32_t min, int32_t max) {
    if (obj) {
        obj->min = min;
        obj->max = max;
    }
}

void lv_slider_set_value(lv_obj_t *obj, int32_t val, lv_anim_enable_t anim) {
    (void)anim;
    if (obj) {
        if (val < obj->min) val = obj->min;
        if (val > obj->max) val = obj->max;
        obj->val = val;
    }
}

int32_t lv_slider_get_value(const lv_obj_t *obj) {
    return obj ? obj->val : 0;
}

int32_t lv_slider_get_min_value(const lv_obj_t *obj) {
    return obj ? obj->min : 0;
}

int32_t lv_slider_get_max_value(const lv_obj_t *obj) {
    return obj ? obj->max : 100;
}

lv_obj_t *lv_btn_create(lv_obj_t *parent) {
    return lv_obj_create(parent);
}
