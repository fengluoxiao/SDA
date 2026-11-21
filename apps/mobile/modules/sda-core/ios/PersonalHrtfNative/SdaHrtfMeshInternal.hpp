#pragma once
#include "SdaHrtfMesh.h"
// Internal only: the review preparer constructs the posterior boundary. No
// imported/user mask can waive the independent depth screen for measured faces.
SdaHrtfMeshResult sda_screen_prepared_surface(SdaHrtfMeshView model,size_t measuredFaces,
 SdaHrtfMeshView review,const double ears[6],double &reviewDeviation,SdaHrtfMeshCancelled,void *);
