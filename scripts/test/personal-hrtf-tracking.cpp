#include "SdaTrackingRecovery.hpp"
#include <cassert>
#include <initializer_list>
#include <limits>
int main() {
    for (int fps : {6, 15, 30, 60}) {
        SdaTrackingRecovery gate;
        assert(!gate.terminal(true, 0));
        // Thirty minutes of rejected frames never clears a measured scan.
        for (int i=0; i<fps*1800; ++i) {
            const double now = 10.0+double(i)/fps;
            assert(!gate.terminal(false, now));
            assert(gate.needsReturnToView(now) == (now >= 13.0));
            assert(gate.needsAlignment(now) == (now >= 10.75));
        }
        assert(!gate.terminal(true, 1810));
        assert(!gate.needsReturnToView(1810));
        assert(!gate.needsAlignment(1810));
        // A brief dropout at each side transition never requests alignment.
        for(int i=0;i<200;i++) {
            const double t=1811+i*0.04;
            assert(!gate.terminal(false,t));
            assert(!gate.needsAlignment(t+0.02));
            assert(!gate.terminal(true,t+0.03));
        }
        assert(!gate.terminal(false, 1820));
        assert(!gate.needsReturnToView(1822.99));
        assert(!gate.terminal(false, 1823));
        assert(gate.needsReturnToView(1823));
        // Recovery can succeed repeatedly without a reset or a new coordinate frame.
        assert(!gate.terminal(true, 2000));
        assert(!gate.needsReturnToView(2000));
        gate.reset();
        assert(!gate.needsReturnToView(5000));
        assert(!gate.needsAlignment(5000));
        assert(!gate.terminal(false, 5000));
        assert(!gate.terminal(false, 10)); // clock rolls back; restart guidance timer.
        assert(!gate.needsReturnToView(12.99));
        assert(gate.needsReturnToView(13));
    }
    for (double bad : {-1.0, std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
        SdaTrackingRecovery gate;
        assert(gate.terminal(false,bad));
        assert(gate.terminal(true,1));
        gate.reset();
        assert(!gate.terminal(true,2));
    }
    using G = SdaTrackingRecovery;
    // Reproduces acos(dot>1) NaN at a near-identity Float32 pose.
    assert(std::isnan(std::acos(1.0000001f)));
    assert(G::axisAngle(1.0000001f,1.0f)==0);
    assert(G::axisAngle(1.00001f,1.00001f)==0);
    assert(std::abs(G::axisAngle(-1.0000001f,1.0f)-3.14159265f)<1e-6f);
    for (int degrees : {1, 10, 45, 90, 135, 179}) {
        float angle=degrees*3.14159265f/180;
        for (float scale : {0.9999f,1.0f,1.0001f})
            assert(std::abs(G::axisAngle(std::cos(angle)*scale,scale)-angle)<1e-5f);
    }
    for(float bad : {0.0f,0.9f,1.1f,std::numeric_limits<float>::infinity()})
        assert(std::isnan(G::axisAngle(1,bad)));
    assert(std::isnan(G::axisAngle(1.1f,1)));
    assert(std::isnan(G::axisAngle(std::numeric_limits<float>::quiet_NaN(),1)));
}
