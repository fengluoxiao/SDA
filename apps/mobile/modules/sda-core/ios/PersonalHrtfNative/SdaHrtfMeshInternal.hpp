#pragma once
#include "SdaHrtfMesh.h"
// Internal only: the review preparer constructs the posterior boundary. No
// imported/user mask can waive the independent depth screen for measured faces.
SdaHrtfMeshResult sda_screen_prepared_surface(SdaHrtfMeshView model,size_t measuredFaces,
 SdaHrtfMeshView review,const double ears[6],double &reviewDeviation,SdaHrtfMeshCancelled,void *);
// Proposed triangles use only review vertex IDs. The original scan points and
// normals, not review vertices or posterior geometry, must witness their interiors.
SdaHrtfMeshResult sda_check_gap_support(SdaHrtfMeshView review,const uint32_t *indices,
 const uint8_t *nearEar,size_t count,size_t &failed,SdaHrtfMeshCancelled,void *);
