/* A fake libretro Core for tests, built by them with the system C compiler (see
 * `libretro::tests::fake_core`). It never loads a Game: retro_load_game first sends FAKE_MESSAGE
 * with RETRO_ENVIRONMENT_SET_MESSAGE, as a real Core does to say why (a missing BIOS), then fails. */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifndef FAKE_MESSAGE
#define FAKE_MESSAGE "The fake Core needs a BIOS."
#endif

enum { RETRO_ENVIRONMENT_SET_MESSAGE = 6 };

struct retro_message {
    const char *msg;
    unsigned frames;
};

struct retro_system_info {
    const char *library_name;
    const char *library_version;
    const char *valid_extensions;
    bool need_fullpath;
    bool block_extract;
};

typedef bool (*retro_environment_t)(unsigned cmd, void *data);

static retro_environment_t environ_cb;

void retro_set_environment(retro_environment_t cb) { environ_cb = cb; }
void retro_set_video_refresh(void *cb) { (void)cb; }
void retro_set_audio_sample(void *cb) { (void)cb; }
void retro_set_audio_sample_batch(void *cb) { (void)cb; }
void retro_set_input_poll(void *cb) { (void)cb; }
void retro_set_input_state(void *cb) { (void)cb; }
void retro_init(void) {}
void retro_deinit(void) {}
unsigned retro_api_version(void) { return 1; }

void retro_get_system_info(struct retro_system_info *info)
{
    info->library_name = "fake";
    info->library_version = "1";
    info->valid_extensions = "";
    info->need_fullpath = true;
    info->block_extract = false;
}

void retro_get_system_av_info(void *info) { (void)info; }
void retro_reset(void) {}
void retro_run(void) {}
size_t retro_serialize_size(void) { return 0; }
bool retro_serialize(void *data, size_t size) { (void)data; (void)size; return false; }
bool retro_unserialize(const void *data, size_t size) { (void)data; (void)size; return false; }

bool retro_load_game(const void *game)
{
    struct retro_message message = { FAKE_MESSAGE, 180 };
    (void)game;
    environ_cb(RETRO_ENVIRONMENT_SET_MESSAGE, &message);
    return false;
}

void retro_unload_game(void) {}
void *retro_get_memory_data(unsigned id) { (void)id; return NULL; }
size_t retro_get_memory_size(unsigned id) { (void)id; return 0; }
