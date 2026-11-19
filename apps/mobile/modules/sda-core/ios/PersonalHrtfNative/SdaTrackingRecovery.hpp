#pragma once
#include <cmath>

// Model-queue only. A rejected ICP frame never becomes measured geometry.
// Allow time to return to the last aligned view, independent of camera FPS.
class SdaTrackingRecovery {
    double lostSince = -1;
    bool ended = false;
public:
    bool terminal(bool merged, double now) {
        if (ended) return true;
        if (!std::isfinite(now) || now < 0) return ended = true;
        if (merged) { lostSince = -1; return false; }
        if (lostSince < 0 || now < lostSince) lostSince = now;
        if (now - lostSince >= 3.0) ended = true;
        return ended;
    }
    void reset() { lostSince = -1; ended = false; }
};
