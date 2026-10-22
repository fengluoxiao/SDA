#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
void sda_metal_set_allowed(int allowed);
uint64_t sda_metal_completed_batches(void);
void *sda_metal_create(size_t rows,size_t bins,size_t parts,char *error,size_t capacity);
int sda_metal_predict(void *context,const float *input,size_t input_length,float *output,size_t output_length,uint64_t timeout_ns,char *error,size_t capacity);
void sda_metal_free(void *context);
#ifdef __cplusplus
}
#endif
