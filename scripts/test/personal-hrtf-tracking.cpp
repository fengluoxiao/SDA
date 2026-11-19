#include "SdaTrackingRecovery.hpp"
#include <cassert>
#include <initializer_list>
#include <limits>
int main() {
    for (int fps : {6, 15, 30, 60}) {
        SdaTrackingRecovery gate;
        assert(!gate.terminal(true, 0));
        // More than the old 6/8 frame cutoff must be recoverable.
        for (int i=0; i<fps*3; ++i) assert(!gate.terminal(false, 10.0+double(i)/fps));
        assert(!gate.terminal(true, 12.99));
        assert(!gate.terminal(false, 15));
        assert(!gate.terminal(false, 17.99));
        assert(gate.terminal(false, 18));
        assert(gate.terminal(true, 19)); // Terminal stays terminal until a new scan.
        gate.reset();
        assert(!gate.terminal(true, 20));
        assert(!gate.terminal(false, 21));
        assert(!gate.terminal(true, 21.1));
        assert(!gate.terminal(false, 100)); // Success clears the old loss timer.
    }
    SdaTrackingRecovery invalid;
    assert(invalid.terminal(false,std::numeric_limits<double>::quiet_NaN()));
}
