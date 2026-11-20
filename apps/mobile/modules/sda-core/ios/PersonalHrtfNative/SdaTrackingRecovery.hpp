#pragma once
#include <algorithm>
#include <cmath>
#include <limits>

// Model-queue only. Rejected frames never become measured geometry. Ordinary
// tracking loss is recoverable for the lifetime of the scan, not a deadline.
class SdaTrackingRecovery {
    double lostSince = -1;
    bool ended = false;
public:
    bool terminal(bool merged, double now) {
        if (ended) return true;
        if (!std::isfinite(now) || now < 0) return ended = true;
        if (merged) { lostSince = -1; return false; }
        if (lostSince < 0 || now < lostSince) lostSince = now;
        return false;
    }
    bool needsReturnToView(double now) const {
        return !ended && lostSince >= 0 && std::isfinite(now) && now - lostSince >= 3.0;
    }
    // Rigid-transform axes drift slightly from unit length in Float32 ICP.
    // Normalize before acos and clamp roundoff, while rejecting malformed axes.
    static float axisAngle(float dot, float lengthProduct) {
        if (!std::isfinite(dot) || !std::isfinite(lengthProduct) ||
            std::abs(lengthProduct - 1.0f) > 0.001f)
            return std::numeric_limits<float>::quiet_NaN();
        const float cosine = dot / lengthProduct;
        if (std::abs(cosine) > 1.0001f) return std::numeric_limits<float>::quiet_NaN();
        return std::acos(std::max(-1.0f, std::min(1.0f, cosine)));
    }
    void reset() { lostSince = -1; ended = false; }
};
