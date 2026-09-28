/* Example test application for testable component.

   This example code is in the Public Domain (or CC0 licensed, at your option.)

   Unless required by applicable law or agreed to in writing, this
   software is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
   CONDITIONS OF ANY KIND, either express or implied.
*/

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include <stdbool.h>
#include "unity.h"
#include "cJSON.h"
#include "driver/uart.h"
/* 
 *  Squeezelite for esp32
 *
 *  (c) Sebastien 2019
 *      Philippe G. 2019, philippe_44@outlook.com
 *
 *  This software is released under the MIT License.
 *  https://opensource.org/licenses/MIT
 *
 */

#include "platform_esp32.h"
#include "led.h"
#include <stdio.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "freertos/task.h"
#include "esp_system.h"
#include "esp_spi_flash.h"
#include "esp_wifi.h"
#include "esp_system.h"
#include <esp_event.h>
#include "nvs_flash.h"
#include "esp_log.h"
#include "freertos/event_groups.h"
#include "mdns.h"
#include "lwip/api.h"
#include "lwip/err.h"
#include "lwip/netdb.h"
#include "nvs_utilities.h"
#include "trace.h"
#include "network_manager.h"
#include "squeezelite-ota.h"
#include <math.h>
#include "audio_controls.h"
#include "platform_config.h"
#include "telnet.h"
#include "messaging.h"
#include "gds.h"
#include "gds_default_if.h"
#include "gds_draw.h"
#include "gds_text.h"
#include "gds_font.h"
#include "display.h"
#include "accessors.h"
#include "cmd_system.h"
#include "cmd_config.h"
#include "cmd_i2ctools.h"
#include "cmd_nvs.h"
#include "tools.h"
const char unknown_string_placeholder[] = "unknown";
const char null_string_placeholder[] = "null";
// as an exception _init function don't need include
extern void services_init(void);
const char * str_or_unknown(const char * str) { return (str?str:unknown_string_placeholder); }
const char * str_or_null(const char * str) { return (str?str:null_string_placeholder); }
bool is_recovery_running;
extern void initialize_console();
/* brief this is an exemple of a callback that you can setup in your own app to get notified of wifi manager event */
esp_err_t update_certificates(bool force){return ESP_OK; }
void init_commands(){
	initialize_console();
	/* Register commands */
	register_system();
	register_config_cmd();
	register_nvs();
	register_i2ctools();
}
void test_init()
{
	const esp_partition_t *running = esp_ota_get_running_partition();
	is_recovery_running = (running != NULL && running->subtype == ESP_PARTITION_SUBTYPE_APP_FACTORY);
	initialize_nvs();
	config_init();
	services_init();
	init_commands();
}

static void print_banner(const char* text);

/* =========================================================================
 * QA-006 Test Suite Helpers and Protocol Definitions
 * ========================================================================= */

static inline uint32_t test_unpack_u32(const uint8_t *p) {
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}

static inline uint16_t test_unpack_u16(const uint8_t *p) {
    return ((uint16_t)p[0] << 8) | (uint16_t)p[1];
}

static inline void test_pack_u32(uint8_t *p, uint32_t val) {
    p[0] = (uint8_t)(val >> 24);
    p[1] = (uint8_t)(val >> 16);
    p[2] = (uint8_t)(val >> 8);
    p[3] = (uint8_t)(val);
}

static inline void test_pack_u16(uint8_t *p, uint16_t val) {
    p[0] = (uint8_t)(val >> 8);
    p[1] = (uint8_t)(val);
}

static bool test_case_insensitive_contains(const char *haystack, const char *needle) {
    if (!haystack || !needle) return false;
    size_t nlen = strlen(needle);
    if (nlen == 0) return true;
    for (; *haystack; haystack++) {
        if (strncasecmp(haystack, needle, nlen) == 0) return true;
    }
    return false;
}

/* -------------------------------------------------------------------------
 * Slimproto Packet Structures and Validation
 * ------------------------------------------------------------------------- */
#pragma pack(push, 1)
typedef struct {
    char opcode[4];
    char command;
    uint8_t autostart;
    uint8_t format;
    uint8_t pcm_sample_size;
    uint8_t pcm_sample_rate;
    uint8_t pcm_channels;
    uint8_t pcm_endianness;
    uint8_t threshold;
    uint8_t spdif_enable;
    uint8_t transition_period;
    uint8_t transition_type;
    uint8_t flags;
    uint8_t output_threshold;
    uint8_t slaves;
    uint32_t replay_gain;
    uint16_t server_port;
    uint32_t server_ip;
} test_slim_strm_t;

typedef struct {
    char opcode[4];
    uint32_t metaint;
    uint8_t loop;
} test_slim_cont_t;

typedef struct {
    char opcode[4];
    uint8_t format;
    uint8_t pcm_sample_size;
    uint8_t pcm_sample_rate;
    uint8_t pcm_channels;
    uint8_t pcm_endianness;
} test_slim_codc_t;

typedef struct {
    char opcode[4];
    uint8_t enable_spdif;
    uint8_t enable_dac;
} test_slim_aude_t;

typedef struct {
    char opcode[4];
    uint32_t old_gainL;
    uint32_t old_gainR;
    uint8_t adjust;
    uint8_t preamp;
    uint32_t gainL;
    uint32_t gainR;
} test_slim_audg_t;

typedef struct {
    char opcode[4];
    uint8_t id;
    char data[];
} test_slim_setd_t;

typedef struct {
    char opcode[4];
    uint32_t server_ip;
} test_slim_serv_t;
#pragma pack(pop)

typedef enum {
    SLIM_PARSE_OK = 0,
    SLIM_PARSE_ERR_TOO_SHORT,
    SLIM_PARSE_ERR_BAD_SIZE,
    SLIM_PARSE_ERR_UNKNOWN_OPCODE,
    SLIM_PARSE_ERR_INVALID_CMD
} slim_parse_err_t;

static slim_parse_err_t test_parse_slimproto(const uint8_t *buf, size_t len, char *opcode_out) {
    if (!buf || len < 4) return SLIM_PARSE_ERR_TOO_SHORT;
    if (opcode_out) {
        memcpy(opcode_out, buf, 4);
        opcode_out[4] = '\0';
    }
    if (memcmp(buf, "strm", 4) == 0) {
        if (len < sizeof(test_slim_strm_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        const test_slim_strm_t *s = (const test_slim_strm_t *)buf;
        if (s->command != 't' && s->command != 'f' && s->command != 'q' &&
            s->command != 'p' && s->command != 'a' && s->command != 'u') {
            return SLIM_PARSE_ERR_INVALID_CMD;
        }
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "cont", 4) == 0) {
        if (len < sizeof(test_slim_cont_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "codc", 4) == 0) {
        if (len < sizeof(test_slim_codc_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "aude", 4) == 0) {
        if (len < sizeof(test_slim_aude_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "audg", 4) == 0) {
        if (len < sizeof(test_slim_audg_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "setd", 4) == 0) {
        if (len < 5) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "serv", 4) == 0) {
        if (len < sizeof(test_slim_serv_t)) return SLIM_PARSE_ERR_BAD_SIZE;
        return SLIM_PARSE_OK;
    } else if (memcmp(buf, "dsco", 4) == 0) {
        return SLIM_PARSE_OK;
    }
    return SLIM_PARSE_ERR_UNKNOWN_OPCODE;
}

static size_t test_parse_setd_name(const uint8_t *pkt, size_t len, char *out_name, size_t max_name_len) {
    if (!pkt || len < 5) return 0;
    const test_slim_setd_t *setd = (const test_slim_setd_t *)pkt;
    if (setd->id != 0) return 0;
    if (len == 5) {
        if (out_name && max_name_len > 0) out_name[0] = '\0';
        return 0;
    }
    size_t dlen = len - 5;
    if (dlen >= max_name_len) dlen = max_name_len - 1;
    if (out_name) {
        memcpy(out_name, setd->data, dlen);
        out_name[dlen] = '\0';
    }
    return dlen;
}

/* -------------------------------------------------------------------------
 * RTSP Header Parsing and Validation
 * ------------------------------------------------------------------------- */
#define TEST_RTSP_MAX_HEADERS 15
#define TEST_RTSP_MAX_CONTENT_LENGTH 65536

typedef struct {
    char key[48];
    char value[128];
} test_rtsp_hdr_t;

typedef struct {
    char method[16];
    char uri[128];
    char version[16];
    test_rtsp_hdr_t headers[TEST_RTSP_MAX_HEADERS];
    int header_count;
    int content_length;
} test_rtsp_parsed_t;

typedef enum {
    TEST_RTSP_OK = 0,
    TEST_RTSP_ERR_NULL,
    TEST_RTSP_ERR_BAD_REQUEST_LINE,
    TEST_RTSP_ERR_BAD_HEADER,
    TEST_RTSP_ERR_TOO_MANY_HEADERS,
    TEST_RTSP_ERR_INVALID_CONTENT_LENGTH
} test_rtsp_err_t;

static test_rtsp_err_t test_parse_rtsp_request(const char *raw, test_rtsp_parsed_t *out) {
    if (!raw || !out) return TEST_RTSP_ERR_NULL;
    memset(out, 0, sizeof(*out));

    const char *p = raw;
    const char *eol = strstr(p, "\r\n");
    if (!eol) eol = strchr(p, '\n');
    if (!eol) return TEST_RTSP_ERR_BAD_REQUEST_LINE;

    char line[256];
    size_t llen = eol - p;
    if (llen >= sizeof(line)) llen = sizeof(line) - 1;
    memcpy(line, p, llen);
    line[llen] = '\0';

    if (sscanf(line, "%15s %127s %15s", out->method, out->uri, out->version) < 2) {
        return TEST_RTSP_ERR_BAD_REQUEST_LINE;
    }

    p = eol + (*eol == '\r' ? 2 : 1);

    while (*p != '\0') {
        if (*p == '\r' && *(p+1) == '\n') break;
        if (*p == '\n') break;

        const char *next_eol = strstr(p, "\r\n");
        size_t cur_len = next_eol ? (size_t)(next_eol - p) : strlen(p);
        if (cur_len >= sizeof(line)) cur_len = sizeof(line) - 1;
        memcpy(line, p, cur_len);
        line[cur_len] = '\0';

        if ((line[0] == ' ' || line[0] == '\t') && out->header_count > 0) {
            char *prev_val = out->headers[out->header_count - 1].value;
            size_t pv_len = strlen(prev_val);
            const char *start = line;
            while (*start == ' ' || *start == '\t') start++;
            snprintf(prev_val + pv_len, sizeof(out->headers[0].value) - pv_len, " %s", start);
        } else {
            char *colon = strchr(line, ':');
            if (!colon) return TEST_RTSP_ERR_BAD_HEADER;

            if (out->header_count >= TEST_RTSP_MAX_HEADERS) {
                return TEST_RTSP_ERR_TOO_MANY_HEADERS;
            }

            *colon = '\0';
            char *k = line;
            while (*k == ' ') k++;
            char *v = colon + 1;
            while (*v == ' ' || *v == '\t') v++;

            strlcpy(out->headers[out->header_count].key, k, sizeof(out->headers[0].key));
            strlcpy(out->headers[out->header_count].value, v, sizeof(out->headers[0].value));

            if (strcasecmp(k, "Content-Length") == 0) {
                long cl = atol(v);
                if (cl < 0 || cl > TEST_RTSP_MAX_CONTENT_LENGTH) {
                    return TEST_RTSP_ERR_INVALID_CONTENT_LENGTH;
                }
                out->content_length = (int)cl;
            }
            out->header_count++;
        }

        if (!next_eol) break;
        p = next_eol + 2;
    }

    return TEST_RTSP_OK;
}

static const char *test_rtsp_lookup_hdr(const test_rtsp_parsed_t *parsed, const char *key) {
    if (!parsed || !key) return NULL;
    for (int i = 0; i < parsed->header_count; i++) {
        if (strcasecmp(parsed->headers[i].key, key) == 0) {
            return parsed->headers[i].value;
        }
    }
    return NULL;
}

/* -------------------------------------------------------------------------
 * DNS Packet Structures and Validation
 * ------------------------------------------------------------------------- */
#define TEST_DNS_QUERY_MAX_LEN 80
#define TEST_DNS_ANSWER_MAX_LEN 96

#pragma pack(push, 1)
typedef struct {
    uint16_t id;
    uint8_t rd : 1;
    uint8_t tc : 1;
    uint8_t aa : 1;
    uint8_t opcode : 4;
    uint8_t qr : 1;
    uint8_t rcode : 4;
    uint8_t z : 3;
    uint8_t ra : 1;
    uint16_t qdcount;
    uint16_t ancount;
    uint16_t nscount;
    uint16_t arcount;
} test_dns_hdr_t;

typedef struct {
    uint16_t name;
    uint16_t type;
    uint16_t class_;
    uint32_t ttl;
    uint16_t rdlength;
    uint32_t rdata;
} test_dns_ans_t;
#pragma pack(pop)

typedef enum {
    TEST_DNS_OK = 0,
    TEST_DNS_ERR_NULL,
    TEST_DNS_ERR_TOO_SHORT,
    TEST_DNS_ERR_TOO_LARGE,
} test_dns_err_t;

static test_dns_err_t test_validate_dns_packet(const uint8_t *pkt, size_t len) {
    if (!pkt) return TEST_DNS_ERR_NULL;
    if (len < sizeof(test_dns_hdr_t)) {
        return TEST_DNS_ERR_TOO_SHORT;
    }
    if ((len + sizeof(test_dns_ans_t) - 1) >= TEST_DNS_ANSWER_MAX_LEN) {
        return TEST_DNS_ERR_TOO_LARGE;
    }
    return TEST_DNS_OK;
}

static void test_sanitize_dns_domain(char *domain, size_t len) {
    if (!domain) return;
    for (size_t i = 0; i < len && domain[i] != '\0'; i++) {
        if (domain[i] < ' ' || domain[i] > 'z') {
            domain[i] = '.';
        }
    }
}

static int test_build_dns_response(const uint8_t *query, size_t query_len, uint32_t resolved_ip, uint8_t *resp_buf, size_t resp_max) {
    if (test_validate_dns_packet(query, query_len) != TEST_DNS_OK) return -1;
    if (resp_max < query_len + sizeof(test_dns_ans_t)) return -1;

    memcpy(resp_buf, query, query_len);
    test_dns_hdr_t *hdr = (test_dns_hdr_t *)resp_buf;
    hdr->qr = 1;
    hdr->aa = 1;
    hdr->rcode = 0;
    hdr->tc = 0;
    hdr->rd = 0;
    hdr->ancount = hdr->qdcount;
    hdr->nscount = 0;
    hdr->arcount = 0;

    test_dns_ans_t *ans = (test_dns_ans_t *)&resp_buf[query_len];
    test_pack_u16((uint8_t *)&ans->name, 0xC00C);
    test_pack_u16((uint8_t *)&ans->type, 1);
    test_pack_u16((uint8_t *)&ans->class_, 1);
    ans->ttl = 0;
    test_pack_u16((uint8_t *)&ans->rdlength, 4);
    test_pack_u32((uint8_t *)&ans->rdata, resolved_ip);

    return (int)(query_len + sizeof(test_dns_ans_t));
}

/* -------------------------------------------------------------------------
 * NVS Credential Redaction Helpers
 * ------------------------------------------------------------------------- */
static bool test_is_nvs_sensitive_key(const char *key) {
    if (!key) return false;
    return (test_case_insensitive_contains(key, "password") ||
            test_case_insensitive_contains(key, "token") ||
            test_case_insensitive_contains(key, "telnet_pwd") ||
            test_case_insensitive_contains(key, "ap_pwd"));
}

static const char *test_redact_nvs_val(const char *key, const char *raw) {
    if (!raw) return "";
    return test_is_nvs_sensitive_key(key) ? "[REDACTED]" : raw;
}

/* -------------------------------------------------------------------------
 * OTA URL Validation Rules
 * ------------------------------------------------------------------------- */
#define TEST_OTA_MIN_URL 10
#define TEST_OTA_MAX_URL 256

typedef enum {
    TEST_OTA_VALID = 0,
    TEST_OTA_ERR_NULL,
    TEST_OTA_ERR_TOO_SHORT,
    TEST_OTA_ERR_TOO_LONG,
    TEST_OTA_ERR_BAD_SCHEME,
    TEST_OTA_ERR_MISSING_BIN,
} test_ota_err_t;

static test_ota_err_t test_validate_ota_url(const char *url) {
    if (!url) return TEST_OTA_ERR_NULL;
    size_t len = strlen(url);
    if (len < TEST_OTA_MIN_URL) return TEST_OTA_ERR_TOO_SHORT;
    if (len > TEST_OTA_MAX_URL) return TEST_OTA_ERR_TOO_LONG;

    if (strncasecmp(url, "https://", 8) != 0) {
        return TEST_OTA_ERR_BAD_SCHEME;
    }

    if (!strstr(url, ".bin")) {
        return TEST_OTA_ERR_MISSING_BIN;
    }

    return TEST_OTA_VALID;
}

/* =========================================================================
 * QA-006 Unit Test Cases
 * ========================================================================= */

/* --- Slimproto Tests --- */
TEST_CASE("Slimproto packet minimum size and truncated packet validation", "[slimproto][protocol]")
{
    char opcode[5] = {0};
    uint8_t short_pkt[3] = {'s', 't', 'r'};

    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_TOO_SHORT, test_parse_slimproto(NULL, 0, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_TOO_SHORT, test_parse_slimproto(short_pkt, 0, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_TOO_SHORT, test_parse_slimproto(short_pkt, 1, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_TOO_SHORT, test_parse_slimproto(short_pkt, 2, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_TOO_SHORT, test_parse_slimproto(short_pkt, 3, opcode));

    uint8_t unknown_pkt[4] = {'x', 'y', 'z', 'w'};
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_UNKNOWN_OPCODE, test_parse_slimproto(unknown_pkt, 4, opcode));
    TEST_ASSERT_EQUAL_STRING("xyzw", opcode);
}

TEST_CASE("Slimproto strm packet bounds, commands, and field unpacking", "[slimproto][protocol]")
{
    char opcode[5] = {0};
    uint8_t truncated_strm[20] = {'s', 't', 'r', 'm'};
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto(truncated_strm, sizeof(truncated_strm), opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto(truncated_strm, sizeof(test_slim_strm_t) - 1, opcode));

    test_slim_strm_t valid_strm;
    memset(&valid_strm, 0, sizeof(valid_strm));
    memcpy(valid_strm.opcode, "strm", 4);
    valid_strm.command = 'p';
    valid_strm.autostart = 1;
    valid_strm.format = 'm';
    test_pack_u32((uint8_t *)&valid_strm.replay_gain, 45000);
    test_pack_u16((uint8_t *)&valid_strm.server_port, 9000);
    test_pack_u32((uint8_t *)&valid_strm.server_ip, 0xC0A8010A);

    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((const uint8_t *)&valid_strm, sizeof(valid_strm), opcode));
    TEST_ASSERT_EQUAL_STRING("strm", opcode);
    TEST_ASSERT_EQUAL_UINT32(45000, test_unpack_u32((const uint8_t *)&valid_strm.replay_gain));
    TEST_ASSERT_EQUAL(9000, test_unpack_u16((const uint8_t *)&valid_strm.server_port));
    TEST_ASSERT_EQUAL_UINT32(0xC0A8010A, test_unpack_u32((const uint8_t *)&valid_strm.server_ip));

    const char valid_cmds[] = {'t', 'f', 'q', 'p', 'a', 'u'};
    for (size_t i = 0; i < sizeof(valid_cmds); i++) {
        valid_strm.command = valid_cmds[i];
        TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((const uint8_t *)&valid_strm, sizeof(valid_strm), opcode));
    }

    valid_strm.command = 'X';
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_INVALID_CMD, test_parse_slimproto((const uint8_t *)&valid_strm, sizeof(valid_strm), opcode));
}

TEST_CASE("Slimproto cont, codc, aude, audg, serv packet boundary checks", "[slimproto][protocol]")
{
    char opcode[5] = {0};

    uint8_t cont_buf[16] = {'c', 'o', 'n', 't'};
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto(cont_buf, 8, opcode));
    test_pack_u32(&cont_buf[4], 16000);
    cont_buf[8] = 1;
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto(cont_buf, 9, opcode));
    TEST_ASSERT_EQUAL_UINT32(16000, test_unpack_u32(&cont_buf[4]));

    test_slim_codc_t codc;
    memcpy(codc.opcode, "codc", 4);
    codc.format = 'f';
    codc.pcm_sample_size = 16;
    codc.pcm_sample_rate = 4;
    codc.pcm_channels = 2;
    codc.pcm_endianness = 1;
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto((uint8_t *)&codc, sizeof(codc) - 1, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((uint8_t *)&codc, sizeof(codc), opcode));

    test_slim_aude_t aude;
    memcpy(aude.opcode, "aude", 4);
    aude.enable_spdif = 1;
    aude.enable_dac = 1;
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto((uint8_t *)&aude, sizeof(aude) - 1, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((uint8_t *)&aude, sizeof(aude), opcode));

    test_slim_audg_t audg;
    memset(&audg, 0, sizeof(audg));
    memcpy(audg.opcode, "audg", 4);
    audg.adjust = 1;
    test_pack_u32((uint8_t *)&audg.gainL, 65536);
    test_pack_u32((uint8_t *)&audg.gainR, 65536);
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto((uint8_t *)&audg, sizeof(audg) - 1, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((uint8_t *)&audg, sizeof(audg), opcode));

    test_slim_serv_t serv;
    memcpy(serv.opcode, "serv", 4);
    test_pack_u32((uint8_t *)&serv.server_ip, 0x7F000001);
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto((uint8_t *)&serv, sizeof(serv) - 1, opcode));
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto((uint8_t *)&serv, sizeof(serv), opcode));

    uint8_t dsco[4] = {'d', 's', 'c', 'o'};
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto(dsco, 4, opcode));
}

TEST_CASE("Slimproto setd name length bounds and clamping", "[slimproto][protocol]")
{
    char opcode[5] = {0};

    uint8_t short_setd[4] = {'s', 'e', 't', 'd'};
    TEST_ASSERT_EQUAL(SLIM_PARSE_ERR_BAD_SIZE, test_parse_slimproto(short_setd, 4, opcode));

    uint8_t query_setd[5] = {'s', 'e', 't', 'd', 0x00};
    TEST_ASSERT_EQUAL(SLIM_PARSE_OK, test_parse_slimproto(query_setd, 5, opcode));

    char name_buf[32] = {0};
    size_t parsed_len = test_parse_setd_name(query_setd, 5, name_buf, sizeof(name_buf));
    TEST_ASSERT_EQUAL(0, parsed_len);

    uint8_t setd_buf[64] = {'s', 'e', 't', 'd', 0x00};
    const char *test_name = "Kitchen Speaker";
    size_t tname_len = strlen(test_name);
    memcpy(&setd_buf[5], test_name, tname_len);
    parsed_len = test_parse_setd_name(setd_buf, 5 + tname_len, name_buf, sizeof(name_buf));
    TEST_ASSERT_EQUAL(tname_len, parsed_len);
    TEST_ASSERT_EQUAL_STRING("Kitchen Speaker", name_buf);

    char small_buf[8] = {0};
    parsed_len = test_parse_setd_name(setd_buf, 5 + tname_len, small_buf, sizeof(small_buf));
    TEST_ASSERT_EQUAL(sizeof(small_buf) - 1, parsed_len);
    TEST_ASSERT_EQUAL_STRING("Kitchen", small_buf);
}

/* --- RTSP Tests --- */
TEST_CASE("RTSP request parsing and method boundary checks", "[rtsp][protocol]")
{
    test_rtsp_parsed_t parsed;

    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_NULL, test_parse_rtsp_request(NULL, &parsed));
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_BAD_REQUEST_LINE, test_parse_rtsp_request("", &parsed));
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_BAD_REQUEST_LINE, test_parse_rtsp_request("INVALID_NO_EOL", &parsed));

    const char *setup_req =
        "SETUP rtsp://192.168.1.50/stream RTSP/1.0\r\n"
        "CSeq: 1\r\n"
        "Transport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n\r\n";

    TEST_ASSERT_EQUAL(TEST_RTSP_OK, test_parse_rtsp_request(setup_req, &parsed));
    TEST_ASSERT_EQUAL_STRING("SETUP", parsed.method);
    TEST_ASSERT_EQUAL_STRING("rtsp://192.168.1.50/stream", parsed.uri);
    TEST_ASSERT_EQUAL_STRING("RTSP/1.0", parsed.version);
    TEST_ASSERT_EQUAL(2, parsed.header_count);

    TEST_ASSERT_EQUAL_STRING("1", test_rtsp_lookup_hdr(&parsed, "CSeq"));
    TEST_ASSERT_EQUAL_STRING("1", test_rtsp_lookup_hdr(&parsed, "cseq"));
    TEST_ASSERT_NOT_NULL(test_rtsp_lookup_hdr(&parsed, "Transport"));
    TEST_ASSERT_NULL(test_rtsp_lookup_hdr(&parsed, "NonExistent"));
}

TEST_CASE("RTSP header syntax, whitespace trimming, and line folding", "[rtsp][protocol]")
{
    test_rtsp_parsed_t parsed;

    const char *bad_hdr =
        "OPTIONS * RTSP/1.0\r\n"
        "CSeq 1\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_BAD_HEADER, test_parse_rtsp_request(bad_hdr, &parsed));

    const char *folded_req =
        "DESCRIBE rtsp://audio RTSP/1.0\r\n"
        "CSeq:    42   \r\n"
        "User-Agent: SqueezeESP32/1.0\r\n"
        " (Build 2026; ESP32-WROVER)\r\n"
        "Accept: application/sdp\r\n\r\n";

    TEST_ASSERT_EQUAL(TEST_RTSP_OK, test_parse_rtsp_request(folded_req, &parsed));
    TEST_ASSERT_EQUAL(3, parsed.header_count);
    TEST_ASSERT_EQUAL_STRING("42", test_rtsp_lookup_hdr(&parsed, "CSeq"));
    TEST_ASSERT_EQUAL_STRING("application/sdp", test_rtsp_lookup_hdr(&parsed, "Accept"));
    TEST_ASSERT_EQUAL_STRING("SqueezeESP32/1.0 (Build 2026; ESP32-WROVER)", test_rtsp_lookup_hdr(&parsed, "User-Agent"));
}

TEST_CASE("RTSP maximum header count boundary enforcement", "[rtsp][protocol]")
{
    test_rtsp_parsed_t parsed;

    char exactly_15[1024];
    int offset = snprintf(exactly_15, sizeof(exactly_15), "OPTIONS * RTSP/1.0\r\n");
    for (int i = 0; i < 15; i++) {
        offset += snprintf(exactly_15 + offset, sizeof(exactly_15) - offset, "Header-%02d: value-%d\r\n", i, i);
    }
    snprintf(exactly_15 + offset, sizeof(exactly_15) - offset, "\r\n");

    TEST_ASSERT_EQUAL(TEST_RTSP_OK, test_parse_rtsp_request(exactly_15, &parsed));
    TEST_ASSERT_EQUAL(15, parsed.header_count);

    char over_15[1024];
    offset = snprintf(over_15, sizeof(over_15), "OPTIONS * RTSP/1.0\r\n");
    for (int i = 0; i < 16; i++) {
        offset += snprintf(over_15 + offset, sizeof(over_15) - offset, "Header-%02d: value-%d\r\n", i, i);
    }
    snprintf(over_15 + offset, sizeof(over_15) - offset, "\r\n");

    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_TOO_MANY_HEADERS, test_parse_rtsp_request(over_15, &parsed));
}

TEST_CASE("RTSP Content-Length boundary validation", "[rtsp][protocol]")
{
    test_rtsp_parsed_t parsed;

    const char *cl_zero = "ANNOUNCE rtsp://srv RTSP/1.0\r\nContent-Length: 0\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_OK, test_parse_rtsp_request(cl_zero, &parsed));
    TEST_ASSERT_EQUAL(0, parsed.content_length);

    const char *cl_max = "ANNOUNCE rtsp://srv RTSP/1.0\r\nContent-Length: 65536\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_OK, test_parse_rtsp_request(cl_max, &parsed));
    TEST_ASSERT_EQUAL(65536, parsed.content_length);

    const char *cl_neg = "ANNOUNCE rtsp://srv RTSP/1.0\r\nContent-Length: -1\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_INVALID_CONTENT_LENGTH, test_parse_rtsp_request(cl_neg, &parsed));

    const char *cl_overflow = "ANNOUNCE rtsp://srv RTSP/1.0\r\nContent-Length: 65537\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_INVALID_CONTENT_LENGTH, test_parse_rtsp_request(cl_overflow, &parsed));

    const char *cl_huge = "ANNOUNCE rtsp://srv RTSP/1.0\r\nContent-Length: 99999999\r\n\r\n";
    TEST_ASSERT_EQUAL(TEST_RTSP_ERR_INVALID_CONTENT_LENGTH, test_parse_rtsp_request(cl_huge, &parsed));
}

/* --- DNS Tests --- */
TEST_CASE("DNS packet length bounds validation (min 12, max 80)", "[dns][protocol]")
{
    uint8_t buffer[128];
    memset(buffer, 0, sizeof(buffer));

    TEST_ASSERT_EQUAL(TEST_DNS_ERR_NULL, test_validate_dns_packet(NULL, 10));

    for (size_t l = 0; l < 12; l++) {
        TEST_ASSERT_EQUAL(TEST_DNS_ERR_TOO_SHORT, test_validate_dns_packet(buffer, l));
    }

    TEST_ASSERT_EQUAL(TEST_DNS_OK, test_validate_dns_packet(buffer, 12));
    TEST_ASSERT_EQUAL(TEST_DNS_OK, test_validate_dns_packet(buffer, 45));
    TEST_ASSERT_EQUAL(TEST_DNS_OK, test_validate_dns_packet(buffer, 80));
    TEST_ASSERT_EQUAL(TEST_DNS_ERR_TOO_LARGE, test_validate_dns_packet(buffer, 81));
    TEST_ASSERT_EQUAL(TEST_DNS_ERR_TOO_LARGE, test_validate_dns_packet(buffer, 96));
    TEST_ASSERT_EQUAL(TEST_DNS_ERR_TOO_LARGE, test_validate_dns_packet(buffer, 128));
}

TEST_CASE("DNS header fields, domain label sanitization, and answer formatting", "[dns][protocol]")
{
    uint8_t query[64];
    memset(query, 0, sizeof(query));

    test_dns_hdr_t *hdr = (test_dns_hdr_t *)query;
    hdr->id = 0x1234;
    hdr->qr = 0;
    hdr->opcode = 0;
    hdr->qdcount = 1;

    const char domain_raw[] = "\x06google\x03com";
    memcpy(&query[sizeof(test_dns_hdr_t) + 1], domain_raw, sizeof(domain_raw));
    size_t qlen = sizeof(test_dns_hdr_t) + 1 + sizeof(domain_raw) + 4;

    TEST_ASSERT_EQUAL(TEST_DNS_OK, test_validate_dns_packet(query, qlen));

    char domain_test[] = "captive\x01portal\x02lan";
    test_sanitize_dns_domain(domain_test, strlen(domain_test));
    TEST_ASSERT_EQUAL_STRING("captive.portal.lan", domain_test);

    uint8_t response[TEST_DNS_ANSWER_MAX_LEN];
    memset(response, 0, sizeof(response));
    int resp_len = test_build_dns_response(query, qlen, 0xC0A80401, response, sizeof(response));

    TEST_ASSERT_EQUAL((int)(qlen + sizeof(test_dns_ans_t)), resp_len);
    test_dns_hdr_t *resp_hdr = (test_dns_hdr_t *)response;
    TEST_ASSERT_EQUAL(1, resp_hdr->qr);
    TEST_ASSERT_EQUAL(1, resp_hdr->aa);
    TEST_ASSERT_EQUAL(0, resp_hdr->rcode);
    TEST_ASSERT_EQUAL(1, resp_hdr->ancount);
}

/* --- NVS & JSON Tests --- */
TEST_CASE("NVS sensitive key detection and credential redaction", "[nvs][security]")
{
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key(NULL));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key(""));

    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("password"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("PASSWORD"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("wifi_password"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("PassWord_1"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("token"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("auth_token"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("SPOTIFY_TOKEN"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("telnet_pwd"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("TELNET_PWD"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("ap_pwd"));
    TEST_ASSERT_TRUE(test_is_nvs_sensitive_key("AP_PWD"));

    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("ssid"));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("channel"));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("host_name"));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("autoexec"));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("equalizer"));
    TEST_ASSERT_FALSE(test_is_nvs_sensitive_key("volume"));

    TEST_ASSERT_EQUAL_STRING("[REDACTED]", test_redact_nvs_val("wifi_password", "SecretWiFi123"));
    TEST_ASSERT_EQUAL_STRING("[REDACTED]", test_redact_nvs_val("auth_token", "bearer-xyz-123"));
    TEST_ASSERT_EQUAL_STRING("LivingRoomSpeaker", test_redact_nvs_val("host_name", "LivingRoomSpeaker"));
    TEST_ASSERT_EQUAL_STRING("MyHomeSSID", test_redact_nvs_val("ssid", "MyHomeSSID"));
}

TEST_CASE("cJSON parser null, empty, and malformed syntax sanity", "[json]")
{
    cJSON *json = cJSON_Parse(NULL);
    TEST_ASSERT_NULL(json);

    json = cJSON_Parse("");
    TEST_ASSERT_NULL(json);

    json = cJSON_Parse("{\"key\": \"val\"");
    TEST_ASSERT_NULL(json);

    json = cJSON_Parse("{\"key\" \"val\"}");
    TEST_ASSERT_NULL(json);

    json = cJSON_Parse("{\"arr\": [1, 2, ]}");
    TEST_ASSERT_NULL(json);
}

TEST_CASE("cJSON structured configuration parsing and redaction integration", "[json][nvs]")
{
    const char *config_str =
        "{"
        "  \"ssid\": \"StudioWiFi\","
        "  \"pass\": \"TopSecret123!\","
        "  \"chan\": 6,"
        "  \"auth\": 3,"
        "  \"enabled\": true"
        "}";

    cJSON *root = cJSON_Parse(config_str);
    TEST_ASSERT_NOT_NULL(root);

    cJSON *ssid_item = cJSON_GetObjectItemCaseSensitive(root, "ssid");
    TEST_ASSERT_NOT_NULL(ssid_item);
    TEST_ASSERT_TRUE(cJSON_IsString(ssid_item));
    TEST_ASSERT_EQUAL_STRING("StudioWiFi", cJSON_GetStringValue(ssid_item));

    cJSON *chan_item = cJSON_GetObjectItemCaseSensitive(root, "chan");
    TEST_ASSERT_NOT_NULL(chan_item);
    TEST_ASSERT_TRUE(cJSON_IsNumber(chan_item));
    TEST_ASSERT_EQUAL(6, (int)chan_item->valuedouble);

    cJSON *enabled_item = cJSON_GetObjectItemCaseSensitive(root, "enabled");
    TEST_ASSERT_NOT_NULL(enabled_item);
    TEST_ASSERT_TRUE(cJSON_IsTrue(enabled_item));

    cJSON *pass_item = cJSON_GetObjectItemCaseSensitive(root, "pass");
    TEST_ASSERT_NOT_NULL(pass_item);
    const char *logged_pass_key = test_redact_nvs_val("wifi_password", cJSON_GetStringValue(pass_item));
    TEST_ASSERT_EQUAL_STRING("[REDACTED]", logged_pass_key);

    cJSON_Delete(root);
}

/* --- OTA URL Tests --- */
TEST_CASE("OTA URL length boundary validation", "[ota][security]")
{
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_NULL, test_validate_ota_url(NULL));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_TOO_SHORT, test_validate_ota_url(""));

    TEST_ASSERT_EQUAL(TEST_OTA_ERR_TOO_SHORT, test_validate_ota_url("http://a."));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_TOO_SHORT, test_validate_ota_url("http://x."));

    char url_256[257];
    memset(url_256, 'a', 256);
    memcpy(url_256, "http://example.com/", 19);
    memcpy(url_256 + 252, ".bin", 4);
    url_256[256] = '\0';
    TEST_ASSERT_EQUAL(256, strlen(url_256));
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url(url_256));

    char url_257[258];
    memset(url_257, 'a', 257);
    memcpy(url_257, "http://example.com/", 19);
    memcpy(url_257 + 253, ".bin", 4);
    url_257[257] = '\0';
    TEST_ASSERT_EQUAL(257, strlen(url_257));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_TOO_LONG, test_validate_ota_url(url_257));
}

TEST_CASE("OTA URL protocol scheme validation", "[ota][security]")
{
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url("http://192.168.1.100/ota.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url("https://releases.firmware.org/v2.bin"));

    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("HTTP://SERVER/IMAGE.BIN"));
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url("HTTPS://SERVER/IMAGE.BIN"));

    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("ftp://firmware.org/app.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("file:///sdcard/app.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("tftp://10.0.0.1/app.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("ws://streaming.org/app.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("//missing-scheme.org/app.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("releases.squeezelite.org/firmware.bin"));
}

TEST_CASE("OTA URL firmware binary extension validation", "[ota][security]")
{
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_BAD_SCHEME, test_validate_ota_url("http://domain.com/squeezelite-esp32.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url("https://domain.com/squeezelite-esp32.bin"));
    TEST_ASSERT_EQUAL(TEST_OTA_VALID, test_validate_ota_url("https://github.com/v1.0/firmware.bin?token=xyz"));

    TEST_ASSERT_EQUAL(TEST_OTA_ERR_MISSING_BIN, test_validate_ota_url("http://domain.com/squeezelite-esp32.zip"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_MISSING_BIN, test_validate_ota_url("http://domain.com/squeezelite-esp32.tar.gz"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_MISSING_BIN, test_validate_ota_url("http://domain.com/squeezelite-esp32.elf"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_MISSING_BIN, test_validate_ota_url("http://domain.com/squeezelite-esp32.hex"));
    TEST_ASSERT_EQUAL(TEST_OTA_ERR_MISSING_BIN, test_validate_ota_url("http://domain.com/firmware-image"));
}

TEST_CASE("Platform tools utf8_decodeLatin1 and high-order byte decoding", "[tools][utf8]")
{
    char test_str[64];
    // ASCII passthrough
    strcpy(test_str, "Standard ASCII 123");
    utf8_decode(test_str);
    TEST_ASSERT_EQUAL_STRING("Standard ASCII 123", test_str);

    // UTF-8 accented character (e.g. Café: C, a, f, 0xC3, 0xA9)
    uint8_t cafe_utf8[] = {'C', 'a', 'f', 0xC3, 0xA9, '\0'};
    strcpy(test_str, (char *)cafe_utf8);
    utf8_decode(test_str);
    // Should safely decode to ISO-8859-1 / CP1252 'é' (0xE9) without sign extension crash
    TEST_ASSERT_EQUAL_HEX8(0xE9, (uint8_t)test_str[3]);
}

TEST_CASE("Platform config PARSE_PARAM macro boundary and NULL safety", "[config][macros]")
{
    int val = -1;
    // NULL safety test (LOGIC-001)
    PARSE_PARAM(NULL, "rate", '=', val);
    TEST_ASSERT_EQUAL(-1, val);

    // Boundary safety test: searching for "rate" must not match "bitrate"
    const char *cfg = "bitrate=320,rate=44100";
    val = -1;
    PARSE_PARAM(cfg, "rate", '=', val);
    TEST_ASSERT_EQUAL(44100, val);

    // String parameter extraction with delimiter
    char str_buf[32] = {0};
    PARSE_PARAM_STR(cfg, "bitrate", '=', str_buf, sizeof(str_buf));
    TEST_ASSERT_EQUAL_STRING("320", str_buf);
}

/* =========================================================================
 * QA-001 Automated Test Harness and Runner Execution
 * ========================================================================= */

#ifndef CONFIG_ESP_CONSOLE_UART_NUM
#define CONFIG_ESP_CONSOLE_UART_NUM 0
#endif

#ifndef TEST_MENU_TIMEOUT_MS
#define TEST_MENU_TIMEOUT_MS 3000
#endif

static bool should_run_automated_mode(void)
{
#if defined(CONFIG_TEST_NON_INTERACTIVE) || defined(CONFIG_BATCH_MODE) || defined(CI) || defined(AUTOMATED_TESTING)
    ESP_LOGI("TEST_HARNESS", "Automated non-interactive mode enabled by build flag.");
    return true;
#else
    char *mode = config_alloc_get(NVS_TYPE_STR, "test_mode");
    if (mode) {
        bool is_auto = (strcasecmp(mode, "interactive") != 0);
        ESP_LOGI("TEST_HARNESS", "NVS test_mode='%s', auto_mode=%d", mode, is_auto);
        free(mode);
        return is_auto;
    }

    printf("\n======================================================================\n");
    printf("   SQUEEZELITE-ESP32 TEST HARNESS                                    \n");
    printf("   Press any key within %d seconds to enter interactive menu...        \n", TEST_MENU_TIMEOUT_MS / 1000);
    printf("   Defaulting to sequential automated test suite execution on timeout.\n");
    printf("======================================================================\n\n");

    int elapsed_ms = 0;
    const int step_ms = 100;
    while (elapsed_ms < TEST_MENU_TIMEOUT_MS) {
        uint8_t ch = 0;
        int bytes = uart_read_bytes(CONFIG_ESP_CONSOLE_UART_NUM, &ch, 1, pdMS_TO_TICKS(step_ms));
        if (bytes > 0 && isprint(ch) && ch != '\r' && ch != '\n') {
            printf("\nInteractive key pressed ('%c'). Starting interactive menu...\n", (char)ch);
            return false;
        }
        elapsed_ms += step_ms;
    }

    printf("\nTimeout expired without interactive input. Executing automated test suite.\n\n");
    return true;
#endif
}

static void run_all_tests_automated(void)
{
    print_banner("RUNNING ALL REGISTERED TEST SUITES SEQUENTIALLY (AUTOMATED MODE)");
    UNITY_BEGIN();
    unity_run_all_tests();
    int failures = UNITY_END();

    printf("\n======================================================================\n");
    printf(" AGGREGATE TEST SUITE EXECUTION SUMMARY                                \n");
    printf("   Total Failures: %d                                                 \n", failures);
    if (failures > 0) {
        printf("   STATUS: FAILED - Returning non-zero exit code: %d                  \n", failures);
        printf("======================================================================\n\n");
        exit(failures > 255 ? 255 : (failures > 0 ? failures : 1));
    } else {
        printf("   STATUS: PASSED - All registered test suites passed successfully     \n");
        printf("======================================================================\n\n");
        exit(0);
    }
}

void app_main()
{
    test_init();

    if (should_run_automated_mode()) {
        run_all_tests_automated();
        return;
    }

    print_banner("Starting interactive test menu");
    /* This function will not return, and will be busy waiting for UART input.
     * Make sure that task watchdog is disabled if you use this function.
     */
    unity_run_menu();
}

static void print_banner(const char* text)
{
    printf("\n#### %s #####\n\n", text);
}





