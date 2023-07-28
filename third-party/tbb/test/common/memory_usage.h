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

// Declarations for simple estimate of the memory being used by a program.
// Not yet implemented for macOS*.
// This header is an optional part of the test harness.
// It assumes that "harness_assert.h" has already been included.

#ifndef __TBB_test_common_memory_usage_H_
#define __TBB_test_common_memory_usage_H_

#include "common/test.h"
#include "utils.h"
#include "utils_assert.h"

#if __unix__ || __sun
#include <sys/resource.h>
#include <unistd.h>
#include <sys/utsname.h> /* for uname */
#include <errno.h>       /* for use in LinuxKernelVersion() */

// Parse file utility for THP info
#include "src/tbbmalloc/shared_utils.h"

#elif __APPLE__ && !__ARM_ARCH
#include <unistd.h>
#include <mach/mach.h>
#include <AvailabilityMacros.h>
#if MAC_OS_X_VERSION_MIN_REQUIRED >= __MAC_10_6 || __IPHONE_OS_VERSION_MIN_REQUIRED >= __IPHONE_8_0
#include <mach/shared_region.h>
#else
#include <mach/shared_memory_server.h>
#endif
#if SHARED_TEXT_REGION_SIZE || SHARED_DATA_REGION_SIZE
const size_t shared_size = SHARED_TEXT_REGION_SIZE+SHARED_DATA_REGION_SIZE;
#else
const size_t shared_size = 0;
#endif

#elif _WIN32 && !__TBB_WIN8UI_SUPPORT
#include <windows.h>
#include <psapi.h>
#if _MSC_VER
#pragma comment(lib, "psapi")
#endif

#endif /* OS selection */

namespace utils {

    enum MemoryStatType {
        currentUsage,
        peakUsage
    };

#if __unix__
    inline unsigned LinuxKernelVersion()
    {
        unsigned digit1, digit2, digit3;
        struct utsname utsnameBuf;

        if (-1 == uname(&utsnameBuf)) {
            CHECK_MESSAGE(false, "Can't call uname: errno = " << errno);
        }
        if (3 != sscanf(utsnameBuf.release, "%u.%u.%u", &digit1, &digit2, &digit3)) {
            CHECK_MESSAGE(false, "Unable to parse OS release: " << utsnameBuf.release);
        }
        return 1000000 * digit1 + 1000 * digit2 + digit3;
    }
#endif

    //! Return estimate of number of bytes of memory that this program is currently using.
    /* Returns 0 if not implemented on platform. */
    std::size_t GetMemoryUsage(MemoryStatType stat = currentUsage) {
        utils::suppress_unused_warning(stat);
#if __TBB_WIN8UI_SUPPORT || defined(WINAPI_FAMILY)
        return 0;
#elif _WIN32
        PROCESS_MEMORY_COUNTERS mem;
        bool status = GetProcessMemoryInfo(GetCurrentProcess(), &mem, sizeof(mem)) != 0;
        ASSERT(status, nullptr);
        return stat == currentUsage ? mem.PagefileUsage : mem.PeakPagefileUsage;
#elif __unix__
        long unsigned size = 0;
        FILE* fst = fopen("/proc/self/status", "r");
        ASSERT(fst != nullptr, nullptr);
        const int BUF_SZ = 200;
        char buf_stat[BUF_SZ];
        const char* pattern = stat == peakUsage ? "VmPeak: %lu" : "VmSize: %lu";
        while (nullptr != fgets(buf_stat, BUF_SZ, fst)) {
            if (1 == sscanf(buf_stat, pattern, &size)) {
                ASSERT(size, "Invalid value of memory consumption.");
                break;
            }
        }
        // VmPeak is available in kernels staring 2.6.15
        if (stat != peakUsage || LinuxKernelVersion() >= 2006015)
            ASSERT(size, "Invalid /proc/self/status format, pattern not found.");
        fclose(fst);
        return size * 1024;
#elif __APPLE__ && !__ARM_ARCH
        // TODO: find how detect peak virtual memory size under macOS
        if (stat == peakUsage)
            return 0;
        kern_return_t status;
        task_basic_info info;
        mach_msg_type_number_t msg_type = TASK_BASIC_INFO_COUNT;
        status = task_info(mach_task_self(), TASK_BASIC_INFO, reinterpret_cast<task_info_t>(&info), &msg_type);
        ASSERT(status == KERN_SUCCESS, nullptr);
        return info.virtual_size - shared_size;
#else
        return 0;
#endif
    }

    //! Use approximately a specified amount of stack space.
    /** Recursion is used here instead of alloca because some implementations of alloca do not use the stack. */
    void UseStackSpace(size_t amount, char* top = nullptr) {
        char x[1000];
        memset(x, -1, sizeof(x));
        if (!top)
            top = x;
        CHECK_MESSAGE(x <= top, "test assumes that stacks grow downwards");
        if (size_t(top - x) < amount)
            UseStackSpace(amount, top);
    }

#if __unix__

    inline bool isTHPEnabledOnMachine() {
        long long thpPresent = 'n';
        long long hugePageSize = -1;

        parseFileItem thpItem[] = { { "[alwa%cs] madvise never\n", thpPresent } };
        parseFileItem hpSizeItem[] = { { "Hugepagesize: %lld kB", hugePageSize } };

        parseFile</*BUFF_SIZE=*/100>("/sys/kernel/mm/transparent_hugepage/enabled", thpItem);
        parseFile</*BUFF_SIZE=*/100>("/proc/meminfo", hpSizeItem);

        if (hugePageSize > -1 && thpPresent == 'y') {
            return true;
        } else {
            return false;
        }
    }
    inline long long getSystemTHPAllocatedSize() {
        long long anonHugePagesSize = 0;
        parseFileItem meminfoItems[] = {
            { "AnonHugePages: %lld kB", anonHugePagesSize } };
        parseFile</*BUFF_SIZE=*/100>("/proc/meminfo", meminfoItems);
        return anonHugePagesSize;
    }
    inline long long getSystemTHPCount() {
        long long anonHugePages = 0;
        parseFileItem vmstatItems[] = {
            { "nr_anon_transparent_hugepages %lld", anonHugePages } };
        parseFile</*BUFF_SIZE=*/100>("/proc/vmstat", vmstatItems);
        return anonHugePages;
    }

#endif // __unix__

} // namespace utils
#endif // __TBB_test_common_memory_usage_H_
