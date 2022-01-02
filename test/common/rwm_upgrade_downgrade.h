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

#ifndef __TBB_test_common_rwm_upgrade_downgrade_H
#define __TBB_test_common_rwm_upgrade_downgrade_H

#include "test.h"
#include "utils.h"
#include <atomic>

static std::atomic<std::size_t> Count;

// A body object for NativeParallelFor
template <typename RWMutex>
struct Hammer : utils::NoAssign {
    RWMutex& mutex_protecting_count;
    mutable volatile int dummy;

    Hammer( RWMutex& m ) : mutex_protecting_count(m) {}

    void operator()( std::size_t ) const {
        for (int j = 0; j < 10000; ++j) {
            // Acquire for reading
            typename RWMutex::scoped_lock lock(mutex_protecting_count, false);

            std::size_t c = Count;
            utils::doDummyWork(10);
            if (lock.upgrade_to_writer()) {
                REQUIRE_MESSAGE(c == Count, "Another thread modified Count while holding read lock");
            } else {
                c = Count;
            }

            for (int k = 0; k < 10; ++k) {
                ++Count;
            }
            lock.downgrade_to_reader();
            utils::doDummyWork(10);
        }
    }
}; // struct Hammer

template <typename RWMutex>
void test_rwm_upgrade_downgrade() {
    RWMutex rw_mutex;
    for (auto p = utils::MinThread; p <= utils::MaxThread; ++p) {
        Count = 0;
        utils::NativeParallelFor(p, Hammer<RWMutex>(rw_mutex));
    }
}

#endif // __TBB_test_common_rwm_upgrade_downgrade_H
