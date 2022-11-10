/*
    Copyright (c) 2005-2022 Intel Corporation

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

#ifndef __TBB_test_common_concurrency_tracker_H
#define __TBB_test_common_concurrency_tracker_H

#include "common/test.h"
#include "utils.h"
#include "spin_barrier.h"
#include "oneapi/tbb/parallel_for.h"

#include <mutex>

namespace utils {

static std::atomic<unsigned>        ctInstantParallelism;
static std::atomic<unsigned>        ctPeakParallelism;
static thread_local std::uintptr_t  ctNested;

class ConcurrencyTracker {
    bool    m_Outer;

    static void Started () {
        unsigned p = ++ctInstantParallelism;
        unsigned q = ctPeakParallelism;
        while( q<p ) {
            ctPeakParallelism.compare_exchange_strong(q, p);
        }
    }

    static void Stopped () {
        //CHECK_MESSAGE ( ctInstantParallelism > 0, "Mismatched call to ConcurrencyTracker::Stopped()" );
        --ctInstantParallelism;
    }
public:
    ConcurrencyTracker() : m_Outer(false) {
        std::uintptr_t nested = ctNested;
        CHECK_FAST(nested <= 1);
        if ( !ctNested ) {
            Started();
            m_Outer = true;
            ctNested = 1;
        }
    }
    ~ConcurrencyTracker() {
        if ( m_Outer ) {
            Stopped();
#if __GNUC__
            // Some GCC versions tries to optimize out this write operation. So we need to perform this cast.
            static_cast<volatile std::uintptr_t&>(ctNested) = 0;
#else
            ctNested = 0;
#endif
        }
    }

    static unsigned PeakParallelism() { return ctPeakParallelism; }
    static unsigned InstantParallelism() { return ctInstantParallelism; }

    static void Reset() {
        CHECK_MESSAGE(ctInstantParallelism == 0, "Reset cannot be called when concurrency tracking is underway");
        ctInstantParallelism = ctPeakParallelism = 0;
    }
}; // ConcurrencyTracker


struct ExactConcurrencyLevel : NoCopy {
private:
    SpinBarrier                 *myBarrier;

    // count unique worker threads
    mutable std::atomic<int>    myUniqueThreadsCnt;
    static thread_local int     myUniqueThreads;
    static std::atomic<int>     myEpoch;

    mutable std::atomic<size_t> myActiveBodyCnt;
    // output parameter for parallel_for body to report that max is reached
    mutable std::atomic<bool>   myReachedMax;
    // zero timeout means no barrier is used during concurrency level detection
    const double                myTimeout;
    const size_t                myConcLevel;

    static std::mutex global_mutex;

    ExactConcurrencyLevel(double timeout, size_t concLevel) :
        myBarrier(nullptr),
        myUniqueThreadsCnt(0), myReachedMax(false),
        myTimeout(timeout), myConcLevel(concLevel) {
        myActiveBodyCnt = 0;
    }
    bool run() {
        const int LOOP_ITERS = 100;
        SpinBarrier barrier((unsigned)myConcLevel, /*throwaway=*/true);
        if (myTimeout != 0.)
            myBarrier = &barrier;
        tbb::parallel_for((size_t)0, myConcLevel*LOOP_ITERS, *this, tbb::simple_partitioner());
        return myReachedMax;
    }
public:
    void operator()(size_t) const {
        size_t v = ++myActiveBodyCnt;
        CHECK_MESSAGE(v <= myConcLevel, "Number of active bodies is too high.");
        if (v == myConcLevel) // record that the max expected concurrency was observed
            myReachedMax = true;
        // try to get barrier when 1st time in the thread
        if (myBarrier) {
            myBarrier->wait();
        }

        if (myUniqueThreads != myEpoch) {
            ++myUniqueThreadsCnt;
            myUniqueThreads = myEpoch;
        }
        for (int i=0; i<100; i++)
            tbb::detail::machine_pause(1);
        --myActiveBodyCnt;
    }

    enum Mode {
        None,
        // When multiple blocking checks are performed, there might be not enough
        // concurrency for all of them. Serialize check() calls.
        Serialize
    };

    // check that we have never got more than concLevel threads,
    // and that in some moment we saw exactly concLevel threads
    static void check(size_t concLevel, Mode m = None) {
        ExactConcurrencyLevel o(30., concLevel);

        bool ok = false;
        if (m == Serialize) {
            std::lock_guard<std::mutex> lock(global_mutex);
            ok = o.run();
        } else {
            ok = o.run();
        }
        CHECK(ok);
    }

    static bool isEqual(size_t concLevel) {
        ExactConcurrencyLevel o(3., concLevel);
        return o.run();
    }

    static void checkLessOrEqual(size_t concLevel) {
        ++ExactConcurrencyLevel::myEpoch;
        ExactConcurrencyLevel o(0., concLevel);

        o.run(); // ignore result, as without a barrier it is not reliable
        CHECK_MESSAGE(o.myUniqueThreadsCnt<=concLevel, "Too many workers observed.");
    }
};

std::mutex ExactConcurrencyLevel::global_mutex;
thread_local int ExactConcurrencyLevel::myUniqueThreads;
std::atomic<int> ExactConcurrencyLevel::myEpoch;

} // namespace Harness

#endif /* __TBB_test_common_concurrency_tracker_H */
