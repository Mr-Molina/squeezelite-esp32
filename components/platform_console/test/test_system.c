/* test_mean.c: Implementation of a testable component.

   This example code is in the Public Domain (or CC0 licensed, at your option.)

   Unless required by applicable law or agreed to in writing, this
   software is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
   CONDITIONS OF ANY KIND, either express or implied.
*/

#include <limits.h>
#include "unity.h"
#include "platform_console.h"
#include "platform_esp32.h"
#include "platform_config.h"
#include "string.h"
struct arg_lit *arglit;
struct arg_int *argint;
struct arg_str *argstr;
struct arg_end *end;

extern int is_output_gpio(struct arg_int * gpio, FILE * f, int * gpio_out, bool mandatory);
extern void initialize_console();
extern esp_err_t run_command(char * line);
static char *buf = NULL;
static char * s_tmp_line_buf=NULL;
static size_t buf_size = 0;
static FILE * f = NULL;
static size_t argc=1;
static char ** argv=NULL;
static bool config_initialized=false;
static void **s_current_argtable = NULL;
static int s_current_argcount = 0;
static char *s_orig_dac_config = NULL;
static bool s_orig_dac_config_saved = false;
static char *s_nvs_value = NULL;
void init_console(){
    if(config_initialized) return;
    initialize_console();
    config_initialized=true;
}

/****************************************************************************************
 * 
 */
void open_mem_stream_file(){
    if (f) {
        fclose(f);
        f = NULL;
    }
    if (buf) {
        free(buf);
        buf = NULL;
    }
    buf_size = 0;
	f = open_memstream(&buf, &buf_size);
}

/****************************************************************************************
 * 
 */
void tearDown(void){
    if (s_current_argtable && s_current_argcount > 0) {
        arg_freetable(s_current_argtable, s_current_argcount);
        s_current_argtable = NULL;
        s_current_argcount = 0;
    }
    if (f) {
        fclose(f);
        f = NULL;
    }
    if (buf) {
        free(buf);
        buf = NULL;
    }
    buf_size = 0;
    if (argv) {
        free(argv);
        argv = NULL;
    }
    if (s_tmp_line_buf) {
        free(s_tmp_line_buf);
        s_tmp_line_buf = NULL;
    }
    if (s_orig_dac_config_saved) {
        if (s_orig_dac_config) {
            config_set_value(NVS_TYPE_STR, "dac_config", s_orig_dac_config);
            free(s_orig_dac_config);
            s_orig_dac_config = NULL;
        } else {
            config_delete_key("dac_config");
        }
        s_orig_dac_config_saved = false;
    }
    if (s_nvs_value) {
        free(s_nvs_value);
        s_nvs_value = NULL;
    }
}

/****************************************************************************************
 * 
 */
void close_flush_all(void * argtable, int count,bool print){
    if (f) {
        fflush (f);
        if(print && buf){
            printf("%s", buf);
        }
    }
    if (argtable && count > 0) {
        s_current_argtable = (void **)argtable;
        s_current_argcount = count;
    }
    tearDown();
}

/****************************************************************************************
 * 
 */
int alloc_split_command_line(char * cmdline){
    if (argv) {
        free(argv);
        argv = NULL;
    }
    argv = (char **) calloc(22, sizeof(char *));
    if (s_tmp_line_buf) {
        free(s_tmp_line_buf);
        s_tmp_line_buf = NULL;
    }
    size_t cmd_len = strlen(cmdline);
    s_tmp_line_buf = (char *) calloc(cmd_len + 1, sizeof(char));
    if (s_tmp_line_buf && argv) {
        strlcpy(s_tmp_line_buf, cmdline, cmd_len + 1);
        argc = esp_console_split_argv(s_tmp_line_buf, argv, 22);
    }

    return 0;
}

/****************************************************************************************
 * 
 */
int alloc_split_parse_command_line(char * cmdline, void ** args){
    alloc_split_command_line(cmdline);
    return arg_parse(argc, argv,args);
}

/****************************************************************************************
 * 
 */
TEST_CASE("Invalid GPIO detected", "[config][ui]")
{
    char * cmdline =  "test -i 55\n";
    void *argtable[] = {
        argint = arg_int1("i","int","<gpio>","GPIO number"),
        end  = arg_end(6)
    };
    s_current_argtable = argtable;
    s_current_argcount = sizeof(argtable)/sizeof(argtable[0]);
    open_mem_stream_file();
    alloc_split_parse_command_line(cmdline, &argtable);
    int out_val = 0;
    TEST_ASSERT_EQUAL_INT_MESSAGE(1,is_output_gpio(argtable[0], f, &out_val, true),"Invalid GPIO not detected");
    TEST_ASSERT_EQUAL_INT_MESSAGE(-1,out_val,"GPIO Should be set to -1");
    fflush (f);
    TEST_ASSERT_EQUAL_STRING_MESSAGE("Invalid int gpio: [55] is not a GPIO\n",buf,"Invalid GPIO message wrong");
    close_flush_all(argtable,sizeof(argtable)/sizeof(argtable[0]),false);
}

/****************************************************************************************
 * 
 */
TEST_CASE("Input Only GPIO detected", "[config][ui]")
{
    char * cmdline =  "test -i 35\n";
    void *argtable[] = {
        argint = arg_int1("i","int","<gpio>","GPIO number"),
        end  = arg_end(6)
    };
    s_current_argtable = argtable;
    s_current_argcount = sizeof(argtable)/sizeof(argtable[0]);
    open_mem_stream_file();
    alloc_split_parse_command_line(cmdline, &argtable);
    int out_val = 0;
    TEST_ASSERT_EQUAL_INT_MESSAGE(1,is_output_gpio(argtable[0], f, &out_val, true),"Input only GPIO not detected");
    TEST_ASSERT_EQUAL_INT_MESSAGE(-1,out_val,"GPIO Should be set to -1");
    fflush (f);
    TEST_ASSERT_EQUAL_STRING_MESSAGE("Invalid int gpio: [35] has input capabilities only\n",buf,"Missing GPIO message wrong");
    close_flush_all(argtable,sizeof(argtable)/sizeof(argtable[0]),false);
}

/****************************************************************************************
 * 
 */
TEST_CASE("Valid GPIO Processed", "[config][ui]")
{
    char * cmdline =  "test -i 33\n";
    void *argtable[] = {
        argint = arg_int1("i","int","<gpio>","GPIO number"),
        end  = arg_end(6)
    };
    s_current_argtable = argtable;
    s_current_argcount = sizeof(argtable)/sizeof(argtable[0]);
    open_mem_stream_file();
    alloc_split_parse_command_line(cmdline, &argtable);
    int out_val = 0;
    TEST_ASSERT_EQUAL_INT_MESSAGE(0,is_output_gpio(argtable[0], f, &out_val, true),"Valid GPIO not recognized");
    TEST_ASSERT_EQUAL_INT_MESSAGE(33,out_val,"GPIO Should be set to 33");
    fflush (f);
    TEST_ASSERT_EQUAL_STRING_MESSAGE("",buf,"Valid GPIO shouldn't produce a message");
    close_flush_all(argtable,sizeof(argtable)/sizeof(argtable[0]),false);
}

/****************************************************************************************
 * 
 */
TEST_CASE("Missing mandatory GPIO detected", "[config][ui]")
{
    char * cmdline =  "test \n";
    void *argtable[] = {
        argint = arg_int1("i","int","<gpio>","GPIO number"),
        end  = arg_end(6)
    };
    s_current_argtable = argtable;
    s_current_argcount = sizeof(argtable)/sizeof(argtable[0]);
    open_mem_stream_file();
    alloc_split_parse_command_line(cmdline, &argtable);
    int out_val = 0;
    TEST_ASSERT_EQUAL_INT_MESSAGE(1,is_output_gpio(argtable[0], f, &out_val, true),"Missing GPIO not detected");
    fflush (f);
    TEST_ASSERT_EQUAL_STRING_MESSAGE("Missing: int\n",buf,"Missing GPIO parameter message wrong");
    TEST_ASSERT_EQUAL_INT_MESSAGE(-1,out_val,"GPIO Should be set to -1");
    close_flush_all(argtable,sizeof(argtable)/sizeof(argtable[0]),false);
}
/****************************************************************************************
 * 
 */
TEST_CASE("dac config command", "[config_cmd]")
{
    s_orig_dac_config = config_alloc_get_str("dac_config", NULL, NULL);
    s_orig_dac_config_saved = true;
    config_set_value(NVS_TYPE_STR, "dac_config", "");
    esp_err_t err = run_command("cfg-hw-dac\n");
    s_nvs_value = config_alloc_get_str("dac_config", NULL, NULL);
    TEST_ASSERT_NOT_NULL(s_nvs_value);
    TEST_ASSERT_EQUAL_MESSAGE(ESP_OK, err, "Running command failed");
    free(s_nvs_value);
    s_nvs_value = NULL;
    if (s_orig_dac_config) {
        config_set_value(NVS_TYPE_STR, "dac_config", s_orig_dac_config);
        free(s_orig_dac_config);
        s_orig_dac_config = NULL;
    } else {
        config_delete_key("dac_config");
    }
    s_orig_dac_config_saved = false;
}
