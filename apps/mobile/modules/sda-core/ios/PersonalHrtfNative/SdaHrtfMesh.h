#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// One shared downstream bound; live capture may use a smaller memory budget.
enum { SDA_HRTF_MAXIMUM_SUPPORT_POINTS = 1500000 };
// Conservative working-space allowance (512 bytes/point), not a measured peak.
// Budget must already reserve room for the app, audio and OS. Zero means no scan.
size_t sda_hrtf_scan_point_limit(uint64_t working_budget);
// Coordinates are measured metres in one rigid reconstruction frame. Normals
// are measured outward normals; no inferred pinna/face geometry is accepted.
typedef struct {
 const float *vertices; size_t vertex_count; // tightly packed xyz
 const uint32_t *triangles; size_t triangle_count; // tightly packed indices
 const float *support; const float *support_normals; size_t support_count;
} SdaHrtfMeshView;
typedef int (*SdaHrtfMeshCancelled)(void *context);
typedef struct {
 int code; // 0 geometric screen passed, 1 rejected, 2 cancelled, 3 input/work bound
 double surface_area, enclosed_volume, maximum_edge, support_distance;
 uint64_t support_samples, intersection_pairs;
 char message[256];
} SdaHrtfMeshResult;
// A passing screen is NECESSARY, never sufficient for acoustic qualification.
// Does not certify ear landmarks, scan precision, spectral convergence, or a
// real person's HRTF. It never repairs holes or installs/activates a profile.
SdaHrtfMeshResult sda_hrtf_screen_head_mesh(SdaHrtfMeshView input,
 SdaHrtfMeshCancelled cancelled, void *context);
// Measurement/review only: retain triangles with measured depth and aligned
// normals over their entire sampled area. Unmeasured closures are discarded.
// face_mask has one byte per input triangle; output is valid only on success.
SdaHrtfMeshResult sda_hrtf_select_measured_faces(SdaHrtfMeshView input,
 uint8_t *face_mask, size_t mask_count, SdaHrtfMeshCancelled cancelled, void *context);
// Allows open boundaries and separate measured patches. Retains finite/index,
// size, winding, non-manifold-edge, intersection and 1.5 mm support checks.
// NEVER use this measurement screen for the closed acoustic solver input.
SdaHrtfMeshResult sda_hrtf_screen_measured_surface(SdaHrtfMeshView input,
 SdaHrtfMeshCancelled cancelled, void *context);
#ifdef __cplusplus
}
#endif
