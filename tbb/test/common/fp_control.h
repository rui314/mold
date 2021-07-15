/*
    Copyright (c) 2005-2021 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#ifndef __TBB_test_common_fp_control_H_
#define __TBB_test_common_fp_control_H_

#include "common/test.h"
#include "oneapi/tbb/detail/_machine.h"

#if ( __TBB_x86_32 || __TBB_x86_64 )

const int 
#if _WIN32 || _WIN64
            FE_TONEAREST = _RC_NEAR,
            FE_DOWNWARD = _RC_DOWN,
            FE_UPWARD = _RC_UP,
            FE_TOWARDZERO = _RC_CHOP,
            SSE_SHIFT = 5,
#else
            FE_TONEAREST = 0x0000,
            FE_DOWNWARD = 0x0400,
            FE_UPWARD = 0x0800,
            FE_TOWARDZERO = 0x0c00,
            SSE_SHIFT = 3,
#endif
            FE_RND_MODE_MASK = FE_TOWARDZERO,
            SSE_RND_MODE_MASK = FE_RND_MODE_MASK << SSE_SHIFT,
            SSE_DAZ = 0x0040,
            SSE_FTZ = 0x8000,
            SSE_MODE_MASK = SSE_DAZ | SSE_FTZ,
            SSE_STATUS_MASK = 0x3F;

const int NumSseModes = 4;
const int SseModes[NumSseModes] = { 0, SSE_DAZ, SSE_FTZ, SSE_DAZ | SSE_FTZ };

inline int GetRoundingMode ( bool checkConsistency = true ) {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.get_env();
    if (checkConsistency) {
        auto sse_rnd_mode = (ctl.mxcsr & SSE_RND_MODE_MASK) >> SSE_SHIFT;
        auto x87_rnd_mode = (ctl.x87cw & FE_RND_MODE_MASK);
        CHECK(sse_rnd_mode == x87_rnd_mode);
    }
    return ctl.x87cw & FE_RND_MODE_MASK;
}

inline void SetRoundingMode ( int mode ) {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.get_env();
    ctl.mxcsr = (ctl.mxcsr & ~SSE_RND_MODE_MASK) | (mode & FE_RND_MODE_MASK) << SSE_SHIFT;
    ctl.x87cw = ((ctl.x87cw & ~FE_RND_MODE_MASK) | (mode & FE_RND_MODE_MASK));

    if (true) {
        auto sse_rnd_mode = (ctl.mxcsr & SSE_RND_MODE_MASK) >> SSE_SHIFT;
        auto x87_rnd_mode = (ctl.x87cw & FE_RND_MODE_MASK);
        CHECK(sse_rnd_mode == x87_rnd_mode);
    }

    ctl.set_env();
}

inline int GetSseMode () {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.get_env();
    return ctl.mxcsr & SSE_MODE_MASK;
}

inline void SetSseMode ( int mode ) {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.get_env();
    ctl.mxcsr = (ctl.mxcsr & ~SSE_MODE_MASK) | (mode & SSE_MODE_MASK);
    ctl.set_env();
}

#elif defined(_M_ARM)
const int NumSseModes = 1;
const int SseModes[NumSseModes] = { 0 };

inline int GetSseMode () { return 0; }
inline void SetSseMode ( int ) {}

const int FE_TONEAREST = _RC_NEAR,
          FE_DOWNWARD = _RC_DOWN,
          FE_UPWARD = _RC_UP,
          FE_TOWARDZERO = _RC_CHOP;

inline int GetRoundingMode ( bool = true ) {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.get_env();
    return ctl.my_ctl;
}
inline void SetRoundingMode ( int mode ) {
    tbb::detail::d1::cpu_ctl_env ctl;
    ctl.my_ctl = mode;
    ctl.set_env();
}

#else /* Other archs */

#include <fenv.h>

const int RND_MODE_MASK = FE_TONEAREST | FE_DOWNWARD | FE_UPWARD | FE_TOWARDZERO;

const int NumSseModes = 1;
const int SseModes[NumSseModes] = { 0 };

inline int GetRoundingMode ( bool = true ) { return fegetround(); }
inline void SetRoundingMode ( int rnd ) { fesetround(rnd); }

inline int GetSseMode () { return 0; }
inline void SetSseMode ( int ) {}

#endif /* Other archs */

const int NumRoundingModes = 4;
const int RoundingModes[NumRoundingModes] = { FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO };
const int numFPModes = NumRoundingModes*NumSseModes;

inline void SetFPMode( int mode ) {
    SetRoundingMode( RoundingModes[mode/NumSseModes%NumRoundingModes] );
    SetSseMode( SseModes[mode%NumSseModes] );
}

#define AssertFPMode( mode ) { \
    CHECK_MESSAGE( GetRoundingMode() == RoundingModes[mode/NumSseModes%NumRoundingModes], "FPU control state has not been set correctly." ); \
    CHECK_MESSAGE( GetSseMode() == SseModes[mode%NumSseModes], "SSE control state has not been set correctly." ); \
}

inline int SetNextFPMode( int mode, int step = 1 ) {
    const int nextMode = (mode+step)%numFPModes;
    SetFPMode( nextMode );
    return nextMode;
}

class FPModeContext {
    int origSse, origRounding;
    int currentMode;
public:
    FPModeContext(int newMode) {
        origSse = GetSseMode();
        origRounding = GetRoundingMode();
        SetFPMode(currentMode = newMode);
    }
    ~FPModeContext() {
        assertFPMode();
        SetRoundingMode(origRounding);
        SetSseMode(origSse);
    }
    int setNextFPMode() {
        assertFPMode();
        return currentMode = SetNextFPMode(currentMode);
    }
    void assertFPMode() {
        AssertFPMode(currentMode);
    }
};
#endif // __TBB_test_common_memory_usagae_H_
