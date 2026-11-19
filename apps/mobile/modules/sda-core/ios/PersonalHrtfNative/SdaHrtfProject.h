#pragma once
#include "SdaHrtfMesh.h"
#ifdef __cplusplus
extern "C" {
#endif
// PREPARATION ONLY. The caller must confirm ear entrances on measured anatomy
// and establish acoustic mesh accuracy/convergence before using the response.
// No ear detection, rescaling, mesh decimation or geometry repair occurs here.
typedef struct {
 SdaHrtfMeshView mesh;
 const uint32_t *left_faces; size_t left_face_count;
 const uint32_t *right_faces; size_t right_face_count;
 double front[3], up[3]; // measured, unit and orthogonal, in reconstruction frame
 size_t bins; // power of two, 16..2048; uniform positive frequencies to 24 kHz
 int azimuth_steps, elevation_steps; // multiples of 8 / even; includes both poles
 double reference_radius; // metres, all evaluation nodes use this common radius
} SdaHrtfProjectInput;
typedef struct {
 int code; // 0 prepared, 1 invalid, 2 cancelled, 3 resource bound
 size_t directions;
 double source_areas[2]; // exact selected triangle areas; no equalization
 double origin[3]; // midpoint of area-weighted left/right source patch centres
 char message[256];
} SdaHrtfProjectResult;
// Writes root/AcousticProject atomically via an exclusively created staging
// directory. root is an existing private local job directory; existing project
// or staging entries (including symlinks) are rejected, never removed/replaced.
// On failure only this call's staging directory is removed, after writers stop.
// Does NOT solve, certify acoustic accuracy, install, or select a personal HRTF.
SdaHrtfProjectResult sda_hrtf_prepare_project(const char *root,
 SdaHrtfProjectInput input, SdaHrtfMeshCancelled cancelled, void *context);
#ifdef __cplusplus
}
#endif
