/*
    Copyright (c) 2020-2022 Intel Corporation

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

#ifndef __TBB_test_common_utils_concurrency_limit_H
#define __TBB_test_common_utils_concurrency_limit_H

#include "config.h"
#include "utils_assert.h"
#include "utils_report.h"
#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/task_scheduler_observer.h"
#include "oneapi/tbb/enumerable_thread_specific.h"

#include <cstddef>
#include <vector>
#include <algorithm>

#if _WIN32 || _WIN64
#include <windows.h>
#elif __unix__
#include <unistd.h>
#if __linux__
#include <sys/sysinfo.h>
#endif
#include <string.h>
#include <sched.h>
#if __FreeBSD__
#include <errno.h>
#include <sys/param.h>
#include <sys/cpuset.h>
#endif
#endif
#include <thread>

namespace utils {
using thread_num_type = std::size_t;

inline thread_num_type get_platform_max_threads() {
    static thread_num_type platform_max_threads = tbb::this_task_arena::max_concurrency();
    return platform_max_threads;
}

inline std::vector<thread_num_type> concurrency_range(thread_num_type max_threads) {
    std::vector<thread_num_type> threads_range;
    thread_num_type step = 1;
    for(thread_num_type thread_num = 1; thread_num <= max_threads; thread_num += step++)
        threads_range.push_back(thread_num);
    if(threads_range.back() != max_threads)
        threads_range.push_back(max_threads);
    // rotate in order to make threads_range non-monotonic
    std::rotate(threads_range.begin(), threads_range.begin() + threads_range.size()/2, threads_range.end());
    return threads_range;
}

inline std::vector<thread_num_type> concurrency_range() {
    static std::vector<thread_num_type> threads_range = concurrency_range(get_platform_max_threads());
    return threads_range;
}

#if !__TBB_TEST_SKIP_AFFINITY

static int maxProcs = 0;

static int get_max_procs() {
    if (!maxProcs) {
#if _WIN32||_WIN64
        DWORD_PTR pam, sam, m = 1;
        GetProcessAffinityMask( GetCurrentProcess(), &pam, &sam );
        int nproc = 0;
        for ( std::size_t i = 0; i < sizeof(DWORD_PTR) * CHAR_BIT; ++i, m <<= 1 ) {
            if ( pam & m )
                ++nproc;
        }
        maxProcs = nproc;
#elif __linux__
        cpu_set_t mask;
        int result = 0;
        sched_getaffinity(0, sizeof(cpu_set_t), &mask);
        int nproc = sysconf(_SC_NPROCESSORS_ONLN);
        for (int i = 0; i < nproc; ++i) {
            if (CPU_ISSET(i, &mask)) ++result;
        }
        maxProcs = result;
#else // FreeBSD
        maxProcs = sysconf(_SC_NPROCESSORS_ONLN);
#endif
    }
    return maxProcs;
}

int get_start_affinity_process() {
#if __linux__
    cpu_set_t mask;
    sched_getaffinity(0, sizeof(cpu_set_t), &mask);

    int result = -1;

    int nproc = sysconf(_SC_NPROCESSORS_ONLN);
    for (int i = 0; i < nproc; ++i) {
        if (CPU_ISSET(i, &mask)) {
            result = i;
            break;
        }
    }
    ASSERT(result != -1, nullptr);
    return result;
#else
    // TODO: add affinity support for Windows and FreeBSD
    return 0;
#endif
}

int limit_number_of_threads( int max_threads ) {
    ASSERT(max_threads >= 1,"The limited number of threads should be positive");
    maxProcs = get_max_procs();
    if (maxProcs < max_threads) {
        // Suppose that process mask is not set so the number of available threads equals maxProcs
        return maxProcs;
    }
#if _WIN32 || _WIN64
    ASSERT(max_threads <= 64, "LimitNumberOfThreads doesn't support max_threads to be more than 64 on Windows");
    DWORD_PTR mask = 1;
    for (int i = 1; i < max_threads; ++i) {
        mask |= mask << 1;
    }
    bool err = !SetProcessAffinityMask(GetCurrentProcess(), mask);
#else
#if __linux__
    using mask_t = cpu_set_t;
#define setaffinity(mask) sched_setaffinity(getpid(), sizeof(mask_t), &mask)
#else /*__FreeBSD*/
    using mask_t = cpuset_t;
#define setaffinity(mask) cpuset_setaffinity(CPU_LEVEL_WHICH, CPU_WHICH_PID, -1, sizeof(mask_t), &mask)
#endif

    mask_t new_mask;
    CPU_ZERO(&new_mask);

    int mask_size = int(sizeof(mask_t) * CHAR_BIT);
    if ( mask_size < maxProcs ) {
        REPORT("The mask size doesn't seem to be big enough to call setaffinity. The call may return an error.");
    }

    ASSERT(max_threads <= int(sizeof(mask_t) * CHAR_BIT), "The mask size is not enough to set the requested number of threads.");
    int st = get_start_affinity_process();
    for (int i = st; i < st + max_threads; ++i) {
        CPU_SET(i, &new_mask);
    }
    int err = setaffinity(new_mask);
#endif
    ASSERT(!err, "Setting process affinity failed");
    return max_threads;
}

#endif // __TBB_TEST_SKIP_AFFINITY

// TODO: consider using cpuset_setaffinity/sched_getaffinity on FreeBSD to enable the functionality
#define OS_AFFINITY_SYSCALL_PRESENT (__linux__ && !__ANDROID__)

#if OS_AFFINITY_SYSCALL_PRESENT
void get_thread_affinity_mask(std::size_t& ncpus, std::vector<int>& free_indexes) {
    cpu_set_t* mask = nullptr;
    ncpus = sizeof(cpu_set_t) * CHAR_BIT;
    do {
        mask = CPU_ALLOC(ncpus);
        if (!mask) break;
        const size_t size = CPU_ALLOC_SIZE(ncpus);
        CPU_ZERO_S(size, mask);
        const int err = sched_getaffinity(0, size, mask);
        if (!err) break;

        CPU_FREE(mask);
        mask = nullptr;
        if (errno != EINVAL) break;
        ncpus <<= 1;
    } while (ncpus < 16 * 1024 /* some reasonable limit */ );
    ASSERT(mask, "Failed to obtain process affinity mask.");

    const size_t size = CPU_ALLOC_SIZE(ncpus);
    const int num_cpus = CPU_COUNT_S(size, mask);
    for (int i = 0; i < num_cpus; ++i) {
        if (CPU_ISSET_S(i, size, mask)) {
            free_indexes.push_back(i);
        }
    }

    CPU_FREE(mask);
}

void pin_thread_imp(std::size_t ncpus, std::vector<int>& free_indexes, std::atomic<int>& curr_idx) {
    const size_t size = CPU_ALLOC_SIZE(ncpus);

    ASSERT(free_indexes.size() > 0, nullptr);
    int mapped_idx = free_indexes[curr_idx++ % free_indexes.size()];

    cpu_set_t *target_mask = CPU_ALLOC(ncpus);
    ASSERT(target_mask, nullptr);
    CPU_ZERO_S(size, target_mask);
    CPU_SET_S(mapped_idx, size, target_mask);
    const int err = sched_setaffinity(0, size, target_mask);
    ASSERT(err == 0, "Failed to set thread affinity");

    CPU_FREE(target_mask);
}
#endif

class thread_pinner {
public:
    thread_pinner() {
        tbb::detail::suppress_unused_warning(thread_index);
#if OS_AFFINITY_SYSCALL_PRESENT
        get_thread_affinity_mask(ncpus, free_indexes);
#endif
    }

    void pin_thread() {
#if OS_AFFINITY_SYSCALL_PRESENT
        pin_thread_imp(ncpus, free_indexes, thread_index);
#endif
    }

private:
#if OS_AFFINITY_SYSCALL_PRESENT
    std::size_t ncpus;
    std::vector<int> free_indexes{};
#endif
    std::atomic<int> thread_index{};
};

class pinning_observer : public tbb::task_scheduler_observer {
    thread_pinner pinner;
    tbb::enumerable_thread_specific<bool> register_threads;
public:
    pinning_observer(tbb::task_arena& arena) : tbb::task_scheduler_observer(arena), pinner() {
        observe(true);
    }

    void on_scheduler_entry( bool ) override {
        bool& is_pinned = register_threads.local();
        if (is_pinned) return;

        pinner.pin_thread();

        is_pinned = true;
    }

    ~pinning_observer() {
        observe(false);
    }
};

#if __unix__
#include <sched.h>
#endif

bool can_change_thread_priority() {
#if __unix__
    pthread_t this_thread = pthread_self();
    sched_param old_params;
    int old_policy;
    int err = pthread_getschedparam(this_thread, &old_policy, &old_params);
    ASSERT(err == 0, nullptr);

    sched_param params;
    params.sched_priority = sched_get_priority_max(SCHED_FIFO);
    ASSERT(params.sched_priority != -1, nullptr);
    err = pthread_setschedparam(this_thread, SCHED_FIFO, &params);
    if (err == 0) {
        err = pthread_setschedparam(this_thread, old_policy, &old_params);
        ASSERT(err == 0, nullptr);
    }
    return err == 0;
#endif
    return false;
}

void increase_thread_priority() {
#if __unix__
    pthread_t this_thread = pthread_self();
    sched_param params;
    params.sched_priority = sched_get_priority_max(SCHED_FIFO);
    ASSERT(params.sched_priority != -1, nullptr);
    int err = pthread_setschedparam(this_thread, SCHED_FIFO, &params);
    ASSERT(err == 0, "Can not change thread priority.");
#endif
}

void decrease_thread_priority() {
#if __unix__
    pthread_t this_thread = pthread_self();
    sched_param params;
    params.sched_priority = sched_get_priority_min(SCHED_FIFO);
    ASSERT(params.sched_priority != -1, nullptr);
    int err = pthread_setschedparam(this_thread, SCHED_FIFO, &params);
    ASSERT(err == 0, "Can not change thread priority.");
#endif
}

} // namespace utils

#endif // __TBB_test_common_utils_concurrency_limit_H
