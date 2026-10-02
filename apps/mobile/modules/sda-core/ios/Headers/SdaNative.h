#ifndef SDA_BRIDGE_H
#define SDA_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
void *sda_ios_create(const char *config, const char *hrtf, char **error);
char *sda_ios_command(void *engine, const char *op, const char *args);
char *sda_ios_feed(void *engine, const uint8_t *bytes, size_t length);
void sda_ios_render(void *engine, float *left, float *right, size_t frames);
void sda_ios_close(void *engine);
void sda_ios_string_free(char *string);
// Control/PCM calls must be serialized by the host; no realtime callback uses this handle.
void *sda_ios_speakers_create(char **error);
char *sda_ios_speakers_feed(void *decoder, const uint8_t *bytes, size_t length, bool finish);
size_t sda_ios_speakers_read(void *decoder, float *pcm, size_t capacity_frames);
void sda_ios_speakers_close(void *decoder);
// Experimental bounded source frames + OAM. Serialized non-realtime calls only.
void *sda_ios_sources_create(char **error);
char *sda_ios_sources_feed(void *decoder, const uint8_t *bytes, size_t length, bool finish);
char *sda_ios_sources_next(void *decoder);
void sda_ios_sources_close(void *decoder);
#endif
