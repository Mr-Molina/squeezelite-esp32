#include "unity.h"
#if defined(ESP_PLATFORM)
#include "unity_test_runner.h"
#endif

#include "ili9341.h"
#include "xpt2046.h"
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

#if !defined(ESP_PLATFORM)
void setUp(void) {}
void tearDown(void) {}
#endif

/* Dummy pixel buffer for flush testing */
static uint16_t s_dummy_pixels[320 * 2];

void test_xpt2046_calibrate_raw_normal(void) {
    xpt2046_set_calibration(300, 3800, 300, 3800);
    int16_t out_x = -1, out_y = -1;

    // Minimum raw coordinates should map exactly to (0, 0)
    xpt2046_calibrate_raw(300, 300, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(0, out_x);
    TEST_ASSERT_EQUAL_INT16(0, out_y);

    // Maximum raw coordinates should map exactly to (319, 239)
    xpt2046_calibrate_raw(3800, 3800, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_WIDTH - 1, out_x);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_HEIGHT - 1, out_y);

    // Midpoint coordinates (300 + 3800) / 2 = 2050
    // X: (2050 - 300) * 319 / 3500 = 1750 * 319 / 3500 = 159.5 -> 159
    // Y: (2050 - 300) * 239 / 3500 = 1750 * 239 / 3500 = 119.5 -> 119
    xpt2046_calibrate_raw(2050, 2050, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(159, out_x);
    TEST_ASSERT_EQUAL_INT16(119, out_y);
}

void test_xpt2046_calibrate_raw_clamping(void) {
    xpt2046_set_calibration(300, 3800, 300, 3800);
    int16_t out_x = -1, out_y = -1;

    // Below minimum bounds should clamp to 0
    xpt2046_calibrate_raw(100, 50, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(0, out_x);
    TEST_ASSERT_EQUAL_INT16(0, out_y);

    // Negative raw ADC should clamp to 0
    xpt2046_calibrate_raw(-200, -500, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(0, out_x);
    TEST_ASSERT_EQUAL_INT16(0, out_y);

    // Above maximum bounds should clamp to max landscape coordinates
    xpt2046_calibrate_raw(4000, 4200, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_WIDTH - 1, out_x);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_HEIGHT - 1, out_y);

    // Mixed out of bounds: X below min, Y above max
    xpt2046_calibrate_raw(50, 4500, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(0, out_x);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_HEIGHT - 1, out_y);

    // Mixed out of bounds: X above max, Y below min
    xpt2046_calibrate_raw(4500, 50, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_WIDTH - 1, out_x);
    TEST_ASSERT_EQUAL_INT16(0, out_y);
}

void test_xpt2046_calibrate_raw_null_safety(void) {
    int16_t out_x = -1;
    int16_t out_y = -1;

    // out_x is NULL: out_y receives calibrated coordinate without crash
    xpt2046_calibrate_raw(2000, 2000, NULL, &out_y);
    TEST_ASSERT_TRUE(out_y >= 0 && out_y < CYD_TFT_HEIGHT);

    // out_y is NULL: out_x receives calibrated coordinate without crash
    xpt2046_calibrate_raw(2000, 2000, &out_x, NULL);
    TEST_ASSERT_TRUE(out_x >= 0 && out_x < CYD_TFT_WIDTH);

    // Both NULL should safely return without crashing
    xpt2046_calibrate_raw(2000, 2000, NULL, NULL);
    TEST_ASSERT_TRUE(true);
}

void test_xpt2046_custom_calibration(void) {
    // Custom range: X (500 to 3500), Y (400 to 3600)
    xpt2046_set_calibration(500, 3500, 400, 3600);
    int16_t out_x = -1, out_y = -1;

    xpt2046_calibrate_raw(500, 400, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(0, out_x);
    TEST_ASSERT_EQUAL_INT16(0, out_y);

    xpt2046_calibrate_raw(3500, 3600, &out_x, &out_y);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_WIDTH - 1, out_x);
    TEST_ASSERT_EQUAL_INT16(CYD_TFT_HEIGHT - 1, out_y);

    // Restore standard CYD calibration defaults
    xpt2046_set_calibration(XPT2046_RAW_X_MIN, XPT2046_RAW_X_MAX,
                           XPT2046_RAW_Y_MIN, XPT2046_RAW_Y_MAX);
}

void test_xpt2046_read_mock(void) {
    int16_t out_x = 999, out_y = 999;

    // When screen is untouched
    xpt2046_set_mock_touch(false, 0, 0);
    bool pressed = xpt2046_read(&out_x, &out_y);
    TEST_ASSERT_FALSE(pressed);
    TEST_ASSERT_EQUAL_INT16(999, out_x);
    TEST_ASSERT_EQUAL_INT16(999, out_y);

    // When screen is touched at center
    xpt2046_set_mock_touch(true, 2050, 2050);
    pressed = xpt2046_read(&out_x, &out_y);
    TEST_ASSERT_TRUE(pressed);
    TEST_ASSERT_EQUAL_INT16(159, out_x);
    TEST_ASSERT_EQUAL_INT16(119, out_y);

    // Read with NULL output pointers should return touch status safely
    pressed = xpt2046_read(NULL, NULL);
    TEST_ASSERT_TRUE(pressed);
}

void test_ili9341_flush_normal_window(void) {
    uint32_t count_before = ili9341_get_flush_count();
    ili9341_flush(10, 20, 100, 150, s_dummy_pixels);

    int32_t x1 = -1, y1 = -1, x2 = -1, y2 = -1;
    ili9341_get_last_flush_window(&x1, &y1, &x2, &y2);

    TEST_ASSERT_EQUAL_INT32(10, x1);
    TEST_ASSERT_EQUAL_INT32(20, y1);
    TEST_ASSERT_EQUAL_INT32(100, x2);
    TEST_ASSERT_EQUAL_INT32(150, y2);
    TEST_ASSERT_EQUAL_UINT32(count_before + 1, ili9341_get_flush_count());
}

void test_ili9341_flush_bounds_clamping(void) {
    uint32_t count_before = ili9341_get_flush_count();

    // Negative coordinates clamped to 0
    ili9341_flush(-25, -40, 150, 100, s_dummy_pixels);
    int32_t x1, y1, x2, y2;
    ili9341_get_last_flush_window(&x1, &y1, &x2, &y2);
    TEST_ASSERT_EQUAL_INT32(0, x1);
    TEST_ASSERT_EQUAL_INT32(0, y1);
    TEST_ASSERT_EQUAL_INT32(150, x2);
    TEST_ASSERT_EQUAL_INT32(100, y2);

    // Oversized coordinates clamped to screen width-1 and height-1
    ili9341_flush(50, 60, 400, 350, s_dummy_pixels);
    ili9341_get_last_flush_window(&x1, &y1, &x2, &y2);
    TEST_ASSERT_EQUAL_INT32(50, x1);
    TEST_ASSERT_EQUAL_INT32(60, y1);
    TEST_ASSERT_EQUAL_INT32(CYD_TFT_WIDTH - 1, x2);
    TEST_ASSERT_EQUAL_INT32(CYD_TFT_HEIGHT - 1, y2);

    // Full screen overflow clamped to complete display area
    ili9341_flush(-100, -100, 500, 500, s_dummy_pixels);
    ili9341_get_last_flush_window(&x1, &y1, &x2, &y2);
    TEST_ASSERT_EQUAL_INT32(0, x1);
    TEST_ASSERT_EQUAL_INT32(0, y1);
    TEST_ASSERT_EQUAL_INT32(CYD_TFT_WIDTH - 1, x2);
    TEST_ASSERT_EQUAL_INT32(CYD_TFT_HEIGHT - 1, y2);

    TEST_ASSERT_EQUAL_UINT32(count_before + 3, ili9341_get_flush_count());
}

void test_ili9341_flush_inverted_coordinates(void) {
    // Passing inverted bounds x1 > x2 and y1 > y2 should be normalized
    ili9341_flush(150, 180, 20, 30, s_dummy_pixels);
    int32_t x1, y1, x2, y2;
    ili9341_get_last_flush_window(&x1, &y1, &x2, &y2);
    TEST_ASSERT_EQUAL_INT32(20, x1);
    TEST_ASSERT_EQUAL_INT32(30, y1);
    TEST_ASSERT_EQUAL_INT32(150, x2);
    TEST_ASSERT_EQUAL_INT32(180, y2);
}

void test_ili9341_flush_invalid_inputs(void) {
    uint32_t count_before = ili9341_get_flush_count();

    // NULL pixel pointer should be rejected
    ili9341_flush(0, 0, 50, 50, NULL);
    TEST_ASSERT_EQUAL_UINT32(count_before, ili9341_get_flush_count());

    // Completely out-of-bounds window (all beyond width/height)
    ili9341_flush(350, 260, 400, 300, s_dummy_pixels);
    TEST_ASSERT_EQUAL_UINT32(count_before, ili9341_get_flush_count());

    // Completely negative window
    ili9341_flush(-200, -200, -50, -50, s_dummy_pixels);
    TEST_ASSERT_EQUAL_UINT32(count_before, ili9341_get_flush_count());
}

void test_ili9341_backlight_control(void) {
    ili9341_set_backlight(75);
    TEST_ASSERT_EQUAL_UINT8(75, ili9341_get_backlight());

    ili9341_set_backlight(0);
    TEST_ASSERT_EQUAL_UINT8(0, ili9341_get_backlight());

    ili9341_set_backlight(100);
    TEST_ASSERT_EQUAL_UINT8(100, ili9341_get_backlight());

    // Over 100% clamped to 100%
    ili9341_set_backlight(180);
    TEST_ASSERT_EQUAL_UINT8(100, ili9341_get_backlight());
}

#if defined(ESP_PLATFORM)
TEST_CASE("CYD XPT2046 Calibrate Raw Normal", "[cyd_drivers]") {
    test_xpt2046_calibrate_raw_normal();
}
TEST_CASE("CYD XPT2046 Calibrate Raw Clamping", "[cyd_drivers]") {
    test_xpt2046_calibrate_raw_clamping();
}
TEST_CASE("CYD XPT2046 Calibrate Raw Null Safety", "[cyd_drivers]") {
    test_xpt2046_calibrate_raw_null_safety();
}
TEST_CASE("CYD XPT2046 Custom Calibration", "[cyd_drivers]") {
    test_xpt2046_custom_calibration();
}
TEST_CASE("CYD XPT2046 Read Mock", "[cyd_drivers]") {
    test_xpt2046_read_mock();
}
TEST_CASE("CYD ILI9341 Flush Normal Window", "[cyd_drivers]") {
    test_ili9341_flush_normal_window();
}
TEST_CASE("CYD ILI9341 Flush Bounds Clamping", "[cyd_drivers]") {
    test_ili9341_flush_bounds_clamping();
}
TEST_CASE("CYD ILI9341 Flush Inverted Coordinates", "[cyd_drivers]") {
    test_ili9341_flush_inverted_coordinates();
}
TEST_CASE("CYD ILI9341 Flush Invalid Inputs", "[cyd_drivers]") {
    test_ili9341_flush_invalid_inputs();
}
TEST_CASE("CYD ILI9341 Backlight Control", "[cyd_drivers]") {
    test_ili9341_backlight_control();
}
#else
int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_xpt2046_calibrate_raw_normal);
    RUN_TEST(test_xpt2046_calibrate_raw_clamping);
    RUN_TEST(test_xpt2046_calibrate_raw_null_safety);
    RUN_TEST(test_xpt2046_custom_calibration);
    RUN_TEST(test_xpt2046_read_mock);
    RUN_TEST(test_ili9341_flush_normal_window);
    RUN_TEST(test_ili9341_flush_bounds_clamping);
    RUN_TEST(test_ili9341_flush_inverted_coordinates);
    RUN_TEST(test_ili9341_flush_invalid_inputs);
    RUN_TEST(test_ili9341_backlight_control);
    return UNITY_END();
}
#endif
