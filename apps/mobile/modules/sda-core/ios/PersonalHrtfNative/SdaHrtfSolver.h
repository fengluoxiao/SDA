#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// In-process, CPU-only Mesh2HRTF. No shell, subprocess or process-wide chdir.
typedef struct SdaHrtfSolver SdaHrtfSolver;
typedef void (*SdaHrtfProgress)(void *context, int frequency, int total);
typedef struct {
 int code; // 0 success, 1 input/solver error, 2 cancelled, 3 memory limit
 uint64_t peak_array_bytes;
 uint64_t remaining_array_bytes;
 char message[512];
} SdaHrtfSolverResult;
// root must be an existing, private job directory. Memory budget: 1-512 MiB.
SdaHrtfSolver *sda_hrtf_solver_create(const char *root, uint64_t memory_budget);
// The owner keeps this handle alive until run returns. Cancellation is thread-safe.
void sda_hrtf_solver_cancel(SdaHrtfSolver *job);
// work is root-relative (e.g. NumCalc/source_1). Runs synchronously on caller's
// worker queue. Calls are serialized within each job; distinct jobs are isolated.
SdaHrtfSolverResult sda_hrtf_solver_run(SdaHrtfSolver *job, const char *work,
 int first_frequency, int last_frequency, int estimate_only,
 SdaHrtfProgress progress, void *context);
void sda_hrtf_solver_destroy(SdaHrtfSolver *job);
// Equivalent to upstream reference_hrtfs + compute_hrirs, inside the app.
// Positive bins f=(k+1)*48000/(2*bins). Input: [ear][bin][real,imag].
// Output: planar [left 2*bins samples][right 2*bins samples]. Preserves
// per-ear gains and phase; never performs per-ear peak normalization.
SdaHrtfSolverResult sda_hrtf_make_hrir(SdaHrtfSolver *job,
 const double *pressure, size_t pressure_values, size_t bins,
 const double *source_area, double reference_radius, int shift,
 float *output, size_t output_samples);

#ifdef __cplusplus
}
#endif
