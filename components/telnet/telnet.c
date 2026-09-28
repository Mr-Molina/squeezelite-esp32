	/**
 * Test the telnet functions.
 *
 * Perform a test using the telnet functions.
 * This code exports two new global functions:
 *
 * void telnet_listenForClients(void (*callback)(uint8_t *buffer, size_t size))
 * void telnet_sendData(uint8_t *buffer, size_t size)
 *
 * For additional details and documentation see:
 * * Free book on ESP32 - https://leanpub.com/kolban-ESP32
 *
 *
 * Neil Kolban <kolban1@kolban.com>
 *
 * ****************************
 * Additional portions were taken from
 * https://github.com/PocketSprite/8bkc-sdk/blob/master/8bkc-components/8bkc-hal/vfs-stdout.c
 *
 */
#include <stdlib.h> // Required for libtelnet.h
#include <esp_log.h>
#include "libtelnet.h"
#include "stdbool.h"
#include <lwip/def.h>
#include <lwip/sockets.h>
#include <errno.h>
#include <string.h>
#include "sdkconfig.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/ringbuf.h"
#include "esp_app_trace.h"
#include "telnet.h"
#include "esp_vfs.h"
#include "esp_vfs_dev.h"
#include "esp_attr.h"
#include "soc/uart_struct.h"
#include "driver/uart.h"
#include "config.h"
#include "platform_config.h"
#include "nvs_utilities.h"
#include "platform_esp32.h"
#include "messaging.h"
#include "tools.h"

/************************************
 * Globals
 */

#define TELNET_STACK_SIZE 4096
#define TELNET_RX_BUF 1024
#define TELNET_AUTH_TIMEOUT_MS 60000
#define TELNET_AUTH_MAX_ATTEMPTS 3

extern bool bypass_network_manager;

typedef struct telnetUserData {
	int sockfd;
	telnet_t *tnHandle;
	char * rxbuf;
	bool is_authenticated;
	int auth_attempts;
	uint32_t auth_timer;
	char auth_buf[64];
	size_t auth_buf_len;
	bool last_was_cr;
} telnet_userdata_t;

const static char TAG[] = "telnet";
static int uart_fd;
static RingbufHandle_t buf_handle;
static size_t send_chunk = 512;
static size_t log_buf_size = 4*1024;
static bool bIsEnabled=false;
static int partnerSocket;
static telnet_t *tnHandle;
static bool bMirrorToUART;
static bool bIsAuthenticated;
static SemaphoreHandle_t telnet_mutex = NULL;

/************************************
 * Forward declarations
 */
static void 	telnet_task(void *data);
static int 		stdout_open(const char * path, int flags, int mode);
static int 		stdout_fstat(int fd, struct stat * st);
static ssize_t 	stdout_write(int fd, const void * data, size_t size);
static void 	handle_telnet_conn();
static size_t 	process_logs( UBaseType_t bytes, bool make_room);
static void 	telnet_event_handler(telnet_t *thisTelnet, telnet_event_t *event, void *userData);

void init_telnet(){
	char *val= get_nvs_value_alloc(NVS_TYPE_STR, "telnet_enable");

	if (!val || strlen(val) == 0 || !strcasestr("YXD",val) ) {
		ESP_LOGI(TAG,"Telnet support disabled");
		if(val) free(val);
		return;
	}

	// if wifi manager is bypassed, there will possibly be no wifi available
	bMirrorToUART = (strcasestr("D",val)!=NULL);
	if (!bMirrorToUART && bypass_network_manager){
		// This isn't supposed to happen, as telnet won't start if wifi manager isn't
		// started. So this is a safeguard only.
		ESP_LOGW(TAG,"Wifi manager is not active.  Forcing console on Serial output.");
	}

	FREE_AND_NULL(val);
	val = get_nvs_value_alloc(NVS_TYPE_STR, "telnet_block");
	if (val){
		int size = atol(val);
		if (size > 0) send_chunk = size;
		free(val);
	}
	val = get_nvs_value_alloc(NVS_TYPE_STR, "telnet_buffer");
	if (val){
		int size = atol(val);
		if (size > 0) log_buf_size = size;
		free(val);
	}
	// Redirect the output to our telnet handler as soon as possible
	StaticRingbuffer_t *buffer_struct = (StaticRingbuffer_t *) heap_caps_malloc(sizeof(StaticRingbuffer_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
	// All non-split ring buffer must have their memory alignment set to 32 bits.
	uint8_t *buffer_storage = (uint8_t *)heap_caps_malloc(sizeof(uint8_t)*log_buf_size, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT );
	buf_handle = xRingbufferCreateStatic(log_buf_size, RINGBUF_TYPE_BYTEBUF, buffer_storage, buffer_struct);
	if (buf_handle == NULL) {
		ESP_LOGE(TAG,"Failed to create ring buffer for telnet!");
		messaging_post_message(MESSAGING_ERROR,MESSAGING_CLASS_SYSTEM,"Failed to allocate memory for telnet buffer");

		return;
	}

	ESP_LOGI(TAG, "***Redirecting log output to telnet");
	esp_vfs_t vfs = { };
	vfs.flags = ESP_VFS_FLAG_DEFAULT;
	vfs.write = &stdout_write;
	vfs.open = &stdout_open;
	vfs.fstat = &stdout_fstat;

	if (bMirrorToUART) uart_fd = open("/dev/uart/0", O_RDWR);

	ESP_ERROR_CHECK(esp_vfs_register("/dev/pkspstdout", &vfs, NULL));
	freopen("/dev/pkspstdout", "w", stdout);
	freopen("/dev/pkspstdout", "w", stderr);

	if (!telnet_mutex) {
		telnet_mutex = xSemaphoreCreateRecursiveMutex();
	}

	bIsEnabled=true;
}

void start_telnet(void * pvParameter){
	static bool isStarted=false;

	if (isStarted || !bIsEnabled) return;

	isStarted=true;	

	StaticTask_t *xTaskBuffer = (StaticTask_t*) heap_caps_malloc(sizeof(StaticTask_t), MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT);
	StackType_t *xStack = heap_caps_malloc(TELNET_STACK_SIZE, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
	
	xTaskCreateStatic( (TaskFunction_t) &telnet_task, "telnet", TELNET_STACK_SIZE, NULL, ESP_TASK_PRIO_MIN, xStack, xTaskBuffer);

}

static void telnet_task(void *data) {
	int serverSocket;
	struct sockaddr_in serverAddr;
	serverAddr.sin_family = AF_INET;
	serverAddr.sin_addr.s_addr = htonl(INADDR_ANY);
	serverAddr.sin_port = htons(23);

	while (1) {
		serverSocket = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
		if (bind(serverSocket, (struct sockaddr *)&serverAddr, sizeof(serverAddr)) >= 0 &&	listen(serverSocket, 1) >= 0) break;
		close(serverSocket);		
		ESP_LOGI(TAG, "can't bind Telnet socket");
		vTaskDelay(pdMS_TO_TICKS(1000));
	}

	while (1) {
		socklen_t len = sizeof(serverAddr);
		int sock = accept(serverSocket, (struct sockaddr *)&serverAddr, &len);

		if (sock >= 0) {
			partnerSocket = sock;
			ESP_LOGI(TAG, "We have a new client connection %d", sock);
			handle_telnet_conn();
			ESP_LOGI(TAG, "Telnet connection terminated %d", sock);
		} else {
			ESP_LOGW(TAG, "accept: %d (%s)", errno, strerror(errno));
		}
	}

	// we should not be here
	close(serverSocket);
	vTaskDelete(NULL);
}

/**
 * Telnet handler.
 */
static void telnet_event_handler(telnet_t *thisTelnet, telnet_event_t *event, void *userData) {
	telnet_userdata_t *telnetUserData = (telnet_userdata_t *)userData;

	switch(event->type) {
	case TELNET_EV_SEND:
		if (telnetUserData->sockfd > 0) {
			send(telnetUserData->sockfd, event->data.buffer, event->data.size, 0);
		}
		break;
	case TELNET_EV_DATA:
		if (telnetUserData->is_authenticated) {
			console_push(event->data.buffer, event->data.size);
			break;
		}

		for (size_t i = 0; i < event->data.size; i++) {
			char c = event->data.buffer[i];

			if (c == '\r' || c == '\n') {
				if (c == '\n' && telnetUserData->last_was_cr) {
					telnetUserData->last_was_cr = false;
					continue;
				}
				telnetUserData->last_was_cr = (c == '\r');

				if (c == '\r' && i + 1 < event->data.size && (event->data.buffer[i + 1] == '\n' || event->data.buffer[i + 1] == '\0')) {
					i++;
					telnetUserData->last_was_cr = false;
				}

				telnetUserData->auth_buf[telnetUserData->auth_buf_len] = '\0';
				char *configured_pwd = (char *)config_alloc_get_str("telnet_pwd", NULL, NULL);
				bool auth_ok = false;

				if (configured_pwd && *configured_pwd) {
					if (strcmp(telnetUserData->auth_buf, configured_pwd) == 0) {
						auth_ok = true;
					}
				} else {
					// If no password is configured in NVS, default to requiring setting a password
					if (telnetUserData->auth_buf_len > 0) {
						config_set_value(NVS_TYPE_STR, "telnet_pwd", telnetUserData->auth_buf);
						ESP_LOGI(TAG, "Telnet password configured and saved to NVS");
						auth_ok = true;
					}
				}
				if (configured_pwd) free(configured_pwd);

				if (auth_ok) {
					if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
					telnetUserData->is_authenticated = true;
					bIsAuthenticated = true;
					if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);
					telnetUserData->auth_attempts = 0;
					telnetUserData->auth_buf_len = 0;
					const char *welcome = "\r\nWelcome to Squeezelite-ESP32 Console\r\n\r\n";
					telnet_send_text(thisTelnet, welcome, strlen(welcome));

					if (i + 1 < event->data.size) {
						console_push(&event->data.buffer[i + 1], event->data.size - (i + 1));
					}
					return;
				} else {
					telnetUserData->auth_attempts++;
					telnetUserData->auth_buf_len = 0;
					ESP_LOGW(TAG, "Telnet authentication failed (attempt %d/%d)", telnetUserData->auth_attempts, TELNET_AUTH_MAX_ATTEMPTS);

					if (telnetUserData->auth_attempts >= TELNET_AUTH_MAX_ATTEMPTS) {
						ESP_LOGW(TAG, "Maximum authentication attempts exceeded, disconnecting client");
						const char *fail_msg = "\r\nAuthentication failed. Connection closed.\r\n";
						telnet_send_text(thisTelnet, fail_msg, strlen(fail_msg));
						if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
						partnerSocket = -1;
						telnetUserData->sockfd = -1;
						bIsAuthenticated = false;
						if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);
						return;
					} else {
						const char *retry_msg = "\r\nPassword: ";
						telnet_send_text(thisTelnet, retry_msg, strlen(retry_msg));
					}
				}
			} else if (c == '\b' || (unsigned char)c == 0x7F) {
				telnetUserData->last_was_cr = false;
				if (telnetUserData->auth_buf_len > 0) {
					telnetUserData->auth_buf_len--;
				}
			} else if (c >= 32 && c <= 126) {
				telnetUserData->last_was_cr = false;
				if (telnetUserData->auth_buf_len < sizeof(telnetUserData->auth_buf) - 1) {
					telnetUserData->auth_buf[telnetUserData->auth_buf_len++] = c;
				}
			}
		}
		break;
	case TELNET_EV_TTYPE:
		telnet_ttype_send(telnetUserData->tnHandle);
		break;
	default:
		break;
	}
}

#define handle_telnet_events telnet_event_handler

static size_t process_logs(UBaseType_t bytes, bool make_room){
	UBaseType_t pending;

	vRingbufferGetInfo(buf_handle, NULL, NULL, NULL, NULL, &pending);

	// nothing to do or we can do 
	if (partnerSocket <= 0 || !bIsAuthenticated || !tnHandle || (make_room && log_buf_size - pending > bytes)) return pending;

	// can't send more than what we have
	if (bytes > pending) bytes = pending;

	if (telnet_mutex && xSemaphoreTakeRecursive(telnet_mutex, pdMS_TO_TICKS(100)) == pdTRUE) {
		while (bytes > 0) {
			if (partnerSocket <= 0 || !bIsAuthenticated || !tnHandle) break;

			size_t size;
			char *item = (char *)xRingbufferReceiveUpTo(buf_handle, &size, pdMS_TO_TICKS(50), bytes);
			
			if (!item) break;
			if (partnerSocket <= 0 || !bIsAuthenticated || !tnHandle) {
				vRingbufferReturnItem(buf_handle, (void *)item);
				break;
			}

			bytes -= size;
			telnet_send_text(tnHandle, item, size);

			vRingbufferReturnItem(buf_handle, (void *)item);
		}
		xSemaphoreGiveRecursive(telnet_mutex);
	}

	return pending - bytes;
}

static void handle_telnet_conn() {
	static const telnet_telopt_t my_telopts[] = {
		{ TELNET_TELOPT_ECHO,      TELNET_WONT, TELNET_DO },
		{ TELNET_TELOPT_TTYPE,     TELNET_WILL, TELNET_DONT },
		{ TELNET_TELOPT_COMPRESS2, TELNET_WONT, TELNET_DO   },
		{ TELNET_TELOPT_ZMP,       TELNET_WONT, TELNET_DO   },
		{ TELNET_TELOPT_MSSP,      TELNET_WONT, TELNET_DO   },
		{ TELNET_TELOPT_BINARY,    TELNET_WILL, TELNET_DO   },
		{ TELNET_TELOPT_NAWS,      TELNET_WILL, TELNET_DONT },
		{TELNET_TELOPT_LINEMODE,   TELNET_WONT, TELNET_DO },
		{ -1, 0, 0 }
	};
	telnet_userdata_t *pTelnetUserData = (telnet_userdata_t *)heap_caps_malloc(sizeof(telnet_userdata_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
	if (!pTelnetUserData) {
		ESP_LOGE(TAG, "Failed to allocate telnet user data");
		if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
		int sock = partnerSocket;
		partnerSocket = 0;
		if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);
		if (sock > 0) close(sock);
		return;
	}
	memset(pTelnetUserData, 0, sizeof(telnet_userdata_t));

	telnet_t *new_handle = telnet_init(my_telopts, telnet_event_handler, 0, pTelnetUserData);

	pTelnetUserData->rxbuf = (char *) heap_caps_malloc(TELNET_RX_BUF, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
	if (!pTelnetUserData->rxbuf) {
		ESP_LOGE(TAG, "Failed to allocate telnet rx buffer");
		telnet_free(new_handle);
		free(pTelnetUserData);
		if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
		int sock = partnerSocket;
		partnerSocket = 0;
		if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);
		if (sock > 0) close(sock);
		return;
	}

	pTelnetUserData->tnHandle = new_handle;
	pTelnetUserData->sockfd = partnerSocket;
	pTelnetUserData->is_authenticated = false;
	pTelnetUserData->auth_attempts = 0;
	pTelnetUserData->auth_timer = (uint32_t)xTaskGetTickCount();
	pTelnetUserData->auth_buf_len = 0;
	pTelnetUserData->last_was_cr = false;

	if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
	tnHandle = new_handle;
	bIsAuthenticated = false;
	if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);

	// Prompt client for password
	char *pwd = (char *)config_alloc_get_str("telnet_pwd", NULL, NULL);
	if (pwd && *pwd) {
		const char *prompt = "Password: ";
		telnet_send_text(tnHandle, prompt, strlen(prompt));
	} else {
		const char *warn_prompt = "\r\n[SECURITY WARNING] No telnet password configured in NVS. Please set a password.\r\nPassword: ";
		telnet_send_text(tnHandle, warn_prompt, strlen(warn_prompt));
	}
	FREE_AND_NULL(pwd);

	bool pending = true;

	while(1) {
		if (partnerSocket < 0) break;

		fd_set rfds, wfds;
		struct timeval timeout = {0, 200*1000};

		FD_ZERO(&rfds);
		FD_SET(partnerSocket, &rfds);

		FD_ZERO(&wfds);
		if (pending && pTelnetUserData->is_authenticated) FD_SET(partnerSocket, &wfds);

		int res = select(partnerSocket + 1, &rfds, &wfds, NULL, &timeout);
		if (res < 0) break;

		if (!pTelnetUserData->is_authenticated) {
			if ((xTaskGetTickCount() - pTelnetUserData->auth_timer) > pdMS_TO_TICKS(TELNET_AUTH_TIMEOUT_MS)) {
				ESP_LOGW(TAG, "Telnet authentication timed out, disconnecting");
				const char *timeout_msg = "\r\nAuthentication timed out. Disconnecting.\r\n";
				telnet_send_text(tnHandle, timeout_msg, strlen(timeout_msg));
				break;
			}
		}

		if (FD_ISSET(partnerSocket, &rfds)) { 
			int len = recv(partnerSocket, pTelnetUserData->rxbuf, TELNET_RX_BUF, 0);
			if (len <= 0) break;
			if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
			if (tnHandle) {
				telnet_recv(tnHandle, pTelnetUserData->rxbuf, (size_t)len);
			}
			if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);
		}

		if (partnerSocket < 0) break;

		if (pTelnetUserData->is_authenticated && FD_ISSET(partnerSocket, &wfds)) {	
			pending = process_logs(send_chunk, false) > 0;
		} else {
			pending = true;
		}
  	} 
	
	if (telnet_mutex) xSemaphoreTakeRecursive(telnet_mutex, portMAX_DELAY);
	telnet_t *to_free = tnHandle;
	tnHandle = NULL;
	int to_close = partnerSocket;
	partnerSocket = 0;
	bIsAuthenticated = false;
	pTelnetUserData->sockfd = -1;
	if (telnet_mutex) xSemaphoreGiveRecursive(telnet_mutex);

	if (to_free) {
		telnet_free(to_free);
	}

	free(pTelnetUserData->rxbuf);
	memset(pTelnetUserData->auth_buf, 0, sizeof(pTelnetUserData->auth_buf));
	free(pTelnetUserData);

	if (to_close > 0) {
		close(to_close);
	}
}

// ******************* stdout/stderr Redirection to ringbuffer
static ssize_t stdout_write(int fd, const void * data, size_t size) {
	// flush the buffer and send item
	if (buf_handle) {
		process_logs(size, true);
		xRingbufferSend(buf_handle, data, size, 0);
	}
	
	// mirror to uart if required
	return (bMirrorToUART || !buf_handle) ? write(uart_fd, data, size) : size;
}

static int stdout_open(const char * path, int flags, int mode) {
	return 0;
}

static int stdout_fstat(int fd, struct stat * st) {
	st->st_mode = S_IFCHR;
	return 0;
}
