/*
    Copyright (c) 2020-2025 Intel Corporation
    Copyright (c) 2025 UXL Foundation Contributors

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
#include <memory>
#include <climits>

#if _WIN32 || _WIN64
#include <windows.h>
#elif __unix__
#include <unistd.h>
#if __linux__
#include <sys/sysinfo.h>
#include <mntent.h>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#endif
#include <string.h>
#include <sched.h>
#if __FreeBSD__
#include <errno.h>
#include <sys/param.h>
#include <sys/cpuset.h>
#endif
#endif

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

#if __linux__
using cpu_set_type = cpu_set_t;
#elif __FreeBSD__
using cpu_set_type = cpuset_t;
#endif

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
#elif __linux__ || __FreeBSD__
        int result = 0;
        cpu_set_type mask;
        sched_getaffinity(0, sizeof(cpu_set_type), &mask);
        int nproc = sysconf(_SC_NPROCESSORS_ONLN);
        for (int i = 0; i < nproc; ++i) {
            if (CPU_ISSET(i, &mask)) ++result;
        }
        maxProcs = result;
#else
        maxProcs = sysconf(_SC_NPROCESSORS_ONLN);
#endif
    }
    return maxProcs;
}

#if __TBB_USE_CGROUPS

// Linux control groups support
class cgroup_info {
public:
    // The algorithm deliberately goes over slow but reliable paths to determine possible CPU
    // constraints. This helps to make sure the optimizations in the code are correct.
    static bool is_cpu_constrained(int& constrained_num_cpus) {
        static const int num_cpus = parse_cpu_constraints();
        if (num_cpus == error_value || num_cpus == unlimited_num_cpus)
            return false;

        constrained_num_cpus = num_cpus;
        return true;
    }

private:
    static void close_file(std::FILE *file) { std::fclose(file); };
    using unique_file_t = std::unique_ptr<std::FILE, decltype(&close_file)>;

    static constexpr int unlimited_num_cpus = INT_MAX;
    static constexpr int error_value = 0; // Some impossible value for the number of CPUs

    static int determine_num_cpus(long long cpu_quota, long long cpu_period) {
        if (0 == cpu_period)
            return error_value; // Avoid division by zero, use the default number of CPUs

        const long long num_cpus = (cpu_quota + cpu_period - 1) / cpu_period;
        return num_cpus > 0 ? int(num_cpus) : 1; // Ensure at least one CPU is returned
    }

    static constexpr std::size_t rel_path_size = 256; // Size of the relative path buffer
    struct cgroup_paths {
        char v1_relative_path[rel_path_size] = {0};
        char v2_relative_path[rel_path_size] = {0};
    };

    static const char* look_for_cpu_controller_path(const char* line, const char* last_char) {
        const char* path_start = line;
        while ((path_start = std::strstr(path_start, "cpu"))) {
            // At least ":/" must be at the end of line for a valid cgroups file
            if (line - path_start == 0 || last_char - path_start <= 3)
                break; // Incorrect line in the cgroup file, skip it

            const char prev_char = *(path_start - 1);
            if (prev_char != ':' && prev_char != ',') {
                ++path_start; // Not a valid "cpu" controller, continue searching
                continue;
            }

            const char next_char = *(path_start + 3);
            if (next_char != ':' && next_char != ',') {
                ++path_start; // Not a valid "cpu" controller, continue searching
                continue;
            }

            path_start = std::strchr(path_start + 3, ':') + 1;
            __TBB_ASSERT(path_start <= last_char, "Too long path?");
            break;
        }
        return path_start;
    }

    static void cache_relative_path_for(FILE* cgroup_fd, cgroup_paths& paths_cache) {
        char* relative_path = nullptr;
        char line[rel_path_size] = {0};
        const char* last_char = line + rel_path_size - 1;

        const char* path_start = nullptr;
        while (std::fgets(line, rel_path_size, cgroup_fd)) {
            path_start = nullptr;

            if (std::strncmp(line, "0::", 3) == 0) {
                path_start = line + 3; // cgroup v2 unified path
                relative_path = paths_cache.v2_relative_path;
            } else {
                // cgroups v1 allows comount multiple controllers against the same hierarchy
                path_start = look_for_cpu_controller_path(line, last_char);
                relative_path = paths_cache.v1_relative_path;
            }

            if (path_start)
                break;    // Found "cpu" controller path
        }

        std::strncpy(relative_path, path_start, rel_path_size);
        relative_path[rel_path_size - 1] = '\0'; // Ensure null-termination after copy

        char* new_line = std::strrchr(relative_path, '\n');
        if (new_line)
            *new_line = '\0';   // Ensure no new line at the end of the path is copied
    }

    static bool try_read_cgroup_v1_num_cpus_from(const char* dir, int& num_cpus) {
        char path[PATH_MAX] = {0};
        if (std::snprintf(path, PATH_MAX, "%s/cpu.cfs_quota_us", dir) < 0)
            return false; // Failed to create path

        unique_file_t fd(std::fopen(path, "r"), &close_file);
        if (!fd)
            return false;

        long long cpu_quota = 0;
        if (std::fscanf(fd.get(), "%lld", &cpu_quota) != 1)
            return false;

        if (-1 == cpu_quota) {
            num_cpus = unlimited_num_cpus; // -1 quota means maximum available CPUs
            return true;
        }

        if (std::snprintf(path, PATH_MAX, "%s/cpu.cfs_period_us", dir) < 0)
            return false; // Failed to create path

        fd.reset(std::fopen(path, "r"));
        if (!fd)
            return false;

        long long cpu_period = 0;
        if (std::fscanf(fd.get(), "%lld", &cpu_period) != 1)
            return false;

        num_cpus = determine_num_cpus(cpu_quota, cpu_period);
        return num_cpus != error_value; // Return true if valid number of CPUs was determined
    }

    static bool try_read_cgroup_v2_num_cpus_from(const char* dir, int& num_cpus) {
        char path[PATH_MAX] = {0};
        if (std::snprintf(path, PATH_MAX, "%s/cpu.max", dir) < 0)
            return false;

        unique_file_t fd(std::fopen(path, "r"), &close_file);
        if (!fd)
            return false;

        long long cpu_period = 0;
        char cpu_quota_str[16] = {0};
        if (std::fscanf(fd.get(), "%15s %lld", cpu_quota_str, &cpu_period) != 2)
            return false;

        if (std::strncmp(cpu_quota_str, "max", 3) == 0) {
            num_cpus = unlimited_num_cpus;  // "max" means no CPU constraint
            return true;
        }

        errno = 0; // Reset errno before strtoll
        char* str_end = nullptr;
        long long cpu_quota = std::strtoll(cpu_quota_str, &str_end, /*base*/ 10);
        if (errno == ERANGE || str_end == cpu_quota_str)
            return false;

        num_cpus = determine_num_cpus(cpu_quota, cpu_period);
        return num_cpus != error_value; // Return true if valid number of CPUs was determined
    }

    static int parse_cgroup_entry(const char* mnt_dir, const char* mnt_type, FILE* cgroup_fd,
                                  cgroup_paths& paths_cache)
    {
        int num_cpus = error_value; // Initialize to an impossible value
        char dir[PATH_MAX] = {0};
        if (!std::strncmp(mnt_type, "cgroup2", 7)) { // Found cgroup v2 mount entry
            // At first, try reading CPU quota directly
            if (try_read_cgroup_v2_num_cpus_from(mnt_dir, num_cpus))
                return num_cpus; // Successfully read number of CPUs for cgroup v2

            if (!*paths_cache.v2_relative_path)
                cache_relative_path_for(cgroup_fd, paths_cache);

            // Now try reading including relative path
            if (std::snprintf(dir, PATH_MAX, "%s/%s", mnt_dir, paths_cache.v2_relative_path) >= 0)
                try_read_cgroup_v2_num_cpus_from(dir, num_cpus);
            return num_cpus;
        }

        __TBB_ASSERT(std::strncmp(mnt_type, "cgroup", 6) == 0, "Unexpected cgroup type");

        if (try_read_cgroup_v1_num_cpus_from(mnt_dir, num_cpus))
            return num_cpus; // Successfully read number of CPUs for cgroup v1

        if (!*paths_cache.v1_relative_path)
            cache_relative_path_for(cgroup_fd, paths_cache);

        if (std::snprintf(dir, PATH_MAX, "%s/%s", mnt_dir, paths_cache.v1_relative_path) >= 0)
            try_read_cgroup_v1_num_cpus_from(dir, num_cpus);
        return num_cpus;
    }

    static int parse_cpu_constraints() {
        // Reading /proc/self/mounts and /proc/self/cgroup anyway, so open them right away
        unique_file_t cgroup_file_ptr(std::fopen("/proc/self/cgroup", "r"), &close_file);
        if (!cgroup_file_ptr)
            return error_value; // Failed to open cgroup file

        auto close_mounts_file = [](std::FILE* file) { endmntent(file); };
        using unique_mounts_file_t = std::unique_ptr<std::FILE, decltype(close_mounts_file)>;
        unique_mounts_file_t mounts_file_ptr(setmntent("/proc/self/mounts", "r"), close_mounts_file);
        if (!mounts_file_ptr)
            return error_value;

        cgroup_paths relative_paths_cache;
        struct mntent mntent;
        constexpr std::size_t buffer_size = 4096; // Allocate a buffer for reading mount entries
        char mount_entry_buffer[buffer_size];

        int found_num_cpus = error_value; // Initialize to an impossible value
        // Read the mounts file and cgroup file to determine the number of CPUs
        while (getmntent_r(mounts_file_ptr.get(), &mntent, mount_entry_buffer, buffer_size)) {
            if (std::strncmp(mntent.mnt_type, "cgroup", 6) == 0) {
                found_num_cpus = parse_cgroup_entry(mntent.mnt_dir, mntent.mnt_type,
                                                    cgroup_file_ptr.get(), relative_paths_cache);
                if (found_num_cpus != error_value)
                    break;
            }
        }
        return found_num_cpus;
    }
};
#endif                          // __TBB_USE_CGROUPS

std::vector<int> get_cpuset_indices() {
    std::vector<int> result;
#if __linux__ || __FreeBSD__
    cpu_set_type mask;
    sched_getaffinity(0, sizeof(cpu_set_type), &mask);
    int nproc = sysconf(_SC_NPROCESSORS_ONLN);
    for (int i = 0; i < nproc; ++i) {
        if (CPU_ISSET(i, &mask)) {
            result.push_back(i);
        }
    }
    ASSERT(!result.empty(), nullptr);
#else
    // TODO: add affinity support for Windows
#endif
    return result;
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
    std::vector<int> cpuset_indices = get_cpuset_indices();
    ASSERT(!cpuset_indices.empty(), "Empty cpuset returned.");

    for (int i = 0; i < max_threads; ++i) {
        CPU_SET(cpuset_indices[i], &new_mask);
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

#if __unix__
class increased_priority_guard {
public:
    increased_priority_guard() : m_backup(get_current_schedparam()) {
        increase_thread_priority();
    }

    ~increased_priority_guard() {
        // restore priority on destruction
        pthread_t this_thread = pthread_self();
        int err = pthread_setschedparam(this_thread, 
            /*policy*/ m_backup.first, /*sched_param*/ &m_backup.second);
        ASSERT(err == 0, nullptr);
    }
private:
    std::pair<int, sched_param> get_current_schedparam() {
        pthread_t this_thread = pthread_self();
        sched_param params;
        int policy = 0;
        int err = pthread_getschedparam(this_thread, &policy, &params);
        ASSERT(err == 0, nullptr);
        return std::make_pair(policy, params);
    }

    void increase_thread_priority() {
        pthread_t this_thread = pthread_self();
        sched_param params;
        params.sched_priority = sched_get_priority_max(SCHED_FIFO);
        ASSERT(params.sched_priority != -1, nullptr);
        int err = pthread_setschedparam(this_thread, SCHED_FIFO, &params);
        ASSERT(err == 0, "Can not change thread priority.");
    }

    std::pair<int, sched_param> m_backup;
};
#else
    class increased_priority_guard{};
#endif

} // namespace utils

#endif // __TBB_test_common_utils_concurrency_limit_H
