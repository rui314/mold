/*
    Copyright (c) 2005-2023 Intel Corporation

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

#ifndef __TBB_malloc_Synchronize_H_
#define __TBB_malloc_Synchronize_H_

#include "oneapi/tbb/detail/_utils.h"

#include <atomic>

//! Stripped down version of spin_mutex.
/** Instances of MallocMutex must be declared in memory that is zero-initialized.
    There are no constructors.  This is a feature that lets it be
    used in situations where the mutex might be used while file-scope constructors
    are running.

    There are no methods "acquire" or "release".  The scoped_lock must be used
    in a strict block-scoped locking pattern.  Omitting these methods permitted
    further simplification. */
class MallocMutex : tbb::detail::no_copy {
    std::atomic_flag m_flag = ATOMIC_FLAG_INIT;

    void lock() {
        tbb::detail::atomic_backoff backoff;
        while (m_flag.test_and_set()) backoff.pause();
    }
    bool try_lock() {
        return !m_flag.test_and_set();
    }
    void unlock() {
        m_flag.clear(std::memory_order_release);
    }

public:
    class scoped_lock : tbb::detail::no_copy {
        MallocMutex& m_mutex;
        bool m_taken;

    public:
        scoped_lock(MallocMutex& m) : m_mutex(m), m_taken(true) {
            m.lock();
        }
        scoped_lock(MallocMutex& m, bool block, bool *locked) : m_mutex(m), m_taken(false) {
            if (block) {
                m.lock();
                m_taken = true;
            } else {
                m_taken = m.try_lock();
            }
            if (locked) *locked = m_taken;
        }

        scoped_lock(scoped_lock& other) = delete;
        scoped_lock& operator=(scoped_lock&) = delete;

        ~scoped_lock() {
            if (m_taken) {
                m_mutex.unlock();
            }
        }
    };
    friend class scoped_lock;
};

inline void SpinWaitWhileEq(const std::atomic<intptr_t>& location, const intptr_t value) {
    tbb::detail::spin_wait_while_eq(location, value);
}

#if USE_PTHREAD && __TBB_SOURCE_DIRECTLY_INCLUDED

inline void SpinWaitUntilEq(const std::atomic<intptr_t>& location, const intptr_t value) {
    tbb::detail::spin_wait_until_eq(location, value);
}

#endif

class AtomicBackoff {
    tbb::detail::atomic_backoff backoff;
public:
    AtomicBackoff() {}
    void pause() { backoff.pause(); }
};

#endif /* __TBB_malloc_Synchronize_H_ */
