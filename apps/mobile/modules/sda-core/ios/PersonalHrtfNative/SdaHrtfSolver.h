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
// iOS process headroom reported by the OS, or 0 on non-iOS test hosts.
uint64_t sda_hrtf_available_memory(void);
// Returns 0 if headroom is unknown/too low. Reserves app/audio/OS room, caps
// scan/reconstruction work at 768 MiB and at one eighth of physical memory.
uint64_t sda_hrtf_recommended_budget(uint64_t headroom, uint64_t physical_memory);
// Serial acoustic solve only. Same reserves, 4 GiB total hard stop, no
// up-front allocation; individual tracked arrays are still at most 1 GiB.
uint64_t sda_hrtf_solve_budget(uint64_t headroom,uint64_t physical_memory);
SdaHrtfSolver *sda_hrtf_create_for_resources(const char *root,uint64_t headroom,uint64_t physical_memory);
// root must be an existing, private job directory. Memory budget: 1-1024 MiB.
SdaHrtfSolver *sda_hrtf_solver_create(const char *root, uint64_t memory_budget);
// The owner keeps this handle alive until run returns. Cancellation is thread-safe.
void sda_hrtf_solver_cancel(SdaHrtfSolver *job);
// work is root-relative (e.g. NumCalc/source_1). Runs synchronously on caller's
// worker queue. Calls are serialized within each job; distinct jobs are isolated.
SdaHrtfSolverResult sda_hrtf_solver_run(SdaHrtfSolver *job, const char *work,
 int first_frequency, int last_frequency, int estimate_only,
 SdaHrtfProgress progress, void *context);
// Read a frequency's actual cluster tree and mandatory coefficient storage;
// no pressure, estimates or logs are written. A pass is necessary, not proof
// that other arrays, other frequencies or future headroom will fit.
SdaHrtfSolverResult sda_hrtf_check_frequency_capacity(SdaHrtfSolver *job,const char *work,int frequency_index);
void sda_hrtf_solver_destroy(SdaHrtfSolver *job);
// Read both ears' completed evaluation pressure files into caller-owned memory.
// Validates the NC input's uniform positive FFT frequencies through 24 kHz and
// every evaluation ID exactly once. Output: [direction][ear][bin][real,imag].
// Caller keeps output alive; <=64 MiB and within job budget. Zeroed on failure.
// Does not certify the input head mesh, ear entrances, or spatial coordinates.
SdaHrtfSolverResult sda_hrtf_read_pressure(SdaHrtfSolver *job,
 const int32_t *node_ids, size_t directions, size_t bins,
 double *output, size_t output_values);
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
