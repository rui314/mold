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

#ifndef __TBB_test_common_spin_barrier_H
#define __TBB_test_common_spin_barrier_H

#include "config.h"

// Do not replace this include by doctest.h
// This headers includes inside benchmark.h and used for benchmarking based on Celero
// Using of both DocTest and Celero frameworks caused unexpected compilation errors.
#include "utils_assert.h"
#include "utils_yield.h"

#include "oneapi/tbb/detail/_machine.h"
#include "oneapi/tbb/detail/_utils.h"
#include "oneapi/tbb/tick_count.h"

#include <atomic>
#include <thread>

namespace utils {

//! Spin WHILE predicate returns true
template <typename Predicate>
void SpinWaitWhile(Predicate pred) {
    int count = 0;
    while (pred()) {
        if (count < 100) {
            tbb::detail::machine_pause(10);
            ++count;
        } else if (count < 200) {
            utils::yield();
            ++count;
        } else {
            std::this_thread::sleep_for(std::chrono::microseconds(count/100));
            if (count < 10000) {
                count += 100;
            }
        }
    }
}

//! Spin WHILE the condition is true.
template <typename T, typename C>
void SpinWaitWhileCondition(const std::atomic<T>& location, C comp) {
    SpinWaitWhile([&] { return comp(location.load(std::memory_order_acquire)); });
}

//! Spin WHILE the value of the variable is equal to a given value
/** T and U should be comparable types. */
template <typename T, typename U>
void SpinWaitWhileEq(const std::atomic<T>& location, const U value) {
    SpinWaitWhileCondition(location, [&value](T t) { return t == value; });
}

//! Spin UNTIL the value of the variable is equal to a given value
/** T and U should be comparable types. */
template<typename T, typename U>
void SpinWaitUntilEq(const std::atomic<T>& location, const U value) {
    SpinWaitWhileCondition(location, [&value](T t) { return t != value; });
}

class WaitWhileEq {
public:
    //! Assignment not allowed
    void operator=( const WaitWhileEq& ) = delete;

    template<typename T, typename U>
    void operator()( const std::atomic<T>& location, U value ) const {
        SpinWaitWhileEq(location, value);
    }
};

class SpinBarrier {
public:
    using size_type = std::size_t;
private:
    size_type myNumThreads;
    std::atomic<size_type> myNumThreadsFinished; // reached the barrier in this epoch
    // the number of times the barrier was opened
    std::atomic<size_type> myEpoch;
    std::atomic<size_type> myLifeTimeGuard;
    // a throwaway barrier can be used only once, then wait() becomes a no-op
    bool myThrowaway;

    struct DummyCallback {
        void operator() () const {}
        template<typename T, typename U>
        void operator()( const T&, U) const {}
    };

public:
    SpinBarrier( const SpinBarrier& ) = delete;    // no copy ctor
    SpinBarrier& operator=( const SpinBarrier& ) = delete; // no assignment

    ~SpinBarrier() {
        while (myLifeTimeGuard.load(std::memory_order_acquire)) {}
    }

    SpinBarrier( size_type nthreads = 0, bool throwaway = false ) {
        initialize(nthreads, throwaway);
    }

    void initialize( size_type nthreads, bool throwaway = false ) {
        myNumThreads = nthreads;
        myNumThreadsFinished = 0;
        myEpoch = 0;
        myThrowaway = throwaway;
        myLifeTimeGuard = 0;
    }

    // Returns whether this thread was the last to reach the barrier.
    // onWaitCallback is called by a thread for waiting;
    // onOpenBarrierCallback is called by the last thread before unblocking other threads.
    template<typename WaitEq, typename Callback>
    bool customWait( const WaitEq& onWaitCallback, const Callback& onOpenBarrierCallback ) {
        if (myThrowaway && myEpoch) {
            return false;
        }

        size_type epoch = myEpoch.load(std::memory_order_relaxed);
        int threadsLeft = static_cast<int>(myNumThreads - myNumThreadsFinished.fetch_add(1, std::memory_order_release) - 1);
        ASSERT(threadsLeft >= 0,"Broken barrier");
        if (threadsLeft > 0) {
            /* this thread is not the last; wait until the epoch changes & return false */
            onWaitCallback(myEpoch, epoch); // acquire myEpoch
            myLifeTimeGuard.fetch_sub(1, std::memory_order_release);
            return false;
        }
        /* reset the barrier, increment the epoch, and return true */
        threadsLeft = static_cast<int>(myNumThreadsFinished.fetch_sub(myNumThreads, std::memory_order_acquire) - myNumThreads);
        ASSERT(threadsLeft == 0,"Broken barrier");
        /* This thread is the last one at the barrier in this epoch */
        onOpenBarrierCallback();
        /* wakes up threads waiting to exit in this epoch */
        myLifeTimeGuard.fetch_add(myNumThreads - 1, std::memory_order_relaxed);
        epoch -= myEpoch.fetch_add(1, std::memory_order_release);
        ASSERT(epoch == 0,"Broken barrier");
        return true;
    }

    // onOpenBarrierCallback is called by the last thread before unblocking other threads.
    template<typename Callback>
    bool wait( const Callback& onOpenBarrierCallback ) {
        return customWait(WaitWhileEq(), onOpenBarrierCallback);
    }

    bool wait() {
        return wait(DummyCallback());
    }

    // Signal to the barrier, rather a semaphore functionality.
    bool signalNoWait() {
        return customWait(DummyCallback(), DummyCallback());
    }
};

} // utils

#endif // __TBB_test_common_spin_barrier_H
