#ifndef SDA_BRIDGE_H
#define SDA_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
void *sda_ios_create(const char *config, const char *hrtf, char **error);
char *sda_ios_command(void *engine, const char *op, const char *args);
char *sda_ios_feed(void *engine, const uint8_t *bytes, size_t length);
void sda_ios_render(void *engine, float *left, float *right, size_t frames);
void sda_ios_close(void *engine);
void sda_ios_string_free(char *string);
#endif
