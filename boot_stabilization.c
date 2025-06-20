/*
 * Power-On Reset Enhancement for ESP32
 *
 * This code implements a software workaround for ESP32s that have
 * issues with cold boot but work fine after programming mode reset.
 *
 * Place this in your main application initialization.
 */

#include "esp_system.h"
#include "esp_log.h"
#include "esp_sleep.h"
#include "driver/rtc_io.h"

static const char *TAG = "BOOT_FIX";

void implement_boot_stabilization(void)
{
     // Check if this is a power-on reset
     esp_reset_reason_t reset_reason = esp_reset_reason();

     if (reset_reason == ESP_RST_POWERON)
     {
          ESP_LOGI(TAG, "Power-on reset detected, implementing stabilization");

          // Add a delay to allow power to stabilize
          vTaskDelay(pdMS_TO_TICKS(100));

          // Configure critical pins with proper pull-ups
          gpio_config_t io_conf = {
              .intr_type = GPIO_INTR_DISABLE,
              .mode = GPIO_MODE_INPUT,
              .pin_bit_mask = (1ULL << GPIO_NUM_0) | (1ULL << GPIO_NUM_2),
              .pull_down_en = 0,
              .pull_up_en = 1,
          };
          gpio_config(&io_conf);

          // Brief delay for GPIO stabilization
          vTaskDelay(pdMS_TO_TICKS(50));

          ESP_LOGI(TAG, "Boot stabilization complete");
     }
}

/*
 * Alternative: Force a software reset on problematic power-on
 * Use this only if the above doesn't work
 */
void force_clean_boot_on_power_on(void)
{
     esp_reset_reason_t reset_reason = esp_reset_reason();

     if (reset_reason == ESP_RST_POWERON)
     {
          static bool first_boot = true;

          if (first_boot)
          {
               ESP_LOGI(TAG, "First power-on boot, forcing clean restart");
               first_boot = false;
               vTaskDelay(pdMS_TO_TICKS(100)); // Allow system to stabilize
               esp_restart();                  // Clean restart
          }
     }
}
