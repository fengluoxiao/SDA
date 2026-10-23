#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// -1: public availability API unavailable on this OS; 0: none; 1: visible ANE.
int sda_coreml_neural_engine_available(void);
// cpu_only is used exclusively by the standalone regression executable.
void *sda_coreml_create(const uint8_t *model, size_t length, size_t rows,
                       size_t bins, size_t inputs, int cpu_only,
                       char *error, size_t error_capacity);
int sda_coreml_predict(void *context, const float *input, size_t input_length,
                       float *output, size_t output_length,
                       char *error, size_t error_capacity);
void sda_coreml_free(void *context);
#ifdef __cplusplus
}
#endif
