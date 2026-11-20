#pragma once
#include <cmath>
#include <cstddef>

// Tangential nearest-neighbour discretization can oscillate after the rigid
// pose is stable. Require three tiny rigid updates AND measured surface overlap.
class SdaTrackingConvergence {
    unsigned stable=0;
public:
    bool observe(float translation, float rotation, float planeRms,
                 std::size_t inliers, std::size_t sourceCount) {
        const bool good=std::isfinite(translation) && translation>=0 && translation<=0.00005f &&
            std::isfinite(rotation) && rotation>=0 && rotation<=0.0002f &&
            std::isfinite(planeRms) && planeRms>=0 && planeRms<=0.001f &&
            inliers>=32 && inliers<=sourceCount && inliers>=sourceCount-inliers;
        stable=good ? stable+1 : 0;
        return stable>=3;
    }
};
